//! Axum router construction and server spawn for the preview server.
//!
//! Builds the single dynamic-directory Axum router used by both GUI mode
//! (zero-flicker rebuild) and CLI mode (static path). Handles asset placeholder
//! injection, conditional-request stripping, and the HTML 404 fallback that keeps
//! the iframe-bridge script alive on missing pages.

use super::placeholder::{handle_asset_request, transparent_stub_response};
use super::port::{verify_server_ready, MOSS_HEALTH_MARKER, MOSS_HEALTH_PATH};
use super::asset_rewriter;
use super::content_wrapper;
use super::iframe_bridge::inject_iframe_bridge;
use axum::{
    body::Body,
    http::{self, Request},
    middleware,
    response::Response,
    routing::get,
    Router,
};
use moss_core::media::html_escape;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

/// Attempt to bind TCP listeners for moss on both IPv4 (`127.0.0.1`) and
/// IPv6 (`[::1]`) loopback for exactly the given port.
///
/// Returns `Err` if either family is unbindable; the caller is expected to
/// move on to the next candidate port in that case.
///
/// ### Why two listeners instead of one wildcard bind
///
/// The obvious approach — binding the IPv6 wildcard `[::]:port` with
/// `IPV6_V6ONLY=0` — does NOT work as a collision detector on macOS / BSD.
/// macOS allows a later `127.0.0.1:port` bind to coexist with a prior
/// `[::]:port` bind, and the more-specific listener wins for incoming
/// localhost traffic. That is exactly the foreign-vs-moss collision pattern
/// this code is supposed to detect and refuse.
///
/// Binding both specific loopback addresses explicitly accomplishes two
/// things:
///
/// 1. **Detection.** If a foreign server holds `127.0.0.1:port` or `[::1]:port`
///    directly, our bind fails on that address and we fall through to the
///    next-port scan.
/// 2. **Correctness in the wildcard case.** Even if a foreign dev server
///    holds the IPv6 wildcard `[::]:port` (eleventy's default), the kernel
///    routes connections to whichever listener has the more specific
///    address. Our two specific-loopback listeners win, so the iframe
///    sees moss content regardless of whether `localhost` resolves to
///    `127.0.0.1` or `::1`.
///
/// See `docs/archive/2026-05-22-preview-port-dual-stack-collision.md`.
async fn try_bind_dual_stack(port: u16) -> Result<(TcpListener, TcpListener), String> {
    let v4_addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], port));
    let v6_addr: SocketAddr = SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port));

    let v4 = TcpListener::bind(&v4_addr)
        .await
        .map_err(|e| format!("bind 127.0.0.1:{}: {}", port, e))?;
    // If v4 succeeded but v6 fails, dropping `v4` here releases the IPv4
    // listener so we don't leak a half-bound port — the next loop iteration
    // (or a competing process) is free to grab the IPv4 side again.
    let v6 = TcpListener::bind(&v6_addr)
        .await
        .map_err(|e| format!("bind [::1]:{}: {}", port, e))?;
    Ok((v4, v6))
}

/// Scan upward from `start_port` until both IPv4 and IPv6 loopback can be
/// bound simultaneously. Returns the bound port plus both listeners.
///
/// This is the **authoritative collision check** for the server-start path:
/// the bind itself is the source of truth for "is this port free?", which
/// closes the TOCTOU window that a separate `is_port_available` →
/// `bind_dual_stack` sequence would leave open. Other callers that just
/// want a non-binding probe (e.g. lifecycle health checks) can still use
/// `port::is_port_available`.
///
/// Scans `MAX_PORT_SCAN` ports (currently 100) starting from `start_port`.
/// Returns the same shape `try_bind_dual_stack` does on success, or an
/// `Err` describing the last bind failure if no port in the range works.
async fn bind_dual_stack_with_scan(
    start_port: u16,
) -> Result<(u16, TcpListener, TcpListener), String> {
    const MAX_PORT_SCAN: u16 = 100;
    let mut last_err: Option<String> = None;
    for port in start_port..start_port.saturating_add(MAX_PORT_SCAN) {
        match try_bind_dual_stack(port).await {
            Ok((v4, v6)) => {
                log::info!(
                    target: "preview",
                    "Preview server bound to port {} (dual-stack: IPv4 + IPv6)",
                    port
                );
                return Ok((port, v4, v6));
            }
            Err(e) => {
                log::info!(
                    target: "preview",
                    "Port {} unavailable, trying next port (foreign server may be holding it): {}",
                    port, e
                );
                last_err = Some(e);
            }
        }
    }
    Err(format!(
        "No dual-stack-bindable port found in range {}..{} (last error: {})",
        start_port,
        start_port.saturating_add(MAX_PORT_SCAN),
        last_err.unwrap_or_else(|| "unknown".to_string())
    ))
}

/// Body served by the [`MOSS_HEALTH_PATH`] endpoint.
///
/// The marker substring is physically substituted from [`MOSS_HEALTH_MARKER`]
/// at format time, so the producer and the consumer
/// ([`super::port::verify_server_ready`]) cannot drift apart: a rename of the
/// marker constant updates both sides automatically. The sync test
/// `moss_health_body_contains_marker` below also locks the invariant.
///
/// `schema: 1` is a forward-compat field for future format evolution. Readers
/// (currently `verify_server_ready`) ignore it; if the body shape changes,
/// bump to `2` and gate the consumer on `schema >= 1`.
///
/// Note that `MOSS_HEALTH_MARKER` already includes its own surrounding double
/// quotes (it is itself a quoted JSON string token), so the format expression
/// drops it in pre-quoted — no extra `\"` needed around the `{}`.
fn moss_health_body() -> String {
    format!(
        "{{\"server\":{},\"version\":\"{}\",\"preview\":true,\"schema\":1}}",
        MOSS_HEALTH_MARKER,
        env!("CARGO_PKG_VERSION")
    )
}

/// `/__moss/source/*path` — serve a project-scoped SOURCE file over HTTP.
///
/// The vault root is derived from the served site directory rather than passed
/// in: the site dir is `<vault>/.moss/build.nosync/…`, and `VaultRoot::find_containing`
/// walks up to the nearest ancestor owning a `.moss/`. The `find_` variant is
/// deliberate — it returns `None` instead of falling back to the starting
/// directory, so a server pointed somewhere unexpected 404s rather than
/// silently exposing whatever folder it happened to start in.
///
/// `path` arrives percent-DECODED (axum's `Path` extractor decodes once), which
/// is what [`serve_source_asset`] expects and matches what the Tauri scheme
/// hands it. Do not decode again — `resolve_scoped` is the containment check,
/// and a second decode would hand it a different string than the one that was
/// checked.
async fn source_asset_route(
    site_dir_state: Arc<std::sync::RwLock<std::path::PathBuf>>,
    path: String,
) -> Response {
    use crate::editor::source_asset::serve_source_asset;

    let site_dir = match site_dir_state.read() {
        Ok(d) => d.clone(),
        Err(_) => return source_asset_404(),
    };
    let Some(vault) = crate::vault::paths::VaultRoot::find_containing(&site_dir) else {
        return source_asset_404();
    };

    let r = serve_source_asset(vault.path(), &path).await;
    Response::builder()
        .status(r.status)
        .header("Content-Type", r.content_type)
        // Source bytes change whenever the user saves; the editor busts with
        // `?_t=`, so never let an intermediary hold a stale copy.
        .header("Cache-Control", "no-cache")
        .body(Body::from(r.body))
        .unwrap_or_else(|_| source_asset_404())
}

fn source_asset_404() -> Response {
    Response::builder()
        .status(404)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Body::from("Not Found"))
        .expect("static 404 response is always valid")
}

/// Handler for [`MOSS_HEALTH_PATH`].
///
/// Returns a JSON body containing the moss-specific marker token used by
/// `verify_server_ready` to confirm that the responding server is moss
/// (rather than a foreign dev server like eleventy that happens to be
/// holding the same port).
async fn moss_health_handler() -> Response<Body> {
    Response::builder()
        .status(http::StatusCode::OK)
        .header("content-type", "application/json; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(moss_health_body()))
        .unwrap()
}

/// A page for a request `ServeDir` could not answer because the file's bytes
/// are still in the cloud. `None` for every other failure — the caller then
/// passes `ServeDir`'s own response through unchanged.
///
/// `.moss/build.nosync/` lives inside the user's vault, so the sync client is free to
/// evict moss's own output. When it does, `ServeDir`'s read fails with
/// `EDEADLK` and tower-http renders that as a bodyless 500: a white void, and
/// no `<html>` for `inject_iframe_bridge` to attach the navigation bridge to.
/// This says what actually happened, asks for the file back, and reloads on a
/// short timer so the page appears on its own when the download lands — no
/// shell wiring, and correct even if the user opened the preview in a browser.
fn cloud_offline_response(
    current_dir: &std::path::Path,
    path: &str,
    status: http::StatusCode,
) -> Option<Response<Body>> {
    if status != http::StatusCode::INTERNAL_SERVER_ERROR {
        return None;
    }
    let encoded = path.trim_start_matches('/');
    let relative = urlencoding::decode(encoded)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| encoded.to_string());
    let joined = current_dir.join(&relative);
    // Resolve the URL to the file ServeDir would actually have opened. `/` and
    // `/about/` name directories, and `is_still_in_the_cloud` is false for
    // anything that is not a regular file — so checking the joined path alone
    // answered "not evicted" for every page on the site, including the home
    // page, and passed the bodyless 500 straight through. That is the single
    // most likely way for a user to meet this function.
    let disk = if joined.is_dir() || relative.is_empty() || relative.ends_with('/') {
        joined.join("index.html")
    } else {
        joined
    };
    if !crate::build::icloud::is_still_in_the_cloud(&disk) {
        return None;
    }
    crate::build::cloud_readiness::request_download(&disk);
    Response::builder()
        // 503, not 500: the page is temporarily unavailable and will come
        // back, which is exactly what this status means.
        .status(http::StatusCode::SERVICE_UNAVAILABLE)
        .header("content-type", "text/html; charset=utf-8")
        .header("cache-control", "no-store")
        .header("retry-after", "2")
        .body(Body::from(format!(
            concat!(
                "<!DOCTYPE html>\n",
                "<html><head><meta charset=\"UTF-8\">",
                "<meta http-equiv=\"refresh\" content=\"2\">",
                "<title>Downloading…</title>\n",
                "<style>body{{font-family:system-ui,sans-serif;display:flex;",
                "align-items:center;justify-content:center;min-height:80vh;color:#888}}",
                ".msg{{text-align:center;max-width:32rem}}p{{margin:.5rem 0 0}}",
                "code{{color:#666}}</style>\n",
                "</head><body><div class=\"msg\"><p>Downloading this page from the cloud.</p>",
                "<p><code>{}</code></p></div></body></html>",
            ),
            html_escape(&relative)
        )))
        .ok()
}

/// "Is this source file cloud-evicted (data not present locally)?"
///
/// A function pointer rather than a direct call so the passthrough's
/// evicted branch is reachable from a test on a platform where no sync client
/// sets a stat-time bit. `icloud::is_evicted` is an `lstat` on macOS/Windows and
/// a hard `false` everywhere else, which would otherwise leave the branch — the
/// one that stops a truncated 200 reaching a `<picture>` — provable only by
/// reading it. Injected at construction, so there is no global to race.
pub type EvictedProbe = fn(&std::path::Path) -> bool;

/// Everything one preview-server start needs, as plain constructor values —
/// the single entry shape [`start_server`] takes. [`ServeConfig::new`] fills
/// the defaults every production caller wants (the real eviction probe, no
/// registry, no carrier); the port base is resolved by the caller — see
/// [`super::port::env_port_base`] — and callers override other fields with
/// struct-update syntax.
pub struct ServeConfig {
    /// Shared live directory pointer. The build atomically switches it during
    /// rebuilds (zero-flicker), and every request re-reads it.
    pub site_dir_cell: Arc<std::sync::RwLock<std::path::PathBuf>>,
    /// When present, the server serves SVG placeholders (and source
    /// passthrough) for assets still being processed.
    pub asset_registry: Option<Arc<crate::types::assets::AssetRegistry>>,
    /// When present, mounts the `/__moss/{invoke,read,mutate,events}` HTTP
    /// command carrier; absent ⇒ the routes do not exist (404, never a gate).
    pub invoke: Option<super::invoke::InvokeCtx>,
    /// First port of the upward scan. Written only by [`Self::new`].
    pub start_port: u16,
    /// Cloud-eviction probe. Only a test passes anything but the real one —
    /// see [`EvictedProbe`] for why the branch it guards cannot otherwise be
    /// reached on a Linux CI box.
    pub is_evicted: EvictedProbe,
}

impl ServeConfig {
    /// Defaults for everything but the two things every caller must decide:
    /// the site-dir cell and the first port of the scan. Callers resolve the
    /// port base *before* construction (tests pass a literal; the app passes
    /// [`super::port::env_port_base`]) so building a config never reads the
    /// environment.
    pub fn new(
        site_dir_cell: Arc<std::sync::RwLock<std::path::PathBuf>>,
        start_port: u16,
    ) -> Self {
        Self {
            site_dir_cell,
            asset_registry: None,
            invoke: None,
            start_port,
            is_evicted: crate::build::icloud::is_evicted,
        }
    }
}

/// Build the router (optionally with the HTTP command carrier) and spawn the
/// dual-stack serve loop — the ONE server entry, for GUI and CLI alike.
///
/// The server can start even for empty folders (no index.html): `ServeDir`
/// returns 404 for missing files, which lets plugins with `before_build` hooks
/// download content that a subsequent rebuild turns into pages.
pub async fn start_server(
    config: ServeConfig,
) -> Result<(u16, tokio::sync::oneshot::Sender<()>), String> {
    let ServeConfig {
        site_dir_cell: site_dir_state,
        asset_registry,
        invoke: invoke_ctx,
        start_port,
        is_evicted,
    } = config;
    // === SETUP PHASE ===
    // Note: We don't check for index.html here - the server can start even for empty folders.
    // ServeDir will return 404 for missing files, and when content is generated (e.g., by
    // a before_build plugin), the file watcher will trigger a rebuild and index.html will exist.

    // Bind BOTH IPv4 + IPv6 loopback up-front. This is the authoritative
    // collision check: the bind itself is the source of truth for "is this
    // port free?", which closes the TOCTOU window that an earlier
    // `is_port_available()` → `bind` sequence used to leave open (a foreign
    // server could slip in between the probe and the bind, leaving moss with
    // a useless half-bind and a confusing "server failed readiness check"
    // error). If neither stack can be bound at any port in the scan range,
    // we return Err synchronously, before spawning anything.
    let (port, listener_v4, listener_v6) = bind_dual_stack_with_scan(start_port).await?;

    // Create shutdown channel for graceful server termination
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // === EXECUTION PHASE ===
    // Start server in background with dynamic directory resolution
    let state_clone = site_dir_state.clone();
    let registry_clone = asset_registry.clone();
    let invoke_ctx_clone = invoke_ctx.clone();

    // Build the router (shared between both code paths)
    //
    // `/__moss/source/*path` is the HTTP mount of the SAME source-asset core the
    // `moss-source://` Tauri scheme uses (`editor::source_asset_protocol::serve_source_asset`).
    // One implementation, two carriers — not two implementations kept in step by a test.
    // It exists so a host with no custom-scheme support (a browser, an Obsidian
    // pane) can still render the editor's SOURCE assets, which ADR-022 requires
    // come from the vault rather than from `.moss/build.nosync`.
    //
    // Internal moss endpoints live under the `/__moss_*` namespace. They MUST
    // be registered as explicit `.route(...)` entries BEFORE the
    // `.fallback(ServeDir)` so a user file at the same path cannot shadow
    // them. Add future moss-internal HTTP endpoints under this prefix,
    // following the same ordering rule.
    let build_router = |state: Arc<std::sync::RwLock<std::path::PathBuf>>,
                        registry: Option<Arc<crate::types::assets::AssetRegistry>>,
                        invoke: Option<super::invoke::InvokeCtx>,
                        is_evicted: EvictedProbe| {
        let registry_for_layer = registry.clone();
        // The `/__moss_health/` route is registered BEFORE `.fallback()` so it
        // always wins over `ServeDir`. The endpoint emits a moss-specific JSON
        // body that `verify_server_ready` checks before accepting a port as
        // moss-owned (see `docs/archive/2026-05-22-preview-port-dual-stack-collision.md`).
        let state_for_source = state.clone();
        let mut router = Router::new()
            .route(MOSS_HEALTH_PATH, get(moss_health_handler))
            .route(
                "/__moss/source/*path",
                get(move |axum::extract::Path(p): axum::extract::Path<String>| {
                    let site_dir = state_for_source.clone();
                    async move { source_asset_route(site_dir, p).await }
                }),
            )
            // Preview-only comment stub: receives the comment form POST and
            // returns an Artalk-shaped success without real-server traffic.
            // Registered before .fallback() so a user file cannot shadow it.
            // The serve-time shim (comment-preview-shim.js) rewrites the
            // form's data-server-url to "/__moss/comments"; the client then
            // POSTs to `/__moss/comments/api/v2/comments`.
            .route(
                "/__moss/comments/api/v2/comments",
                axum::routing::post(super::comment_stub::handle_comment_stub),
            );

        // Read-only HTTP command carrier (`POST /__moss/invoke/*cmd`). Registered
        // BEFORE `.fallback()` (same `/__moss/` ordering rule) and only when a
        // carrier context was threaded in — an absent carrier means the route
        // does not exist (404), never a runtime gate. It inherits the outermost
        // `validate_host_origin` layer and the loopback bind added below, so it
        // needs no auth of its own. See `super::invoke`.
        if let Some(ctx) = invoke {
            // Three carriers share one InvokeCtx (one vault-bound session: the
            // project root and the token that unlocks it, replaced together on
            // a folder switch): read-only (`/__moss/invoke`, token-FREE),
            // authed-read (`/__moss/read`, token-GATED), and mutation
            // (`/__moss/mutate`, token-GATED). The gates bind and compare at
            // request time, never at router-build time, and hand the admitted
            // session to the handler as a request extension — so a folder
            // switch retires the previous vault's token immediately and no
            // request straddles two vaults. Clone what each route needs
            // BEFORE the closures consume their copies.
            let gate_for_read = (ctx.clone(), state.clone());
            let gate_for_mutate = (ctx.clone(), state.clone());
            let gate_for_events = (ctx.clone(), state.clone());
            let ctx_for_invoke = ctx;
            let state_for_invoke = state.clone();

            router = router
                .route(
                    "/__moss/invoke/*cmd",
                    axum::routing::post(
                        move |axum::extract::Path(cmd): axum::extract::Path<String>,
                              axum::Json(args): axum::Json<serde_json::Value>| {
                            let ctx = ctx_for_invoke.clone();
                            let site_dir = state_for_invoke.clone();
                            async move {
                                super::invoke::handle_invoke(ctx, site_dir, cmd, args).await
                            }
                        },
                    ),
                )
                .route(
                    "/__moss/read/*cmd",
                    axum::routing::post(
                        |session: axum::Extension<Arc<super::invoke::Session>>,
                         axum::extract::Path(cmd): axum::extract::Path<String>,
                         axum::Json(args): axum::Json<serde_json::Value>| async move {
                            super::invoke::handle_authed_read(session, cmd, args).await
                        },
                    )
                    // SAME token gate as the mutation route: booting the editor is
                    // a session-scoped capability. `route_layer` runs after the
                    // outer trust boundary and method routing (foreign Origin →
                    // 403, GET → 405), then this → 401 / 415.
                    .route_layer(middleware::from_fn(move |request, next| {
                        let (ctx, site_dir) = gate_for_read.clone();
                        async move { super::carrier_token::require_carrier_token(ctx, site_dir, request, next).await }
                    })),
                )
                .route(
                    "/__moss/mutate/*cmd",
                    axum::routing::post(
                        |session: axum::Extension<Arc<super::invoke::Session>>,
                         axum::extract::Path(cmd): axum::extract::Path<String>,
                         axum::Json(args): axum::Json<serde_json::Value>| async move {
                            super::invoke::handle_mutate(session, cmd, args).await
                        },
                    )
                    // Token + content-type gate on the mutation route ONLY.
                    // `route_layer` runs after the outer trust boundary and after
                    // method routing, so: foreign Origin → 403, GET → 405, then
                    // this → 401 (bad token) / 415 (wrong media type).
                    .route_layer(middleware::from_fn(move |request, next| {
                        let (ctx, site_dir) = gate_for_mutate.clone();
                        async move { super::carrier_token::require_carrier_token(ctx, site_dir, request, next).await }
                    })),
                )
                // The event carrier: one long-lived SSE stream carrying the
                // same typed bus the desktop frontend gets over Tauri IPC.
                // Token-gated like `/__moss/read` — these events describe the
                // vault — but with the media-type half of that gate dropped,
                // since a GET has no body. See `super::events`.
                .route(
                    "/__moss/events",
                    axum::routing::get(super::events::handle_events).route_layer(
                        middleware::from_fn(move |request, next| {
                            let (ctx, site_dir) = gate_for_events.clone();
                            async move { super::carrier_token::require_carrier_token_only(ctx, site_dir, request, next).await }
                        }),
                    ),
                );
        }

        router
            .fallback(move |request: Request<Body>| {
                let state = state.clone();
                let registry = registry.clone();
                async move {
                    // Read current directory from shared state
                    // This is the key to zero-flicker: we resolve the path on each request
                    let current_dir = state.read().unwrap().clone();
                    let path = request.uri().path().to_string();

                    // Check if this is a request for an asset being processed
                    // If so, serve a placeholder SVG instead of 404
                    if let Some(ref reg) = registry {
                        // SOURCE PASSTHROUGH (instant sharp preview).
                        //
                        // The rule: **the preview shows the author their real
                        // file.** For a promised-but-not-yet-encoded variant URL
                        // (image webp/avif or video mp4) whose encoded output is
                        // not yet on disk, serve the FULL ORIGINAL source bytes.
                        // Not a blurred LQIP — that is a PUBLISHED-site
                        // technique, aimed at a visitor waiting on a network, and
                        // it is exactly wrong here: an author looking at their
                        // photo to judge it cannot tell a low-quality stand-in
                        // from a badly-encoded result. `ServeFile` is Range-aware
                        // → video seeking + Content-Length. This takes PRECEDENCE
                        // over the Pending placeholder branch below, and that
                        // order is the rule, not an accident.
                        //
                        // Path-traversal-safe: `src_abs` is a pre-registered
                        // absolute path (set in the same set_pending loop), never
                        // client-joined. A terminally FAILED variant is EXCLUDED
                        // so its warning SVG (handle_asset_request) still wins —
                        // the encode failure stays visible.
                        let encoded = path.trim_start_matches('/');
                        let normalized = urlencoding::decode(encoded)
                            .map(|s| s.into_owned())
                            .unwrap_or_else(|_| encoded.to_string());
                        let is_failed = matches!(
                            reg.get(&normalized),
                            Some(crate::types::assets::AssetState::Failed(_))
                        );
                        if !is_failed && !current_dir.join(&normalized).exists() {
                            if let Some(src_abs) = reg.source_passthrough(&normalized) {
                                // A cloud-evicted original is NOT servable, and
                                // finding that out from `ServeFile` is too late:
                                // it opens the file (a plain `open` succeeds
                                // under the dataless-fail-fast policy), sizes it
                                // from `stat`, and returns 200 with a streaming
                                // body that then dies mid-read on `EDEADLK`. A
                                // truncated 200 on a chosen `<source>` is as
                                // unrecoverable as the 404 ADR-013 forbids. The
                                // `SF_DATALESS` bit is an `lstat` — no download,
                                // no block — so ask before opening and let the
                                // placeholder handler stand in until the
                                // download lands (false on Linux, where no
                                // sync client sets a stat-time bit).
                                if is_evicted(&src_abs) {
                                    log::debug!(
                                        "[preview] source passthrough for {} skipped — {} is cloud-evicted",
                                        normalized,
                                        src_abs.display()
                                    );
                                    return handle_asset_request(&path, reg, &current_dir)
                                        .unwrap_or_else(transparent_stub_response);
                                }
                                // Strip conditional headers so we always return a
                                // fresh 200/206 with the real bytes (mirrors the
                                // ServeDir path below).
                                let (mut parts, body) = request.into_parts();
                                parts.headers.remove(http::header::IF_MODIFIED_SINCE);
                                parts.headers.remove(http::header::IF_NONE_MATCH);
                                let request = Request::from_parts(parts, body);
                                match ServeFile::new(&src_abs).oneshot(request).await {
                                    Ok(resp) if resp.status().is_success() => {
                                        // These bytes are a STAND-IN that a later
                                        // request at this same URL must not get.
                                        // The encoder lands the real variant
                                        // seconds later and the bridge re-fetches
                                        // the URL; without this header the
                                        // browser is free to keep the original,
                                        // because `ServeFile` dates its
                                        // `Last-Modified` from the SOURCE file —
                                        // a photo shot last year earns a
                                        // heuristic freshness lifetime of weeks
                                        // (RFC 9111 § 4.2.2), so the swap never
                                        // happens and the preview shows the
                                        // uncompressed original until a reload.
                                        let mut resp = resp.map(Body::new);
                                        resp.headers_mut().insert(
                                            http::header::CACHE_CONTROL,
                                            http::HeaderValue::from_static(
                                                "no-cache, no-store, must-revalidate",
                                            ),
                                        );
                                        return resp;
                                    }
                                    // Source missing / iCloud-dataless / unreadable.
                                    // Hand off to the placeholder handler, which
                                    // knows what each media type should stand
                                    // behind (a neutral box for video; the 1×1
                                    // transparent stub for an image variant —
                                    // never anything that could be mistaken for
                                    // the author's own picture), and stub if it
                                    // declines. Never 404 a chosen <source>
                                    // (ADR-013), and never block the request
                                    // thread on a synchronous materialize
                                    // (build/media/icloud.rs rule).
                                    _ => {
                                        log::debug!(
                                            "[preview] source passthrough for {} could not read {} — \
                                             serving a placeholder until the encode lands",
                                            normalized,
                                            src_abs.display()
                                        );
                                        return handle_asset_request(&path, reg, &current_dir)
                                            .unwrap_or_else(transparent_stub_response);
                                    }
                                }
                            }
                        }

                        if let Some(response) = handle_asset_request(&path, reg, &current_dir) {
                            return response;
                        }
                    }

                    // Strip conditional request headers to prevent 304 Not Modified responses.
                    // This is critical because:
                    // 1. 304 responses have no body, so bridge script can't be injected
                    // 2. Without bridge script, navigation events aren't sent
                    // 3. Preview mode needs fresh content on every request
                    let (mut parts, body) = request.into_parts();
                    parts.headers.remove(http::header::IF_MODIFIED_SINCE);
                    parts.headers.remove(http::header::IF_NONE_MATCH);
                    let request = Request::from_parts(parts, body);

                    // Serve the file from the current directory
                    let service = ServeDir::new(&current_dir);
                    match service.oneshot(request).await {
                        Ok(response) => {
                            if response.status() == http::StatusCode::NOT_FOUND {
                                super::log_preview_404(&current_dir, &path);
                                // Return a proper HTML 404 page so the iframe-bridge
                                // middleware can inject its script and keep navigation alive.
                                // Without this, the iframe gets a bare 404 with no body,
                                // the bridge script isn't injected, and navigation is dead.
                                Response::builder()
                                    .status(404)
                                    .header("content-type", "text/html; charset=utf-8")
                                    .body(Body::from(format!(
                                        concat!(
                                            "<!DOCTYPE html>\n",
                                            // `moss-not-found` is the machine-readable marker the
                                            // iframe-bridge reports back to the shell so it can
                                            // distinguish this synthetic 404 from a real page
                                            // titled "Not Found" — drives the rename 404-retry
                                            // (preview-actions.ts `decideRenameRetry`). Do not
                                            // rename without updating the bridge + retry consumer.
                                            "<html><head><meta charset=\"UTF-8\">",
                                            "<meta name=\"moss-not-found\" content=\"1\">",
                                            "<title>Not Found</title>\n",
                                            "<style>body{{font-family:system-ui,sans-serif;display:flex;",
                                            "align-items:center;justify-content:center;min-height:80vh;color:#888}}",
                                            ".msg{{text-align:center}}h1{{font-size:4rem;margin:0;font-weight:200}}",
                                            "p{{margin:.5rem 0 0}}</style>\n",
                                            "</head><body><div class=\"msg\"><h1>404</h1><p>{}</p></div></body></html>",
                                        ),
                                        html_escape(&path)
                                    )))
                                    .unwrap()
                            } else {
                                // `ServeDir` turns every non-NotFound io error into a
                                // bodyless 500 — including the `EDEADLK` a page gets when
                                // the cloud has evicted moss's own build output, which is
                                // possible because `.moss/build.nosync/` lives inside the user's
                                // vault. A bodyless 500 renders as a white void with no
                                // bridge script injected, so navigation dies with it.
                                match cloud_offline_response(&current_dir, &path, response.status()) {
                                    Some(offline) => offline,
                                    None => response.map(|body| Body::new(body)),
                                }
                            }
                        },
                        Err(_) => Response::builder().status(500).body(Body::empty()).unwrap(),
                    }
                }
            })
            .layer(middleware::from_fn(content_wrapper::wrap_non_html_content))
            .layer({
                let registry = registry_for_layer.clone();
                middleware::from_fn(move |request, next| {
                    let registry = registry.clone();
                    async move {
                        match registry {
                            Some(reg) => asset_rewriter::rewrite_ready_assets(request, next, reg).await,
                            None => Ok(next.run(request).await),
                        }
                    }
                })
            })
            .layer(middleware::from_fn(inject_iframe_bridge))
            // Outermost layer: added last, so it runs FIRST — ahead of routing,
            // ServeDir and bridge injection — and covers every route including
            // the health check. Refuses a non-loopback Host (DNS-rebinding
            // defense) and a foreign Origin before any handler sees the request.
            .layer(middleware::from_fn(
                super::trust_boundary::validate_host_origin,
            ))
    };

    // Bind the carrier to the served vault before the first request, which
    // mints the session token and publishes it to the vault's loopback-readable
    // file — so a local client (coding agent / Playwright) can authenticate
    // before it makes its first request. A server not rooted in a vault (some
    // headless CLI cases) simply binds nothing: the read-only carrier is
    // unaffected, and any gated request is 401. NEVER logs the token value.
    if let Some(ctx) = &invoke_ctx {
        ctx.bind(&site_dir_state);
    }

    // Spawn the server task. We use a helper closure to avoid duplicating the
    // serve logic — both code paths run the same axum::serve with graceful shutdown.
    let app = build_router(state_clone, registry_clone, invoke_ctx_clone, is_evicted);

    // Both listeners were bound above. Hand them straight to the serve
    // loops — no further chance for a foreign process to slip in.
    let serve_future = async move {
        // Drive both listeners with the same router. Two oneshot
        // receivers feed off the single shutdown signal via a
        // broadcast channel-of-one pattern.
        let (broadcast_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let shutdown_v4 = {
            let mut rx = broadcast_tx.subscribe();
            async move { let _ = rx.recv().await; }
        };
        let shutdown_v6 = {
            let mut rx = broadcast_tx.subscribe();
            async move { let _ = rx.recv().await; }
        };

        let app_v4 = app.clone();
        let app_v6 = app;

        let v4_serve = async move {
            if let Err(e) = axum::serve(listener_v4, app_v4)
                .with_graceful_shutdown(shutdown_v4)
                .await
            {
                log::error!(target: "preview", "Preview server (IPv4) error: {}", e);
            }
        };
        let v6_serve = async move {
            if let Err(e) = axum::serve(listener_v6, app_v6)
                .with_graceful_shutdown(shutdown_v6)
                .await
            {
                log::error!(target: "preview", "Preview server (IPv6) error: {}", e);
            }
        };

        // Wait for the external shutdown signal, then fan it out to
        // both serve loops.
        let bridge = async move {
            let _ = shutdown_rx.await;
            let _ = broadcast_tx.send(());
        };

        tokio::join!(v4_serve, v6_serve, bridge);
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(serve_future);
    } else {
        // Fallback for test environment — spawn a thread with its own runtime
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(serve_future);
        });
    }

    // Verify server is ready before returning
    log::debug!(target: "preview", "Verifying server is ready on port {}", port);
    if verify_server_ready(port).await.is_err() {
        return Err(format!(
            "Server started but failed readiness check on port {}",
            port
        ));
    }

    log::info!(target: "preview", "Preview server ready on http://localhost:{}", port);
    Ok((port, shutdown_tx))
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
