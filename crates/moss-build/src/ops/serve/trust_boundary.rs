//! The preview server's trust boundary: central `Host` + `Origin` validation.
//!
//! Binding to loopback stops other machines on the LAN and nothing else. It does
//! not stop a webpage in the user's own browser: a page on `evil.com` can send a
//! request to `http://localhost:<port>` and the handler runs — CORS only governs
//! whether the *response* is readable, never whether the request executes. And
//! DNS rebinding upgrades that from a blind write to a same-origin read: the
//! attacker re-resolves their own hostname to `127.0.0.1`, and the browser, which
//! keys the same-origin policy on the *hostname*, now treats the attacker's page
//! as same-origin with us.
//!
//! Two server-side checks close that, and only server-side — the browser will not
//! do it for us:
//!
//! - **`Host`** is the only defense against DNS rebinding. After a rebind the
//!   socket still arrives on our loopback port, but the browser sends the
//!   attacker's hostname (`evil.com`) in `Host`, because that is what the URL bar
//!   says. A page cannot forge `Host` — it is a forbidden header name — so
//!   allowlisting exactly our loopback hostnames rejects the rebound request with
//!   `421 Misdirected Request` while every legitimate client passes.
//! - **`Origin`** rejects an ordinary cross-site request before rebinding even
//!   starts. It is absent on top-level navigations and same-origin GETs (which
//!   must pass), present and loopback for our own page's `fetch`/POST, and
//!   present-and-foreign — or the opaque `null` of a sandboxed iframe — for an
//!   attacker (which must not).
//!
//! This is the MCP Inspector remediation (CVE-2025-49596) and the fix Transmission
//! shipped for CVE-2018-5702, applied here *before* a mutating surface exists
//! rather than after a disclosure. Today the only network route is read-only
//! (`/__moss/source/*` serves authored source bytes), so this layer needs no
//! token yet; the token floor arrives with the first carrier that mutates.
//!
//! The helpers are pure and validate the **hostname only**, never the port: the
//! preview port is dynamic (8080–8179) and is implicitly correct because the
//! connection already arrived on the bound socket. Rebinding is a hostname attack.

use axum::{
    body::Body,
    http::{self, Request, StatusCode},
    middleware::Next,
    response::Response,
};

/// The three loopback hostnames every legitimate preview client sends. The app
/// hands the iframe `http://localhost:<port>`; the CLI health check uses the
/// `127.0.0.1` literal; `::1` is bound by the server and included for robustness
/// even though no client emits it today. A real domain name — including one an
/// attacker has rebound to `127.0.0.1` — is deliberately absent.
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Extract the hostname from a `Host`/authority value, dropping any `:port` and
/// the brackets around an IPv6 literal. `localhost:8080` → `localhost`,
/// `[::1]:8080` → `::1`, `127.0.0.1` → `127.0.0.1`.
fn host_name(value: &str) -> &str {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix('[') {
        // IPv6 literal: `[::1]` or `[::1]:port`. Take what's inside the brackets;
        // a missing `]` is malformed and falls through to fail the allowlist.
        return rest.split(']').next().unwrap_or(rest);
    }
    // `hostname:port` or a bare hostname (a bare hostname has no colon).
    v.split_once(':').map(|(name, _)| name).unwrap_or(v)
}

/// True iff `host` names a loopback hostname. `None`/empty is false: HTTP/1.1
/// requires `Host`, so a missing one is not a client we recognise.
pub fn host_is_loopback(host: Option<&str>) -> bool {
    match host {
        None => false,
        Some(h) if h.trim().is_empty() => false,
        Some(h) => LOOPBACK_HOSTS.contains(&host_name(h)),
    }
}

/// True iff `origin` is safe: absent (navigations, the health check, same-origin
/// GETs) or an `http://` loopback origin (our own page). A present foreign origin
/// is rejected, and the opaque `null` origin — sent by a sandboxed iframe on any
/// page — is never allowlisted.
pub fn origin_is_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) if o.trim().is_empty() => true,
        Some("null") => false,
        Some(o) => match o.trim().strip_prefix("http://") {
            Some(authority) => LOOPBACK_HOSTS.contains(&host_name(authority)),
            // No `https://` loopback preview exists, so a non-http origin to us is
            // not ours.
            None => false,
        },
    }
}

/// Central middleware: reject a request whose `Host` is not loopback (`421`) or
/// whose `Origin` is present-and-foreign (`403`), before it reaches any route.
///
/// Added as the outermost layer so it runs first — ahead of routing, `ServeDir`
/// and bridge injection — and so it covers **every** route, including
/// `/__moss_health/` (whose `127.0.0.1` Host and absent Origin both pass, which
/// is why startup readiness is not special-cased here).
pub async fn validate_host_origin(request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();

    let host = headers.get(http::header::HOST).and_then(|v| v.to_str().ok());
    if !host_is_loopback(host) {
        drain_body(request.into_body()).await;
        return refuse(
            StatusCode::MISDIRECTED_REQUEST,
            "moss preview refuses this Host — it serves loopback only.",
        );
    }

    let origin = headers.get(http::header::ORIGIN).and_then(|v| v.to_str().ok());
    if !origin_is_allowed(origin) {
        drain_body(request.into_body()).await;
        return refuse(
            StatusCode::FORBIDDEN,
            "moss preview refuses this cross-origin request.",
        );
    }

    next.run(request).await
}

/// Drain a request body a refusal is about to drop instead of route.
///
/// Dropping an unread `Body` forces hyper to close the connection rather than
/// risk reusing it — the unread bytes would corrupt the next request's parse.
/// On Windows, a close with bytes still sitting in the socket's receive
/// buffer surfaces to the client as `WSAECONNABORTED` mid-status-line, not
/// the graceful close the same sequence gets on Unix loopback. Shared by
/// every early-refusal gate ahead of the mutation carrier: this module's two,
/// and `carrier_token`'s.
pub(crate) async fn drain_body(body: Body) {
    use http_body_util::{BodyExt, Limited};
    // Bounded because this collects an untrusted body with no size check of its own.
    const DRAIN_LIMIT: usize = 1024 * 1024;
    let _ = Limited::new(body, DRAIN_LIMIT).collect().await;
}

fn refuse(status: StatusCode, message: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(message))
        .expect("static refusal response is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_pass_regardless_of_port_or_brackets() {
        for h in [
            "localhost",
            "localhost:8080",
            "localhost:8179",
            "127.0.0.1",
            "127.0.0.1:8080",
            "[::1]",
            "[::1]:8080",
        ] {
            assert!(host_is_loopback(Some(h)), "should pass: {h}");
        }
    }

    #[test]
    fn a_rebound_domain_is_refused_even_though_it_resolves_to_loopback() {
        // The whole point: after DNS rebinding the socket arrives on our loopback
        // port, but the browser still sends the attacker's hostname in Host. This
        // is the one assertion that proves the rebinding defense.
        assert!(!host_is_loopback(Some("evil.com")));
        assert!(!host_is_loopback(Some("evil.com:8080")));
        // A subdomain/suffix of a loopback name must not sneak through.
        assert!(!host_is_loopback(Some("localhost.evil.com")));
        assert!(!host_is_loopback(Some("127.0.0.1.evil.com")));
        // Other loopback IPs in 127.0.0.0/8 are not on the exact allowlist.
        assert!(!host_is_loopback(Some("127.0.0.2:8080")));
    }

    #[test]
    fn missing_or_empty_host_is_refused() {
        assert!(!host_is_loopback(None));
        assert!(!host_is_loopback(Some("")));
        assert!(!host_is_loopback(Some("   ")));
    }

    #[test]
    fn origin_absent_passes_but_foreign_and_null_do_not() {
        // Absent: top-level navigation, the CLI health check, same-origin GET.
        assert!(origin_is_allowed(None));
        assert!(origin_is_allowed(Some("")));
        // Our own page.
        assert!(origin_is_allowed(Some("http://localhost:8080")));
        assert!(origin_is_allowed(Some("http://127.0.0.1:8080")));
        assert!(origin_is_allowed(Some("http://[::1]:8080")));
        // Foreign, opaque, and wrong-scheme origins are refused.
        assert!(!origin_is_allowed(Some("null")));
        assert!(!origin_is_allowed(Some("http://evil.com")));
        assert!(!origin_is_allowed(Some("http://localhost.evil.com")));
        assert!(!origin_is_allowed(Some("https://localhost:8080")));
    }
}
