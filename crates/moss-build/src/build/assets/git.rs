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

/// The pinned dugite-native archive for each platform moss downloads git on:
/// the platform key, the archive's platform name and its SHA-256.
const ARCHIVES: &[(&str, &str, &str)] = &[
    ("darwin-arm64", "macOS-arm64", "7786cbf4874561b33a23faebb82bba0e8e9d1a26f1989722877fcc3d0376cebe"),
    ("darwin-x64", "macOS-x64", "d01649772ec5b997b01d9c605ac39ca7a782e4fb37584058c323043bb8803e53"),
    ("linux-x64", "ubuntu-x64", "3c7eed853f5d07a35c6e7e72735d473c08d8f0c414ab95b90f8bd9aeca3ad4f0"),
    ("windows-x64", "windows-x64", "6df0b7fbd3d28768356786719b9a81d1b071dbce2ed80705edd90c7151c4581f"),
];

// =============================================================================
// BinaryConfig for git
// =============================================================================

/// Returns a `BinaryConfig` for portable git (dugite-native).
///
/// This config is consumed by `binary_resolver::resolve_binary()` to locate and cache git. `download_tool(Tool::Git, ..)` fetches it after the user agrees. Sources are pinned to a specific dugite-native
/// release with SHA-256 checksums for reproducible builds.
pub fn git_binary_config() -> BinaryConfig {
    let sources = ARCHIVES
        .iter()
        .map(|(platform, asset, sha256)| {
            let url = format!(
                "https://github.com/desktop/dugite-native/releases/download/v{DUGITE_VERSION}/dugite-native-v{DUGITE_VERSION}-{DUGITE_COMMIT}-{asset}.tar.gz",
            );
            (platform.to_string(), BinarySource {
                github: None,
                direct_url: Some(url),
                sha256: Some(sha256.to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            })
        })
        .collect();

    BinaryConfig {
        name: "git".to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: vec!["--version".to_string()],
            pattern: Some(r"git version (\d+\.\d+\.\d+)".to_string()),
        }),
        sources,
        archive_layout: Some(ArchiveLayout {
            // The Windows build's `cmd/git.exe` is the launcher that puts its
            // own helpers on the PATH, as Git for Windows does.
            binary_path: if cfg!(windows) { "cmd/git.exe" } else { "bin/git" }.to_string(),
            // Marked executable after extraction, which only Unix needs.
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
        assert!(config.archive_layout.is_some());
        assert_eq!(config.cache_dir, Some("git-portable".to_string()));
        // Validate config consistency (sha256 + pinned URL)
        crate::build::assets::binary_resolver::validate_config(&config).unwrap();
    }

    #[test]
    fn git_downloads_on_every_platform_moss_ships() {
        let config = git_binary_config();
        for platform in ["darwin-arm64", "darwin-x64", "linux-x64", "windows-x64"] {
            let source = config.sources.get(platform).unwrap_or_else(|| panic!("no git download for {platform}"));
            assert_eq!(source.sha256.as_deref().map(str::len), Some(64), "{platform}");
        }
        let windows = config.sources["windows-x64"].direct_url.as_deref().unwrap();
        assert!(windows.ends_with("-windows-x64.tar.gz"), "{windows}");
    }

    #[cfg(windows)]
    #[test]
    fn the_downloaded_windows_git_is_the_cmd_launcher() {
        let layout = git_binary_config().archive_layout.unwrap();
        assert_eq!(layout.binary_path, "cmd/git.exe");
    }
}
