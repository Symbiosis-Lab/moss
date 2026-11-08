use super::{
    super::invoke::{self, InvokeCtx, Session},
    auth::{self, UiAuth},
};
use axum::{
    body::Body,
    extract::{Extension, Path},
    http::{Request, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

pub(super) fn router(
    auth: Arc<dyn UiAuth>,
    origin: String,
    ctx: Option<InvokeCtx>,
    site_dir: Arc<RwLock<PathBuf>>,
    kind: super::super::ownership::HostKind,
    notify: Arc<tokio::sync::Notify>,
) -> Router {
    let invoke_ctx = ctx.clone();
    let invoke_dir = site_dir.clone();
    let source_dir = site_dir.clone();
    super::routes::mint_router(auth.clone(), origin)
        .route(
            "/__moss/invoke/*cmd",
            post(move |Path(cmd): Path<String>, Json(args): Json<Value>| {
                let ctx = invoke_ctx.clone().expect("admission requires a carrier");
                let dir = invoke_dir.clone();
                async move { invoke::handle_invoke(ctx, dir, cmd, args).await }
            }),
        )
        .route(
            "/__moss/read/*cmd",
            post(
                |session: Extension<Arc<Session>>,
                 Path(cmd): Path<String>,
                 Json(args): Json<Value>| async move {
                    invoke::handle_authed_read(session, cmd, args).await
                },
            ),
        )
        .route(
            "/__moss/mutate/*cmd",
            post(
                |session: Extension<Arc<Session>>,
                 Path(cmd): Path<String>,
                 Json(args): Json<Value>| async move {
                    invoke::handle_mutate(session, cmd, args).await
                },
            ),
        )
        .route("/__moss/events", get(super::super::events::handle_events))
        // The editor shows a page's images from its source, behind the session.
        .route("/__moss/source/*path", get({
            let dir = source_dir;
            move |Path(path): Path<String>, axum::extract::RawQuery(query): axum::extract::RawQuery| {
                let dir = dir.clone();
                async move { super::super::router::source_asset_route(dir, path, query).await }
            }
        }))
        .route(
            "/__moss/upload",
            post(
                |Extension(session): Extension<Arc<Session>>, request: Request<Body>| async move {
                    super::super::upload_route::handle_admitted_upload(session, request).await
                },
            )
            .layer(axum::extract::DefaultBodyLimit::max(
                super::super::upload_route::TOTAL_MAX_UPLOAD_BYTES,
            )),
        )
        .route(
            "/__moss/yield",
            post(move || {
                let notify = notify.clone();
                async move { super::super::yield_route::handle_admitted_yield(kind, notify) }
            }),
        )
        .route_layer(middleware::from_fn(
            move |mut request: Request<Body>, next: middleware::Next| {
                let (ctx, dir) = (ctx.clone(), site_dir.clone());
                async move {
                    let session = request
                        .extensions()
                        .get::<auth::BrowserSession>()
                        .and(ctx.and_then(|ctx| ctx.bind(&dir)));
                    let Some(session) = session else {
                        super::super::trust_boundary::drain_body(request.into_body()).await;
                        return StatusCode::UNAUTHORIZED.into_response();
                    };
                    request.extensions_mut().insert(session);
                    next.run(request).await
                }
            },
        ))
}
