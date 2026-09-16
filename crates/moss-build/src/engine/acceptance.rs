//! #789 Phase-2 acceptance: a synthetic IIFE plugin exercises invoke + event.listen
//! + fetch + setTimeout through QuickJsEngine end-to-end. Proves the host-binding
//! shim WITHOUT github/matters auth (Phase 3).

#[cfg(test)]
mod tests {
    use crate::engine::host_fns::dispatch_command_test_stub;
    use crate::engine::QuickJsEngine;

    const SYNTHETIC_BUNDLE: &str = r#"
        "use strict";
        var SyntheticPlugin = (() => ({
            async process(ctx) {
                // 1. setTimeout works (tokio-backed).
                await new Promise((r) => setTimeout(r, 5));
                // 2. event.listen registers; host will deliver 'browser-closed'.
                let closedReason = null;
                await globalThis.__TAURI__.event.listen('browser-closed', (e) => {
                    closedReason = e.payload.reason;
                });
                // 3. fetch the stub server.
                const r = await fetch(ctx.stubUrl);
                const ok = r.ok;
                // 4. invoke html_to_markdown via __TAURI__.core.invoke.
                const md = await globalThis.__TAURI__.core.invoke('html_to_markdown', { html: '<b>hi</b>' });
                // 5. Bounded poll for the host-delivered event — deterministic.
                for (let i = 0; i < 50 && closedReason === null; i++) {
                    await new Promise((r) => setTimeout(r, 10));
                }
                return { timerRan: true, fetchOk: ok, markdown: md, closedReason };
            }
        }))();
    "#;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn synthetic_plugin_runs_through_quickjs_engine() {
        let port = crate::engine::fetch_shim::tests::spawn_stub();
        let stub_url = format!("http://127.0.0.1:{port}/x");

        let (engine, mut dispatch_rx) = QuickJsEngine::new(None);

        // Host dispatch task: answer html_to_markdown WITHOUT an AppHandle (test stub).
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let r = dispatch_command_test_stub("/tmp/p", req.plugin.as_deref(), &req.cmd, &req.args_json).await;
                let _ = req.reply.send(r);
            }
        });

        // Delivered while the hook is in flight; the plugin's bounded poll
        // loop observes it deterministically.
        let deliver =
            crate::engine::tests::deliver_event_until_aborted(&engine, "synthetic", "browser-closed", r#"{"reason":"user"}"#);

        let result_json = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            engine.dispatch_hook_raw(
                "synthetic",
                SYNTHETIC_BUNDLE,
                "SyntheticPlugin",
                "process",
                &serde_json::json!({ "stubUrl": stub_url }).to_string(),
                "/tmp/proj",
                "/tmp/proj/.moss",
            ),
        )
        .await
        .expect("hook must not hang (10s timeout)")
        .expect("dispatch ok");
        deliver.abort();

        let v: serde_json::Value = serde_json::from_str(&result_json).unwrap();
        assert_eq!(v["timerRan"], serde_json::json!(true));
        assert_eq!(v["fetchOk"], serde_json::json!(true));
        assert_eq!(v["markdown"], serde_json::json!("**hi**")); // htmd converts <b> -> **
        assert_eq!(v["closedReason"], serde_json::json!("user"));

        engine.shutdown().await;
    }

    /// C.11 live gate (MOSS_NET_TESTS + GITHUB_TOKEN): the engine fetch shim
    /// against real api.github.com — case-folded X-OAuth-Scopes header read,
    /// exactly auth.ts:135-150's shape. Run manually:
    ///   MOSS_NET_TESTS=1 GITHUB_TOKEN=$(gh auth token) cargo test --lib --features mcp live_github_scopes
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn live_github_scopes_header_through_engine_fetch() {
        if std::env::var("MOSS_NET_TESTS").is_err() { return; }
        let Ok(token) = std::env::var("GITHUB_TOKEN") else { return; };
        let bundle = r#""use strict"; var P = (() => ({
            async probe(ctx) {
                const r = await fetch("https://api.github.com/user", {
                    headers: { "Authorization": "Bearer " + ctx.token }
                });
                return { ok: r.ok, scopes: r.headers.get("X-OAuth-Scopes") };
            }
        }))();"#;
        let (engine, mut dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let _ = req.reply.send(Err("no cmd".into()));
            }
        });
        let ctx = serde_json::json!({ "token": token }).to_string();
        let out = engine
            .dispatch_hook_raw("p", bundle, "P", "probe", &ctx, "", "")
            .await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true), "got {v}");
        assert!(v["scopes"].is_string(), "X-OAuth-Scopes must surface through the case-folded shim; got {v}");
        engine.shutdown().await;
    }

    /// C.12: the matters waitForToken topology — initial delay, cookie poll loop,
    /// browser-closed abort — at 1000x-shortened timings, with scripted
    /// get_plugin_cookie responses (empty ×3, then the token).
    const WAIT_FOR_TOKEN_BUNDLE: &str = r#""use strict"; var P = (() => ({
        async process(ctx) {
            let closed = false;
            await globalThis.__TAURI__.event.listen('browser-closed', () => { closed = true; });
            const initialDelayMs = 20, pollIntervalMs = 10, maxWaitMs = 2000;
            await new Promise(r => setTimeout(r, initialDelayMs));
            const start = Date.now();
            while (Date.now() - start < maxWaitMs) {
                if (closed) return { success: true, message: "closed" };
                const cookies = await globalThis.__TAURI__.core.invoke('get_plugin_cookie',
                    { pluginName: 'matters', projectPath: '/p' });
                const tok = (cookies || []).find(c => c.name === '__access_token');
                if (tok) return { success: true, message: "token:" + tok.value };
                await new Promise(r => setTimeout(r, pollIntervalMs));
            }
            return { success: false, message: "timeout" };
        }
    }))();"#;

    fn scripted_cookie_host(
        mut dispatch_rx: tokio::sync::mpsc::UnboundedReceiver<crate::engine::host_fns::DispatchRequest>,
        empty_polls_before_token: usize,
        serve_token_ever: bool,
    ) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls2 = calls.clone();
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let reply = match req.cmd.as_str() {
                    "get_plugin_cookie" => {
                        let n = calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if serve_token_ever && n >= empty_polls_before_token {
                            Ok(r#"[{"name":"__access_token","value":"tok-123","domain":"matters.town"}]"#.to_string())
                        } else {
                            Ok("[]".to_string())
                        }
                    }
                    "__engine_listen__" => Ok("null".to_string()),
                    other => Err(format!("no cmd: {other}")),
                };
                let _ = req.reply.send(reply);
            }
        });
        calls
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn matters_long_poll_resolves_on_cookie_arrival() {
        let (engine, dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        let calls = scripted_cookie_host(dispatch_rx, 3, true);
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            engine.dispatch_hook_raw("matters", WAIT_FOR_TOKEN_BUNDLE, "P", "process", "{}", "/p", "/p/.moss"),
        ).await.expect("must not hang").unwrap();
        assert!(out.contains("token:tok-123"), "got {out}");
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 4, "the poll loop must have iterated");
        engine.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn matters_long_poll_aborts_on_browser_closed() {
        let (engine, dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        let _calls = scripted_cookie_host(dispatch_rx, usize::MAX, false);
        let deliver_engine = engine.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            deliver_engine.deliver_event("matters", "browser-closed", r#"{"reason":"user"}"#);
        });
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            engine.dispatch_hook_raw("matters", WAIT_FOR_TOKEN_BUNDLE, "P", "process", "{}", "/p", "/p/.moss"),
        ).await.expect("must not hang").unwrap();
        assert!(out.contains("closed"), "browser-closed must abort the poll; got {out}");
        engine.shutdown().await;
    }

    // ===== C.13: engine-parity + ordering + class-eliminator regression =====

    /// C.13 parity: rejected Promises must surface as Err (never hang or kill the
    /// engine), and a caught rejection must resolve normally. Spec §7.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rejected_hook_promise_becomes_error_and_caught_rejection_resolves() {
        let (engine, mut dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let _ = req.reply.send(Err("no cmd".into()));
            }
        });
        let bundle = r#""use strict"; var P = (() => ({
            async reject(ctx) { return Promise.reject(new Error("nope")); },
            async caught(ctx) { return await Promise.reject(new Error("x")).catch(() => ({ recovered: true })); }
        }))();"#;
        let err = engine.dispatch_hook_raw("p", bundle, "P", "reject", "{}", "", "")
            .await.expect_err("rejection must surface as Err");
        assert!(err.contains("nope"), "got {err}");
        let out = engine.dispatch_hook_raw("p", bundle, "P", "caught", "{}", "", "")
            .await.expect(".catch must recover");
        assert!(out.contains("recovered"));
        engine.shutdown().await;
    }

    /// C.13 ordering: quickjs must run sync → microtask → macrotask in browser
    /// order. The spec §7 parity requirement.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn microtask_runs_before_timer_macrotask() {
        let (engine, mut dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let _ = req.reply.send(Err("no cmd".into()));
            }
        });
        let bundle = r#""use strict"; var P = (() => ({
            async process(ctx) {
                const order = [];
                const macro_ = new Promise(r => setTimeout(() => { order.push("macro"); r(); }, 0));
                Promise.resolve().then(() => order.push("micro"));
                order.push("sync");
                await macro_;
                return { order };
            }
        }))();"#;
        let out = engine.dispatch_hook_raw("p", bundle, "P", "process", "{}", "", "").await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["order"], serde_json::json!(["sync", "micro", "macro"]),
            "quickjs ordering must match browser semantics; got {v}");
        engine.shutdown().await;
    }

    /// C.13 wire-shape parity: the fetch_url arm's reply must have EXACTLY the four
    /// keys {status, ok, body_base64, content_type} (snake_case) with no extras.
    /// A field rename or addition would silently break every plugin's HTTP path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_url_arm_reply_has_exact_snake_case_shape() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/shape")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("hello")
            .create_async()
            .await;
        let args = serde_json::json!({
            "url": format!("{}/shape", server.url()),
            "timeoutMs": 5000
        })
        .to_string();
        let reply = dispatch_command_test_stub("/tmp/none", None, "fetch_url", &args)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        let obj = v.as_object().expect("fetch_url reply must be a JSON object");
        assert_eq!(
            obj.len(),
            4,
            "fetch_url reply must have EXACTLY 4 keys (status, ok, body_base64, content_type); got {obj:?}"
        );
        assert!(obj.contains_key("status"), "missing 'status'");
        assert!(obj.contains_key("ok"), "missing 'ok'");
        assert!(obj.contains_key("body_base64"), "missing 'body_base64'");
        assert!(obj.contains_key("content_type"), "missing 'content_type'");
        assert!(
            !obj.contains_key("bodyBase64"),
            "camelCase 'bodyBase64' would break every plugin"
        );
    }

    /// C.13 class-eliminator regression: the original Phase-0 deterministic repro is
    /// "Rust blocks on a signal from a suspended WEBVIEW". On quickjs the class is
    /// structurally absent because there is NO webview. A full-featured hook (timers +
    /// fetch-shim against a local stub + invoke + event listen/deliver) completes with
    /// ZERO webviews in the process — which is literally `new(None)`'s
    /// environment (design-spec §7: "the Phase-0 deterministic repro becomes a
    /// regression test that passes on quickjs"). Headless == the strongest form of
    /// the repro: no webview exists to suspend.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn hook_completes_with_zero_webviews_in_process() {
        let port = crate::engine::fetch_shim::tests::spawn_stub();
        let stub_url = format!("http://127.0.0.1:{port}/x");

        let (engine, mut dispatch_rx) = QuickJsEngine::new(None);
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let r = dispatch_command_test_stub("/tmp/p", req.plugin.as_deref(), &req.cmd, &req.args_json).await;
                let _ = req.reply.send(r);
            }
        });

        // Deliver browser-closed while the hook is parked — the engine must not
        // require any webview to unblock it.
        let deliver =
            crate::engine::tests::deliver_event_until_aborted(&engine, "zero-wv", "browser-closed", r#"{"reason":"user"}"#);

        let result_json = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            engine.dispatch_hook_raw(
                "zero-wv",
                SYNTHETIC_BUNDLE,
                "SyntheticPlugin",
                "process",
                &serde_json::json!({ "stubUrl": stub_url }).to_string(),
                "/tmp/proj",
                "/tmp/proj/.moss",
            ),
        )
        .await
        .expect("hook must not hang (10s timeout — zero-webview class-eliminator)")
        .expect("dispatch ok");
        deliver.abort();

        let v: serde_json::Value = serde_json::from_str(&result_json).unwrap();
        assert_eq!(v["timerRan"], serde_json::json!(true));
        assert_eq!(v["fetchOk"], serde_json::json!(true));
        assert_eq!(v["closedReason"], serde_json::json!("user"));

        engine.shutdown().await;
    }

    // ===== C.12 live gate (below) =====

}
