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
//!
//! ## `ServeConfig::extra_hosts` — an operator-named exception
//!
//! When `ServeConfig::bind` opts the server into a non-loopback address,
//! `extra_hosts` names the `Host`/`Origin` values that address is reachable
//! as. Those hosts get the same trust loopback gets, for a reason a rebound
//! domain never has: the operator named this one directly, when they set
//! `ServeConfig::extra_hosts`, rather than it arriving by a DNS trick after
//! the fact. An extra host's `Origin` is accepted over `https://` as well as
//! `http://` — unlike loopback, which never has a TLS preview — because a
//! reverse proxy terminating TLS in front of this plain-HTTP engine is the
//! expected way a non-loopback bind gets reached at all.

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

/// True iff `host` names a loopback hostname OR one of `extra_hosts` — exact
/// match, case-insensitive, port stripped the same way loopback matching
/// strips it. See this module's `extra_hosts` doc section for why an
/// operator-named host earns the same trust loopback gets.
pub fn host_is_allowed(host: Option<&str>, extra_hosts: &[String]) -> bool {
    if host_is_loopback(host) {
        return true;
    }
    match host {
        Some(h) if !h.trim().is_empty() => {
            let name = host_name(h);
            extra_hosts.iter().any(|e| e.eq_ignore_ascii_case(name))
        }
        _ => false,
    }
}

/// True iff `origin` is safe: absent (navigations, the health check, same-origin
/// GETs), an `http://` loopback origin (our own page), or an `http://`/`https://`
/// origin naming one of `extra_hosts`. A present foreign origin is rejected, and
/// the opaque `null` origin — sent by a sandboxed iframe on any page — is never
/// allowlisted.
pub fn origin_is_allowed(origin: Option<&str>, extra_hosts: &[String]) -> bool {
    match origin {
        None => true,
        Some(o) if o.trim().is_empty() => true,
        Some("null") => false,
        Some(o) => {
            let o = o.trim();
            if let Some(authority) = o.strip_prefix("http://") {
                let name = host_name(authority);
                if LOOPBACK_HOSTS.contains(&name) || extra_hosts.iter().any(|e| e.eq_ignore_ascii_case(name)) {
                    return true;
                }
            }
            // No `https://` loopback preview exists, so the TLS scheme is
            // accepted only for an operator-named extra host (see this
            // module's `extra_hosts` doc section on why that host — unlike
            // loopback — expects a TLS-terminating proxy in front of it).
            if let Some(authority) = o.strip_prefix("https://") {
                if extra_hosts.iter().any(|e| e.eq_ignore_ascii_case(host_name(authority))) {
                    return true;
                }
            }
            false
        }
    }
}

/// Central middleware: reject a request whose `Host` is not loopback (`421`) or
/// whose `Origin` is present-and-foreign (`403`), before it reaches any route.
///
/// Added as the outermost layer so it runs first — ahead of routing, `ServeDir`
/// and bridge injection — and so it covers **every** route, including
/// `/__moss_health/` (whose `127.0.0.1` Host and absent Origin both pass, which
/// is why startup readiness is not special-cased here).
pub async fn validate_host_origin(
    extra_hosts: std::sync::Arc<Vec<String>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers();

    let host = headers.get(http::header::HOST).and_then(|v| v.to_str().ok());
    if !host_is_allowed(host, &extra_hosts) {
        drain_body(request.into_body()).await;
        return refuse(
            StatusCode::MISDIRECTED_REQUEST,
            "moss preview refuses this Host — it serves loopback (or an explicitly configured host) only.",
        );
    }

    let origin = headers.get(http::header::ORIGIN).and_then(|v| v.to_str().ok());
    if !origin_is_allowed(origin, &extra_hosts) {
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
        assert!(origin_is_allowed(None, &[]));
        assert!(origin_is_allowed(Some(""), &[]));
        // Our own page.
        assert!(origin_is_allowed(Some("http://localhost:8080"), &[]));
        assert!(origin_is_allowed(Some("http://127.0.0.1:8080"), &[]));
        assert!(origin_is_allowed(Some("http://[::1]:8080"), &[]));
        // Foreign, opaque, and wrong-scheme origins are refused.
        assert!(!origin_is_allowed(Some("null"), &[]));
        assert!(!origin_is_allowed(Some("http://evil.com"), &[]));
        assert!(!origin_is_allowed(Some("http://localhost.evil.com"), &[]));
        assert!(!origin_is_allowed(Some("https://localhost:8080"), &[]));
    }

    #[test]
    fn an_extra_host_is_allowed_on_host_and_origin_but_an_unlisted_one_is_not() {
        let extra = vec!["preview.example.com".to_string()];
        // Host: case-insensitive, port stripped, loopback still passes too.
        assert!(host_is_allowed(Some("preview.example.com"), &extra));
        assert!(host_is_allowed(Some("preview.example.com:9443"), &extra));
        assert!(host_is_allowed(Some("PREVIEW.EXAMPLE.COM"), &extra));
        assert!(host_is_allowed(Some("localhost"), &extra));
        assert!(!host_is_allowed(Some("other.example.com"), &extra));
        assert!(!host_is_allowed(Some("preview.example.com.evil.com"), &extra));
        assert!(!host_is_allowed(None, &extra));

        // Origin: both schemes for the extra host (a TLS-terminating proxy is
        // the expected front door), still no https for plain loopback.
        assert!(origin_is_allowed(Some("http://preview.example.com"), &extra));
        assert!(origin_is_allowed(Some("https://preview.example.com"), &extra));
        assert!(origin_is_allowed(Some("http://localhost:8080"), &extra));
        assert!(!origin_is_allowed(Some("https://localhost:8080"), &extra));
        assert!(!origin_is_allowed(Some("http://other.example.com"), &extra));
    }
}
