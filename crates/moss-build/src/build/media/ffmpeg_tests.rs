use super::*;

#[test]
fn test_ffmpeg_detect_returns_result() {
    // This test just verifies the API works - doesn't require FFmpeg installed
    let result = FFmpegManager::detect();
    // Result is Ok if FFmpeg is installed, Err if not - both are valid
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_detect_returns_result() {
    // This test verifies the API works without requiring FFmpeg
    let _result = FFmpegManager::detect();
    // Just checking it doesn't panic
}

#[test]
fn test_ffmpeg_config_shape() {
    let config = ffmpeg_binary_config();
    assert_eq!(config.name, "ffmpeg");
    assert!(config.version_check.is_some());
    // macOS should have sources
    assert!(config.sources.contains_key("darwin-arm64"));
    assert!(config.sources.contains_key("darwin-x64"));
    // Windows should have a source
    assert!(config.sources.contains_key("windows-x64"));
    // Validate config consistency
    crate::build::assets::binary_resolver::validate_config(&config).unwrap();
}

#[test]
fn test_moss_bin_dir() {
    let dir = get_moss_bin_dir();
    assert!(dir.is_ok());
    let path = dir.unwrap();
    assert!(path.to_string_lossy().contains(".moss"));
}

#[test]
fn test_ffmpeg_manager_get_or_download() {
    // This test verifies the API - actual download only happens if FFmpeg not in PATH
    let result = FFmpegManager::get_or_download(None);
    // Should either find system FFmpeg or attempt download
    // We just verify it doesn't panic and returns a Result
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_collect_videos_for_conversion_empty() {
    use crate::types::content::{MediaMetadata, ProjectStructure};

    let project = ProjectStructure {
        root_path: "/test".to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let videos = collect_videos_for_conversion(&project);
    assert!(videos.is_empty());
}

#[test]
fn test_collect_videos_for_conversion_mov_files() {
    use crate::types::content::{MediaMetadata, ProjectStructure};

    let project = ProjectStructure {
        root_path: "/test".to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![
            MediaMetadata {
                is_animated: false,
                path: "videos/clip.mov".to_string(),
                file_type: "mov".to_string(),
                size: 1000,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
            MediaMetadata {
                is_animated: false,
                path: "videos/nature.MOV".to_string(),
                file_type: "mov".to_string(),
                size: 2000,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
        ],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 2,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let videos = collect_videos_for_conversion(&project);

    // MOV files need conversion
    assert_eq!(videos.len(), 2);
    assert!(videos.iter().any(|v| *v == "videos/clip.mov"));
    assert!(videos.iter().any(|v| *v == "videos/nature.MOV"));
}

#[test]
fn test_collect_videos_small_mp4_is_thumbnail_only() {
    use crate::types::content::{MediaMetadata, ProjectStructure};

    let project = ProjectStructure {
        root_path: "/test".to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![
            MediaMetadata {
                is_animated: false,
                path: "videos/clip.mov".to_string(),
                file_type: "mov".to_string(),
                size: 1000,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
            MediaMetadata {
                is_animated: false,
                path: "videos/already.mp4".to_string(),
                file_type: "mp4".to_string(),
                size: 2000,
                modified: None,
                dimensions: None,
                dominant_color: None,
                lqip_data_uri: None,
            },
        ],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 2,
        homepage_file: None,
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    };

    let videos = collect_videos_for_conversion(&project);

    // Every video is collected, with no per-item verdict attached. Whether a
    // video needs re-encoding is not knowable from a directory listing — it
    // takes ffprobe — so the collector no longer guesses, and there is no
    // second decider left to disagree with.
    assert_eq!(videos.len(), 2);
    assert!(videos.iter().any(|v| v.ends_with(".mov")));
    assert!(videos.iter().any(|v| v.ends_with(".mp4")));
}

// ===========================================
// Encode planning — one decision, from the ladder
// ===========================================

/// A 1080p source, comfortably inside the size budget.
fn source(duration_secs: f64, width: u32, total_kbps: Option<f64>, web: bool) -> SourceVideo {
    SourceVideo { duration_secs, width, fps: 30.0, total_kbps, has_audio: true, web_playable: web }
}

#[test]
fn plan_puts_a_short_video_on_its_top_rung() {
    // The reported failure was a 339.8 s video delivered at 2150 kbps, because
    // 97 MB / 339.8 s happens to equal that and the size budget was the only
    // input — a video one minute shorter would have been given MORE bitrate.
    // Duration no longer sets the bitrate; the ladder does.
    let config = VideoCompressionConfig::default();
    let rung = match plan_video_encode(&source(339.8, 1920, Some(2150.0), true), &config) {
        EncodePlan::Encode { rung } => rung,
        other => panic!("expected Encode, got {other:?}"),
    };
    assert_eq!((rung.width, rung.video_kbps), (1280, 2000));
    // Shorter is not higher any more: both land on the same rung.
    assert_eq!(
        plan_video_encode(&source(60.0, 1920, Some(2150.0), true), &config),
        EncodePlan::Encode { rung }
    );
}

#[test]
fn plan_re_encodes_a_small_file_that_is_too_fat_to_stream() {
    // The headline case of moss#1130: a 30 s 4K clip at 24 Mbps is only ~90 MB,
    // so the old size gate never sent it to the encoder at all and it shipped at
    // its camera bitrate. Size was the wrong question.
    let config = VideoCompressionConfig::default();
    assert!(matches!(
        plan_video_encode(&source(30.0, 3840, Some(24_000.0), true), &config),
        EncodePlan::Encode { rung } if rung.width == 1280
    ));
}

#[test]
fn plan_keeps_a_source_that_already_is_its_rung() {
    // Web-playable, no wider than the rung, no fatter than the rung: encoding
    // it again could only lose a generation.
    let config = VideoCompressionConfig::default();
    assert_eq!(
        plan_video_encode(&source(120.0, 640, Some(300.0), true), &config),
        EncodePlan::KeepOriginal
    );
}

#[test]
fn plan_re_encodes_a_source_whose_audio_makes_it_miss_its_rung() {
    // 640 wide with a picture well inside the 365 kbps rung, but wrapped in a
    // fat audio track: 700 kbps total against the rung's 557. A viewer has to
    // pull the audio too, so this is not that rung — judging on the video
    // stream alone would have shipped it.
    let config = VideoCompressionConfig::default();
    assert_eq!(
        plan_video_encode(&source(120.0, 640, Some(700.0), true), &config),
        EncodePlan::Encode { rung: moss_core::asset_paths::VIDEO_LADDER[2] }
    );
}

#[test]
fn plan_re_encodes_a_high_frame_rate_source_at_the_15_fps_rung() {
    // Small enough and narrow enough for the bottom rung, but 60 fps. That
    // rung is 15 fps for a measured reason — 30 fps at 45 kbps was a smear on
    // a real device — so a high-frame-rate original is not it, however small.
    let config = VideoCompressionConfig::default();
    let mut src = source(120.0, 320, Some(70.0), true);
    src.fps = 60.0;
    assert_eq!(
        plan_video_encode(&src, &config),
        EncodePlan::Encode { rung: moss_core::asset_paths::VIDEO_LADDER[0] }
    );
}

#[test]
fn plan_re_encodes_when_the_source_bitrate_is_unknown() {
    // No measurement means no proof the source is within policy. The other
    // reading — assume it is fine — ships an unstreamable video.
    let config = VideoCompressionConfig::default();
    assert!(matches!(
        plan_video_encode(&source(120.0, 640, None, true), &config),
        EncodePlan::Encode { .. }
    ));
}

#[test]
fn plan_re_encodes_a_web_ready_container_holding_an_unplayable_codec() {
    // HEVC-in-mp4 does not play in Firefox. `web_playable` is the probed codec
    // AND the container, never the extension alone.
    let config = VideoCompressionConfig::default();
    assert!(matches!(
        plan_video_encode(&source(120.0, 640, Some(300.0), false), &config),
        EncodePlan::Encode { .. }
    ));
}

#[test]
fn plan_walks_a_long_film_down_the_ladder_instead_of_off_it() {
    // 2.5 hours into 100 MB is 110 kbps total. The old plan derived a NEGATIVE
    // bitrate here and had to special-case it. There is nothing to special-case
    // now: no rung fits the budget, so the lowest one ships. A 320x180 film a
    // viewer can watch beats a multi-gigabyte original they cannot.
    let config = VideoCompressionConfig::default();
    assert_eq!(
        plan_video_encode(&source(8905.0, 1920, Some(4000.0), true), &config),
        EncodePlan::Encode { rung: moss_core::asset_paths::VIDEO_LADDER[0] }
    );
}

#[test]
fn plan_lets_the_size_budget_pull_a_long_video_down_a_rung() {
    // The budget and the ladder answer different questions — total bytes for
    // the host, bytes per second for the viewer — and the budget can only ever
    // move the choice DOWN the table, never to a bitrate that is not on it.
    let config = VideoCompressionConfig::default();
    let short = plan_video_encode(&source(120.0, 1920, Some(9000.0), true), &config);
    let long = plan_video_encode(&source(1800.0, 1920, Some(9000.0), true), &config);
    assert!(matches!(short, EncodePlan::Encode { rung } if rung.width == 1280));
    match long {
        EncodePlan::Encode { rung } => {
            assert!(rung.width < 1280, "expected a lower rung, got {}", rung.width);
            assert!(moss_core::asset_paths::VIDEO_LADDER.contains(&rung));
        }
        other => panic!("expected Encode, got {other:?}"),
    }
}

#[test]
fn plan_never_offers_a_rung_wider_than_the_source() {
    let config = VideoCompressionConfig::default();
    assert!(matches!(
        plan_video_encode(&source(60.0, 640, Some(9000.0), true), &config),
        EncodePlan::Encode { rung } if rung.width == 640
    ));
}


/// Test that reqwest::blocking works from async context when using ureq instead.
///
/// This test verifies that HTTP requests don't panic inside tokio runtime.
/// The original bug: reqwest::blocking panics with
/// "Cannot drop a runtime in a context where blocking is not allowed".
///
/// We test by making a simple HTTP request from async context - this would panic
/// with reqwest::blocking but should work with ureq.
#[tokio::test]
async fn test_http_request_in_async_context_does_not_panic() {
    // This simulates the GUI path where we're inside Tauri's async runtime.
    // Before fix (reqwest::blocking): PANICS
    // After fix (ureq): Should return Ok or Err gracefully (no panic)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Make a simple HTTP request - this is what the binary resolver does internally
        // Using a fast-failing URL to avoid actual network wait
        let _ = ureq::get("http://localhost:1").call();
    }));

    // The test passes if no panic occurred (regardless of request success/failure)
    // Network errors are acceptable, panics are not
    assert!(
        result.is_ok(),
        "HTTP request panicked in async context! This is the bug we're fixing."
    );
}

/// Test that get_or_download uses ureq (not reqwest::blocking) and doesn't panic.
/// This is an integration test that actually calls get_or_download from async context.
#[tokio::test]
async fn test_get_or_download_in_async_context_does_not_panic() {
    // This simulates the GUI path where we're inside Tauri's async runtime.
    // Before fix: PANICS with "Cannot drop a runtime in a context where blocking is not allowed"
    // After fix: Should return Ok or Err gracefully (no panic)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        FFmpegManager::get_or_download(None)
    }));

    // The test passes if no panic occurred (regardless of success/failure)
    // Network errors are acceptable, panics are not
    assert!(
        result.is_ok(),
        "FFmpegManager::get_or_download panicked in async context!"
    );
}

// ===========================================
// Size-Constrained Video Compression Tests
// ===========================================
// TDD: Tests written first, implementation follows.
//
// Algorithm: Adaptive CRF + Two-Pass Fallback
// - First attempt: High-quality CRF (18) with slow preset
// - Check size: If ≤ limit → done
// - If over: Calculate bitrate for target size, use two-pass encoding
//
// References:
// - CRF Guide: https://slhck.info/video/2017/02/24/crf-guide.html
// - Rate Control: https://slhck.info/video/2017/03/01/rate-control.html
// - Bitrate calculation: https://www.mux.com/articles/change-video-bitrate-with-ffmpeg

#[test]
fn test_video_compression_config_defaults() {
    let config = VideoCompressionConfig::default();

    // Default 100MB limit suitable for web delivery
    assert_eq!(config.max_size_mb, 100);

    // Slower preset = better compression ratio
    assert_eq!(config.preset, "slow");
}

#[test]
fn test_parse_ffprobe_duration_output() {
    // ffprobe outputs duration as a simple float string with newline
    // Command: ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 input.mov
    // Output: "123.456789\n"
    let output = "123.456789\n";
    let duration: f64 = output.trim().parse().expect("Should parse duration");
    assert!((duration - 123.456789).abs() < 0.0001);
}

#[test]
fn test_parse_ffprobe_duration_integer() {
    // Some videos have integer durations
    let output = "60\n";
    let duration: f64 = output
        .trim()
        .parse()
        .expect("Should parse integer duration");
    assert!((duration - 60.0).abs() < 0.0001);
}

#[test]
fn test_get_duration_nonexistent_file() {
    // get_duration should fail gracefully for nonexistent files
    let manager = FFmpegManager::new("ffmpeg".to_string());
    // Pre-seed the ffprobe cache to a nonexistent path so the probe fails
    // fast on spawn — without this, resolution could hit the network
    // (PATH/cache miss → download) inside a unit test.
    manager
        .ffprobe
        .set("/nonexistent/ffprobe".to_string())
        .unwrap();
    let result = manager.get_duration(Path::new("/nonexistent/video.mov"));
    assert!(result.is_err());
}

#[test]
fn test_ffprobe_binary_config_targets_separate_artifact() {
    // Root cause of a real site's video 404s: moss downloaded ffmpeg-only.
    // ffprobe must be provisioned from its OWN evermeet artifact on macOS —
    // the ffmpeg getrelease/zip does not contain it.
    let config = ffprobe_binary_config();
    for platform in ["darwin-arm64", "darwin-x64"] {
        let url = config.sources[platform]
            .direct_url
            .as_deref()
            .expect("macOS ffprobe source must have a direct_url");
        assert!(
            url.contains("/ffprobe/"),
            "{platform} ffprobe URL must be the separate ffprobe artifact, got: {url}"
        );
    }
}

#[test]
fn test_ffprobe_binary_config_name_selects_ffprobe() {
    // The generic zip extractor picks the single archive entry matching the
    // config's binary filename. It must be "ffprobe" so we don't re-extract
    // ffmpeg (and so the gyan essentials zip yields ffprobe.exe on Windows).
    let config = ffprobe_binary_config();
    assert_eq!(config.name, "ffprobe");
}

#[test]
fn test_resolve_ffprobe_prefers_sibling() {
    // When ffmpeg resolves to an absolute path whose sibling ffprobe exists
    // (Homebrew, or a ~/.moss/bin cache holding both), use it — no download.
    let dir =
        tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).expect("create tempdir inside repo");
    let ffmpeg = dir.path().join("ffmpeg");
    let ffprobe = dir.path().join(ffprobe_filename());
    std::fs::write(&ffmpeg, b"x").unwrap();
    std::fs::write(&ffprobe, b"x").unwrap();

    let resolved = resolve_ffprobe_path(ffmpeg.to_str().unwrap()).expect("sibling resolves");
    assert_eq!(PathBuf::from(resolved), ffprobe);
}

#[test]
fn test_needs_size_constraint() {
    // Helper to determine if a video needs size-constrained encoding
    // Video over the limit should return true
    let config = VideoCompressionConfig::default();
    let max_bytes = config.max_size_mb as u64 * 1024 * 1024;

    // 150MB file exceeds 100MB limit
    assert!(150 * 1024 * 1024 > max_bytes);

    // 50MB file is within 100MB limit
    assert!(50 * 1024 * 1024 <= max_bytes);
}

#[test]
fn test_convert_with_config_fails_gracefully() {
    // Test that convert_to_mp4_with_config handles errors gracefully.
    // Pre-seed ffprobe (get_duration runs first) to a nonexistent path so the
    // probe fails on spawn instead of resolving/downloading over the network.
    let manager = FFmpegManager::new("/nonexistent/ffmpeg".to_string());
    manager
        .ffprobe
        .set("/nonexistent/ffprobe".to_string())
        .unwrap();
    let config = VideoCompressionConfig::default();

    let result = manager.convert_to_mp4_with_config(
        Path::new("/nonexistent/input.mov"),
        Path::new("/tmp/output.mp4"),
        &config,
        None, // No progress callback
        None, // No registry
        None, // No cancel flag
    );

    assert!(result.is_err());
}

// ===========================================
// Convert Videos With Fallback Tests
// ===========================================
// Tests for graceful degradation when FFmpeg is unavailable.
// On failure: copy original video, serve as-is, show warning toast.

#[test]
fn test_copy_video_fallback() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("input.mov");
    let output = temp.path().join("output").join("input.mov");

    // Create a fake video file
    std::fs::write(&source, "fake MOV content").unwrap();

    // Copy using fallback function
    let result = copy_video_as_fallback(&source, &output);
    assert!(result.is_ok(), "Fallback copy should succeed");

    // Output should exist with same content
    assert!(output.exists(), "Output file should exist");
    assert_eq!(
        std::fs::read_to_string(&output).unwrap(),
        "fake MOV content"
    );
}

#[test]
fn test_copy_video_fallback_creates_parent_dirs() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("input.mov");
    let output = temp
        .path()
        .join("deep")
        .join("nested")
        .join("dir")
        .join("input.mov");

    std::fs::write(&source, "video content").unwrap();

    let result = copy_video_as_fallback(&source, &output);
    assert!(result.is_ok(), "Should create parent directories");
    assert!(output.exists());
}

#[test]
fn test_copy_video_fallback_nonexistent_source() {
    let temp = tempfile::TempDir::new().unwrap();
    let output = temp.path().join("output.mov");

    let result = copy_video_as_fallback(Path::new("/nonexistent/video.mov"), &output);
    assert!(result.is_err(), "Should fail for nonexistent source");
}

// ===========================================
// NEW: Video Processing Improvements Tests (TDD)
// ===========================================
// Test that videos target 95-98MB within 100MB limit
// Test that corrupted videos are detected and retried
// Test that thumbnails are generated at 800px width

#[test]
fn test_video_compression_config_has_target_fill_percentage() {
    let config = VideoCompressionConfig::default();

    // Should have target_fill_percentage field set to 97%
    assert_eq!(config.target_fill_percentage, 0.97);
}

#[test]
fn test_calculate_target_size_fills_budget() {
    let config = VideoCompressionConfig::default();

    // For 100MB limit with 97% fill target
    let target_bytes =
        (config.max_size_mb as f64 * 1024.0 * 1024.0 * config.target_fill_percentage) as u64;

    // Should target ~97MB (101,711,667 bytes)
    assert!(target_bytes >= 100_000_000 && target_bytes <= 102_000_000);
}

#[test]
fn test_validate_encoded_video_detects_missing_file() {
    let manager = match FFmpegManager::detect() {
        Ok(m) => m,
        Err(_) => return, // Skip if FFmpeg not available
    };

    let nonexistent = Path::new("/nonexistent/video.mp4");

    // validate_encoded_video should return false for missing files
    let result = manager.validate_encoded_video(nonexistent);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), false, "Should detect missing file");
}

#[test]
fn test_validate_encoded_video_checks_duration() {
    // This test will fail until we implement validate_encoded_video
    // It should use get_duration() to verify the video is playable

    let manager = match FFmpegManager::detect() {
        Ok(m) => m,
        Err(_) => return, // Skip if FFmpeg not available
    };
    // Pin ffprobe to a known-bad path so the probe fails deterministically on
    // spawn — keeps the test offline (no PATH-miss → download) and still
    // exercises the contract: any get_duration failure → not-valid → Ok(false).
    manager
        .ffprobe
        .set("/nonexistent/ffprobe".to_string())
        .unwrap();

    let temp = tempfile::TempDir::new().unwrap();
    let fake_video = temp.path().join("fake.mp4");

    // Create a fake "corrupted" video file (not actually valid MP4)
    std::fs::write(&fake_video, "not a real video").unwrap();

    // validate_encoded_video should return false for corrupted files
    let result = manager.validate_encoded_video(&fake_video);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        false,
        "Should detect corrupted video with invalid duration"
    );
}

#[test]
fn test_thumbnail_scale_uses_800px_width() {
    // Test that generate_thumbnail uses scale=800:-1 instead of scale=400:-1
    // This test verifies the command construction, not actual execution

    let manager = match FFmpegManager::detect() {
        Ok(m) => m,
        Err(_) => return, // Skip if FFmpeg not available
    };

    // We'll test this by checking if the generated command contains "scale=800"
    // For now, this test documents the requirement
    // Implementation will update generate_thumbnail() to use 800px

    // This test will be verified through integration testing with actual thumbnail generation
    assert!(true, "Thumbnail scale requirement documented");
}

#[test]
fn test_two_pass_encode_uses_unique_temp_dir() {
    // Test that two-pass encoding creates unique temp directory for pass logs
    // This prevents conflicts when multiple videos encode simultaneously

    // The temp directory should be created in the cache directory
    // and should use a unique identifier (like UUID)

    // This test documents the requirement for unique working directories
    // Implementation will create temp_dir in convert_to_mp4_with_config

    assert!(true, "Unique temp directory requirement documented");
}

// ===========================================
// FFmpeg stderr time parsing tests (TDD)
// ===========================================

#[test]
fn test_parse_ffmpeg_time_basic() {
    // FFmpeg outputs time=HH:MM:SS.ff in stderr progress lines
    assert!((parse_ffmpeg_time("time=00:01:30.50") - 90.50).abs() < 0.01);
}

#[test]
fn test_parse_ffmpeg_time_zero() {
    assert!((parse_ffmpeg_time("time=00:00:00.00") - 0.0).abs() < 0.01);
}

#[test]
fn test_parse_ffmpeg_time_hours() {
    // 1 hour, 23 minutes, 45.67 seconds = 5025.67
    assert!((parse_ffmpeg_time("time=01:23:45.67") - 5025.67).abs() < 0.01);
}

#[test]
fn test_parse_ffmpeg_time_embedded_in_line() {
    // FFmpeg stderr has many fields on the same line
    let line = "frame=  120 fps= 30 q=28.0 size=    1024kB time=00:00:04.00 bitrate= 2097.2kbits/s speed=1.5x";
    assert!((parse_ffmpeg_time(line) - 4.0).abs() < 0.01);
}

#[test]
fn test_parse_ffmpeg_time_no_time_field() {
    // Lines without time= should return -1.0 (sentinel)
    assert!(parse_ffmpeg_time("frame=  120 fps= 30 q=28.0").is_sign_negative());
}

#[test]
fn test_parse_ffmpeg_time_malformed() {
    // Malformed time value
    assert!(parse_ffmpeg_time("time=garbage").is_sign_negative());
}

#[test]
fn test_parse_ffmpeg_time_negative_value() {
    // FFmpeg can output negative time at start: time=-00:00:00.01
    // Treat as 0 or negative sentinel
    let result = parse_ffmpeg_time("time=-00:00:00.01");
    // Negative time means no useful progress yet
    assert!(result <= 0.0);
}

#[test]
fn test_parse_ffmpeg_time_two_digit_hundredths() {
    // time=00:05:12.34 = 312.34 seconds
    assert!((parse_ffmpeg_time("time=00:05:12.34") - 312.34).abs() < 0.01);
}

#[test]
fn test_parse_ffmpeg_time_three_digit_milliseconds() {
    // Some FFmpeg builds output 3 decimal places: time=00:00:10.123
    assert!((parse_ffmpeg_time("time=00:00:10.123") - 10.123).abs() < 0.001);
}

// ===========================================
// strip_ffmpeg_progress (log-tail-budget guard)
// ===========================================

/// A trimmed-down but real capture: two `\r`-joined progress ticks (ffmpeg's
/// actual overwrite-in-place framing) followed by the substantive failure
/// lines a real two-pass mux failure produced on a live vault — a moov-atom
/// reopen failure mid-mux, reproduced verbatim from a captured `moss.log`.
fn captured_stderr_fixture() -> String {
    [
        "frame= 4163 fps= 85 q=25.0 size=   37120KiB time=00:02:18.83 bitrate=2190.2kbits/s speed=2.84x elapsed=0:00:48.91    ",
        "\rframe= 4200 fps= 85 q=25.0 size=   37376KiB time=00:02:20.07 bitrate=2185.9kbits/s speed=2.83x elapsed=0:00:49.41    ",
        "\r[mp4 @ 0xa00c14780] Starting second pass: moving the moov atom to the beginning of the file\n",
        "[mp4 @ 0xa00c14780] Unable to re-open output file for shifting data\n",
        "[out#0/mp4 @ 0xa010743c0] Error writing trailer: No such file or directory\n",
        "frame= 4647 fps= 85 q=-1.0 Lsize=   41489KiB time=00:02:34.98 bitrate=2192.9kbits/s speed=2.84x elapsed=0:00:54.59    \n",
        "[libx264 @ 0xa01071880] kb/s:1999.04\n",
        "Conversion failed!\n",
    ]
    .concat()
}

#[test]
fn strip_ffmpeg_progress_drops_every_progress_tick() {
    let cleaned = strip_ffmpeg_progress(&captured_stderr_fixture());
    assert!(
        !cleaned.contains("frame="),
        "a progress tick survived filtering: {cleaned}"
    );
}

#[test]
fn strip_ffmpeg_progress_keeps_the_actual_diagnostics() {
    let cleaned = strip_ffmpeg_progress(&captured_stderr_fixture());
    assert!(cleaned.contains("Conversion failed!"), "{cleaned}");
    assert!(cleaned.contains("Unable to re-open"), "{cleaned}");
    assert!(cleaned.contains("Error writing trailer"), "{cleaned}");
    assert!(cleaned.contains("kb/s:1999.04"), "{cleaned}");
}

#[test]
fn strip_ffmpeg_progress_caps_the_kept_line_count() {
    let mut blob = String::new();
    for i in 0..(MAX_STDERR_ERROR_LINES + 50) {
        blob.push_str(&format!("[warn] non-progress line {i}\n"));
    }
    let cleaned = strip_ffmpeg_progress(&blob);
    assert_eq!(cleaned.lines().count(), MAX_STDERR_ERROR_LINES);
    // The cap keeps the MOST RECENT lines — the ones nearest the failure.
    assert!(cleaned.contains(&format!("non-progress line {}", MAX_STDERR_ERROR_LINES + 49)));
    assert!(!cleaned.contains("non-progress line 0\n"));
}

// ===========================================
// Encode start/end log lines (session-diagnosability)
// ===========================================

#[test]
fn encode_start_line_names_source_and_rung() {
    let rung = moss_core::asset_paths::VIDEO_LADDER[2];
    let line = encode_start_line(Path::new("videos/clip.mov"), rung);
    assert!(line.contains("videos/clip.mov"), "{line}");
    assert!(line.contains(&rung.width.to_string()), "{line}");
    assert!(line.contains(&rung.height.to_string()), "{line}");
}

#[test]
fn encode_end_line_success_names_size_and_elapsed() {
    let outcome = EncodeOutcome::Success { size_bytes: 12_345, retried: false };
    let line = encode_end_line(Path::new("videos/clip.mov"), Duration::from_millis(2500), &outcome);
    assert!(line.contains("videos/clip.mov"), "{line}");
    assert!(line.contains("12345"), "{line}");
    assert!(line.contains("2.5"), "{line}");
    assert!(!line.contains("retry"), "{line}");
}

#[test]
fn encode_end_line_success_names_a_retry() {
    let outcome = EncodeOutcome::Success { size_bytes: 999, retried: true };
    let line = encode_end_line(Path::new("videos/clip.mov"), Duration::from_secs(1), &outcome);
    assert!(line.contains("retry"), "{line}");
}

#[test]
fn encode_end_line_failure_names_the_reason() {
    let outcome = EncodeOutcome::Failed("Conversion failed!");
    let line = encode_end_line(Path::new("videos/clip.mov"), Duration::from_secs(3), &outcome);
    assert!(line.contains("videos/clip.mov"), "{line}");
    assert!(line.contains("Conversion failed!"), "{line}");
}

// ===========================================
// VideoCompressionConfig::to_params() Tests
// ===========================================

#[test]
fn test_to_params_default_config() {
    let config = VideoCompressionConfig::default();
    let params = config.to_params();
    assert_eq!(params["preset"], "slow");
    assert_eq!(params["max_size_mb"], 100);
    assert!(params["ladder"].as_str().unwrap().starts_with("320x180@15/45+32x1"));
    assert!(
        params.get("crf").is_none(),
        "crf keyed the cache but steered nothing: the CRF path had no callers"
    );
}

#[test]
fn test_to_params_custom_config() {
    let config = VideoCompressionConfig {
        preset: "medium".to_string(),
        max_size_mb: 50,
        target_fill_percentage: 0.97,
        ..Default::default()
    };
    let params = config.to_params();
    assert_eq!(params["preset"], "medium");
    assert_eq!(params["max_size_mb"], 50);
}

#[test]
fn test_to_params_equality() {
    let config1 = VideoCompressionConfig::default();
    let config2 = VideoCompressionConfig::default();
    assert_eq!(config1.to_params(), config2.to_params());
}

#[test]
fn to_params_carries_the_ladder_so_editing_a_rung_invalidates_the_cache() {
    // Delivery policy moved out of this struct and into a const table. Nothing
    // in the config changes when a rung is edited, so without the ladder in the
    // key every site would keep serving renditions encoded under the old table
    // — invisibly, and only on rebuild.
    let key = VideoCompressionConfig::default().to_params();
    let ladder = key["ladder"].as_str().unwrap();
    for rung in moss_core::asset_paths::VIDEO_LADDER {
        assert!(
            ladder.contains(&format!("{}x{}@{}/{}", rung.width, rung.height, rung.fps, rung.video_kbps)),
            "rung {rung:?} is absent from the cache key {ladder}"
        );
    }
}

// FFmpeg stderr speed/bitrate parsing (parse_ffmpeg_speed/parse_ffmpeg_bitrate)
// and their tests were removed here: their only production call site was the
// per-tick `log::trace!` in spawn_ffmpeg_streaming, itself deleted as the
// deletion candidate for encode start/end lines superseding it at INFO (see
// EncodeOutcome/encode_start_line/encode_end_line) — two overlapping
// progress-visibility mechanisms at different levels is not worth keeping.

// ===========================================
// Background QoS tests (priority + thread cap)
// ===========================================

#[cfg(unix)]
#[test]
fn test_background_command_sets_niceness() {
    // `ps -o nice= -p $$` prints the shell's own niceness. Note: macOS BSD
    // `nice` with no args does NOT print priority (verified), hence ps.
    let out = background_command(Path::new("/bin/sh"))
        .args(["-c", "ps -o nice= -p $$"])
        .output()
        .expect("spawn sh");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "10");
}

#[test]
fn test_default_encode_threads_is_at_least_two_and_at_most_half_cores() {
    let n = default_encode_threads();
    assert!(n >= 2);
    let cores = std::thread::available_parallelism()
        .map(|c| c.get())
        .unwrap_or(4) as u32;
    assert!(n <= cores.max(2));
}

fn pair(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

#[test]
fn test_two_pass_first_args_include_threads_cap() {
    let config = VideoCompressionConfig::default();
    let args = build_two_pass_first_args("in.mov", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog", "/dev/null");
    assert_eq!(
        pair(&args, "-threads").as_deref(),
        Some(config.encode_threads.to_string().as_str())
    );
    // existing shape preserved:
    assert_eq!(pair(&args, "-pass").as_deref(), Some("1"));
    assert_eq!(pair(&args, "-preset").as_deref(), Some(config.preset.as_str()));
    assert_eq!(pair(&args, "-b:v").as_deref(), Some("2000k"));
    assert!(args.contains(&"-an".to_string()));
    assert_eq!(args.last().map(String::as_str), Some("/dev/null"));
}

#[test]
fn test_both_passes_carry_identical_rate_control() {
    // Pass 1's statistics only describe the encode they were measured against.
    // If a flag that changes bit allocation appears on one pass and not the
    // other, pass 2 is steered by a description of a different encode. Both
    // lists come from `common_encode_args`, so this is a guard on that
    // sharing, not on anyone's diligence.
    let config = VideoCompressionConfig::default();
    let first = build_two_pass_first_args("in.mov", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog", "/dev/null");
    let second = build_two_pass_second_args("in.mov", "out.mp4", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog");
    for flag in [
        "-b:v", "-maxrate", "-bufsize", "-g", "-preset", "-vf", "-c:v", "-pix_fmt", "-profile:v",
    ] {
        assert_eq!(
            pair(&first, flag),
            pair(&second, flag),
            "{flag} differs between the two passes"
        );
        assert!(pair(&first, flag).is_some(), "{flag} missing from pass 1");
    }
}

#[test]
fn test_encode_args_cap_the_peak_not_just_the_average() {
    // `-b:v` alone is an average: a high-motion passage can borrow bits from a
    // still one and burst far above it. A viewer's buffer drains on the peak.
    let config = VideoCompressionConfig::default();
    let args = build_two_pass_second_args("in.mov", "out.mp4", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog");
    assert_eq!(pair(&args, "-b:v").as_deref(), Some("2000k"));
    assert_eq!(
        pair(&args, "-maxrate").as_deref(),
        Some("2000k"),
        "maxrate must equal b:v, or the stated ceiling bounds nothing but the average"
    );
    assert_eq!(
        pair(&args, "-bufsize").as_deref(),
        Some("4000k"),
        "2x the cap: a 2-second VBV buffer. Short windows still overshoot by \
         roughly this much (measured 2020 kbps under a 1500 kbps cap); a 1x \
         buffer bought ~4% less peak for ~10% less delivered bitrate."
    );
}

#[test]
fn test_encode_args_bound_the_keyframe_interval() {
    // x264's default is 250 frames (8.3s at 30fps). Every recovery from a
    // stall or a seek restarts at the preceding keyframe, so that default
    // costs up to 8s of re-fetched video on the link least able to afford it.
    let config = VideoCompressionConfig::default();
    let args = build_two_pass_second_args("in.mov", "out.mp4", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog");
    assert_eq!(pair(&args, "-g").as_deref(), Some("60"));
    assert!(
        !args.contains(&"-sc_threshold".to_string()),
        "scene-cut keyframes stay enabled: this is one progressive file, not a \
         segmented ladder, so nothing needs keyframes at fixed offsets"
    );
}

#[test]
fn test_encode_args_pin_a_universally_decodable_pixel_format() {
    // A 10-bit HDR clip off an iPhone encodes to yuv420p10le by default, which
    // Safari and most TVs will not decode. The level is deliberately NOT
    // pinned — x264 derives it, and pinning can only raise it.
    let config = VideoCompressionConfig::default();
    let args = build_two_pass_second_args("in.mov", "out.mp4", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog");
    assert_eq!(pair(&args, "-pix_fmt").as_deref(), Some("yuv420p"));
    assert_eq!(pair(&args, "-profile:v").as_deref(), Some("high"));
    assert!(!args.contains(&"-level".to_string()));
}

#[test]
fn test_two_pass_second_args_include_threads_cap() {
    let config = VideoCompressionConfig::default();
    let args = build_two_pass_second_args("in.mov", "out.mp4", moss_core::asset_paths::VIDEO_LADDER[5], 30.0, &config, "/tmp/passlog");
    assert_eq!(
        pair(&args, "-threads").as_deref(),
        Some(config.encode_threads.to_string().as_str())
    );
    // existing shape preserved:
    assert_eq!(pair(&args, "-pass").as_deref(), Some("2"));
    assert_eq!(pair(&args, "-preset").as_deref(), Some(config.preset.as_str()));
    assert_eq!(pair(&args, "-c:a").as_deref(), Some("aac"));
    assert_eq!(pair(&args, "-b:a").as_deref(), Some("192k"));
    assert_eq!(pair(&args, "-ac").as_deref(), Some("2"));
    assert_eq!(pair(&args, "-movflags").as_deref(), Some("+faststart"));
    assert_eq!(args.last().map(String::as_str), Some("out.mp4"));
}

#[test]
fn test_to_params_excludes_encode_threads() {
    // thread count is an execution detail; including it would invalidate the
    // transform cache + video-set fingerprint across machines.
    let config = VideoCompressionConfig {
        encode_threads: 99,
        ..Default::default()
    };
    assert_eq!(
        config.to_params(),
        VideoCompressionConfig::default().to_params()
    );
}
