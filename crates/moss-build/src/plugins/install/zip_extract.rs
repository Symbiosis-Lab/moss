//! Hardened ZIP extraction shared by the build-time plugin downloader
//! (`build.rs`, which includes this file via `#[path]`) and the runtime
//! registry installer ([`super::registry_client::artifact`]).
//!
//! Depends ONLY on `std` + `zip` so `build.rs` — a separate compilation unit
//! that cannot resolve `crate::` paths — can include this file directly. Do
//! NOT add `crate::`/`super::` references outside the `#[cfg(test)]` module.
//!
//! Hardening (the `build.rs` extractor this replaces was Zip-Slip-vulnerable —
//! it joined raw entry names onto the target dir with no containment check):
//!   - every entry path must resolve strictly INSIDE `target_dir`
//!     (`enclosed_name()` rejects `..` and absolute paths);
//!   - symlink entries are rejected outright (a symlink plus a follow-up
//!     write-through entry is a directory-escape primitive);
//!   - total decompressed size and entry count are capped (zip-bomb backstop).

use std::io;
use std::io::{Read, Seek};
use std::path::Path;

/// Backstops against zip bombs. Plugin/theme archives are small; these are
/// generous ceilings, not tight limits.
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_UNCOMPRESSED: u64 = 512 * 1024 * 1024; // 512 MiB

/// Unix file-type bits: `S_IFMT` masks the type, `S_IFLNK` marks a symlink.
const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;

#[derive(Debug)]
pub enum ZipExtractError {
    /// An entry's path escaped `target_dir` (`..`, absolute, or otherwise
    /// non-enclosed). Carries the offending entry name.
    PathTraversal(String),
    /// An entry is a symlink; symlinks are rejected outright.
    SymlinkEntry(String),
    /// Decompressed size or entry count exceeded the zip-bomb ceilings.
    TooLarge,
    /// The archive could not be opened or an entry could not be read.
    Zip(String),
    /// A filesystem write failed.
    Io(String),
}

impl std::fmt::Display for ZipExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipExtractError::PathTraversal(name) => {
                write!(f, "zip entry '{name}' escapes the target directory")
            }
            ZipExtractError::SymlinkEntry(name) => {
                write!(f, "zip entry '{name}' is a symlink (rejected)")
            }
            ZipExtractError::TooLarge => {
                write!(f, "zip exceeds the decompressed-size or entry-count limit")
            }
            ZipExtractError::Zip(e) => write!(f, "zip error: {e}"),
            ZipExtractError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ZipExtractError {}

/// True when a zip entry's unix mode marks it as a symlink.
fn is_symlink_mode(mode: Option<u32>) -> bool {
    matches!(mode, Some(m) if m & S_IFMT == S_IFLNK)
}

/// Extract `bytes` (a ZIP archive) into `target_dir`, rejecting any entry that
/// would escape the directory, any symlink entry, and archives that blow the
/// zip-bomb ceilings. `target_dir` is created if absent.
pub fn extract_zip_safe(bytes: &[u8], target_dir: &Path) -> Result<(), ZipExtractError> {
    extract_zip_safe_limited(bytes, target_dir, MAX_ENTRIES, MAX_TOTAL_UNCOMPRESSED)
}

/// `extract_zip_safe` with explicit ceilings, so tests can exercise the
/// zip-bomb guards without materializing a 20k-entry / 512-MiB archive.
fn extract_zip_safe_limited(
    bytes: &[u8],
    target_dir: &Path,
    max_entries: usize,
    max_total_uncompressed: u64,
) -> Result<(), ZipExtractError> {
    extract_zip_safe_limited_reader(io::Cursor::new(bytes), target_dir, max_entries, max_total_uncompressed)
}

/// `extract_zip_path_limited` with explicit ceilings, so tests can exercise
/// the zip-bomb guards without materializing a 20k-entry / 512-MiB archive.
///
/// A stack artifact is measured in hundreds of megabytes, so the executor
/// (`system::stack_exec::extract`) cannot buffer one into memory the way
/// [`extract_zip_safe`] does for a plugin/theme zip — this streams straight
/// from the file. Same guards, same code: only the reader differs.
pub(crate) fn extract_zip_path_limited(
    archive: &Path,
    target_dir: &Path,
    max_entries: usize,
    max_total_uncompressed: u64,
) -> Result<(), ZipExtractError> {
    let file = std::fs::File::open(archive).map_err(|e| ZipExtractError::Io(e.to_string()))?;
    extract_zip_safe_limited_reader(file, target_dir, max_entries, max_total_uncompressed)
}

fn extract_zip_safe_limited_reader<R: Read + Seek>(
    reader: R,
    target_dir: &Path,
    max_entries: usize,
    max_total_uncompressed: u64,
) -> Result<(), ZipExtractError> {
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| ZipExtractError::Zip(e.to_string()))?;

    if archive.len() > max_entries {
        return Err(ZipExtractError::TooLarge);
    }

    std::fs::create_dir_all(target_dir).map_err(|e| ZipExtractError::Io(e.to_string()))?;

    // Budget of decompressed bytes remaining across ALL entries. Enforced on
    // bytes actually written, not on `entry.size()` (the header-declared
    // uncompressed size, which an attacker sets freely — a lying header would
    // otherwise let `io::copy` stream an unbounded bomb to disk).
    let mut remaining = max_total_uncompressed;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ZipExtractError::Zip(e.to_string()))?;
        let name = entry.name().to_string();

        // Symlink entries are rejected outright: a symlink plus a follow-up
        // write-through entry is a directory-escape primitive, and no consumer
        // of this extractor should be materializing links from an archive.
        if is_symlink_mode(entry.unix_mode()) {
            return Err(ZipExtractError::SymlinkEntry(name));
        }

        // `enclosed_name()` is None for any path with a `..` component or a
        // root/absolute prefix — the Zip-Slip guard. Never join the raw name.
        let rel = entry
            .enclosed_name()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| ZipExtractError::PathTraversal(name.clone()))?;

        let outpath = target_dir.join(&rel);
        if name.ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| ZipExtractError::Io(e.to_string()))?;
        } else {
            if let Some(p) = outpath.parent() {
                std::fs::create_dir_all(p).map_err(|e| ZipExtractError::Io(e.to_string()))?;
            }
            let mut out =
                // allow:raw_write plugin zips extract under .moss/plugins/, not build output
                std::fs::File::create(&outpath).map_err(|e| ZipExtractError::Io(e.to_string()))?;
            // Read at most `remaining + 1` decompressed bytes: if the entry
            // yields more than the budget, `written` exceeds `remaining` and we
            // fail closed before the bomb is fully materialized.
            let mut bounded = entry.by_ref().take(remaining.saturating_add(1));
            let written =
                io::copy(&mut bounded, &mut out).map_err(|e| ZipExtractError::Io(e.to_string()))?;
            if written > remaining {
                return Err(ZipExtractError::TooLarge);
            }
            remaining -= written;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "zip_extract_tests.rs"]
pub(crate) mod tests;
