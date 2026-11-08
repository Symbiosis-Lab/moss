//! The UI listener has its own origin and accepts only browser sessions.
use super::{
    super::invoke::InvokeCtx,
    auth::{self, UiAuth},
};
use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

#[derive(Clone, Copy)]
pub enum UiPort {
    Fixed(u16),
    ScanFrom(u16),
}

#[derive(Clone)]
pub struct UiOriginConfig {
    pub port: UiPort,
    pub assets: &'static [super::assets::UiAsset],
    pub auth: Arc<dyn UiAuth>,
    /// Where a proxy serves the UI and the preview, when one does.
    pub public_url: Option<super::PublicUrl>,
    pub site_root: PathBuf,
}

impl UiOriginConfig {
    /// The origin sign-in links name: the public one when set.
    pub(crate) fn origin(&self, port: u16) -> String {
        self.public_url.as_ref().map_or_else(|| format!("http://localhost:{port}"), |p| p.ui.as_str().to_owned())
    }

    pub(crate) fn validate(
        &self,
        served: &std::path::Path,
    ) -> Result<(), String> {
        if !self.site_root.is_absolute() || !self.site_root.is_dir() {
            return Err("UI site root must be an explicit absolute directory".into());
        }
        self.auth.validate_location(&self.site_root)?;
        self.auth.validate_location(served)?;
        Ok(())
    }
}

#[derive(Clone)]
struct Boundary {
    auth: Arc<dyn UiAuth>,
    origins: Vec<(String, String)>,
}

impl Boundary {
    async fn check(self, mut request: Request<Body>, next: Next) -> Response {
        let origin = match super::super::trust_boundary::admit_origin(request.headers(), &self.origins) {
            Ok(origin) => origin.to_owned(),
            Err(refusal) => return refusal.response(request).await,
        };
        let headers = request.headers();
        let sign_in_page = request.method() == axum::http::Method::GET
            && request.uri().path() == "/__moss/sign-in";
        let navigation = request.method() == axum::http::Method::GET
            && request.uri().path() == "/"
            && headers
                .get("sec-fetch-mode")
                .is_some_and(|v| v == "navigate")
            && headers
                .get("sec-fetch-dest")
                .is_some_and(|v| v == "document");
        // A browser's PDF plugin fetches a source with no fetch metadata. The
        // strict session cookie never rides on a cross-site request, so a
        // source read without metadata is admitted here and nowhere else.
        let source_read = matches!(*request.method(), axum::http::Method::GET | axum::http::Method::HEAD)
            && request.uri().path().starts_with("/__moss/source/");
        let fetch_ok = sign_in_page
            || match headers.get("sec-fetch-site").map(|v| v.as_bytes()) {
                Some(b"same-origin") => true,
                Some(b"none") => navigation,
                None => source_read,
                Some(_) => false,
            };
        if !fetch_ok {
            super::super::trust_boundary::drain_body(request.into_body()).await;
            return StatusCode::FORBIDDEN.into_response();
        }
        request
            .extensions_mut()
            .insert(super::routes::RequestOrigin(origin.clone()));
        // The one place a cookie is admitted; routes read the session from here.
        let session = super::sessions::cookie(request.headers())
            .filter(|s| self.auth.admits(s))
            .map(|s| auth::BrowserSession { auth: self.auth.clone(), secret: s.to_owned() });
        let cookie = session.as_ref().map(|s| super::sessions::set_cookie(&s.secret, &origin, super::sessions::SESSION_SECONDS));
        if let Some(session) = session {
            request.extensions_mut().insert(session);
        }
        let mut response = next.run(request).await;
        if !response.headers().contains_key("set-cookie") {
            if let Some(cookie) = cookie {
                response.headers_mut().insert("set-cookie", cookie.parse().unwrap());
            }
        }
        if !response.headers().contains_key("cache-control") {
            response.headers_mut().insert("cache-control", "no-store".parse().unwrap());
        }
        response
            .headers_mut()
            .insert("referrer-policy", "no-referrer".parse().unwrap());
        response
    }
}

pub(crate) fn router(
    config: UiOriginConfig,
    port: u16,
    preview_port: u16,
    ctx: Option<InvokeCtx>,
    site_dir: Arc<RwLock<PathBuf>>,
    kind: super::super::ownership::HostKind,
    notify: Arc<tokio::sync::Notify>,
) -> Result<Router, String> {
    if port == preview_port {
        return Err("UI and preview must have separate origins".into());
    }
    let origins = super::super::trust_boundary::origins(port, config.public_url.as_ref().map(|p| &p.ui));
    let origin = config.origin(port);
    let public_url = config.public_url.as_ref().map(|p| p.ui.as_str().to_owned());
    // A browser reaches the preview at the host it reached the UI by: a
    // cookie is scoped to the host, not the port.
    let public_preview = config.public_url.as_ref().map(|p| p.preview.as_str().to_owned());
    let boundary = Boundary { auth: config.auth.clone(), origins };
    let carrier = super::carrier::router(config.auth.clone(), origin, ctx, site_dir, kind, notify);
    let host = super::assets::router(config.assets);
    Ok(host
        .route(
            "/__moss/config",
            axum::routing::get(move |Extension(origin): Extension<super::routes::RequestOrigin>| async move {
                let preview_url = match &public_preview {
                    Some(preview) if public_url.as_deref() == Some(origin.0.as_str()) => preview.clone(),
                    _ => {
                        let mut url = url::Url::parse(&origin.0).expect("an admitted origin parses");
                        url.set_port(Some(preview_port)).expect("an http origin takes a port");
                        url.origin().ascii_serialization()
                    }
                };
                axum::Json(serde_json::json!({"preview_url": preview_url, "public_url": public_url}))
            }),
        )
        .merge(super::routes::router(config.auth))
        .merge(carrier)
        .layer(middleware::from_fn(move |request, next| {
            boundary.clone().check(request, next)
        }))
        .layer(middleware::from_fn(|request, next: Next| async move {
            let mut response = next.run(request).await;
            // The entry page frames the shell and the editor; nothing else may frame either.
            response.headers_mut().append("content-security-policy", "frame-ancestors 'self'".parse().unwrap());
            response
        })))
}

pub(crate) struct Listener {
    pub port: u16,
    v4: tokio::net::TcpListener,
    v6: tokio::net::TcpListener,
    app: Router,
}

pub(crate) async fn bind(server: &super::super::ServeConfig, preview_port: u16) -> Result<Option<Listener>, String> {
    let Some(config) = &server.ui else {
        return Ok(None);
    };
    let (port, v4, v6) = match config.port {
        UiPort::ScanFrom(base) => super::super::port::bind_with_scan(base).await?,
        UiPort::Fixed(port) => super::super::port::bind_fixed(port).await?,
    };
    let app = router(
        config.clone(),
        port,
        preview_port,
        server.invoke.clone(),
        server.site_dir_cell.clone(),
        server.kind,
        server.yield_notify.clone(),
    )?;
    Ok(Some(Listener {
        port,
        v4,
        v6,
        app,
    }))
}

pub(crate) async fn drive(
    listener: Option<Listener>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    if let Some(listener) = listener {
        super::super::port::drive(listener.v4, listener.v6, listener.app, shutdown)
            .await;
    }
}
