//! One dispatcher, one hardened arm per `archive_format`. Every arm writes
//! only under `into`, which the caller ([`super::stage`]) always resolves
//! through `layout`'s enclosure guards first — nothing here trusts a
//! declared path on its own.

use std::path::Path;

use super::StackExecError;
use crate::system::tar_safe::{extract_tar_gz, SymlinkPolicy, TarSafeError};

/// Backstops against a stack artifact bomb. An order of magnitude above the
/// largest stack artifact that exists today (the 167 MB OnionPress DMG) and
/// still far below a disk — generous headroom, not a tight limit. Kept
/// separate from `plugins::install::zip_extract`'s own ceilings, which are
/// sized for a JavaScript plugin and stay private to that module.
const STACK_MAX_TOTAL_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
const STACK_MAX_ENTRIES: usize = 100_000;

/// Extract `artifact` (whose bytes were produced by the format named by
/// `format`) into `into`.
///
/// - `"zip"` goes through the same hardened path `plugins::install::zip_extract`
///   already enforces for plugin archives, with ceilings sized for a stack.
/// - `"tar.gz"` is written fresh: `tar::Archive::unpack` is never called —
///   it can follow symlinks and crafted paths outside `into`, and a later
///   ruling removed the hardcoded-checksum premise that waiver used to rest
///   on (`build::assets::binary_resolver::extract_tar_gz_to_cache`).
/// - `"dmg"` is macOS-only: attach, copy the sole top-level item out of the
///   mounted volume, detach, and assert no quarantine xattr survived.
/// - `"raw"` is a single file: `artifact` IS the binary, copied to `into`
///   verbatim.
pub fn extract(format: &str, artifact: &Path, into: &Path) -> Result<(), StackExecError> {
    match format {
        "zip" => crate::plugins::install::zip_extract::extract_zip_path_limited(
            artifact,
            into,
            STACK_MAX_ENTRIES,
            STACK_MAX_TOTAL_UNCOMPRESSED,
        )
        .map_err(|e| StackExecError::Extract(e.to_string())),
        // Hardened `.tar.gz` extraction, via the module both this executor
        // and `moss desktop install` share (`crate::system::tar_safe`). A stack
        // artifact never needs a symlink — `SymlinkPolicy::Reject` — and
        // `STACK_MAX_ENTRIES`/`STACK_MAX_TOTAL_UNCOMPRESSED` stay here,
        // since this module remains the single owner of the stack's
        // ceilings for both its zip and tar arms.
        "tar.gz" => {
            let archive_name = artifact.display().to_string();
            let file = std::fs::File::open(artifact)
                .map_err(|e| StackExecError::Extract(format!("open {archive_name}: {e}")))?;
            extract_tar_gz(file, into, SymlinkPolicy::Reject, STACK_MAX_ENTRIES, STACK_MAX_TOTAL_UNCOMPRESSED)
                .map_err(|e| match e {
                    TarSafeError::UnsafeEntry { entry } => {
                        StackExecError::UnsafeEntry { archive: archive_name.clone(), entry }
                    }
                    TarSafeError::Io(msg) => StackExecError::Extract(format!("{archive_name}: {msg}")),
                })
        }
        "dmg" => extract_dmg(artifact, into),
        "raw" => extract_raw(artifact, into),
        other => Err(StackExecError::UnsupportedFormat { format: other.to_string() }),
    }
}

/// A `raw` artifact IS the binary: copy it to `into` verbatim. `into` is
/// always resolved by the caller through `layout::binary_path`/`enclosed`
/// before reaching here — this arm trusts that resolution rather than
/// re-deriving a name from the artifact's own (attacker-influenced) file
/// name.
fn extract_raw(artifact: &Path, into: &Path) -> Result<(), StackExecError> {
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| StackExecError::Extract(format!("mkdir {}: {e}", parent.display())))?;
    }
    // allow:raw_write the raw stack artifact IS the binary; it lands under ~/.moss/stacks/, not build output
    std::fs::copy(artifact, into)
        .map_err(|e| StackExecError::Extract(format!("copy {} to {}: {e}", artifact.display(), into.display())))?;
    Ok(())
}

// ============================================================================
// dmg (macOS only)
// ============================================================================

#[cfg(target_os = "macos")]
fn extract_dmg(dmg: &Path, into: &Path) -> Result<(), StackExecError> {
    use crate::system::bounded_command::{run_bounded, STOP_GRACE};
    use std::time::Duration;

    const DISK_TIMEOUT: Duration = Duration::from_secs(2 * 60);
    const COPY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

    /// Attach guard: `Drop` detaches and MEANS it. "Resource busy" is
    /// ordinary right after a large `cp -R` (Spotlight and Finder both open
    /// the volume behind moss's back), and a silently failed detach pins the
    /// backing file's blocks for the life of the process. Retry, then
    /// force, then say so. Moved from `stack_install.rs`'s `AttachedDmg`,
    /// kept verbatim including the retry-then-force ladder.
    struct AttachedDmg {
        target: std::path::PathBuf,
        mount: Option<std::path::PathBuf>,
    }

    impl AttachedDmg {
        fn attach(dmg: &Path) -> Result<Self, StackExecError> {
            let mut cmd = std::process::Command::new("hdiutil");
            cmd.args(["attach", "-nobrowse", "-noverify", "-noautoopen"]).arg(dmg);
            let out = run_bounded(&mut cmd, DISK_TIMEOUT, "hdiutil attach")
                .map_err(StackExecError::Extract)?;
            if !out.status.success() {
                return Err(StackExecError::Extract(format!("hdiutil attach failed: {}", out.stderr)));
            }
            let target = parse_hdiutil_detach_target(&out.stdout).ok_or_else(|| {
                StackExecError::Extract(format!("hdiutil attach reported no device: {}", out.stdout))
            })?;
            Ok(Self { mount: parse_hdiutil_mount_point(&out.stdout), target })
        }

        fn mount(&self) -> Result<&Path, StackExecError> {
            self.mount.as_deref().ok_or_else(|| {
                StackExecError::Extract(format!(
                    "hdiutil attached {} without mounting a volume — nothing to stage from",
                    self.target.display()
                ))
            })
        }
    }

    impl Drop for AttachedDmg {
        fn drop(&mut self) {
            for (attempt, args) in
                [["detach", "-quiet"], ["detach", "-quiet"], ["detach", "-force"]].iter().enumerate()
            {
                if attempt > 0 {
                    std::thread::sleep(STOP_GRACE);
                }
                let mut cmd = std::process::Command::new("hdiutil");
                cmd.args(args).arg(&self.target);
                match run_bounded(&mut cmd, DISK_TIMEOUT, "hdiutil detach") {
                    Ok(out) if out.status.success() => return,
                    Ok(out) => log::warn!(
                        "[stack-exec] hdiutil {} {} exited {} — {}",
                        args.join(" "),
                        self.target.display(),
                        out.status,
                        out.stderr.lines().last().unwrap_or("")
                    ),
                    Err(e) => log::warn!("[stack-exec] {e}"),
                }
            }
            log::error!(
                "[stack-exec] {} is STILL attached — its disk space stays pinned until moss restarts",
                self.target.display()
            );
        }
    }

    let image = AttachedDmg::attach(dmg)?;
    let mount = image.mount()?;

    // The mounted volume's sole top-level, non-symlink entry is the bundle
    // to copy out — never a hardcoded name: a DMG-format source names no
    // bundle name of its own, so the mount's contents are trusted to
    // contain exactly one real item (the app bundle), plus optionally a
    // drag-install "Applications" alias, which is a symlink and is skipped.
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(mount)
        .map_err(|e| StackExecError::Extract(format!("read {}: {e}", mount.display())))?
    {
        let entry = entry.map_err(|e| StackExecError::Extract(format!("read {}: {e}", mount.display())))?;
        let file_type = entry
            .file_type()
            .map_err(|e| StackExecError::Extract(format!("stat {}: {e}", entry.path().display())))?;
        if file_type.is_symlink() {
            continue;
        }
        candidates.push(entry.path());
    }
    let src = match candidates.as_slice() {
        [only] => only.clone(),
        [] => return Err(StackExecError::Extract(format!("no bundle found in {}", mount.display()))),
        _ => {
            return Err(StackExecError::Extract(format!(
                "{} contains more than one top-level item; the dmg arm cannot tell which is the bundle",
                mount.display()
            )))
        }
    };

    // `into` is `bundle_root`'s scratch sibling, not the bundle itself: `cp
    // -R src into` (the old shape) creates `into` AS a copy of `src`'s
    // CONTENTS when `into` does not exist yet, which drops the bundle's own
    // name and flattens `Contents/`, `MacOS/` etc. straight onto `into`. A
    // declared `executable` like `OnionPress.app/Contents/MacOS/onionpress`
    // then names a path that never exists. Creating `into` as a directory
    // FIRST and copying to `into.join(src.file_name())` keeps the bundle
    // nested under its own name, matching what `layout::binary_path`
    // resolves a declared `executable` against.
    std::fs::create_dir_all(into)
        .map_err(|e| StackExecError::Extract(format!("mkdir {}: {e}", into.display())))?;
    let bundle_name = src.file_name().ok_or_else(|| {
        StackExecError::Extract(format!("{}: mounted item has no file name", src.display()))
    })?;
    let dest = into.join(bundle_name);
    let mut cmd = std::process::Command::new("cp");
    cmd.arg("-R").arg(&src).arg(&dest);
    let cp = run_bounded(&mut cmd, COPY_TIMEOUT, "cp -R").map_err(StackExecError::Extract)?;
    if !cp.status.success() {
        return Err(StackExecError::Extract(format!(
            "cp -R failed: {}",
            cp.stderr.lines().last().unwrap_or("")
        )));
    }

    // Drop `image` (detaching) before asserting no quarantine xattr — the
    // copy is already complete and there is no reason to hold the volume
    // attached any longer than the `cp -R` needed it.
    drop(image);

    assert_no_quarantine(&dest)
}

#[cfg(not(target_os = "macos"))]
fn extract_dmg(_dmg: &Path, _into: &Path) -> Result<(), StackExecError> {
    Err(StackExecError::UnsupportedFormat { format: "dmg (macOS only)".to_string() })
}

#[cfg(target_os = "macos")]
fn assert_no_quarantine(app: &Path) -> Result<(), StackExecError> {
    use crate::system::bounded_command::run_bounded;
    use std::time::Duration;
    const DISK_TIMEOUT: Duration = Duration::from_secs(2 * 60);

    let mut cmd = std::process::Command::new("xattr");
    cmd.args(["-p", "com.apple.quarantine"]).arg(app);
    let out = run_bounded(&mut cmd, DISK_TIMEOUT, "xattr probe").map_err(StackExecError::Extract)?;
    // `xattr -p` exits 0 iff the attribute is present. A programmatic
    // (non-LaunchServices) download never sets it, so absence is asserted.
    if out.status.success() {
        Err(StackExecError::Extract(format!(
            "staged bundle carries com.apple.quarantine (would trip Gatekeeper): {}",
            app.display()
        )))
    } else {
        Ok(())
    }
}

/// Parse the mount point (`/Volumes/…`) out of `hdiutil attach -nobrowse`
/// output. Columns are tab-separated; the mount point is the trailing field
/// of the line whose last field is an absolute `/Volumes` path. Moved from
/// `stack_install.rs`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_hdiutil_mount_point(stdout: &str) -> Option<std::path::PathBuf> {
    for line in stdout.lines() {
        if let Some(last) = line.split('\t').map(str::trim).filter(|s| !s.is_empty()).next_back() {
            if last.starts_with("/Volumes/") {
                return Some(std::path::PathBuf::from(last));
            }
        }
    }
    None
}

/// What `hdiutil detach` can be pointed at, from the same output: the mount
/// point when there is one, else the `/dev/disk` entry — an image that
/// attached without a nameable volume is still attached.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_hdiutil_detach_target(stdout: &str) -> Option<std::path::PathBuf> {
    parse_hdiutil_mount_point(stdout)
        .or_else(|| stdout.split_whitespace().find(|t| t.starts_with("/dev/disk")).map(std::path::PathBuf::from))
}

#[cfg(test)]
#[path = "extract_tests.rs"]
mod extract_tests;
