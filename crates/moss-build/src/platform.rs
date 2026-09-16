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
