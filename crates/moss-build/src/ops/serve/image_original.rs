//! Resolve a missing preview image against the generation the server serves.

use axum::{body::Body, http::{self, Request}, response::Response};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use crate::build::manifest::preview_originals::{Original, Originals};

pub(super) struct RecordedOriginal {
    paths: crate::moss_paths::MossPaths,
    original: Original,
    migration: Option<Originals>,
}

/// Read source identities only from evidence bound to the selected generation.
pub(super) async fn recorded_original(
    root: &Path, url: &str,
) -> Option<RecordedOriginal> {
    let root = root.canonicalize().ok()?;
    let vault = crate::vault::paths::VaultRoot::find_containing(&root)?;
    let mp = crate::moss_paths::MossPaths::from_moss_dir(vault.path().join(".moss"));
    let generation = root.file_name()?.to_str()?.to_string();
    if generation.len() != 16 || !generation.bytes().all(|b| b.is_ascii_hexdigit())
        || mp.generation_dir(&generation).canonicalize().ok()? != root { return None; }
    contained_candidate(mp.project_root(), &crate::build::manifest::preview_originals::receipt_path(&mp.generations_dir(), &generation))?;
    let hashes = contained_candidate(mp.project_root(), &mp.hashes())?;
    let generations = mp.generations_dir();
    let url = url.to_string();
    let read = tokio::task::spawn_blocking(move || {
        if !crate::platform::fail_fast_on_this_thread() { return None; }
        let original = match Originals::read(&generations, &generation).ok()? {
            Some(receipt) => (receipt.get(&url)?, None),
            None => {
                // Older builds have source hashes but no original receipt.
                // The content-derived id must prove this is their manifest,
                // even when no original copy made it into the output tree.
                if crate::build::icloud::is_still_in_the_cloud(&hashes) { return None; }
                let hashes: crate::types::content::SiteHashes = serde_json::from_slice(&std::fs::read(hashes).ok()?).ok()?;
                if crate::build::assets::paths::compute_manifest_generation_id(&hashes.files) != generation { return None; }
                let receipt = Originals::legacy(&hashes, generation);
                (receipt.get(&url)?, Some(receipt))
            }
        };
        let (original, migration) = original;
        if original.oid.len() != 64 || !original.oid.bytes().all(|b| b.is_ascii_hexdigit())
            || Path::new(&original.source).components().any(|part| !matches!(part,
                std::path::Component::Normal(name) if !name.to_string_lossy().starts_with('.')))
            || original.source.is_empty() { return None; }
        Some((original, migration))
    });
    let (original, migration) = tokio::time::timeout(Duration::from_secs(3), read).await.ok()?.ok()??;
    Some(RecordedOriginal { paths: mp, original, migration })
}

enum OriginalBytes {
    Ready(PathBuf),
    Pending(Option<PathBuf>),
    Invalid,
}

/// Pin a verified original in the local content-addressed store before
/// reopening it for Range-aware serving. The mutable author file is copied
/// once; hashing and serving both consume that owned snapshot.
impl RecordedOriginal {
    pub(super) async fn serve(
        self,
        request: Request<Body>,
        is_evicted: super::router::EvictedProbe,
    ) -> Option<Response<Body>> {
        let Self { paths: mp, original, migration } = self;
        let mime = crate::editor::resolve::asset_resolver::mime_for_path(Path::new(&original.source)).to_string();
        let deadline = Instant::now() + Duration::from_secs(3);
        let path = loop {
            let mp = crate::moss_paths::MossPaths::from_moss_dir(mp.root().to_path_buf());
            let original = original.clone();
            let ready = tokio::task::spawn_blocking(move || prepare_original(&mp, &original, is_evicted));
            match tokio::time::timeout_at(deadline.into(), ready).await.ok()?.ok()? {
                OriginalBytes::Ready(path) => break path,
                OriginalBytes::Invalid => return Some(Response::builder().status(404).body(Body::from("Not Found")).unwrap()),
                OriginalBytes::Pending(path) => {
                    if let Some(path) = path { crate::build::cloud_readiness::request_download_foreground(&path); }
                    if Instant::now() >= deadline { return None; }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        };
        if let Some(receipt) = migration {
            let generations = mp.generations_dir();
            let bind = tokio::task::spawn_blocking(move || {
                let lock = crate::build::store_gc::GenerationWriteLock::try_acquire(&generations, &receipt.generation).ok()??;
                let result = generations.join(&receipt.generation).is_dir().then(|| receipt.bind(&generations).unwrap_or(false));
                lock.finish();
                result.filter(|bound| *bound)
            });
            tokio::time::timeout_at(deadline.into(), bind).await.ok()?.ok()??;
        }
        let mut response = serve_original(&path, request, is_evicted).await?;
        response.headers_mut().insert(http::header::CONTENT_TYPE, http::HeaderValue::from_str(&mime).ok()?);
        Some(response)
    }

}

fn prepare_original(
    mp: &crate::moss_paths::MossPaths,
    original: &Original,
    is_evicted: super::router::EvictedProbe,
) -> OriginalBytes {
    use crate::build::cache::{ObjectStore, RecordMode};
    if !crate::platform::fail_fast_on_this_thread() { return OriginalBytes::Pending(None); }
    // Test the local replica before the optional shared replica. Neither a
    // cloud-only cache query nor an unused author source starts a download.
    for base in [mp.cache_local_objects(), mp.cache_objects()] {
        let store = ObjectStore::new(base.clone());
        let path = store.blob_path(&original.oid);
        let Some(owner) = contained_candidate(mp.project_root(), &base) else { return OriginalBytes::Invalid; };
        let Some(path) = contained_candidate(mp.project_root(), &path)
            .filter(|path| path.starts_with(owner)) else { return OriginalBytes::Invalid; };
        if is_evicted(&path) || crate::build::icloud::is_still_in_the_cloud(&path) { continue; }
        if let Some(path) = store.get_path(&original.oid) { return OriginalBytes::Ready(path); }
    }
    let joined = mp.project_root().join(&original.source);
    let source = crate::editor::source_asset::resolve_scoped(mp.project_root(), &original.source);
    if source.is_none() {
        if joined.exists() { return OriginalBytes::Invalid; }
        // A hidden cloud stub has no file at the original path yet. Validate
        // its parent before handing even that path to the native reader.
        let safe_parent = joined.parent().and_then(|parent| parent.canonicalize().ok())
            .zip(mp.project_root().canonicalize().ok())
            .is_some_and(|(parent, root)| parent.starts_with(root));
        if safe_parent && !joined.exists() && crate::build::icloud::is_still_in_the_cloud(&joined) {
            return OriginalBytes::Pending(Some(joined));
        }
        let shared = ObjectStore::new(mp.cache_objects()).blob_path(&original.oid);
        let Some(shared) = contained_candidate(mp.project_root(), &shared) else { return OriginalBytes::Invalid; };
        if crate::build::icloud::is_still_in_the_cloud(&shared) || is_evicted(&shared) {
            return OriginalBytes::Pending(Some(shared));
        }
        return OriginalBytes::Invalid;
    }
    let source = source.unwrap();
    if is_evicted(&source) || crate::build::icloud::is_still_in_the_cloud(&source) {
        return OriginalBytes::Pending(Some(source));
    }
    if contained_candidate(mp.project_root(), &mp.cache_tmp()).is_none()
        || contained_candidate(mp.project_root(), &mp.cache_local_objects()).is_none()
        || crate::build::io_utils::create_output_dir_all(&mp.cache_tmp()).is_err() { return OriginalBytes::Invalid; }
    let snapshot = mp.cache_tmp().join(format!("preview-original-{}.pending", uuid::Uuid::new_v4()));
    let pinned = (|| -> std::io::Result<OriginalBytes> {
        let mut input = std::fs::File::open(&source)?;
        // allow:raw_write an owned source snapshot under the local build cache
        let mut output = std::fs::File::create(&snapshot)?;
        std::io::copy(&mut input, &mut output)?;
        if output.metadata()?.len() != original.size { return Ok(OriginalBytes::Invalid); }
        drop(output);
        if ObjectStore::hash_file(&snapshot).map_err(std::io::Error::other)? != original.oid {
            return Ok(OriginalBytes::Invalid);
        }
        let local = ObjectStore::new(mp.cache_local_objects());
        if contained_candidate(mp.project_root(), &local.blob_path(&original.oid)).is_none() { return Ok(OriginalBytes::Invalid); }
        let oid = local.store_file(&snapshot, RecordMode::Request).map_err(std::io::Error::other)?;
        match local.get_path(&oid) {
            Some(path) => Ok(OriginalBytes::Ready(path)),
            None => Ok(OriginalBytes::Pending(None)),
        }
    })();
    // allow:unlink this request's temporary source snapshot
    let _ = std::fs::remove_file(snapshot);
    match pinned {
        Ok(result) => result,
        Err(error) if crate::build::icloud::is_offline_not_absent(&source, &error) => OriginalBytes::Pending(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OriginalBytes::Invalid,
        Err(_) => OriginalBytes::Pending(None),
    }
}

/// Validate an existing file or the nearest existing ancestor of a pending
/// path before any hash, copy, or foreground download crosses the boundary.
fn contained_candidate(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let mut parent = candidate;
    let mut missing = Vec::new();
    while std::fs::symlink_metadata(parent).is_err() {
        missing.push(parent.file_name()?);
        parent = parent.parent()?;
    }
    let mut canonical = parent.canonicalize().ok()?;
    if !canonical.starts_with(root) { return None; }
    for name in missing.into_iter().rev() { canonical.push(name); }
    Some(canonical)
}

#[cfg(test)]
#[path = "image_original_tests.rs"]
mod tests;

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
