//! Merging an embedding host's routes into the served router — split out of
//! `router.rs` purely to keep that file under its own size gate; the logic
//! itself is a one-line `Router::merge`. See `ServeConfig::host_routes` for
//! what the field is for and the ordering guarantee this depends on.

use axum::Router;

/// Merge `host_routes` into `router`, or hand `router` back unchanged when
/// the host contributed nothing. The caller merges before `.fallback()`, so
/// a host route wins over a site file at the same path and inherits the
/// trust-boundary layer applied to the whole router.
pub(super) fn merge_host_routes(router: Router, host_routes: Option<Router>) -> Router {
    match host_routes {
        Some(host_routes) => router.merge(host_routes),
        None => router,
    }
}
