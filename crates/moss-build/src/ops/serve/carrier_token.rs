//! Per-session token that gates the HTTP **mutation** carrier
//! (`POST /__moss/mutate/<cmd>`). The read-only carrier
//! (`POST /__moss/invoke/<cmd>`) stays token-free — public-read, exactly like
//! `/__moss/source/*`.
//!
//! ## Why a token at all — what the loopback floor cannot stop
//!
//! `trust_boundary::validate_host_origin` already refuses a rebound `Host`
//! (421) and a foreign `Origin` (403), and the socket is loopback-only. That is
//! the right floor for a READ surface. It is not enough for a WRITE surface: a
//! page the user opens in their own browser sends NO `Origin` on a top-level
//! navigation and can be coaxed into sending none on some requests, and the
//! whole point of DNS rebinding is to make the browser believe it IS loopback.
//! The floor keeps *other machines* and *ordinary cross-site* callers out; it
//! does not, by itself, keep a hostile *local browser page* from POSTing a
//! mutation to the port.
//!
//! ## The token, and why a local file delivers it
//!
//! moss mints one random token per vault the server binds and writes it to a
//! **owner-readable local file** in this computer's folder record, outside the vault
//! (see [`token_path`]). A client proves it is a genuine local
//! process — a coding agent, a Playwright driver, moss's own tooling — by
//! reading that file and echoing the token in a custom request header
//! ([`TOKEN_HEADER`], `X-Moss-Token`). Two properties make this defeat the
//! local-browser-page threat the floor leaves open:
//!
//!   * **A browser page cannot read a local file.** `fetch('file:///…')` is
//!     blocked; there is no API that hands a web page the bytes of
//!     the folder's `http-token` record on this computer. So a page cannot learn the token.
//!   * **A browser page cannot forge the custom header cross-origin without a
//!     CORS preflight** the server never answers. `X-Moss-Token` is not a
//!     CORS-safelisted header, so any cross-origin `fetch` that sets it triggers
//!     an `OPTIONS` preflight; moss emits no permissive `Access-Control-*`
//!     response, so the browser refuses to send the real request.
//!
//! This is the shape Jupyter uses (a token in a loopback-readable runtime file)
//! paired with the MCP Inspector remediation for CVE-2025-49596 (a session
//! token layered on top of Host/Origin checks).
//!
//! ## Token lifecycle
//!
//! 1. **Mint** — [`mint`] draws 256 bits of entropy (two `uuid` v4 values)
//!    when [`super::session::InvokeCtx::bind`] binds a vault the carrier was
//!    not already serving: at start-up, and again on every folder switch,
//!    which retires the previous vault's token. It never
//!    persists across restarts: a new server, a new token, and the old file
//!    is overwritten.
//! 2. **Publish** — [`publish`] writes the token to
//!    the folder's `http-token` record on this computer (see [`token_path`]) in the same bind.
//!    This local record stays outside synced and served output and survives cache
//!    purges; `build::io_utils::write_output` lands it via temp+rename.
//! 3. **Obtain (client)** — after successful publication, stderr emits exactly one
//!    `HTTP carrier token file: <JSON-encoded absolute path>` line, even in quiet
//!    mode. A local client JSON-decodes the path, reads and trims the file, and
//!    sends the value as `X-Moss-Token`.
//! 4. **Verify (server)** — [`constant_time_eq`] compares the header against the
//!    session token in the mutation middleware; a miss is `401`.
//!
//! ## Security hygiene
//!
//! The token is a secret. It is NEVER placed in a URL, a log line, or an error
//! body — the only sinks are the local file and the constant-time comparison.
//! [`build::io_utils::write_output`] lands umask-default permissions, so
//! [`publish`] tightens the file to **owner-only (0600) on unix** afterwards: on
//! a multi-user host the loopback floor trusts every local *process*, but the
//! token need only be readable by the *user* who runs moss. [`publish`]'s error
//! path names the *path*, never the bytes.
//!
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::moss_paths::MossPaths;

/// The custom request header a client echoes the session token in. Not a
/// CORS-safelisted name, so a cross-origin browser `fetch` that sets it is
/// forced through a preflight the server never approves.
pub const TOKEN_HEADER: &str = "x-moss-token";

/// The file the session token is published to: in this computer's record for
/// the folder, outside it, so the secret never reaches a second machine and no
/// cache purge removes it while the server runs.
pub fn token_path(vault_root: &Path) -> PathBuf {
    MossPaths::new(vault_root).http_token()
}

/// The token a local client reads from [`token_path`], or `None` while no
/// server has published one.
pub fn read(vault_root: &Path) -> Option<String> {
    std::fs::read_to_string(token_path(vault_root)).ok().map(|token| token.trim().to_owned())
}

/// The agent a token-carrying request goes out on. It follows no redirect,
/// which would carry the token to another listener.
pub(crate) fn agent(timeout: std::time::Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout(timeout).build()
}

/// Mint a fresh session token — 256 bits of entropy, hex, no separators. Two
/// `uuid` v4 values concatenated (128 + 128 bits); `uuid` is already a
/// dependency, so this adds no crate. The value is opaque to clients: they read
/// it and echo it, never parse it.
pub fn mint() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Publish `token` to [`token_path`] so a local client can read it, by temp
/// and rename.
///
/// The error string names the path only — never the token bytes.
pub fn publish(vault_root: &Path, token: &str) -> Result<(), String> {
    let path = token_path(vault_root);
    crate::build::io_utils::write_output(&path, token.as_bytes())
        .map_err(|e| format!("Failed to write HTTP carrier token at {}: {e}", path.display()))?;
    // write_output lands umask-default perms; restrict the secret to owner-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            format!(
                "Failed to restrict HTTP carrier token perms at {}: {e}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Constant-time equality for the token check, so a mismatch cannot be timed
/// byte-by-byte. Length is compared first (unavoidable, and a wrong length is
/// simply wrong); equal-length inputs are compared with no early return.
pub fn constant_time_eq(provided: &str, expected: &Arc<String>) -> bool {
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_agent_follows_no_redirect() {
        use std::io::{Read, Write};
        let elsewhere = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let target = elsewhere.local_addr().unwrap();
        let first = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!("http://{}/", first.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = first.accept().unwrap();
            let _ = stream.read(&mut [0; 4096]);
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://{target}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let status = match agent(std::time::Duration::from_secs(2)).get(&url).set(TOKEN_HEADER, "secret").call() {
            Ok(response) => response.status(),
            Err(ureq::Error::Status(status, _)) => status,
            Err(e) => panic!("{e}"),
        };
        assert_eq!(status, 302);
        elsewhere.set_nonblocking(true).unwrap();
        assert!(elsewhere.accept().is_err(), "the token must not follow the redirect");
    }

    #[test]
    fn mint_is_random_and_wide() {
        let a = mint();
        let b = mint();
        assert_ne!(a, b, "two mints must differ");
        assert_eq!(a.len(), 64, "two v4 uuids = 64 hex chars = 256 bits");
        assert!(
            a.chars().all(|c| c.is_ascii_hexdigit()),
            "token must be hex only, got: {a}"
        );
    }

    #[test]
    fn constant_time_eq_matches_only_the_exact_token() {
        let expected = Arc::new("abc123".to_string());
        assert!(constant_time_eq("abc123", &expected));
        assert!(!constant_time_eq("abc124", &expected), "one byte off must fail");
        assert!(!constant_time_eq("abc12", &expected), "short must fail");
        assert!(!constant_time_eq("abc1234", &expected), "long must fail");
        assert!(!constant_time_eq("", &expected), "empty must fail");
    }

    #[test]
    fn the_token_lives_outside_the_folder_in_its_record() {
        let vault = tempfile::tempdir().unwrap();
        let p = token_path(vault.path());
        assert_eq!(p, MossPaths::new(vault.path()).record_dir().join("http-token"));
        assert!(!p.starts_with(std::fs::canonicalize(vault.path()).unwrap()), "token must never sit in the synced folder; got {}", p.display());
    }

    #[test]
    fn publish_then_read_round_trips_the_token() {
        // Repo-local temp per project rule (never /tmp for real writes).
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let vault = tempfile::TempDir::new_in(&base).unwrap();
        let token = mint();
        publish(vault.path(), &token).expect("publish must succeed");
        let read = std::fs::read_to_string(token_path(vault.path())).expect("token file must read");
        assert_eq!(read, token, "the published bytes must be exactly the token");
    }

    #[cfg(unix)]
    #[test]
    fn published_token_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let vault = tempfile::TempDir::new_in(&base).unwrap();
        publish(vault.path(), &mint()).expect("publish must succeed");
        let mode = std::fs::metadata(token_path(vault.path()))
            .expect("token file must exist")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the token is a secret — it must be readable only by its owner, got {:o}",
            mode & 0o777
        );
    }
}

// ── Route gates ────────────────────────────────────────────────
//
// The middleware that enforces the token lives here rather than in `router.rs`:
// it is token policy, not routing, and the router only needs to know which gate
// each route wears.

use axum::{
    body::Body,
    http::{self, Request},
    response::Response,
};

/// Token + content-type gate for the token-gated carriers (`POST
/// /__moss/mutate/*cmd` and `POST /__moss/read/*cmd`), applied as a `route_layer`
/// on those routes ONLY. It runs AFTER the outer `validate_host_origin` trust
/// boundary (so a foreign Origin is already refused with 403 before this sees the
/// request) and AFTER method routing (so a GET is already 405). Method (POST) is
/// therefore enforced structurally by `axum::routing::post`; this adds the two
/// inner gates:
///
///   * **Token** — the `X-Moss-Token` header must equal the per-session token,
///     compared in constant time. Missing / empty / wrong → **401**. This is the
///     defense against a local browser page that can reach the loopback port but
///     cannot read the token file (see this module's header). Checked FIRST so an
///     unauthenticated caller learns nothing about the rest of the request.
///   * **Content-Type** — must be `application/json`. Otherwise → **415**. (A
///     valid JSON body is still required by the handler's `Json` extractor; this
///     makes the media-type contract explicit at the gate.)
///
/// The token never appears in any response this produces — refusals carry a
/// static reason string only.
///
/// The token half is [`admit`], shared with the event stream
/// (`GET /__moss/events`) and the yield route (`POST /__moss/yield`, see
/// `super::yield_route`), which need the same 401 and no body at all.
pub(crate) async fn require_carrier_token(
    ctx: super::invoke::InvokeCtx,
    site_dir: crate::moss_paths::ServedCell,
    mut request: Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let session = match admit(&ctx, &site_dir, &request) {
        Ok(session) => session,
        Err(refusal) => {
            // admit() only borrowed `request`; drain it here, on the owned
            // value, before dropping it with the refusal (see trust_boundary
            // module docs on why an unread body aborts the connection).
            super::trust_boundary::drain_body(request.into_body()).await;
            return refusal;
        }
    };

    let content_type_ok = request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.trim_start()
                .to_ascii_lowercase()
                .starts_with("application/json")
        })
        .unwrap_or(false);
    if !content_type_ok {
        super::trust_boundary::drain_body(request.into_body()).await;
        return Response::builder()
            .status(http::StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .header("content-type", "text/plain; charset=utf-8")
            .header("cache-control", "no-store")
            .body(Body::from(
                "moss carrier requires a JSON body (Content-Type: application/json).",
            ))
            .expect("static 415 response is always valid");
    }

    request.extensions_mut().insert(session);
    next.run(request).await
}

/// The `X-Moss-Token` check alone: the admitted session when it passes, the
/// 401 when it does not. Constant-time compare; the token never reaches the response.
///
/// Separate from [`require_carrier_token`] because the event stream is a `GET`
/// with no body, so the media-type gate that belongs on the two command routes
/// would refuse every legitimate subscriber with a 415.
///
/// `pub(crate)`, not `fn`, so `super::yield_route::handle_yield` can run the
/// same check directly rather than growing a second token comparison.
pub(crate) fn admit(
    ctx: &super::invoke::InvokeCtx,
    site_dir: &crate::moss_paths::ServedCell,
    request: &Request<Body>,
) -> Result<Arc<super::invoke::Session>, Response> {
    // Bind BEFORE comparing: across a folder switch the presented token must
    // be judged against the vault the server now serves, whose token is fresh,
    // so a tab holding the previous vault's token is 401 here rather than
    // admitted and then run against the new vault. The
    // session admitted here is the one the handler runs against.
    let Some(session) = ctx.bind(site_dir) else {
        return Err(unauthorized());
    };
    let provided = request
        .headers()
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty());
    if let Some(provided) = provided {
        if constant_time_eq(provided, session.token()) {
            return Ok(session);
        }
    }
    Err(unauthorized())
}

/// `pub(crate)` so the yield route's no-session-bound case (no carrier to
/// authenticate against at all) can hand back the exact same refusal rather
/// than a second, slightly different 401 body.
pub(crate) fn unauthorized() -> Response {
    Response::builder()
        .status(http::StatusCode::UNAUTHORIZED)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(
            "moss carrier requires a valid X-Moss-Token session token.",
        ))
        .expect("static 401 response is always valid")
}

/// Token gate for `GET /__moss/events` — [`admit`] and nothing else.
pub(crate) async fn require_carrier_token_only(
    ctx: super::invoke::InvokeCtx,
    site_dir: crate::moss_paths::ServedCell,
    mut request: Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    match admit(&ctx, &site_dir, &request) {
        Ok(session) => {
            request.extensions_mut().insert(session);
            next.run(request).await
        }
        Err(refusal) => refusal,
    }
}

