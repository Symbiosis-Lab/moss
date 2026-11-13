//! Cloud-file materialization primitives: the macOS file-provider I/O policy
//! and Foundation file coordination, crossed with `build::cloud_prefetch` —
//! their only production caller. The app's `platform` facade re-exports these
//! (`set_dataless_fail_fast` is called at startup, app-side).
//!
//! Only `build::cloud_prefetch`'s workers may call the materializers — see
//! each module's header for why (coordination can hang a thread indefinitely).
#[cfg(target_os = "macos")]
pub mod file_coordination;
#[cfg(target_os = "macos")]
pub mod iopolicy;
// A site's root folder shows moss's leaf in the file manager.
pub mod folder_icon;
#[cfg(target_os = "macos")]
pub mod download_request;

/// Ask the system to download a cloud placeholder, without waiting for it. See
/// [`download_request`]. Any thread may call it.
#[cfg(target_os = "macos")]
pub fn request_download(path: &std::path::Path) -> Result<(), String> {
    download_request::request_download(path)
}

/// Windows placeholders hydrate on a read and Linux sync clients present
/// ordinary files, so there is nothing to ask for here yet.
#[cfg(not(target_os = "macos"))]
pub fn request_download(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}

/// Allow the calling thread to materialize dataless files, overriding the
/// process-wide fail-fast policy. See [`iopolicy`] — only the dedicated
/// `build::cloud_prefetch` workers may call this.
#[cfg(target_os = "macos")]
pub fn materialize_on_this_thread() -> bool {
    iopolicy::materialize_on_this_thread()
}

/// Nothing to opt in to elsewhere: Windows Cloud Filter placeholders hydrate
/// on a real read already, Linux sync clients present ordinary files, and
/// neither has a fail-fast policy to override.
#[cfg(not(target_os = "macos"))]
pub fn materialize_on_this_thread() -> bool {
    false
}

/// A rename that never replaces `to`: it fails with `AlreadyExists`, decided
/// by the kernel in the same step as the move, so nothing can slip in between
/// a check and the rename (`renamex_np(RENAME_EXCL)` on macOS,
/// `renameat2(RENAME_NOREPLACE)` on Linux). Elsewhere, or on a filesystem
/// that refuses the flag, it checks and then renames, which leaves a window
/// of microseconds; on Windows a directory is never replaced either way.
pub fn rename_exclusive(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = |p: &std::path::Path| {
            std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
        };
        let (f, t) = (c(from)?, c(to)?);
        // SAFETY: both are NUL-terminated paths that outlive the call.
        #[cfg(target_os = "macos")]
        let rc = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
        // SAFETY: as above; AT_FDCWD resolves both relative to the cwd, as `rename` does.
        #[cfg(target_os = "linux")]
        let rc = unsafe { libc::renameat2(libc::AT_FDCWD, f.as_ptr(), libc::AT_FDCWD, t.as_ptr(), libc::RENAME_NOREPLACE) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if !matches!(err.raw_os_error(), Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS)) {
            return Err(err);
        }
    }
    if to.symlink_metadata().is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(from, to)
}

/// Opt this process out of implicit dataless-file materialization,
/// process-wide. See [`iopolicy`] for what this changes and why.
#[cfg(target_os = "macos")]
pub fn set_dataless_fail_fast() -> bool {
    iopolicy::set_dataless_fail_fast()
}

/// No fail-fast policy exists off macOS; there is nothing to opt out of.
#[cfg(not(target_os = "macos"))]
pub fn set_dataless_fail_fast() -> bool {
    false
}

/// Keep ordinary blocking workers from implicitly downloading cloud files.
#[cfg(target_os = "macos")]
pub fn fail_fast_on_this_thread() -> bool { iopolicy::fail_fast_on_this_thread() }

#[cfg(not(target_os = "macos"))]
pub fn fail_fast_on_this_thread() -> bool { true }

/// Ask the file provider for a dataless file's **whole** contents, via
/// Foundation file coordination, and block until it has them.
///
/// **Only `build::cloud_prefetch`'s reader pool may call this.** Coordination
/// beats the process-wide fail-fast policy, so unlike an ordinary read it can
/// hang any thread indefinitely. See [`file_coordination`].
#[cfg(target_os = "macos")]
pub fn materialize_whole_file(path: &std::path::Path) -> Result<(), String> {
    file_coordination::materialize_whole_file(path)
}

/// Not available on this platform — the caller falls back to reading the
/// file, which is already a whole-file read here.
#[cfg(not(target_os = "macos"))]
pub fn materialize_whole_file(_path: &std::path::Path) -> Result<(), String> {
    Err("file coordination is macOS-only".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn dataless_reads_fail_fast() -> bool { iopolicy::reads_fail_fast() }

#[cfg(not(target_os = "macos"))]
pub(crate) fn dataless_reads_fail_fast() -> bool { false }
