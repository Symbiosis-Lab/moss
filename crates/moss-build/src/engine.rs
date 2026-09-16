//! Off-webview plugin execution engine (#789, Phase 2).
//!
//! **Architecture C — the integration core.** An rquickjs / quickjs-ng runtime is
//! `!Send` (single-threaded; we deliberately do NOT enable the experimental
//! `parallel` feature). But moss's build pipeline awaits plugin hooks inside
//! `tauri::async_runtime::spawn(async move { … })` on the MULTI-THREADED tokio
//! runtime (`build.rs` `spawn_process_hooks`), where the spawned future must be
//! `Send`. A `!Send` engine value therefore cannot be held across an `.await` in
//! that task.
//!
//! Resolution: the engine owns its `!Send` runtime + per-plugin Contexts on a
//! dedicated OS thread running a current-thread tokio runtime + `LocalSet`;
//! callers interact over channels. Every public method returns a future that holds
//! only `Send` channel handles, so it composes with the `Send` build path. This is
//! the load-bearing boundary the feasibility review flagged.
//!
//! ## The drive-loop resolution (load-bearing — see [`engine_loop`])
//!
//! The job loop runs a `tokio::select!` that CONCURRENTLY (a) awaits the next job on
//! `rx.recv()` and (b) pumps `runtime.drive()`. Each `DispatchHook` runs the hook on
//! its OWN `spawn_local` task (so the loop is free to re-select); a `DeliverEvent`
//! runs INLINE in its loop arm (a quick "restore Persistent + call handler"). The
//! select pumps the rquickjs schedular every iteration, so a hook parked on an
//! in-hook `setTimeout` makes progress AND a queued `DeliverEvent` is dequeued and
//! delivered while that hook is still parked — the Task-14a topology.
//!
//! Why `drive()` and NOT `idle()` (empirically verified against rquickjs 0.12):
//! - `AsyncRuntime::idle()` (rquickjs-core `runtime/async.rs`) returns `Poll::Ready`
//!   ONLY when the schedular is `Empty`; while any spawned future is parked on a
//!   tokio timer (`SchedularPoll::Pending`) it stays parked HOLDING THE RUNTIME LOCK.
//!   A parked hook task's `WithFuture::poll` needs that same lock
//!   (`context/async/future.rs`), so `idle().await` after dispatching a hook STARVES
//!   the hook for its whole multi-tick duration — the loop never returns to `recv()`,
//!   and a queued `DeliverEvent` is never dequeued (the verified blocker).
//! - `AsyncRuntime::drive()`'s `DriveFuture` (rquickjs-core `runtime/spawner.rs`)
//!   re-acquires the lock, runs pending jobs, polls the schedular, then returns
//!   `Poll::Pending` and RELEASES the lock between ticks. So a parked hook task can
//!   acquire the lock and progress, and the `select!` re-evaluates `rx.recv()` every
//!   tick. This is the load-bearing difference: idle parks holding the lock; drive
//!   releases it.
//!
//! A resident `tokio::task::spawn_local(runtime.drive())` was rejected earlier
//! (Task 8) because it stalled in-hook host-fn futures; the select! is the resolution
//! — drive() is polled in lock-step with the job recv, not as a detached task. The
//! 11b smoke test (`dispatch_hook_runs_trivial_bundle_with_timer`, single in-hook
//! `setTimeout`) and `deliver_event_reaches_hook_parked_in_poll_loop` (the Task-14a
//! parked-while-deliver topology) are the acceptance gates for this decision.

pub mod app_host;
pub(crate) mod fetch_shim;
pub mod host_fns;
pub(crate) mod loader;
pub(crate) mod marshal;
pub mod tauri_bridge;
pub(crate) mod url_shim;
pub(crate) mod web_shims;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rquickjs::{async_with, AsyncContext, AsyncRuntime, Function, Object, Value};
use self::app_host::SharedAppHost;
use tokio::sync::{mpsc, oneshot};

use self::host_fns::{DispatchBridge, DispatchRequest};

/// Generation of the hook future currently being POLLED on this engine thread.
/// None while drive()'s pending jobs / inline DeliverEvent run — that JS is
/// killable only by the global kill flag (accepted gap, D1).
/// CAUTION (D1 false-positive direction): Some(gen) does NOT mean the on-stack
/// JS belongs to that hook — WithFuture::poll pumps the runtime-wide schedular,
/// so a sibling plugin's timer/promise job can run under this gen. Mitigated by
/// the biased select below (a cancelled hook's future is never re-polled);
/// residual: a cancel landing while a sibling slice is on-stack can interrupt
/// that one slice (swallowed by web_shims' `let _ = cb.call`, no poisoning).
thread_local! {
    static CURRENT_GEN: Cell<Option<u64>> = const { Cell::new(None) };
    /// gen → CancellationToken for every in-flight hook task on this thread.
    static HOOK_TOKENS: RefCell<HashMap<u64, tokio_util::sync::CancellationToken>> =
        RefCell::new(HashMap::new());
}

/// Poll-wrapper stamping CURRENT_GEN around every poll of the hook future, so the
/// interrupt handler can attribute the on-stack JS slice to a cancellable hook.
/// Box::pin keeps this Unpin (one allocation per dispatch — negligible) and avoids
/// unsafe pin projection.
struct GenScoped<F> {
    gen: u64,
    inner: std::pin::Pin<Box<F>>,
}
impl<F: std::future::Future> GenScoped<F> {
    fn new(gen: u64, inner: F) -> Self {
        Self { gen, inner: Box::pin(inner) }
    }
}
impl<F: std::future::Future> std::future::Future for GenScoped<F> {
    type Output = F::Output;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<F::Output> {
        CURRENT_GEN.with(|c| c.set(Some(self.gen)));
        let r = self.inner.as_mut().poll(cx);
        CURRENT_GEN.with(|c| c.set(None));
        r
    }
}

/// A unit of work handed to the engine thread.
enum Job {
    /// Run `hook` on `plugin`'s loaded instance. Get-or-creates the per-plugin
    /// Context (installing all web/host shims + the bundle on first sight), then
    /// runs the hook on its own `spawn_local` task and replies with the JSON of the
    /// resolved return value.
    DispatchHook {
        plugin: String,
        bundle: String,
        global_name: String,
        hook: String,
        context_json: String,
        project_path: String,
        moss_dir: String,
        reply: oneshot::Sender<Result<String, String>>,
        /// Per-hook cancellation (condition 6, D3): the manager cancels this from the
        /// three timeout sites. Killing the hook parked: select arm fires first (biased,
        /// cancelled-arm-first is load-bearing — see the spawn_local task below).
        /// Killing a wedged sync loop: interrupt handler reads CURRENT_GEN + HOOK_TOKENS.
        cancel: tokio_util::sync::CancellationToken,
    },
    /// Deliver a host event to `plugin`'s registered `__TAURI__.event` listeners.
    /// Fire-and-forget (no reply). Dropped silently if the plugin isn't loaded.
    DeliverEvent {
        plugin: String,
        name: String,
        payload_json: String,
    },
    /// Teardown: cancel every plugin's timers, drop all Contexts, ack, then the
    /// thread's recv loop ends.
    Shutdown { reply: oneshot::Sender<()> },
    /// Test-only: panic inline in the job loop — deterministic engine-thread death
    /// for the condition-5 respawn tests. Never constructed in release builds.
    #[cfg(any(test, feature = "test-fixtures"))]
    PanicForTest,
}

/// Everything `engine_thread_main` needs, parked until the first job (D9:
/// zero-plugin folders must not pay an OS thread + rquickjs runtime per
/// folder-open). Wrapped in `Arc<Mutex<Option<_>>>` so ALL clones of the handle
/// race-safely share one spawn-once gate: the first `ensure_thread` call to
/// observe `Some(_)` under the mutex wins and spawns; subsequent callers (and
/// all clones) observe `None` and skip. The `jobs` channel is created eagerly
/// and buffers jobs sent before the thread starts — the thread drains them as
/// soon as it enters its recv loop.
struct PendingSpawn {
    rx: mpsc::UnboundedReceiver<Job>,
    dispatch_tx: mpsc::UnboundedSender<DispatchRequest>,
    app: Option<SharedAppHost>,
    kill: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Handle to a JavaScript engine running on its own OS thread.
///
/// Cheap to clone (Arc clones). When the last clone drops AND no job has ever
/// been sent, `pending_spawn` is dropped (channels close, no thread to join).
/// When the thread IS alive, it exits when its `rx` close (all `jobs` senders
/// dropped across all clones).
#[derive(Clone)]
pub struct QuickJsEngine {
    jobs: mpsc::UnboundedSender<Job>,
    /// Kept so the engine handle owns a live sender to the dispatch channel even if
    /// the host task hasn't been spawned yet — prevents a premature channel close.
    #[allow(dead_code)]
    dispatch_tx: mpsc::UnboundedSender<DispatchRequest>,
    /// Out-of-band kill switch (Phase-4 condition 1, D1): set by `shutdown()`
    /// BEFORE sending Job::Shutdown; read by the rquickjs interrupt handler.
    /// Out-of-band because a hook wedged in synchronous JS never lets the loop
    /// service the job channel — this flag is the only signal that reaches it.
    kill: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Lazy-spawn gate (D9): parked until the first job. All clones share this
    /// Arc; the take-once under the mutex ensures exactly one thread is spawned
    /// regardless of how many clones call `ensure_thread` concurrently.
    pending_spawn: std::sync::Arc<std::sync::Mutex<Option<PendingSpawn>>>,
}

impl QuickJsEngine {
    /// Spawn the engine. Returns the engine handle plus the `DispatchRequest`
    /// receiver the caller MUST drain on a host task, which routes
    /// `__TAURI__.core.invoke`. `app` is `None` on a headless `moss build`
    /// (#1019) and in tests, where `__TAURI__.event.emit` becomes a no-op and
    /// dispatch routes through the injected `HostState` instead.
    pub fn new(app: Option<SharedAppHost>) -> (Self, mpsc::UnboundedReceiver<DispatchRequest>) {
        let (jobs, rx) = mpsc::unbounded_channel::<Job>();
        let (dispatch_tx, dispatch_rx) = mpsc::unbounded_channel::<DispatchRequest>();
        let dispatch_tx_for_thread = dispatch_tx.clone();
        let kill = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let kill_for_thread = kill.clone();
        // D9 lazy spawn: park everything the thread needs; the OS thread is
        // deferred to `ensure_thread()` on first job send. Zero-plugin
        // folder-opens pay NO thread or rquickjs runtime cost.
        let pending_spawn = std::sync::Arc::new(std::sync::Mutex::new(Some(PendingSpawn {
            rx,
            dispatch_tx: dispatch_tx_for_thread,
            app,
            kill: kill_for_thread,
        })));
        (Self { jobs, dispatch_tx, kill, pending_spawn }, dispatch_rx)
    }

    /// First-job lazy spawn (D9). Take-once under the mutex makes concurrent
    /// clones race-safe: exactly one caller observes `Some` and spawns; all
    /// others observe `None` and skip.
    ///
    /// Called as the FIRST LINE of every send path (`dispatch_hook_with_cancel`,
    /// `deliver_event`, `panic_engine_thread_for_test`). The `DeliverEvent` case
    /// spawns the thread even though a plugin with no Context will just hit the
    /// `plugins.get` miss arm — the cost is one extra thread startup for the
    /// rare first-bridged-event-before-dispatch shape; the uniformity is worth
    /// more than the micro-optimization.
    fn ensure_thread(&self) {
        let pending = self.pending_spawn.lock().unwrap().take();
        if let Some(p) = pending {
            static ENGINE_SEQ: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let n = ENGINE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::thread::Builder::new()
                .name(format!("moss-plugin-engine-{n}"))
                .spawn(move || engine_thread_main(p.rx, p.dispatch_tx, p.app, p.kill))
                .expect("failed to spawn moss-plugin-engine thread");
        }
    }

    /// Run `hook` on `plugin`'s bundle and return the JSON of the resolved value.
    ///
    /// The per-plugin Context is created (and the bundle loaded with all shims) on
    /// first dispatch and reused across calls. Uses a never-cancelled token — for
    /// cancellable dispatch see `dispatch_hook_with_cancel`.
    #[allow(clippy::too_many_arguments)]
    pub async fn dispatch_hook_raw(
        &self,
        plugin: &str,
        bundle: &str,
        global_name: &str,
        hook: &str,
        context_json: &str,
        project_path: &str,
        moss_dir: &str,
    ) -> Result<String, String> {
        self.dispatch_hook_with_cancel(
            plugin,
            bundle,
            global_name,
            hook,
            context_json,
            project_path,
            moss_dir,
            tokio_util::sync::CancellationToken::new(), // never cancelled
        )
        .await
    }

    /// `dispatch_hook_raw` with an out-of-band CancellationToken (condition 6, D3).
    /// Cancelling kills the hook BOTH parked (select arm) and wedged in synchronous
    /// JS (interrupt handler via the engine thread's generation attribution).
    #[allow(clippy::too_many_arguments)]
    pub async fn dispatch_hook_with_cancel(
        &self,
        plugin: &str,
        bundle: &str,
        global_name: &str,
        hook: &str,
        context_json: &str,
        project_path: &str,
        moss_dir: &str,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<String, String> {
        // D9 lazy spawn: ensure the OS thread exists before we send the job.
        // Jobs sent before the thread starts simply buffer in the unbounded channel.
        self.ensure_thread();
        let (reply, reply_rx) = oneshot::channel();
        self.jobs
            .send(Job::DispatchHook {
                plugin: plugin.to_string(),
                bundle: bundle.to_string(),
                global_name: global_name.to_string(),
                hook: hook.to_string(),
                context_json: context_json.to_string(),
                project_path: project_path.to_string(),
                moss_dir: moss_dir.to_string(),
                reply,
                cancel,
            })
            .map_err(|_| "plugin engine thread has exited".to_string())?;
        reply_rx
            .await
            .map_err(|_| "plugin engine dropped the hook without replying".to_string())?
    }

    /// Test-only: panic inline in the job loop — deterministic engine-thread death
    /// for the condition-5 respawn tests. Never constructed in release builds.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn panic_engine_thread_for_test(&self) {
        // D9: ensure the thread is up before we send the panic job.
        self.ensure_thread();
        let _ = self.jobs.send(Job::PanicForTest);
    }

    /// Test-only: returns true if the OS thread has been spawned (D9 lazy-spawn
    /// regression gate). False = engine constructed but no job sent yet.
    /// Uses the take-once mutex: `pending_spawn` is `None` iff `ensure_thread`
    /// has already run (i.e. the thread has been spawned).
    #[cfg(test)]
    pub(crate) fn thread_spawned_for_test(&self) -> bool {
        self.pending_spawn.lock().unwrap().is_none()
    }

    /// Deliver a host event to `plugin`'s `__TAURI__.event` listeners. Fire-and-forget.
    pub fn deliver_event(&self, plugin: &str, name: &str, payload_json: &str) {
        // D9 lazy spawn: the bridged event path must also ensure the thread exists.
        // A plugin with no Context yet will hit the `plugins.get` miss arm (no-op),
        // but the thread must be up to receive future dispatch jobs.
        self.ensure_thread();
        let _ = self.jobs.send(Job::DeliverEvent {
            plugin: plugin.to_string(),
            name: name.to_string(),
            payload_json: payload_json.to_string(),
        });
    }

    /// Cancel all timers, drop every Context, and wait for the engine thread to ack.
    pub async fn shutdown(&self) {
        // Never-spawned engine (D9): nothing to kill — drop the parked state so
        // the channel halves close; Drop is a no-op. The D4 teardown tombstone
        // (EngineSlot::torn_down) is set BEFORE this call in plugin_engine.rs, so
        // the D9 shutdown race (ensure_thread losing to shutdown's pending-take →
        // send into a dropped rx) funnels into the "never-accepted" sentinel and
        // is blocked by the same tombstone.
        if self.pending_spawn.lock().unwrap().take().is_some() {
            return;
        }
        // Set BEFORE sending (D1): a wedged thread never dequeues the job; the
        // interrupt handler sees the flag at the next JS poll point and raises the
        // uncatchable "interrupted", unwinding the on-stack slice so the select!
        // loop resumes and dequeues Shutdown. Teardown runs no JS, so the
        // still-set flag cannot interrupt teardown itself.
        self.kill.store(true, std::sync::atomic::Ordering::Relaxed);
        let (reply, reply_rx) = oneshot::channel();
        if self.jobs.send(Job::Shutdown { reply }).is_ok() {
            let _ = reply_rx.await;
        }
    }
}

/// Per-plugin runtime record: one Context plus its `!Send` timer + event state.
/// `fingerprint` hashes the bundle source that built this Context — a dispatch
/// arriving with DIFFERENT bytes (plugin update/reinstall via the live
/// BundleSource) evicts the stale Context (B.7; first-sight-wins is otherwise
/// forever).
struct PluginRuntime {
    ctx: AsyncContext,
    timers: Rc<RefCell<self::web_shims::TimerRegistry>>,
    hub: Rc<RefCell<self::tauri_bridge::EventHub>>,
    fingerprint: u64,
    /// Live hook tasks on this Context (engine thread only). Eviction with
    /// in_flight > 0 RETIRES the runtime instead of killing its timers — a
    /// cancel_all here orphans the in-flight hook's awaited setTimeout forever
    /// (Phase-4 condition 4, D2).
    in_flight: Rc<Cell<usize>>,
    /// D3 hygiene: set by a cancelled/interrupted hook task to signal that this
    /// Context has half-mutated JS state. The loop-top poison sweep retires it
    /// on the next iteration (rides D2's retire machinery so sibling in-flight
    /// hooks on the same Context are not orphaned).
    poisoned: Rc<Cell<bool>>,
}

/// Full single-runtime teardown ordering (timers → TIMERS thread-local entry →
/// listener Persistents WHILE the runtime is alive → caller drops the Context).
async fn teardown_one(pr: &PluginRuntime) {
    pr.timers.borrow_mut().cancel_all();
    let ctx = pr.ctx.clone();
    let _ = async_with!(ctx => |ctx| {
        self::web_shims::clear_context_timers(&ctx);
    })
    .await;
    pr.hub.borrow_mut().listeners.clear();
}

/// Tear down retirees whose last in-flight hook has finished (checked at the top
/// of every job-loop iteration — sufficient, since counts only change on this
/// thread between iterations).
async fn drain_retired(retired: &mut Vec<PluginRuntime>) {
    let mut i = 0;
    while i < retired.len() {
        if retired[i].in_flight.get() == 0 {
            let pr = retired.swap_remove(i);
            teardown_one(&pr).await;
        } else {
            i += 1;
        }
    }
}

/// Hash of the bundle source backing a plugin's Context (eviction key — B.7).
fn bundle_fingerprint(bundle: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bundle.hash(&mut h);
    h.finish()
}

/// Entry point for the dedicated engine thread: owns the `!Send` rquickjs runtime
/// + per-plugin Contexts and services jobs until the job channel closes.
fn engine_thread_main(
    rx: mpsc::UnboundedReceiver<Job>,
    dispatch_tx: mpsc::UnboundedSender<DispatchRequest>,
    app_for_engine: Option<SharedAppHost>,
    kill: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let tokio_rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            drain_with_error(rx, format!("engine tokio runtime build failed: {e}"));
            return;
        }
    };

    let local = tokio::task::LocalSet::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        local.block_on(&tokio_rt, async move {
            engine_loop(rx, dispatch_tx, app_for_engine, kill).await;
        });
    }));
    if result.is_err() {
        // All subsequent API calls fail fast with "plugin engine thread has exited".
        // Per-manager cardinality (D1) means folder close/reopen replaces the engine;
        // automatic respawn is the Phase-4 hard-gate.
        log::error!(target: "plugin", "moss-plugin-engine thread PANICKED — engine dead until manager teardown/recreate");
    }
}

/// The LocalSet body. Lives in its own async fn so `?`-free job handling reads
/// straight-line and the `!Send` state never escapes the LocalSet.
async fn engine_loop(
    mut rx: mpsc::UnboundedReceiver<Job>,
    dispatch_tx: mpsc::UnboundedSender<DispatchRequest>,
    app_for_engine: Option<SharedAppHost>,
    kill: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let runtime = match AsyncRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            drain_with_error_async(&mut rx, format!("rquickjs AsyncRuntime init failed: {e}")).await;
            return;
        }
    };

    // Phase-4 watchdog (D1 + D3, invariant 6): per-Runtime interrupt handler. Runs on
    // THIS thread inside an executing JS frame every ~10k poll points, forever —
    // relaxed atomic loads + thread-local reads ONLY; never call back into the runtime.
    // Returns true → quickjs-ng raises UNCATCHABLE InternalError: interrupted.
    {
        let kill = kill.clone();
        runtime
            .set_interrupt_handler(Some(Box::new(move || {
                // Global shutdown kill flag (condition 1, D1): set by shutdown() BEFORE
                // sending Job::Shutdown so a wedged sync-loop thread can be unblocked.
                if kill.load(std::sync::atomic::Ordering::Relaxed) {
                    return true;
                }
                // Per-hook cancel (condition 6, D3): attribute the on-stack JS slice
                // to the currently-polled hook via CURRENT_GEN, then check its token.
                // CAUTION: Some(gen) does NOT mean this JS belongs to that hook —
                // WithFuture::poll can pump sibling plugin slices under this gen
                // (see CURRENT_GEN doc-comment above). Mitigated by biased select in
                // the hook task (cancelled-arm-first, so a cancelled hook is never
                // re-polled); residual: one innocent sibling slice may be interrupted
                // (thrown "interrupted" is swallowed by web_shims' `let _ = cb.call`,
                // no poisoning of the sibling — accepted gap, D1).
                CURRENT_GEN.with(|c| c.get()).is_some_and(|g| {
                    HOOK_TOKENS.with(|t| {
                        t.borrow().get(&g).is_some_and(|tok| tok.is_cancelled())
                    })
                })
            })))
            .await;
    }

    // NO resident drive() task — see the module-level "drive-loop resolution" note.
    // Instead each loop iteration `select!`s over `rx.recv()` AND `runtime.drive()`,
    // so the schedular is pumped (releasing the lock between ticks) while we await the
    // next job. A `DispatchHook` runs on its own `spawn_local` task; a `DeliverEvent`
    // runs INLINE so it lands the moment the loop dequeues it (it cannot be a
    // `spawn_local` — `drive()` pumps the rquickjs schedular, not the tokio LocalSet,
    // so a spawned deliver task would stall).
    let bridge = DispatchBridge::new(dispatch_tx);
    let mut plugins: HashMap<String, PluginRuntime> = HashMap::new();
    // Runtimes evicted while a hook was in flight (condition 4, D2): kept alive
    // UNTOUCHED (timers keep running) until their in_flight count hits 0, then
    // torn down at the loop-top drain.
    let mut retired: Vec<PluginRuntime> = Vec::new();
    // Monotonically-increasing generation counter: each DispatchHook gets a unique
    // gen stamped by GenScoped so the interrupt handler can attribute on-stack JS
    // to its cancellable hook (condition 6, D3).
    let mut next_hook_gen: u64 = 0;

    loop {
        // D3 hygiene: retire poisoned runtimes (cancelled/interrupted hooks leave
        // half-mutated JS state); next dispatch re-inits a fresh Context.
        let poisoned_names: Vec<String> = plugins
            .iter()
            .filter(|(_, pr)| pr.poisoned.get())
            .map(|(n, _)| n.clone())
            .collect();
        for name in poisoned_names {
            if let Some(pr) = plugins.remove(&name) {
                if pr.in_flight.get() > 0 {
                    retired.push(pr);
                } else {
                    teardown_one(&pr).await;
                }
            }
        }
        drain_retired(&mut retired).await;
        let job = tokio::select! {
            // `drive()` returns Poll::Pending after each tick (never Ready while the
            // runtime is alive), releasing the lock so a parked hook task can progress.
            // It loses the select race against a ready job, then re-arms next iteration.
            _ = runtime.drive() => continue,
            maybe_job = rx.recv() => match maybe_job {
                Some(job) => job,
                None => break, // all job-sender handles dropped
            },
        };
        match job {
            Job::DispatchHook {
                plugin,
                bundle,
                global_name,
                hook,
                context_json,
                project_path,
                moss_dir,
                reply,
                cancel,
            } => {
                let fp = bundle_fingerprint(&bundle);
                if plugins.get(&plugin).map(|pr| pr.fingerprint != fp).unwrap_or(false) {
                    if let Some(pr) = plugins.remove(&plugin) {
                        if pr.in_flight.get() > 0 {
                            // Retire, don't kill (condition 4, D2): the in-flight
                            // hook keeps its Context + timers and finishes naturally;
                            // teardown happens at the loop-top drain once quiesced.
                            retired.push(pr);
                        } else {
                            teardown_one(&pr).await;
                        }
                    }
                }
                // D3 hygiene (eager path): if the existing Context is poisoned
                // (half-mutated JS state from a cancelled/interrupted hook), retire it
                // now at dispatch time.  The loop-top poison sweep (above) is the
                // primary path when no further dispatch arrives; this check covers the
                // race where a new dispatch arrives before the loop-top runs again
                // (i.e., the recv() arm won the select INSIDE the same loop iteration
                // in which the hook task marked poisoned while the engine was parked).
                if plugins.get(&plugin).map(|pr| pr.poisoned.get()).unwrap_or(false) {
                    if let Some(pr) = plugins.remove(&plugin) {
                        if pr.in_flight.get() > 0 {
                            retired.push(pr);
                        } else {
                            teardown_one(&pr).await;
                        }
                    }
                }
                // Get-or-create the per-plugin Context, installing every shim + the
                // bundle ONCE on first sight.
                if !plugins.contains_key(&plugin) {
                    let ctx = match AsyncContext::full(&runtime).await {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = reply.send(Err(format!("context init failed: {e}")));
                            continue;
                        }
                    };
                    let timers = Rc::new(RefCell::new(self::web_shims::TimerRegistry::default()));
                    let hub = Rc::new(RefCell::new(self::tauri_bridge::EventHub::default()));
                    // Clone for the init block: project_path is also used by run_hook
                    // later in this arm, so the outer binding must stay alive.
                    let project_path_for_init = project_path.clone();
                    let init: Result<(), String> = async_with!(ctx => |ctx| {
                        // Real bundles + moss-api address globals via `window.`
                        // (sdk-bundles probe R1). A bare alias — the SDK only ever
                        // writes window.mossApi inside panel HTML, never in engine code.
                        ctx.eval::<(), _>("globalThis.window = globalThis")
                            .map_err(|e| format!("init shim error: {e}"))?;
                        self::web_shims::install_console(&ctx)
                            .map_err(|e| format!("install_console error: {e}"))?;
                        self::web_shims::install_base64(&ctx)
                            .map_err(|e| format!("install_base64 error: {e}"))?;
                        self::web_shims::install_crypto(&ctx)
                            .map_err(|e| format!("install_crypto error: {e}"))?;
                        self::web_shims::install_text_codec(&ctx)
                            .map_err(|e| format!("install_text_codec error: {e}"))?;
                        self::web_shims::install_timers(&ctx, timers.clone())
                            .map_err(|e| format!("install_timers error: {e}"))?;
                        self::url_shim::install_url(&ctx)
                            .map_err(|e| format!("install_url error: {e}"))?;
                        self::fetch_shim::install_fetch(&ctx)
                            .map_err(|e| format!("install_fetch error: {e}"))?;
                        self::host_fns::install_invoke(&ctx, bridge.clone(), &plugin)
                            .map_err(|e| format!("install_invoke error: {e}"))?;
                        self::tauri_bridge::install_event(
                            &ctx,
                            app_for_engine.clone(),
                            hub.clone(),
                            bridge.clone(),
                            &plugin,
                        )
                        .map_err(|e| format!("install_event error: {e}"))?;
                        // Test-only panic trigger: exercises the hook-task catch_unwind path
                        // (A.5). Never compiled into release builds.
                        #[cfg(test)]
                        ctx.globals().set(
                            "__moss_test_panic",
                            rquickjs::prelude::Func::from(|| -> bool { panic!("deliberate test panic (A.5)") }),
                        ).map_err(|e| format!("install __moss_test_panic error: {e}"))?;
                        // Bundle eval: catch the JS exception so the error message carries
                        // the actual throw text rather than the opaque "Exception generated
                        // by QuickJS" from rquickjs::Error::Exception.
                        if let Err(e) = self::loader::load_bundle(&ctx, &bundle, &global_name) {
                            if matches!(e, rquickjs::Error::Exception) {
                                let caught = ctx.catch();
                                return Err(format!("{caught:?}"));
                            }
                            return Err(format!("{e}"));
                        }
                        // Condition 2 (D5): webview parity — onload once, awaited,
                        // throw = init failure through the cleanup below. Inline
                        // await is sound: WithFuture::poll pumps the schedular
                        // (rquickjs context/async/future.rs, verified).
                        if let Err(e) = self::loader::call_onload(&ctx, &project_path_for_init).await {
                            return Err(format!("onload error: {e}"));
                        }
                        Ok::<(), String>(())
                    })
                    .await
                    .map_err(|e| format!("plugin {plugin} init error: {e}"));
                    if let Err(e) = init {
                        // Mirror teardown_plugins' order for the half-built Context:
                        // cancel timers, clear this Context's thread-local TIMERS entry
                        // (raw-pointer keyed — a leaked entry gets silently ADOPTED by a
                        // future Context allocated at the same address), drop Persistents
                        // while the runtime is alive, THEN drop the Context.
                        timers.borrow_mut().cancel_all();
                        let _ = async_with!(ctx => |ctx| {
                            self::web_shims::clear_context_timers(&ctx);
                        })
                        .await;
                        hub.borrow_mut().listeners.clear();
                        let _ = reply.send(Err(e));
                        continue;
                    }
                    plugins.insert(
                        plugin.clone(),
                        PluginRuntime {
                            ctx,
                            timers,
                            hub,
                            fingerprint: fp,
                            in_flight: Rc::new(Cell::new(0)),
                            poisoned: Rc::new(Cell::new(false)),
                        },
                    );
                }

                let pr = plugins.get(&plugin).unwrap();
                let ctx = pr.ctx.clone();
                // Track the live hook on its runtime (condition 4, D2): incremented
                // BEFORE spawn_local, decremented as the task's LAST statement — so
                // an eviction arriving mid-hook sees in_flight > 0 and retires.
                let in_flight = pr.in_flight.clone();
                let poisoned = pr.poisoned.clone();
                in_flight.set(in_flight.get() + 1);
                // Per-hook generation (condition 6, D3): assign a unique gen and
                // register the cancel token so the interrupt handler can reach it.
                next_hook_gen += 1;
                let gen = next_hook_gen;
                HOOK_TOKENS.with(|t| t.borrow_mut().insert(gen, cancel.clone()));
                let hook_for_task = hook.clone();
                // Run the hook on its OWN spawn_local task (do NOT await inline) so a
                // later DeliverEvent can be dequeued while the hook is parked in its
                // poll loop. The hook's in-hook timer/fetch futures are pumped by the
                // loop's `runtime.drive()` (it polls the schedular every select tick),
                // and the LocalSet executor polls this hook task. No `idle()` here:
                // `idle()` would park HOLDING the runtime lock and starve this very task.
                tokio::task::spawn_local(async move {
                    use futures::FutureExt;
                    let hook_fut = std::panic::AssertUnwindSafe(GenScoped::new(
                        gen,
                        run_hook(&ctx, &plugin, &hook_for_task, &context_json, &project_path, &moss_dir),
                    ))
                    .catch_unwind();
                    let result = tokio::select! {
                        // biased + cancelled-arm-FIRST is load-bearing (D1 false-positive
                        // direction): polling hook_fut pumps the runtime-wide schedular
                        // under THIS hook's gen, so re-polling a cancelled hook risks the
                        // interrupt handler killing an innocent sibling's JS slice. Once
                        // cancelled, never poll hook_fut again.
                        biased;
                        // Parked case: the hook is awaiting a JS promise/timer and the
                        // cancel arm fires promptly. The wedged sync-loop case never
                        // reaches this select — the interrupt handler unwinds it first,
                        // then this arm fires on the next re-poll.
                        _ = cancel.cancelled() => Err(format!("hook {hook_for_task} cancelled by host")),
                        r = hook_fut => r.unwrap_or_else(|panic| {
                            let msg = panic
                                .downcast_ref::<&str>()
                                .map(|s| s.to_string())
                                .or_else(|| panic.downcast_ref::<String>().cloned())
                                .unwrap_or_else(|| "non-string panic payload".into());
                            log::error!(target: "plugin", "hook {hook_for_task} PANICKED: {msg}");
                            Err(format!("hook {hook_for_task} panicked: {msg}"))
                        }),
                    };
                    if cancel.is_cancelled() {
                        // D3 hygiene: half-mutated JS state — retire this Context at
                        // the next loop-top drain (rides D2's retire machinery; a
                        // kill-style evict would orphan sibling in-flight hooks).
                        poisoned.set(true);
                    }
                    HOOK_TOKENS.with(|t| { t.borrow_mut().remove(&gen); });
                    let _ = reply.send(result);
                    in_flight.set(in_flight.get() - 1);
                });
            }

            Job::DeliverEvent {
                plugin,
                name,
                payload_json,
            } => {
                if let Some(pr) = plugins.get(&plugin) {
                    let ctx = pr.ctx.clone();
                    let hub = pr.hub.clone();
                    // Deliver INLINE (not on a spawn_local task): `runtime.drive()` only
                    // pumps the rquickjs schedular, not the tokio LocalSet, so a spawned
                    // deliver task would stall. `deliver` is quick (restore Persistent +
                    // call handler), so the brief inline await is fine and lands the
                    // event the instant the loop dequeues it — while the hook is parked.
                    let _ = async_with!(ctx => |ctx| {
                        self::tauri_bridge::deliver(&ctx, &hub, &name, &payload_json)
                    })
                    .await;
                }
            }

            #[cfg(any(test, feature = "test-fixtures"))]
            Job::PanicForTest => panic!("deliberate engine-thread death (P4 condition 5 test)"),

            Job::Shutdown { reply } => {
                // Retirees too — force teardown regardless of in_flight (their
                // parked hooks die with the runtime; replies drop → the adapter's
                // "dropped the hook without replying" sentinel fires, which is loud).
                for pr in retired.drain(..) {
                    teardown_one(&pr).await;
                }
                teardown_plugins(&mut plugins).await;
                // Close the job channel BEFORE acking (Phase-4 condition 1): once
                // `shutdown()` returns, a late dispatch must fail fast at send time
                // with the never-accepted sentinel ("plugin engine thread has
                // exited") instead of racing the thread's exit — a send landing in
                // the ack→thread-exit window would otherwise be queued and dropped,
                // yielding the ambiguous "dropped the hook without replying" (the
                // D4 sentinel for a possibly-half-executed hook, which this is not).
                rx.close();
                let _ = reply.send(());
                break;
            }
        }
    }

    // Channel closed without an explicit Shutdown (all handles dropped) — still
    // cancel timers so no orphaned tokio tasks linger.
    // Retirees too — force teardown regardless of in_flight (their
    // parked hooks die with the runtime; replies drop → the adapter's
    // "dropped the hook without replying" sentinel fires, which is loud).
    for pr in retired.drain(..) {
        teardown_one(&pr).await;
    }
    teardown_plugins(&mut plugins).await;
}

/// Run one hook on its plugin Context and marshal the resolved return value to JSON.
/// All JS handles stay inside the `async_with!` closure; only an owned `String`
/// crosses back out.
async fn run_hook(
    ctx: &AsyncContext,
    plugin: &str,
    hook: &str,
    context_json: &str,
    project_path: &str,
    moss_dir: &str,
) -> Result<String, String> {
    // async_with! returns Result<T, rquickjs::Error>. rquickjs::Error::Exception
    // renders as the opaque "Exception generated by QuickJS" — unhelpful for plugin
    // diagnostics. We intercept Exception inside the closure (where ctx is accessible)
    // via ctx.catch() to extract the actual JS exception message. The closure returns
    // Result<String, String> (not rquickjs::Error) so the enriched message survives
    // the outer map_err. Same pattern as bundle eval (engine.rs:351-358).
    let hook_name = hook.to_string();
    let hook_for_err = hook.to_string();
    async_with!(ctx => |ctx| {
        // Inner async block returns Result<String, String> with enriched error messages.
        let inner: Result<String, String> = async {
            self::loader::set_internal_context(&ctx, plugin, project_path, moss_dir)
                .map_err(|e| format!("{e}"))?;
            let instance: Object = ctx.globals().get("__moss_loaded")
                .map_err(|e| format!("{e}"))?;
            // Missing-hook sentinel (pre-merge review Fix 2): fetch the property as a
            // Value first — an absent hook reads as `undefined` (Ok), and a non-function
            // would FromJs-fail with an rquickjs message the manager can't match. The
            // sentinel substring MUST stay byte-identical to the webview runtime's
            // (plugin-runtime.ts:312): manager.rs `execute_configure_domain` matches
            // `e.contains("Hook function not found")` to gracefully skip deploy
            // plugins that don't implement the hook.
            let hook_val: Value = instance.get(&hook_name as &str)
                .map_err(|e| format!("{e}"))?;
            let hook_fn: Function = hook_val
                .into_function()
                .ok_or_else(|| format!("Hook function not found: {hook_name}"))?;
            let arg: Value = ctx.json_parse(context_json)
                .map_err(|e| format!("{e}"))?;
            let ret: Value = match hook_fn.call((arg,)) {
                Ok(v) => v,
                Err(e) => {
                    // Sync-phase throw OR interrupt: same ctx.catch() enrichment as
                    // the promise branch — without it an interrupt here is opaque
                    // (engine-kill-respawn probe Facts §1).
                    if matches!(e, rquickjs::Error::Exception) {
                        let caught = ctx.catch();
                        return Err(format!("{caught:?}"));
                    }
                    return Err(format!("{e}"));
                }
            };
            // MaybePromise::from_value returns Self (NOT a Result) — no `?`. Awaiting it
            // drives this Context's WithFuture, which is what fires in-hook timers/fetch.
            let mp = rquickjs::promise::MaybePromise::from_value(ret);
            let resolved = match mp.into_future().await {
                Ok(v) => v,
                Err(e) => {
                    // Promise rejection: rquickjs::Error::Exception renders as the opaque
                    // "Exception generated by QuickJS". ctx.catch() retrieves the actual JS
                    // exception value (spec §7 parity: browsers surface the rejection
                    // message; same pattern used at bundle eval time, engine.rs:351-358).
                    if matches!(e, rquickjs::Error::Exception) {
                        let caught = ctx.catch();
                        return Err(format!("{caught:?}"));
                    }
                    return Err(format!("{e}"));
                }
            };
            let json = self::marshal::js_to_json(&ctx, resolved)
                .map_err(|e| format!("{e}"))?;
            self::loader::clear_internal_context(&ctx)
                .map_err(|e| format!("{e}"))?;
            Ok(json.to_string())
        }
        .await;
        Ok::<Result<String, String>, rquickjs::Error>(inner)
    })
    .await
    .map_err(|e| format!("hook {hook_for_err} error: {e}"))
    .and_then(|inner| inner.map_err(|e| format!("hook {hook_for_err} error: {e}")))
}

/// Per-plugin teardown, ordered so the runtime is freed cleanly (no quickjs-ng
/// `gc_obj_list` assertion at `JS_FreeRuntime`):
/// 1. Cancel timers + clear the per-Context timer-registry map entry.
/// 2. Clear the EventHub's `Persistent<Function>` listeners — DROPPING them WHILE THE
///    RUNTIME IS STILL ALIVE so each `Persistent::drop` unroots from the live runtime
///    (a `Persistent` dropped after the runtime is freed leaves its JS object rooted
///    in `gc_obj_list` → the abort). This is why the clear happens here, before the
///    Contexts drop in `plugins.clear()`.
/// 3. Drop all Contexts (`plugins.clear()`).
async fn teardown_plugins(plugins: &mut HashMap<String, PluginRuntime>) {
    for pr in plugins.values() {
        teardown_one(pr).await;
    }
    plugins.clear();
}

/// Reply to every queued + future job with the same fatal init error (sync, before
/// the runtime exists).
fn drain_with_error(mut rx: mpsc::UnboundedReceiver<Job>, err: String) {
    while let Ok(job) = rx.try_recv() {
        reply_fatal(job, &err);
    }
    rx.close();
}

/// Async variant (inside the runtime).
async fn drain_with_error_async(rx: &mut mpsc::UnboundedReceiver<Job>, err: String) {
    while let Some(job) = rx.recv().await {
        reply_fatal(job, &err);
    }
}

fn reply_fatal(job: Job, err: &str) {
    match job {
        Job::DispatchHook { reply, .. } => {
            let _ = reply.send(Err(err.to_string()));
        }
        Job::DeliverEvent { .. } => {}
        Job::Shutdown { reply } => {
            let _ = reply.send(());
        }
        #[cfg(any(test, feature = "test-fixtures"))]
        Job::PanicForTest => {}
    }
}

#[cfg(test)]
mod acceptance;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
