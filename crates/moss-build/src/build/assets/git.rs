//! Git binary management: detection and download-on-demand.
//!
//! This module provides a thin wrapper around the unified [`binary_resolver`]
//! pipeline for locating or downloading a portable git binary. It defines the
//! dugite-native `BinaryConfig` and exposes `GitManager` as a convenience API.
//!
//! Resolution order (handled by `binary_resolver::resolve_binary`):
//!
//! 1. Check system PATH for `git`
//! 2. Check `~/.moss/bin/git-portable/bin/git` cache
//! 3. Download dugite-native pre-built git from GitHub Releases
//!
//! # Usage
//!
//! ```rust,ignore
//! use moss::build::assets::git::GitManager;
//!
//! // Quick: use system git or download if needed
//! let git = GitManager::get_or_download(None)?;
//! println!("Using git at: {}", git.bin_path);
//! ```

use std::collections::HashMap;
use std::process::Command;

use super::binary_resolver::{
    ArchiveFormat, ArchiveLayout, BinaryConfig, BinarySource, VersionCheck,
};
use super::download::DownloadProgress;

// =============================================================================
// Constants — dugite-native release
// =============================================================================

/// dugite-native release version.
const DUGITE_VERSION: &str = "2.53.0";

/// Short commit hash for the dugite-native release.
const DUGITE_COMMIT: &str = "6981a1f";

/// SHA-256 checksum for macOS arm64 archive.
const CHECKSUM_ARM64: &str = "7786cbf4874561b33a23faebb82bba0e8e9d1a26f1989722877fcc3d0376cebe";

/// SHA-256 checksum for macOS x64 archive.
const CHECKSUM_X64: &str = "d01649772ec5b997b01d9c605ac39ca7a782e4fb37584058c323043bb8803e53";

// =============================================================================
// BinaryConfig for git
// =============================================================================

/// Returns a `BinaryConfig` for portable git (dugite-native).
///
/// This config is consumed by `binary_resolver::resolve_binary()` to locate,
/// cache, and download git. Sources are pinned to a specific dugite-native
/// release with SHA-256 checksums for reproducible builds.
pub fn git_binary_config() -> BinaryConfig {
    let arm64_url = format!(
        "https://github.com/desktop/dugite-native/releases/download/v{}/dugite-native-v{}-{}-macOS-arm64.tar.gz",
        DUGITE_VERSION, DUGITE_VERSION, DUGITE_COMMIT,
    );
    let x64_url = format!(
        "https://github.com/desktop/dugite-native/releases/download/v{}/dugite-native-v{}-{}-macOS-x64.tar.gz",
        DUGITE_VERSION, DUGITE_VERSION, DUGITE_COMMIT,
    );

    BinaryConfig {
        name: "git".to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: vec!["--version".to_string()],
            pattern: Some(r"git version (\d+\.\d+\.\d+)".to_string()),
        }),
        sources: HashMap::from([
            ("darwin-arm64".to_string(), BinarySource {
                github: None,
                direct_url: Some(arm64_url),
                sha256: Some(CHECKSUM_ARM64.to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            }),
            ("darwin-x64".to_string(), BinarySource {
                github: None,
                direct_url: Some(x64_url),
                sha256: Some(CHECKSUM_X64.to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            }),
        ]),
        archive_layout: Some(ArchiveLayout {
            binary_path: "bin/git".to_string(),
            executable_dirs: vec!["bin".to_string(), "libexec/git-core".to_string()],
        }),
        cache_dir: Some("git-portable".to_string()),
        required_disk_space: Some(200 * 1024 * 1024),
    }
}

// =============================================================================
// GitManager
// =============================================================================

/// Manages a git binary — either system-installed or downloaded portable.
#[allow(dead_code)]
pub struct GitManager {
    /// Path to the git binary (either `"git"` for system, or absolute path).
    pub bin_path: String,
}

#[allow(dead_code)]
impl GitManager {
    /// Detects git in the system PATH.
    ///
    /// Runs `git --version` to verify git is available and functional.
    ///
    /// # Returns
    /// * `Ok(GitManager)` - git found in PATH
    /// * `Err(String)` - git not found or not functional
    pub fn detect() -> Result<Self, String> {
        let output = Command::new("git")
            .arg("--version")
            .output()
            .map_err(|e| format!("git not found: {}", e))?;

        if output.status.success() {
            Ok(Self {
                bin_path: "git".to_string(),
            })
        } else {
            Err("git --version failed".to_string())
        }
    }

    /// Gets git from system PATH, previously downloaded portable, or downloads it.
    ///
    /// Delegates to `binary_resolver::resolve_binary()` with the dugite-native
    /// `BinaryConfig`. Resolution order:
    /// 1. System git (in PATH)
    /// 2. Previously downloaded portable git (`~/.moss/bin/git-portable/bin/git`)
    /// 3. Download dugite-native portable git
    ///
    /// # Arguments
    /// * `on_progress` - Optional progress callback for the download step.
    ///
    /// # Returns
    /// * `Ok(GitManager)` - Ready to use git
    /// * `Err(String)` - git unavailable and download failed
    pub fn get_or_download(
        on_progress: Option<&DownloadProgress>,
    ) -> Result<Self, String> {
        let config = git_binary_config();
        let resolution =
            super::binary_resolver::resolve_binary(&config, None, true, on_progress)?;
        Ok(Self {
            bin_path: resolution.path,
        })
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // detect() tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_git_detect_returns_result() {
        // detect() should not panic. On most dev machines git is installed,
        // but we only assert it returns a Result (not that it's Ok).
        let result = GitManager::detect();
        // If git is on the system, it should succeed
        if result.is_ok() {
            assert_eq!(result.unwrap().bin_path, "git");
        }
        // If git is not installed, it should return Err (not panic)
    }

    // -------------------------------------------------------------------------
    // get_or_download() tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_git_get_or_download() {
        // On a dev machine with git installed, this should succeed via detect().
        // This test exercises the full resolution chain — at minimum step 1 (system git).
        #[cfg(windows)]
        if crate::running_under_wine() {
            eprintln!("SKIP test_git_get_or_download: Wine ships no git.exe (real Windows CI has one)");
            return;
        }
        let result = GitManager::get_or_download(None);
        assert!(
            result.is_ok(),
            "get_or_download should succeed on a dev machine: {:?}",
            result.err()
        );
        let manager = result.unwrap();
        // The bin_path should be non-empty
        assert!(
            !manager.bin_path.is_empty(),
            "bin_path should be non-empty"
        );
    }

    // -------------------------------------------------------------------------
    // git_binary_config() tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_git_config_shape() {
        let config = git_binary_config();
        assert_eq!(config.name, "git");
        assert!(config.version_check.is_some());
        assert!(config.sources.contains_key("darwin-arm64"));
        assert!(config.sources.contains_key("darwin-x64"));
        assert!(config.archive_layout.is_some());
        assert_eq!(config.cache_dir, Some("git-portable".to_string()));
        // Validate config consistency (sha256 + pinned URL)
        crate::build::assets::binary_resolver::validate_config(&config).unwrap();
    }

    #[test]
    fn test_git_checksums_match_constants() {
        let config = git_binary_config();
        let arm64_source = config.sources.get("darwin-arm64").unwrap();
        let x64_source = config.sources.get("darwin-x64").unwrap();
        assert_eq!(arm64_source.sha256.as_deref(), Some(CHECKSUM_ARM64));
        assert_eq!(x64_source.sha256.as_deref(), Some(CHECKSUM_X64));
    }
}
