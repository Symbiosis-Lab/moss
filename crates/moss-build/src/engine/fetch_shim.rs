//! fetch() shim returning a minimal Response/Headers (report 5 §1d). Routes to
//! do_http_request_full (PUT/HEAD/headers). The async fetch fn uses Func::from(Async(..))
//! → real JS Promise. json()/text() are also async over the captured body. Headers.get
//! is case-insensitive (keys stored lowercased).

use rquickjs::{Ctx, Exception, Object, Result, function::Async, prelude::Func};

use crate::plugins::runtime::download::{do_http_request_full, FetchMethod};

/// Build the Response object from a completed FetchFull.
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

/// Install globalThis.fetch.
pub fn install_fetch(ctx: &Ctx<'_>) -> Result<()> {
    ctx.globals().set("fetch", Func::from(Async(host_fetch)))?;
    Ok(())
}

/// fetch(url, init?) -> Promise<Response>. init = { method?, headers?, body? }.
///
/// All JS access (reading `init`) happens BEFORE the `do_http_request_full(...).await`
/// — no JS handle is touched after the I/O await (async-lock discipline).
async fn host_fetch<'js>(
    ctx: Ctx<'js>,
    url: String,
    init: rquickjs::function::Opt<Object<'js>>,
) -> Result<Object<'js>> {
    let (method_str, headers, body) = match init.0 {
        None => ("GET".to_string(), None, None),
        Some(obj) => {
            let m: Option<String> = obj.get("method").ok();
            let b: Option<String> = obj.get("body").ok();
            let h: Option<std::collections::HashMap<String, String>> = obj.get("headers").ok();
            (m.unwrap_or_else(|| "GET".into()).to_uppercase(), h, b)
        }
    };
    let method = match method_str.as_str() {
        "GET" => FetchMethod::Get,
        "HEAD" => FetchMethod::Head,
        "POST" => FetchMethod::Post { body: body.unwrap_or_default() },
        "PUT" => FetchMethod::Put { body: body.unwrap_or_default() },
        other => return Err(Exception::throw_type(&ctx, &format!("Unsupported method: {other}"))),
    };
    let full = do_http_request_full(url, method, headers, Some(30_000))
        .await
        .map_err(|e| Exception::throw_message(&ctx, &e))?;
    build_response(&ctx, full.status, full.ok, full.headers, full.body)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rquickjs::{async_with, AsyncContext, AsyncRuntime};
    use std::io::Write;
    use std::net::TcpListener;

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
    async fn fetch_returns_response_with_json_and_caseinsensitive_header() {
        let port = spawn_stub();
        let local = tokio::task::LocalSet::new();
        local.run_until(async move {
            let rt = AsyncRuntime::new().unwrap();
            let ctx = AsyncContext::full(&rt).await.unwrap();
            let (ok, login, scopes): (bool, String, String) = async_with!(ctx => |ctx| {
                install_fetch(&ctx).unwrap();
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
