//! Per-folder session: owns UiBound work and is cancelled on window close.
//!
//! Cancellation has a single source of
//! truth: the session's `cancel` token, fired by the close handler in
//! `system/utils.rs`. Three classes of consumers attach:
//!
//! 1. New code: `await session.cancel.cancelled()` directly.
//! 2. AtomicBool consumers: bridged via `bridge_to_atomic` at construction.
//! 3. Legacy `*ConversionState` types with their own `.cancel()` API:
//!    bridged via `bridge_call` (see `build_with_preview_window`).
//!
//! `spawn_ui_bound` is scaffolding for future code that wants to spawn a
//! cancellable task into the session's `JoinSet`. Today the JoinSet is
//! empty in production — UiBound work uses the `services.begin/end_ui_bound`
//! counter pattern instead.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct FolderSession {
    pub folder: PathBuf,
    pub cancel: CancellationToken,
    tasks: Mutex<JoinSet<()>>,
    /// Counter of in-flight UiBound work (build, image/video conversion,
    /// asset copy). Used by `wait_for_in_flight_work` and the window-close
    /// handler to decide whether to wait for the folder's work to drain.
    /// Folder-scoped (per-FolderSession) so two folders cannot conflate
    /// each other's progress.
    ///
    /// A plain level is enough because the build is FROZEN for the duration of
    /// a publish: `build_shell/watch.rs::attempt_admitted_rebuild`
    /// admits nothing while `deploy::publish_in_flight()`, so the set the
    /// drain waits on can only shrink and the level reaches zero. The
    /// monotonic `admitted`/`completed` totals that briefly lived here existed
    /// to survive admissions *during* the wait; they were fungible (any task's
    /// completion credited any other's snapshot), so they could bound
    /// genuinely-old site-sized work by a 300 s settle timer — an I1
    /// violation. The freeze removes the premise; do not reintroduce them.
    ui_bound: AtomicU32,
    /// Serializes access to the folder's mutable `.moss/build.nosync/staging/`
    /// directory between (a) a build's synchronous stage-writing span
    /// (`generate_blocking_content` through notebook processing, in
    /// `build/pipeline.rs::build_inner`) and (b) a PRIOR generation's
    /// detached seal task reading/deleting from that SAME directory
    /// (`ship_phase` + the stale-file/dir cleanup in
    /// `build.rs::advertise_sealed`) — both of which run only AFTER
    /// `await_completion`, i.e. after the rebuild worker has moved on from
    /// this build (see `build_shell/watch.rs::attempt_admitted_rebuild`).
    ///
    /// Without this, a new rebuild's writes into `staging/` can interleave
    /// with a previous generation's still-running seal task reading from —
    /// or deleting stale entries in — that same directory, producing a
    /// generation that gets silently promoted to `current` despite being a
    /// torn mix of two builds' content (or directly deleting a live-served
    /// file out from under a request). This is the root cause resolved by
    /// the seal-persist-race-404 fix.
    ///
    /// Deliberately narrower than `ui_bound`: it is held only across the
    /// fast synchronous stage-writing span (through notebook processing),
    /// NEVER across the slow `await_completion` media-conversion wait or
    /// the genuinely-async background image/video/asset workers — so
    /// back-to-back text edits are never throttled by video/image
    /// encoding, only by (at most) one prior generation's
    /// materialize+cleanup pass.
    ///
    /// `Arc`-wrapped so `register_session_in_registry` can CARRY IT FORWARD
    /// into the next session on a reopen of the same folder, instead of
    /// minting a fresh, unrelated `Mutex`. The lock's whole job is
    /// serializing writes to a physical directory (`stage_dir`) that does
    /// not change identity across a reopen — only the session (cancellation,
    /// UI-bound counter) does. Before this shared it, a build that started
    /// under the PRIOR session and was still running (unkillable —
    /// `spawn_blocking`, see the worker's `OverdueWatchdog` doc) held the old
    /// `Mutex`, while any build admitted under the NEW session — including
    /// the reopened folder's own first build — acquired a brand-new, wholly
    /// unrelated one: two pipelines writing `stage_dir` with no exclusion
    /// between them, the same failure class part 1 of the open-double-build
    /// fix closed for the pre-worker window, reachable here instead via a
    /// reopen. See `tests::reopen_shares_the_stage_write_lock` below.
    stage_write_lock: Arc<Mutex<()>>,
    /// The sweep's "project unavailable" verdict (watcher-reliability design,
    /// 2026-08-18, "Root-gone is a verdict, not a mechanism"): consecutive
    /// root-unreadable sweep passes put the folder here, and a readable pass
    /// clears it. The vault root being gone, renamed, or provider-flapped is
    /// never a rebuild — every disappearance verdict is masked while this
    /// would be true, so the failure mode is one log line, not a storm.
    ///
    /// State only, like the worker's `degraded` flag: nothing gates on it.
    /// The sweep reads it every tick and surfaces transitions to the preview
    /// as `FolderHealthChanged` — the full-window project-unavailable state.
    unavailable: AtomicBool,
}

impl FolderSession {
    pub fn new(folder: PathBuf) -> Arc<Self> {
        Self::with_stage_write_lock(folder, Arc::new(Mutex::new(())))
    }

    /// As [`new`](Self::new), but reusing a `stage_write_lock` a PRIOR
    /// session for the same folder already held, instead of minting a fresh,
    /// unrelated one. `register_session_in_registry` is the one caller: see
    /// the field doc for why a reopen must carry this specific lock forward.
    fn with_stage_write_lock(folder: PathBuf, stage_write_lock: Arc<Mutex<()>>) -> Arc<Self> {
        Arc::new(Self {
            folder,
            cancel: CancellationToken::new(),
            tasks: Mutex::new(JoinSet::new()),
            ui_bound: AtomicU32::new(0),
            stage_write_lock,
            unavailable: AtomicBool::new(false),
        })
    }

    /// Acquire the stage-write guard from async code — the detached seal
    /// task's post-`await_completion` tail (`ship_phase` + stale-file/dir
    /// cleanup in `advertise_sealed`).
    pub async fn lock_stage_write(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.stage_write_lock.lock().await
    }

    /// Acquire the stage-write guard from sync code running on tokio's
    /// blocking pool (`build_inner`, invoked exclusively via
    /// `tokio::task::spawn_blocking`). Guards the build's own synchronous
    /// stage-dir writes against a prior generation's still-running seal
    /// tail.
    ///
    /// # Panics
    /// Panics if called outside a blocking-pool context (e.g. directly from
    /// a plain `async fn` body without `spawn_blocking`) — see
    /// `tokio::sync::Mutex::blocking_lock`.
    pub fn blocking_lock_stage_write(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.stage_write_lock.blocking_lock()
    }

    /// Non-blocking probe of the stage-write guard — the rebuild worker's
    /// try-admission (phase 1b of the watcher-reliability design: "the worker
    /// admits the next build only if `stage_write_lock.try_lock()` succeeds").
    /// `None` means a prior generation's seal tail (or a wedged build) still
    /// holds it; the worker re-parks the request instead of queueing a
    /// blocking-pool thread behind the holder.
    pub fn try_lock_stage_write(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        self.stage_write_lock.try_lock().ok()
    }

    pub fn begin_ui_bound(&self) {
        self.ui_bound.fetch_add(1, Ordering::SeqCst);
    }

    /// Saturating at zero is a double-end guard: an unbalanced `end` would
    /// otherwise underflow the level and make a busy folder look idle, which
    /// is the one direction that matters (deploy would read the slot mid-seal).
    pub fn end_ui_bound(&self) {
        let _ = self.ui_bound.fetch_update(
            Ordering::SeqCst, Ordering::SeqCst,
            |v| if v == 0 { None } else { Some(v - 1) });
    }

    pub fn has_ui_bound(&self) -> bool {
        self.ui_bound.load(Ordering::SeqCst) > 0
    }

    /// Flip the "project unavailable" verdict; returns the previous value so
    /// the sweep logs each transition exactly once. See the field doc.
    pub fn set_unavailable(&self, v: bool) -> bool {
        self.unavailable.swap(v, Ordering::SeqCst)
    }

    pub fn is_unavailable(&self) -> bool {
        self.unavailable.load(Ordering::SeqCst)
    }

    /// Spawn a UiBound future. Cancelled on window close.
    pub async fn spawn_ui_bound<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let token = self.cancel.clone();
        self.tasks.lock().await.spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {}
                _ = fut => {}
            }
        });
    }

    /// Bridge: forward this session's cancel token to a legacy AtomicBool flag.
    /// Spawns one detached task that flips the flag when the token fires.
    /// Useful for new `Arc<AtomicBool>` flags that are not wrapped in a
    /// `*ConversionState`. For legacy state objects with their own `.cancel()`
    /// API, use `bridge_call` instead.
    ///
    /// Two-phase semantics:
    ///
    /// 1. **Synchronous seed.** If the token is already cancelled at the
    ///    call site, the flag is stored `true` BEFORE spawning the bridge
    ///    task. This eliminates a race where a synchronous consumer
    ///    (`spawn_blocking` runner that never yields) could observe `false`
    ///    on its first `Ordering::SeqCst` load because the bridge task
    ///    hadn't been polled yet. Was the root cause of a batched-test
    ///    flake in 2026-05; the test continued to pass in isolation
    ///    because lighter scheduler load happened to poll the bridge in
    ///    time.
    /// 2. **Async bridge.** Spawns a task that awaits `cancelled().await`
    ///    and stores `true` when the token fires — handles the
    ///    "cancelled DURING run" case (folder switch mid-conversion,
    ///    deploy cancel button, etc.) that the synchronous seed cannot
    ///    cover by construction.
    ///
    /// Callers MUST NOT add their own pre-seed; this seam owns the race
    /// elimination contract. Adding a second seed is harmless but
    /// indicates a misunderstanding of the seam — flag it in review.
    pub fn bridge_to_atomic(&self, flag: Arc<AtomicBool>) {
        if self.cancel.is_cancelled() {
            flag.store(true, Ordering::SeqCst);
        }
        let token = self.cancel.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            flag.store(true, Ordering::SeqCst);
        });
    }

    /// Bridge: call a closure when the cancel token fires.
    ///
    /// Used to forward cancellation to legacy state objects with their own
    /// `.cancel()` API (e.g., `VideoConversionState`). Spawns one detached
    /// task per call that awaits the token and then runs the closure.
    pub fn bridge_call(&self, on_cancel: impl FnOnce() + Send + 'static) {
        let token = self.cancel.clone();
        tokio::spawn(async move {
            token.cancelled().await;
            on_cancel();
        });
    }

    /// Fire cancel and drain the JoinSet within `grace`. Returns the number
    /// of tasks that did not finish before the grace window expired.
    pub async fn shutdown(self: Arc<Self>, grace: Duration) -> usize {
        self.cancel.cancel();
        let mut tasks = self.tasks.lock().await;
        let mut leaked = 0usize;
        let deadline = tokio::time::Instant::now() + grace;
        loop {
            tokio::select! {
                next = tasks.join_next() => match next {
                    None => break,
                    Some(_) => continue,
                },
                _ = tokio::time::sleep_until(deadline) => {
                    leaked = tasks.len();
                    tasks.abort_all();
                    break;
                }
            }
        }
        leaked
    }
}

/// Registry of active folder sessions, keyed by canonicalized folder path.
///
/// moss enforces one open folder at a time, so this
/// registry typically holds zero or one session. When a new folder is opened,
/// `swap_in` cancels and removes any existing session before inserting the new one.
#[derive(Default)]
pub struct FolderSessionRegistry {
    sessions: StdMutex<HashMap<String, Arc<FolderSession>>>,
}

/// The process-global registry (2026-08-24 extension: lifetime
/// differences live in the `FolderSession`, and the session must therefore be
/// reachable without an `AppHandle`). Until then this was `app.manage`d, which
/// made every headless path session-less by construction — the watcher's
/// sweep fabricated standalone sessions and the seal tail had no lock to
/// take. Same pattern as the search lane's `LANES` and the sweep's own
/// `SWEEPS`: a `Mutex`-guarded map whose one instance is the process.
static REGISTRY: std::sync::OnceLock<FolderSessionRegistry> = std::sync::OnceLock::new();

pub fn registry() -> &'static FolderSessionRegistry {
    REGISTRY.get_or_init(FolderSessionRegistry::default)
}

impl FolderSessionRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, folder: &str) -> Option<Arc<FolderSession>> {
        self.sessions.lock().unwrap().get(folder).cloned()
    }

    /// Insert a new session, returning any session that was previously registered
    /// for the same folder (caller is responsible for shutting it down).
    pub fn insert(&self, folder: String, session: Arc<FolderSession>) -> Option<Arc<FolderSession>> {
        self.sessions.lock().unwrap().insert(folder, session)
    }

    pub fn remove(&self, folder: &str) -> Option<Arc<FolderSession>> {
        self.sessions.lock().unwrap().remove(folder)
    }

    /// Take all currently registered sessions, leaving the registry empty.
    /// Used on folder switch to cancel any prior session.
    pub fn drain(&self) -> Vec<Arc<FolderSession>> {
        let mut guard = self.sessions.lock().unwrap();
        guard.drain().map(|(_, v)| v).collect()
    }
}

/// Construct a `FolderSession` and register it in the process-global
/// [`registry`]. Called from both GUI (`build_with_preview_window`) and CLI
/// (`start_cli_build`) so both paths share the same session lifecycle.
///
/// Drains and shuts down any prior session for the same folder before
/// installing the new one.
///
/// **Pre-Track A this also installed `bridge_call` adapters from the session's
/// cancel token to the three `*ConversionState::cancel()` methods** (hence its
/// former name, `register_session_with_bridges`). Those methods are now
/// `#[deprecated]` no-ops (the underlying `cancelled: AtomicBool` field was
/// removed); installing bridges to no-op closures is pure overhead so the
/// bridges have been removed. Cancellation now flows exclusively through
/// `FolderSession::cancel`; consumers that need to read it install their own
/// short-lived `bridge_to_atomic` (see e.g. `run_video_conversion`,
/// `run_image_conversion`, `run_notebook_processing`).
pub fn register_session(folder_path: &str) -> Arc<FolderSession> {
    register_session_in_registry(registry(), folder_path)
}

/// Inner helper: drain prior session(s), construct a new session, insert into
/// the registry. Extracted from `register_session` so it can be
/// unit-tested without a `tauri::AppHandle`.
pub(crate) fn register_session_in_registry(
    reg: &FolderSessionRegistry,
    folder_path: &str,
) -> Arc<FolderSession> {
    // Step 1: drain any prior session(s) (folder-switch path). A prior
    // session FOR THIS SAME FOLDER (a reopen) hands its stage_write_lock
    // forward to the new session below, BEFORE its shutdown is spawned —
    // shutdown fires `cancel` and drains `tasks` (empty in production; see
    // the module doc), it does NOT wait for `ui_bound` to reach zero, so a
    // build that started under the prior session can still be running,
    // unkillable, when this function returns. Reusing the same `Mutex`
    // (rather than each session minting its own) is what keeps that build
    // and anything admitted under the NEW session serialized against each
    // other regardless — see the field doc on `stage_write_lock`.
    let mut reused_stage_write_lock = None;
    for prior in reg.drain() {
        if prior.folder == std::path::Path::new(folder_path) {
            reused_stage_write_lock = Some(prior.stage_write_lock.clone());
        }
        tokio::spawn(async move {
            let leaked = prior.shutdown(Duration::from_secs(2)).await;
            if leaked > 0 {
                log::warn!(
                    "FolderSession folder-switch dropped {} task(s) past grace",
                    leaked
                );
            }
        });
    }

    // Step 2: construct the new session and insert.
    let session = match reused_stage_write_lock {
        Some(lock) => FolderSession::with_stage_write_lock(PathBuf::from(folder_path), lock),
        None => FolderSession::new(PathBuf::from(folder_path)),
    };
    if let Some(prior) = reg.insert(folder_path.to_string(), session.clone()) {
        // Belt-and-suspenders: if a prior session was somehow still registered
        // (Step 1 drain raced with another insert, or future refactor decouples
        // drain from insert), shut it down rather than leak its tasks.
        tokio::spawn(async move {
            let leaked = prior.shutdown(Duration::from_secs(2)).await;
            if leaked > 0 {
                log::warn!(
                    "FolderSession replaced-via-insert dropped {} task(s) past grace",
                    leaked
                );
            }
        });
    }

    session
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `end_ui_bound` saturates at zero. An unbalanced end must not underflow
    /// the level: `has_ui_bound()` is what the close handler and the publish
    /// drain consult, and a wrapped `u32` would report a busy folder as idle,
    /// letting deploy read the manifest slot mid-seal.
    #[test]
    fn a_no_op_end_ui_bound_does_not_underflow_the_level() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        s.end_ui_bound(); // must not panic or underflow
        assert!(!s.has_ui_bound());

        s.begin_ui_bound();
        assert!(s.has_ui_bound());
        s.end_ui_bound();
        assert!(!s.has_ui_bound());
        s.end_ui_bound(); // the extra one is the double-end being guarded
        assert!(!s.has_ui_bound());
    }

    /// The rebuild worker's try-admission probe: never blocks, answers "held"
    /// while any holder (seal tail or build) has the guard, and succeeds
    /// again the moment it is released.
    #[tokio::test]
    async fn try_lock_stage_write_probes_without_blocking() {
        let s = FolderSession::new(PathBuf::from("/tmp/test-try-lock"));
        let guard = s.lock_stage_write().await;
        assert!(
            s.try_lock_stage_write().is_none(),
            "the probe must report a held lock, not wait for it"
        );
        drop(guard);
        assert!(
            s.try_lock_stage_write().is_some(),
            "the probe must succeed once the holder releases"
        );
    }

    /// Reopening the same folder must inherit the PRIOR session's
    /// `stage_write_lock`, not mint a fresh, unrelated one — the reopen half
    /// of the seal-persist-race-404 class part 1 of the open-double-build fix
    /// left open. `register_session_in_registry`'s shutdown of the prior
    /// session fires `cancel` and drains `tasks` (empty in production), but
    /// does NOT wait for `ui_bound` to reach zero — a build started under the
    /// prior session can still be running (unkillable — `spawn_blocking`)
    /// when the new session is handed back. Simulated here by simply holding
    /// the first session's guard across the reopen, standing in for that
    /// still-running build: the second session's probe must see it held.
    #[tokio::test]
    async fn reopen_shares_the_stage_write_lock() {
        let reg = FolderSessionRegistry::new();
        let folder = "/tmp/reopen-stage-lock-test";

        let first = register_session_in_registry(&reg, folder);
        let held = first.lock_stage_write().await; // stands in for an in-flight build

        let second = register_session_in_registry(&reg, folder);
        assert!(
            second.try_lock_stage_write().is_none(),
            "a reopen must inherit the prior session's stage_write_lock — a \
             build still running under the OLD session must block a build \
             admitted under the NEW one, not race it on an unrelated Mutex"
        );

        drop(held);
        assert!(
            second.try_lock_stage_write().is_some(),
            "the inherited lock must still work once the prior build releases it"
        );
    }

    /// The inheritance above is keyed on folder path, not "whatever was
    /// registered" — opening a DIFFERENT folder while one is in flight must
    /// get its own, independent lock rather than blocking on an unrelated
    /// folder's build.
    #[tokio::test]
    async fn opening_a_different_folder_gets_its_own_stage_write_lock() {
        let reg = FolderSessionRegistry::new();
        let a = register_session_in_registry(&reg, "/tmp/reopen-stage-lock-test-a");
        let _held = a.lock_stage_write().await;

        let b = register_session_in_registry(&reg, "/tmp/reopen-stage-lock-test-b");
        assert!(
            b.try_lock_stage_write().is_some(),
            "a different folder's session must not inherit folder A's lock"
        );
    }

    /// Deterministic proof (no wall-clock/virtual-time dependency — pure
    /// cooperative-scheduling handshake) that `stage_write_lock` serializes
    /// a prior generation's seal-tail (`ship_phase` + stale cleanup) against
    /// the next rebuild's stage-writing phase — the seal-persist-race-404
    /// fix.
    ///
    /// Task A simulates the seal-tail: acquires the guard, signals that it
    /// holds it, then waits to be told to release. Task B simulates the
    /// next rebuild's stage-write phase: attempts to acquire the SAME guard
    /// while A still holds it. The test explicitly asserts B has logged
    /// NOTHING while A holds the lock (proving genuine mutual exclusion,
    /// not just lucky ordering), then releases A and confirms B only
    /// proceeds afterward.
    #[tokio::test]
    async fn stage_write_lock_serializes_seal_tail_against_next_rebuild_write() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let log: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
        let a_acquired = Arc::new(tokio::sync::Notify::new());
        let release_a = Arc::new(tokio::sync::Notify::new());

        // Task A simulates a build's detached seal-tail (ship_phase +
        // stale-file/dir cleanup in advertise_sealed): holds the guard
        // until explicitly told to release (analogous to walking/copying
        // staging/ taking real wall-clock time).
        let log_a = log.clone();
        let s_a = s.clone();
        let a_acquired_tx = a_acquired.clone();
        let release_a_rx = release_a.clone();
        let task_a = tokio::spawn(async move {
            let _guard = s_a.lock_stage_write().await;
            log_a.lock().unwrap().push("seal-tail-enter");
            a_acquired_tx.notify_one();
            release_a_rx.notified().await;
            log_a.lock().unwrap().push("seal-tail-exit");
        });

        // Wait until A actually holds the lock before spawning B.
        a_acquired.notified().await;

        // Task B simulates the NEXT rebuild's stage-writing phase
        // (generate_blocking_content..notebook processing in build_inner),
        // triggered by a file edit while A's seal-tail is still running.
        let log_b = log.clone();
        let s_b = s.clone();
        let task_b = tokio::spawn(async move {
            let _guard = s_b.lock_stage_write().await;
            log_b.lock().unwrap().push("next-rebuild-write-enter");
        });

        // Give the (single-threaded) executor a chance to poll B up to its
        // (blocking) lock acquisition. No real-time wait — just cooperative
        // yields — so this is not a wall-clock race.
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }

        // Prove B is genuinely blocked on the guard while A still holds it.
        assert_eq!(
            *log.lock().unwrap(),
            vec!["seal-tail-enter"],
            "next rebuild's stage-write must not proceed while the prior \
             generation's seal-tail still holds stage_write_lock"
        );

        release_a.notify_one();
        task_a.await.unwrap();
        task_b.await.unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            vec!["seal-tail-enter", "seal-tail-exit", "next-rebuild-write-enter"],
            "the next rebuild's stage-dir write must not start until the prior \
             generation's seal-tail released stage_write_lock — otherwise the \
             two can interleave and corrupt the materialized generation"
        );
    }

    /// Proves the guard actually excludes across the real production
    /// interop: `build_inner` (pipeline.rs) acquires via
    /// `blocking_lock_stage_write()` on tokio's blocking pool, while
    /// `advertise_sealed` (build.rs) acquires via
    /// `lock_stage_write().await` from a plain async task. Both must
    /// exclude each other on the SAME underlying mutex, not just
    /// same-flavor callers (covered by the test above).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stage_write_lock_serializes_across_blocking_and_async_callers() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let log: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
        let acquired = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());

        // Async side simulates advertise_sealed's seal-tail.
        let s1 = s.clone();
        let log1 = log.clone();
        let acquired1 = acquired.clone();
        let release1 = release.clone();
        let async_task = tokio::spawn(async move {
            let _g = s1.lock_stage_write().await;
            log1.lock().unwrap().push("async-enter");
            acquired1.notify_one();
            release1.notified().await;
            log1.lock().unwrap().push("async-exit");
        });

        acquired.notified().await;

        // Blocking side simulates build_inner running on the blocking pool.
        let s2 = s.clone();
        let log2 = log.clone();
        let blocking_task = tokio::task::spawn_blocking(move || {
            let _g = s2.blocking_lock_stage_write();
            log2.lock().unwrap().push("blocking-enter");
        });

        // Bounded poll (real OS thread on the other side, so we can't use
        // cooperative yields as in the pure-async test above) proving the
        // blocking caller does not proceed while the async guard is held.
        // Same polling pattern as this file's existing
        // bridge_flips_atomic/bridge_call_invokes_closure tests.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            *log.lock().unwrap(),
            vec!["async-enter"],
            "blocking_lock_stage_write() must not proceed while the async \
             lock_stage_write() guard is still held — they must share the \
             SAME underlying lock"
        );

        release.notify_one();
        async_task.await.unwrap();
        blocking_task.await.unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            vec!["async-enter", "async-exit", "blocking-enter"]
        );
    }

    #[tokio::test]
    async fn cancellation_aborts_future() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        s.spawn_ui_bound(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            c.fetch_add(1, Ordering::SeqCst);
        }).await;
        s.clone().shutdown(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fast_task_completes_within_grace() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        s.spawn_ui_bound(async move {
            // tasks not selecting on cancel get aborted by JoinSet
            // so we demonstrate one that does the work first
            c.fetch_add(1, Ordering::SeqCst);
        }).await;
        // give the task a tick to run before we shutdown
        tokio::task::yield_now().await;
        let leaked = s.clone().shutdown(Duration::from_millis(500)).await;
        assert_eq!(leaked, 0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bridge_flips_atomic() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let flag = Arc::new(AtomicBool::new(false));
        s.bridge_to_atomic(flag.clone());
        assert!(!flag.load(Ordering::SeqCst));
        s.cancel.cancel();
        // Bridge task is detached; give it a moment.
        for _ in 0..100 {
            if flag.load(Ordering::SeqCst) { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn bridge_call_invokes_closure() {
        let s = FolderSession::new(PathBuf::from("/tmp/test"));
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        s.bridge_call(move || { c.fetch_add(42, Ordering::SeqCst); });
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        s.cancel.cancel();
        for _ in 0..100 {
            if counter.load(Ordering::SeqCst) > 0 { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(counter.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn registry_insert_and_remove() {
        let reg = FolderSessionRegistry::new();
        let s = FolderSession::new(PathBuf::from("/tmp/foo"));
        assert!(reg.get("/tmp/foo").is_none());
        let prev = reg.insert("/tmp/foo".to_string(), s.clone());
        assert!(prev.is_none());
        assert!(reg.get("/tmp/foo").is_some());
        let removed = reg.remove("/tmp/foo");
        assert!(removed.is_some());
        assert!(reg.get("/tmp/foo").is_none());
    }

    #[test]
    fn registry_drain_takes_all() {
        let reg = FolderSessionRegistry::new();
        reg.insert("/a".to_string(), FolderSession::new(PathBuf::from("/a")));
        reg.insert("/b".to_string(), FolderSession::new(PathBuf::from("/b")));
        let drained = reg.drain();
        assert_eq!(drained.len(), 2);
        assert!(reg.get("/a").is_none());
        assert!(reg.get("/b").is_none());
    }

    /// Helper smoke test: registers a new session and the registry holds it.
    ///
    /// Tests `register_session_in_registry` (the testable inner function)
    /// rather than `register_session` directly, because the
    /// latter requires a `tauri::AppHandle` that's unavailable in unit tests.
    /// The outer wrapper is a thin AppHandle adapter — it just looks up the
    /// registry from managed state and delegates here.
    #[tokio::test]
    async fn helper_registers_new_session() {
        let reg = FolderSessionRegistry::new();
        let session = register_session_in_registry(&reg, "/tmp/test-folder");
        assert!(reg.get("/tmp/test-folder").is_some());
        assert!(Arc::ptr_eq(&reg.get("/tmp/test-folder").unwrap(), &session));
    }

    /// Helper drains prior session for the same folder and registers the new one.
    #[tokio::test]
    async fn helper_drains_prior_session_for_same_folder() {
        let reg = FolderSessionRegistry::new();
        let first = register_session_in_registry(&reg, "/tmp/folder-A");
        let first_token = first.cancel.clone();
        let second = register_session_in_registry(&reg, "/tmp/folder-A");

        // The drain spawn fires shutdown which fires cancel; wait for it.
        for _ in 0..100 {
            if first_token.is_cancelled() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(first_token.is_cancelled(), "prior session not drained");
        assert!(Arc::ptr_eq(&reg.get("/tmp/folder-A").unwrap(), &second));
    }

    /// Helper returns a session even when the registry has no prior entries
    /// (first-build path; nothing to drain).
    #[tokio::test]
    async fn helper_first_build_no_drain() {
        let reg = FolderSessionRegistry::new();
        let session = register_session_in_registry(&reg, "/tmp/first");
        // Token starts un-cancelled.
        assert!(!session.cancel.is_cancelled());
        assert!(reg.get("/tmp/first").is_some());
    }
}
