//! Observes the build root's on-disk identity, for the log only.
//!
//! A cloud sync client can rename the live `.moss/build` aside — to `build 2`,
//! `build 31`, … — and put a fresh directory at the original path, while a
//! build is writing to it and the preview server is serving it. The rename
//! preserves the inode, so the OLD directory keeps existing (just under a new
//! name) while every guard in this crate that holds the root BY PATH silently
//! starts reading or writing the NEW one. Today's logs cannot show that this
//! happened; this module makes it visible.
//!
//! NO behaviour change. Nothing here refuses a build, retries anything, or
//! re-marks a directory — it only observes and logs. A later change can hold
//! the root by a directory handle instead of a path; this one is scoped to
//! giving that change something to point at.
//!
//! The line is logged where a swap changes what a reader should conclude: at
//! build start, after the ship (promoted, superseded or withheld alike), when
//! the preview server adopts a tree, and on a preview 404.

use std::path::Path;

/// A directory's on-disk identity: the device and inode `stat` reports for
/// it, on the platforms where that means something. `None` on a platform
/// where the concept does not apply, or where the path could not be stat'd
/// (does not exist yet, permission denied) — every caller treats absence the
/// same way it treats "nothing to compare against".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RootIdentity {
    dev: u64,
    ino: u64,
}

#[cfg(unix)]
fn stat_identity(path: &Path) -> Option<RootIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some(RootIdentity {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// No inode concept off unix — absence, not a cfg branch every caller repeats.
#[cfg(not(unix))]
fn stat_identity(_path: &Path) -> Option<RootIdentity> {
    None
}

/// One look at the build root: its identity, how many renamed-aside siblings
/// a cloud sync client has left next to it, and whether the cloud-sync
/// exclusion marker is still attached to it.
struct RootObservation {
    identity: Option<RootIdentity>,
    siblings: usize,
    marked: bool,
}

/// Observe `build_root` as it stands right now. Pure I/O, no caching — a
/// rename-aside is exactly the kind of change a cached answer would miss.
fn observe(build_root: &Path) -> RootObservation {
    RootObservation {
        identity: stat_identity(build_root),
        siblings: count_renamed_aside_siblings(build_root),
        marked: crate::moss_paths::has_cloud_sync_marker(build_root),
    }
}

/// Sibling directories a cloud sync client left behind when it renamed the
/// live root aside to make room for a fresh directory at the same path: the
/// root's own name, one space, and one or more decimal digits — `build 2`,
/// `build 31`. A suffix with no separating space (`builder`), a dotted
/// variant (`build.nosync`), a further-renamed copy (`build 2 copy`), and a
/// plain file sharing the shape all do not count.
fn count_renamed_aside_siblings(build_root: &Path) -> usize {
    let (Some(name), Some(parent)) = (
        build_root.file_name().and_then(|n| n.to_str()),
        build_root.parent(),
    ) else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|candidate| is_renamed_aside(candidate, name))
        })
        .count()
}

fn is_renamed_aside(candidate: &str, name: &str) -> bool {
    candidate
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(' '))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Pure formatter for the `build.root` observability line, in the house
/// style of `format_build_summary` (`build/phase.rs`): space-separated
/// `key=value` pairs, one line, no trailing punctuation. `swapped` is
/// included only when `Some` — the start-of-build line has nothing yet to
/// compare against, and a serve-time line has no start identity in scope.
fn format_build_root(phase: &str, observation: &RootObservation, swapped: Option<bool>) -> String {
    let (dev, ino) = match observation.identity {
        Some(id) => (id.dev.to_string(), id.ino.to_string()),
        None => ("?".to_string(), "?".to_string()),
    };
    let mut line = format!(
        "build.root phase={phase} dev={dev} ino={ino} siblings={} marked={}",
        observation.siblings, observation.marked
    );
    if let Some(swapped) = swapped {
        line.push_str(&format!(" swapped={swapped}"));
    }
    line
}

/// Observe `build_root`, log one `build.root` line for `phase`, and hand back
/// the identity observed.
///
/// `since_start` is what this build's `"start"` line returned. `None` means
/// there is nothing to compare against — the start line itself, a serve-time
/// line, or a start that could not be observed — and the line then carries no
/// `swapped` at all, so an unknown start never reads as a swap.
///
/// `swapped=true` logs at WARN; everything else logs at INFO, matching
/// `log_build_summary`'s `target: "build"` convention.
pub(crate) fn log_build_root(
    build_root: &Path,
    phase: &'static str,
    since_start: Option<RootIdentity>,
) -> Option<RootIdentity> {
    let observation = observe(build_root);
    let swapped = since_start.map(|start| Some(start) != observation.identity);
    let line = format_build_root(phase, &observation, swapped);
    if swapped == Some(true) {
        log::warn!(target: "build", "{}", line);
    } else {
        log::info!(target: "build", "{}", line);
    }
    observation.identity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn identity_changes_across_a_rename_aside_and_the_formatter_reports_it() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("build");
        std::fs::create_dir(&build).unwrap();
        let before = observe(&build);
        assert!(
            before.identity.is_some(),
            "unix must resolve an identity for an existing directory"
        );
        assert_eq!(before.siblings, 0);

        // The rename a cloud sync client makes: the old directory keeps its
        // inode under a new name, and a fresh directory appears at the old
        // path.
        std::fs::rename(&build, tmp.path().join("build 2")).unwrap();
        std::fs::create_dir(&build).unwrap();
        let after = observe(&build);

        assert_ne!(
            before.identity, after.identity,
            "a fresh directory at the same path is a different inode"
        );
        assert_eq!(after.siblings, 1, "exactly the one renamed-aside sibling");

        let line = format_build_root("ship", &after, Some(before.identity != after.identity));
        assert!(line.contains("swapped=true"), "{line}");
    }

    #[test]
    fn sibling_counter_counts_renamed_aside_directories_only() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("build");
        std::fs::create_dir(&build).unwrap();
        for name in [
            "build 2",
            "build 31",
            "builder",
            "build.nosync",
            "build 2 copy",
        ] {
            std::fs::create_dir(tmp.path().join(name)).unwrap();
        }
        // A file sharing the renamed-aside shape must not count — only a
        // directory can be what a cloud client renamed the root to.
        std::fs::write(tmp.path().join("build 3"), b"not a directory").unwrap();

        assert_eq!(observe(&build).siblings, 2, "only `build 2` and `build 31`");
    }

    #[test]
    fn formatter_matches_the_house_style_exactly() {
        let observation = RootObservation {
            identity: Some(RootIdentity {
                dev: 16777234,
                ino: 228848875,
            }),
            siblings: 28,
            marked: false,
        };
        assert_eq!(
            format_build_root("ship", &observation, Some(true)),
            "build.root phase=ship dev=16777234 ino=228848875 siblings=28 marked=false swapped=true"
        );
    }

    #[test]
    fn formatter_omits_swapped_and_shows_unknown_identity_when_absent() {
        let observation = RootObservation {
            identity: None,
            siblings: 0,
            marked: true,
        };
        assert_eq!(
            format_build_root("start", &observation, None),
            "build.root phase=start dev=? ino=? siblings=0 marked=true"
        );
    }
}
