//! Directory-based asset resolution pipeline.
//!
//! Provides a system for resolving directory-based assets (like JupyterLite)
//! through a 3-step resolution chain:
//!
//! 1. Check the `~/.moss/assets/<name>/` cache — valid iff the directory's
//!    archive-digest marker matches the configured `sha256`
//! 2. Download from configured source (GitHub Release or direct URL)
//! 3. Atomic extraction: unzip to `.tmp` directory, then rename into place
//!
//! # Difference from `binary_resolver`
//!
//! `binary_resolver` handles single executable binaries with version checks
//! (runs the binary with `--version`), system PATH lookup, and per-platform
//! downloads. This module handles **directory-based assets** — a directory of
//! static files (like JupyterLite's HTML/JS/WASM distribution) that:
//!
//! - Are platform-independent (one download for all OS/arch)
//! - Cannot be "executed" or version-checked as a command
//! - Are cached as a directory, not a single file
//!
//! # Cache identity is the archive digest
//!
//! The cache used to be "valid because a VERSION file exists", which meant an
//! installed machine never upgraded: the version was read but compared to
//! nothing, and the download URL said `latest`, so two machines building the
//! same site could hold different bundles forever. Now the configured
//! `sha256` is both the download check and the cache key — extraction records
//! the archive digest in the directory, and a pin bump invalidates the cache
//! by construction.

use std::path::{Path, PathBuf};

use super::download::{check_disk_space, download_with_progress, verify_sha256, DownloadProgress};

/// Download timeout in seconds (5 minutes).
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

/// Marker file inside a cached asset directory recording the SHA-256 of the
/// archive it was extracted from. Written into the extraction temp dir before
/// the atomic rename, so a directory either carries its true digest or does
/// not exist.
const ARCHIVE_DIGEST_MARKER: &str = ".moss-archive-sha256";

// =============================================================================
// Types
// =============================================================================

/// Configuration for a directory-based asset that can be cached and downloaded.
///
/// Unlike `BinaryConfig`, this describes a directory of static files (not an
/// executable). Resolution is simpler: check cache → download.
#[derive(Debug, Clone)]
pub struct AssetConfig {
    /// Human-readable name, e.g. "jupyterlite".
    /// Also used as the subdirectory name under `~/.moss/assets/`.
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

// =============================================================================
// Main resolution function
// =============================================================================

/// Resolves an asset directory, returning its absolute path.
///
/// 1. Check `~/.moss/assets/<name>/` cache — if its archive-digest marker
///    matches `config.sha256`, return it
/// 2. Download from configured URL (with progress reporting), verify SHA-256
/// 3. Atomic extraction: unzip to `.tmp`, then rename
///
/// A cache from an older moss (no marker) or a different pin simply fails the
/// comparison and is replaced by the pinned download.
pub fn resolve_asset_directory(
    config: &AssetConfig,
    on_progress: Option<&DownloadProgress>,
) -> Result<PathBuf, String> {
    resolve_asset_directory_in(&get_moss_assets_dir()?, config, on_progress)
}

/// [`resolve_asset_directory`] against an explicit assets root — the testable
/// core; the public entry point supplies `~/.moss/assets/`.
fn resolve_asset_directory_in(
    assets_root: &Path,
    config: &AssetConfig,
    on_progress: Option<&DownloadProgress>,
) -> Result<PathBuf, String> {
    let asset_dir = assets_root.join(&config.name);

    // Step 1: Check cache — valid iff the recorded archive digest matches the pin.
    let marker = asset_dir.join(ARCHIVE_DIGEST_MARKER);
    match std::fs::read_to_string(&marker) {
        Ok(recorded) if recorded.trim() == config.sha256 => {
            log::info!(
                "Using cached {} assets at {}",
                config.name,
                asset_dir.display()
            );
            return Ok(asset_dir);
        }
        Ok(recorded) => {
            log::info!(
                "Cached {} assets are a different bundle (have {}, pin wants {}) — re-downloading",
                config.name,
                recorded.trim(),
                config.sha256
            );
        }
        Err(_) => {} // no cache, or a pre-pin cache with no marker
    }

    // Step 2: Download
    log::info!("Downloading {} from {}...", config.name, config.download_url);

    if let Some(required) = config.required_disk_space {
        check_disk_space(assets_root, required)?;
    }

    let data = download_with_progress(&config.download_url, DOWNLOAD_TIMEOUT_SECS, on_progress)?;
    verify_sha256(&data, &config.sha256)?;
    log::info!("SHA-256 verified for {}", config.name);

    // Step 3: Atomic extraction (records the digest inside the new directory)
    extract_zip_to_asset_dir(&data, &asset_dir, &config.sha256)?;

    log::info!(
        "Successfully cached {} assets at {}",
        config.name,
        asset_dir.display()
    );

    Ok(asset_dir)
}

// =============================================================================
// Helpers
// =============================================================================

/// Returns the `~/.moss/assets/` directory, creating it if needed.
///
/// Separate from `~/.moss/bin/` to distinguish directory-based assets
/// from executable binaries. Do NOT change this to `~/.moss/theme/` —
/// theme/ is for user-facing design assets, assets/ is for cached
/// build tool downloads (JupyterLite, etc.).
pub fn get_moss_assets_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Cannot determine home directory".to_string())?;
    let assets_dir = home.join(".moss").join("assets");
    if !assets_dir.exists() {
        std::fs::create_dir_all(&assets_dir)
            .map_err(|e| format!("Failed to create assets directory: {}", e))?;
    }
    Ok(assets_dir)
}

/// Extracts a zip archive to an asset directory using atomic rename.
///
/// 1. Extracts to `<asset_dir>.tmp/` and writes the archive-digest marker there
/// 2. If `<asset_dir>` already exists (stale/corrupt/pre-pin), removes it
/// 3. Renames `.tmp` → final directory
///
/// This prevents corrupt cache states if the process is interrupted mid-extraction.
fn extract_zip_to_asset_dir(data: &[u8], asset_dir: &Path, sha256: &str) -> Result<(), String> {
    let tmp_dir = asset_dir.with_extension("tmp");

    // Clean up any previous failed extraction
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)
            .map_err(|e| format!("Failed to clean up temp directory: {}", e))?;
    }

    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp extraction directory: {}", e))?;

    // Extract zip
    let cursor = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Failed to open zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        let name = file.name().to_string();

        // Security: reject entries that would resolve outside the extraction directory.
        // Check for path traversal patterns in the raw name (../, absolute paths).
        // Path::join + starts_with alone is insufficient because Path::starts_with
        // does component-by-component matching without canonicalization.
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            log::warn!("Skipping zip entry with unsafe path: {}", name);
            continue;
        }

        let out_path = tmp_dir.join(&name);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed to create directory {}: {}", name, e))?;
        } else {
            // Ensure parent directory exists
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir for {}: {}", name, e))?;
            }

            let mut outfile = std::fs::File::create(&out_path)  // allow:raw_write fresh extraction into a temp dir that is renamed into place below
                .map_err(|e| format!("Failed to create file {}: {}", name, e))?;

            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to write file {}: {}", name, e))?;
        }
    }

    // The marker rides through the rename with the content it describes, so
    // an interrupted extraction can never leave a directory claiming a digest
    // it does not have.
    std::fs::write(tmp_dir.join(ARCHIVE_DIGEST_MARKER), sha256) // allow:raw_write same fresh temp dir as above
        .map_err(|e| format!("Failed to write archive digest marker: {}", e))?;

    // Atomic rename: remove old dir if present, then rename tmp → final
    if asset_dir.exists() {
        std::fs::remove_dir_all(asset_dir)
            .map_err(|e| format!("Failed to remove old asset directory: {}", e))?;
    }

    std::fs::rename(&tmp_dir, asset_dir)
        .map_err(|e| format!("Failed to rename temp directory to final: {}", e))?;

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

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

        let asset_dir = tmp.path().join("test-asset");
        extract_zip_to_asset_dir(&data, &asset_dir, &sha).unwrap();

        // Unreachable URL proves the cache hit never downloads.
        let config = AssetConfig {
            name: "test-asset".to_string(),
            download_url: "https://127.0.0.1:1/nonexistent.zip".to_string(),
            sha256: sha,
            required_disk_space: None,
        };
        let resolved = resolve_asset_directory_in(tmp.path(), &config, None).unwrap();
        assert_eq!(resolved, asset_dir);
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
        assert!(asset_dir.join("VERSION").exists(), "VERSION should exist");
        assert!(
            asset_dir.join("index.html").exists(),
            "index.html should exist"
        );
        assert_eq!(
            std::fs::read_to_string(asset_dir.join(ARCHIVE_DIGEST_MARKER)).unwrap(),
            sha,
            "extraction must record the archive digest it came from"
        );

        let html = std::fs::read_to_string(asset_dir.join("index.html")).unwrap();
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

        // Extract should replace
        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha256_hex(&buf));
        assert!(result.is_ok());

        // Old file gone, new file present
        assert!(
            !asset_dir.join("old.txt").exists(),
            "Old file should be removed"
        );
        assert!(
            asset_dir.join("new.txt").exists(),
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

        assert!(asset_dir.join("safe.txt").exists());
        // The evil file should NOT have been extracted outside
        assert!(
            !tmp.path().join("evil.txt").exists(),
            "Path traversal entry should be skipped"
        );
    }

    #[test]
    fn test_extract_zip_cleans_up_tmp_on_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let asset_dir = tmp.path().join("test-cleanup");
        let tmp_dir = asset_dir.with_extension("tmp");

        // Simulate a leftover .tmp from a previous failed extraction
        std::fs::create_dir_all(&tmp_dir).unwrap();
        std::fs::write(tmp_dir.join("leftover.txt"), "leftover").unwrap();

        let buf = zip_of(&[("VERSION", b"v1")]);
        let result = extract_zip_to_asset_dir(&buf, &asset_dir, &sha256_hex(&buf));
        assert!(result.is_ok());

        // tmp should be cleaned up, final dir should exist
        assert!(!tmp_dir.exists(), ".tmp dir should be cleaned up");
        assert!(asset_dir.join("VERSION").exists());
    }
}
