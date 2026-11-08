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
//! shipped for CVE-2018-5702. The token-free routes are read-only; the mutating
//! carriers add a token on top (`carrier_token`).
//!
//! Allowed origins derive from the bound port and, for the UI listener, one
//! optional HTTPS public origin. A request must address one of them by `Host`,
//! and an `Origin` it carries must be that same origin.

use axum::{
    body::Body,
    http::{self, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::Response,
};

/// A full HTTPS origin (scheme, host and optional port; nothing else) that a
/// reverse proxy serves the UI at. Checked once, where it is parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicOrigin(String);

impl PublicOrigin {
    pub fn parse(origin: &str) -> Result<Self, String> {
        let url = url::Url::parse(origin).map_err(|_| "invalid public URL")?;
        if url.scheme() != "https" || url.origin().ascii_serialization() != origin {
            return Err("public URL must be a full HTTPS origin without a path".into());
        }
        Ok(Self(origin.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Every origin a listener on `port` answers as, each with the `Host`
/// authority that names it.
pub(crate) fn origins(port: u16, public: Option<&PublicOrigin>) -> Vec<(String, String)> {
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .map(|host| format!("http://{host}:{port}"))
        .chain(public.map(|p| p.as_str().to_owned()))
        .map(|origin| {
            let authority = origin.split_once("://").map_or("", |(_, a)| a).to_owned();
            (origin, authority)
        })
        .collect()
}

/// Why [`admit_origin`] refused a request; [`Refusal::response`] answers it.
pub(crate) struct Refusal(StatusCode, &'static str);

impl Refusal {
    /// Drains the body first: see [`drain_body`].
    pub(crate) async fn response(self, request: Request<Body>) -> Response {
        drain_body(request.into_body()).await;
        refuse(self.0, self.1)
    }
}

/// The origin a request addresses, or its refusal: `421` when its `Host` names
/// none of `origins`, `403` when it carries an `Origin` other than that one.
pub(crate) fn admit_origin<'a>(headers: &HeaderMap, origins: &'a [(String, String)]) -> Result<&'a str, Refusal> {
    let host = headers.get(http::header::HOST).and_then(|v| v.to_str().ok());
    let Some((origin, _)) = origins.iter().find(|(_, authority)| Some(authority.as_str()) == host) else {
        return Err(Refusal(
            StatusCode::MISDIRECTED_REQUEST,
            "moss refuses this Host: it answers only at its own origins.",
        ));
    };
    if headers.get(http::header::ORIGIN).is_some_and(|v| v.to_str().ok() != Some(origin.as_str())) {
        return Err(Refusal(StatusCode::FORBIDDEN, "moss refuses this cross-origin request."));
    }
    Ok(origin)
}

/// Central middleware: reject a request whose `Host` is not loopback (`421`) or
/// whose `Origin` is present-and-foreign (`403`), before it reaches any route.
///
/// Added as the outermost layer so it runs first — ahead of routing, `ServeDir`
/// and bridge injection — and so it covers **every** route, including
/// `/__moss_health/` (whose `127.0.0.1` Host and absent Origin both pass, which
/// is why startup readiness is not special-cased here).
pub async fn validate_host_origin(
    origins: std::sync::Arc<Vec<(String, String)>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Err(refusal) = admit_origin(request.headers(), &origins) {
        return refusal.response(request).await;
    }
    next.run(request).await
}

/// Drain a request body a refusal is about to drop instead of route.
///
/// Dropping an unread `Body` forces hyper to close the connection rather than
/// risk reusing it — the unread bytes would corrupt the next request's parse.
/// On Windows, a close with bytes still sitting in the socket's receive
/// buffer surfaces to the client as `WSAECONNABORTED` mid-status-line, not
/// the graceful close the same sequence gets on Unix loopback. Every
/// early refusal ahead of a carrier drains through this.
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

    fn admits(host: &str, origin: Option<&str>, origins: &[(String, String)]) -> Result<String, u16> {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::HOST, host.parse().unwrap());
        if let Some(origin) = origin {
            headers.insert(http::header::ORIGIN, origin.parse().unwrap());
        }
        admit_origin(&headers, origins).map(str::to_owned).map_err(|r| r.0.as_u16())
    }

    #[test]
    fn derived_origins_match_scheme_host_and_port() {
        let public = PublicOrigin::parse("https://preview.example.test:9443").unwrap();
        let origins = origins(8080, Some(&public));
        for (host, origin) in [
            ("localhost:8080", "http://localhost:8080"),
            ("127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("[::1]:8080", "http://[::1]:8080"),
            ("preview.example.test:9443", "https://preview.example.test:9443"),
        ] {
            assert_eq!(admits(host, None, &origins), Ok(origin.to_owned()), "{host}");
            assert_eq!(admits(host, Some(origin), &origins), Ok(origin.to_owned()), "{host}");
        }
        for origin in ["null", "http://127.0.0.1:8080", "http://localhost:8081", "https://localhost:8080", "https://evil.test"] {
            assert_eq!(admits("localhost:8080", Some(origin), &origins), Err(403), "{origin}");
        }
        for host in ["", "localhost", "localhost:8081", "[::1]", "::1:8080", "preview.example.test", "evil.test:8080", "localhost.evil.test:8080", "127.0.0.2:8080"] {
            assert_eq!(admits(host, None, &origins), Err(421), "{host}");
        }
    }

    #[test]
    fn public_url_requires_https_origin() {
        for origin in ["http://example.test", "https://example.test/", "https://example.test/path", "https://example.test?x", "https://example.test#x", "https://user@example.test", "https://example.test:bad", "https://example.test:65536"] {
            assert!(PublicOrigin::parse(origin).is_err(), "{origin}");
        }
        assert!(PublicOrigin::parse("https://example.test:9443").is_ok());
    }
}
