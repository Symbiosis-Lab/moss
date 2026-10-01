//! `GET /__moss/session?token=<t>&next=<path>` — exchange the carrier token
//! for a `moss_token` cookie, so a browser tab reached through
//! `ServeConfig::bind`/`extra_hosts` can carry the session without any
//! client-side script reading `X-Moss-Token` itself.
//!
//! An infrastructure route like health and source (`router.rs`'s ordering
//! rule): registered unconditionally, no frontend binding, never a carrier
//! command.
//!
//! ## Contract
//!
//! * A `token` matching the bound session's → **303** to `next`, with
//!   `Set-Cookie: moss_token=<t>; HttpOnly; SameSite=Strict; Path=/` (plus
//!   `Secure` when the request arrived over TLS — see [`request_is_tls`]).
//!   `next` must be a same-origin path starting with `/`; anything else is
//!   **400** before a cookie is ever set.
//! * No carrier bound, or a `token` that does not match → **401**, the exact
//!   body [`super::carrier_token::unauthorized`] already produces for the
//!   other token-gated routes.
//! * The token never appears in a log line this module writes. [`maybe_announce_line`]
//!   is the one deliberate exception elsewhere in this contract — see
//!   `carrier_token.rs`'s "Security hygiene" section for why.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};

/// Query params `GET /__moss/session` takes. Both required — a request
/// missing either is a 400 from axum's `Query` extractor before this
/// module's own logic runs.
#[derive(serde::Deserialize)]
pub(crate) struct SessionQuery {
    pub(crate) token: String,
    pub(crate) next: String,
}

/// True iff this request arrived over TLS, as reported by a terminating
/// reverse proxy. The engine itself always speaks plain HTTP — checked only
/// when `extra_hosts` is non-empty, because a loopback-only server has no
/// proxy in front of it to set this header, and trusting it there would let
/// a local page mint itself a `Secure` cookie for nothing.
fn request_is_tls(headers: &HeaderMap, extra_hosts_configured: bool) -> bool {
    extra_hosts_configured
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

/// A `next` this route will redirect to: a same-origin path. Starting with
/// `/` but not `//` or `/\` — both are a protocol-relative URL a browser
/// resolves against another origin (a special-scheme URL normalises a
/// backslash to a slash), which is exactly what "same-origin" rules out.
fn next_is_same_origin(next: &str) -> bool {
    // A browser normalises a backslash to a slash in a special-scheme URL, so
    // `/\evil.com` resolves to `//evil.com` — the same protocol-relative
    // escape `//` guards against — even though it does not literally start
    // with `//` here.
    next.starts_with('/')
        && !matches!(next.as_bytes().get(1), Some(b'/') | Some(b'\\'))
}

fn bad_next() -> Response {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(
            "moss session route requires a same-origin `next` path starting with `/`.",
        ))
        .expect("static 400 response is always valid")
}

/// Handler for `GET /__moss/session`. See the module doc for the contract.
pub(crate) async fn handle_session(
    ctx: Option<super::invoke::InvokeCtx>,
    site_dir: Arc<RwLock<PathBuf>>,
    extra_hosts: Arc<Vec<String>>,
    query: SessionQuery,
    headers: HeaderMap,
) -> Response {
    let Some(ctx) = ctx else {
        return super::carrier_token::unauthorized();
    };
    let Some(session) = ctx.bind(&site_dir) else {
        return super::carrier_token::unauthorized();
    };
    if !super::carrier_token::constant_time_eq(&query.token, session.token()) {
        return super::carrier_token::unauthorized();
    }
    if !next_is_same_origin(&query.next) {
        return bad_next();
    }

    let secure = if request_is_tls(&headers, !extra_hosts.is_empty()) {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/{secure}",
        super::carrier_token::SESSION_COOKIE,
        query.token,
    );
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::SET_COOKIE, cookie)
        .header(header::LOCATION, query.next)
        .header("cache-control", "no-store")
        .body(Body::empty())
        .expect("static 303 response is always valid")
}

/// The one line `ServeConfig::announce_token` prints to stderr — a function
/// so a test can assert on the exact string without capturing a real stderr
/// writer.
pub(crate) fn announce_line(host: &str, port: u16, token: &str) -> String {
    format!("Open http://{host}:{port}/__moss/session?token={token}&next=/ to sign in")
}

/// What `router::start_server` prints at bind time, computed as pure data so
/// the "prints nothing" cases are a unit test rather than a stderr capture.
/// `None` whenever any of the three conditions this feature requires is
/// missing: `announce_token` opted in, `bind` actually configured a
/// non-loopback address, and a session token exists to announce (absent only
/// when the served directory resolves to no vault).
pub(crate) fn maybe_announce_line(
    bind: Option<std::net::IpAddr>,
    announce_token: bool,
    extra_hosts: &[String],
    port: u16,
    token: Option<&str>,
) -> Option<String> {
    let addr = bind.filter(|_| announce_token)?;
    let token = token?;
    let host = extra_hosts
        .first()
        .cloned()
        .unwrap_or_else(|| addr.to_string());
    Some(announce_line(&host, port, token))
}

#[cfg(test)]
mod tests {
    use super::super::invoke::InvokeCtx;
    use super::super::router::{start_server, ServeConfig};
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn served_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let vault = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let site_dir = vault.path().join(".moss/build.nosync/current");
        std::fs::create_dir_all(&site_dir).unwrap();
        (vault, site_dir)
    }

    /// Raw TCP so the response is read before any client follows the 303 —
    /// `ureq` would otherwise chase the redirect and hand back the wrong
    /// response's headers.
    fn raw_get(port: u16, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        let status = resp
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (status, resp)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_valid_token_sets_the_cookie_and_redirects_to_next() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64300)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let (status, resp) = raw_get(port, &format!("/__moss/session?token={token}&next=/editor"));
        assert_eq!(status, 303, "full response: {resp}");
        assert!(
            resp.to_lowercase().contains("location: /editor"),
            "must redirect to next: {resp}"
        );
        assert!(
            resp.contains(&format!("moss_token={token}"))
                && resp.to_lowercase().contains("httponly")
                && resp.to_lowercase().contains("samesite=strict"),
            "must set the session cookie: {resp}"
        );

        let _ = shutdown_tx.send(());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wrong_token_is_401() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64310)
        })
        .await
        .expect("server should start");
        let _ = ctx.token();

        let (status, _) = raw_get(port, "/__moss/session?token=not-the-real-token&next=/");
        assert_eq!(status, 401);

        let _ = shutdown_tx.send(());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_non_same_origin_next_is_400() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64320)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("bound").to_string();

        let (status, _) = raw_get(port, &format!("/__moss/session?token={token}&next=//evil.com"));
        assert_eq!(status, 400, "a protocol-relative next must be refused");

        // `/\evil.com` does not literally start with `//`, but a browser
        // normalises the backslash to a slash in a special-scheme URL, so it
        // resolves to `//evil.com` — the same open redirect by another name.
        let (status, _) =
            raw_get(port, &format!("/__moss/session?token={token}&next=/%5Cevil.com"));
        assert_eq!(status, 400, "a backslash-escaped next must be refused too");

        let _ = shutdown_tx.send(());
    }

    #[test]
    fn maybe_announce_line_is_none_unless_bind_the_flag_and_a_token_all_agree() {
        let addr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        assert!(
            maybe_announce_line(None, true, &[], 8080, Some("tok")).is_none(),
            "bind: None must never announce, even with the flag on"
        );
        assert!(maybe_announce_line(Some(addr), false, &[], 8080, Some("tok")).is_none());
        assert!(maybe_announce_line(Some(addr), true, &[], 8080, None).is_none());
    }

    #[test]
    fn maybe_announce_line_names_the_first_extra_host_and_the_token() {
        let addr = std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0));
        let extra = vec!["preview.example.com".to_string()];
        let line = maybe_announce_line(Some(addr), true, &extra, 8080, Some("tok123"))
            .expect("all three conditions hold");
        assert_eq!(
            line,
            "Open http://preview.example.com:8080/__moss/session?token=tok123&next=/ to sign in"
        );
    }
}
