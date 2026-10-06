//! `POST /__moss/yield` — ask this server to give up ownership of the folder
//! it serves, so a fuller engine (the desktop app) can take over.
//!
//! An infrastructure route, registered unconditionally beside health, source
//! and comments (`router.rs`'s ordering rule), never a carrier command: it
//! changes who serves the folder, not the site's content or state, and has
//! no frontend binding.
//!
//! ## Contract
//!
//! * **Auth** — the same per-vault bearer token the mutation/read carriers
//!   check ([`super::carrier_token::admit`]). The caller reads it from
//!   `<vault>/.moss/build.nosync/http-token`, which only the same user can
//!   read, and sends it as `X-Moss-Token`.
//! * No token, a wrong token, or no carrier bound at all (this server was
//!   started with no [`super::invoke::InvokeCtx`], so there is no session to
//!   authenticate against) → **401**, the exact body
//!   [`super::carrier_token::unauthorized`] already produces for the other
//!   token-gated routes.
//! * The owner is a [`super::ownership::HostKind::Desktop`] host — the
//!   desktop app already IS the fuller engine a yield exists to hand the
//!   folder to, so it is never itself yielded → **409**
//!   `{"yielding":false,"reason":"desktop"}`.
//! * Otherwise → **202** `{"yielding":true}`, and this handler fires the
//!   [`tokio::sync::Notify`] that `ops::run_headless_build`'s own ctrl-c
//!   `select!` also waits on. The actual effect — stop the watcher, shut the
//!   server down the same way ctrl-c already does, release ownership, and
//!   (for a `--watch` owner) stand by for whoever claims the folder next —
//!   happens there, not in this handler: the route's job ends at admitting
//!   the request and raising the signal.
//!
//! Never GET: a yield changes state, so like the other command routes it is
//! POST-only, enforced structurally by `axum::routing::post` at the router.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use axum::{
    body::Body,
    http::{self, Request},
    response::Response,
};

/// Handler for `POST /__moss/yield`. See the module doc for the contract.
pub(crate) async fn handle_yield(
    ctx: Option<super::invoke::InvokeCtx>,
    site_dir: Arc<RwLock<PathBuf>>,
    kind: super::ownership::HostKind,
    yield_notify: Arc<tokio::sync::Notify>,
    request: Request<Body>,
) -> Response {
    let Some(ctx) = ctx else {
        super::trust_boundary::drain_body(request.into_body()).await;
        return super::carrier_token::unauthorized();
    };
    if let Err(refusal) = super::carrier_token::admit(&ctx, &site_dir, &request) {
        super::trust_boundary::drain_body(request.into_body()).await;
        return refusal;
    }
    super::trust_boundary::drain_body(request.into_body()).await;

    if kind == super::ownership::HostKind::Desktop {
        return Response::builder()
            .status(http::StatusCode::CONFLICT)
            .header("content-type", "application/json; charset=utf-8")
            .header("cache-control", "no-store")
            .body(Body::from("{\"yielding\":false,\"reason\":\"desktop\"}"))
            .expect("static 409 body is always valid");
    }

    yield_notify.notify_one();
    Response::builder()
        .status(http::StatusCode::ACCEPTED)
        .header("content-type", "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from("{\"yielding\":true}"))
        .expect("static 202 body is always valid")
}

#[cfg(test)]
mod tests {
    use super::super::carrier_token::TOKEN_HEADER;
    use super::super::invoke::InvokeCtx;
    use super::super::ownership::HostKind;
    use super::super::router::{start_server, ServeConfig};
    use std::sync::{Arc, RwLock};

    fn served_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let vault = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let site_dir = vault.path().join(".moss/build.nosync/current");
        std::fs::create_dir_all(&site_dir).unwrap();
        (vault, site_dir)
    }

    /// No token at all → 401, same as the mutate/read carriers.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn yield_without_a_token_is_401() {
        let (_vault, site_dir) = served_vault();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(InvokeCtx::standalone()),
            kind: HostKind::Cli,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64100)
        })
        .await
        .expect("server should start");

        let url = format!("http://localhost:{port}/__moss/yield");
        match ureq::post(&url).timeout(std::time::Duration::from_secs(5)).send_string("") {
            Err(ureq::Error::Status(401, _)) => {}
            Ok(resp) => panic!("a yield request with no token must 401; got {}", resp.status()),
            Err(e) => panic!("expected a 401, got transport error: {e}"),
        }
        let _ = shutdown_tx.send(());
    }

    /// A `Desktop` owner refuses the yield outright — it is the fuller engine
    /// this route exists to hand folders TO, so it is never the one yielded.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn yield_against_a_desktop_owner_is_409_and_names_the_reason() {
        let (_vault, site_dir) = served_vault();
        let ctx = InvokeCtx::standalone();
        let (port, shutdown_tx) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            kind: HostKind::Desktop,
            ..ServeConfig::new(Arc::new(RwLock::new(site_dir)), 64110)
        })
        .await
        .expect("server should start");
        let token = ctx.token().expect("start_server binds the served vault").to_string();

        let url = format!("http://localhost:{port}/__moss/yield");
        match ureq::post(&url)
            .set(TOKEN_HEADER, &token)
            .timeout(std::time::Duration::from_secs(5))
            .send_string("")
        {
            Err(ureq::Error::Status(409, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                assert!(
                    body.contains("\"yielding\":false") && body.contains("\"reason\":\"desktop\""),
                    "a Desktop owner's refusal must name the reason; got: {body}"
                );
            }
            Ok(resp) => panic!("a Desktop owner must refuse a yield with 409; got {}", resp.status()),
            Err(e) => panic!("expected a 409, got transport error: {e}"),
        }
        let _ = shutdown_tx.send(());
    }
}
