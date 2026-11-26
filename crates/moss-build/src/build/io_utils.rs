//! THE write primitive for moss's regenerable output tree (`.moss/build.nosync/**`).
//!
//! The rule this module upholds:
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
//! had evicted moss's own output.
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
//! This module owns atomic site-output writes. There is no per-build
//! path-claim registry: a concurrent build racing the shared staging tree
//! between seal and ship is covered by content-addressed manifest entries.
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

/// `create_dir_all` for the output tree. The tree lives under the platform
/// cache directory, which no cloud provider manages, so a directory there is
/// never dataless.
pub fn create_output_dir_all(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

/// Read a transform record's text. The one read step of a record, so a test can
/// make it fail the way a provider or a file system does ([`fault::fail_reads`]).
pub(crate) fn read_record_file(path: &Path) -> io::Result<String> {
    #[cfg(test)]
    if let Some(e) = fault::read_failure(path) {
        return Err(e);
    }
    fs::read_to_string(path)
}

/// `rename`, or across volumes a copy beside `to` renamed into place, then
/// the original removed. Any other rename error leaves the original in place. The copy keeps the `.pending.` name the sweep skips
/// until it lands.
pub(crate) fn move_across_volumes(from: &Path, to: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> bool {
    // allow:unlink callers move between stores, never out of staging
    match rename(from, to) {
        Ok(()) => return true,
        Err(e) if e.kind() != io::ErrorKind::CrossesDevices => return false,
        Err(_) => {}
    }
    let pending = to.with_file_name(format!("{}.pending.{}", to.file_name().and_then(|n| n.to_str()).unwrap_or("blob"), std::process::id()));
    // allow:unlink the copy lands in place before the original goes
    let landed = fs::copy(from, &pending).and_then(|_| fs::rename(&pending, to)).is_ok(); // allow:raw_write a pending copy only this call names
    if !landed {
        // allow:unlink our own pending copy
        let _ = fs::remove_file(&pending);
        return false;
    }
    // allow:unlink the original, once its copy has landed
    fs::remove_file(from).is_ok()
}

#[cfg(test)]
pub(crate) mod fault {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    thread_local! {
        static FAILING_READS: RefCell<Option<(PathBuf, i32, u32)>> = const { RefCell::new(None) };
    }

    /// On this thread, make the next `times` record reads of `path` fail with the
    /// operating-system error `errno`.
    pub(crate) fn fail_reads(path: &Path, errno: i32, times: u32) {
        FAILING_READS.with(|f| *f.borrow_mut() = Some((path.to_path_buf(), errno, times)));
    }

    pub(super) fn read_failure(path: &Path) -> Option<std::io::Error> {
        FAILING_READS.with(|f| {
            let mut f = f.borrow_mut();
            let (target, errno, left) = f.as_mut()?;
            (path == target && *left > 0).then(|| {
                *left -= 1;
                std::io::Error::from_raw_os_error(*errno)
            })
        })
    }
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

/// Append telemetry without truncating a destination; evicted output starts fresh.
pub(crate) fn append_output(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    ensure_parent(path)?;
    if crate::build::icloud::is_evicted(path) {
        return write_output(path, bytes);
    }
    fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(bytes)
}

/// Copy `src` onto `dst` in the output tree, atomically and without ever
/// materializing `dst`.
///
/// The copy lands in a temp sibling first, so `fs::copy`'s `O_TRUNC` open never
/// sees the real destination. On APFS `fs::copy` still becomes a COW reflink
/// (`fclonefileat(2)`) into the temp, so this keeps the near-zero disk cost the
/// iCloud hardlink rule depends on — and, because the
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

/// Whether [`clone_output_dir`] can ever succeed on this platform.
pub const CLONES_DIRS: bool = cfg!(target_os = "macos");

/// Clone the whole tree at `src` to `dst`, which must not exist, in one
/// copy-on-write `clonefile(2)`: every file in `dst` is an independent inode
/// sharing `src`'s extents, the same thing [`copy_output`] makes one file at a
/// time. `ENOTSUP` off APFS, and `Unsupported` off macOS, where no single call
/// clones a directory. Symlinks inside the tree are cloned as links.
#[cfg(target_os = "macos")]
pub fn clone_output_dir(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    // `CLONE_NOFOLLOW` from <sys/clonefile.h>, which `libc` does not export.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    let src = std::ffi::CString::new(src.as_os_str().as_bytes())?;
    let dst = std::ffi::CString::new(dst.as_os_str().as_bytes())?;
    match unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn clone_output_dir(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Writes `bytes` to `path` only if the current file content differs.
///
/// Returns `Ok(true)` if the file was written, `Ok(false)` if it was already
/// identical. The comparison read is best-effort by design: an unreadable
/// destination (missing, or dataless under the fail-fast policy) simply means
/// "different", and the write proceeds. There is nothing there to
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
/// each new video run wiped it on entry — taking the temps of an image batch,
/// or of the run it had just joined, mid-write.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// `<tmp_root>/<name>-<uuid>`. A directory that cannot be made is logged;
    /// the writes into it then fail on their own and are reported there.
    pub fn new(tmp_root: &Path, name: &str) -> Self {
        let dir = tmp_root.join(format!("{name}-{}", uuid::Uuid::new_v4()));
        if let Err(e) = create_output_dir_all(&dir) {
            log::info!("[scratch] could not create {}: {}", dir.display(), e);
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
/// output is regenerable and counts as gone, but an I/O error other
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
        Ok(meta) => probe_stat(path, &meta),
        Err(e) => probe_error(path, e),
    }
}

/// [`probe_path`] given `meta`, `path`'s own `symlink_metadata`.
fn probe_stat(path: &Path, meta: &fs::Metadata) -> Presence {
    if meta.file_type().is_symlink() {
        Presence::Present
    } else if !meta.is_file() {
        Presence::Absent
    } else if meta.len() == 0 || crate::build::icloud::is_evicted_stat(path, meta) {
        Presence::Evicted
    } else {
        Presence::Present
    }
}

/// [`Presence`] of the output a manifest entry with octal `mode` names at `path`.
///
/// A `120000:` entry is asked by the link. Any other entry is asked of the
/// object ship will actually read, through the link: a dangling symlink
/// standing where a file entry expects bytes would otherwise be called present
/// and then fail the copy in `ship_phase`.
pub fn probe_output(path: &Path, mode: &str) -> Presence {
    probe_output_stat(path, mode, fs::symlink_metadata(path))
}

/// [`probe_output`] given `stat`, `path`'s own `symlink_metadata`, for a
/// caller that already took it. One `lstat` answers unless the leaf is a link
/// a file entry reads through; only then is the path resolved. The `lstat`
/// itself already follows every ancestor.
pub fn probe_output_stat(path: &Path, mode: &str, stat: io::Result<fs::Metadata>) -> Presence {
    match stat {
        Ok(meta) if meta.file_type().is_symlink() && mode != crate::types::content::MODE_SYMLINK => {
            match fs::canonicalize(path) {
                Ok(real) => probe_path(&real),
                Err(e) => probe_error(path, e),
            }
        }
        Ok(meta) => probe_stat(path, &meta),
        Err(e) => probe_error(path, e),
    }
}

/// Is there an output at `path` that moss can ship without reading it back?
///
/// The read half of the "dataless is absent" rule, for callers whose "not
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
