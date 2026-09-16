use super::*;

/// Helper: create a minimal BinaryConfig for testing.
fn test_config(name: &str) -> BinaryConfig {
    BinaryConfig {
        name: name.to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::new(),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    }
}

/// Helper: create a BinaryConfig with a version check.
fn test_config_with_version_check(
    name: &str,
    args: Vec<&str>,
    pattern: Option<&str>,
) -> BinaryConfig {
    BinaryConfig {
        name: name.to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: args.into_iter().map(String::from).collect(),
            pattern: pattern.map(String::from),
        }),
        sources: HashMap::new(),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    }
}

// -------------------------------------------------------------------------
// resolve_binary tests
// -------------------------------------------------------------------------

#[test]
fn test_resolve_system_path() {
    // /bin/echo is universally available on Unix; on Windows echo is a cmd
    // builtin with no .exe, so probe for cmd itself instead.
    let name = if cfg!(target_os = "windows") { "cmd" } else { "echo" };
    let config = test_config(name);
    let result = resolve_binary(&config, None, false, None);
    assert!(
        result.is_ok(),
        "{name} should be found in system PATH: {:?}",
        result.err()
    );
    let resolution = result.unwrap();
    // Windows appends `.exe` to the resolved binary filename.
    let expected_path = if cfg!(target_os = "windows") {
        "cmd.exe"
    } else {
        "echo"
    };
    assert_eq!(resolution.path, expected_path);
    assert!(
        matches!(resolution.source, ResolutionSource::SystemPath),
        "Should be resolved from system PATH, got: {:?}",
        resolution.source
    );
}

#[test]
fn test_resolve_configured_path() {
    // A configured path that exists on the host: `/bin/echo` on Unix,
    // `cmd.exe` on Windows.
    #[cfg(target_os = "windows")]
    let configured = "C:\\Windows\\System32\\cmd.exe";
    #[cfg(not(target_os = "windows"))]
    let configured = "/bin/echo";

    let config = test_config("echo");
    let result = resolve_binary(&config, Some(configured), false, None);
    assert!(
        result.is_ok(),
        "Configured {} should work: {:?}",
        configured,
        result.err()
    );
    let resolution = result.unwrap();
    assert_eq!(resolution.path, configured);
    assert!(
        matches!(resolution.source, ResolutionSource::ConfiguredPath),
        "Should be resolved from configured path, got: {:?}",
        resolution.source
    );
}

#[test]
fn test_resolve_no_download() {
    // A nonexistent binary with auto_download=false should fail.
    let config = test_config("definitely_not_a_real_binary_xyz123");
    let result = resolve_binary(&config, None, false, None);
    assert!(result.is_err(), "Should fail for nonexistent binary");
    let err = result.unwrap_err();
    assert!(
        err.contains("not found"),
        "Error should mention 'not found', got: {}",
        err
    );
    assert!(
        err.contains("Auto-download is disabled"),
        "Error should mention auto-download disabled, got: {}",
        err
    );
}

// -------------------------------------------------------------------------
// check_binary_works tests
// -------------------------------------------------------------------------

#[test]
fn test_check_binary_works_with_version() {
    // Run a host binary that echoes its args and capture "hello".
    // Unix: `/bin/echo hello world`. Windows: `cmd.exe /c echo hello world`.
    #[cfg(target_os = "windows")]
    let (bin, args) = (
        "C:\\Windows\\System32\\cmd.exe",
        vec!["/c", "echo", "hello", "world"],
    );
    #[cfg(not(target_os = "windows"))]
    let (bin, args) = ("/bin/echo", vec!["hello", "world"]);

    let config = test_config_with_version_check("echo", args, Some(r"(hello)"));
    let result = check_binary_works(bin, &config);
    assert!(
        result.is_ok(),
        "check_binary_works should succeed for {}: {:?}",
        bin,
        result.err()
    );
    let version = result.unwrap();
    assert_eq!(
        version,
        Some("hello".to_string()),
        "Should capture 'hello' from echo output"
    );
}

#[test]
fn test_check_binary_works_no_pattern() {
    #[cfg(target_os = "windows")]
    let (bin, args) = ("C:\\Windows\\System32\\cmd.exe", vec!["/c", "echo", "test"]);
    #[cfg(not(target_os = "windows"))]
    let (bin, args) = ("/bin/echo", vec!["test"]);

    let config = test_config_with_version_check("echo", args, None);
    let result = check_binary_works(bin, &config);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        None,
        "No pattern means no version extracted"
    );
}

#[test]
fn test_check_binary_works_nonexistent() {
    let config = test_config("not_a_binary_xyz");
    let result = check_binary_works("/nonexistent/path/not_a_binary_xyz", &config);
    assert!(result.is_err(), "Should fail for nonexistent binary");
}

// -------------------------------------------------------------------------
// Platform detection tests
// -------------------------------------------------------------------------

#[test]
fn test_platform_detection() {
    let platform = get_current_platform();
    assert!(
        platform.is_ok(),
        "Platform detection should succeed: {:?}",
        platform.err()
    );
    let p = platform.unwrap();
    // Must be in the form "os-arch"
    assert!(
        p.contains('-'),
        "Platform should be in 'os-arch' format, got: {}",
        p
    );

    let parts: Vec<&str> = p.split('-').collect();
    assert_eq!(parts.len(), 2, "Platform should have exactly 2 parts");

    // OS should be one of the supported values
    assert!(
        ["darwin", "linux", "windows"].contains(&parts[0]),
        "OS should be darwin/linux/windows, got: {}",
        parts[0]
    );

    // Arch should be one of the supported values
    assert!(
        ["arm64", "x64"].contains(&parts[1]),
        "Arch should be arm64/x64, got: {}",
        parts[1]
    );
}

#[test]
fn test_parse_platform() {
    let (os, arch) = parse_platform("darwin-arm64").unwrap();
    assert_eq!(os, "darwin");
    assert_eq!(arch, "arm64");

    let (os, arch) = parse_platform("linux-x64").unwrap();
    assert_eq!(os, "linux");
    assert_eq!(arch, "x64");

    assert!(parse_platform("invalid").is_err());
}

// -------------------------------------------------------------------------
// validate_config tests
// -------------------------------------------------------------------------

#[test]
fn test_validate_config_sha256_requires_pinned() {
    // sha256 + GitHub "latest" (no tag) should be rejected.
    let config = BinaryConfig {
        name: "test".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::from([(
            "darwin-arm64".to_string(),
            BinarySource {
                github: Some(GitHubSource {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                    asset_pattern: "test-{version}.tar.gz".to_string(),
                    tag: None, // "latest" — not pinned
                }),
                direct_url: None,
                sha256: Some("abc123".to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            },
        )]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    };

    let result = validate_config(&config);
    assert!(result.is_err(), "sha256 + latest should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.contains("sha256") && err.contains("pinned"),
        "Error should mention sha256 and pinned, got: {}",
        err
    );
}

#[test]
fn test_validate_config_sha256_with_direct_url() {
    // sha256 + direct_url is valid.
    let config = BinaryConfig {
        name: "test".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::from([(
            "darwin-arm64".to_string(),
            BinarySource {
                github: None,
                direct_url: Some("https://example.com/binary.tar.gz".to_string()),
                sha256: Some("abc123".to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            },
        )]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    };

    assert!(
        validate_config(&config).is_ok(),
        "sha256 + direct_url should be valid"
    );
}

#[test]
fn test_validate_config_sha256_with_tag() {
    // sha256 + GitHub with pinned tag is valid.
    let config = BinaryConfig {
        name: "test".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::from([(
            "darwin-arm64".to_string(),
            BinarySource {
                github: Some(GitHubSource {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                    asset_pattern: "test-{version}.tar.gz".to_string(),
                    tag: Some("v1.0.0".to_string()),
                }),
                direct_url: None,
                sha256: Some("abc123".to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            },
        )]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    };

    assert!(
        validate_config(&config).is_ok(),
        "sha256 + github.tag should be valid"
    );
}

#[test]
fn test_validate_config_no_sha256() {
    // No sha256 — anything goes.
    let config = BinaryConfig {
        name: "test".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::from([(
            "darwin-arm64".to_string(),
            BinarySource {
                github: Some(GitHubSource {
                    owner: "owner".to_string(),
                    repo: "repo".to_string(),
                    asset_pattern: "test-{version}.tar.gz".to_string(),
                    tag: None, // "latest" — fine without sha256
                }),
                direct_url: None,
                sha256: None,
                archive_format: Some(ArchiveFormat::TarGz),
            },
        )]),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    };

    assert!(
        validate_config(&config).is_ok(),
        "No sha256 should always be valid"
    );
}

// -------------------------------------------------------------------------
// get_cached_binary_path tests
// -------------------------------------------------------------------------

#[test]
fn test_get_cached_binary_path_simple() {
    // Flat binary: no cache_dir, no layout → ~/.moss/bin/ffmpeg
    let config = test_config("ffmpeg");
    let path = get_cached_binary_path(&config).unwrap();
    // Normalize separators so the `/`-form suffix matches on Windows too;
    // Windows also appends `.exe` to the binary filename.
    let path_str = moss_core::slug::normalize_separators(&path.to_string_lossy());
    let expected = if cfg!(target_os = "windows") {
        ".moss/bin/ffmpeg.exe"
    } else {
        ".moss/bin/ffmpeg"
    };
    assert!(
        path_str.ends_with(expected),
        "Simple binary should be at ~/{}, got: {}",
        expected,
        path_str
    );
}

#[test]
fn test_get_cached_binary_path_with_layout() {
    // Nested layout: cache_dir + layout → ~/.moss/bin/git-portable/bin/git
    let config = BinaryConfig {
        name: "git".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::new(),
        archive_layout: Some(ArchiveLayout {
            binary_path: "bin/git".to_string(),
            executable_dirs: vec!["bin".to_string()],
        }),
        cache_dir: Some("git-portable".to_string()),
        required_disk_space: None,
    };
    let path = get_cached_binary_path(&config).unwrap();
    // Normalize separators so the `/`-form suffix matches on Windows too.
    // The nested layout uses `archive_layout.binary_path` verbatim, so no
    // `.exe` is appended here.
    let path_str = moss_core::slug::normalize_separators(&path.to_string_lossy());
    assert!(
        path_str.ends_with(".moss/bin/git-portable/bin/git"),
        "Nested binary should be at ~/.moss/bin/git-portable/bin/git, got: {}",
        path_str
    );
}

#[test]
fn test_get_cached_binary_path_cache_dir_no_layout() {
    // Cache dir but no layout → ~/.moss/bin/subdir/hugo
    let config = BinaryConfig {
        name: "hugo".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::new(),
        archive_layout: None,
        cache_dir: Some("hugo-cache".to_string()),
        required_disk_space: None,
    };
    let path = get_cached_binary_path(&config).unwrap();
    // Normalize separators so the `/`-form suffix matches on Windows too;
    // this branch uses the binary filename, so Windows appends `.exe`.
    let path_str = moss_core::slug::normalize_separators(&path.to_string_lossy());
    let expected = if cfg!(target_os = "windows") {
        ".moss/bin/hugo-cache/hugo.exe"
    } else {
        ".moss/bin/hugo-cache/hugo"
    };
    assert!(
        path_str.ends_with(expected),
        "Cache-dir binary should be at ~/{}, got: {}",
        expected,
        path_str
    );
}

// -------------------------------------------------------------------------
// get_binary_filename tests
// -------------------------------------------------------------------------

#[test]
fn test_get_binary_filename_default() {
    let config = test_config("hugo");
    let name = get_binary_filename(&config);
    // On non-Windows, should be "hugo"
    if cfg!(target_os = "windows") {
        assert_eq!(name, "hugo.exe");
    } else {
        assert_eq!(name, "hugo");
    }
}

#[test]
fn test_get_binary_filename_override() {
    let mut config = test_config("git");
    config.binary_name = Some("git-custom".to_string());
    let name = get_binary_filename(&config);
    if cfg!(target_os = "windows") {
        assert_eq!(name, "git-custom.exe");
    } else {
        assert_eq!(name, "git-custom");
    }
}

// -------------------------------------------------------------------------
// resolve_asset_pattern tests
// -------------------------------------------------------------------------

#[test]
fn test_github_url_construction() {
    let pattern = "hugo_extended_{version}_{os}-{arch}.tar.gz";
    let result = resolve_asset_pattern(pattern, "0.123.0", "darwin", "arm64");
    assert_eq!(
        result, "hugo_extended_0.123.0_darwin-arm64.tar.gz",
        "All placeholders should be resolved"
    );
}

#[test]
fn test_github_url_construction_no_placeholders() {
    let pattern = "binary.tar.gz";
    let result = resolve_asset_pattern(pattern, "1.0", "linux", "x64");
    assert_eq!(result, "binary.tar.gz", "No placeholders means no changes");
}

#[test]
fn test_github_url_construction_dugite_pattern() {
    let pattern = "dugite-native-v{version}-macOS-{arch}.tar.gz";
    let result = resolve_asset_pattern(pattern, "2.53.0", "darwin", "arm64");
    assert_eq!(result, "dugite-native-v2.53.0-macOS-arm64.tar.gz");
}

// -------------------------------------------------------------------------
// ArchiveFormat serialization tests
// -------------------------------------------------------------------------

#[test]
fn test_archive_format_serialization() {
    // Verify snake_case serialization
    let tar_gz = serde_json::to_string(&ArchiveFormat::TarGz).unwrap();
    assert_eq!(tar_gz, "\"tar_gz\"");

    let zip = serde_json::to_string(&ArchiveFormat::Zip).unwrap();
    assert_eq!(zip, "\"zip\"");

    let raw = serde_json::to_string(&ArchiveFormat::Raw).unwrap();
    assert_eq!(raw, "\"raw\"");
}

#[test]
fn test_resolution_source_serialization() {
    let configured = serde_json::to_string(&ResolutionSource::ConfiguredPath).unwrap();
    assert_eq!(configured, "\"configured_path\"");

    let downloaded = serde_json::to_string(&ResolutionSource::Downloaded).unwrap();
    assert_eq!(downloaded, "\"downloaded\"");
}

// -------------------------------------------------------------------------
// BinaryConfig round-trip serialization test
// -------------------------------------------------------------------------

#[test]
fn test_binary_config_round_trip() {
    let config = BinaryConfig {
        name: "hugo".to_string(),
        binary_name: None,
        version_check: Some(VersionCheck {
            args: vec!["version".to_string()],
            pattern: Some(r"v(\d+\.\d+\.\d+)".to_string()),
        }),
        sources: HashMap::from([(
            "darwin-arm64".to_string(),
            BinarySource {
                github: Some(GitHubSource {
                    owner: "gohugoio".to_string(),
                    repo: "hugo".to_string(),
                    asset_pattern: "hugo_extended_{version}_{os}-{arch}.tar.gz".to_string(),
                    tag: Some("v0.123.0".to_string()),
                }),
                direct_url: None,
                sha256: Some("abc123def456".to_string()),
                archive_format: Some(ArchiveFormat::TarGz),
            },
        )]),
        archive_layout: None,
        cache_dir: Some("hugo".to_string()),
        required_disk_space: Some(100 * 1024 * 1024),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: BinaryConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "hugo");
    assert!(deserialized.version_check.is_some());
    assert!(deserialized.sources.contains_key("darwin-arm64"));
    assert_eq!(deserialized.cache_dir, Some("hugo".to_string()));
}

// -------------------------------------------------------------------------
// check_binary_works with version_check: None (existence-only)
// -------------------------------------------------------------------------

#[test]
fn test_check_binary_works_no_version_check() {
    // With version_check: None, an absolute path just checks file existence.
    // Use a binary guaranteed to exist on each host.
    #[cfg(target_os = "windows")]
    let bin = "C:\\Windows\\System32\\cmd.exe";
    #[cfg(not(target_os = "windows"))]
    let bin = "/bin/echo";

    let config = test_config("echo");
    let result = check_binary_works(bin, &config);
    assert!(result.is_ok(), "Existence check should pass for {}", bin);
    assert_eq!(result.unwrap(), None);
}

#[test]
fn test_check_binary_works_no_version_check_bare_name() {
    // With version_check: None, a bare name verifies it's in PATH.
    // (echo is a cmd builtin on Windows, so probe for cmd there.)
    let name = if cfg!(target_os = "windows") { "cmd" } else { "echo" };
    let config = test_config(name);
    let result = check_binary_works(name, &config);
    assert!(result.is_ok(), "Bare '{name}' should be found in PATH");
    assert_eq!(result.unwrap(), None);
}

// -------------------------------------------------------------------------
// resolve_download_url tests (direct_url bypass)
// -------------------------------------------------------------------------

#[test]
fn test_direct_url_bypass_api() {
    // direct_url should be returned immediately without any GitHub API call.
    let source = BinarySource {
        github: Some(GitHubSource {
            owner: "should-not-be-called".to_string(),
            repo: "should-not-be-called".to_string(),
            asset_pattern: "irrelevant".to_string(),
            tag: None,
        }),
        direct_url: Some("https://example.com/binary.tar.gz".to_string()),
        sha256: None,
        archive_format: Some(ArchiveFormat::TarGz),
    };

    let (url, version) = resolve_download_url(&source, "darwin-arm64").unwrap();
    assert_eq!(url, "https://example.com/binary.tar.gz");
    assert!(version.is_none(), "direct_url should not produce a version");
}

// -------------------------------------------------------------------------
// Extraction tests (tar.gz and zip)
// -------------------------------------------------------------------------

#[test]
fn test_extract_tar_gz_atomic() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    // Create a synthetic .tar.gz with a single file "testbin"
    let mut tar_builder = tar::Builder::new(Vec::new());
    let content = b"#!/bin/sh\necho hello";
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar_builder
        .append_data(&mut header, "testbin", &content[..])
        .unwrap();
    let tar_data = tar_builder.into_inner().unwrap();

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_data).unwrap();
    let gz_data = encoder.finish().unwrap();

    // Extract to a temp directory
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config("testbin");

    extract_tar_gz_to_cache(&gz_data, &config, tmp.path()).unwrap();

    // The file should exist (no cache_dir, so extracted to cache_parent)
    let extracted = tmp.path().join("testbin");
    assert!(
        extracted.exists(),
        "Extracted binary should exist at {}",
        extracted.display()
    );
}

#[test]
fn test_extract_tar_gz_complex_layout() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    // Create a synthetic .tar.gz with nested structure: bin/git, libexec/git-core/git-upload-pack
    let mut tar_builder = tar::Builder::new(Vec::new());

    let git_content = b"#!/bin/sh\necho git";
    let mut header = tar::Header::new_gnu();
    header.set_size(git_content.len() as u64);
    header.set_mode(0o755);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar_builder
        .append_data(&mut header, "bin/git", &git_content[..])
        .unwrap();

    let helper_content = b"#!/bin/sh\necho helper";
    let mut header2 = tar::Header::new_gnu();
    header2.set_size(helper_content.len() as u64);
    header2.set_mode(0o755);
    header2.set_entry_type(tar::EntryType::Regular);
    header2.set_cksum();
    tar_builder
        .append_data(
            &mut header2,
            "libexec/git-core/git-upload-pack",
            &helper_content[..],
        )
        .unwrap();

    let tar_data = tar_builder.into_inner().unwrap();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_data).unwrap();
    let gz_data = encoder.finish().unwrap();

    // Config with cache_dir and archive_layout
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir_name = format!("test-git-{}", std::process::id());
    let config = BinaryConfig {
        name: "git".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::new(),
        archive_layout: Some(ArchiveLayout {
            binary_path: "bin/git".to_string(),
            executable_dirs: vec!["bin".to_string(), "libexec/git-core".to_string()],
        }),
        cache_dir: Some(cache_dir_name.clone()),
        required_disk_space: None,
    };

    // We can't use extract_tar_gz_to_cache directly since it uses get_moss_bin_dir().
    // Instead, test the underlying tar extraction logic directly.
    let decoder = flate2::read::GzDecoder::new(&gz_data[..]);
    let mut archive = tar::Archive::new(decoder);
    let extract_dir = tmp.path().join(&cache_dir_name);
    std::fs::create_dir_all(&extract_dir).unwrap();
    archive.unpack(&extract_dir).unwrap();

    // Verify nested structure
    assert!(extract_dir.join("bin/git").exists(), "bin/git should exist");
    assert!(
        extract_dir
            .join("libexec/git-core/git-upload-pack")
            .exists(),
        "libexec/git-core/git-upload-pack should exist"
    );

    // Test executable permissions
    #[cfg(unix)]
    {
        set_executable_permissions(
            &extract_dir,
            &["bin".to_string(), "libexec/git-core".to_string()],
        )
        .unwrap();

        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(extract_dir.join("bin/git"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(perms & 0o755, 0o755, "bin/git should have 755 permissions");
        let perms2 = std::fs::metadata(extract_dir.join("libexec/git-core/git-upload-pack"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            perms2 & 0o755,
            0o755,
            "git-upload-pack should have 755 permissions"
        );
    }
}

#[test]
fn test_extract_zip_simple() {
    use std::io::Write;

    // Create a synthetic zip with the binary inside. The resolver looks
    // for the platform binary filename, which is `testbin.exe` on Windows.
    let bin_name = if cfg!(target_os = "windows") {
        "testbin.exe"
    } else {
        "testbin"
    };
    let tmp = tempfile::tempdir().unwrap();
    let mut zip_buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let options = zip::write::FileOptions::default().unix_permissions(0o755);
        writer.start_file(bin_name, options).unwrap();
        writer.write_all(b"#!/bin/sh\necho hello").unwrap();
        writer.finish().unwrap();
    }

    let config = test_config("testbin");
    extract_zip_to_cache(&zip_buf, &config, tmp.path()).unwrap();

    let extracted = tmp.path().join(bin_name);
    assert!(
        extracted.exists(),
        "Extracted binary should exist at {}",
        extracted.display()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&extracted).unwrap().permissions().mode();
        assert_eq!(perms & 0o755, 0o755, "Should have executable permissions");
    }
}

#[test]
fn test_extract_zip_nested() {
    use std::io::Write;

    // Create a zip with the binary in a subdirectory. The resolver looks
    // for the platform binary filename, which is `testbin.exe` on Windows.
    let bin_name = if cfg!(target_os = "windows") {
        "testbin.exe"
    } else {
        "testbin"
    };
    let tmp = tempfile::tempdir().unwrap();
    let mut zip_buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let options = zip::write::FileOptions::default().unix_permissions(0o755);
        writer
            .start_file(format!("subdir/{bin_name}"), options)
            .unwrap();
        writer.write_all(b"#!/bin/sh\necho hello").unwrap();
        writer.finish().unwrap();
    }

    let config = test_config("testbin");
    extract_zip_to_cache(&zip_buf, &config, tmp.path()).unwrap();

    let extracted = tmp.path().join(bin_name);
    assert!(
        extracted.exists(),
        "Should find binary even in subdirectory"
    );
}

// -------------------------------------------------------------------------
// Executable permissions test
// -------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn test_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Create files with non-executable permissions
    let file1 = bin_dir.join("tool1");
    let file2 = bin_dir.join("tool2");
    std::fs::write(&file1, "binary1").unwrap();
    std::fs::write(&file2, "binary2").unwrap();
    std::fs::set_permissions(&file1, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&file2, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Apply executable permissions
    set_executable_permissions(tmp.path(), &["bin".to_string()]).unwrap();

    let perms1 = std::fs::metadata(&file1).unwrap().permissions().mode();
    let perms2 = std::fs::metadata(&file2).unwrap().permissions().mode();
    assert_eq!(perms1 & 0o755, 0o755, "tool1 should be executable");
    assert_eq!(perms2 & 0o755, 0o755, "tool2 should be executable");
}

// -------------------------------------------------------------------------
// resolve_binary with cached binary
// -------------------------------------------------------------------------

#[test]
fn test_resolve_cached() {
    // Create a fake cached binary in a temp dir, then verify resolve_binary finds it.
    // We can't easily override ~/.moss/bin/, but we can test get_cached_binary_path
    // and check_binary_works independently — the integration is tested by
    // test_resolve_system_path (which exercises the resolution chain).
    //
    // Test the cache path computation + binary check as a combined unit:
    let config = BinaryConfig {
        name: "echo".to_string(),
        binary_name: None,
        version_check: None,
        sources: HashMap::new(),
        archive_layout: None,
        cache_dir: None,
        required_disk_space: None,
    };

    let cached_path = get_cached_binary_path(&config).unwrap();
    // Normalize separators so the `/`-form suffix matches on Windows too;
    // Windows appends `.exe` to the binary filename.
    let cached_str = moss_core::slug::normalize_separators(&cached_path.to_string_lossy());
    let expected = if cfg!(target_os = "windows") {
        ".moss/bin/echo.exe"
    } else {
        ".moss/bin/echo"
    };
    assert!(
        cached_str.ends_with(expected),
        "Cache path should be ~/{}, got: {}",
        expected,
        cached_path.display()
    );
}

#[test]
fn test_resolve_fallthrough() {
    // A nonexistent binary should fall through all steps and report not found.
    // This exercises steps 1→2→3→4 (with auto_download=false).
    let config = test_config("nonexistent_binary_xyz_123_abc");
    let result = resolve_binary(&config, Some("/nonexistent/path/binary"), false, None);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not found"),
        "Should indicate binary not found: {}",
        err
    );
}

// -------------------------------------------------------------------------
// SHA-256 verification in download pipeline
// -------------------------------------------------------------------------

#[test]
fn test_download_and_verify_sha256() {
    // verify_sha256 is tested in download.rs, but we verify the integration
    // point: that a wrong checksum produces a clear error message.
    use crate::build::assets::download::verify_sha256;

    let data = b"test data";
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = verify_sha256(data, wrong_hash);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("SHA-256 mismatch"),
        "Should mention SHA-256 mismatch: {}",
        err
    );
}
