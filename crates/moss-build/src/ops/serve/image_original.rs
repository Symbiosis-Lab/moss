//! Resolve a missing preview image against the generation the server serves.

use axum::{body::Body, http::{self, Request}, response::Response};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tower::ServiceExt;
use tower_http::services::ServeFile;

/// Compare resolved generation directories, not path names: a server may hold
/// either the `current` symlink or the generation it names.
pub(super) fn serves_current(root: &Path) -> bool {
    let Some(vault) = crate::vault::paths::VaultRoot::find_containing(root) else { return false; };
    let current = crate::moss_paths::MossPaths::from_moss_dir(vault.path().join(".moss")).current_ptr();
    matches!((root.canonicalize(), current.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Find a unique original output in the selected generation. The URL is
/// validated before any directory is read, and the result cannot escape that
/// generation through a symlink. No current build's registry is consulted.
pub(super) fn original_in_served_root(root: &Path, url: &str) -> Option<PathBuf> {
    if !url.ends_with(".webp") { return None; }
    let served = crate::build::served_path::ServedPath::from_source(url).ok()?;
    if served.as_str() != url { return None; }
    let rel = Path::new(served.as_str());
    let parent = rel.parent()?;
    let selected_root = root.canonicalize().ok()?;
    let directory = root.join(parent).canonicalize().ok()?;
    if !directory.starts_with(&selected_root) { return None; }
    let rung_width = url.strip_suffix(".webp")
        .and_then(|stem| stem.rsplit_once(".w"))
        .and_then(|(_, width)| width.parse::<u32>().ok())
        .filter(|width| moss_core::asset_paths::LADDER.contains(width));
    let mut match_path = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
        let file = entry.path();
        let source = crate::build::icloud::icloud_stub_target(&file).unwrap_or(file);
        let Some(name) = source.file_name() else { continue; };
        let candidate = parent.join(name);
        let Some(candidate_rel) = candidate.to_str() else { continue; };
        let Some(ext) = candidate.extension().and_then(|ext| ext.to_str()) else { continue; };
        if !moss_core::asset_paths::is_ladder_source_ext(ext) { continue; }
        let matches = moss_core::asset_paths::to_webp(candidate_rel) == url
            || rung_width.is_some_and(|width|
                moss_core::asset_paths::to_webp_rung(candidate_rel, width) == url);
        if !matches { continue; }
        let disk = if source.exists() {
            let Ok(canonical) = source.canonicalize() else { continue; };
            if !canonical.starts_with(&selected_root) { continue; }
            canonical
        } else {
            if !crate::build::icloud::is_still_in_the_cloud(&source) { continue; }
            source
        };
        if match_path.replace(disk).is_some() { return None; }
    }
    match_path
}

/// Wait only for this image's foreground materialization. A dropped request
/// drops this future; the bounded native reader keeps its own lifetime.
pub(super) async fn serve_original(
    source: &Path,
    request: Request<Body>,
    is_evicted: super::router::EvictedProbe,
) -> Option<Response<Body>> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();
    let mut headers = request.headers().clone();
    headers.remove(http::header::IF_MODIFIED_SINCE);
    headers.remove(http::header::IF_NONE_MATCH);
    if !source.exists() && !crate::build::icloud::is_still_in_the_cloud(source) { return None; }
    // Keep image requests short enough that pending media cannot occupy the
    // browser's HTTP slots while page, style, and map requests need them. The
    // native foreground fetch continues after this request returns 503.
    let deadline = Instant::now() + Duration::from_secs(3);
    if is_evicted(source) || crate::build::icloud::is_still_in_the_cloud(source) {
        crate::build::cloud_readiness::request_download_foreground(source);
    }
    while is_evicted(source) || crate::build::icloud::is_still_in_the_cloud(source) {
        if !source.exists() && !crate::build::icloud::is_still_in_the_cloud(source) { return None; }
        if Instant::now() >= deadline { return None; }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut next = Request::new(Body::empty());
    *next.method_mut() = method;
    *next.uri_mut() = uri;
    *next.version_mut() = version;
    *next.headers_mut() = headers;
    let response = tokio::time::timeout_at(deadline.into(), ServeFile::new(source).oneshot(next))
        .await.ok()?.ok()?;
    if !response.status().is_success() { return None; }
    // The materializer has asked for the entire file before this point. Keep
    // ServeFile's existing streaming and Range behavior for large originals.
    let mut response = response.map(Body::new);
    response.headers_mut().insert(http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-cache, no-store, must-revalidate"));
    Some(response)
}
