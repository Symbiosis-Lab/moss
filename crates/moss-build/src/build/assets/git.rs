//! Git binary configuration.
//!
//! Defines the dugite-native `BinaryConfig` for a portable git. Finding git
//! and downloading it are handled by [`binary_resolver`]: `resolve_binary`
//! looks for an installed git, and `download_tool(Tool::Git, ..)` fetches the
//! dugite-native build from GitHub Releases when the user asks for it.

use std::collections::HashMap;

use super::binary_resolver::{
    ArchiveFormat, ArchiveLayout, BinaryConfig, BinarySource, VersionCheck,
};

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
/// This config is consumed by `binary_resolver::resolve_binary()` to locate and cache git. `download_tool(Tool::Git, ..)` fetches it after the user agrees. Sources are pinned to a specific dugite-native
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
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
