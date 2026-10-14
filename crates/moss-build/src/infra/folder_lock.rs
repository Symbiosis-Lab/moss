//! A cross-PROCESS advisory lock on one folder's build.
//!
//! `FolderSession::stage_write_lock` (`system/folder_session.rs`) and
//! `build::lifecycle::promote_lock` already order a folder's staging writes
//! and its `current` swap — but both are a `Mutex` living on one process's
//! heap, so they cannot see a SECOND `moss` process (a headless CLI build
//! started while `--watch` is already running the same folder, or a
//! terminal `moss build` racing the desktop app). This is the piece that
//! reaches across that gap: one `flock`, held while a build renders into
//! `stage_dir` and while it promotes a generation, so that no two processes
//! render or promote one folder at the same time. The stamp a build takes
//! right after acquiring it (`locked_at_nanos`, minted in `build_inner`) is
//! what a promotion is compared by, so across processes the order of
//! promotions equals the order of renders.
//!
//! It does not cover everything a build writes into `stage_dir`. A
//! long-lived runtime's background encoders, and its seal's repair pass and
//! `hashes.json` write (`advertise_sealed`), run after its render without
//! this lock; they are ordered against that process's own builds only, by
//! the stage-write guard.
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
//! ## One hold for a one-shot build, two windows for a long-lived one
//!
//! [`acquire`] is taken in `build_inner` (`build/pipeline.rs`), before the
//! render writes `stage_dir`. What happens after the render depends on the
//! runtime.
//!
//! A one-shot build (`moss build`, `moss deploy`, `moss history --save`) keeps
//! holding it through its seal and its promotion, and `run_materialize_phase`
//! (`build/seal_phase.rs`) releases it once `current` has moved. That whole
//! span reads and deletes from `stage_dir`: the final sweep removes whatever
//! this build's manifest does not name, and the ship reads what the render
//! wrote. Released at the end of the render, the lock let a second process
//! render into that same span. Each build's sweep then deleted the other's
//! stylesheet (its name is its content hash, so two builds with different
//! stylesheets name different files), and the first build's seal saw pages
//! naming a stylesheet it never wrote: its publish refused, or a generation
//! without its stylesheet was promoted.
//!
//! A long-lived runtime (the app, `moss build --serve --watch`) releases it
//! at the end of the render and takes it again, freshly, in
//! `run_materialize_phase`, never holding it across the gap between them.
//! That gap can be long: the materialize phase runs detached, often behind an
//! idle debounce, so a build's render can finish while its own materialize is
//! still pending, or — in `overlap_tests.rs` — deliberately held back
//! unpolled to exercise `lifecycle::promote`'s epoch ordering with a second,
//! real build of the SAME folder in between. A single lock held from render
//! through promote was tried first and reverted: it deadlocked that
//! legitimate, already-tested overlap, because the second build's own
//! render needs the very same lock the first build's still-pending
//! materialize was holding. A one-shot build cannot deadlock that way: its
//! materialize runs inline through `seal_phase::run_now`, never behind a
//! debounce or another build, and every path takes this lock before the
//! stage-write guard, including a materialize that has to wait for it.
//!
//! The price of the one-shot hold falls on whoever waits for it. Another
//! process's render, and a long-lived process's pending materialize, wait
//! out a terminal build's whole span, video encoding included. A folder
//! switch in the app awaits its pending materialize with no bound, and a
//! headless watch rebuild that waits past 300 s is reported overdue (only
//! reported, never aborted). The wait is logged, with its length, when it
//! ends.

use std::path::{Path, PathBuf};

use fs2::FileExt;

/// The held lock. Releasing is `drop`, and so is the holding process dying —
/// the kernel clears an `flock` the moment its last descriptor closes, which
/// is the whole point: a killed `moss build` must never leave the next one
/// waiting forever on a lock nobody will ever release.
///
/// Not re-entrant: each [`acquire`] opens the file anew, and a second open
/// contends with the first even inside one process, so taking the lock again
/// while this is held blocks forever. Hand the `Held` on instead.
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
    lock_path_named(folder, "build")
}

/// `<moss_home>/locks/<folder-id>.<name>` — one lock file per (folder, name),
/// so a different job on the same folder can hold its own lock without waiting
/// on the build's.
pub fn lock_path_named(folder: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(crate::infra::home::moss_home()?.join("locks").join(format!("{}.{name}", folder_id(folder))))
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
    acquire_named(folder, "build")
}

/// [`acquire`] for a lock other than the build's, keyed by `name` (for example
/// `"moderation"`, held only for the instant a comment hide is signed and
/// appended, which must never wait on a build that holds the build lock for
/// minutes).
pub fn acquire_named(folder: &Path, name: &str) -> Result<Held, String> {
    let path = lock_path_named(folder, name)?;
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
            if name == "build" {
                log::info!("waiting for another moss process building this folder");
            } else {
                log::info!("waiting for another moss process holding the {name} lock on this folder");
            }
            let waited = std::time::Instant::now();
            file.lock_exclusive().map_err(|e| format!("lock {}: {e}", path.display()))?;
            log::info!("waited {:?} for the {name} lock on this folder", waited.elapsed());
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
