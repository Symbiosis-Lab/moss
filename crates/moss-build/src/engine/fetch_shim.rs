//! fetch() shim returning a minimal Response/Headers. Routes through the
//! host's `fetch` dispatch, which checks the plugin's declared hosts. The
//! async fetch fn uses Func::from(Async(..)) → real JS Promise. json()/text() are also async over the captured body. Headers.get
//! is case-insensitive (keys stored lowercased).

use rquickjs::{Ctx, Exception, Object, Result, function::{Async, Opt}, prelude::Func};

use super::host_fns::DispatchBridge;

/// Build the Response object from a completed fetch.
///
/// `json()`/`text()` are async host fns over the captured body, each registered as
/// `Func::from(Async(closure))`. The closures capture ONLY an owned `Vec<u8>` (a
/// `Send`/`'static` value — never a JS handle), so the returned future borrows
/// nothing from `'js` and the async-lock discipline holds across the I/O boundary.
///
/// `text()` resolves `String` directly. `json()` returns `Value<'js>`, which a
/// closure cannot generalize over (the `for<'js>` HRTB fails on the invariant
/// `'js` — the same constraint the URL/timer shims hit), so its closure forwards to
/// the FREE async fn [`json_body`] (explicit `<'js>` param) which does generalize.
fn build_response<'js>(
    ctx: &Ctx<'js>,
    status: u16,
    ok: bool,
    headers: std::collections::HashMap<String, String>,
    body: Vec<u8>,
) -> Result<Object<'js>> {
    let resp = Object::new(ctx.clone())?;
    resp.set("ok", ok)?;
    resp.set("status", status)?;

    // Headers { get(name): string|null } — case-insensitive (keys already lowercased).
    let headers_obj = Object::new(ctx.clone())?;
    let hmap = headers;
    headers_obj.set(
        "get",
        Func::from(move |name: String| -> Option<String> {
            hmap.get(&name.to_lowercase()).cloned()
        }),
    )?;
    resp.set("headers", headers_obj)?;

    // text(): Promise<string>. The captured `Vec<u8>` is owned + 'static, so the
    // returned future borrows no JS handle — the async-lock discipline holds.
    let body_for_text = body.clone();
    resp.set(
        "text",
        Func::from(Async(move || {
            let b = body_for_text.clone();
            async move { Ok::<String, rquickjs::Error>(String::from_utf8_lossy(&b).into_owned()) }
        })),
    )?;

    // json(): Promise<any> — parse the body JSON. Registered as a FREE async fn so
    // the `Value<'js>` return generalizes; the closure just clones the owned bytes
    // and forwards `ctx` (no JS handle captured across the I/O boundary).
    let body_for_json = body;
    resp.set(
        "json",
        Func::from(Async(move |ctx: Ctx<'js>| {
            let b = body_for_json.clone();
            json_body(ctx, b)
        })),
    )?;
    Ok(resp)
}

/// Parse the captured body as JSON into a JS value. Free async fn with an explicit
/// `<'js>` param: the `Value<'js>` return generalizes over the JS lifetime.
async fn json_body<'js>(ctx: Ctx<'js>, body: Vec<u8>) -> Result<rquickjs::Value<'js>> {
    let s = String::from_utf8_lossy(&body).into_owned();
    ctx.json_parse(s)
}

/// Install globalThis.fetch. `bridge` is bound to the Context's plugin, so a
/// request reaches only the hosts that plugin declares.
pub fn install_fetch<'js>(ctx: &Ctx<'js>, bridge: DispatchBridge, ended: std::rc::Rc<std::cell::Cell<bool>>) -> Result<()> {
    ctx.globals().set(
        "fetch",
        Func::from(Async(move |ctx: Ctx<'js>, url: String, init: Opt<Object<'js>>| {
            let ended = ended.get();
            // Read `init` now: the async body must not touch a JS handle.
            let args = fetch_args(url, init.0);
            let bridge = bridge.clone();
            async move {
                if ended {
                    return Err(super::web_shims::run_ended(&ctx));
                }
                host_fetch(ctx, bridge, args).await
            }
        })),
    )?;
    Ok(())
}

/// fetch(url, init?) as the `fetch` dispatch's args. init = { method?, headers?, body?, redirect? }.
fn fetch_args(url: String, init: Option<Object<'_>>) -> serde_json::Value {
    let mut args = serde_json::json!({ "url": url });
    if let Some(obj) = init {
        for key in ["method", "body", "redirect"] {
            if let Ok(Some(v)) = obj.get::<_, Option<String>>(key) {
                args[key] = v.into();
            }
        }
        if let Ok(Some(h)) = obj.get::<_, Option<std::collections::HashMap<String, String>>>("headers") {
            args["headers"] = serde_json::json!(h);
        }
    }
    args
}

async fn host_fetch<'js>(ctx: Ctx<'js>, bridge: DispatchBridge, args: serde_json::Value) -> Result<Object<'js>> {
    #[derive(serde::Deserialize)]
    struct Reply {
        status: u16,
        ok: bool,
        headers: std::collections::HashMap<String, String>,
        body_base64: String,
    }
    use base64::Engine as _;
    let reply = bridge.call("fetch".into(), args.to_string()).await.map_err(|e| Exception::throw_message(&ctx, &e))?;
    let r: Reply = serde_json::from_str(&reply).map_err(|e| Exception::throw_message(&ctx, &format!("bad fetch reply: {e}")))?;
    let body = base64::engine::general_purpose::STANDARD.decode(r.body_base64).map_err(|e| Exception::throw_message(&ctx, &e.to_string()))?;
    build_response(&ctx, r.status, r.ok, r.headers, body)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};
    use std::io::Write;
    use std::net::TcpListener;

    /// A bridge answering as `plugin` declaring `hosts`.
    pub(crate) fn bridge_declaring(plugin: &'static str, hosts: &'static [&'static str]) -> DispatchBridge {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::engine::host_fns::DispatchRequest>();
        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let r = crate::engine::host_fns::dispatch_command_test_stub_declaring("/tmp/none", plugin, hosts, &req.cmd, &req.args_json).await;
                let _ = req.reply.send(r);
            }
        });
        DispatchBridge::new(tx)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_reaches_only_the_hosts_its_plugin_declares() {
        let port = spawn_stub();
        let local = tokio::task::LocalSet::new();
        let outcome: String = local.run_until(async move {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            async_with!(ctx => |ctx| {
                install_fetch(&ctx, bridge_declaring("p", &["api.example.com"]), Default::default()).unwrap();
                let code = format!("let outcome; try {{ await fetch('http://127.0.0.1:{port}/user'); outcome = 'reached'; }} catch (e) {{ outcome = e.message; }} outcome");
                let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
                let envelope: Object = promise.into_future().await.unwrap();
                envelope.get("value").unwrap()
            }).await
        }).await;
        assert_eq!(outcome, "p tried to reach 127.0.0.1, which it does not list in its manifest.");
    }

    // Tiny one-shot HTTP server returning a fixed JSON body + a custom header.
    // pub(crate): the Task-14 acceptance test reuses it.
    pub(crate) fn spawn_stub() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut s = stream.unwrap();
                use std::io::Read;
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let body = br#"{"login":"octocat"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-OAuth-Scopes: repo, user\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.write_all(body);
            }
        });
        port
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_redirect_options_reach_the_transport() {
        use crate::plugins::runtime::redirect_tests::RedirectServer;
        for (option, expected, requests) in [("error", "rejected", 0), ("manual", "302", 0), ("follow", "200", 1), ("", "200", 1)] {
            let server = RedirectServer::new(302);
            let url = server.url.clone();
            let local = tokio::task::LocalSet::new();
            let result: String = local.run_until(async move {
                let rt = AsyncRuntime::new().unwrap();
                let ctx = AsyncContext::full(&rt).await.unwrap();
                async_with!(ctx => |ctx| {
                    install_fetch(&ctx, bridge_declaring("local", &["127.0.0.1"]), Default::default()).unwrap();
                    let redirect = if option.is_empty() { String::new() } else { format!(", redirect: '{option}'") };
                    let code = format!("let outcome; try {{ outcome = String((await fetch('{url}', {{method: 'POST', body: 'payload'{redirect}}})).status); }} catch (e) {{ outcome = 'rejected'; }} outcome");
                    let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
                    let envelope: Object = promise.into_future().await.unwrap();
                    envelope.get("value").unwrap()
                }).await
            }).await;
            let (body, actual) = server.finish();
            assert_eq!(body, "payload");
            assert_eq!(actual, requests, "{option}");
            assert_eq!(result, expected, "{option}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_sends_a_delete() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::thread::spawn(move || {
            let mut s = listener.incoming().next().unwrap().unwrap();
            use std::io::Read;
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).unwrap();
            let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or_default().to_string()
        });
        let local = tokio::task::LocalSet::new();
        let status: u16 = local.run_until(async move {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            async_with!(ctx => |ctx| {
                install_fetch(&ctx, bridge_declaring("local", &["127.0.0.1"]), Default::default()).unwrap();
                let code = format!("(await fetch('http://127.0.0.1:{port}/posts/7', {{ method: 'DELETE' }})).status");
                let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
                let envelope: Object = promise.into_future().await.unwrap();
                envelope.get("value").unwrap()
            }).await
        }).await;
        assert_eq!(status, 204);
        assert_eq!(seen.join().unwrap(), "DELETE /posts/7 HTTP/1.1");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_returns_response_with_json_and_caseinsensitive_header() {
        let port = spawn_stub();
        let local = tokio::task::LocalSet::new();
        local.run_until(async move {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            let (ok, login, scopes): (bool, String, String) = async_with!(ctx => |ctx| {
                install_fetch(&ctx, bridge_declaring("local", &["127.0.0.1"]), Default::default()).unwrap();
                // Top-level await: with `promise: true` (eval_promise) the eval result
                // promise resolves to the async-eval envelope `{ value: <completion> }`
                // — QuickJS's JS_EVAL_FLAG_ASYNC shape. The fetch/json host-fn promises
                // are awaited INSIDE async_with (rquickjs polls Async host fns through the
                // same with-machinery — NO external rt.drive() task, which would contend
                // for the runtime and stall the host-fn futures).
                let code = format!(r#"
                    const r = await fetch('http://127.0.0.1:{port}/user');
                    const j = await r.json();
                    ({{ ok: r.ok, login: j.login, scopes: r.headers.get('x-oauth-scopes') }})
                "#);
                let promise: rquickjs::Promise = ctx.eval_promise(code.as_bytes()).unwrap();
                let envelope: Object = promise.into_future().await.unwrap();
                let res: Object = envelope.get("value").unwrap(); // unwrap async-eval { value }
                (res.get("ok").unwrap(), res.get("login").unwrap(), res.get("scopes").unwrap())
            }).await;
            assert!(ok);
            assert_eq!(login, "octocat");
            assert_eq!(scopes, "repo, user"); // case-insensitive: queried 'x-oauth-scopes'
        }).await;
    }
}
