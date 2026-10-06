//! Engine abstraction for plugin hook dispatch (Phase 2).
//!
//! Abstracts ONLY the dispatch primitive (eval_to_runtime, manager.rs:365).
//! PluginHookState plumbing stays host-side and engine-agnostic (signal_route).
//! MUST be Send + Sync; returned futures MUST be Send — the build pipeline awaits
//! them on the process's one runtime through the [`Spawner`] port. The engine OWNS
//! any !Send runtime behind a thread/channel boundary (engine.rs) so the handle is Send.
//!
//! Nothing here names tauri: the desktop arrives as [`AdapterHost`] — the shared
//! [`AppHost`] seam, the injected [`HostState`], the reporter and the spawner.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use crate::engine::app_host::ListenerId;
use crate::plugins::adapter_host::AdapterHost;

#[cfg(any(test, feature = "test-fixtures"))]
use crate::plugins::hook_state::PluginHookState;
use crate::engine::host_fns::dispatch_command;
use crate::engine::QuickJsEngine;
use super::signal_route::HookSignal;

/// Cloneable handle the engine uses to emit a hook's signal stream. Forwards into
/// route_signal on the host side. The plugin emits via the
/// `plugin_message` command (IPC); for QuickJsEngine the host-binding shim calls
/// this directly (no IPC).
#[derive(Clone)]
pub struct HookSignalSink {
    inner: std::sync::Arc<dyn Fn(HookSignal) + Send + Sync>,
}

impl HookSignalSink {
    pub fn new(f: impl Fn(HookSignal) + Send + Sync + 'static) -> Self {
        Self { inner: std::sync::Arc::new(f) }
    }
    pub fn emit(&self, signal: HookSignal) {
        (self.inner)(signal);
    }
}

/// Everything one hook dispatch needs. project_path/moss_dir feed
/// __MOSS_INTERNAL_CONTEXT__ on the quickjs path (loader::set_internal_context);
/// the webview path keeps reading project_path out of the context JSON (the
/// manager still injects it there — one authoritative channel per engine).
#[derive(Clone, Debug)]
pub struct HookInvocation {
    pub plugin_name: String,
    pub hook_name: String,
    pub context: serde_json::Value,
    pub project_path: String,
    pub moss_dir: String,
}

#[async_trait]
pub trait PluginEngine: Send + Sync {
    /// Dispatch the hook described by `invocation`. Returns when dispatch is
    /// ACCEPTED; the existing oneshot/select! in execute_plugin_javascript owns
    /// completion. Progress/log/error/complete flow through `sink`.
    async fn dispatch_hook(&self, invocation: HookInvocation, sink: HookSignalSink) -> Result<(), String>;

    /// Teardown: cancel drive(), abort timers, drop Contexts.
    async fn teardown(&self);

    /// Best-effort kill of an in-flight hook (Phase-4 condition 6, D3). Default
    /// no-op: an engine without a per-hook kill lets its runaway JS die with
    /// webview destroy at folder close — parity). QuickJsEngineAdapter cancels the
    /// dispatch's CancellationToken, killing the hook parked OR wedged in sync JS.
    fn cancel_hook(&self, _plugin_name: &str, _hook_name: &str) {}
}


/// One loadable plugin bundle: the IIFE source + the global the instance binds to.
#[derive(Clone, Debug)]
pub struct PluginBundle {
    pub bundle: String,
    pub global_name: String,
}

/// Live bundle lookup, owned by the manager side. Reads FRESH from the installed
/// plugin dir on every dispatch (memory rule: prefer fresh reads) — the same
/// <project>/.moss/plugins/<name>/<entry> path the webview init reads
/// (manager.rs:827-829), so both engines execute the SAME bytes.
pub type BundleSource = Arc<dyn Fn(&str) -> Result<PluginBundle, String> + Send + Sync>;

/// Per-dispatch sink entry. `generation` disambiguates an orphaned dispatch
/// (cancelled by the manager's inactivity watchdog, which calls `cancel_hook`
/// and stops waiting — the engine-side hook keeps running) from a fresh
/// re-dispatch of the same (plugin, hook).
///
/// BE PRECISE ABOUT WHAT THE TAG DOES AND DOES NOT GUARANTEE (adapter-manager
/// probe risks 5–6; do not overstate this in derived doc-comments):
/// - It DOES guarantee the orphan's completion task cannot REMOVE the live
///   dispatch's sink entry — without the tag, that removal would silently drop
///   the new dispatch's heartbeats and earn a spurious 60s inactivity timeout.
/// - It does NOT stop the orphan's in-flight plugin_message progress/error
///   signals from landing in the NEW dispatch's sink: JS messages carry no
///   generation, so the (plugin, hook) map lookup routes them to whichever
///   entry is live.
/// - It does NOT stop the orphan's terminal Complete: that is emitted on the
///   orphan's own CAPTURED sink (bypassing the map and this tag entirely), and
///   PluginHookState::send_completion is first-wins (commands.rs:242-254) — an
///   orphan finishing first hands the manager the orphan's result.
/// Both residual hazards are WEBVIEW PARITY: the webview path's
/// (plugin, hook)-keyed plugin_message routing + send_completion has the
/// identical behavior. True engine-side hook cancellation (which kills the
/// orphan instead of racing it) is the Phase-4 CancellationToken item.
struct SinkEntry {
    generation: u64,
    sink: HookSignalSink,
    /// CancellationToken for this dispatch (Phase-4 condition 6, D3). Cancelled
    /// by `cancel_hook` on the adapter; the engine-side select! arm and interrupt
    /// handler propagate the cancel to parked and sync-wedged hooks respectively.
    cancel: tokio_util::sync::CancellationToken,
}

type SinkMap = Arc<Mutex<HashMap<(String, String), SinkEntry>>>;

/// Host-side registry of bridged `app.listen` registrations, keyed by
/// (plugin, event name) — ONE forwarding listener per pair, installed on the
/// first `__engine_listen__` from that plugin's JS `listen(name)` call.
///
/// Lifetime note (known gap, Phase-3 accepted): bridged registrations live
/// until adapter TEARDOWN, not per-JS-unlisten — bounded by the handful of
/// event names plugins use (`browser-closed`, `github:deploy-choice`,
/// `github:auth-cancel`). A post-unlisten forward is dropped harmlessly by
/// `deliver` (no hub entry).
type BridgedListeners = Arc<Mutex<HashMap<(String, String), ListenerId>>>;

/// Engine slot with a respawn generation (condition 5, D4). Swapped ONLY by
/// `respawn_engine` (idempotent via the generation compare); std Mutex, NEVER
/// held across an await. Lock order where both are taken: slot THEN bridged.
struct EngineSlot {
    engine: QuickJsEngine,
    respawn_gen: u64,
    /// Teardown tombstone (D4): set by `teardown()` under this lock BEFORE
    /// `engine.shutdown()`; `respawn_engine` refuses when set. Without it a
    /// NORMAL folder close with an in-flight hook respawns an engine nothing
    /// tears down, and a dispatch racing teardown retries — and executes —
    /// after the folder closed.
    torn_down: bool,
}

/// `PluginEngine` over the off-webview [`QuickJsEngine`].
///
/// On construction it spawns ONE host dispatch task that drains the engine's
/// `DispatchRequest` receiver. That task is **sink-aware**: a `plugin_message`
/// command is NOT a
/// host command — it is translated into the matching [`HookSignal`] and emitted on
/// the in-flight dispatch's sink, selected from the `(plugin, hook)`-keyed map.
/// Every other command routes to the real host dispatcher (production) or a stub
/// (test).
pub struct QuickJsEngineAdapter {
    /// Engine slot behind a lock for adapter-internal respawn (condition 5, D4).
    /// std Mutex, never held across an await.
    engine: Arc<Mutex<EngineSlot>>,
    bundles: BundleSource,
    /// Per-manager project path fed to `EngineHost` in `spawn_host_dispatch` so
    /// command arms resolve project-relative paths via the per-manager authority (D1)
    /// rather than the JS `projectPath` arg (webview-command parity: AppState there,
    /// per-manager `project_path` here).
    project_path: String,
    sinks: SinkMap,
    next_generation: Arc<std::sync::atomic::AtomicU64>,
    /// The manager's host record, cloned in: the state every host call stamps,
    /// the desktop (if any) the app-only arms and the `listen` bridge reach,
    /// and the runtime the dispatch tasks run on. Held so a respawned dispatch
    /// task carries the SAME registries the dead one did.
    host: AdapterHost,
    /// Shared with the manager: the capabilities each discovered plugin declared.
    /// Resolving a grant here (not from disk in the command arm) is what keeps a
    /// runtime manifest rewrite from widening what a plugin may do.
    grants: crate::engine::host_fns::GrantRegistry,
    /// Bridged host→engine listen registrations (Task 11b) — shared with the
    /// host dispatch task, drained at teardown.
    bridged: BridgedListeners,
}

impl QuickJsEngineAdapter {
    /// Production ctor. Spawns the host dispatch task wired to the real
    /// [`dispatch_command`]. `host.app` is `None` on a headless build,
    /// where the app-only arms refuse by name. `bundles` is the live
    /// [`BundleSource`] (fresh read per dispatch — `bundle_source_for_project`).
    pub fn new(
        host: AdapterHost,
        bundles: BundleSource,
        project_path: String,
        grants: crate::engine::host_fns::GrantRegistry,
    ) -> Self {
        let (engine, dispatch_rx) = QuickJsEngine::new(host.app.clone());
        let sinks: SinkMap = Arc::new(Mutex::new(HashMap::new()));
        let bridged: BridgedListeners = Arc::new(Mutex::new(HashMap::new()));
        let slot = Arc::new(Mutex::new(EngineSlot { engine: engine.clone(), respawn_gen: 0, torn_down: false }));
        spawn_host_dispatch(
            dispatch_rx,
            sinks.clone(),
            host.clone(),
            project_path.clone(),
            Some(engine.clone()),
            bridged.clone(),
            grants.clone(),
        );
        Self {
            engine: slot,
            bundles,
            project_path,
            sinks,
            next_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            host,
            bridged,
            grants,
        }
    }

    /// Test ctor (Task 14b): no `AppHandle`, no manager. Carries ONE inline bundle
    /// registered under plugin name `"p"`, and spawns a sink-aware dispatch task.
    /// That task runs the REAL command arms against a standalone
    /// `HostState` — not a narrowed stub — so a test here reaches the same code a
    /// headless `moss build` does.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn new_for_test(bundle: &str, global_name: &str) -> Self {
        Self::new_for_test_multi(vec![("p", bundle, global_name)])
    }

    /// Test ctor for MULTIPLE inline bundles: `(plugin_name, bundle, global_name)`
    /// triples. No `AppHandle`; non-`plugin_message` commands run the real arms
    /// against a standalone `HostState`. Placeholder project path.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn new_for_test_multi(bundles: Vec<(&str, &str, &str)>) -> Self {
        Self::new_for_test_multi_in("/tmp/p", bundles)
    }

    /// [`new_for_test_multi`] with an explicit `project_path` (Task-18 gate test 2
    /// dispatches project-file command arms against a real temp project).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn new_for_test_multi_in(project_path: &str, bundles: Vec<(&str, &str, &str)>) -> Self {
        let (engine, dispatch_rx) = QuickJsEngine::new(None);
        let sinks: SinkMap = Arc::new(Mutex::new(HashMap::new()));
        let bridged: BridgedListeners = Arc::new(Mutex::new(HashMap::new()));
        let slot = Arc::new(Mutex::new(EngineSlot { engine: engine.clone(), respawn_gen: 0, torn_down: false }));
        // No desktop → no engine handle either: the __engine_listen__ arm needs
        // BOTH to register a forwarding listen; in tests it replies null only.
        let host = AdapterHost::headless(crate::build::ports::reporter::discard_owned());
        spawn_host_dispatch(
            dispatch_rx,
            sinks.clone(),
            host.clone(),
            project_path.to_string(),
            None,
            bridged.clone(),
            crate::engine::host_fns::GrantRegistry::default(),
        );
        let map: HashMap<String, PluginBundle> = bundles
            .into_iter()
            .map(|(name, bundle, global_name)| {
                (
                    name.to_string(),
                    PluginBundle {
                        bundle: bundle.to_string(),
                        global_name: global_name.to_string(),
                    },
                )
            })
            .collect();
        let bundles: BundleSource = Arc::new(move |plugin_name: &str| {
            map.get(plugin_name)
                .cloned()
                .ok_or_else(|| format!("no bundle registered for plugin '{plugin_name}'"))
        });
        Self {
            engine: slot,
            bundles,
            project_path: project_path.to_string(),
            sinks,
            next_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            host,
            bridged,
            grants: crate::engine::host_fns::GrantRegistry::default(),
        }
    }

    /// Test-only: the hook state this adapter's host dispatch stamps, so a test
    /// can observe the in-flight host-call count the watchdog reads.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn hook_state_for_test(&self) -> PluginHookState {
        self.host.state.hooks.clone()
    }

    /// Test-only: return a clone of the current engine handle so tests can kill it.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn engine_handle_for_test(&self) -> QuickJsEngine {
        self.engine.lock().unwrap().engine.clone()
    }
}

/// Idempotent respawn: swap in a fresh QuickJsEngine + host dispatch task, and
/// unlisten + clear every bridged forwarder — they captured the DEAD engine, and
/// their (plugin, name) dedup keys would block re-registration against the new
/// one (listeners permanently dead otherwise — engine-kill-respawn probe §5.3).
fn respawn_engine(
    slot: &Arc<Mutex<EngineSlot>>,
    observed_gen: u64,
    host: &AdapterHost,
    sinks: &SinkMap,
    project_path: &str,
    bridged: &BridgedListeners,
    grants: &crate::engine::host_fns::GrantRegistry,
) {
    let mut s = slot.lock().unwrap();
    if s.respawn_gen != observed_gen {
        return; // a concurrent failed dispatch already respawned
    }
    if s.torn_down {
        return; // adapter torn down (folder close) — NEVER respawn into a dead adapter (D4 tombstone)
    }
    if s.respawn_gen >= 8 {
        log::error!(
            target: "plugin",
            "plugin engine died {} times this session — giving up on respawn \
             (deterministic startup failure?); close and reopen the folder",
            s.respawn_gen
        );
        return; // D4 respawn cap: no per-dispatch respawn+log storm
    }
    log::error!(
        target: "plugin",
        "plugin engine thread died — respawning (in-flight hooks and JS state of the old engine are lost)"
    );
    let ids: Vec<ListenerId> = bridged.lock().unwrap().drain().map(|(_, id)| id).collect();
    if let Some(app) = host.app.as_ref() {
        for id in ids { app.unlisten(id); }
    }
    let (engine, dispatch_rx) = QuickJsEngine::new(host.app.clone());
    spawn_host_dispatch(
        dispatch_rx,
        sinks.clone(),
        host.clone(),
        project_path.to_string(),
        Some(engine.clone()),
        bridged.clone(),
        grants.clone(),
    );
    s.engine = engine;
    s.respawn_gen += 1;
}

/// Spawn the host dispatch task. Every command runs the real
/// [`dispatch_command`] against `host.state`. A `plugin_message`
/// command is intercepted, translated to a [`HookSignal`], and emitted on the
/// sink the `(plugin, hook)`-keyed map holds for that in-flight dispatch.
///
/// `project_path` feeds `EngineHost` so the command arms resolve project-relative
/// paths via the per-manager authority (D1) rather than the JS `projectPath` arg.
///
/// Task 11b: an `__engine_listen__` request (fired by the engine-side `listen`
/// shim) registers ONE forwarding [`AppHost::listen`] per (plugin, name) that
/// feeds the app's event bus into `engine.deliver_event`. `engine`/`bridged`
/// are `None`/unused on the test path (no desktop → nothing to listen on).
fn spawn_host_dispatch(
    mut dispatch_rx: tokio::sync::mpsc::UnboundedReceiver<
        crate::engine::host_fns::DispatchRequest,
    >,
    sinks: SinkMap,
    host: AdapterHost,
    project_path: String,
    engine: Option<QuickJsEngine>,
    bridged: BridgedListeners,
    grants: crate::engine::host_fns::GrantRegistry,
) {
    drop(host.spawner.clone().spawn(Box::pin(async move {
        while let Some(req) = dispatch_rx.recv().await {
            // The host's own in-flight work counts as plugin activity. A slow arm
            // — github deploy's `execute_binary` git push, a large upload —
            // emits no plugin signal, so the manager's 60 s inactivity watchdog
            // used to kill a hook whose work WAS advancing; that is the whole
            // reason first-party plugins ship fake heartbeats. The guard is held
            // for the call's lifetime and restamps the clock on drop, so the
            // timeout measures silence AFTER the host finished, not during it.
            // The inline arms below drop it immediately, which is correct: they
            // ARE instantaneous.
            let busy = req
                .plugin
                .as_deref()
                .map(|p| host.state.hooks.begin_host_call(p));
            // Test-only slow command: the head-of-line-blocking gate
            // (`heartbeat_flows_during_blocking_command`) needs a generic host command
            // that parks ~300ms, standing in for github deploy's `execute_binary`
            // git push.
            #[cfg(test)]
            if req.cmd == "__test_slow__" {
                let ms = serde_json::from_str::<serde_json::Value>(&req.args_json)
                    .ok()
                    .and_then(|v| v.get("ms").and_then(|m| m.as_u64()))
                    .unwrap_or(300);
                tokio::spawn(async move {
                    let _busy = busy;
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                    let _ = req.reply.send(Ok("null".to_string()));
                });
                continue;
            }
            if req.cmd == "__engine_listen__" {
                if let (Some(app), Some(engine)) = (host.app.as_ref(), engine.as_ref()) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&req.args_json) {
                        let plugin = v.get("plugin").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_string();
                        let key = (plugin.clone(), name.clone());
                        let mut reg = bridged.lock().unwrap();
                        if !plugin.is_empty() && !name.is_empty() && !reg.contains_key(&key) {
                            let engine = engine.clone();
                            let (p2, n2) = (plugin.clone(), name.clone());
                            let id = app.listen(
                                name.clone(),
                                Box::new(move |payload| engine.deliver_event(&p2, &n2, payload)),
                            );
                            reg.insert(key, id);
                        }
                    }
                }
                let _ = req.reply.send(Ok("null".to_string()));
                continue;
            }
            if req.cmd == "plugin_message" {
                // Translate the JS-emitted plugin_message into a HookSignal on the
                // dispatch's sink. The args JSON is { pluginName, hookName, message }.
                if let Some((plugin, hook, signal)) = parse_plugin_message(&req.args_json) {
                    let sink = sinks.lock().unwrap().get(&(plugin, hook)).map(|e| e.sink.clone());
                    match sink {
                        Some(sink) => sink.emit(signal),
                        None => log::debug!(target: "plugin", "plugin_message with no in-flight sink (late signal?) — dropped"),
                    }
                } else {
                    // Shape-drift diagnostics: a malformed payload would otherwise be
                    // silently acked with null and the signal lost.
                    let snippet: String = req.args_json.chars().take(200).collect();
                    log::warn!(
                        target: "plugin",
                        "{} with unrecognized payload shape — dropped: {snippet}",
                        req.cmd
                    );
                }
                // plugin_message resolves to null (fire-and-forget on the JS side).
                let _ = req.reply.send(Ok("null".to_string()));
                continue;
            }
            // Inline-vs-spawned split (pre-merge review Fix 3): `plugin_message` and
            // `__engine_listen__` above run INLINE because their ORDERING matters —
            // heartbeat/error signals must hit the sink in emit order, and a listen
            // registration must land before events the hook then waits on. The
            // GENERIC arm is SPAWNED for webview concurrency parity: a slow command
            // (github deploy's `execute_binary` git push, timeoutMs up to 600s)
            // awaited inline would head-of-line-block every queued heartbeat behind
            // it and trip the manager's 60s INACTIVITY_TIMEOUT mid-deploy. The
            // per-request captures are cheap owned clones (a handful of Arcs +
            // project_path String); `req` moves in carrying its reply oneshot.
            let app = host.app.clone();
            let state = host.state.clone();
            let project_path = project_path.clone();
            // Resolve the caller's grants from the shared snapshot, NOT from the
            // manifest on disk. Per-dispatch (not once per Context) so a plugin
            // installed after this engine was built is picked up as soon as the
            // manager re-snapshots — an unresolved name simply denies.
            let caller = req.plugin.as_deref().unwrap_or_default();
            let declared = grants.declared_for(caller);
            drop(host.spawner.spawn(Box::pin(async move {
                let _busy = busy;
                let result = dispatch_command(
                    app.as_deref(),
                    &state,
                    &project_path,
                    req.plugin.as_deref(),
                    declared.as_ref(),
                    &req.cmd,
                    &req.args_json,
                )
                .await;
                let _ = req.reply.send(result);
            })));
        }
    })));
}

/// Parse `{ pluginName, hookName, message: { type, .. } }` → (plugin, hook, signal).
/// `None` for unrecognized shapes; the message is the SDK wire shape
/// [`HookSignal`] itself derives.
fn parse_plugin_message(args_json: &str) -> Option<(String, String, HookSignal)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let plugin = v.get("pluginName")?.as_str()?.to_string();
    let hook = v.get("hookName")?.as_str()?.to_string();
    let signal: HookSignal = serde_json::from_value(v.get("message")?.clone()).ok()?;
    Some((plugin, hook, signal))
}

#[async_trait]
impl PluginEngine for QuickJsEngineAdapter {
    async fn dispatch_hook(&self, invocation: HookInvocation, sink: HookSignalSink) -> Result<(), String> {
        let plugin_name = invocation.plugin_name;
        let hook_name = invocation.hook_name;
        let project_path = invocation.project_path;
        let moss_dir = invocation.moss_dir;

        // Live lookup, still BEFORE the spawn — "no bundle / load error" stays an
        // acceptance-time Err (no signal, no spawned completion task).
        let bundle = (self.bundles)(&plugin_name)?;

        // Strip the SDK-internal project_path key before serializing: the manager
        // injects it for the WEBVIEW runtime to extract; on quickjs the loader sets
        // __MOSS_INTERNAL_CONTEXT__ instead. Strip it so the hook receives the same
        // context object (extract-and-strip).
        let mut context = invocation.context;
        if let Some(obj) = context.as_object_mut() {
            obj.remove("project_path");
        }

        let context_json = serde_json::to_string(&context)
            .map_err(|e| format!("serialize hook context: {e}"))?;

        let key = (plugin_name.clone(), hook_name.clone());
        let generation = self
            .next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Phase-4 condition 6 (D3): one CancellationToken per dispatch. Stored in
        // the SinkEntry so cancel_hook can reach it; cloned into the spawn so the
        // completion task forwards it to dispatch_hook_with_cancel on the engine.
        let cancel = tokio_util::sync::CancellationToken::new();
        self.sinks
            .lock()
            .unwrap()
            .insert(key.clone(), SinkEntry { generation, sink: sink.clone(), cancel: cancel.clone() });

        // Spawn the completion await (return-at-ACCEPTANCE — the manager's
        // oneshot/select! owns completion). QuickJsEngine is Clone (two senders);
        // the future is Send (only owned Strings + channel handles cross).
        let slot = self.engine.clone();
        let sinks = self.sinks.clone();
        let bridged = self.bridged.clone();
        let host = self.host.clone();
        let project_path_for_respawn = self.project_path.clone();
        let grants_for_respawn = self.grants.clone();
        let (plugin, hook) = key.clone();
        drop(self.host.spawner.spawn(Box::pin(async move {
            let (engine, observed_gen) = {
                let s = slot.lock().unwrap();
                (s.engine.clone(), s.respawn_gen)
            };
            let mut result = engine
                .dispatch_hook_with_cancel(
                    &plugin,
                    &bundle.bundle,
                    &bundle.global_name,
                    &hook,
                    &context_json,
                    &project_path,
                    &moss_dir,
                    cancel.clone(),
                )
                .await;
            // Sentinel-gated respawn (D4): the EXACT sentinel strings here are
            // matched against engine.rs:249/:252 — a reword there breaks recovery.
            if matches!(&result, Err(e) if e == "plugin engine thread has exited") {
                // Never accepted → side-effect-free → respawn + retry ONCE (D4).
                respawn_engine(&slot, observed_gen, &host, &sinks, &project_path_for_respawn, &bridged, &grants_for_respawn);
                let engine = slot.lock().unwrap().engine.clone();
                result = engine
                    .dispatch_hook_with_cancel(
                        &plugin,
                        &bundle.bundle,
                        &bundle.global_name,
                        &hook,
                        &context_json,
                        &project_path,
                        &moss_dir,
                        cancel,
                    )
                    .await;
            } else if matches!(&result, Err(e) if e == "plugin engine dropped the hook without replying") {
                // Accepted-then-died: side effects may have half-run (git push,
                // syndication POST) — respawn for FUTURE dispatches, do NOT retry.
                respawn_engine(&slot, observed_gen, &host, &sinks, &project_path_for_respawn, &bridged, &grants_for_respawn);
            }
            // Remove our entry ONLY if it is still ours (a re-dispatch may have
            // replaced it — see SinkEntry doc).
            {
                let mut map = sinks.lock().unwrap();
                if map.get(&(plugin.clone(), hook.clone())).map(|e| e.generation) == Some(generation) {
                    map.remove(&(plugin.clone(), hook.clone()));
                }
            }
            // Exactly ONE ADAPTER-EMITTED terminal signal, honest about success
            // (success = result.success when the hook returned an object carrying
            // one; otherwise true).
            // CAVEAT (scoped guarantee): moss-api's sendMessage routes
            // type:'complete'/'error' over the plugin_message INVOKE
            // (messaging.ts:56-66) — the interception below forwards those to the
            // sink, where send_completion is first-wins; a BUNDLE-emitted complete
            // would consume the oneshot and this authoritative Complete (carrying
            // the parsed HookResult) would be dropped. Task 14 verifies neither
            // shipped bundle calls that path before C.11/C.12 rely on it.
            match result {
                Ok(json) => {
                    let parsed: Option<crate::plugins::types::HookResult> = serde_json::from_str(&json).ok();
                    let success = parsed.as_ref().map(|r| r.success).unwrap_or(true);
                    let error = if success { None } else { parsed.as_ref().and_then(|r| r.message.clone()) };
                    sink.emit(HookSignal::Complete { success, error, result: parsed });
                }
                Err(msg) => {
                    sink.emit(HookSignal::Complete { success: false, error: Some(msg), result: None });
                }
            }
        })));
        Ok(())
    }


    async fn teardown(&self) {
        // Teardown tombstone (D4): set torn_down under the slot lock FIRST, before
        // engine.shutdown(). This prevents respawn_engine from firing after teardown —
        // without it, Task 2's shutdown drain drops in-flight retirees' replies
        // producing the exact "dropped the hook" sentinel, which would deterministically
        // respawn a fresh engine + dispatch task nothing tears down after folder close.
        let engine = {
            let mut s = self.engine.lock().unwrap();
            s.torn_down = true;
            s.engine.clone()
        };
        // Unlisten every bridged app.listen registration BEFORE engine shutdown
        // so no forwarder fires into a dead engine (deliver_event on a shut-down
        // engine is a harmless dropped send, but the registration would leak).
        if let Some(app) = &self.host.app {
            let ids: Vec<ListenerId> =
                self.bridged.lock().unwrap().drain().map(|(_, id)| id).collect();
            for id in ids {
                app.unlisten(id);
            }
        }
        engine.shutdown().await;
    }

    /// Known orphan subclass (pre-existing, D3 coverage note): sinks is keyed
    /// (plugin, hook), so on the no-singleflight with_result path two overlapping
    /// same-key dispatches leave only the NEWEST entry's token reachable here —
    /// an older orphan stays unkillable. Same class exists on webview (no per-hook
    /// kill at all); not widened by Phase 4.
    fn cancel_hook(&self, plugin_name: &str, hook_name: &str) {
        let token = self
            .sinks
            .lock()
            .unwrap()
            .get(&(plugin_name.to_string(), hook_name.to_string()))
            .map(|e| e.cancel.clone());
        if let Some(token) = token {
            log::warn!(
                target: "plugin",
                "cancelling in-flight hook {plugin_name}::{hook_name}"
            );
            token.cancel();
        }
    }
}

/// Compile-time assertion that `QuickJsEngineAdapter::dispatch_hook` returns a Send
/// future (the build pipeline awaits hooks on the multi-thread runtime). NEVER
/// CALLED — only needs to compile.
#[allow(dead_code)]
fn _assert_quickjs_dispatch_future_is_send(e: &QuickJsEngineAdapter) {
    fn assert_send<T: Send>(_: &T) {}
    let inv = HookInvocation {
        plugin_name: "p".into(), hook_name: "process".into(),
        context: serde_json::json!({}),
        project_path: "/tmp/p".into(), moss_dir: "/tmp/p/.moss".into(),
    };
    let fut = e.dispatch_hook(inv, HookSignalSink::new(|_| {}));
    assert_send(&fut);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: a sink that forwards every signal into an mpsc the test can await.
    fn channel_sink() -> (HookSignalSink, tokio::sync::mpsc::UnboundedReceiver<HookSignal>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (HookSignalSink::new(move |s| { let _ = tx.send(s); }), rx)
    }

    /// Await the terminal Complete from a channel_sink rx, with a guard timeout.
    async fn wait_complete(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<HookSignal>,
    ) -> (bool, Option<String>) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Some(HookSignal::Complete { success, error, .. }) => break (success, error),
                    Some(_) => continue,
                    None => panic!("sink channel closed before Complete"),
                }
            }
        })
        .await
        .expect("Complete must arrive within 5s")
    }

    /// Test helper: build a `HookInvocation` with `/tmp/p` paths (adequate for most
    /// tests; use explicit fields when testing path forwarding).
    fn inv(plugin: &str, hook: &str, context: serde_json::Value) -> HookInvocation {
        HookInvocation {
            plugin_name: plugin.to_string(),
            hook_name: hook.to_string(),
            context,
            project_path: "/tmp/p".to_string(),
            moss_dir: "/tmp/p/.moss".to_string(),
        }
    }

    /// A.1 RED: dispatch_hook must return at ACCEPTANCE, not completion. The hook
    /// parks 300ms; dispatch must resolve well before that; Complete arrives later.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_returns_at_acceptance_while_hook_parked() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 300)); return { success: true }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        let t0 = std::time::Instant::now();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(150),
            "dispatch_hook must return at acceptance (took {:?})", t0.elapsed()
        );
        let (success, _) = wait_complete(&mut rx).await;
        assert!(success);
        adapter.teardown().await;
    }

    /// A.2 RED: plugin-reported failure must propagate (success = result.success).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn complete_propagates_plugin_reported_failure() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { return { success: false, message: "boom" }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        let (success, error) = wait_complete(&mut rx).await;
        assert!(!success, "plugin-reported success:false must propagate");
        assert_eq!(error.as_deref(), Some("boom"));
        adapter.teardown().await;
    }

    /// B.10 (Task 13): a deploy-shaped bundle returning a full HookResult must
    /// surface `result` through Complete — the manager's
    /// `execute_plugin_javascript_with_result` select! reads exactly this field
    /// out of the completion oneshot to hand deployment info back to the caller.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn complete_carries_deploy_hook_result() {
        let bundle = r#""use strict"; var P = (() => ({
            async deploy(ctx) { return { success: true, message: "deployed to pages" }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "deploy", serde_json::json!({})), sink).await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(HookSignal::Complete { result, .. }) = rx.recv().await { break result; }
            }
        }).await.unwrap();
        assert_eq!(result.expect("HookResult must parse").message.as_deref(), Some("deployed to pages"));
        adapter.teardown().await;
    }

    /// A.1 error path: post-fix the manager is parked in its select! when an engine
    /// error lands, so it MUST arrive as Complete{success:false} (an unsignalled
    /// error would cost the full 60s inactivity timeout).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_error_emits_complete_failure() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { throw new Error("hook exploded"); }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        // dispatch itself succeeds (acceptance) — the error is a SIGNAL now.
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        let (success, error) = wait_complete(&mut rx).await;
        assert!(!success);
        assert!(error.unwrap().contains("hook process error"), "engine error text must surface");
        adapter.teardown().await;
    }

    /// Sink-map proof: two plugins' concurrent hooks must each see ONLY their own
    /// signals (the one-slot SinkSlot cross-routes them — this is the keyed-map RED).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_hooks_route_signals_to_their_own_sinks() {
        let slow = r#""use strict"; var A = (() => ({
            async process(ctx) {
                await new Promise(r => setTimeout(r, 50));
                await __TAURI__.core.invoke("plugin_message", { pluginName: "a", hookName: "process",
                    message: { type: "progress", phase: "from-a", current: 1, total: 1, message: null } });
                await new Promise(r => setTimeout(r, 100));
                return { success: true };
            }
        }))();"#;
        let fast = r#""use strict"; var B = (() => ({
            async process(ctx) {
                await __TAURI__.core.invoke("plugin_message", { pluginName: "b", hookName: "process",
                    message: { type: "progress", phase: "from-b", current: 1, total: 1, message: null } });
                return { success: true };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test_multi(vec![
            ("a", slow, "A"),
            ("b", fast, "B"),
        ]);
        let (sink_a, rx_a) = channel_sink();
        let (sink_b, rx_b) = channel_sink();
        adapter.dispatch_hook(inv("a", "process", serde_json::json!({})), sink_a).await.unwrap();
        adapter.dispatch_hook(inv("b", "process", serde_json::json!({})), sink_b).await.unwrap();
        let collect = |mut rx: tokio::sync::mpsc::UnboundedReceiver<HookSignal>| async move {
            let mut phases = Vec::new();
            loop {
                match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                    Ok(Some(HookSignal::Progress { phase, .. })) => phases.push(phase),
                    Ok(Some(HookSignal::Complete { .. })) => break phases,
                    Ok(Some(_)) => continue,
                    _ => panic!("signal stream ended early"),
                }
            }
        };
        let (phases_a, phases_b) = tokio::join!(collect(rx_a), collect(rx_b));
        assert_eq!(phases_a, vec!["from-a".to_string()], "sink A saw {phases_a:?}");
        assert_eq!(phases_b, vec!["from-b".to_string()], "sink B saw {phases_b:?}");
        adapter.teardown().await;
    }

    /// BundleSource error stays an acceptance-time Err (no signal, no spawn).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_bundle_errors_at_acceptance() {
        let adapter = QuickJsEngineAdapter::new_for_test_multi(vec![]);
        let (sink, _rx) = channel_sink();
        let err = adapter.dispatch_hook(inv("ghost", "process", serde_json::json!({})), sink)
            .await.expect_err("unknown plugin must Err at acceptance");
        assert!(err.contains("ghost"));
        adapter.teardown().await;
    }

    struct FakeEngine;

    #[async_trait]
    impl PluginEngine for FakeEngine {
        async fn dispatch_hook(&self, _inv: HookInvocation, sink: HookSignalSink) -> Result<(), String> {
            sink.emit(HookSignal::Complete { success: true, error: None, result: None });
            Ok(())
        }
            async fn teardown(&self) {}
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dyn_engine_future_is_send() {
        let engine: std::sync::Arc<dyn PluginEngine> = std::sync::Arc::new(FakeEngine);
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen2 = seen.clone();
        let sink = HookSignalSink::new(move |s| {
            if matches!(s, HookSignal::Complete { success: true, .. }) {
                seen2.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let e = engine.clone();
        tokio::spawn(async move {
            e.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await
        })
        .await
        .unwrap()
        .unwrap();
        assert!(seen.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// 11d gate: the QuickJs adapter runs a trivial bundle end-to-end (off-webview,
    /// no AppHandle) and the sink receives `Complete { success: true }` carrying the
    /// hook's returned `HookResult`. (Migrated to channel_sink/wait_complete —
    /// dispatch now returns at acceptance, so asserting immediately after it
    /// returns would race the spawned completion task.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quickjs_adapter_dispatches_trivial_hook_to_sink() {
        let bundle =
            r#""use strict"; var P = (() => ({ async process(ctx) { return { success: true, message: "ok" }; } }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");

        let (sink, mut rx) = channel_sink();
        adapter
            .dispatch_hook(inv("p", "process", serde_json::json!({})), sink)
            .await
            .unwrap();

        let (success, _) = wait_complete(&mut rx).await;
        assert!(success, "sink must receive Complete {{ success: true }}");
        adapter.teardown().await;
    }

    /// A.4 / Task-5: After teardown, a dispatch must fail FAST with the thread-exited
    /// error as a Complete{success:false} signal (post-A.1, errors are signals).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_after_teardown_signals_failure_fast() {
        let bundle = r#""use strict"; var P = (() => ({ async process(ctx) { return {}; } }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        adapter.teardown().await;
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        let (success, error) = wait_complete(&mut rx).await;
        assert!(!success);
        // RACE (ground-truth review): after shutdown()'s ack the job receiver stays
        // alive until engine_loop returns and the thread exits — a dispatch landing
        // in that window is dropped and surfaces as "dropped the hook without
        // replying" (engine.rs:189) instead of the send-failure message. Both are
        // honest fast-failures; accept EITHER:
        let msg = error.unwrap();
        assert!(
            msg.contains("plugin engine thread has exited")
                || msg.contains("dropped the hook without replying"),
            "unexpected teardown error: {msg}"
        );
    }

    /// Sink-aware `plugin_message` translation (Task-14b path): a hook that emits a
    /// `progress` plugin_message via `__TAURI__.core.invoke` must reach the sink as a
    /// `HookSignal::Progress` BEFORE the terminal `Complete`. (Migrated to
    /// channel_sink — signals are awaited from the channel, not read after dispatch.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quickjs_adapter_routes_plugin_message_progress_to_sink() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                await __TAURI__.core.invoke("plugin_message", {
                    pluginName: "p", hookName: "process",
                    message: { type: "progress", phase: "running", current: 1, total: 3, message: "step" }
                });
                return { success: true };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");

        let (sink, mut rx) = channel_sink();
        adapter
            .dispatch_hook(inv("p", "process", serde_json::json!({})), sink)
            .await
            .unwrap();

        let mut got = Vec::new();
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(HookSignal::Progress { phase, current, total, .. })) => {
                    got.push(format!("progress:{phase}:{current}/{total}"));
                }
                Ok(Some(HookSignal::Complete { success, .. })) => {
                    got.push(format!("complete:{success}"));
                    break;
                }
                Ok(Some(_)) => continue,
                _ => panic!("signal stream ended early, got {got:?}"),
            }
        }
        assert_eq!(
            got,
            vec!["progress:running:1/3".to_string(), "complete:true".to_string()],
            "progress must arrive BEFORE the terminal complete"
        );
        adapter.teardown().await;
    }

    /// Pre-merge review Fix 3 RED: a slow generic host command must NOT
    /// head-of-line-block queued `plugin_message` heartbeats. Webview parity: the
    /// webview runtime processes invokes concurrently, so github deploy's
    /// `execute_binary` git push (timeoutMs up to 600s) never starves the progress
    /// heartbeats that keep the manager's 60s INACTIVITY_TIMEOUT at bay. The hook
    /// fires a ~300ms `__test_slow__` invoke (stub-side sleep) while emitting a
    /// progress heartbeat every ~30ms; ≥2 Progress signals must reach the sink
    /// BEFORE the hook completes. A sequential dispatch loop queues the heartbeats
    /// behind the slow reply, so only ONE lands before Complete.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn heartbeat_flows_during_blocking_command() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                let done = false;
                __TAURI__.core.invoke("__test_slow__", { ms: 300 }).then(() => { done = true; });
                let beats = 0;
                while (!done && beats < 50) {
                    beats += 1;
                    await __TAURI__.core.invoke("plugin_message", { pluginName: "p", hookName: "process",
                        message: { type: "progress", phase: "hb", current: beats, total: 0, message: null } });
                    await new Promise(r => setTimeout(r, 30));
                }
                return { success: true };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        // 10s guard: a head-of-line regression must fail fast, never hang CI.
        let progress_before_complete = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            async {
                let mut n = 0u32;
                loop {
                    match rx.recv().await {
                        Some(HookSignal::Progress { .. }) => n += 1,
                        Some(HookSignal::Complete { .. }) => break n,
                        Some(_) => continue,
                        None => panic!("sink closed before Complete"),
                    }
                }
            },
        )
        .await
        .expect("hook must complete within 10s");
        assert!(
            progress_before_complete >= 2,
            "heartbeats must keep flowing while the slow command is in flight; got \
             {progress_before_complete} Progress signal(s) before Complete — a \
             sequential dispatch loop queues them behind the 300ms command"
        );
        adapter.teardown().await;
    }

    /// Bug 4 RED: while the host runs a plugin's call, the plugin must be marked
    /// busy in the SAME `PluginHookState` the manager's inactivity watchdog reads
    /// (`hook_is_making_progress`). Without the bracket a hook that awaits one
    /// slow host arm — github deploy's `execute_binary` git push — looks silent
    /// for its whole duration and is killed at 60 s, which is why first-party
    /// plugins ship fake heartbeats. `__test_slow__` stands in for that arm.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_slow_host_call_marks_the_plugin_busy() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                await __TAURI__.core.invoke("__test_slow__", { ms: 300 });
                return { success: true };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let hook_state = adapter.hook_state_for_test();
        assert!(!hook_state.is_making_progress("p", "process"), "idle before dispatch");

        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();

        // Poll rather than sleep a fixed slice: the reprieve must be observable
        // at SOME point during the 300ms call, not at one exact instant.
        let saw_busy = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if hook_state.is_making_progress("p", "process") { break true; }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or(false);
        assert!(saw_busy, "the host call must mark the plugin busy while it runs");

        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await {
                Ok(Some(HookSignal::Complete { .. })) => break,
                Ok(Some(_)) => continue,
                _ => panic!("hook must complete"),
            }
        }
        assert!(
            !hook_state.is_making_progress("p", "process"),
            "the reprieve must end with the call — otherwise the watchdog is disarmed for good"
        );
        adapter.teardown().await;
    }

    /// B.8d RED: a hook's event.emit('plugin-message', {progress}) must reach the
    /// SINK (today it goes to emit_to("action-panel") — a black hole headless).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn emitted_plugin_message_reaches_sink() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                await __TAURI__.event.emit("plugin-message", {
                    pluginName: "p", hookName: "process",
                    message: { type: "progress", phase: "hb", current: 1, total: 1, message: null }
                });
                return { success: true };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(inv("p", "process", serde_json::json!({})), sink).await.unwrap();
        let mut saw_progress = false;
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                Ok(Some(HookSignal::Progress { phase, .. })) if phase == "hb" => saw_progress = true,
                Ok(Some(HookSignal::Complete { .. })) => break,
                Ok(Some(_)) => continue,
                _ => panic!("stream ended early"),
            }
        }
        assert!(saw_progress, "event-channel plugin-message must route into the sink");
        adapter.teardown().await;
    }

    /// B.6: the hook must see the REAL project_path via __MOSS_INTERNAL_CONTEXT__,
    /// and must NOT see the SDK-internal project_path key on its context object.
    /// The bundle returns { success: true, message: ip + "|" + md + "|" + leaked }
    /// where ip = __MOSS_INTERNAL_CONTEXT__.project_path, md = .moss_dir, leaked =
    /// Object.prototype.hasOwnProperty.call(ctx, "project_path") (must be false).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hook_sees_internal_context_and_clean_context_object() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                const ip = window.__MOSS_INTERNAL_CONTEXT__.project_path;
                const md = window.__MOSS_INTERNAL_CONTEXT__.moss_dir;
                const leaked = Object.prototype.hasOwnProperty.call(ctx, "project_path");
                return { success: true, message: ip + "|" + md + "|" + leaked };
            }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        // Inject project_path as the manager does — the adapter must strip it before
        // handing context to the hook AND set __MOSS_INTERNAL_CONTEXT__ via the loader.
        let mut context = serde_json::json!({ "trigger": "build" });
        context.as_object_mut().unwrap().insert("project_path".into(), serde_json::json!("/proj"));
        adapter.dispatch_hook(HookInvocation {
            plugin_name: "p".into(),
            hook_name: "process".into(),
            context,
            project_path: "/proj".into(),
            moss_dir: "/proj/.moss".into(),
        }, sink).await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(HookSignal::Complete { result, .. }) = rx.recv().await {
                    break result;
                }
            }
        })
        .await
        .expect("Complete must arrive within 5s");
        let hook_result = result.expect("hook must return a parseable HookResult");
        assert!(hook_result.success, "hook must succeed");
        assert_eq!(
            hook_result.message.as_deref(),
            Some("/proj|/proj/.moss|false"),
            "project_path and moss_dir must reach __MOSS_INTERNAL_CONTEXT__; project_path must NOT leak into ctx"
        );
        adapter.teardown().await;
    }

    /// Convenience wrapper: `HookInvocation` with empty context (most cancel/
    /// cancel-scoping tests need no context fields).
    fn test_invocation(plugin: &str, hook: &str) -> HookInvocation {
        inv(plugin, hook, serde_json::json!({}))
    }

    // ─── Task 5: engine respawn tests ───────────────────────────────────────────

    /// Condition 5 (D4): a dead engine thread is respawned by the adapter; a
    /// dispatch the dead engine NEVER ACCEPTED ("thread has exited") is retried
    /// once and succeeds on the fresh engine.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_respawns_dead_engine_and_retries_never_accepted_dispatch() {
        let healthy = r#""use strict"; var P = (() => ({
            async process(ctx) { return { success: true, message: "alive" }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(healthy, "P");
        adapter.engine_handle_for_test().panic_engine_thread_for_test();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await; // thread dies
        let (sink, mut rx) = channel_sink();
        adapter.dispatch_hook(test_invocation("p", "process"), sink).await.unwrap();
        let (success, error) = wait_complete(&mut rx).await;
        assert!(success, "respawned engine must serve the retried dispatch; err={error:?}");
        adapter.teardown().await;
    }

    /// The accepted-then-died case ("dropped the hook without replying") must NOT
    /// retry (side effects may have half-run: git push, syndication POST) but must
    /// still respawn for FUTURE dispatches.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_hook_respawns_without_retry() {
        let parker = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { ok: true }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(parker, "P");
        let (sink1, mut rx1) = channel_sink();
        adapter.dispatch_hook(test_invocation("p", "process"), sink1).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await; // hook accepted + parked
        adapter.engine_handle_for_test().panic_engine_thread_for_test();
        let (success1, error1) = wait_complete(&mut rx1).await;
        assert!(!success1, "the in-flight hook must FAIL (no silent retry of half-run side effects)");
        assert!(error1.unwrap().contains("dropped the hook"), "must carry the accepted-then-died sentinel");
        // Future dispatches work — the respawn happened.
        let (sink2, mut rx2) = channel_sink();
        adapter.dispatch_hook(test_invocation("p", "process"), sink2).await.unwrap();
        // The parker bundle parks 60s — Progress isn't the point; ACCEPTANCE on a
        // live engine is. Cancel it to finish fast:
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        adapter.cancel_hook("p", "process");
        let (_s2, e2) = wait_complete(&mut rx2).await;
        assert!(e2.unwrap_or_default().contains("cancel"), "second dispatch ran on a LIVE engine");
        adapter.teardown().await;
    }

    /// Teardown tombstone (D4 — review blocker): a NORMAL teardown must NEVER
    /// trigger a respawn. Task 2's shutdown drain drops in-flight retirees'
    /// replies, producing the exact "dropped the hook without replying" sentinel
    /// at every folder close with an in-flight hook — without the tombstone that
    /// deterministically leaks a fresh engine + dispatch task nothing tears down,
    /// and a racing "thread has exited" dispatch would RETRY a hook (file writes,
    /// network POSTs) after the folder closed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_respawn_after_teardown() {
        let parker = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { ok: true }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(parker, "P");
        let (sink1, mut rx1) = channel_sink();
        adapter.dispatch_hook(test_invocation("p", "process"), sink1).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await; // accepted + parked
        adapter.teardown().await; // shutdown drain drops the reply → "dropped the hook" sentinel
        let (s1, e1) = wait_complete(&mut rx1).await;
        assert!(!s1, "the in-flight hook fails at teardown");
        assert!(e1.unwrap_or_default().contains("dropped the hook"));
        // Post-teardown dispatch fails FAST with the dead-engine sentinel and is
        // NOT served by a respawned engine (the parker would otherwise be parked
        // on a fresh engine and this wait_complete would time out).
        let (sink2, mut rx2) = channel_sink();
        adapter.dispatch_hook(test_invocation("p", "process"), sink2).await.unwrap();
        let (s2, e2) = wait_complete(&mut rx2).await;
        assert!(!s2, "no respawn after teardown");
        assert!(
            e2.unwrap_or_default().contains("thread has exited"),
            "must be the dead-engine sentinel, not a live run on a leaked respawn"
        );
    }

    /// Condition 6 through the TRAIT: cancel_hook on a parked hook produces a
    /// prompt Complete{success:false} instead of the 60s inactivity degrade.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn adapter_cancel_hook_completes_promptly() {
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { ok: true }; }
        }))();"#;
        let adapter = QuickJsEngineAdapter::new_for_test(bundle, "P");
        let (sink, mut rx) = channel_sink();
        adapter
            .dispatch_hook(test_invocation("p", "process"), sink)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        adapter.cancel_hook("p", "process");
        let (success, error) = wait_complete(&mut rx).await; // 5s guard inside
        assert!(!success, "cancelled hook must Complete with success:false");
        assert!(error.unwrap().contains("cancel"), "error text must say cancelled");
        adapter.teardown().await;
    }

}
