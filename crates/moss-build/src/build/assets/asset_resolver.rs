//! Directory assets are verified against their pinned archive digest, extracted
//! into producer-owned staging, and published once under an immutable cache key.
//! A sealed file inventory detects incomplete caches without rehashing each file.
//! Legacy mutable caches are left untouched and rebuilt into the managed layout.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use serde::{Deserialize, Serialize};
use super::download::{check_disk_space, download_with_progress, verify_sha256, DownloadProgress};

const DOWNLOAD_TIMEOUT_SECS: u64 = 300;
const MANIFEST: &str = ".moss-asset-manifest.json";
const CONTENT: &str = "content";

/// Configuration for a directory-based asset that can be cached and downloaded.
///
/// Unlike `BinaryConfig`, this describes a directory of static files (not an
/// executable). Resolution is simpler: check cache → download.
#[derive(Debug, Clone)]
pub struct AssetConfig {
    /// Human-readable name, e.g. "jupyterlite".
    /// Used with the archive digest as the cache directory key.
    pub name: String,

    /// URL to download the asset archive from. Must be a pinned, immutable
    /// URL (a tagged release, never `latest`) — the `sha256` below has to
    /// keep matching what this serves.
    /// Currently only zip archives are supported.
    pub download_url: String,

    /// Expected SHA-256 checksum of the archive (hex string). Verified on
    /// download, and doubles as the cache key (see module docs).
    pub sha256: String,

    /// Minimum disk space required (in bytes) before attempting a download.
    pub required_disk_space: Option<u64>,
}


#[derive(Serialize, Deserialize)]
struct AssetManifest {
    archive_sha256: String,
    files: BTreeMap<String, u64>,
}

/// Resolve the pinned bundle, returning its content directory. Internal cache
/// metadata lives beside the content, so copying this path never publishes it.
pub fn resolve_asset_directory(
    config: &AssetConfig,
    on_progress: Option<&DownloadProgress>,
) -> Result<PathBuf, String> {
    resolve_asset_directory_in(&get_moss_assets_dir()?, config, on_progress)
}

fn managed_directory(root: &Path, config: &AssetConfig) -> Result<PathBuf, String> {
    if !safe_relative_path(&config.name) || Path::new(&config.name).components().count() != 1
        || config.sha256.len() != 64 || !config.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid asset name or archive digest".into());
    }
    Ok(root.join(format!("{}-{}", config.name, config.sha256)))
}

fn resolve_asset_directory_in(
    assets_root: &Path,
    config: &AssetConfig,
    on_progress: Option<&DownloadProgress>,
) -> Result<PathBuf, String> {
    let asset_dir = managed_directory(assets_root, config)?;
    if cache_ready(&asset_dir, &config.sha256)? {
        return Ok(asset_dir.join(CONTENT));
    }
    log::info!("Downloading {} from {}...", config.name, config.download_url);
    if let Some(required) = config.required_disk_space {
        check_disk_space(assets_root, required)?;
    }
    let data = download_with_progress(&config.download_url, DOWNLOAD_TIMEOUT_SECS, on_progress)?;
    let staged = StagedAsset::extract(&data, assets_root, &config.sha256)?;
    staged.publish(&asset_dir)
}

/// The assets directory is machine state, separate from user design assets.
pub fn get_moss_assets_dir() -> Result<PathBuf, String> {
    let assets_dir = crate::infra::home::moss_home()?.join("assets");
    std::fs::create_dir_all(&assets_dir) // allow:raw_write machine asset cache
        .map_err(|e| format!("Failed to create assets directory: {e}"))?;
    Ok(assets_dir)
}

// Use the same lexical boundary for archive entries and persisted inventory.
// Reject Windows separators/prefixes even when this cache is produced on Unix.
fn safe_relative_path(name: &str) -> bool {
    !name.is_empty() && !name.contains("..") && !name.contains('\\') && !name.contains(':')
        && Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
}

fn cache_ready(directory: &Path, sha256: &str) -> Result<bool, String> {
    for (path, is_directory) in [(directory.to_path_buf(), true), (directory.join(CONTENT), true), (directory.join(MANIFEST), false)] {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if (is_directory && !metadata.is_dir()) || (!is_directory && !metadata.is_file()) => return Ok(false),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("Read asset cache metadata: {e}")),
        }
    }
    let bytes = match std::fs::read(directory.join(MANIFEST)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Read asset inventory: {e}")),
    };
    let manifest: AssetManifest = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(false),
    };
    if manifest.archive_sha256 != sha256 || manifest.files.is_empty()
        || manifest.files.keys().any(|name| !safe_relative_path(name)) {
        return Ok(false);
    }
    let content = directory.join(CONTENT);
    for (name, length) in manifest.files {
        // Check every component without following symlinks out of the cache.
        let mut path = content.clone();
        let components: Vec<_> = Path::new(&name).components().collect();
        for (index, component) in components.iter().enumerate() {
            path.push(component.as_os_str());
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(format!("Read cached asset metadata: {e}")),
            };
            let last = index + 1 == components.len();
            if (last && (!metadata.is_file() || metadata.len() != length))
                || (!last && !metadata.is_dir()) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The staging directory and its sealed inventory have one producer owner.
struct StagedAsset {
    directory: tempfile::TempDir,
    sha256: String,
}

impl StagedAsset {
    fn extract(data: &[u8], assets_root: &Path, sha256: &str) -> Result<Self, String> {
        verify_sha256(data, sha256)?;
        let directory = tempfile::Builder::new().prefix(".moss-asset-pending-")
            .tempdir_in(assets_root).map_err(|e| format!("Create asset staging: {e}"))?;
        let content = directory.path().join(CONTENT);
        std::fs::create_dir(&content) // allow:raw_write owned cache staging
            .map_err(|e| format!("Create asset content: {e}"))?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))
            .map_err(|e| format!("Failed to open zip archive: {e}"))?;
        let mut files = BTreeMap::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| format!("Read zip entry {i}: {e}"))?;
            let name = file.name().trim_end_matches('/').to_string();
            if !safe_relative_path(&name) {
                log::warn!("Skipping zip entry with unsafe path: {}", file.name());
                continue;
            }
            let path = content.join(&name);
            if file.is_dir() {
                std::fs::create_dir_all(path) // allow:raw_write owned cache staging
                    .map_err(|e| format!("Create asset directory: {e}"))?;
            } else {
                std::fs::create_dir_all(path.parent().unwrap()) // allow:raw_write owned cache staging
                    .map_err(|e| format!("Create asset parent: {e}"))?;
                let mut output = std::fs::File::create(path) // allow:raw_write owned cache staging
                    .map_err(|e| format!("Create asset file: {e}"))?;
                let length = std::io::copy(&mut file, &mut output)
                    .map_err(|e| format!("Extract asset file: {e}"))?;
                output.sync_all().map_err(|e| format!("Flush asset file: {e}"))?;
                files.insert(name, length);
            }
        }
        if files.is_empty() { return Err("Asset archive contains no safe files".into()); }
        crate::infra::atomic_write::write_json_atomic(&directory.path().join(MANIFEST),
            &AssetManifest { archive_sha256: sha256.into(), files })?;
        Ok(Self { directory, sha256: sha256.into() })
    }

    fn publish(self, asset_dir: &Path) -> Result<PathBuf, String> {
        let root = asset_dir.parent().ok_or("Asset cache has no parent")?;
        let root = root.canonicalize().map_err(|e| format!("Canonicalize assets root: {e}"))?;
        let namespace = format!("asset-{}", asset_dir.file_name().ok_or("Asset cache has no name")?.to_string_lossy());
        let _lock = crate::infra::folder_lock::acquire_named(&root, &namespace)?;
        if cache_ready(asset_dir, &self.sha256)? {
            return Ok(asset_dir.join(CONTENT));
        }
        let quarantine = if asset_dir.try_exists().map_err(|e| format!("Inspect asset cache: {e}"))? {
            // Own only invalid content. Ready caches and other digest keys are
            // never removed while another producer prepares its bundle.
            let quarantine = tempfile::Builder::new().prefix(".moss-asset-invalid-")
                .tempdir_in(&root).map_err(|e| format!("Create asset quarantine: {e}"))?;
            std::fs::rename(asset_dir, quarantine.path().join("cache")) // allow:unlink quarantine invalid machine cache
                .map_err(|e| format!("Quarantine incomplete asset cache: {e}"))?;
            Some(quarantine)
        } else { None };
        if let Err(error) = std::fs::rename(self.directory.path(), asset_dir) { // allow:unlink publish owned cache staging
            if let Some(quarantine) = quarantine {
                let retained = quarantine.keep();
                return Err(format!("Publish asset cache: {error}; invalid cache retained at {}", retained.display()));
            }
            return Err(format!("Publish asset cache: {error}"));
        }
        // Successful repair discards only the invalid content this attempt owns.
        drop(quarantine);
        Ok(asset_dir.join(CONTENT))
    }
}

#[cfg(test)]
fn extract_zip_to_asset_dir(data: &[u8], asset_dir: &Path, sha256: &str) -> Result<PathBuf, String> {
    StagedAsset::extract(data, asset_dir.parent().unwrap(), sha256)?.publish(asset_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A minimal in-memory zip with the given (name, content) entries.
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options = zip::write::FileOptions::default();
            for (name, content) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(data))
    }

    #[test]
    fn test_get_moss_assets_dir_creates_directory() {
        let dir = get_moss_assets_dir();
        assert!(dir.is_ok(), "Should create/return assets dir: {:?}", dir);
        let dir = dir.unwrap();
        assert!(dir.exists(), "Assets directory should exist");
        assert!(
            dir.ends_with("assets"),
            "Must end with 'assets', not 'theme'. \
             ~/.moss/assets/ is for cached build tool downloads (JupyterLite, etc.). \
             ~/.moss/theme/ is for user-facing design assets. Got: {:?}",
            dir
        );
    }

    #[test]
    fn resolve_returns_cache_whose_digest_matches_the_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let data = zip_of(&[("index.html", b"<html>cached</html>")]);
        let sha = sha256_hex(&data);

        let asset_dir = tmp.path().join(format!("test-asset-{sha}"));
        extract_zip_to_asset_dir(&data, &asset_dir, &sha).unwrap();

        // Unreachable URL proves the cache hit never downloads.
        let config = AssetConfig {
            name: "test-asset".to_string(),
            download_url: "https://127.0.0.1:1/nonexistent.zip".to_string(),
            sha256: sha,
            required_disk_space: None,
        };
        let resolved = resolve_asset_directory_in(tmp.path(), &config, None).unwrap();
        assert_eq!(resolved, asset_dir.join(CONTENT));
    }

    #[test]
    fn resolve_rejects_cache_from_a_different_pin() {
        // A directory extracted under one pin must not satisfy another —
        // this is the "installed machine never upgrades" bug. The stale cache
        // fails the digest compare and resolution proceeds to download, which
        // errors here (unreachable URL) instead of returning the stale path.
        let tmp = tempfile::tempdir().unwrap();
        let data = zip_of(&[("index.html", b"<html>old bundle</html>")]);
        extract_zip_to_asset_dir(&data, &tmp.path().join("test-asset"), &sha256_hex(&data))
            .unwrap();

        let config = AssetConfig {
            name: "test-asset".to_string(),
            download_url: "https://127.0.0.1:1/nonexistent.zip".to_string(),
            sha256: "0".repeat(64),
            required_disk_space: None,
        };
        let err = resolve_asset_directory_in(tmp.path(), &config, None).unwrap_err();
        assert!(
            err.contains("ownload") || err.contains("http") || err.contains("127.0.0.1"),
            "stale cache should fall through to (failed) download, got: {err}"
        );
    }

    #[test]
    fn resolve_rejects_pre_pin_cache_without_marker() {
        // A cache left by an older moss has no digest marker; it must not be
        // trusted just because the directory exists (the old VERSION-file rule).
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-asset");
        std::fs::create_dir_all(&asset_dir).unwrap();
        std::fs::write(asset_dir.join("VERSION"), "jupyterlite=0.8.0\n").unwrap();

        let config = AssetConfig {
            name: "test-asset".to_string(),
            download_url: "https://127.0.0.1:1/nonexistent.zip".to_string(),
            sha256: "0".repeat(64),
            required_disk_space: None,
        };
        assert!(
            resolve_asset_directory_in(tmp.path(), &config, None).is_err(),
            "markerless cache must fall through to download, not be returned"
        );
    }

    #[test]
    fn test_extract_zip_to_asset_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-extract");
        let buf = zip_of(&[("VERSION", b"test=1.0.0\n"), ("index.html", b"<html>test</html>")]);
        let sha = sha256_hex(&buf);

        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha);
        assert!(result.is_ok(), "Extract should succeed: {:?}", result);

        assert!(asset_dir.exists(), "Asset dir should exist");
        assert!(asset_dir.join(CONTENT).join("VERSION").exists(), "VERSION should exist");
        assert!(
            asset_dir.join(CONTENT).join("index.html").exists(),
            "index.html should exist"
        );
        assert_eq!(
            serde_json::from_slice::<AssetManifest>(&std::fs::read(asset_dir.join(MANIFEST)).unwrap()).unwrap().archive_sha256,
            sha,
            "sealed inventory must record the verified archive digest"
        );

        let html = std::fs::read_to_string(asset_dir.join(CONTENT).join("index.html")).unwrap();
        assert_eq!(html, "<html>test</html>");
    }

    #[test]
    fn test_extract_zip_atomic_replaces_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-replace");

        // Create an initial directory with old content
        std::fs::create_dir_all(&asset_dir).unwrap();
        std::fs::write(asset_dir.join("old.txt"), "old content").unwrap();

        let buf = zip_of(&[("new.txt", b"new content")]);

        // Invalid managed content is quarantined before replacement.
        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha256_hex(&buf));
        assert!(result.is_ok());

        // Old file gone, new file present
        assert!(
            !asset_dir.join("old.txt").exists(),
            "Old file should be removed"
        );
        assert!(
            asset_dir.join(CONTENT).join("new.txt").exists(),
            "New file should exist"
        );
    }

    #[test]
    fn test_extract_zip_skips_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-traversal");

        let buf = zip_of(&[("safe.txt", b"safe"), ("../evil.txt", b"evil")]);
        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha256_hex(&buf));
        assert!(result.is_ok());

        assert!(asset_dir.join(CONTENT).join("safe.txt").exists());
        // The evil file should NOT have been extracted outside
        assert!(
            !tmp.path().join("evil.txt").exists(),
            "Path traversal entry should be skipped"
        );
    }

    #[test]
    fn test_extract_zip_preserves_other_producers_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-cleanup");
        let tmp_dir = asset_dir.with_extension("tmp");

        // Simulate a leftover .tmp from a previous failed extraction
        std::fs::create_dir_all(&tmp_dir).unwrap();
        std::fs::write(tmp_dir.join("leftover.txt"), "leftover").unwrap();

        let buf = zip_of(&[("VERSION", b"v1")]);
        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha256_hex(&buf));
        assert!(result.is_ok());

        // Staging belonging to another producer is never deleted.
        assert!(tmp_dir.join("leftover.txt").exists(), "Other producers staging must remain untouched");
        assert!(asset_dir.join(CONTENT).join("VERSION").exists());
    }
    #[test]
    fn legacy_matching_marker_does_not_certify_partial_content() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join("test-asset");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join(".moss-archive-sha256"), "0".repeat(64)).unwrap();
        std::fs::write(legacy.join("index.html"), "partial").unwrap();
        let config = AssetConfig { name: "test-asset".into(), sha256: "0".repeat(64),
            download_url: "https://127.0.0.1:1/nonexistent.zip".into(), required_disk_space: None };
        assert!(resolve_asset_directory_in(tmp.path(), &config, None).is_err());
        assert!(legacy.join("index.html").exists(), "legacy evidence stays untouched");
    }

    #[test]
    fn concurrent_producers_publish_complete_once_and_reuse_winner() {
        let tmp = tempfile::tempdir().unwrap();
        let data = zip_of(&[("first.html", b"first"), ("config-utils.js", b"second")]);
        let sha = sha256_hex(&data);
        let destination = tmp.path().join(format!("bundle-{sha}"));
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let publish_a = std::sync::Arc::new(std::sync::Barrier::new(2));
        let publish_b = std::sync::Arc::new(std::sync::Barrier::new(2));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let producers: Vec<_> = [publish_a.clone(), publish_b.clone()].into_iter().map(|publish| {
            let start = start.clone();
            let ready = ready_tx.clone();
            let data = data.clone();
            let sha = sha.clone();
            let root = tmp.path().to_path_buf();
            let destination = destination.clone();
            std::thread::spawn(move || {
                start.wait();
                let staged = StagedAsset::extract(&data, &root, &sha).unwrap();
                ready.send(staged.directory.path().to_path_buf()).unwrap();
                publish.wait();
                staged.publish(&destination).unwrap()
            })
        }).collect();
        start.wait();
        let pending_first = ready_rx.recv().unwrap();
        let pending_second = ready_rx.recv().unwrap();
        assert_ne!(pending_first, pending_second);
        let mut producers = producers.into_iter();
        let producer_a = producers.next().unwrap();
        let producer_b = producers.next().unwrap();
        publish_a.wait();
        let published = producer_a.join().unwrap();
        assert!(cache_ready(&destination, &sha).unwrap());
        let pending_b = if pending_first.exists() { pending_first } else { pending_second };
        assert!(pending_b.join(CONTENT).join("first.html").exists());
        let winner = destination.join("published-owner");
        std::fs::write(&winner, b"producer-a").unwrap();
        publish_b.wait();
        assert_eq!(producer_b.join().unwrap(), published);
        assert_eq!(std::fs::read(&winner).unwrap(), b"producer-a", "ready winner must never be replaced");
        assert!(cache_ready(&destination, &sha).unwrap());
        assert_eq!(std::fs::read(published.join("first.html")).unwrap(), b"first");
        assert!(!pending_b.exists(), "loser cleans only its owned staging");
    }

    #[test]
    fn inventory_detects_missing_truncated_and_unsafe_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let data = zip_of(&[("nested/module.js", b"complete")]);
        let sha = sha256_hex(&data);
        let destination = tmp.path().join("managed");
        let content = extract_zip_to_asset_dir(&data, &destination, &sha).unwrap();
        std::fs::write(content.join("nested/module.js"), b"short").unwrap();
        assert!(!cache_ready(&destination, &sha).unwrap());
        std::fs::remove_file(content.join("nested/module.js")).unwrap();
        assert!(!cache_ready(&destination, &sha).unwrap());
        let manifest = AssetManifest { archive_sha256: sha.clone(), files: BTreeMap::from([("../outside".into(), 1)]) };
        std::fs::write(destination.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(!cache_ready(&destination, &sha).unwrap());
        let repaired = extract_zip_to_asset_dir(&data, &destination, &sha).unwrap();
        assert_eq!(std::fs::read(repaired.join("nested/module.js")).unwrap(), b"complete");
        assert!(cache_ready(&destination, &sha).unwrap());
        assert!(!std::fs::read_dir(tmp.path()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(".moss-asset-invalid-")), "successful repair removes only its invalid quarantine");
    }

    #[test]
    fn different_pins_preserve_published_readers() {
        let tmp = tempfile::tempdir().unwrap();
        let first = zip_of(&[("index.html", b"first")]);
        let second = zip_of(&[("index.html", b"second")]);
        let first_sha = sha256_hex(&first);
        let second_sha = sha256_hex(&second);
        let first_dir = tmp.path().join(format!("bundle-{first_sha}"));
        let second_dir = tmp.path().join(format!("bundle-{second_sha}"));
        let old_reader = extract_zip_to_asset_dir(&first, &first_dir, &first_sha).unwrap();
        let new_reader = extract_zip_to_asset_dir(&second, &second_dir, &second_sha).unwrap();
        assert_ne!(old_reader, new_reader);
        assert_eq!(std::fs::read(old_reader.join("index.html")).unwrap(), b"first");
        assert_eq!(std::fs::read(new_reader.join("index.html")).unwrap(), b"second");
    }

    #[test]
    fn resolver_repairs_partial_legacy_and_managed_caches_from_pinned_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let data = zip_of(&[("index.html", b"entry"), ("module.js", b"complete")]);
        let sha = sha256_hex(&data);
        let mut server = mockito::Server::new();
        let download = server.mock("GET", "/bundle.zip").with_status(200)
            .with_body(data).expect(2).create();
        let config = AssetConfig { name: "bundle".into(), sha256: sha.clone(),
            download_url: format!("{}/bundle.zip", server.url()), required_disk_space: None };
        let legacy = tmp.path().join("bundle");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join(".moss-archive-sha256"), &sha).unwrap();
        std::fs::write(legacy.join("index.html"), b"entry").unwrap();
        let resolved = resolve_asset_directory_in(tmp.path(), &config, None).unwrap();
        assert_eq!(std::fs::read(resolved.join("module.js")).unwrap(), b"complete");
        assert!(!legacy.join("module.js").exists(), "old cache is preserved");
        std::fs::remove_file(resolved.join("module.js")).unwrap();
        let repaired = resolve_asset_directory_in(tmp.path(), &config, None).unwrap();
        assert_eq!(repaired, resolved);
        assert_eq!(std::fs::read(repaired.join("module.js")).unwrap(), b"complete");
        assert!(!repaired.join(MANIFEST).exists(), "inventory must not ship with content");
        download.assert();
    }

}
