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
//! The lock file sits in `~/.moss/locks/`, one file per folder — never
//! inside the folder it guards, for the same reason `deploy::stack_activity`
//! keeps its lock a sibling of `~/.moss/stacks/<id>`: a lock file inside a
//! directory its own holder might remove leaves two processes each holding
//! a lock on a different unlinked inode, unable to see each other.
//!
//! The file lives only while it is held. The holder unlinks it before
//! unlocking, and an acquirer checks, once it holds a lock, that the path
//! still names the file it locked, opening again if not, as often as it
//! takes. A waiter that was blocked on the removed file therefore moves to
//! the new one instead of holding a lock nobody else can see. So `~/.moss/locks`
//! holds only what is held now, however many temporary folders are built.
//! On Windows, where an open file cannot be replaced this way, the file
//! stays.
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
//! A one-shot build (`moss build`, `moss deploy`) keeps
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
    path: PathBuf,
}

impl Drop for Held {
    fn drop(&mut self) {
        // Unlinked while still held, so no process can open it after this
        // and hold a lock on a file the next acquirer never sees.
        #[cfg(unix)]
        // allow:unlink the lock file this process holds, under ~/.moss/locks
        let _ = std::fs::remove_file(&self.path);
        let _ = self.file.unlock();
    }
}

/// Whether `path` still names the open `file`. The previous holder unlinks
/// the lock file on release, so a lock taken on a file that is gone, or was
/// replaced, excludes nobody.
#[cfg(unix)]
fn still_named(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(held), Ok(named)) => held.dev() == named.dev() && held.ino() == named.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn still_named(_file: &std::fs::File, _path: &Path) -> bool {
    true
}

use crate::infra::folder_store::name_for;

/// `<moss_home>/locks/<folder-id>.<name>` — one lock file per (folder, name),
/// so a different job on the same folder can hold its own lock without waiting
/// on the build's.
pub fn lock_path_named(folder: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(crate::infra::home::moss_home()?.join("locks").join(format!("{}.{name}", name_for(folder))))
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
    acquire_named_noting_wait(folder, name, || {})
}

/// [`acquire_named`] that calls `on_wait` once, before blocking, when another
/// process or thread holds the lock, so a caller can tell its user why
/// nothing is happening.
pub fn acquire_named_noting_wait(folder: &Path, name: &str, on_wait: impl FnOnce()) -> Result<Held, String> {
    let path = lock_path_named(folder, name)?;
    let mut on_wait = Some(on_wait);
    let held = hold(&path, |file| {
        if let Some(on_wait) = on_wait.take() {
            if name == "build" {
                log::info!("waiting for another moss process building this folder");
            } else {
                log::info!("waiting for another moss process holding the {name} lock on this folder");
            }
            on_wait();
        }
        let waited = std::time::Instant::now();
        file.lock().map_err(|e| format!("lock {}: {e}", path.display()))?;
        log::info!("waited {:?} for the {name} lock on this folder", waited.elapsed());
        Ok(true)
    })?;
    Ok(held.expect("a blocking acquire always ends holding the lock"))
}

/// The lock named `name` on `folder` if nobody holds it, `None` if someone
/// does. Never waits.
pub fn try_acquire_named(folder: &Path, name: &str) -> Result<Option<Held>, String> {
    hold(&lock_path_named(folder, name)?, |_| Ok(false))
}

const PUBLISH_PIN: &str = "pin-";

/// A lock held for as long as one publish keeps a generation of `folder`
/// pinned. Each publish takes a name of its own, so publishes never wait on
/// one another, and [`publish_pinned`] sees them all.
pub fn hold_publish_pin(folder: &Path) -> Result<Held, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!("{PUBLISH_PIN}{}-{n}", std::process::id());
    try_acquire_named(folder, &name)?.ok_or_else(|| format!("the {name} lock is already held"))
}

/// Whether a publish, in any process, holds a generation of `folder` pinned.
/// A pin file nobody holds, left by a killed publish, is taken and so removed.
pub fn publish_pinned(folder: &Path) -> bool {
    let Ok(lock) = lock_path_named(folder, PUBLISH_PIN) else { return false };
    let (Some(dir), Some(prefix)) = (lock.parent(), lock.file_name().and_then(|n| n.to_str())) else { return false };
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(prefix)) else { return false };
        !matches!(try_acquire_named(folder, &format!("{PUBLISH_PIN}{rest}")), Ok(Some(_)))
    })
}

/// Opens and locks `path` until the file it locked is still the one at
/// `path`. When another holder has the lock, `contended` either waits for it
/// (`true`) or gives up (`false`, so `None`).
///
/// There is no cap on reopening: a lock file stops being named only when
/// its holder released it, so every reopen follows someone else's progress.
/// A run of reopens without end can only mean a file system whose file
/// identity is unstable: after a thousand it is logged once, and each further
/// reopen waits 10 ms so it costs no CPU, rather than failing the lock.
fn hold(path: &Path, mut contended: impl FnMut(&std::fs::File) -> Result<bool, String>) -> Result<Option<Held>, String> {
    let mut reopens = 0u32;
    loop {
        let file = open_lock_file(path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                if !contended(&file)? {
                    return Ok(None);
                }
            }
            Err(e) => return Err(format!("lock {}: {e}", path.display())),
        }
        if still_named(&file, path) {
            return Ok(Some(Held { file, path: path.to_path_buf() }));
        }
        reopens = reopens.saturating_add(1);
        if reopens == 1000 {
            log::warn!("lock {}: reopened {reopens} times, still not the file at that path", path.display());
        }
        if reopens >= 1000 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Remove, on a background thread, unheld job locks in `<moss_home>/locks`.
/// Owner locks are left for the next owner, so collection cannot contend with
/// nonblocking admission. Since a released lock unlinks itself, an unheld file there
/// is one a moss that predates that left behind, or an old `.epoch` record:
/// litter, however many there are. A held lock fails the `flock` and stays.
/// The directory is its own progress record, so a sweep cut off by the
/// process exiting resumes on the next one; once the litter is gone a sweep
/// costs one short directory read. One sweep at a time per process.
pub fn spawn_litter_sweep() {
    static SWEEPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Ok(dir) = crate::infra::home::moss_home().map(|home| home.join("locks")) else { return };
    if SWEEPING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new().name("moss-lock-sweep".into()).spawn(move || {
        #[cfg(target_os = "macos")]
        crate::infra::qos::set_background(true);
        let removed = sweep_litter(&dir, SWEEP_BATCH, SWEEP_PAUSE);
        if removed > 0 {
            log::info!("removed {removed} stale lock files from {}", dir.display());
        }
        SWEEPING.store(false, std::sync::atomic::Ordering::Release);
    });
    if spawned.is_err() {
        SWEEPING.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Files unlinked between pauses. Each unlink is an event the file-system
/// indexers (`fseventsd`, Spotlight) process, and a pile of a million removed
/// in one go keeps them busy for minutes.
const SWEEP_BATCH: usize = 256;
const SWEEP_PAUSE: std::time::Duration = std::time::Duration::from_secs(1);

/// [`spawn_litter_sweep`]'s pass over `dir`, returning how many files it
/// removed. Streams the directory: it never lists every name at once.
#[cfg(unix)]
fn sweep_litter(dir: &Path, batch: usize, pause: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        // Ownership admission is nonblocking: even the sweep's brief lock
        // would look like another server. A crashed owner's file is reclaimed
        // when that folder is next claimed, then unlinked by its guard.
        if path.extension().is_some_and(|ext| ext == "owner") {
            continue;
        }
        // allow:raw_write opens an existing lock file under ~/.moss/locks to lock it, never creates one
        let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else { continue };
        if file.try_lock().is_err() || !still_named(&file, &path) {
            continue;
        }
        // The release path: unlinked while held, so an acquirer that opened
        // it meanwhile finds it gone and opens a new one.
        drop(Held { file, path });
        removed += 1;
        if removed % batch == 0 {
            std::thread::sleep(pause);
        }
    }
    removed
}

/// A file stays open while it is locked on Windows, so a lock file there is
/// never removed and there is nothing to sweep.
#[cfg(not(unix))]
fn sweep_litter(_dir: &Path, _batch: usize, _pause: std::time::Duration) -> usize {
    0
}

fn open_lock_file(path: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    // allow:raw_write the lock file is machine state under ~/.moss/locks, not build output
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))
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

/// Where generation `id`'s promotion record is kept, beside it: the
/// `locked_at` stamp of the build that last promoted it. Written before the
/// flip of `current`, so the flip carries its epoch with it. A crash between
/// the write and the flip leaves a record on a generation `current` does not
/// name, which nothing reads.
pub fn promotion_record_path(generations: &Path, id: &str) -> PathBuf {
    generations.join(format!(".{id}.promoted.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PromotionRecord {
    locked_at: u64,
}

/// The `locked_at` of the build that promoted the generation `current` names,
/// or `0` when nothing is promoted, or that generation has no readable record
/// (it was promoted by a moss that kept none, or its record was purged). A
/// build compared against `0` promotes over it.
///
/// **Precondition, not enforced here:** the caller must hold [`acquire`]'s
/// lock for this SAME `folder` for the whole read-compare-write span around
/// this call. This file does no locking of its own — a read racing a
/// concurrent write is exactly the cross-process ordering bug it exists to
/// close, so the exclusion has to come from outside.
pub fn last_promoted_epoch(folder: &Path) -> u64 {
    let mp = crate::moss_paths::MossPaths::new(folder);
    let Ok(id) = mp.current_generation_id() else { return 0 };
    std::fs::read(promotion_record_path(&mp.generations_dir(), &id))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PromotionRecord>(&bytes).ok())
        .map_or(0, |record| record.locked_at)
}

/// Record that the build stamped `locked_at` is promoting generation `id`.
/// Same locking precondition as [`last_promoted_epoch`].
pub fn record_promotion(generations: &Path, id: &str, locked_at: u64) -> Result<(), String> {
    let json = serde_json::to_string(&PromotionRecord { locked_at }).map_err(|e| e.to_string())?;
    crate::infra::atomic_write::write_atomic(&promotion_record_path(generations, id), &json)
}

#[cfg(test)]
#[path = "folder_lock_tests.rs"]
mod folder_lock_tests;
