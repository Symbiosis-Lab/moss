//! Generic binary resolution pipeline.
//!
//! Provides a unified system for resolving external binaries (FFmpeg, Git, Hugo, etc.)
//! through a 4-step resolution chain:
//!
//! 1. Check a user-configured path
//! 2. Check the system PATH
//! 3. Check the `~/.moss/bin/` cache
//! 4. Download on demand (if `auto_download` is true)
//!
//! This module is the core of the unified binary resolution system that replaces
//! three separate resolution systems (FFmpeg, Git, Hugo) with one pipeline.
//!
//! # Design
//!
//! - Uses `ureq` (sync HTTP client) to avoid async runtime conflicts with Tauri.
//! - Reuses `download_with_progress()`, `verify_sha256()`, and `check_disk_space()`
//!   from the existing `download.rs` module.
//! - Supports GitHub Releases API and direct URL download sources.
//! - Atomic extraction: archives are extracted to a `.tmp` directory first, then
//!   renamed into place — preventing corrupt cache states.
//!
//! # Example
//!
//! ```rust,ignore
//! use moss::build::assets::binary_resolver::*;
//!
//! let config = BinaryConfig {
//!     name: "hugo".to_string(),
//!     binary_name: None,
//!     version_check: Some(VersionCheck {
//!         args: vec!["version".to_string()],
//!         pattern: Some(r"v(\d+\.\d+\.\d+)".to_string()),
//!     }),
//!     sources: HashMap::new(),
//!     archive_layout: None,
//!     cache_dir: None,
//!     required_disk_space: None,
//! };
//!
//! let resolution = resolve_binary(&config, None, true, None)?;
//! println!("Binary at: {} (source: {:?})", resolution.path, resolution.source);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::download::{check_disk_space, download_with_progress, verify_sha256, DownloadProgress};
use crate::build::media::ffmpeg::get_moss_bin_dir;

/// Download timeout in seconds (5 minutes). Matches the timeout used across
/// all binary download modules (ffmpeg.rs, git.rs).
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

// =============================================================================
// Types
// =============================================================================

/// Configuration for a binary that can be resolved, cached, and downloaded.
///
/// Each binary (hugo, ffmpeg, git, etc.) gets one `BinaryConfig` describing
/// how to find it, verify it, and download it for each supported platform.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BinaryConfig {
    /// Human-readable name, e.g. "hugo", "ffmpeg", "git".
    pub name: String,

    /// Filename of the binary if different from `name`.
    /// For example, a binary named "git" but distributed as "git.exe" on Windows.
    /// If `None`, `name` is used (with `.exe` appended on Windows).
    pub binary_name: Option<String>,

    /// How to verify the binary works and extract its version string.
    pub version_check: Option<VersionCheck>,

    /// Platform-specific download sources, keyed by platform string
    /// (e.g. "darwin-arm64", "darwin-x64", "linux-x64", "windows-x64").
    pub sources: HashMap<String, BinarySource>,

    /// For complex archives (like dugite-native) where the binary is nested
    /// inside the archive and multiple directories need executable permissions.
    pub archive_layout: Option<ArchiveLayout>,

    /// Subdirectory under `~/.moss/bin/` for caching.
    /// If `None`, the binary is placed directly in `~/.moss/bin/`.
    pub cache_dir: Option<String>,

    /// Minimum disk space required (in bytes) before attempting a download.
    /// Checked via `check_disk_space()` before downloading.
    pub required_disk_space: Option<u64>,
}

/// Platform-specific download source for a binary.
///
/// Either `github` or `direct_url` must be provided. If both are provided,
/// `direct_url` takes precedence (it avoids an API call).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BinarySource {
    /// Fetch a release asset via the GitHub Releases API.
    pub github: Option<GitHubSource>,

    /// A pinned URL to download from directly (no API call needed).
    pub direct_url: Option<String>,

    /// Expected SHA-256 checksum of the downloaded file (hex string).
    /// If provided, the download is verified before extraction.
    pub sha256: Option<String>,

    /// Archive format of the downloaded file.
    /// If `None`, the downloaded bytes are treated as a raw binary.
    pub archive_format: Option<ArchiveFormat>,
}

/// GitHub Releases download source.
///
/// Resolves a release asset URL by querying the GitHub Releases API.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GitHubSource {
    /// Repository owner (e.g. "gohugoio").
    pub owner: String,

    /// Repository name (e.g. "hugo").
    pub repo: String,

    /// Asset filename pattern with placeholders: `{version}`, `{os}`, `{arch}`.
    /// Example: `"hugo_extended_{version}_{os}-{arch}.tar.gz"`
    pub asset_pattern: String,

    /// Specific release tag (e.g. `"v0.123.0"`).
    /// If `None`, the "latest" release is fetched.
    pub tag: Option<String>,
}

/// Describes the internal layout of an archive for complex distributions.
///
/// Used when the binary is not at the archive root (e.g. dugite-native puts
/// git at `bin/git` inside the tarball).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ArchiveLayout {
    /// Path to the main binary inside the archive (e.g. "bin/git").
    pub binary_path: String,

    /// Directories where all files need `chmod +x` after extraction.
    /// Example: `["bin", "libexec/git-core"]` for dugite-native.
    pub executable_dirs: Vec<String>,
}

/// How to verify a binary works and extract its version.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct VersionCheck {
    /// Arguments to pass (e.g. `["--version"]`).
    pub args: Vec<String>,

    /// Regex pattern with one capture group to extract the version string.
    /// If `None`, the binary is considered valid if it exits successfully.
    pub pattern: Option<String>,
}

/// Archive format for downloaded binaries.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    /// gzip-compressed tar archive (.tar.gz).
    TarGz,
    /// Zip archive (.zip).
    Zip,
    /// Raw binary (no archive wrapper).
    Raw,
}

/// How the binary was found during resolution.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// Binary was found at a user-configured path.
    ConfiguredPath,
    /// Binary was found in the system PATH.
    SystemPath,
    /// Binary was found in the `~/.moss/bin/` cache.
    Cache,
    /// Binary was downloaded and cached.
    Downloaded,
}

/// The result of successfully resolving a binary.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BinaryResolution {
    /// Absolute path to the binary (or just the name if found in system PATH).
    pub path: String,

    /// Version string extracted from the binary output, if available.
    pub version: Option<String>,

    /// How the binary was found.
    pub source: ResolutionSource,
}

// =============================================================================
// Main resolution function
// =============================================================================

/// Resolves a binary through a 4-step resolution chain.
///
/// 1. Check `configured_path` (if provided) — validates the binary works
/// 2. Check system PATH — looks up by binary name
/// 3. Check `~/.moss/bin/` cache — looks for a previously downloaded copy
/// 4. Download (if `auto_download` is true) — fetches from configured source
///
/// If a cached binary exists but fails the version check, it is deleted and
/// the resolver falls through to download. This handles corrupt cache recovery.
///
/// # Arguments
///
/// * `config` - Binary configuration describing name, sources, and verification.
/// * `configured_path` - Optional user-specified path to check first.
/// * `auto_download` - Whether to download the binary if not found locally.
/// * `on_progress` - Optional progress callback for downloads.
///
/// # Returns
///
/// * `Ok(BinaryResolution)` - Binary found and verified.
/// * `Err(String)` - Binary not found or all resolution steps failed.
pub fn resolve_binary(
    config: &BinaryConfig,
    configured_path: Option<&str>,
    auto_download: bool,
    on_progress: Option<&DownloadProgress>,
) -> Result<BinaryResolution, String> {
    validate_config(config)?;
    let binary_filename = get_binary_filename(config);

    // Step 1: Check configured path
    if let Some(path) = configured_path {
        if Path::new(path).exists() {
            match check_binary_works(path, config) {
                Ok(version) => {
                    return Ok(BinaryResolution {
                        path: path.to_string(),
                        version,
                        source: ResolutionSource::ConfiguredPath,
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Configured path '{}' for {} failed validation: {}",
                        path,
                        config.name,
                        e
                    );
                }
            }
        } else {
            log::warn!(
                "Configured path '{}' for {} does not exist",
                path,
                config.name
            );
        }
    }

    // Step 2: Check system PATH
    match check_binary_works(&binary_filename, config) {
        Ok(version) => {
            return Ok(BinaryResolution {
                path: binary_filename.clone(),
                version,
                source: ResolutionSource::SystemPath,
            });
        }
        Err(_) => {
            log::debug!("{} not found in system PATH", config.name);
        }
    }

    // Step 3: Check ~/.moss/bin/ cache
    let cached_path = get_cached_binary_path(config)?;
    if cached_path.exists() {
        let cached_str = cached_path.to_string_lossy().to_string();
        match check_binary_works(&cached_str, config) {
            Ok(version) => {
                return Ok(BinaryResolution {
                    path: cached_str,
                    version,
                    source: ResolutionSource::Cache,
                });
            }
            Err(e) => {
                // Corrupt cache: delete and fall through to download
                log::warn!(
                    "Cached {} at '{}' failed validation ({}), removing corrupt cache",
                    config.name,
                    cached_str,
                    e
                );
                delete_cached_binary(config, &cached_path);
            }
        }
    }

    // Step 4: Download if auto_download is enabled
    if !auto_download {
        return Err(format!(
            "{} not found (checked: configured path, system PATH, cache). \
             Auto-download is disabled.",
            config.name
        ));
    }

    let resolution = download_and_extract(config, on_progress)?;
    Ok(resolution)
}

// =============================================================================
// Config validation
// =============================================================================

/// Validates a `BinaryConfig` for logical consistency.
///
/// Enforces the checksum policy: if `sha256` is provided, the download URL
/// must be deterministic — either a `direct_url` or a pinned `github.tag`.
/// Using `sha256` with GitHub "latest" releases is rejected because the URL
/// changes on each release.
///
/// # Returns
///
/// * `Ok(())` - Config is valid.
/// * `Err(String)` - Config has logical inconsistencies.
pub fn validate_config(config: &BinaryConfig) -> Result<(), String> {
    for (platform, source) in &config.sources {
        if source.sha256.is_some() {
            // sha256 requires a pinned URL: either direct_url or github with a tag
            let has_direct_url = source.direct_url.is_some();
            let has_pinned_github = source
                .github
                .as_ref()
                .is_some_and(|gh| gh.tag.is_some());

            if !has_direct_url && !has_pinned_github {
                return Err(format!(
                    "Config error for {} (platform '{}'): sha256 checksum requires a \
                     pinned download URL. Provide either 'direct_url' or 'github.tag'. \
                     Cannot use sha256 with GitHub 'latest' releases (the URL changes).",
                    config.name, platform
                ));
            }
        }
    }

    Ok(())
}

// =============================================================================
// Download and extraction
// =============================================================================

/// Downloads a binary for the current platform and extracts it to the cache.
///
/// This is step 4 of the resolution chain. It:
/// 1. Detects the current platform
/// 2. Looks up the download source for this platform
/// 3. Checks available disk space
/// 4. Resolves the download URL (direct or via GitHub API)
/// 5. Downloads with progress reporting
/// 6. Verifies SHA-256 checksum (if configured)
/// 7. Extracts based on archive format (tar.gz, zip, or raw)
/// 8. Sets executable permissions
///
/// # Returns
///
/// * `Ok(BinaryResolution)` - Binary downloaded, verified, and cached.
/// * `Err(String)` - Any step in the pipeline failed.
fn download_and_extract(
    config: &BinaryConfig,
    on_progress: Option<&DownloadProgress>,
) -> Result<BinaryResolution, String> {
    let platform = get_current_platform()?;

    let source = config.sources.get(&platform).ok_or_else(|| {
        format!(
            "No download source configured for {} on platform '{}'",
            config.name, platform
        )
    })?;

    // Check disk space
    let bin_dir = get_moss_bin_dir()?;
    if let Some(required) = config.required_disk_space {
        check_disk_space(&bin_dir, required)?;
    }

    // Resolve the download URL
    let (url, version) = resolve_download_url(source, &platform)?;

    log::info!("Downloading {} from {}...", config.name, url);

    // Download
    let data = download_with_progress(&url, DOWNLOAD_TIMEOUT_SECS, on_progress)?;

    // Verify SHA-256 if configured
    if let Some(ref expected_sha256) = source.sha256 {
        verify_sha256(&data, expected_sha256)?;
        log::info!("SHA-256 verified for {}", config.name);
    }

    // Determine cache destination
    let cached_path = get_cached_binary_path(config)?;
    let cache_parent = cached_path
        .parent()
        .ok_or_else(|| "Cannot determine cache parent directory".to_string())?;

    // Extract based on archive format
    match source.archive_format {
        Some(ArchiveFormat::TarGz) => {
            extract_tar_gz_to_cache(&data, config, cache_parent)?;
        }
        Some(ArchiveFormat::Zip) => {
            extract_zip_to_cache(&data, config, cache_parent)?;
        }
        Some(ArchiveFormat::Raw) | None => {
            // Raw binary — write directly
            std::fs::create_dir_all(cache_parent).map_err(|e| {
                format!(
                    "Failed to create cache directory {}: {}",
                    cache_parent.display(),
                    e
                )
            })?;
            std::fs::write(&cached_path, &data).map_err(|e| {  // allow:raw_write downloaded binary under the binaries cache, not the site output tree
                format!(
                    "Failed to write binary to {}: {}",
                    cached_path.display(),
                    e
                )
            })?;

            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&cached_path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| {
                        format!(
                            "Failed to set executable permissions on {}: {}",
                            cached_path.display(),
                            e
                        )
                    })?;
            }
        }
    }

    // Verify the extracted binary works
    let cached_str = cached_path.to_string_lossy().to_string();
    let extracted_version = match check_binary_works(&cached_str, config) {
        Ok(v) => v,
        Err(e) => {
            // Clean up on failure
            delete_cached_binary(config, &cached_path);
            return Err(format!(
                "Downloaded {} failed verification: {}",
                config.name, e
            ));
        }
    };

    // Use extracted version if available, fall back to URL-derived version
    let final_version = extracted_version.or(version);

    log::info!(
        "{} installed to {} (version: {})",
        config.name,
        cached_str,
        final_version.as_deref().unwrap_or("unknown")
    );

    Ok(BinaryResolution {
        path: cached_str,
        version: final_version,
        source: ResolutionSource::Downloaded,
    })
}

// =============================================================================
// URL resolution
// =============================================================================

/// Resolves the download URL for a binary source.
///
/// For `direct_url`: returns the URL as-is.
/// For `github`: calls the GitHub Releases API to find the asset URL,
/// resolving placeholders in the `asset_pattern`.
///
/// # Returns
///
/// * `Ok((url, version))` - Download URL and optional version string.
/// * `Err(String)` - URL resolution failed.
fn resolve_download_url(
    source: &BinarySource,
    platform: &str,
) -> Result<(String, Option<String>), String> {
    // Prefer direct_url if available
    if let Some(ref url) = source.direct_url {
        return Ok((url.clone(), None));
    }

    // Fall back to GitHub API
    let github = source.github.as_ref().ok_or_else(|| {
        "BinarySource has neither direct_url nor github configured".to_string()
    })?;

    let api_url = match &github.tag {
        Some(tag) => format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            github.owner, github.repo, tag
        ),
        None => format!(
            "https://api.github.com/repos/{}/{}/releases/latest",
            github.owner, github.repo
        ),
    };

    let response = ureq::get(&api_url)
        .set("User-Agent", "moss")
        .set("Accept", "application/vnd.github.v3+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("GitHub API request failed for {}: {}", api_url, e))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Failed to parse GitHub API response: {}", e))?;

    // Extract tag name (version)
    let tag_name = json["tag_name"]
        .as_str()
        .ok_or_else(|| "GitHub API response missing 'tag_name'".to_string())?;

    // Strip leading 'v' for the version string
    let version = tag_name.strip_prefix('v').unwrap_or(tag_name).to_string();

    // Resolve asset pattern placeholders
    let (os, arch) = parse_platform(platform)?;
    let asset_name = github
        .asset_pattern
        .replace("{version}", &version)
        .replace("{os}", os)
        .replace("{arch}", arch);

    // Find the matching asset
    let assets = json["assets"]
        .as_array()
        .ok_or_else(|| "GitHub API response missing 'assets' array".to_string())?;

    let asset_url = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|name| name == asset_name)
        })
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| {
            let available: Vec<String> = assets
                .iter()
                .filter_map(|a| a["name"].as_str().map(String::from))
                .collect();
            format!(
                "Asset '{}' not found in release {}. Available assets: {:?}",
                asset_name, tag_name, available
            )
        })?;

    Ok((asset_url.to_string(), Some(version)))
}

/// Constructs a resolved asset name from a GitHub source pattern and version info.
///
/// This is the pure logic extracted for testability — no network calls.
///
/// # Arguments
///
/// * `pattern` - Asset pattern with `{version}`, `{os}`, `{arch}` placeholders.
/// * `version` - Version string (without leading 'v').
/// * `os` - Operating system string (e.g. "darwin", "linux", "windows").
/// * `arch` - Architecture string (e.g. "arm64", "x64").
#[allow(dead_code)]
pub fn resolve_asset_pattern(pattern: &str, version: &str, os: &str, arch: &str) -> String {
    pattern
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
}

// =============================================================================
// Binary checking
// =============================================================================

/// Checks if a binary works by running it with version check args.
///
/// Runs `Command::new(path).args(version_check.args)` and optionally parses
/// the version from stdout/stderr using the configured regex pattern.
///
/// If no `version_check` is configured, the binary is only checked for existence.
///
/// # Returns
///
/// * `Ok(Some(version))` - Binary works and version was extracted.
/// * `Ok(None)` - Binary works but no version pattern configured.
/// * `Err(String)` - Binary failed to execute or exited with error.
pub fn check_binary_works(
    path: &str,
    config: &BinaryConfig,
) -> Result<Option<String>, String> {
    let version_check = match &config.version_check {
        Some(vc) => vc,
        None => {
            // No version check configured — just verify the binary exists and is runnable.
            // For absolute paths, check file existence. For bare names (system PATH),
            // try to execute with --help to verify it's findable.
            let path_obj = Path::new(path);
            if path_obj.is_absolute() {
                if path_obj.exists() {
                    return Ok(None);
                }
                return Err(format!("{} does not exist", path));
            }
            // Bare name — try to run it to verify it's in PATH
            let output = Command::new(path)
                .arg("--help")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("Failed to run {}: {}", path, e))?;
            // Accept any exit code — some binaries return non-zero for --help.
            // The fact that it ran at all means it exists in PATH.
            let _ = output.status;
            return Ok(None);
        }
    };

    let output = Command::new(path)
        .args(&version_check.args)
        .output()
        .map_err(|e| format!("Failed to run {} {:?}: {}", path, version_check.args, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} {:?} exited with non-zero status: {}",
            path,
            version_check.args,
            stderr.trim()
        ));
    }

    // Parse version from output
    let version = if let Some(ref pattern) = version_check.pattern {
        let combined_output = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let re = regex::Regex::new(pattern)
            .map_err(|e| format!("Invalid version regex '{}': {}", pattern, e))?;

        re.captures(&combined_output)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    } else {
        None
    };

    Ok(version)
}

// =============================================================================
// Cache path helpers
// =============================================================================

/// Computes the path where a binary would be cached under `~/.moss/bin/`.
///
/// If `config.cache_dir` is set (e.g. "git-portable"), the binary is under
/// `~/.moss/bin/{cache_dir}/{archive_layout.binary_path}`.
///
/// If `config.cache_dir` is `None`, the binary is directly at
/// `~/.moss/bin/{binary_filename}`.
///
/// # Examples
///
/// - FFmpeg (flat): `~/.moss/bin/ffmpeg`
/// - Git (nested): `~/.moss/bin/git-portable/bin/git`
pub fn get_cached_binary_path(config: &BinaryConfig) -> Result<PathBuf, String> {
    let bin_dir = get_moss_bin_dir()?;
    let binary_filename = get_binary_filename(config);

    match (&config.cache_dir, &config.archive_layout) {
        // Nested layout: cache_dir + archive binary_path
        (Some(cache_dir), Some(layout)) => Ok(bin_dir.join(cache_dir).join(&layout.binary_path)),
        // Cache dir but no layout: cache_dir + binary filename
        (Some(cache_dir), None) => Ok(bin_dir.join(cache_dir).join(&binary_filename)),
        // No cache dir: flat binary in bin/
        (None, _) => Ok(bin_dir.join(&binary_filename)),
    }
}

/// Returns the binary filename, accounting for `binary_name` override and Windows `.exe`.
pub fn get_binary_filename(config: &BinaryConfig) -> String {
    let base = config
        .binary_name
        .as_deref()
        .unwrap_or(&config.name);

    if cfg!(target_os = "windows") && !base.ends_with(".exe") {
        format!("{}.exe", base)
    } else {
        base.to_string()
    }
}

/// Deletes a cached binary (and its parent cache_dir if applicable).
fn delete_cached_binary(config: &BinaryConfig, cached_path: &Path) {
    if let Some(ref cache_dir) = config.cache_dir {
        // Delete the entire cache directory (e.g. git-portable/)
        if let Ok(bin_dir) = get_moss_bin_dir() {
            let dir = bin_dir.join(cache_dir);
            if dir.exists() {
                let _ = std::fs::remove_dir_all(&dir);
                log::info!("Removed corrupt cache directory: {}", dir.display());
            }
        }
    } else if cached_path.exists() {
        let _ = std::fs::remove_file(cached_path);
        log::info!("Removed corrupt cached binary: {}", cached_path.display());
    }
}

// =============================================================================
// Platform detection
// =============================================================================

/// Returns the current platform string (e.g. "darwin-arm64", "linux-x64").
///
/// Maps Rust's `std::env::consts` values to the platform naming convention
/// used in `BinaryConfig.sources` keys.
///
/// # Platform mapping
///
/// | Rust OS      | Rust ARCH   | Platform string  |
/// |--------------|-------------|------------------|
/// | `macos`      | `aarch64`   | `darwin-arm64`   |
/// | `macos`      | `x86_64`    | `darwin-x64`     |
/// | `linux`      | `x86_64`    | `linux-x64`      |
/// | `linux`      | `aarch64`   | `linux-arm64`    |
/// | `windows`    | `x86_64`    | `windows-x64`    |
/// | `windows`    | `aarch64`   | `windows-arm64`  |
pub fn get_current_platform() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => return Err(format!("Unsupported operating system: {}", other)),
    };

    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => return Err(format!("Unsupported architecture: {}", other)),
    };

    Ok(format!("{}-{}", os, arch))
}

/// Parses a platform string like "darwin-arm64" into ("darwin", "arm64").
fn parse_platform(platform: &str) -> Result<(&str, &str), String> {
    let parts: Vec<&str> = platform.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid platform string: '{}'", platform));
    }
    Ok((parts[0], parts[1]))
}

// =============================================================================
// Archive extraction
// =============================================================================

/// Extracts a `.tar.gz` archive to the cache using atomic extraction.
///
/// Extraction is performed into a temporary `.tmp` directory, then atomically
/// renamed into place. This prevents corrupt cache states if extraction is
/// interrupted (e.g. by a crash or Ctrl+C).
///
/// After extraction, executable permissions are set on files in directories
/// specified by `config.archive_layout.executable_dirs`.
fn extract_tar_gz_to_cache(
    data: &[u8],
    config: &BinaryConfig,
    cache_parent: &Path,
) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    // Determine the final directory
    let final_dir = match &config.cache_dir {
        Some(cache_dir) => {
            let bin_dir = get_moss_bin_dir()?;
            bin_dir.join(cache_dir)
        }
        None => cache_parent.to_path_buf(),
    };

    // Append ".tmp" to the directory name. We avoid `with_extension("tmp")` because
    // it replaces existing extensions (e.g. "some.dir" → "some.tmp" instead of "some.dir.tmp").
    let tmp_dir = final_dir.with_file_name(format!(
        "{}.tmp",
        final_dir.file_name().unwrap_or_default().to_string_lossy()
    ));

    // Clean up any previous failed extraction
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)
            .map_err(|e| format!("Failed to clean up temp dir {}: {}", tmp_dir.display(), e))?;
    }

    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir {}: {}", tmp_dir.display(), e))?;

    // Extract tar.gz
    //
    // SAFETY (path traversal): `tar::Archive::unpack` can follow symlinks and write outside
    // `tmp_dir` if the archive contains crafted path entries. Acceptable here because archives
    // are (a) fetched from a pinned URL, verified against a hardcoded SHA-256 — NEVER a
    // plugin-declared one; those extract through the hardened `system::stack_exec::extract`
    // instead (S4, ADR-080 item 5) — or (b) fetched from GitHub Releases over HTTPS (trusted,
    // signed). The atomic rename ensures partial extraction never becomes the final state.
    let decoder = GzDecoder::new(data);
    let mut archive = Archive::new(decoder);

    archive
        .unpack(&tmp_dir)
        .map_err(|e| format!("Failed to extract tar.gz archive for {}: {}", config.name, e))?;

    // Set executable permissions
    #[cfg(unix)]
    if let Some(ref layout) = config.archive_layout {
        set_executable_permissions(&tmp_dir, &layout.executable_dirs)?;
    }

    // Atomic rename: remove old, rename tmp to final
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir).map_err(|e| {
            format!(
                "Failed to remove old cache dir {}: {}",
                final_dir.display(),
                e
            )
        })?;
    }

    // Ensure parent directory exists
    if let Some(parent) = final_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create parent directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }

    std::fs::rename(&tmp_dir, &final_dir).map_err(|e| {
        format!(
            "Failed to rename {} -> {}: {}",
            tmp_dir.display(),
            final_dir.display(),
            e
        )
    })?;

    Ok(())
}

/// Extracts a binary from a zip archive to the cache.
///
/// Searches all entries in the zip for a file matching the binary filename
/// and extracts it to the cache location. This pattern handles both flat
/// zip files and those with subdirectory structures.
fn extract_zip_to_cache(
    data: &[u8],
    config: &BinaryConfig,
    cache_parent: &Path,
) -> Result<(), String> {
    use std::io::Cursor;

    let binary_filename = get_binary_filename(config);
    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("Failed to open zip archive for {}: {}", config.name, e))?;

    std::fs::create_dir_all(cache_parent).map_err(|e| {
        format!(
            "Failed to create cache directory {}: {}",
            cache_parent.display(),
            e
        )
    })?;

    let dest_path = cache_parent.join(&binary_filename);

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        let name = file.name().to_string();

        // Look for the binary by exact filename (might be in a subdirectory).
        // Use exact match after path splitting to avoid false positives
        // (e.g. "malicious_testbin" matching "testbin").
        let entry_filename = name.rsplit('/').next().unwrap_or(&name);
        if entry_filename == binary_filename {
            let mut dest_file = std::fs::File::create(&dest_path).map_err(|e| {  // allow:raw_write zip extraction into the binaries cache, not the site output tree
                format!(
                    "Failed to create file {}: {}",
                    dest_path.display(),
                    e
                )
            })?;

            std::io::copy(&mut file, &mut dest_file).map_err(|e| {
                format!(
                    "Failed to extract {} from zip: {}",
                    binary_filename, e
                )
            })?;

            // Make executable on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| {
                        format!(
                            "Failed to set executable permissions on {}: {}",
                            dest_path.display(),
                            e
                        )
                    })?;
            }

            return Ok(());
        }
    }

    Err(format!(
        "{} binary ('{}') not found in zip archive",
        config.name, binary_filename
    ))
}

/// Sets executable permissions (0o755) on all files in the specified directories.
///
/// This mirrors the pattern from `git.rs` where dugite-native archives need
/// `chmod +x` on `bin/` and `libexec/git-core/` directories.
#[cfg(unix)]
fn set_executable_permissions(
    base_dir: &Path,
    executable_dirs: &[String],
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    for dir_name in executable_dirs {
        let dir = base_dir.join(dir_name);
        if !dir.exists() {
            continue;
        }

        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();

            if path.is_file() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| {
                        format!(
                            "Failed to set executable permissions on {}: {}",
                            path.display(),
                            e
                        )
                    })?;
            }
        }
    }

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[path = "binary_resolver_tests.rs"]
mod tests;
