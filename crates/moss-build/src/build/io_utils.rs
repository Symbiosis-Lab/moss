//! THE write primitive for moss's regenerable output tree (`.moss/build.nosync/**`).
//!
//! Per ADR-043 (`docs/decisions/ADR-043-regenerable-output-dataless-is-absent.md`):
//! **under `.moss/build.nosync/`, a dataless destination is absent.** Nothing here
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
//! `.moss/build.nosync/staging/` live while the build writes into it. A concurrent
//! reader sees the old file or the new one, never a torn one.
//!
//! The temp suffix is `.pending.<uuid>`, not `.tmp` — iCloud Drive excludes
//! `.tmp` from sync, which is the wrong signal for a file about to become a
//! real output. Same convention as `emit/math_png.rs` and `og_card.rs`.
//!
//! This module owns atomic site-output writes today. ADR-052 sketched folding
//! it into a future `StageWriter` with a per-build path-claim registry (M6b);
//! that registry was never built, and the correctness gap it was later
//! invoked for — a concurrent build racing the shared staging tree between
//! seal and ship — shipped instead as content-addressed manifest entries. See
//! ADR-052's 2026-09-17 update for what, if anything, a registry still owns.
//! `output_write_invariant_test` keeps raw writes out of `src/build/` unless
//! they carry an `// allow:raw_write <reason>` marker.

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
/// `fs::create_dir_all` under `.moss/build.nosync/` can fail `EDEADLK` — "Resource
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
/// descend into `.moss/build.nosync` and `watch::path_is_watchable` refuses to watch
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
/// **Only `.moss/build.nosync/` may be replaced.** The walk starts at the filesystem
/// root, so without [`is_regenerable_output`] the destructive arm could fire on
/// an ancestor — and the vault directory itself is an ancestor of every output
/// path moss writes. Deleting it would destroy the user's site to fix a
/// scratch directory. Everything above `.moss/build.nosync/` reports the error
/// unchanged: nothing there is regenerable, so "unreadable is not absent" is
/// still the rule.
pub fn create_output_dir_all(dir: &Path) -> io::Result<()> {
    match refused(dir, Refusal::Chain).map_or_else(|| fs::create_dir_all(dir), Err) {
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
        match refused(&built, Refusal::Entry).map_or_else(|| fs::create_dir(&built), Err) {
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

#[derive(Clone, Copy, PartialEq)]
enum Refusal {
    /// A `create_dir_all` reaching through the refused directory.
    Chain,
    /// A `create_dir` of the refused directory itself.
    Entry,
}

/// The `EDEADLK` a cloud provider answers for a directory it will not
/// materialize, when a test has asked for one ([`fault::refuse_dataless`]); no
/// CI moss runs on has a File Provider vault to produce the real one.
#[cfg(test)]
fn refused(dir: &Path, how: Refusal) -> Option<io::Error> {
    fault::refuses(dir, how).then(|| io::Error::from_raw_os_error(libc::EDEADLK))
}

#[cfg(not(test))]
fn refused(_dir: &Path, _how: Refusal) -> Option<io::Error> {
    None
}

#[cfg(test)]
pub(crate) mod fault {
    use super::Refusal;
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    thread_local! {
        static REFUSED: RefCell<Option<(PathBuf, bool)>> = const { RefCell::new(None) };
    }

    /// On this thread, refuse `dir` the way a provider refuses a dataless
    /// directory: every `create_dir_all` through it, and the first
    /// `create_dir` of it.
    pub(crate) fn refuse_dataless(dir: &Path) {
        REFUSED.with(|r| *r.borrow_mut() = Some((dir.to_path_buf(), true)));
    }

    pub(super) fn refuses(path: &Path, how: Refusal) -> bool {
        REFUSED.with(|r| {
            let mut r = r.borrow_mut();
            let Some((dir, entry_left)) = r.as_mut() else { return false };
            match how {
                Refusal::Chain => path.starts_with(&*dir) && *entry_left,
                Refusal::Entry if path == dir && *entry_left => {
                    *entry_left = false;
                    true
                }
                Refusal::Entry => false,
            }
        })
    }
}

/// Is `dir` at or below some `.moss/build.nosync/`?
///
/// The one question that licenses deleting a directory. Matched on the path's
/// own components rather than against a folder root, because `ensure_parent`'s
/// callers hold an output path and nothing else — the same reason
/// `cloud_ledger` is keyed by path. `.moss/build.nosync` itself does **not** qualify:
/// remaking it would drop every sealed generation at once, and the gate that
/// keeps a served site up depends on one surviving.
fn is_regenerable_output(dir: &Path) -> bool {
    let parts: Vec<_> = dir.components().map(|c| c.as_os_str()).collect();
    parts
        .windows(2)
        .position(|w| w[0] == ".moss" && w[1] == "build.nosync")
        // `+ 2` is the `.moss/build.nosync` pair itself; a qualifying path has at
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

/// `remove_dir_all` for the output tree, with an absent directory as success.
///
/// The one door for removing a directory under `.moss/build.nosync/`, beside
/// [`create_output_dir_all`] for making one. Unlinking does not touch data
/// extents, so it cannot materialize a dataless child.
pub fn remove_output_dir_all(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// A scratch directory under `cache/tmp` that one encode run or image batch
/// owns, removed when the owner drops it. Runs used to share `cache/tmp`, and
/// each new video run wiped it on entry — taking the temps and two-pass logs of
/// an image batch, or of the run it had just joined, mid-write.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// `<tmp_root>/<name>-<uuid>`. A directory that cannot be made is logged;
    /// the writes into it then fail on their own and are reported there.
    pub fn new(tmp_root: &Path, name: &str) -> Self {
        let dir = tmp_root.join(format!("{name}-{}", uuid::Uuid::new_v4()));
        if let Err(e) = create_output_dir_all(&dir) {
            log::warn!("[scratch] could not create {}: {}", dir.display(), e);
        }
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = remove_output_dir_all(&self.0);
    }
}

/// Remove every scratch directory under `tmp_root`: what a dead process left.
/// Only safe before anything in this process can be writing there.
pub fn clear_scratch(tmp_root: &Path) {
    if let Err(e) = remove_output_dir_all(tmp_root) {
        log::debug!("[scratch] could not clear {}: {}", tmp_root.display(), e);
    }
}

/// Put a symlink to `target` at `dest`, replacing a file or link already there
/// without a moment where `dest` is absent: the link is made at a pending
/// sibling and renamed over `dest`. A directory standing at `dest` fails the
/// rename and is left alone for the staging sweep.
#[cfg(unix)]
pub fn replace_with_symlink(target: &Path, dest: &Path) -> io::Result<()> {
    let pending = pending_sibling(dest);
    std::os::unix::fs::symlink(target, &pending)?;
    fs::rename(&pending, dest).inspect_err(|_| {
        let _ = fs::remove_file(&pending);
    })
}

/// What a presence check on regenerable output could actually learn.
///
/// `Absent` and `Evicted` are answers; `Unverified` is the absence of one. The
/// distinction is the whole point: under `.moss/build.nosync/` a dataless or 0-byte
/// output is regenerable and counts as gone (ADR-043), but an I/O error other
/// than a positive `NotFound` says nothing about whether the bytes are there.
/// On a cloud-managed build tree `EDEADLK`, `EACCES` and a `NotFound` with a
/// `.name.icloud` stub beside it are ordinary inputs, and a caller that reads
/// any of them as absence drops, strips or deletes a healthy output — one
/// presence pass that could read none of 822 entries dropped all 822.
/// Destructive callers match on this; only non-destructive ones may collapse
/// it to [`output_present`].
#[derive(Debug)]
pub enum Presence {
    Present,
    /// Positively gone: `NotFound` with no cloud placeholder standing in, or a
    /// non-file where a file output belongs.
    Absent,
    /// Stat succeeded and the output is a 0-byte stub or dataless.
    Evicted,
    /// The check itself failed, so nothing is known.
    Unverified(io::Error),
}

impl Presence {
    pub fn is_present(&self) -> bool {
        matches!(self, Presence::Present)
    }
}

/// Classify a failed stat or resolve of `path`.
fn probe_error(path: &Path, err: io::Error) -> Presence {
    if crate::build::icloud::is_definitely_absent(path, &err) {
        Presence::Absent
    } else {
        Presence::Unverified(err)
    }
}

/// Probe `path` itself, never through a final symlink: a link is present when
/// the link is (`120000:` manifest entries ship with `read_link`). Both the
/// regular-file test and `is_evicted` are false for a link, so a probe that
/// forgot this arm would unlink every preserved symlink.
pub fn probe_path(path: &Path) -> Presence {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Presence::Present,
        Ok(meta) if !meta.is_file() => Presence::Absent,
        Ok(meta) if meta.len() == 0 || crate::build::icloud::is_evicted(path) => Presence::Evicted,
        Ok(_) => Presence::Present,
        Err(e) => probe_error(path, e),
    }
}

/// [`Presence`] of the output a manifest entry with octal `mode` names at `path`.
///
/// A `120000:` entry is asked by the link. Any other entry is asked of the
/// object ship will actually read, through the link: a dangling symlink
/// standing where a file entry expects bytes would otherwise be called present
/// and then fail the copy in `ship_phase`.
pub fn probe_output(path: &Path, mode: &str) -> Presence {
    if mode == crate::types::content::MODE_SYMLINK {
        return probe_path(path);
    }
    match fs::canonicalize(path) {
        Ok(real) => probe_path(&real),
        Err(e) => probe_error(path, e),
    }
}

/// Is there an output at `path` that moss can ship without reading it back?
///
/// The read half of ADR-043's "dataless is absent", for callers whose "not
/// present" arm only produces a replacement through temp-and-rename and
/// removes nothing. A caller that drops, strips, fails or deletes on the
/// answer must use [`probe_output`] instead, because this collapses
/// `Unverified` into "not present".
pub fn output_present(path: &Path) -> bool {
    probe_path(path).is_present()
}

/// [`output_present`] for a path a manifest entry names, where `mode` is that
/// entry's octal mode. Same non-destructive restriction.
pub fn entry_output_present(path: &Path, mode: &str) -> bool {
    probe_output(path, mode).is_present()
}

#[cfg(test)]
#[path = "io_utils_tests.rs"]
mod tests;
