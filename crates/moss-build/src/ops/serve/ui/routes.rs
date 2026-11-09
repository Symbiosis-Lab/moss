use super::auth::{SignInError, UiAuth};
use axum::{
    extract::Extension,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct RequestOrigin(pub String);

// Fragments never reach the server or a reverse proxy access log.
const SCRIPT: &str = r#"
const code = new URLSearchParams(location.hash.slice(1)).get('code');
history.replaceState(null, '', '/__moss/sign-in');
fetch('/__moss/sign-in', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({code})
}).then(response => {
    if (response.ok) location.replace('/');
    else document.body.textContent = 'Sign-in failed. Request a new link.';
}).catch(() => {
    document.body.textContent = 'Unable to sign in. Please try again.';
});
"#;

async fn sign_in_page() -> Response {
    let hash = STANDARD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        SCRIPT.as_bytes(),
    ));
    let csp = format!("default-src 'none'; script-src 'sha256-{hash}'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'");
    ([("content-security-policy", csp), ("cache-control", "no-store".into()), ("referrer-policy", "no-referrer".into())],
        Html(format!("<!doctype html><meta charset=\"utf-8\"><title>Sign in</title><body>Signing in…<script>{SCRIPT}</script>"))).into_response()
}

#[derive(Deserialize)]
struct Credentials {
    code: String,
}

pub(crate) fn router(auth: Arc<dyn UiAuth>) -> Router {
    let sign_in = auth.clone();
    Router::new()
        .route(
            "/__moss/sign-in",
            get(sign_in_page).post(
                move |Extension(origin): Extension<RequestOrigin>,
                      headers: HeaderMap,
                      Json(credentials): Json<Credentials>| {
                    let auth = sign_in.clone();
                    async move {
                        let agent = headers
                            .get("user-agent")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");
                        match auth.sign_in(&credentials.code, agent) {
                            Ok(secret) => (
                                StatusCode::NO_CONTENT,
                                [(
                                    "set-cookie",
                                    super::sessions::set_cookie(&secret, &origin.0, super::sessions::SESSION_SECONDS),
                                )],
                            )
                                .into_response(),
                            Err(SignInError::Unauthorized) => {
                                StatusCode::UNAUTHORIZED.into_response()
                            }
                            Err(SignInError::Unavailable) => {
                                StatusCode::SERVICE_UNAVAILABLE.into_response()
                            }
                        }
                    }
                },
            ),
        )
        .route(
            "/__moss/sign-out",
            post(
                move |Extension(origin): Extension<RequestOrigin>,
                      session: Option<Extension<super::auth::BrowserSession>>| {
                    let auth = auth.clone();
                    async move {
                        let Some(Extension(session)) = session else {
                            return StatusCode::UNAUTHORIZED.into_response();
                        };
                        if auth.sign_out(&session.secret).is_err() {
                            return StatusCode::SERVICE_UNAVAILABLE.into_response();
                        }
                        let cookie = super::sessions::set_cookie("", &origin.0, 0);
                        (StatusCode::NO_CONTENT, [("set-cookie", cookie)]).into_response()
                    }
                },
            ),
        )
}

/// `POST /__moss/sign-in/new`: a fresh link at the UI's canonical origin. The
/// UI listener mounts it behind the browser session; the preview listener
/// mounts it behind the token file, for a second local `moss edit`.
pub(crate) fn mint_router(auth: Arc<dyn UiAuth>, origin: String) -> Router {
    Router::new().route("/__moss/sign-in/new", post(move || {
        let (auth, origin) = (auth.clone(), origin.clone());
        async move {
            match auth.mint_sign_in() {
                Ok(link) => {
                    let url = format!("{origin}{}", link.url_path());
                    let qr = crate::build::media::qr::generate_qr_svg(&url).ok();
                    Json(serde_json::json!({"url": url, "qr_svg": qr})).into_response()
                }
                Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            }
        }
    }))
}

pub(crate) fn preview_mint_router(server: &super::super::ServeConfig, ui_port: Option<u16>) -> Option<Router> {
    let ui = server.ui.as_ref()?;
    let (ctx, dir) = (server.invoke.clone()?, server.site_dir_cell.clone());
    Some(mint_router(ui.auth.clone(), ui.origin(ui_port?)).route_layer(axum::middleware::from_fn(
        move |request, next| {
            let (ctx, dir) = (ctx.clone(), dir.clone());
            async move { super::super::carrier_token::require_carrier_token_only(ctx, dir, request, next).await }
        },
    )))
}
