//! The per-folder build lifecycle: which tree the preview serves, when a build
//! may unlink from staging, and which render `current` holds.
//!
//! One record per `.moss` directory, and the only writer of the served pointer.
//! The rule it keeps: the preview never moves from a tree showing render r to a
//! generation older than r. A rebuild parks the preview on `current` only once
//! the last render shown on staging has been promoted there; otherwise the
//! preview stays on staging and that build unlinks nothing. The one sanctioned
//! step back is [`withdraw_render`], taken when the render on screen is itself
//! the tree that could not be read.
//!
//! Nothing here is persisted. Every render number was minted by this process,
//! and the two that get compared are `Option`s that start `None`, so a number
//! from a dead process can never be compared with one from this one.

pub(crate) mod cas_heal;
pub(crate) mod root_identity;
pub(crate) mod tree_migration;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock};

use crate::moss_paths::MossPaths;
use crate::types::runtime::SiteDirectoryState;

/// The cell the preview server reads its root from, per request.
pub(crate) type ServedCell = Arc<RwLock<PathBuf>>;

#[derive(Default)]
struct FolderLifecycle {
    /// The cell [`adopt_server`] registered.
    served: Option<ServedCell>,
    /// Last render number minted by [`show_render`].
    next_render: u64,
    /// The render on the cell while it names staging; `None` when this record
    /// did not put it there.
    shown_render: Option<u64>,
    /// The render of the generation this process last promoted.
    current_render: Option<u64>,
    /// Highest promotion epoch that reached `current`.
    promoted_epoch: u64,
    /// Open [`CacheWriteLease`]s: builds between `run_pipeline` entry and the
    /// end of the seal tail's `materialize_and_promote`.
    build_writers: usize,
    /// Open [`EncodeLease`]s: detached encodes, which outlive their build.
    encode_writers: usize,
    /// A [`CacheGcToken`] is out.
    gc_running: bool,
    /// This build's handle on `.moss/build.nosync`, opened by
    /// [`open_build_root_handle`] once at build start. `MossPaths::build_dir()`
    /// answers from it, through [`held_build_root_path`], for every
    /// `MossPaths` built from this folder's root — not just the instance that
    /// opened it — so a cloud sync client's rename-aside mid-build stops
    /// silently retargeting every accessor built on `build_dir()`. `None`
    /// before a build opens one, and forever on a platform
    /// [`root_identity::BuildRootHandle::open`] cannot support.
    build_root_handle: Option<Arc<root_identity::BuildRootHandle>>,
}

/// One folder's record. Leases, permits and seal tails hold it, which is what
/// keeps [`lock_for`] from evicting it under them.
#[derive(Default)]
pub(crate) struct LifecycleCell {
    state: Mutex<FolderLifecycle>,
    /// Signalled when a cache GC ends, for writers waiting to start.
    changed: Condvar,
    /// Serializes the `current` swap, which on Windows copies a whole
    /// generation — so it is never taken under `state`.
    promote_lock: Mutex<()>,
}

impl LifecycleCell {
    fn state(&self) -> MutexGuard<'_, FolderLifecycle> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static RECORDS: LazyLock<Mutex<HashMap<PathBuf, Arc<LifecycleCell>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The record for `mp`'s folder.
///
/// Drain on switch, like `FolderSessionRegistry`: looking up a folder with no
/// record first evicts every other record nothing else holds. A record a lease,
/// permit or seal tail still holds is kept until a later switch finds it idle —
/// evicting it would hand the next lookup a fresh record with no leases, and a
/// build would then read "nobody else is writing" while someone is.
pub(crate) fn lock_for(mp: &MossPaths) -> Arc<LifecycleCell> {
    {
        let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(record) = records.get(mp.root()) {
            return record.clone();
        }
    }
    // RECORDS is unlocked from here to the re-lock below. `cache_tmp()`
    // derives from `build_dir()`, which (via `held_build_root_path`) reads
    // this very registry for whichever folder it's asked about — nesting
    // that read under the lock above would be this thread trying to lock a
    // non-reentrant `Mutex` it already holds: a guaranteed self-deadlock, not
    // a wait. The gap this opens (two first-lookups of the same new folder
    // racing to insert) is closed by the second `records.get` below.
    //
    // The first lookup of a folder in this process comes before anything here
    // writes its scratch, so what `cache/tmp` holds is a dead process's.
    static SCRATCH_CLEARED: LazyLock<Mutex<std::collections::HashSet<PathBuf>>> =
        LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
    if SCRATCH_CLEARED.lock().unwrap_or_else(PoisonError::into_inner).insert(mp.root().to_path_buf()) {
        crate::build::io_utils::clear_scratch(&mp.cache_tmp());
    }
    let mut records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(record) = records.get(mp.root()) {
        return record.clone();
    }
    records.retain(|_, record| Arc::strong_count(record) > 1);
    let record = Arc::new(LifecycleCell::default());
    records.insert(mp.root().to_path_buf(), record.clone());
    record
}

/// Open a handle on `mp`'s `.moss/build.nosync` and make it the answer every
/// `MossPaths` for this folder's `build_dir()` gives from now on. Opens by
/// the plain joined path, deliberately not through `build_dir()` itself — a
/// build must look at whatever is actually at the canonical location right
/// now, not wherever an EARLIER build's handle (if one is still installed)
/// last resolved to.
///
/// Best-effort: a platform (or filesystem) [`root_identity::BuildRootHandle::open`]
/// cannot open falls back to today's by-path resolution, logged once so a
/// swap it then cannot see is not a silent failure.
pub(crate) fn open_build_root_handle(mp: &MossPaths) {
    let joined = mp.root().join("build.nosync");
    match root_identity::BuildRootHandle::open(&joined) {
        Ok(handle) => lock_for(mp).state().build_root_handle = Some(Arc::new(handle)),
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {}
        Err(e) => log::warn!(
            "build root handle: could not open {} — falling back to path-based resolution: {e}",
            joined.display()
        ),
    }
}

/// The build root's current path, from a handle already held for `root` (a
/// `.moss` directory) — `None` when no build for this folder has opened one
/// yet, which tells `MossPaths::build_dir()` to fall back to the joined
/// path. A plain lookup, not `lock_for`: inserting a fresh, handle-less
/// record for every folder a caller merely asks about would leak one per
/// distinct path a curious reader ever passed in.
pub(crate) fn held_build_root_path(root: &Path) -> Option<PathBuf> {
    let handle = {
        let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
        let found = records.get(root)?.state().build_root_handle.clone();
        found?
    };
    match handle.current_path() {
        Ok(path) => Some(path),
        // Once, not per call: `build_dir()` runs thousands of times a build.
        Err(e) => {
            static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log::warn!("build root handle open for {} but current_path() failed — falling back to the joined path: {e}", root.display());
            }
            None
        }
    }
}

fn point(cell: &ServedCell, to: &Path) -> bool {
    SiteDirectoryState { current_dir: cell.clone() }.switch_to(to.to_path_buf())
}

fn read(cell: &ServedCell) -> PathBuf {
    cell.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Register the preview server's cell for this folder, pointing it here only
/// when it names nothing or another folder's tree.
///
/// That is a folder switch or a cold start. A cell already on this folder's
/// build tree is left alone, so the second of two builds started a second
/// apart does not pull the preview back off the first one's render.
pub(crate) fn adopt_server(mp: &MossPaths, cell: &ServedCell) {
    let target = mp.initial_serve_dir();
    let build_dir = mp.build_dir();
    root_identity::log_build_root(&mp.root().join("build.nosync"), "serve", None);
    let record = lock_for(mp);
    let mut st = record.state();
    st.served = Some(cell.clone());
    let shown = read(cell);
    if shown.as_os_str().is_empty() || !shown.starts_with(&build_dir) {
        point(cell, &target);
        st.shown_render = None;
        log::info!("serve: adopted the preview for {} on {}", mp.project_root().display(), target.display());
    }
}

/// Proof that nothing reads the staged tree this build is about to unlink from,
/// and that no earlier build's workers are still writing it.
///
/// A detached encode outlives its build, so the permit also carries what such
/// encodes own in staging: the outputs of every video still running, and what
/// a finished run put there that no build has registered yet.
pub(crate) struct SweepPermit {
    _record: Arc<LifecycleCell>,
    encode_outputs: std::collections::HashSet<String>,
}

impl SweepPermit {
    /// Whether the sweep must keep `key` for a running or finished encode:
    /// the output itself, or a `.tmp.`/`.pending.` sibling it is being written
    /// through.
    pub(crate) fn keeps(&self, key: &str) -> bool {
        if self.encode_outputs.contains(key) {
            return true;
        }
        [".tmp.", ".pending."]
            .iter()
            .filter_map(|marker| key.rfind(marker).map(|i| &key[..i]))
            .any(|base| self.encode_outputs.contains(base))
    }

    /// Whether `dir` (relative to staging) holds an output the sweep keeps.
    pub(crate) fn keeps_dir(&self, dir: &Path) -> bool {
        self.encode_outputs.iter().any(|key| Path::new(key).starts_with(dir))
    }
}

/// The permit for a one-shot build's reclaim of its own staging, after its
/// seal: that caller drops the runtime as soon as the tail returns, so no
/// server and no later build will read staging in this process again. Minted
/// at exactly that call site (`staging_unlink_invariant_test` pins it).
pub(crate) fn final_build_permit(mp: &MossPaths) -> SweepPermit {
    SweepPermit { _record: lock_for(mp), encode_outputs: Default::default() }
}

/// A GC token for tests that drive `cache::gc` directly, on a record of their
/// own so no folder's leases are consulted.
#[cfg(test)]
pub(crate) fn gc_token_for_test() -> CacheGcToken {
    CacheGcToken { record: Arc::new(LifecycleCell::default()) }
}

/// A permit for tests that drive a sweep directly.
#[cfg(test)]
pub(crate) fn permit_for_test() -> SweepPermit {
    SweepPermit { _record: Arc::new(LifecycleCell::default()), encode_outputs: Default::default() }
}

/// Move the preview off staging for a rebuild when that is no step back, and
/// say whether this build may unlink from staging.
///
/// A permit needs both: the cell is off staging — on `current` or on another
/// tree — or it shows a render `current` already holds, in which case it moves
/// to `current` first; and no build other than the caller holds a
/// [`CacheWriteLease`], because a withheld or slow build's workers outlive its
/// render and are still writing staging. Otherwise the pointer stays where it
/// is and this build unlinks nothing. Never waits.
///
/// `encode_outputs` is `VideoConversionState::protected_outputs`, snapshotted
/// by the caller.
pub(crate) fn park_for_rebuild(
    mp: &MossPaths,
    holds_lease: bool,
    encode_outputs: std::collections::HashSet<String>,
) -> Option<SweepPermit> {
    let staging = mp.staging_dir();
    let current = mp.current_ptr();
    let current_exists = current.exists();
    let current_gen = mp.current_generation_id().unwrap_or_else(|_| "none".to_string());
    let record = lock_for(mp);
    let st = record.state();
    let others_writing = st.build_writers.saturating_sub(usize::from(holds_lease));
    let on_staging = st.served.as_ref().is_some_and(|cell| read(cell) == staging);
    let caught_up = matches!((st.shown_render, st.current_render), (Some(s), Some(c)) if c >= s);
    let render = |r: Option<u64>| r.map_or("none".to_string(), |r| r.to_string());
    if others_writing == 0 && (!on_staging || (caught_up && current_exists)) {
        if on_staging {
            if let Some(cell) = &st.served {
                point(cell, &current);
            }
        }
        log::info!(
            "serve: rebuild parked on current (gen {}, render #{}) — staging sweep permitted",
            current_gen,
            render(st.current_render)
        );
        return Some(SweepPermit { _record: record.clone(), encode_outputs });
    }
    log::info!(
        "serve: rebuild left preview on {} (shows render #{}, current gen {} is render #{}; {} earlier build(s) still writing) — no unlinks this build",
        if on_staging { "staging" } else { "its tree" },
        render(st.shown_render),
        current_gen,
        render(st.current_render),
        others_writing
    );
    None
}

/// Mint this render's number and, when it may be shown, point the preview at
/// staging. Called under the stage-write lock, once per render.
///
/// Returns the number and the directory to announce as served: staging for a
/// shown render; for a withheld one the cell and `shown_render` are left alone
/// and the answer is `current`, or nothing on a vault that has never promoted.
pub(crate) fn show_render(mp: &MossPaths, publishable: bool) -> (u64, Option<PathBuf>) {
    let staging = mp.staging_dir();
    let current = mp.current_ptr();
    let current_exists = current.exists();
    let record = lock_for(mp);
    let mut st = record.state();
    st.next_render += 1;
    let seq = st.next_render;
    if !publishable {
        return (seq, current_exists.then_some(current));
    }
    if let Some(cell) = &st.served {
        point(cell, &staging);
    }
    st.shown_render = Some(seq);
    (seq, Some(staging))
}

/// Repoint `current` at `gen_id` unless a newer build already has, and record
/// that `current` now holds `render`. `Ok(false)` is refused-as-stale.
///
/// moss#968 §5d: a `generation_id` is a content hash with no order, and seal
/// tails run detached, so build N's tail can land after build N+1's. The epoch
/// is what orders them. The swap runs under its own lock and the render is
/// recorded only after it, so a reader between the two sees the older render
/// and withholds a permit rather than granting one early.
pub(crate) fn promote(
    mp: &MossPaths,
    epoch: u64,
    render: Option<u64>,
    gen_id: &str,
) -> std::io::Result<bool> {
    let record = lock_for(mp);
    let _swap = record.promote_lock.lock().unwrap_or_else(PoisonError::into_inner);
    let promoted = record.state().promoted_epoch;
    if promoted >= epoch {
        log::info!("promotion refused: gen {} epoch {} < promoted {}", gen_id, epoch, promoted);
        return Ok(false);
    }
    mp.set_current_ptr(gen_id)?;
    let mut st = record.state();
    st.promoted_epoch = epoch;
    st.current_render = render;
    Ok(true)
}

/// Return the preview to `current` when the render on screen is `render` and
/// its generation was withheld as unreadable.
///
/// The one move to an older generation this module makes. A newer render
/// already on screen is left alone, and a vault with nothing promoted stays on
/// staging because there is nothing better to show.
pub(crate) fn withdraw_render(mp: &MossPaths, render: Option<u64>) {
    let Some(render) = render else { return };
    let current = mp.current_ptr();
    if !current.exists() {
        return;
    }
    let current_gen = mp.current_generation_id().unwrap_or_else(|_| "unknown".to_string());
    let record = lock_for(mp);
    let mut st = record.state();
    if st.shown_render != Some(render) {
        return;
    }
    if let Some(cell) = &st.served {
        point(cell, &current);
    }
    st.shown_render = None;
    log::warn!("serve: render #{} withdrawn — preview back on gen {}", render, current_gen);
}

/// A build that may still write the object store or staging: held from
/// `run_pipeline` entry through the seal tail's `materialize_and_promote`
/// (`ship_phase`) — `BackgroundHandle::await_completion` hands it back
/// instead of dropping it once the background workers have joined, and
/// `build::advertise_sealed` drops it explicitly right after, before that
/// same tail's own `collect_build_store` can trigger cache GC. Widened from
/// stopping at `await_completion`, which dropped it before the seal tail had
/// even started — letting a concurrent `collect_build_store` GC a CAS blob
/// the seal tail's own `ship_phase` still needed to read.
pub(crate) struct CacheWriteLease {
    record: Arc<LifecycleCell>,
}

/// Wait out a running cache GC — the one wait a lease can have, as long as the
/// GC walk — then count one more writer of kind `writers`.
fn enter_writer(mp: &MossPaths, writers: fn(&mut FolderLifecycle) -> &mut usize) -> Arc<LifecycleCell> {
    let record = lock_for(mp);
    let started = std::time::Instant::now();
    let mut st = record.state();
    while st.gc_running {
        st = record.changed.wait(st).unwrap_or_else(PoisonError::into_inner);
    }
    *writers(&mut st) += 1;
    drop(st);
    let waited = started.elapsed();
    if waited > std::time::Duration::from_secs(1) {
        log::warn!("cache write lease waited {} ms for a running cache GC", waited.as_millis());
    }
    record
}

pub(crate) fn cache_write_lease(mp: &MossPaths) -> CacheWriteLease {
    CacheWriteLease { record: enter_writer(mp, |st| &mut st.build_writers) }
}

impl Drop for CacheWriteLease {
    fn drop(&mut self) {
        let mut st = self.record.state();
        st.build_writers = st.build_writers.saturating_sub(1);
    }
}

/// A detached encode that may still store to the object store after its build's
/// lease is gone. Taken at dispatch, while that lease is open, so it never
/// waits; kept apart from build leases so a long encode refuses cache GC
/// without also refusing every staging sweep.
pub(crate) struct EncodeLease {
    record: Arc<LifecycleCell>,
}

pub(crate) fn encode_lease(mp: &MossPaths) -> EncodeLease {
    EncodeLease { record: enter_writer(mp, |st| &mut st.encode_writers) }
}

impl Drop for EncodeLease {
    fn drop(&mut self) {
        let mut st = self.record.state();
        st.encode_writers = st.encode_writers.saturating_sub(1);
    }
}

/// Licence to collect this folder's object store: no build or encode holds a
/// lease, and none can take one until the token drops. `cache::gc` requires
/// one, so "never during a build" is a type rather than a comment.
pub(crate) struct CacheGcToken {
    record: Arc<LifecycleCell>,
}

/// A GC token when nothing is writing the store, else the open lease counts
/// (build, encode). Never waits.
pub(crate) fn try_begin_cache_gc(mp: &MossPaths) -> Result<CacheGcToken, (usize, usize)> {
    let record = lock_for(mp);
    let mut st = record.state();
    if st.build_writers > 0 || st.encode_writers > 0 || st.gc_running {
        return Err((st.build_writers, st.encode_writers));
    }
    st.gc_running = true;
    drop(st);
    Ok(CacheGcToken { record })
}

impl Drop for CacheGcToken {
    fn drop(&mut self) {
        self.record.state().gc_running = false;
        self.record.changed.notify_all();
    }
}

/// What the record holds, for tests: (shown render, current render, open
/// build leases).
#[cfg(test)]
pub(crate) fn snapshot(mp: &MossPaths) -> (Option<u64>, Option<u64>, usize) {
    let record = lock_for(mp);
    let st = record.state();
    (st.shown_render, st.current_render, st.build_writers)
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
