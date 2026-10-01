//! A cross-PROCESS advisory lock on one folder's build.
//!
//! `FolderSession::stage_write_lock` (`system/folder_session.rs`) and
//! `build::lifecycle::promote_lock` already order a folder's staging writes
//! and its `current` swap — but both are a `Mutex` living on one process's
//! heap, so they cannot see a SECOND `moss` process (a headless CLI build
//! started while `--watch` is already running the same folder, or a
//! terminal `moss build` racing the desktop app). This is the piece that
//! reaches across that gap: one `flock`, held from before a build starts
//! writing `stage_dir` until after its generation has been promoted, so
//! that across processes the order of promotions equals the order of
//! builds.
//!
//! The lock file sits BESIDE `~/.moss/locks/`, one file per folder — never
//! inside the folder it guards, for the same reason `deploy::stack_activity`
//! keeps its lock a sibling of `~/.moss/stacks/<id>`: a lock file inside a
//! directory its own holder might remove leaves two processes each holding
//! a lock on a different unlinked inode, unable to see each other.
//!
//! `<folder-id>` is a sha256 of the folder's canonicalized path — the same
//! path-to-hex convention `types::content::symlink_entry` already uses for a
//! path-shaped content key — so two spellings of one folder (a relative vs.
//! absolute form, a trailing separator, a `..` component) always resolve to
//! the same lock file.
//!
//! ## Two windows, not one continuous hold
//!
//! [`acquire`] is taken TWICE per build, at exactly the two points
//! `FolderSession::stage_write_lock` already is: once in `build_inner`
//! (`build/pipeline.rs`), covering that build's own synchronous render span,
//! and again in `run_materialize_phase` (`build/seal_phase.rs`), covering the
//! materialize-through-`lifecycle::promote` span — released after each,
//! never held continuously across the gap between them.
//!
//! That gap can be long: the materialize phase runs detached, often behind an
//! idle debounce, so a build's render can finish while its own materialize is
//! still pending, or — in `overlap_tests.rs` — deliberately held back
//! unpolled to exercise `lifecycle::promote`'s epoch ordering with a second,
//! real build of the SAME folder in between. A single lock held from render
//! through promote was tried first and reverted: it deadlocked that
//! legitimate, already-tested overlap, because the second build's own
//! render needs the very same lock the first build's still-pending
//! materialize was holding. Two separate acquisitions close the cross-process
//! gap — a second PROCESS cannot interleave its stage_dir writes or its
//! `current_ptr` swap with this one's — without touching that in-process
//! concurrency at all.

use std::path::{Path, PathBuf};

use fs2::FileExt;

/// The held lock. Releasing is `drop`, and so is the holding process dying —
/// the kernel clears an `flock` the moment its last descriptor closes, which
/// is the whole point: a killed `moss build` must never leave the next one
/// waiting forever on a lock nobody will ever release.
#[derive(Debug)]
pub struct Held {
    file: std::fs::File,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// A stable id for `folder`, the same for every spelling that names the same
/// directory. Canonicalizes first — collapsing `.`, a trailing separator, or
/// a `..` component onto one path — then sha256-hexes the result.
///
/// Falls back to the path as given, lexically, when the folder cannot be
/// canonicalized (`std::fs::canonicalize` requires the path to already
/// exist on disk): the lock still needs something to name, and every real
/// build call site canonicalizes a folder that is already there.
///
/// `pub(crate)` rather than private: `ops::serve::ownership` names the same
/// folder for its owner record and lock, and must agree with this one on
/// what a folder is called.
pub(crate) fn folder_id(folder: &Path) -> String {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    format!("{:x}", Sha256::digest(canonical.to_string_lossy().as_bytes()))
}

/// `<moss_home>/locks/<folder-id>.build` — the lock file [`acquire`] takes
/// for `folder`. Exposed so a test (or a future `moss doctor`) can name the
/// file without acquiring it.
pub fn lock_path_for(folder: &Path) -> Result<PathBuf, String> {
    Ok(crate::infra::home::moss_home()?.join("locks").join(format!("{}.build", folder_id(folder))))
}

/// Acquire the exclusive build lock for `folder`, creating
/// `<moss_home>/locks/` on demand.
///
/// Blocks until acquired. If the lock is not immediately available, logs one
/// line at `info` the first time contention is seen — so a wait that would
/// otherwise look like a hang names its own cause — then blocks on the
/// kernel's own wait queue rather than polling: unlike
/// `deploy::stack_activity`'s bounded wait (an install has a plausible worst
/// case to time out against), a build lock has no such ceiling — the only
/// legitimate holder is another `moss` build of this SAME folder, and making
/// this one wait exactly as long as that takes is the correct behavior, not
/// a fallback.
pub fn acquire(folder: &Path) -> Result<Held, String> {
    let path = lock_path_for(folder)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    // allow:raw_write the lock file is machine state under ~/.moss/locks, not build output
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;

    match file.try_lock_exclusive() {
        Ok(()) => {}
        // fs2's WOULD_BLOCK-shaped error, not a hardcoded ErrorKind — see
        // `deploy::stack_activity::hold` for why a contended try-lock's
        // error kind is platform-dependent.
        Err(e) if e.kind() == fs2::lock_contended_error().kind() => {
            log::info!("waiting for another moss process building this folder");
            file.lock_exclusive().map_err(|e| format!("lock {}: {e}", path.display()))?;
        }
        Err(e) => return Err(format!("lock {}: {e}", path.display())),
    }
    Ok(Held { file })
}

/// [`acquire`] from an async context: the underlying `flock` call blocks a
/// thread, so this runs it on `spawn_blocking` rather than the caller's own
/// task. `None` on any failure to acquire (a `flock`/IO error, or the
/// blocking task itself panicking), logged and treated as best-effort —
/// `run_materialize_phase`'s only caller proceeds without cross-process
/// ordering for this one build rather than losing an otherwise-successful
/// promotion over it.
pub async fn acquire_async(folder: &Path) -> Option<Held> {
    let folder = folder.to_path_buf();
    match tokio::task::spawn_blocking(move || acquire(&folder)).await {
        Ok(Ok(held)) => Some(held),
        Ok(Err(e)) => {
            log::error!("could not take the cross-process folder lock, proceeding without it: {e}");
            None
        }
        Err(e) => {
            log::error!("folder-lock task panicked, proceeding without it: {e}");
            None
        }
    }
}

/// `<moss_home>/locks/<folder-id>.epoch` — where [`last_promoted_epoch`] and
/// [`record_promoted_epoch`] keep the last cross-process-visible promotion
/// time for `folder`. Sibling to the lock file itself, same [`folder_id`].
pub fn epoch_path_for(folder: &Path) -> Result<PathBuf, String> {
    Ok(crate::infra::home::moss_home()?.join("locks").join(format!("{}.epoch", folder_id(folder))))
}

/// The last promotion time recorded for `folder`, or `0` if none has been —
/// a missing file and a corrupt one both read as "nothing promoted yet",
/// the same "unreadable is never a promise" posture `.moss/deploy/` already
/// takes for the analogous local record.
///
/// **Precondition, not enforced here:** the caller must hold [`acquire`]'s
/// lock for this SAME `folder` for the whole read-compare-write span around
/// this call. This file does no locking of its own — a read racing a
/// concurrent write is exactly the cross-process ordering bug it exists to
/// close, so the exclusion has to come from outside.
pub fn last_promoted_epoch(folder: &Path) -> u64 {
    let Ok(path) = epoch_path_for(folder) else { return 0 };
    std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Record `epoch` as the last promotion time for `folder`. Same locking
/// precondition as [`last_promoted_epoch`] — see that function's doc.
pub fn record_promoted_epoch(folder: &Path, epoch: u64) -> Result<(), String> {
    crate::infra::atomic_write::write_atomic(&epoch_path_for(folder)?, &epoch.to_string())
}

#[cfg(test)]
#[path = "folder_lock_tests.rs"]
mod folder_lock_tests;
