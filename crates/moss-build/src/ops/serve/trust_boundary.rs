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
//! Allowed origins derive from the bound port and, behind a public URL, one
//! HTTPS origin per listener: the UI's, and the preview's at the next port. A
//! request must address one of them by `Host`, and an `Origin` it carries must
//! be that same origin.

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

/// Where a proxy serves the UI and the preview: the same host, two ports.
/// The preview needs an origin of its own, since it runs the site's scripts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicUrl {
    pub ui: PublicOrigin,
    pub preview: PublicOrigin,
}

impl PublicUrl {
    /// The preview at `preview_port`, or at the UI's next port: a proxy
    /// serving the UI at `https://host` then serves the preview at `https://host:444`.
    pub fn new(ui: PublicOrigin, preview_port: Option<u16>) -> Result<Self, String> {
        let mut url = url::Url::parse(ui.as_str()).expect("checked in parse");
        let ui_port = url.port_or_known_default().expect("https has a default port");
        let port = match preview_port {
            Some(port) => port,
            None => ui_port.checked_add(1).ok_or("the public URL's port leaves no next port for the preview")?,
        };
        if port == 0 || port == ui_port {
            return Err("the public preview port must differ from the public URL's".into());
        }
        url.set_port(Some(port)).expect("an https URL takes a port");
        let preview = PublicOrigin(url.origin().ascii_serialization());
        Ok(Self { ui, preview })
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

/// Who may address the preview: its loopback origins and, behind a public
/// URL, its public origin too, where every request but the readiness check
/// needs credentials.
pub(crate) struct Admission {
    origins: Vec<(String, String)>,
    credentials: Option<Credentials>,
}

/// The carrier a local moss process reaches, and the folder whose token it holds.
pub(crate) type Carrier = (super::invoke::InvokeCtx, std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>);

/// What admits a request to a preview that has a public URL: a signed-in
/// browser's session, or the carrier token a local moss process reads from
/// the folder. A proxy forwards with whatever Host it likes from a loopback
/// peer, so neither the Host nor the peer tells a remote reader from a local one.
struct Credentials {
    auth: std::sync::Arc<dyn super::ui::auth::UiAuth>,
    carrier: Option<Carrier>,
}

/// What a request's credentials come to. A token that is presented and
/// wrong is refused, never treated as no credential at all.
enum Verdict {
    Admitted,
    Anonymous,
    Refused,
}

impl Credentials {
    fn judge(&self, request: &Request<Body>) -> Verdict {
        if super::ui::sessions::cookie(request.headers()).is_some_and(|s| self.auth.admits(s)) {
            return Verdict::Admitted;
        }
        if !request.headers().contains_key(super::carrier_token::TOKEN_HEADER) {
            return Verdict::Anonymous;
        }
        match &self.carrier {
            Some((ctx, dir)) if super::carrier_token::admit(ctx, dir, request).is_ok() => Verdict::Admitted,
            _ => Verdict::Refused,
        }
    }
}

impl Admission {
    pub(crate) fn new(port: u16, ui: Option<&super::ui::UiOriginConfig>, carrier: Option<Carrier>) -> Self {
        let public = ui.and_then(|ui| Some((&ui.public_url.as_ref()?.preview, ui.auth.clone())));
        Self {
            origins: origins(port, public.as_ref().map(|(preview, _)| *preview)),
            credentials: public.map(|(_, auth)| Credentials { auth, carrier }),
        }
    }
}

/// Central middleware: reject a request whose `Host` is not one of the
/// preview's origins (`421`) or whose `Origin` is present-and-foreign
/// (`403`), and, behind a public URL, one without credentials (`401`),
/// before it reaches any route.
///
/// Added as the outermost layer so it runs first — ahead of routing,
/// `ServeDir` and bridge injection — and covers **every** route. Without
/// credentials, `/__moss_health/` answers only that moss is here, and
/// nothing of the folder or the process.
pub async fn validate_host_origin(
    admission: std::sync::Arc<Admission>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Err(refusal) = admit_origin(request.headers(), &admission.origins) {
        return refusal.response(request).await;
    }
    match admission.credentials.as_ref().map(|c| c.judge(&request)) {
        None | Some(Verdict::Admitted) => next.run(request).await,
        Some(Verdict::Anonymous) if request.uri().path() == super::port::MOSS_HEALTH_PATH => {
            drain_body(request.into_body()).await;
            super::port::readiness_only()
        }
        Some(Verdict::Anonymous | Verdict::Refused) => Refusal(StatusCode::UNAUTHORIZED, "Sign in to see this preview.").response(request).await,
    }
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

    #[test]
    fn the_preview_answers_at_the_next_port_or_the_one_named() {
        let preview = |origin: &str, port| {
            PublicUrl::new(PublicOrigin::parse(origin).unwrap(), port).map(|p| p.preview.as_str().to_owned())
        };
        assert_eq!(preview("https://edit.example.test", None).unwrap(), "https://edit.example.test:444");
        assert_eq!(preview("https://edit.example.test:9443", None).unwrap(), "https://edit.example.test:9444");
        assert_eq!(preview("https://edit.example.test", Some(8443)).unwrap(), "https://edit.example.test:8443");
        assert_eq!(preview("https://edit.example.test:8443", Some(443)).unwrap(), "https://edit.example.test");
        for (origin, port) in [("https://edit.example.test", Some(443)), ("https://edit.example.test:65535", None), ("https://edit.example.test", Some(0))] {
            assert!(preview(origin, port).is_err(), "{origin} {port:?}");
        }
    }
}
