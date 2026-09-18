//! The per-folder rebuild worker: a capacity-1 request slot drained by one
//! dedicated task per watched folder.
//!
//! Phase 1a of `docs/archive/2026-08-18-watcher-reliability-architecture.md`
//! ("Piece 2 — a rebuild path that cannot wedge"). Before this, every producer
//! of a rebuild — the watcher's select loop, the target reconciler, the cloud
//! supervisor, the publish thaw — awaited the ENTIRE build inline through
//! `trigger_rebuild_with_lock`, so one wedged build parked event processing,
//! reconciliation and shutdown for the life of the process. Now producers
//! enqueue and return; the worker builds one at a time.
//!
//! ## The primitive
//!
//! A `Mutex<Option<RebuildRequest>>` slot plus a `tokio::sync::Notify`, chosen
//! over `mpsc::channel(1)` because the slot must support MERGE on conflict:
//! requests carry `(rename_pairs, BuildTrigger)`, and a second request landing
//! while the slot is occupied has to coalesce with the occupant rather than be
//! dropped (`try_send` would drop the payload silently, and merging outside
//! the channel reopens a check-then-act gap). With the mutex, take-vs-enqueue
//! is a single critical section.
//!
//! ## Why no request can be lost
//!
//! Every slot mutation happens under the mutex and is followed by
//! `Notify::notify_one`, which stores a permit when no worker is waiting. The
//! worker only ever sleeps in `notified()`, and re-reads the slot after every
//! wake — so an enqueue that lands between the worker's empty `take()` and its
//! `notified().await` leaves a permit that completes the wait immediately.
//! There is no window in which a filled slot coexists with a worker that will
//! sleep without a pending wake.

use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use futures::FutureExt;

use crate::build::BuildTrigger;

/// How long the worker waits before re-trying admission when the folder's
/// stage-write lock is held (design doc, Piece 2: "Re-admission is try-lock,
/// not pile-up … it retries next tick"). The common holder is a prior
/// generation's seal tail, which releases without poking the slot — so the
/// retry must be timer-driven, not poke-driven, or a parked request could
/// outlive the contention that parked it.
pub const LOCK_RETRY_TICK: Duration = Duration::from_millis(500);

/// First delay after a failed build; doubles per consecutive failure.
pub const BACKOFF_BASE: Duration = Duration::from_secs(2);

/// Ceiling for the failed-build backoff (design doc: `min(2^n × interval,
/// cap)`). A deterministic failure — bad frontmatter, disk full — retries at
/// this cadence forever instead of spinning the CPU at event speed.
pub const BACKOFF_CAP: Duration = Duration::from_secs(64);

/// How long a rebuild may run before the watchdog calls it overdue. Purely
/// diagnostic — see [`OverdueWatchdog`].
pub const REBUILD_OVERDUE_AFTER: Duration = Duration::from_secs(300);

/// Delay before honoring the slot again after the `n`-th consecutive build
/// failure (`n >= 1`): 2s, 4s, 8s, … capped at [`BACKOFF_CAP`].
pub fn backoff_delay(consecutive_failures: u32) -> Duration {
    let shift = consecutive_failures.saturating_sub(1).min(31);
    BACKOFF_BASE
        .checked_mul(1u32.checked_shl(shift).unwrap_or(u32::MAX))
        .unwrap_or(BACKOFF_CAP)
        .min(BACKOFF_CAP)
}

/// One coalesced "make the screen match the disk" request.
///
/// Nearly idempotent — the payload only *narrows* what the rebuild does
/// (rename pairs feed the refresh diff; a `ContentOnly` trigger allows
/// incremental skips), so merging two requests may only widen, never drop.
#[derive(Debug, Clone, PartialEq)]
pub struct RebuildRequest {
    /// Source-relative rename pairs for the rebuild event's
    /// `moved_output_paths` — how the preview follows a renamed page.
    pub rename_pairs: Vec<(String, String)>,
    /// What the producing batch looked like — see `BuildTrigger`.
    pub trigger: BuildTrigger,
    /// `Some(paths)` when EVERY event behind this request was a gate-eligible
    /// `Modify` (`PumpGate::DeferHashCheck`): the worker runs the content-hash
    /// gate over these paths at admission and disposes of the request when
    /// nothing changed — the pump no longer reads files (phase 1b). `None`
    /// means at least one producer needs the rebuild regardless (create/
    /// remove, lost-events full scan, reconciler, cloud supervisor, thaw).
    pub gate_paths: Option<Vec<std::path::PathBuf>>,
}

impl RebuildRequest {
    pub fn full() -> Self {
        RebuildRequest {
            rename_pairs: Vec::new(),
            trigger: BuildTrigger::Full,
            gate_paths: None,
        }
    }

    /// Run the deferred content-hash gate: `false` means every producing path
    /// is byte-identical to what the last build recorded, so this request may
    /// be dropped. `true` for a request that never deferred (`gate_paths:
    /// None`) and on any error — the gate fails open, including on a panic
    /// inside the check.
    ///
    /// The stat+SHA-256 runs on the blocking pool via plain tokio, not
    /// `tauri::async_runtime`: this runs on whichever runtime the caller
    /// blocks on (the app's tauri runtime, or headless's own tokio runtime),
    /// and `spawn_blocking` from inside a runtime lands on that same one.
    ///
    /// One owner deliberately. The app shell and the headless watcher each
    /// carried a verbatim copy in both their inline and admission arms, and on
    /// 2026-08-31 the two app copies were promoted to `info` while the two
    /// headless copies stayed `debug` — the same gate reporting itself
    /// differently depending on which binary ran it.
    ///
    /// `at` names the arm in the log line ("at admission", "inline"). INFO,
    /// not DEBUG: production logs are INFO+, and a `Send Logs` upload showing
    /// a trigger with no build is the only way to tell a duplicate FS event
    /// the gate absorbed from one it failed to absorb. Diagnosing the
    /// 2026-08-31 rebuild-pairs report stalled on exactly that missing line.
    pub async fn gate_admits(&self, folder_path: &str, at: &str) -> bool {
        let Some(gate_paths) = self.gate_paths.as_ref() else {
            return true;
        };
        let baseline = crate::system::build_records::records().content_hashes(folder_path);
        let folder = folder_path.to_string();
        let paths = gate_paths.clone();
        let count = paths.len();
        let proceed = tokio::task::spawn_blocking(move || {
            crate::build::watch::should_rebuild_for_paths(&folder, &paths, baseline.as_ref())
        })
        .await
        .unwrap_or(true);
        if !proceed {
            log::info!(
                target: "moss::build::watch",
                "Gated {at}: all {count} path(s) unchanged vs. manifest"
            );
        }
        proceed
    }
}

/// Coalesce two requests into one.
///
/// The trigger is merged by [`BuildTrigger::merge`], NOT degraded to `Full`.
/// This used to return `..RebuildRequest::full()` unconditionally, reasoning
/// that two batches never classified together have no narrower honest trigger.
/// Wrong in one direction — the gates are predicates over the path set, so the
/// union of two content-only batches carries exactly the evidence one batch
/// holding both paths would have. It was also the dominant rebuild on a busy
/// vault: a full reparse and 223-page render where one markdown file changed.
///
/// Rename pairs are still dropped in favor of the manifest diff surfacing the
/// structural change, and the admission gate still only widens: the union is
/// suppressible iff BOTH sides were (their path sets union); one unconditional
/// side makes the whole request unconditional. Suppress-when-unsure would eat
/// a real change.
pub fn merge_requests(occupant: RebuildRequest, incoming: RebuildRequest) -> RebuildRequest {
    let gate_paths = match (occupant.gate_paths, incoming.gate_paths) {
        (Some(mut a), Some(b)) => {
            for p in b {
                if !a.contains(&p) {
                    a.push(p);
                }
            }
            Some(a)
        }
        _ => None,
    };
    RebuildRequest {
        gate_paths,
        trigger: occupant.trigger.merge(incoming.trigger),
        rename_pairs: Vec::new(),
    }
}

/// Shared handle to one folder's request slot. Producers enqueue/poke through
/// it; the worker loop drains it; `start_file_watching` owns its lifecycle.
pub struct WorkerHandle {
    slot: Mutex<Option<RebuildRequest>>,
    /// Wakes the worker. `notify_one` stores a permit when the worker is not
    /// currently waiting, which is what closes the check-then-act gap — see
    /// the module doc.
    work: tokio::sync::Notify,
    /// The folder's build is overdue (set by [`OverdueWatchdog`], cleared when
    /// the build finally finishes). Observability only — nothing gates on it.
    degraded: std::sync::atomic::AtomicBool,
    /// An admission attempt is in flight right now. Observability for the
    /// sweep, which skips its stat walk while a build runs (the design's
    /// staleness bound assumes it: comparing disk against a baseline mid-
    /// replacement would race the very thing it verifies). Never a
    /// coordination mechanism — a stale read costs one deferred walk.
    building: std::sync::atomic::AtomicBool,
    /// Bumped by every producer-side wake (enqueue, poke, shutdown) BEFORE its
    /// `notify_one`. Lets a deferral wait distinguish a FRESH wake from the
    /// stale permit its own request's enqueue left behind — `Notify` stores a
    /// permit when nobody is waiting, and without this the first backoff /
    /// lock-retry wait after a busy spell would end instantly and for free.
    wake_seq: std::sync::atomic::AtomicU64,
    shutdown: std::sync::atomic::AtomicBool,
    /// Finish time and wall duration of the most recently COMPLETED admission
    /// — the sweep's measured-from-finish pacing input (phase 3): its
    /// arrival-driven rebuild interval is `max(floor, last build duration)`,
    /// and before this signal existed it could only measure its own enqueue
    /// (milliseconds), leaving the floor to do all the pacing. Observability
    /// only, like `building` — never a coordination mechanism.
    last_completed: Mutex<Option<(std::time::Instant, Duration)>>,
}

impl WorkerHandle {
    fn new() -> Self {
        WorkerHandle {
            slot: Mutex::new(None),
            work: tokio::sync::Notify::new(),
            degraded: std::sync::atomic::AtomicBool::new(false),
            building: std::sync::atomic::AtomicBool::new(false),
            wake_seq: std::sync::atomic::AtomicU64::new(0),
            shutdown: std::sync::atomic::AtomicBool::new(false),
            last_completed: Mutex::new(None),
        }
    }

    /// Record a completed admission (build ran, or the gate disposed of the
    /// request) — called by the worker loop, read by the sweep's pacing.
    fn record_completion(&self, started: std::time::Instant) {
        let finished = std::time::Instant::now();
        let mut slot = self
            .last_completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some((finished, finished.duration_since(started)));
    }

    /// When the last admission completed, and how long it ran. `None` until
    /// the folder's first completed build.
    pub fn last_completed(&self) -> Option<(std::time::Instant, Duration)> {
        *self
            .last_completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Is the in-flight build past its deadline? (Diagnostics/UX surface;
    /// never a coordination mechanism.) Read by the sweep every tick and
    /// surfaced to the preview as `FolderHealthChanged` — a corner-panel
    /// advisory (moss#1075).
    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::SeqCst)
    }

    fn set_degraded(&self, v: bool) {
        self.degraded.store(v, Ordering::SeqCst);
    }

    /// Current wake sequence — see the field doc. Snapshot it at dequeue time;
    /// a later mismatch means a producer woke the worker since then.
    fn wake_seq(&self) -> u64 {
        self.wake_seq.load(Ordering::SeqCst)
    }

    fn bump_wake_seq(&self) {
        self.wake_seq.fetch_add(1, Ordering::SeqCst);
    }

    fn lock_slot(&self) -> std::sync::MutexGuard<'_, Option<RebuildRequest>> {
        self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Deposit a request, merging with any occupant, and wake the worker.
    pub fn enqueue(&self, mut req: RebuildRequest) {
        {
            let mut slot = self.lock_slot();
            // A build can copy a source file early, then compute its final
            // content-hash stash after a newer save lands. In that ordering
            // the stash describes the newer source while the promoted output
            // still contains the older bytes. The catch-up request must not
            // be gated against that mixed baseline or it will be discarded.
            // `take_for_attempt` sets `building` under this same slot lock, so
            // there is no dequeue-to-admission window where an arrival can
            // retain a suppressible gate.
            if self.building.load(Ordering::SeqCst) {
                req.gate_paths = None;
            }
            *slot = Some(match slot.take() {
                Some(occupant) => merge_requests(occupant, req),
                None => req,
            });
        }
        self.bump_wake_seq();
        self.work.notify_one();
    }

    /// Remove the queued request, if any.
    pub fn take(&self) -> Option<RebuildRequest> {
        self.lock_slot().take()
    }

    /// Dequeue the next request and mark its admission in flight as one
    /// atomic slot operation. Producers use that mark to make arrivals during
    /// the attempt unconditional; see [`enqueue`](Self::enqueue).
    fn take_for_attempt(&self) -> Option<RebuildRequest> {
        let mut slot = self.lock_slot();
        let req = slot.take();
        if req.is_some() {
            self.building.store(true, Ordering::SeqCst);
        }
        req
    }

    /// Return a dequeued-but-not-admitted request to the slot (the freeze
    /// path). Anything that arrived in between merges with it; the request is
    /// never lost. Does NOT wake the worker — the caller is the worker, and it
    /// goes back to waiting for a poke.
    pub fn restore(&self, req: RebuildRequest) {
        let mut slot = self.lock_slot();
        *slot = Some(match slot.take() {
            Some(newer) => merge_requests(req, newer),
            None => req,
        });
    }

    /// [`restore`](Self::restore) plus re-raising the caller's derived
    /// "request queued" flag — the lock-busy and failed-build parks.
    ///
    /// Admission clears `rebuild_pending` at dequeue, and
    /// `deploy::wait_for_rebuild_quiet` reads only `is_rebuilding ||
    /// rebuild_pending` — so a restore that does NOT re-raise the flag makes
    /// a parked request invisible to a publish, which then uploads output
    /// missing the user's last save. Lock-busy recurs after EVERY build (the
    /// seal tail holds the stage lock briefly), so this is the common case,
    /// not a corner. The flag is stored BEFORE the restore so no dequeue can
    /// interleave and clear it out from under the park.
    ///
    /// The publish-FREEZE park deliberately stays on plain `restore`: its
    /// request must be invisible to the quiet-wait, or a deploy waiting
    /// inside its own freeze would stall on the request the freeze itself
    /// parked (see `attempt_admitted_rebuild`).
    pub fn restore_visibly(
        &self,
        pending: Option<&std::sync::atomic::AtomicBool>,
        req: RebuildRequest,
    ) {
        if let Some(p) = pending {
            p.store(true, Ordering::SeqCst);
        }
        self.restore(req);
    }

    /// Is a request queued right now? Observability only — the sweep reads
    /// it to defer its walk while a rebuild is already owed (walking then
    /// could only re-discover the same drift, and its enqueue's poke would
    /// cut the failed-build backoff short, turning a deterministic failure
    /// into a walk-cadence retry loop). Never the coordination mechanism.
    pub fn slot_occupied(&self) -> bool {
        self.lock_slot().is_some()
    }

    /// Is an admission attempt in flight? See the `building` field.
    pub fn is_building(&self) -> bool {
        self.building.load(Ordering::SeqCst)
    }

    /// Wake the worker to re-check the slot (the publish thaw's re-admission
    /// signal). Idempotent and cheap when the slot is empty.
    pub fn poke(&self) {
        self.bump_wake_seq();
        self.work.notify_one();
    }

    /// Ask the worker to exit. It finishes any in-flight build first — the
    /// build runs on `spawn_blocking` and is unkillable anyway — and drops any
    /// still-queued request, which is correct at folder close.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.bump_wake_seq();
        self.work.notify_one();
    }

    fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

/// What one admission attempt did with its dequeued request. The variants
/// that defer (`Parked`, `LockBusy`, `Failed`) all promise the request was
/// RESTORED to the slot first — the loop only decides how long to wait, never
/// whether the request survives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttemptOutcome {
    /// A build ran to success, or the request was legitimately disposed of
    /// (admission-time gate said nothing changed). Take the next request
    /// immediately.
    Completed,
    /// Deferred with an external wake guaranteed (the publish freeze — the
    /// thaw pokes every slot). Park until poked.
    Parked,
    /// The stage-write lock was held (a prior generation's seal tail, or a
    /// wedged build). Retry after [`LOCK_RETRY_TICK`] — the holder releases
    /// without poking — or earlier on a poke.
    LockBusy,
    /// The build ran and failed. Retry after [`backoff_delay`], or earlier
    /// when a new change event lands in the slot (its enqueue pokes) — a
    /// failing site must not spin, a fixed site must rebuild promptly.
    Failed,
}

/// The worker loop. Generic over the admission attempt so it is testable
/// without an `AppHandle` or a real build.
///
/// `attempt` receives a dequeued request and reports an [`AttemptOutcome`];
/// the loop owns the retry policy: park on `Parked`, timer-retry on
/// `LockBusy`, exponential backoff on `Failed` (one counter, reset on the
/// next `Completed` — design doc, Piece 2, "Failed-build backoff"). Every
/// wait also completes on a poke, so no deferral outlives a new trigger.
pub async fn run_worker_loop<Fut>(
    handle: Arc<WorkerHandle>,
    mut attempt: impl FnMut(RebuildRequest) -> Fut,
) where
    Fut: std::future::Future<Output = AttemptOutcome>,
{
    let mut consecutive_failures: u32 = 0;
    loop {
        if handle.shutdown_requested() {
            break;
        }
        match handle.take_for_attempt() {
            None => handle.work.notified().await,
            Some(req) => {
                // Snapshot BEFORE the attempt: any producer wake from here on
                // (a new change event, a thaw poke) is fresh and may cut a
                // deferral wait short; the permit our own request's enqueue
                // left behind is not.
                let seq_at_dequeue = handle.wake_seq();
                // PANIC ISOLATION: an unwind out of the attempt would kill
                // this task while the registry still hands out the handle —
                // every later enqueue would land in a slot nobody drains, a
                // permanent silent dead preview (the exact failure this
                // worker exists to prevent). Catch it, put the request back
                // (a double-restore, if the attempt restored before
                // panicking, merges harmlessly), and let backoff absorb it.
                let backup = req.clone();
                let started = std::time::Instant::now();
                let outcome = match std::panic::AssertUnwindSafe(attempt(req))
                    .catch_unwind()
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(_) => {
                        log::error!(
                            target: "moss::build::watch",
                            "Rebuild admission attempt PANICKED — restoring the request; the worker stays up and backs off"
                        );
                        handle.restore(backup);
                        AttemptOutcome::Failed
                    }
                };
                handle.building.store(false, Ordering::SeqCst);
                match outcome {
                    AttemptOutcome::Completed => {
                        consecutive_failures = 0;
                        handle.record_completion(started);
                    }
                    AttemptOutcome::Parked => handle.work.notified().await,
                    AttemptOutcome::LockBusy => {
                        wait_for_wake_or(&handle, LOCK_RETRY_TICK, seq_at_dequeue).await
                    }
                    AttemptOutcome::Failed => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        let delay = backoff_delay(consecutive_failures);
                        log::warn!(
                            target: "moss::build::watch",
                            "Build failed ({} in a row) — next attempt in {:?} (or on a new change)",
                            consecutive_failures,
                            delay
                        );
                        wait_for_wake_or(&handle, delay, seq_at_dequeue).await;
                    }
                }
            }
        }
    }
}

/// Wait up to `delay`, returning early only on a FRESH producer wake (one
/// whose `wake_seq` bump postdates `seq_at_dequeue`) — the "a fixed site
/// rebuilds promptly" half of the backoff rule. A stale stored permit is
/// consumed and the wait continues, so a busy spell cannot buy the next
/// failure a free instant retry.
async fn wait_for_wake_or(handle: &WorkerHandle, delay: Duration, seq_at_dequeue: u64) {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if handle.wake_seq() != seq_at_dequeue || handle.shutdown_requested() {
            return;
        }
        tokio::select! {
            _ = handle.work.notified() => {}
            _ = tokio::time::sleep_until(deadline) => return,
        }
    }
}

// ---------------------------------------------------------------------------
// Overdue watchdog: timeout = diagnostics, never an escape hatch
// ---------------------------------------------------------------------------

/// Armed around each admitted build. If the build outlives `deadline`, ONE
/// ERROR names the folder and the build's age, and the handle goes degraded —
/// and that is all. Nothing is aborted, cancelled, or released by timer: the
/// design doc's v2 timeout ("release the lock, next build proceeds") was
/// unsound twice over — the wedge holds the stage-write lock inside
/// `spawn_blocking` (unkillable; each retry would park another blocking
/// thread behind it), and a zombie that un-wedged after its successor would
/// promote stale output (that half is closed by admission-time epochs).
/// Dropping the watchdog (build finished, or panicked) disarms it and clears
/// the degraded state.
pub struct OverdueWatchdog {
    task: tokio::task::JoinHandle<()>,
    handle: Arc<WorkerHandle>,
    /// Set in `Drop` BEFORE `abort()`. The timer body has no await after its
    /// sleep, so once the sleep completes the abort can no longer stop it —
    /// a build finishing at ~deadline would otherwise race the fired timer
    /// into a false "overdue" ERROR for a build that already ended, plus a
    /// degraded flag stuck true until the next build (forever on an idle
    /// folder). The task re-checks this flag after the sleep and stands down.
    disarmed: Arc<std::sync::atomic::AtomicBool>,
    folder: String,
    started: std::time::Instant,
}

impl OverdueWatchdog {
    pub fn arm(handle: Arc<WorkerHandle>, folder: &str, deadline: Duration) -> Self {
        let started = std::time::Instant::now();
        let disarmed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = {
            let handle = handle.clone();
            let disarmed = disarmed.clone();
            let folder = folder.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(deadline).await;
                if disarmed.load(Ordering::SeqCst) {
                    return; // the build finished as the timer fired
                }
                handle.set_degraded(true);
                log::error!(
                    target: "moss::build::watch",
                    "Rebuild of '{}' is overdue: still running after {}s (deadline {}s). Not aborting — the build owns unkillable blocking work; this timer only reports.",
                    folder,
                    started.elapsed().as_secs(),
                    deadline.as_secs()
                );
            })
        };
        OverdueWatchdog { task, handle, disarmed, folder: folder.to_string(), started }
    }
}

impl Drop for OverdueWatchdog {
    fn drop(&mut self) {
        self.disarmed.store(true, Ordering::SeqCst);
        self.task.abort();
        if self.handle.is_degraded() {
            log::warn!(
                target: "moss::build::watch",
                "Overdue rebuild of '{}' ended after {}s — degraded state cleared",
                self.folder,
                self.started.elapsed().as_secs()
            );
        }
        // Unconditional: whatever interleaving the fired timer won, the flag
        // must not describe a build that no longer exists.
        self.handle.set_degraded(false);
    }
}

// ---------------------------------------------------------------------------
// Registry: folder path → worker handle
// ---------------------------------------------------------------------------

/// Process-global (like the publish latch it cooperates with) rather than
/// Tauri-managed state, so the headless `moss build --serve --watch` path —
/// which has no `AppHandle` — gets the same worker.
static WORKERS: LazyLock<Mutex<std::collections::HashMap<String, Arc<WorkerHandle>>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

fn workers() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, Arc<WorkerHandle>>> {
    WORKERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Create and register the slot for `folder`. If one is somehow still
/// registered (mirrors `FileWatcherState`'s "shouldn't happen, but be safe"),
/// the old worker is asked to exit.
pub fn register(folder: &str) -> Arc<WorkerHandle> {
    let handle = Arc::new(WorkerHandle::new());
    if let Some(old) = workers().insert(folder.to_string(), handle.clone()) {
        old.request_shutdown();
    }
    handle
}

/// Remove `folder`'s registration — but only if it still maps to `handle`,
/// so a replaced worker's late deregistration cannot evict its successor.
pub fn deregister(folder: &str, handle: &Arc<WorkerHandle>) {
    let mut map = workers();
    if map.get(folder).is_some_and(|h| Arc::ptr_eq(h, handle)) {
        map.remove(folder);
    }
}

/// The slot for `folder`, if a watcher (and thus a worker) is running for it.
pub fn get(folder: &str) -> Option<Arc<WorkerHandle>> {
    workers().get(folder).cloned()
}

/// Wake every worker to re-check its slot. Called by the publish thaw
/// (`deploy::freeze::PublishGuard::drop`): with per-folder slots, requests
/// deferred during the freeze stay parked in their own folder's slot, so the
/// thaw no longer needs to know which folders deferred — a poke over an empty
/// slot costs one wake and nothing else. This replaces the global
/// `deferred_rebuild: Option<String>` side channel, which could lose one of
/// two folders' catch-ups.
pub fn poke_all() {
    for handle in workers().values() {
        handle.poke();
    }
}

/// Kill switch (phase 1a soak): `MOSS_REBUILD_WORKER=off` routes
/// `trigger_rebuild_with_lock` to the old inline-await body. Read once — the
/// routing must not flip mid-session.
pub fn worker_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        !std::env::var("MOSS_REBUILD_WORKER").is_ok_and(|v| v.eq_ignore_ascii_case("off"))
    })
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
