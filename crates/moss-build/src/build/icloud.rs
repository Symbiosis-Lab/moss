//! iCloud Drive eviction detection and the "unreadable is not absent" gate.
//!
//! Detects files managed by iCloud Drive's file provider (`fileproviderd`).
//! Used by the scan phase to count managed files for the iCloud progress
//! indicator, and by media metadata extraction to skip reads that would
//! block the time-critical blocking build phase.
//!
//! ## How It Works
//!
//! On macOS (Sonoma+), iCloud Drive sets the `SF_DATALESS` flag (0x40000000)
//! on files managed by the file provider. `is_evicted()` is useful for
//! *counting* such files (scan phase) or *deferring* reads that would block
//! time-critical phases (media metadata during the blocking phase).
//!
//! As of `platform::macos::iopolicy::set_dataless_fail_fast` (called once at
//! startup — see docs/archive/2026-08-03-dataless-fail-fast-and-build-driven-
//! cloud-gate.md), reading a dataless file's content on macOS no longer
//! blocks-then-succeeds: it fails immediately with `EDEADLK`. Use
//! `is_dataless_unavailable()` to recognize that failure and defer instead
//! of hard-failing. **Do NOT use `is_evicted()` as a skip gate for file copy
//! operations** — `fs::copy`/`fs::read` on a dataless file is the correct
//! way to materialize it (or fails fast and should be deferred); silently
//! skipping evicted files causes missing assets in the output.
//!
//! This module uses `symlink_metadata()` (lstat) to detect the flag
//! without triggering downloads.
//!
//! ## The "unreadable is not absent" invariant
//!
//! `is_dataless_unavailable()` and `is_definitely_absent()` exist because
//! moss has several paths that treat a read failure as license to write:
//! regenerating a site's identity key, rewriting `config.toml` migrations,
//! deleting build output for a "removed" page. A dataless file that merely
//! failed to materialize must never look like a missing file to those paths
//! — see design doc §4 for the full destructive-path inventory and the
//! pre-Sonoma (macOS 12–13) `.name.icloud`-stub wrinkle `is_definitely_absent`
//! also guards against.
//!
//! ## Platform
//!
//! macOS: `SF_DATALESS` via `st_flags` (above). Windows: OneDrive/Dropbox
//! Files-On-Demand sets `FILE_ATTRIBUTE_OFFLINE` /
//! `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` on placeholder files, readable via
//! `std::os::windows::fs::MetadataExt::file_attributes()` — no extra crate.
//! Other platforms (Linux sync clients mostly use FUSE with no equivalent
//! stat-time bit): `is_evicted()` always returns false.

use std::path::Path;

/// SF_DATALESS flag value from macOS `<sys/stat.h>`.
/// When set in `stat.st_flags`, the file's data extents are not present
/// on disk (iCloud has evicted them to the cloud).
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x40000000;

/// Check whether a file is evicted (cloud-only) on iCloud Drive.
///
/// Uses `symlink_metadata()` (lstat) which reads inode metadata without
/// triggering a download of the file's data extents.
///
/// Returns `false` for non-existent files, directories, or on non-macOS platforms.
#[cfg(target_os = "macos")]
fn is_evicted_on_disk(path: &Path) -> bool {
    use std::os::darwin::fs::MetadataExt;

    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            // Only check regular files (not directories or symlinks)
            if !meta.is_file() {
                return false;
            }
            (meta.st_flags() & SF_DATALESS) != 0
        }
        Err(_) => false,
    }
}

/// Windows placeholder-file attribute bits (`<winnt.h>`), set by Files-On-
/// Demand sync clients (OneDrive, Dropbox) on cloud-only placeholders.
#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// Check whether a file is a cloud-only placeholder on Windows.
///
/// Uses `symlink_metadata()` (lstat-equivalent) — reads directory-entry
/// attributes without triggering a download.
#[cfg(target_os = "windows")]
fn is_evicted_on_disk(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() {
                return false;
            }
            (meta.file_attributes()
                & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS))
                != 0
        }
        Err(_) => false,
    }
}

/// Non-macOS, non-Windows stub: always returns false.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn is_evicted_on_disk(_path: &Path) -> bool {
    false
}

/// Is `path` a cloud-only placeholder right now? One `lstat`, never a read — the
/// platform check is the three `is_evicted_on_disk` variants above.
pub fn is_evicted(path: &Path) -> bool {
    #[cfg(test)]
    if pretend::marked(path) {
        return true;
    }
    is_evicted_on_disk(path)
}

/// Test seam for everything that branches on a file being in the cloud.
///
/// Only the file provider can set `SF_DATALESS`, and off macOS the check is a
/// compile-time `false`, so no test could put a file in the cloud: the code that
/// must not read one (the hash index's `resolve`, the scan's evicted branch, the
/// CAS heal) had nothing but a real dataless file to be tried against. A path
/// marked here answers `is_evicted` (and so `is_still_in_the_cloud`) as true until
/// the guard drops. Keyed by exact path, so tests on their own temp dirs never see
/// each other's marks.
#[cfg(test)]
pub(crate) mod pretend {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static MARKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(crate) fn marked(path: &Path) -> bool {
        MARKED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == path)
    }

    /// Mark `path` as cloud-only for the life of the returned guard.
    pub(crate) fn evicted(path: &Path) -> Guard {
        MARKED.lock().unwrap_or_else(|e| e.into_inner()).push(path.to_path_buf());
        Guard(path.to_path_buf())
    }

    pub(crate) struct Guard(PathBuf);

    impl Drop for Guard {
        fn drop(&mut self) {
            MARKED.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| *p != self.0);
        }
    }
}

/// Whether a DIRECTORY carries the dataless flag. [`is_evicted`] answers only
/// for regular files; this exists for forensics on a vanished CAS shard, where
/// a dataless shard directory and a deleted one are two different culprits.
#[cfg(target_os = "macos")]
pub fn is_dataless_dir(path: &Path) -> bool {
    use std::os::darwin::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .map(|meta| meta.is_dir() && (meta.st_flags() & SF_DATALESS) != 0)
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn is_dataless_dir(_path: &Path) -> bool {
    false
}

/// True when `err` is the dataless-fail-fast policy refusing to materialize a
/// cloud file — NOT evidence the file is missing or genuinely broken. See the
/// module doc's "unreadable is not absent" section.
///
/// On macOS this is `EDEADLK` (errno **11** — not 35, despite some bug
/// reports; verified against `/usr/include/sys/errno.h` and empirically
/// against a live dataless file). Non-macOS platforms never arm the
/// fail-fast policy (see design doc §9, "Windows fail-fast" non-goal), so
/// this always returns `false` there — those reads still block-then-succeed
/// or fail for unrelated reasons, as before.
pub fn is_dataless_unavailable(err: &std::io::Error) -> bool {
    #[cfg(target_os = "macos")]
    {
        err.raw_os_error() == Some(libc::EDEADLK)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = err;
        false
    }
}

/// True only when `path` is **positively** gone: a `NotFound` error with no
/// cloud placeholder standing in for it. Every destructive path (identity
/// regeneration, config migrations, stale-output deletion — design doc §4)
/// must gate on this, never on `err.kind() == NotFound` alone.
///
/// Why the extra check: on macOS 12–13 (pre-Sonoma), an evicted iCloud file
/// is a hidden `.name.icloud` stub rather than a `SF_DATALESS` file, and the
/// *real* path returns ENOENT — indistinguishable from "gone" by error kind
/// alone. This is a live gap today, independent of the fail-fast policy
/// above; see design doc §4 "The macOS 12/13 wrinkle".
///
/// Cheap: at most one extra `lstat`, only reached on the already-rare
/// `NotFound` path.
pub fn is_definitely_absent(path: &Path, err: &std::io::Error) -> bool {
    if err.kind() != std::io::ErrorKind::NotFound {
        return false;
    }
    !has_icloud_stub_sibling(path)
}

#[cfg(target_os = "macos")]
fn has_icloud_stub_sibling(path: &Path) -> bool {
    let (Some(name), Some(parent)) = (path.file_name().and_then(|n| n.to_str()), path.parent())
    else {
        return false;
    };
    parent.join(format!(".{name}.icloud")).symlink_metadata().is_ok()
}

#[cfg(not(target_os = "macos"))]
fn has_icloud_stub_sibling(_path: &Path) -> bool {
    false
}

/// Is this file still waiting on the cloud?
///
/// Both eviction forms in one predicate, because a caller polling for arrival
/// has to handle both or it will mistake one for a deletion. The pre-Sonoma
/// form is the trap: the real path does not exist, so a plain
/// `is_evicted(path)` says false and `path.exists()` says false, and a poller
/// that checks either alone concludes the user deleted the file.
pub fn is_still_in_the_cloud(path: &Path) -> bool {
    is_evicted(path) || (!path.exists() && has_icloud_stub_sibling(path))
}

/// Did this read fail because the bytes are in the cloud, rather than because
/// there is nothing there?
///
/// The union of the two classifiers above, which is the question every caller
/// that would otherwise *drop* a file actually has: both the fail-fast policy's
/// `EDEADLK` and a pre-Sonoma `ENOENT`-with-a-placeholder mean "come back
/// later," and neither is evidence of absence. Anything else — a real
/// `NotFound`, a permission error, bad I/O — is not this.
pub fn is_offline_not_absent(path: &Path, err: &std::io::Error) -> bool {
    is_dataless_unavailable(err)
        || (err.kind() == std::io::ErrorKind::NotFound && !is_definitely_absent(path, err))
}

/// The inverse of `has_icloud_stub_sibling`: if `path` **is** a pre-Sonoma
/// placeholder (`.name.icloud`), return the real path it stands for.
///
/// Why the scan needs this and a `SF_DATALESS` check is not enough: on macOS
/// 12–13 eviction *replaces* the file with this hidden sibling, so the real
/// name is absent from the directory walk entirely. A page evicted that way
/// does not look offline to moss — it looks **deleted**, which is precisely
/// what makes stale-output cleanup delete its still-good HTML. Recognizing the
/// placeholder is the only way the build can tell "the user removed this page"
/// from "the OS put this page in the cloud."
///
/// Returns `None` on any name that isn't the placeholder form, and always on
/// non-macOS — no other platform uses it.
#[cfg(target_os = "macos")]
pub fn icloud_stub_target(path: &Path) -> Option<std::path::PathBuf> {
    let name = path.file_name()?.to_str()?;
    let real = name.strip_prefix('.')?.strip_suffix(".icloud")?;
    if real.is_empty() {
        return None;
    }
    Some(path.parent()?.join(real))
}

#[cfg(not(target_os = "macos"))]
pub fn icloud_stub_target(_path: &Path) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_evicted_returns_false_for_regular_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        assert!(
            !is_evicted(&file_path),
            "Regular local file should not be evicted"
        );
    }

    #[test]
    fn test_is_evicted_returns_false_for_nonexistent() {
        let path = Path::new("/tmp/this_file_does_not_exist_icloud_test_12345");
        assert!(
            !is_evicted(path),
            "Non-existent file should not be evicted"
        );
    }

    #[test]
    fn test_is_evicted_returns_false_for_directory() {
        let dir = TempDir::new().unwrap();
        assert!(
            !is_evicted(dir.path()),
            "Directory should not be evicted"
        );
    }

    #[test]
    fn not_found_is_never_dataless_unavailable() {
        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(
            !is_dataless_unavailable(&err),
            "ENOENT must never be classified as a dataless stall"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn edeadlk_is_dataless_unavailable() {
        let err = std::io::Error::from_raw_os_error(libc::EDEADLK);
        assert!(is_dataless_unavailable(&err));
        // The neighbouring errno must not be swept in: EAGAIN is 35 on macOS
        // and means something entirely unrelated.
        let eagain = std::io::Error::from_raw_os_error(libc::EAGAIN);
        assert!(!is_dataless_unavailable(&eagain));
    }

    #[test]
    fn truly_missing_file_is_definitely_absent() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("gone.txt");
        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(is_definitely_absent(&missing, &err));
    }

    #[test]
    fn non_not_found_error_is_not_definitely_absent() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("gone.txt");
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(
            !is_definitely_absent(&missing, &err),
            "only a NotFound error can ever mean the file is gone"
        );
    }

    /// Pre-Sonoma iCloud represents an evicted file as a hidden `.name.icloud`
    /// stub, and the real path returns a genuine ENOENT. Treating that as
    /// "absent" is what lets a destructive path delete or regenerate live data.
    #[cfg(target_os = "macos")]
    #[test]
    fn icloud_stub_sibling_blocks_definitely_absent() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("secret-key");
        fs::write(dir.path().join(".secret-key.icloud"), b"stub").unwrap();

        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(
            !is_definitely_absent(&real, &err),
            "an .icloud stub sibling means the file exists but is evicted"
        );
    }

    /// The scan sees the placeholder, never the name it stands for — so this
    /// mapping is the only thing that tells the build a page went to the cloud
    /// rather than into the trash.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_placeholder_names_the_file_it_stands_for() {
        let dir = TempDir::new().unwrap();
        assert_eq!(
            icloud_stub_target(&dir.path().join(".notes.md.icloud")),
            Some(dir.path().join("notes.md"))
        );
    }

    /// The trap this predicate exists for: a pre-Sonoma placeholder makes the
    /// real path fail BOTH `is_evicted` (the stat fails) and `exists()`. A
    /// poller checking either one alone concludes the user deleted the file.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_placeholder_still_counts_as_waiting_on_the_cloud() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("page.md");
        fs::write(dir.path().join(".page.md.icloud"), b"").unwrap();

        assert!(!is_evicted(&real), "the stat itself fails — this is the trap");
        assert!(!real.exists());
        assert!(is_still_in_the_cloud(&real));

        let gone = dir.path().join("never-existed.md");
        assert!(!is_still_in_the_cloud(&gone), "a plain missing file is not waiting");
    }

    /// A user's own dotfile, and a file that merely ends in `.icloud`, are
    /// ordinary content. Mapping either one would invent a page that does not
    /// exist and keep stale HTML alive forever.
    #[cfg(target_os = "macos")]
    #[test]
    fn ordinary_names_are_not_placeholders() {
        let dir = TempDir::new().unwrap();
        for name in [".gitignore", "backup.icloud", ".icloud", "notes.md"] {
            assert_eq!(
                icloud_stub_target(&dir.path().join(name)),
                None,
                "{name} is not an iCloud placeholder"
            );
        }
    }
}
