use super::*;

/// Deliver `name` to `plugin` every 10 ms until the returned task is aborted.
/// Delivery before the plugin's listener is registered is a silent drop (the
/// `plugins.get` miss arm in [`QuickJsEngine::deliver_event`]), so a test
/// delivering once on a timer races bundle compilation on a loaded box
/// (#1138); repetition closes the race for any idempotent payload. Abort the
/// handle once the hook under test has resolved.
pub(crate) fn deliver_event_until_aborted(
    engine: &QuickJsEngine,
    plugin: &str,
    name: &str,
    payload_json: &str,
) -> tokio::task::JoinHandle<()> {
    let engine = engine.clone();
    let (plugin, name, payload_json) =
        (plugin.to_string(), name.to_string(), payload_json.to_string());
    tokio::spawn(async move {
        loop {
            engine.deliver_event(&plugin, &name, &payload_json);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
}

/// Compile-time `!Send`→`Send` boundary proof on the REAL dispatch path
/// (replaces the Phase-1 eval_future_is_send).
#[test]
fn dispatch_hook_raw_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}
    let (engine, _rx) = QuickJsEngine::new(None);
    let fut = engine.dispatch_hook_raw("p", "var P = {};", "P", "h", "{}", "", "");
    assert_send(&fut);
}

/// 11b drive()-resolution gate. The bundle's `process` hook AWAITS an in-hook
/// `setTimeout` before returning — this catches any drive/timer stall at 11b
/// (the Task-8 finding) rather than at Task 14. It MUST resolve (not hang) and
/// return the doubled value.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispatch_hook_runs_trivial_bundle_with_timer() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({ async process(ctx) { await new Promise(r => setTimeout(r, 5)); return { doubled: ctx.n * 2 }; } }))();"#;
    let out = engine
        .dispatch_hook_raw(
            "p",
            bundle,
            "P",
            "process",
            &serde_json::json!({"n":21}).to_string(),
            "/tmp/proj",
            "/tmp/proj/.moss",
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["doubled"], serde_json::json!(42));
    engine.shutdown().await;
}

/// R1 (sdk-bundles probe): real bundles touch `window` at load time and inside
/// hooks. The github bundle's `window.GithubPlugin = GithubPlugin;` is the
/// load-time shape; the SDK's `const w = window;` is the hook-time shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn window_alias_exists_in_plugin_contexts() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // window.* at BUNDLE-EVAL time (github's shape) and inside the hook.
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { return { aliased: window === globalThis, ic: typeof window.__TAURI__ }; }
        }))(); window.P_MARK = P;"#;
    let out = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "/tmp/p", "/tmp/p/.moss")
        .await
        .expect("bundle with window.* must load and run");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["aliased"], serde_json::json!(true));
    assert_eq!(v["ic"], serde_json::json!("object"));
    engine.shutdown().await;
}

/// D-fix: a bundle whose top-level code starts a timer then throws must reply
/// the init error AND not poison the plugin name — a follow-up dispatch with a
/// GOOD bundle for the same name succeeds (failed init never inserted the
/// Context; the cleanup cancels the orphan timer + clears the TIMERS entry).
///
/// REGRESSION PIN: passes today because a failed init never inserts the Context
/// (engine.rs replies Err and continues before `plugins.insert`), so the retry
/// already succeeds. The timer-cancel/TIMERS-entry cleanup this task adds is
/// unobservable from the test (engine thread-local is not accessible from the
/// test thread) — the test pins the contract against future reorderings.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn init_failure_cleans_up_and_allows_retry() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bad = r#""use strict"; setTimeout(() => {}, 60000); throw new Error("init boom");"#;
    let err = engine
        .dispatch_hook_raw("p", bad, "P", "process", "{}", "", "")
        .await
        .expect_err("top-level throw must fail init");
    assert!(err.contains("init boom"), "got: {err}");
    let good =
        r#""use strict"; var P = (() => ({ async process(ctx) { return { ok: true }; } }))();"#;
    let out = engine
        .dispatch_hook_raw("p", good, "P", "process", "{}", "", "")
        .await
        .expect("retry with a good bundle must succeed");
    assert!(out.contains("true"));
    engine.shutdown().await;
}

/// EvalI64-removal replacement: per-plugin Context state persists across
/// dispatches (the Phase-1 `reuses_context_across_calls` contract, now on the
/// REAL dispatch path).
///
/// REGRESSION PIN: EvalI64 uses a separate `scratch` Context and never touches
/// plugin Contexts; this test exists so the contract survives the EvalI64
/// DELETION, not because EvalI64 interferes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugin_context_state_persists_across_dispatches() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { globalThis.__n = (globalThis.__n || 0) + 1; return { n: globalThis.__n }; }
        }))();"#;
    let first = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .unwrap();
    let second = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .unwrap();
    assert!(
        first.contains("1") && second.contains("2"),
        "got {first} then {second}"
    );
    engine.shutdown().await;
}

/// Task-14a topology regression. A hook registers a `browser-closed` listener,
/// then BOUNDED-POLLS (`for i in 0..50 { await setTimeout(10) }`) for the event's
/// reason. A `DeliverEvent` is queued ~30ms in. With the old `idle()`-after-dispatch
/// design the loop is parked in `idle()` (holding the runtime lock) for the hook's
/// whole ~500ms duration, so the `DeliverEvent` is never dequeued and the hook
/// returns `reason: null` (or, with the inline guard below, the await would HANG
/// past the 5s timeout). With the drive-loop fix the event is dequeued and
/// delivered INLINE while the hook is parked, so the hook sees `reason: "user"` and
/// exits early. This would FAIL on the pre-fix engine; it is the acceptance gate.
///
/// `new(None)` passes no app handle, which makes only `emit` a no-op;
/// `install_event`'s `listen` (registers the Persistent in the hub) and `deliver`
/// do not touch `app`, so the listen + deliver path is fully exercised.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deliver_event_reaches_hook_parked_in_poll_loop() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // Hook: register a browser-closed listener, then bounded-poll for the reason.
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                let reason = null;
                await globalThis.__TAURI__.event.listen('browser-closed', (e) => { reason = e.payload.reason; });
                for (let i = 0; i < 50 && reason === null; i++) { await new Promise(r => setTimeout(r, 10)); }
                return { reason };
            }
        }))();"#;
    let deliver = deliver_event_until_aborted(&engine, "p", "browser-closed", r#"{"reason":"user"}"#);
    // Guard the await so a regression HANGS-FAIL fast instead of stalling CI.
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.dispatch_hook_raw(
            "p",
            bundle,
            "P",
            "process",
            &serde_json::json!({}).to_string(),
            "/tmp/p",
            "/tmp/p/.moss",
        ),
    )
    .await
    .expect("hook must not hang")
    .unwrap();
    deliver.abort();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["reason"],
        serde_json::json!("user"),
        "DeliverEvent must reach the parked hook"
    );
    engine.shutdown().await;
}

/// listen() must fire a host-side __engine_listen__ registration request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_notifies_host_bridge() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            if req.cmd == "__engine_listen__" {
                seen2.lock().unwrap().push(req.args_json.clone());
            }
            let _ = req.reply.send(Ok("null".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                await globalThis.__TAURI__.event.listen("github:deploy-choice", () => {});
                return { success: true };
            }
        }))();"#;
    engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .unwrap();
    let got = seen.lock().unwrap().clone();
    assert!(
        got.iter()
            .any(|j| j.contains("github:deploy-choice") && j.contains("\"p\"")),
        "expected __engine_listen__ for (p, github:deploy-choice); got {got:?}"
    );
    engine.shutdown().await;
}

/// Pre-merge review Fix 2: a plugin lacking the requested hook must error with
/// the WEBVIEW sentinel "Hook function not found" (plugin-runtime.ts:312) —
/// manager.rs `execute_configure_domain` matches that exact substring to
/// gracefully skip deploy plugins that don't implement `configure_domain`.
/// Pre-fix the quickjs path surfaces an rquickjs FromJs message instead,
/// turning the graceful skip into a hard Err.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_hook_errors_with_webview_sentinel() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({ async process(ctx) { return {}; } }))();"#;
    let err = engine
        .dispatch_hook_raw("p", bundle, "P", "configure_domain", "{}", "", "")
        .await
        .expect_err("a missing hook must Err");
    assert!(
        err.contains("Hook function not found"),
        "manager.rs's graceful-skip needle must match (webview parity); got: {err}"
    );
    engine.shutdown().await;
}

/// B.7 RED: a changed bundle for a known plugin name must be picked up
/// (today: first-sight-wins serves v1 forever).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_bundle_evicts_stale_context() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let v1 = r#""use strict"; var P = (() => ({ async process(ctx) { return { v: 1 }; } }))();"#;
    let v2 = r#""use strict"; var P = (() => ({ async process(ctx) { return { v: 2 }; } }))();"#;
    let first = engine
        .dispatch_hook_raw("p", v1, "P", "process", "{}", "", "")
        .await
        .unwrap();
    let second = engine
        .dispatch_hook_raw("p", v2, "P", "process", "{}", "", "")
        .await
        .unwrap();
    assert!(first.contains("1"), "got {first}");
    assert!(
        second.contains("2"),
        "stale Context served after bundle change: {second}"
    );
    engine.shutdown().await;
}

/// A.5: a panic raised while a hook runs must surface as a STRUCTURED error
/// (never the silent "dropped the hook without replying"), and the engine must
/// survive to serve the next dispatch. The panic source is a sync host fn
/// (installed test-only) invoked from JS. NOTE (D6): rquickjs may convert the
/// panic at the FFI boundary into a JS exception — that also satisfies the
/// contract; what this test forbids is the silent-swallow path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_panic_surfaces_structured_error_and_engine_survives() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { __moss_test_panic(); return { unreachable: true }; }
        }))();"#;
    let err = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .expect_err("a panicking hook must error");
    assert!(
        !err.contains("dropped the hook without replying"),
        "panic must NOT be silently swallowed; got: {err}"
    );
    // Engine thread is still alive: the next dispatch (same plugin, Context
    // already loaded) succeeds.
    let bundle_ok =
        r#""use strict"; var Q = (() => ({ async process(ctx) { return { ok: true }; } }))();"#;
    let out = engine
        .dispatch_hook_raw("q", bundle_ok, "Q", "process", "{}", "", "")
        .await
        .expect("engine must survive a hook panic");
    assert!(out.contains("true"));
    engine.shutdown().await;
}

/// Condition 1 (D1): shutdown must complete even when a hook is wedged in a
/// SYNCHRONOUS JS loop. The out-of-band kill flag + interrupt handler raise the
/// uncatchable "interrupted" InternalError at the next poll point (~10k loop
/// iterations), unwinding the slice so the select! loop can dequeue Shutdown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_interrupts_synchronous_infinite_loop() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            process(ctx) { while (true) {} }
        }))();"#;
    let engine2 = engine.clone();
    let hook = tokio::spawn(async move {
        engine2
            .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // let it wedge
    tokio::time::timeout(std::time::Duration::from_secs(5), engine.shutdown())
        .await
        .expect("shutdown must complete despite the wedged sync loop (condition 1)");
    let err = hook.await.unwrap().expect_err("the wedged hook must error");
    assert!(
        err.to_lowercase().contains("interrupt"),
        "interrupt must surface in the hook error (sync-call ctx.catch enrichment); got: {err}"
    );
    // The dead engine fails fast with the accepted sentinel.
    let post = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .expect_err("post-shutdown dispatch must fail");
    assert!(
        post.contains("plugin engine thread has exited"),
        "got: {post}"
    );
}

/// Condition 4 (D2): bundle eviction during an in-flight hook of the SAME
/// plugin must NOT cancel that hook's timers (today the orphaned hook's
/// awaited setTimeout never settles → reply never sent → 60s degrade).
/// Retire-don't-kill: the old hook completes naturally; the new bundle
/// serves the next dispatch immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eviction_during_in_flight_hook_lets_it_complete() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let v1 = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 400)); return { version: 1 }; }
        }))();"#;
    let v2 = r#""use strict"; var P = (() => ({
            async process(ctx) { return { version: 2 }; }
        }))();"#;
    let engine_slow = engine.clone();
    let slow = tokio::spawn(async move {
        engine_slow
            .dispatch_hook_raw("p", v1, "P", "process", "{}", "", "")
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // Different bytes → fingerprint mismatch → eviction while v1's hook is parked.
    let out2 = engine
        .dispatch_hook_raw("p", v2, "P", "process", "{}", "", "")
        .await
        .expect("new bundle must serve immediately (not queued behind the retiree)");
    assert!(out2.contains("2"), "got {out2}");
    let out1 = tokio::time::timeout(std::time::Duration::from_secs(5), slow)
        .await
        .expect("retired hook must complete naturally, not park forever (condition 4)")
        .unwrap()
        .expect("retired hook must succeed");
    assert!(
        out1.contains("1"),
        "the OLD hook must return its own result; got {out1}"
    );
    engine.shutdown().await;
}

/// Condition 6, parked case: cancelling the token kills a hook parked at an
/// await (the select-wrap arm; token wakers cross threads).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_token_kills_parked_hook() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { ok: true }; }
        }))();"#;
    let cancel = tokio_util::sync::CancellationToken::new();
    let engine2 = engine.clone();
    let c2 = cancel.clone();
    let hook = tokio::spawn(async move {
        engine2
            .dispatch_hook_with_cancel("p", bundle, "P", "process", "{}", "", "", c2)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    cancel.cancel();
    let err = tokio::time::timeout(std::time::Duration::from_secs(3), hook)
        .await
        .expect("cancel must kill the parked hook promptly")
        .unwrap()
        .expect_err("cancelled hook must error");
    assert!(err.contains("cancelled"), "got: {err}");
    engine.shutdown().await;
}

/// Condition 6, sync-loop case: the token reaches the interrupt handler via
/// the CURRENT_GEN attribution and kills a wedged synchronous loop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_token_interrupts_sync_loop_hook() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            process(ctx) { while (true) {} }
        }))();"#;
    let cancel = tokio_util::sync::CancellationToken::new();
    let engine2 = engine.clone();
    let c2 = cancel.clone();
    let hook = tokio::spawn(async move {
        engine2
            .dispatch_hook_with_cancel("p", bundle, "P", "process", "{}", "", "", c2)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    cancel.cancel();
    let err = tokio::time::timeout(std::time::Duration::from_secs(5), hook)
        .await
        .expect("cancel must kill the wedged sync-loop hook")
        .unwrap()
        .expect_err("cancelled/interrupted hook must error");
    // The error text mentions "cancel" (select arm) or "interrupt" (via interrupt handler).
    let err_lower = err.to_lowercase();
    assert!(
        err_lower.contains("cancel") || err_lower.contains("interrupt"),
        "error must mention cancel or interrupt; got: {err}"
    );
    // Engine must survive: a follow-up dispatch of a DIFFERENT plugin succeeds.
    let bundle_ok =
        r#""use strict"; var Q = (() => ({ async process(ctx) { return { ok: true }; } }))();"#;
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.dispatch_hook_raw("q", bundle_ok, "Q", "process", "{}", "", ""),
    )
    .await
    .expect("engine must survive per-hook cancellation")
    .expect("follow-up dispatch must succeed");
    assert!(out.contains("true"), "got: {out}");
    engine.shutdown().await;
}

/// Cancellation is scoped: cancelling plugin A's hook must not disturb
/// plugin B's concurrently-parked hook.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_is_scoped_to_its_hook() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // A parks 60s; B parks 300ms then returns ok.
    let bundle_a = r#""use strict"; var A = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { plugin: "a" }; }
        }))();"#;
    let bundle_b = r#""use strict"; var B = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 300)); return { plugin: "b" }; }
        }))();"#;
    let cancel_a = tokio_util::sync::CancellationToken::new();
    let engine_a = engine.clone();
    let ca2 = cancel_a.clone();
    let hook_a = tokio::spawn(async move {
        engine_a
            .dispatch_hook_with_cancel("a", bundle_a, "A", "process", "{}", "", "", ca2)
            .await
    });
    let engine_b = engine.clone();
    let hook_b = tokio::spawn(async move {
        engine_b
            .dispatch_hook_raw("b", bundle_b, "B", "process", "{}", "", "")
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    cancel_a.cancel(); // cancel A only
                       // A must error with "cancelled".
    let err_a = tokio::time::timeout(std::time::Duration::from_secs(3), hook_a)
        .await
        .expect("cancel must kill A promptly")
        .unwrap()
        .expect_err("A must error after cancel");
    assert!(err_a.contains("cancelled"), "got: {err_a}");
    // B must complete naturally and succeed.
    let out_b = tokio::time::timeout(std::time::Duration::from_secs(5), hook_b)
        .await
        .expect("B must complete within its natural time")
        .unwrap()
        .expect("B must succeed — A cancel must not disturb it");
    assert!(out_b.contains("\"b\""), "got: {out_b}");
    engine.shutdown().await;
}

/// D3 hygiene: a cancelled plugin's Context is POISONED and retired — the
/// next dispatch of the same plugin gets a FRESH Context (half-mutated JS
/// state does not leak forward).
///
/// Sensitivity note: hook2 uses the SAME bundle bytes as hook1.  The
/// fingerprint therefore MATCHES, so D2's retire-on-mismatch eviction does
/// NOT fire.  The ONLY path to a fresh Context is the poison sweep
/// (`poisoned.set(true)` → loop-top drain).  hook2 calls a different exported
/// function (`check`) so it returns promptly rather than parking again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_plugin_context_is_retired() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // Single bundle used for BOTH dispatches — same bytes → same fingerprint →
    // fingerprint-mismatch eviction (D2) cannot fire; only the poison sweep can
    // retire the Context.  `process` sets __mark=1 and parks; `check` reads it.
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                globalThis.__mark = 1;
                await new Promise(r => setTimeout(r, 60000));
                return { ok: true };
            },
            async check(ctx) {
                return { mark: typeof globalThis.__mark };
            }
        }))();"#;
    let cancel = tokio_util::sync::CancellationToken::new();
    let engine2 = engine.clone();
    let c2 = cancel.clone();
    // Hook 1: sets a global, parks 60s. Cancel it → Context poisoned.
    let hook1 = tokio::spawn(async move {
        engine2
            .dispatch_hook_with_cancel("p", bundle, "P", "process", "{}", "", "", c2)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    cancel.cancel();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), hook1)
        .await
        .expect("cancel must kill hook1 promptly");
    // Give the engine a moment to process the poison sweep at the next loop top.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // Hook 2: same plugin, same bundle bytes (fingerprint match → D2 eviction
    // cannot fire).  The poison sweep is the only mechanism that retires the
    // half-mutated Context.  A fresh Context re-loads the bundle but has never
    // called `process`, so __mark is undefined.
    let bundle2 = bundle; // bytes-identical — required by the test contract above
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.dispatch_hook_raw("p", bundle2, "P", "check", "{}", "", ""),
    )
    .await
    .expect("hook2 must complete")
    .expect("hook2 must succeed");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["mark"],
        serde_json::json!("undefined"),
        "fresh Context must NOT see the cancelled hook's __mark global; got: {out}"
    );
    engine.shutdown().await;
}

/// Condition 2 (D5): onload({project_path}) runs ONCE at Context init (not
/// per hook), awaited, before the first hook — webview parity
/// (plugin-runtime.ts:185-191).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn onload_called_once_with_project_path() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            onload(opts) {
                globalThis.__onload_calls = (globalThis.__onload_calls || 0) + 1;
                globalThis.__onload_path = opts.project_path;
            },
            async process(ctx) {
                return { calls: globalThis.__onload_calls, path: globalThis.__onload_path };
            }
        }))();"#;
    let first = engine
        .dispatch_hook_raw(
            "p",
            bundle,
            "P",
            "process",
            "{}",
            "/tmp/proj-x",
            "/tmp/proj-x/.moss",
        )
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["calls"], serde_json::json!(1));
    assert_eq!(v["path"], serde_json::json!("/tmp/proj-x"));
    let second = engine
        .dispatch_hook_raw(
            "p",
            bundle,
            "P",
            "process",
            "{}",
            "/tmp/proj-x",
            "/tmp/proj-x/.moss",
        )
        .await
        .unwrap();
    let v2: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(
        v2["calls"],
        serde_json::json!(1),
        "onload must NOT re-run per dispatch"
    );
    engine.shutdown().await;
}

/// onload throw = INIT failure (webview parity: catch at plugin-runtime.ts:
/// 208-221 fails init) — and the name is not poisoned: a corrected bundle
/// (different bytes → re-init) succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn onload_throw_fails_init_and_allows_retry() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // bad: onload throws — init must fail, error mentioning "onload" and "onload boom".
    let bad = r#""use strict"; var P = (() => ({
            onload(opts) { throw new Error("onload boom"); },
            async process(ctx) { return { ok: true }; }
        }))();"#;
    let err = engine
        .dispatch_hook_raw(
            "p",
            bad,
            "P",
            "process",
            "{}",
            "/tmp/proj",
            "/tmp/proj/.moss",
        )
        .await
        .expect_err("onload throw must fail init");
    assert!(
        err.to_lowercase().contains("onload"),
        "error must mention onload; got: {err}"
    );
    assert!(
        err.contains("onload boom"),
        "error must carry the thrown message; got: {err}"
    );
    // good: different bytes (no onload) → re-init on the same plugin name → succeeds.
    let good = r#""use strict"; var P = (() => ({
            async process(ctx) { return { ok: true }; }
        }))();"#;
    let out = engine
        .dispatch_hook_raw(
            "p",
            good,
            "P",
            "process",
            "{}",
            "/tmp/proj",
            "/tmp/proj/.moss",
        )
        .await
        .expect("corrected bundle (different bytes) must succeed after a failed onload");
    assert!(out.contains("true"), "got: {out}");
    engine.shutdown().await;
}

/// An ASYNC onload is awaited before the first hook observes its effects
/// (WithFuture pumps the schedular — D5 grounding).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_onload_awaited_before_hook() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    // onload: async — awaits a 50ms timer, then sets globalThis.__ready = true.
    // process: returns { ready: globalThis.__ready === true }.
    let bundle = r#""use strict"; var P = (() => ({
            async onload(opts) {
                await new Promise(r => setTimeout(r, 50));
                globalThis.__ready = true;
            },
            async process(ctx) {
                return { ready: globalThis.__ready === true };
            }
        }))();"#;
    let out = engine
        .dispatch_hook_raw(
            "p",
            bundle,
            "P",
            "process",
            "{}",
            "/tmp/proj",
            "/tmp/proj/.moss",
        )
        .await
        .expect("hook must succeed after async onload completes");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["ready"],
        serde_json::json!(true),
        "async onload must be fully awaited before the hook runs; got: {out}"
    );
    engine.shutdown().await;
}

/// D9: constructing an engine must NOT spawn the OS thread (post-flip every
/// folder-open builds one and most projects never dispatch a hook). The
/// thread spawns on the FIRST job; shutdown of a never-spawned engine is a
/// no-op that returns immediately.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_thread_spawn_is_lazy() {
    let (engine, _rx) = QuickJsEngine::new(None);
    assert!(
        !engine.thread_spawned_for_test(),
        "construction must not spawn the thread"
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), engine.shutdown())
        .await
        .expect("shutdown of a never-spawned engine must be an immediate no-op");

    let (engine2, mut rx2) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = rx2.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle =
        r#""use strict"; var P = (() => ({ async process(ctx) { return { ok: true }; } }))();"#;
    let out = engine2
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .unwrap();
    assert!(out.contains("true"));
    assert!(
        engine2.thread_spawned_for_test(),
        "first job must have spawned the thread"
    );
    engine2.shutdown().await;
}

/// Interrupt-under-drive (gc_decref_child history, engine-kill-respawn probe):
/// a sync loop inside a TIMER CALLBACK runs under drive()'s pending-job pump,
/// not under a hook poll. The global kill flag must still kill it, and the
/// pinned rquickjs 0.12.0 must not corrupt refcounts doing so (no abort).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_interrupts_sync_loop_in_timer_callback_under_drive() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });
    let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) { setTimeout(() => { while (true) {} }, 10); return { ok: true }; }
        }))();"#;
    let out = engine
        .dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "")
        .await
        .expect("the hook itself returns before the timer fires");
    assert!(out.contains("true"));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // timer fires; drive() wedges
    tokio::time::timeout(std::time::Duration::from_secs(5), engine.shutdown())
        .await
        .expect("shutdown must interrupt a sync loop inside a drive()-pumped timer callback");
}

/// Gate 2 combined conditions: D2 (retire-don't-kill) then D3 (cancel via
/// interrupt) on the same engine — two concurrent hooks on the same plugin "p".
/// D2 fires first (hook2 dispatch evicts hook1's Context via fingerprint
/// mismatch); D3 fires next (cancel tokens fired back-to-back interrupt both
/// parked hooks).  The engine must survive both and accept a third dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phase4_retire_and_cancel_concurrent() {
    let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
    tokio::spawn(async move {
        while let Some(req) = dispatch_rx.recv().await {
            let _ = req.reply.send(Err("no cmd".into()));
        }
    });

    // bundle_v1 and bundle_v2 have DIFFERENT bytes (different global var names
    // + whitespace) so the fingerprint check fires retire-don't-kill when hook2
    // arrives while hook1 is still parked.
    let bundle_v1 = r#""use strict";  var V1 = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { v: 1 }; }
        }))();"#;
    let bundle_v2 = r#""use strict"; var V2 = (() => ({
            async process(ctx) { await new Promise(r => setTimeout(r, 60000)); return { v: 2 }; }
        }))();"#;

    // hook1: plugin "p", bundle_v1 — parks 60s.
    let cancel1 = tokio_util::sync::CancellationToken::new();
    let engine1 = engine.clone();
    let c1 = cancel1.clone();
    let hook1 = tokio::spawn(async move {
        engine1
            .dispatch_hook_with_cancel("p", bundle_v1, "V1", "process", "{}", "", "", c1)
            .await
    });

    // Let hook1 get into the JS event loop before dispatching hook2.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // hook2: SAME plugin "p", different bundle bytes → triggers retire-don't-kill
    // eviction of hook1's Context.  hook2 also parks 60s.
    let cancel2 = tokio_util::sync::CancellationToken::new();
    let engine2 = engine.clone();
    let c2 = cancel2.clone();
    let hook2 = tokio::spawn(async move {
        engine2
            .dispatch_hook_with_cancel("p", bundle_v2, "V2", "process", "{}", "", "", c2)
            .await
    });

    // Let hook2 settle into its park before cancelling.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // D3: cancel both hooks back-to-back.
    // D2 (bundle mismatch retire) already fired when hook2 was dispatched above.
    cancel1.cancel();
    cancel2.cancel();

    // hook1 must error (cancelled or interrupted).
    let err1 = tokio::time::timeout(std::time::Duration::from_secs(5), hook1)
        .await
        .expect("cancel must resolve hook1 promptly")
        .unwrap()
        .expect_err("hook1 must error after cancel");
    let err1_lower = err1.to_lowercase();
    assert!(
        err1_lower.contains("cancel") || err1_lower.contains("interrupt"),
        "hook1 error must mention cancel or interrupt; got: {err1}"
    );

    // hook2 must error (cancelled or interrupted).
    let err2 = tokio::time::timeout(std::time::Duration::from_secs(5), hook2)
        .await
        .expect("cancel must resolve hook2 promptly")
        .unwrap()
        .expect_err("hook2 must error after cancel");
    let err2_lower = err2.to_lowercase();
    assert!(
        err2_lower.contains("cancel") || err2_lower.contains("interrupt"),
        "hook2 error must mention cancel or interrupt; got: {err2}"
    );

    // Engine must survive both D2 + D3 firing on the same plugin.
    // A third dispatch with bundle_v3 (returns immediately) must succeed.
    let bundle_v3 = r#""use strict"; var V3 = (() => ({
            async process(ctx) { return { survived: true }; }
        }))();"#;
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.dispatch_hook_raw("p", bundle_v3, "V3", "process", "{}", "", ""),
    )
    .await
    .expect("engine must survive D2+D3 concurrent on same plugin")
    .expect("third dispatch must succeed");
    assert!(out.contains("true"), "got: {out}");

    engine.shutdown().await;
}
