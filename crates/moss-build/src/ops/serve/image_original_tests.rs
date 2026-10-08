use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::ops::serve::router::{start_server, ServeConfig};
use crate::types::content::SiteHashes;
use std::sync::{Arc, RwLock};

const SOURCE: &str = "作品/Portrait, Tomorrow/assets/cover.jpg";
const LEGACY: &str = "作品/portrait-tomorrow/assets/cover.webp";
const ALIAS: &str = "gallery/portraits/assets/cover.webp";
const BYTES: &[u8] = b"original version of the portrait";

struct Site {
    _temp: tempfile::TempDir,
    mp: crate::moss_paths::MossPaths,
}

impl Site {
    fn new(legacy: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mp = crate::moss_paths::MossPaths::new(&root);
        let source = root.join(SOURCE);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(source, BYTES).unwrap();
        let site = Self { _temp: temp, mp };
        let sealed = site.manifest(!legacy);
        let result = crate::build::ship::materialize_and_promote(&sealed, &site.mp, &site.mp.staging_dir(), 1, None,
            crate::build::ship::ShipVerdict::Ship).unwrap();
        assert!(matches!(result, crate::build::ship::Promotion::Promoted));
        sealed.write_to_disk(&site.mp.hashes()).unwrap();
        if legacy {
            std::fs::remove_file(crate::build::manifest::preview_originals::receipt_path(
                &site.mp.generations_dir(), sealed.generation_id())).unwrap();
        }
        site
    }

    fn manifest(&self, receipt: bool) -> crate::build::manifest::SealedManifest {
        let source = self.mp.project_root().join(SOURCE);
        let mut hashes = SiteHashes::default();
        hashes.sources.insert(SOURCE.into(), crate::build::types::SourceMetadata::from_stat(
            crate::build::cache::ObjectStore::hash_file(&source).unwrap(),
            crate::build::stat::FileStat::of(&std::fs::metadata(&source).unwrap()),
        ));
        let mut pending = PendingManifest::new(hashes);
        if receipt {
            pending.register_preview_original(ALIAS.into(), SOURCE.into());
            pending.register_preview_original("gallery/portraits/assets/cover.w800.webp".into(), SOURCE.into());
        }
        std::fs::create_dir_all(self.mp.staging_dir()).unwrap();
        let html = format!("<!doctype html><img src='/{}'>", if receipt { ALIAS } else { LEGACY });
        std::fs::write(self.mp.staging_dir().join("index.html"), &html).unwrap();
        pending.register(&ServedPath::from_source("index.html").unwrap(), html.as_bytes(), HashBucket::Files);
        pending.seal()
    }

    async fn server(&self, probe: super::super::router::EvictedProbe) -> (u16, tokio::sync::oneshot::Sender<()>) {
        let registry = Arc::new(crate::types::assets::AssetRegistry::new());
        registry.set_failed(ALIAS.into(), "a newer attempt failed".into());
        registry.set_source_passthrough(ALIAS.into(), self.mp.project_root().join("unrelated.jpg"));
        start_server(ServeConfig {
            asset_registry: Some(registry), is_evicted: probe,
            ..ServeConfig::new(Arc::new(RwLock::new(self.mp.current_ptr())), 61850)
        }).await.unwrap()
    }

    fn receipt(&self) -> PathBuf {
        crate::build::manifest::preview_originals::receipt_path(&self.mp.generations_dir(), &self.mp.current_generation_id().unwrap())
    }

    fn oid(&self) -> String {
        crate::build::cache::ObjectStore::hash_file(&self.mp.project_root().join(SOURCE)).unwrap()
    }
}

fn bytes(response: ureq::Response) -> Vec<u8> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).unwrap();
    bytes
}

fn status(url: &str) -> u16 {
    match ureq::get(url).call() {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(status, _)) => status,
        Err(error) => panic!("HTTP failed: {error}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_receipt_serves_alias_range_and_head_over_newer_registry() {
    let site = Site::new(false);
    assert!(!site.mp.current_ptr().join(ALIAS).exists());
    let (port, shutdown) = site.server(|_| false).await;
    let url = format!("http://localhost:{port}/{ALIAS}");
    let response = ureq::get(&url).set("Range", "bytes=2-9").call().unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.header("Content-Type"), Some("image/jpeg"));
    assert_eq!(bytes(response), BYTES[2..10]);
    let legacy = format!("http://localhost:{port}/{}", urlencoding::encode(LEGACY));
    assert_eq!(bytes(ureq::get(&legacy).call().unwrap()), BYTES,
        "a new receipt also serves canonical source aliases retained in cached HTML");
    let response = ureq::head(&url).call().unwrap();
    assert_eq!(response.header("Content-Length"), Some(BYTES.len().to_string().as_str()));
    assert!(bytes(response).is_empty());
    assert_eq!(bytes(ureq::get(&format!("http://localhost:{port}/gallery/portraits/assets/cover.w800.webp")).call().unwrap()), BYTES);
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_legacy_missing_copies_pin_canonical_identity_once() {
    let site = Site::new(true);
    let oid = site.oid();
    assert!(!site.mp.current_ptr().join(LEGACY).exists());
    let (port, shutdown) = site.server(|_| false).await;
    let url = format!("http://localhost:{port}/{}", urlencoding::encode(LEGACY));
    assert_eq!(bytes(ureq::get(&url).call().unwrap()), BYTES);
    assert!(site.receipt().exists());
    let frozen = std::fs::read(site.receipt()).unwrap();
    let warm = site.manifest(false);
    crate::build::ship::materialize_and_promote(&warm, &site.mp, &site.mp.staging_dir(), 2, None,
        crate::build::ship::ShipVerdict::Ship).unwrap();
    assert_eq!(std::fs::read(site.receipt()).unwrap(), frozen, "a warm producer map cannot replace legacy proof");
    std::fs::write(site.mp.project_root().join(SOURCE), b"newer portrait version").unwrap();
    let newer = site.manifest(false);
    // The HTML and output-derived id are unchanged; source evidence is newer.
    assert_eq!(newer.generation_id(), site.mp.current_generation_id().unwrap());
    newer.write_to_disk(&site.mp.hashes()).unwrap();
    assert_eq!(bytes(ureq::get(&url).call().unwrap()), BYTES);
    let local = crate::build::cache::ObjectStore::new(site.mp.cache_local_objects());
    std::fs::remove_file(local.blob_path(&oid)).unwrap();
    assert_eq!(status(&url), 404, "a mutated source cannot impersonate the selected original");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_cloud_cas_retries_without_encoding() {
    fn evicted(path: &Path) -> bool { path.with_extension("evicted").exists() }
    let site = Site::new(false);
    let store = crate::build::cache::ObjectStore::new(site.mp.cache_objects());
    let blob = store.blob_path(&site.oid());
    std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
    std::fs::write(blob.with_extension("evicted"), b"pending").unwrap();
    std::fs::remove_file(site.mp.project_root().join(SOURCE)).unwrap();
    let (port, shutdown) = site.server(evicted).await;
    let url = format!("http://localhost:{port}/{ALIAS}");
    assert_eq!(status(&url), 503);
    std::fs::write(&blob, BYTES).unwrap();
    std::fs::remove_file(blob.with_extension("evicted")).unwrap();
    assert_eq!(bytes(ureq::get(&url).call().unwrap()), BYTES);
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_rejects_corrupt_receipt_and_source_or_cas_escape() {
    let site = Site::new(false);
    let (port, shutdown) = site.server(|_| false).await;
    let url = format!("http://localhost:{port}/{ALIAS}");
    let mut receipt: crate::build::manifest::preview_originals::Originals = serde_json::from_slice(&std::fs::read(site.receipt()).unwrap()).unwrap();
    receipt.generation = "0000000000000000".into();
    std::fs::write(site.receipt(), serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert_eq!(status(&url), 404);
    std::fs::write(site.receipt(), b"not JSON").unwrap();
    assert_eq!(status(&url), 404);
    assert_ne!(status(&format!("http://localhost:{port}/%2e%2e/private.webp")), 200);
    let _ = shutdown.send(());

    #[cfg(unix)] {
        let site = Site::new(false);
        let external = tempfile::tempdir().unwrap();
        let secret = external.path().join("cover.jpg");
        std::fs::write(&secret, b"external unrelated bytes").unwrap();
        let store = crate::build::cache::ObjectStore::new(site.mp.cache_objects());
        let blob = store.blob_path(&site.oid());
        std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&secret, &blob).unwrap();
        let (port, shutdown) = site.server(|_| false).await;
        assert_eq!(status(&format!("http://localhost:{port}/{ALIAS}")), 404);
        assert!(std::fs::symlink_metadata(&blob).unwrap().file_type().is_symlink(), "CAS must reject an escape before hashing or deleting it");
        std::fs::remove_file(blob).unwrap();
        std::fs::remove_file(site.mp.project_root().join(SOURCE)).unwrap();
        std::os::unix::fs::symlink(secret, site.mp.project_root().join(SOURCE)).unwrap();
        assert_eq!(status(&format!("http://localhost:{port}/{ALIAS}")), 404);
        let _ = shutdown.send(());
    }
}

#[test]
fn generation_original_identity_collision_keeps_first_receipt_while_valid_output_promotes() {
    let site = Site::new(false);
    let prior = std::fs::read(site.receipt()).unwrap();
    std::fs::write(site.mp.project_root().join(SOURCE), b"new original same output").unwrap();
    let newer = site.manifest(true);
    assert_eq!(newer.generation_id(), site.mp.current_generation_id().unwrap());
    for epoch in [0, 2] {
        let outcome = crate::build::ship::materialize_and_promote(&newer, &site.mp, &site.mp.staging_dir(), epoch, None,
            crate::build::ship::ShipVerdict::Ship).unwrap();
        assert!(matches!(outcome, crate::build::ship::Promotion::Promoted | crate::build::ship::Promotion::Superseded));
        assert_eq!(std::fs::read(site.receipt()).unwrap(), prior);
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_unreadable_known_source_is_retryable() {
    use std::os::unix::fs::PermissionsExt;
    let site = Site::new(false);
    let source = site.mp.project_root().join(SOURCE);
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o0)).unwrap();
    let (port, shutdown) = site.server(|_| false).await;
    let url = format!("http://localhost:{port}/{ALIAS}");
    let pending = status(&url);
    std::fs::set_permissions(source, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(pending, 503, "a read error does not disprove this generation's source identity");
    assert_eq!(bytes(ureq::get(&url).call().unwrap()), BYTES);
    let _ = shutdown.send(());
}
