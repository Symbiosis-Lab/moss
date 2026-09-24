//! One hardened `.tar.gz` extractor, shared by the stack executor and
//! `moss desktop install`.
//! `tar::Archive::unpack` is never called: it resolves a hardlink's source
//! against the process cwd rather than the extraction root, so a
//! `parent.join(link_name)` enclosure check on the entry's declared path is
//! never actually checking the path `unpack` opens — the bug both prior
//! extractors independently worked around by writing the loop by hand.

use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// How a symlink entry is handled. A stack artifact never needs one — see
/// [`SymlinkPolicy::Reject`] — but a future `moss.app` that embeds a
/// framework would carry one, so the installer opts into
/// [`SymlinkPolicy::ContainedRelative`].
pub enum SymlinkPolicy {
    /// Refuse the entry. A stack artifact never needs one and a link plus a
    /// follow-up write-through entry is a directory-escape primitive.
    Reject,
    /// Materialize a symlink whose target is relative and contains no `..`
    /// component; refuse any other. Descent-only links cannot ascend, so
    /// containment holds by induction with no per-entry path math and no
    /// exposure to a chain through an earlier link.
    ContainedRelative,
}

#[derive(Debug)]
pub enum TarSafeError {
    UnsafeEntry { entry: String },
    Io(String),
}

impl std::fmt::Display for TarSafeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TarSafeError::UnsafeEntry { entry } => write!(f, "unsafe archive entry: {entry}"),
            TarSafeError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// A relative entry path resolved onto `target_dir`, rejecting any component
/// that would escape it (`..`, an absolute prefix, a root). Mirrors
/// `zip::ZipFile::enclosed_name()`'s guard for the `tar` crate, which has no
/// equivalent built in.
///
/// An entry that normalizes to empty (`./`, `moss.app/./`) is `Ok(None)`
/// when `is_dir` — accepted and ignored, because `tar czf x .` produces
/// exactly this and refusing it would turn an ordinary archive into a
/// refused install — and `Err` otherwise, since an empty path for a
/// non-directory entry names nothing to write.
fn enclosed_entry(
    target_dir: &Path,
    entry_path: &Path,
    is_dir: bool,
) -> Result<Option<PathBuf>, TarSafeError> {
    let mut rel = PathBuf::new();
    for component in entry_path.components() {
        match component {
            Component::Normal(part) => rel.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(TarSafeError::UnsafeEntry { entry: entry_path.display().to_string() });
            }
        }
    }
    if rel.as_os_str().is_empty() {
        if is_dir {
            return Ok(None);
        }
        return Err(TarSafeError::UnsafeEntry { entry: entry_path.display().to_string() });
    }
    Ok(Some(target_dir.join(rel)))
}

/// A symlink target that is relative and carries no `..` component. Lexical
/// containment on the *resolved* path — `normalize(parent.join(link)).
/// starts_with(target_dir)` — is not sound: `d/e/s1 -> ../../x` then
/// `s2 -> d/e/s1/../../..` normalizes inside `target_dir` and resolves
/// outside it once `s1` is followed, and a later `s2/pwn` file entry then
/// writes through the chain. Refusing any `..` component in the target
/// closes that off without resolving anything: a descent-only link can
/// never climb back out, by induction over how many of them a path passes
/// through.
fn is_contained_relative_target(target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    !target.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
}

/// Extract a `.tar.gz` stream into `target_dir`, entry by entry, never via
/// `tar::Archive::unpack`.
///
/// - Every entry's path must resolve strictly inside `target_dir`
///   ([`enclosed_entry`]).
/// - Hardlinks are refused unconditionally under both policies: their
///   source is meaningless without a filesystem `unpack` would have to
///   resolve against the process cwd, and a stack or app bundle never ships
///   one.
/// - Symlinks follow `symlinks`. On a non-unix target a symlink entry is
///   always refused, since no caller extracts a bundle there.
/// - fifo/char/block entries are skipped: never materialized from a
///   downloaded archive.
/// - Regular (and GNU-sparse) files are written with a budget enforced on
///   bytes actually WRITTEN, never the header's claimed size — `tar`, like
///   `zip`, lets an entry declare any size it likes.
/// - **Directory modes are not preserved.** A `0o500` directory entry
///   arriving before its children would break the writes that follow it,
///   and a caller's failure-path `remove_dir_all`. Every created directory
///   gets the platform default mode instead.
/// - **File modes are masked to `0o755`.** `set_permissions` ignores the
///   umask, so an unmasked `0o777` header would land a world-writable file;
///   masking keeps the executable bits (what a caller's declared-binary
///   chmod and a bundled `.app`'s `Contents/MacOS/*` both need) while
///   group/other write and setuid/setgid/sticky never survive.
pub fn extract_tar_gz(
    reader: impl Read,
    target_dir: &Path,
    symlinks: SymlinkPolicy,
    max_entries: usize,
    max_bytes: u64,
) -> Result<(), TarSafeError> {
    let decoder = flate2::read::GzDecoder::new(reader);
    let mut archive = tar::Archive::new(decoder);

    std::fs::create_dir_all(target_dir)
        .map_err(|e| TarSafeError::Io(format!("create {}: {e}", target_dir.display())))?;

    let entries = archive.entries().map_err(|e| TarSafeError::Io(e.to_string()))?;

    let mut remaining = max_bytes;
    let mut count = 0usize;
    for entry in entries {
        let mut entry = entry.map_err(|e| TarSafeError::Io(e.to_string()))?;

        count += 1;
        if count > max_entries {
            return Err(TarSafeError::Io(format!("archive exceeds the {max_entries}-entry limit")));
        }

        let entry_type = entry.header().entry_type();
        let entry_path = entry.path().map_err(|e| TarSafeError::Io(e.to_string()))?.into_owned();

        // A hardlink's source is resolved by `tar::Entry::unpack` against
        // the process cwd, not `target_dir` — no join-based enclosure check
        // here is ever checking the path that would actually be opened, so
        // the entry type is refused outright rather than validated.
        // allow:hard_link `is_hard_link` is tar's own entry-type predicate, not `fs::hard_link` — this refuses a hardlinked entry, it does not create one.
        if entry_type.is_hard_link() {
            return Err(TarSafeError::UnsafeEntry { entry: entry_path.display().to_string() });
        }

        let outpath = match enclosed_entry(target_dir, &entry_path, entry_type.is_dir())? {
            Some(path) => path,
            None => continue,
        };

        if entry_type.is_dir() {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| TarSafeError::Io(format!("mkdir {}: {e}", outpath.display())))?;
            continue;
        }

        if entry_type.is_symlink() {
            let refuse = || TarSafeError::UnsafeEntry { entry: entry_path.display().to_string() };
            match symlinks {
                SymlinkPolicy::Reject => return Err(refuse()),
                SymlinkPolicy::ContainedRelative => {
                    if !cfg!(unix) {
                        return Err(refuse());
                    }
                    let link_name = entry.link_name().map_err(|e| TarSafeError::Io(e.to_string()))?.ok_or_else(refuse)?;
                    if !is_contained_relative_target(&link_name) {
                        return Err(refuse());
                    }
                    if let Some(parent) = outpath.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| TarSafeError::Io(format!("mkdir {}: {e}", parent.display())))?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&link_name, &outpath)
                        .map_err(|e| TarSafeError::Io(format!("symlink {}: {e}", outpath.display())))?;
                }
            }
            continue;
        }

        // A GNU-sparse entry's `Read` impl transparently fills its holes
        // with zeroes as it is read, so it is materialized exactly like a
        // regular file — that expansion is exactly the budget this loop
        // bounds.
        if !(entry_type.is_file() || entry_type.is_gnu_sparse()) {
            // fifo/char/block/etc: never materialized from a downloaded
            // archive.
            continue;
        }

        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TarSafeError::Io(format!("mkdir {}: {e}", parent.display())))?;
        }
        let mode = entry.header().mode().ok();
        // allow:raw_write extraction targets are never build output (a stack under ~/.moss/stacks/, or an installer temp dir)
        let mut out = std::fs::File::create(&outpath)
            .map_err(|e| TarSafeError::Io(format!("create {}: {e}", outpath.display())))?;
        // Read at most `remaining + 1` bytes: if the entry yields more than
        // the budget, `written` exceeds `remaining` and extraction fails
        // closed before a bomb is fully materialized, regardless of what
        // the header claimed.
        let mut bounded = (&mut entry).take(remaining.saturating_add(1));
        let written = io::copy(&mut bounded, &mut out)
            .map_err(|e| TarSafeError::Io(format!("write {}: {e}", outpath.display())))?;
        if written > remaining {
            return Err(TarSafeError::Io("archive exceeds the decompressed-size budget".to_string()));
        }
        remaining -= written;
        drop(out);

        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode & 0o755))
                .map_err(|e| TarSafeError::Io(format!("chmod {}: {e}", outpath.display())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tar_safe_tests.rs"]
mod tar_safe_tests;
