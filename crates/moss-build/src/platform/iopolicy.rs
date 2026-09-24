//! Process-wide dataless-file materialization policy (macOS-native; raw syscall).
//!
//! `setiopolicy_np` is declared in `<sys/resource.h>` but not exposed by the
//! `libc` crate (checked 0.2.177–0.2.182) or by objc2 — it predates the
//! File Provider APIs and is a plain BSD syscall wrapper, so this binds it
//! directly rather than pulling in a dependency for one function.
//!
//! Constants below are transcribed from
//! `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/sys/resource.h`
//! (verified against a live SDK, not guessed) and confirmed against
//! `man getiopolicy_np` and Apple technote TN3150
//! ("Getting ready for dataless files").

use std::os::raw::c_int;

const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
const IOPOL_SCOPE_PROCESS: c_int = 0;
const IOPOL_SCOPE_THREAD: c_int = 1;
const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;
const IOPOL_MATERIALIZE_DATALESS_FILES_ON: c_int = 2;

unsafe extern "C" {
    fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
}

/// Opt this process out of implicit dataless-file materialization, process-wide.
///
/// After this call, a `read`/`mmap`/`clonefile`/`copyfile(COPYFILE_ALL)` of a
/// cloud-evicted (`SF_DATALESS`) file fails immediately with `EDEADLK` instead
/// of blocking on `fileproviderd` — which can hang forever if the OS abandons
/// a download without retrying (the failure mode that motivated this design).
/// `stat`/`lstat`/xattr calls are unaffected, and so is a plain `open` — but
/// **not `open` with `O_TRUNC`**, which is what `std::fs::write`,
/// `std::fs::copy` and `File::create` all use. Truncation requires
/// materialization, so those fail `EDEADLK` against a dataless destination too.
/// This doc comment previously claimed `open` was
/// unconditionally unaffected — see `build::io_utils` for how the build tree writes
/// instead. Materialization becomes explicit, and confined to the download
/// workers, via [`materialize_on_this_thread`].
///
/// Child processes (`fork`+`exec`, `Command::spawn`) inherit this policy —
/// verified empirically, and documented by `man setiopolicy_np`: "New
/// processes inherit the policy of their parent process."
///
/// Note this does NOT protect against `NSFileCoordinator`-mediated reads
/// (`coordinateReadingItemAtURL:`) or `NSDocument` — xnu gives per-thread
/// Foundation coordination an override that beats even the process policy,
/// by design ("to make API contracts consistent"). moss uses `NSFileCoordinator`
/// in exactly one place — [`super::file_coordination`], called only by the
/// `build::cloud_prefetch` reader pool, because coordination is the documented
/// way to make a provider send a whole file rather than the 4 MiB range a POSIX
/// read asks for. Treat a coordinated read anywhere else as a bug: it reaches
/// the same per-thread override that [`materialize_on_this_thread`] grants
/// deliberately, but without opting in, so it can hang a thread that has no
/// business being lost.
///
/// Returns `false` (and logs) on any nonzero return rather than panicking:
/// Apple's own `removefile` notes that "some sandboxed processes may crash
/// when calling getiopolicy_np" (rdar://76141982) — the setter is not known
/// to share that issue, but a process-wide I/O policy is not worth crashing
/// startup over if it does.
pub fn set_dataless_fail_fast() -> bool {
    // SAFETY: `setiopolicy_np` is a plain BSD syscall wrapper (no pointers,
    // no callback, no aliasing concerns) taking three `int`s and returning an
    // `int`. All three arguments are compile-time constants verified against
    // the SDK header cited above.
    let rc = unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_PROCESS,
            IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
        )
    };
    if rc != 0 {
        log::warn!(
            "[iopolicy] setiopolicy_np(MATERIALIZE_DATALESS_FILES, PROCESS, OFF) \
             returned {rc} — dataless reads will still block on this process"
        );
        return false;
    }
    true
}

/// Opt **the calling thread** back in to dataless materialization, overriding
/// the process-wide fail-fast policy set by [`set_dataless_fail_fast`].
///
/// This is the whole download mechanism. After this call, a plain `read` of a
/// dataless file on this thread blocks while the OS fetches the bytes, and
/// returns once the file is materialized — for **any** File Provider, with no
/// provider-specific API, entitlement, or item identifier.
///
/// # Why a thread policy is the right shape
///
/// xnu checks the thread decoration *before* the process one and returns
/// unconditionally, so thread-ON beats process-OFF —
/// `vfs_context_dataless_materialization_is_prevented()` in
/// `bsd/vfs/vfs_syscalls.c`:
///
/// ```text
/// /*
///  * Per-thread decorations override any process-wide decorations.
///  * (Foundation uses this, and this overrides even the dataless-
///  * manipulation entitlement so as to make API contracts consistent.)
///  */
/// if (ut->uu_flag & UT_NSPACE_FORCEDATALESSFAULTS) { return 0; }
/// ```
///
/// That gives exactly the split moss wants: every ordinary read still fails
/// fast (a build must never block on the network), while the download workers
/// — and only they — are allowed to wait. The kernel routes every dataless
/// fault to one system-wide resolver (`filecoordinationd`), whose registration
/// is exclusive, which is why this is provider-agnostic by construction rather
/// than by a list of providers moss would have to keep current.
///
/// # The sticky bit, and why this is only ever called on threads moss owns
///
/// The policy is a sticky flag on the `uthread`, not per-syscall state: it
/// survives every later read on that thread until changed or the thread exits.
/// On a pool moss does not own (tokio's blocking pool, GCD, rayon) that would
/// leak materialization permission into whatever unrelated work runs next on
/// the same thread — silently re-arming the blocking reads the fail-fast policy
/// exists to prevent. Apple's TN3150 handles this by saving the prior value and
/// restoring it around the region. moss avoids the problem instead: the only
/// callers are the dedicated, long-lived threads in `build::cloud_prefetch`,
/// which do nothing else for their whole lifetime, so set-once at thread start
/// is both correct and simpler than save/restore.
///
/// Constants transcribed from the same SDK header as above and cross-checked
/// against `bsd/sys/resource.h`: `IOPOL_SCOPE_THREAD = 1`,
/// `IOPOL_MATERIALIZE_DATALESS_FILES_ON = 2`.
///
/// Returns `false` (and logs) if the policy could not be set — the caller then
/// still works, but its reads fail `EDEADLK` instead of downloading, which the
/// prefetch pool reports as a failed materialization rather than a hang.
pub fn materialize_on_this_thread() -> bool {
    // SAFETY: as `set_dataless_fail_fast` — a plain BSD syscall wrapper taking
    // three `int`s, all compile-time constants verified against the SDK header.
    let rc = unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_THREAD,
            IOPOL_MATERIALIZE_DATALESS_FILES_ON,
        )
    };
    if rc != 0 {
        log::warn!(
            "[iopolicy] setiopolicy_np(MATERIALIZE_DATALESS_FILES, THREAD, ON) \
             returned {rc} — this worker cannot download evicted files"
        );
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a behavioral test (no dataless file fixture in CI) — just proves
    /// the syscall binding links and returns cleanly on this machine.
    #[test]
    fn set_dataless_fail_fast_does_not_panic() {
        let _ = set_dataless_fail_fast();
    }
}
