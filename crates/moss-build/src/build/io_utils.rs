//! THE write primitive for moss's regenerable output tree (`.moss/build/**`).
//!
//! Per ADR-043 (`docs/decisions/ADR-043-regenerable-output-dataless-is-absent.md`):
//! **under `.moss/build/`, a dataless destination is absent.** Nothing here
//! reads the destination's bytes, waits for them, or asks the cloud provider
//! for them — the file is regenerable by definition and moss already holds the
//! replacement in memory.
//!
//! That is not an optimization, it is the correctness rule. `std::fs::write`
//! opens with `O_WRONLY|O_CREAT|O_TRUNC`, and `O_TRUNC` *requires*
//! materialization: against a cloud-evicted (`SF_DATALESS`) destination, under
//! the process-wide fail-fast policy (`platform::macos::iopolicy`), it returns
//! `EDEADLK` — "Resource deadlock avoided (os error 11)". Every raw `fs::write`
//! landing in the output tree therefore failed the whole build whenever iCloud
//! had evicted moss's own output (moss#964).
//!
//! So every write here is **temp + `rename(2)`**. `rename` does not touch data
//! extents, so it cannot materialize and cannot `EDEADLK`; it is also atomic
//! within a directory, which matters because the preview server serves
//! `.moss/build/staging/` live while the build writes into it. A concurrent
//! reader sees the old file or the new one, never a torn one.
//!
//! The temp suffix is `.pending.<uuid>`, not `.tmp` — iCloud Drive excludes
//! `.tmp` from sync, which is the wrong signal for a file about to become a
//! real output. Same convention as `emit/math_png.rs` and `og_card.rs`.
//!
//! This module is the seed of M6b's StageWriter: pure I/O, no `tauri`, so it
//! moves into `crates/moss-build` unchanged. `output_write_invariant_test`
//! keeps raw writes out of `src/build/` unless they carry an
//! `// allow:raw_write <reason>` marker.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A temp sibling of `path` in the same directory, so the `rename` stays within
/// one filesystem (a cross-device rename is `EXDEV`, not a silent copy).
fn pending_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.pending.{}", uuid::Uuid::new_v4()))
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        create_output_dir_all(parent)?;
    }
    Ok(())
}

/// `create_dir_all` for the output tree: ADR-043's rule, applied to directories.
///
/// Every *file* write here already treats a dataless destination as absent. The
/// directories those files live in had no such rule, and a plain
/// `fs::create_dir_all` under `.moss/build/` can fail `EDEADLK` — "Resource
/// deadlock avoided (os error 11)" — when the provider will not hand back a
/// directory entry it is managing. That is the same fail-fast policy
/// (`platform::macos::iopolicy`) that `write_output` exists to route around,
/// reached one level up.
///
/// It is not hypothetical: it took down the *first* preview of a Google Drive
/// vault with `Failed to create staging directory: Resource deadlock avoided
/// (os error 11)` on the onboarding overlay, before the build could reach the
/// cloud gate that would have explained itself (moss#982 follow-up).
///
/// **Why replace rather than wait.** A regenerable directory is absent when it
/// is unreadable, exactly as a regenerable file is. Waiting is not merely slow
/// here, it is unreachable: `build_shell::watch::sweep::should_descend` refuses to
/// descend into `.moss/build` and `watch::path_is_watchable` refuses to watch
/// it, so nothing would ever request the download and no arrival would ever
/// schedule the rebuild that lowers a gate. A waiting screen raised over this
/// path could never come down — the hazard `build::outcome::Disposition::Report`
/// documents. So the entry is removed and remade.
///
/// `remove_dir_all` is safe against dataless children: `unlink` does not touch
/// data extents, so it cannot materialize and cannot deadlock (same property
/// `Disposition::Discard` relies on). The cost of removing is one non-incremental
/// build, which is the same cost the `Discard` arm already accepts.
///
/// **Only `.moss/build/` may be replaced.** The walk starts at the filesystem
/// root, so without [`is_regenerable_output`] the destructive arm could fire on
/// an ancestor — and the vault directory itself is an ancestor of every output
/// path moss writes. Deleting it would destroy the user's site to fix a
/// scratch directory. Everything above `.moss/build/` reports the error
/// unchanged: nothing there is regenerable, so "unreadable is not absent" is
/// still the rule.
pub fn create_output_dir_all(dir: &Path) -> io::Result<()> {
    match fs::create_dir_all(dir) {
        Ok(()) => return Ok(()),
        Err(e) if !crate::build::icloud::is_dataless_unavailable(&e) => return Err(e),
        Err(e) if !is_regenerable_output(dir) => return Err(e),
        Err(_) => {}
    }

    // Walk down from the root and rebuild the first component the provider
    // refuses. `create_dir_all` reports one error for the whole chain without
    // saying which component produced it, so the component has to be found by
    // creating them one at a time.
    let mut built = PathBuf::new();
    for component in dir.components() {
        built.push(component);
        match fs::create_dir(&built) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e)
                if crate::build::icloud::is_dataless_unavailable(&e)
                    && is_regenerable_output(&built) =>
            {
                log::warn!(
                    "[output-dir] {} could not be created — the cloud provider refused to \
                     materialize it. It is regenerable output, so it is being replaced rather \
                     than waited for (ADR-043).",
                    built.display()
                );
                fs::remove_dir_all(&built).or_else(|rm| {
                    // A file (or a dataless placeholder for one) sitting where a
                    // directory belongs: `remove_dir_all` gives ENOTDIR, and
                    // `remove_file` is the right verb. Still an unlink, so still
                    // cannot materialize.
                    if rm.kind() == io::ErrorKind::NotFound { Ok(()) } else { fs::remove_file(&built) }
                })?;
                fs::create_dir(&built)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Is `dir` at or below some `.moss/build/`?
///
/// The one question that licenses deleting a directory. Matched on the path's
/// own components rather than against a folder root, because `ensure_parent`'s
/// callers hold an output path and nothing else — the same reason
/// `cloud_ledger` is keyed by path. `.moss/build` itself does **not** qualify:
/// remaking it would drop every sealed generation at once, and the gate that
/// keeps a served site up depends on one surviving.
fn is_regenerable_output(dir: &Path) -> bool {
    let parts: Vec<_> = dir.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(2)
        .position(|w| w[0] == ".moss" && w[1] == "build")
        // `+ 2` is the `.moss/build` pair itself; a qualifying path has at
        // least one component below it.
        .is_some_and(|at| parts.len() > at + 2)
}

/// Finish an output write: `rename` the already-populated `tmp` over `path`,
/// removing `tmp` if that fails so a failed build leaves no `.pending.*` litter
/// behind.
fn commit(tmp: &Path, path: &Path) -> io::Result<()> {
    match fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(tmp);
            Err(e)
        }
    }
}

/// Write `bytes` to `path` in the output tree, atomically and without ever
/// materializing the destination.
///
/// Creates parent directories as needed. Replaces whatever is at `path` —
/// including a cloud-evicted placeholder, a hardlink into the CAS, or a
/// symlink — with a fresh independent inode.
pub fn write_output(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_parent(path)?;
    let tmp = pending_sibling(path);
    match fs::write(&tmp, bytes) {  // allow:raw_write the temp is freshly minted by this call — never dataless
        Ok(()) => commit(&tmp, path),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Copy `src` onto `dst` in the output tree, atomically and without ever
/// materializing `dst`.
///
/// The copy lands in a temp sibling first, so `fs::copy`'s `O_TRUNC` open never
/// sees the real destination. On APFS `fs::copy` still becomes a COW reflink
/// (`fclonefileat(2)`) into the temp, so this keeps the near-zero disk cost the
/// iCloud hardlink rule (CLAUDE.md, ADR-013) depends on — and, because the
/// result is a fresh inode, it subsumes `ship::unlink_if_hardlinked_to` on the
/// paths that use it.
///
/// Note `src` is still *read*: this defends against an evicted **destination**,
/// not an evicted source.
pub fn copy_output(src: &Path, dst: &Path) -> io::Result<()> {
    ensure_parent(dst)?;
    let tmp = pending_sibling(dst);
    match fs::copy(src, &tmp) {  // allow:raw_write the temp is freshly minted by this call — never dataless
        Ok(_) => commit(&tmp, dst),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Writes `bytes` to `path` only if the current file content differs.
///
/// Returns `Ok(true)` if the file was written, `Ok(false)` if it was already
/// identical. The comparison read is best-effort by design: an unreadable
/// destination (missing, or dataless under the fail-fast policy) simply means
/// "different", and the write proceeds. Per ADR-043 there is nothing there to
/// preserve, so there is nothing to wait for.
pub fn write_output_if_changed(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if let Ok(existing) = fs::read(path) {
        if existing == bytes {
            return Ok(false);
        }
    }
    write_output(path, bytes)?;
    Ok(true)
}

/// Is there an output at `path` that moss can ship without reading it back?
///
/// The read half of ADR-043's "dataless is absent": every presence check on
/// regenerable output asks this, so "already built" cannot mean a cloud-only
/// placeholder that the next `copy_output` will fail on. The checks this
/// replaces predate the 0-byte-stub era and none consulted the evicted bit,
/// so each was a latent form of the same incident — the check says present,
/// the later copy fails, and the failure is fatal.
///
/// A symlink is present when the link itself is (`120000:` manifest entries
/// are shipped with `read_link`, never by reading the target). Both the
/// regular-file test and `is_evicted` are false for a link, so a predicate
/// that forgot this arm would unlink every preserved symlink.
pub fn output_present(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => true,
        Ok(meta) => meta.is_file() && meta.len() > 0 && !crate::build::icloud::is_evicted(path),
        Err(_) => false,
    }
}

/// [`output_present`] for a path a manifest entry names, where `mode` is that
/// entry's octal mode.
///
/// The plain predicate answers by the link, which is right for a `120000:`
/// entry and wrong for a `100644:` one: a dangling symlink standing where a
/// file entry expects bytes would be called present, skipped by the presence
/// pass, and then fail the copy in `ship_phase`. Asking through the link for a
/// file entry keeps the manifest and the disk agreeing on the same object.
pub fn entry_output_present(path: &Path, mode: &str) -> bool {
    if mode == crate::types::content::MODE_SYMLINK {
        return output_present(path);
    }
    // Resolve first, then ask of the object ship will actually read. Asking
    // `is_file()` (which follows) beside `output_present` (which does not)
    // answered yes for a link onto an evicted or 0-byte target — the same
    // class this predicate exists to remove, one indirection down.
    // `canonicalize` fails on a dangling link, which is the absent answer.
    match fs::canonicalize(path) {
        Ok(real) => output_present(&real),
        Err(_) => false,
    }
}

#[cfg(test)]
#[path = "io_utils_tests.rs"]
mod tests;
