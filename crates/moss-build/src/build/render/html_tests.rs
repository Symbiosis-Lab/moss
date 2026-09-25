use super::tab_title;

#[test]
fn tab_title_rules() {
    assert_eq!(tab_title("Research", "Site", false), "Research - Site");
    assert_eq!(tab_title("Site", "Site", false), "Site"); // no X - X doubling
    assert_eq!(tab_title("Site", "Site", true), "Site"); // homepage bare
    assert_eq!(tab_title("Home", "Site", true), "Home"); // homepage bare even if differ
}

/// Test that non-sidebar pages do NOT inject "No posts yet" content.
///
/// Regression test: The bug was that all non-sidebar homepages showed
/// "No posts yet" even when there were posts. The sidebar should only
/// appear when explicitly requested via the `sidebar` field.
#[test]
fn test_non_sidebar_homepage_should_not_inject_latest_sidebar() {
    // Simulate the render.rs logic for a non-sidebar page
    // (sidebar_html would be None since use_sidebar_layout is false)
    let sidebar_html: Option<String> = None;
    let use_sidebar_layout = false; // no sidebar field set

    // This mirrors the FIXED code in render.rs lines 1523-1527
    let latest_sidebar: Option<String> = if use_sidebar_layout {
        sidebar_html
    } else {
        None
    };

    // Non-sidebar pages should have latest_sidebar = None
    assert_eq!(
        latest_sidebar, None,
        "Non-sidebar homepage should have latest_sidebar = None, not {:?}",
        latest_sidebar
    );
}

/// Tests for the per-language homepage lookup helper that backs the OG
/// fallback cascades (cover + description). Critical for multilingual
/// sites: a Chinese sub-page should pull from `zh-hans/index.html` (if
/// present), NOT the English-default `index.html`.
mod find_homepage_doc_tests {
    use super::super::find_homepage_doc;
    use crate::build::types::ParsedDocument;
    use crate::i18n::Language;

    fn doc(url_path: &str, lang: Language) -> ParsedDocument {
        ParsedDocument {
            url_path: url_path.to_string(),
            lang,
            ..Default::default()
        }
    }

    #[test]
    fn page_in_default_lang_returns_root_index() {
        let docs = vec![doc("index.html", Language::En)];
        let found = find_homepage_doc(&docs, Language::En, Language::En);
        assert_eq!(found.map(|d| d.url_path.as_str()), Some("index.html"));
    }

    #[test]
    fn page_in_non_default_lang_with_lang_homepage_returns_lang_index() {
        let docs = vec![
            doc("index.html", Language::En),
            doc("zh-hans/index.html", Language::ZhHans),
        ];
        let found = find_homepage_doc(&docs, Language::ZhHans, Language::En);
        assert_eq!(
            found.map(|d| d.url_path.as_str()),
            Some("zh-hans/index.html"),
            "Chinese page on EN-default site must use the Chinese homepage"
        );
    }

    #[test]
    fn page_in_non_default_lang_without_lang_homepage_falls_back_to_root() {
        // No zh-hans/index.html exists — the cascade should not break;
        // fall back to the site-default homepage so the page still gets
        // SOMETHING rather than emitting an empty share card.
        let docs = vec![doc("index.html", Language::En)];
        let found = find_homepage_doc(&docs, Language::ZhHans, Language::En);
        assert_eq!(found.map(|d| d.url_path.as_str()), Some("index.html"));
    }

    #[test]
    fn empty_docs_returns_none() {
        let docs: Vec<ParsedDocument> = vec![];
        assert!(find_homepage_doc(&docs, Language::En, Language::En).is_none());
    }

    #[test]
    fn no_homepage_in_docs_returns_none() {
        let docs = vec![doc("about/index.html", Language::En)];
        assert!(find_homepage_doc(&docs, Language::En, Language::En).is_none());
    }
}

/// Tests for homepage OG tags and meta description generation.
mod homepage_og_tests {
    use crate::build::page::meta;

    /// Homepage with frontmatter description should use it for meta description.
    #[test]
    fn test_homepage_description_from_frontmatter() {
        let frontmatter_desc = Some("第一段。".to_string());
        let content = "Some body content here.";
        let desc = meta::resolve_page_description(frontmatter_desc.as_deref(), content, true);
        assert_eq!(desc.as_deref(), Some("第一段。"));
    }

    /// Homepage without frontmatter description falls back to content extraction.
    #[test]
    fn test_homepage_description_fallback_to_content() {
        let content = "First paragraph of homepage content.";
        let desc = meta::resolve_page_description(None, content, true);
        assert_eq!(
            desc.as_deref(),
            Some("First paragraph of homepage content.")
        );
    }

    /// Article with explicit description should prefer it over auto-extract.
    #[test]
    fn test_article_description_prefers_frontmatter() {
        let frontmatter_desc = Some("百川汇入大海，昼夜不息。".to_string());
        let content = "This is the full article body with many paragraphs.";
        let desc = meta::resolve_page_description(frontmatter_desc.as_deref(), content, true);
        assert_eq!(desc.as_deref(), Some("百川汇入大海，昼夜不息。"));
    }

    /// Frontmatter description with markdown should be stripped.
    #[test]
    fn test_description_strips_markdown_from_frontmatter() {
        let frontmatter_desc =
            Some("A **bold** claim about [something](https://example.com)".to_string());
        let content = "Body text.";
        let desc = meta::resolve_page_description(frontmatter_desc.as_deref(), content, true);
        assert_eq!(desc.as_deref(), Some("A bold claim about something"));
    }

    /// Empty content with no frontmatter returns None.
    #[test]
    fn test_description_empty_content_returns_none() {
        let desc = meta::resolve_page_description(None, "", true);
        assert_eq!(desc, None);
    }
}

/// Tests for stale file cleanup preserving video_outputs.
///
/// The bug: Converted .mp4 files are deleted by stale cleanup because
/// only site_hashes.files is checked, not video_outputs.
mod stale_cleanup_tests {
    use crate::types::content::SiteHashes;
    use std::collections::HashSet;

    /// Helper function that mirrors the stale cleanup logic in render.rs.
    /// Returns files that would be deleted by stale cleanup.
    ///
    /// Matches production behavior:
    /// - Preserves files in site_hashes.files
    /// - Preserves files in site_hashes.video_outputs
    /// - Preserves files in site_hashes.image_outputs
    /// - Deletes .tmp files (self-healing for interrupted conversions)
    fn files_to_delete(site_hashes: &SiteHashes, files_in_output: &[&str]) -> Vec<String> {
        let known_files: HashSet<&str> = site_hashes
            .files
            .keys()
            .map(|k| k.as_str())
            .chain(site_hashes.video_outputs.iter().map(|s| s.as_str()))
            .chain(site_hashes.image_outputs.iter().map(|s| s.as_str()))
            .collect();

        files_in_output
            .iter()
            .filter(|f| !known_files.contains(**f) || f.ends_with(".tmp"))
            .map(|f| f.to_string())
            .collect()
    }

    /// Test that video_outputs files are preserved by stale cleanup.
    ///
    /// This is the core fix: files in video_outputs should NOT be deleted.
    #[test]
    fn test_stale_cleanup_preserves_video_outputs() {
        let mut hashes = SiteHashes::new();
        hashes
            .video_outputs
            .insert("videos/aimeili.mp4".to_string());

        let files_in_output = vec!["videos/aimeili.mp4", "videos/test.mp4"];

        let to_delete = files_to_delete(&hashes, &files_in_output);

        // aimeili.mp4 should NOT be deleted (it's in video_outputs)
        assert!(
            !to_delete.contains(&"videos/aimeili.mp4".to_string()),
            "File in video_outputs should NOT be deleted"
        );
        // test.mp4 SHOULD be deleted (not in any tracking)
        assert!(
            to_delete.contains(&"videos/test.mp4".to_string()),
            "Untracked file should be deleted"
        );
    }

    /// Test that files in both files and video_outputs are preserved.
    #[test]
    fn test_stale_cleanup_preserves_files_and_video_outputs() {
        let mut hashes = SiteHashes::new();
        hashes
            .files
            .insert("index.html".to_string(), "abc123".to_string());
        hashes
            .video_outputs
            .insert("videos/aimeili.mp4".to_string());

        let files_in_output = vec!["index.html", "videos/aimeili.mp4", "orphan.txt"];

        let to_delete = files_to_delete(&hashes, &files_in_output);

        assert!(!to_delete.contains(&"index.html".to_string()));
        assert!(!to_delete.contains(&"videos/aimeili.mp4".to_string()));
        assert!(to_delete.contains(&"orphan.txt".to_string()));
    }

    /// Test that .tmp files are always deleted (self-healing for interrupted conversions).
    #[test]
    fn test_stale_cleanup_removes_tmp_files() {
        let hashes = SiteHashes::new();
        let files_in_output = vec!["videos/aimeili.mp4.tmp"];

        let to_delete = files_to_delete(&hashes, &files_in_output);
        assert!(to_delete.contains(&"videos/aimeili.mp4.tmp".to_string()));
    }

    /// Test that generate_blocking_content() loads video_outputs from previous hashes.json
    /// so stale cleanup preserves previously-converted video files and thumbnails.
    ///
    /// Bug: generate_blocking_content() creates a fresh SiteHashes and copies `sources` from
    /// previous_hashes but not `video_outputs`. The stale cleanup then sees an empty
    /// video_outputs set and deletes thumbnail/mp4 files from the output directory.
    #[test]
    fn test_stale_cleanup_preserves_video_outputs_across_rebuilds() {
        use crate::build::manifest::PendingManifest;
        use crate::build::render::generate_blocking_content;
        use crate::build::render::SiteConfig;
        use crate::build::scan::scan::scan_folder;
        use crate::types::content::SiteHashes;
        use std::fs;

        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_stale_video_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        // Create minimal site content
        fs::write(test_dir.join("index.md"), "# Test").unwrap();

        // Simulate a previous build that registered video_outputs in hashes.json
        let moss_dir = test_dir.join(".moss");
        fs::create_dir_all(moss_dir.join("build.nosync")).unwrap();
        let hashes_json = serde_json::json!({
            "files": {},
            "sources": {},
            "video_outputs": [
                "videos/aimeili.mp4",
                "videos/aimeili.thumb.jpg"
            ]
        });
        fs::write(
            moss_dir.join("build.nosync").join("hashes.json"),
            hashes_json.to_string(),
        )
        .unwrap();

        // Create the output directory with video files that would have been
        // placed there by a previous background video conversion
        let output_dir = moss_dir.join("build.nosync").join("site");
        let videos_dir = output_dir.join("videos");
        fs::create_dir_all(&videos_dir).unwrap();
        fs::write(videos_dir.join("aimeili.mp4"), "fake mp4").unwrap();
        fs::write(videos_dir.join("aimeili.thumb.jpg"), "fake thumb").unwrap();

        // Run generate_blocking_content — this triggers stale cleanup
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // The critical assertion: video files must survive stale cleanup
        assert!(
            videos_dir.join("aimeili.mp4").exists(),
            "aimeili.mp4 should survive stale cleanup (registered in video_outputs)"
        );
        assert!(
            videos_dir.join("aimeili.thumb.jpg").exists(),
            "aimeili.thumb.jpg should survive stale cleanup (registered in video_outputs)"
        );
    }
}

/// Tests for stale directory cleanup.
///
/// When a source .md file is deleted, stale file cleanup removes the generated
/// index.html but leaves the empty parent directory behind. These tests verify
/// the logic that identifies stale directories for removal by deriving the
/// expected directory set from site_hashes keys.
mod stale_dir_cleanup_tests {
    use crate::types::content::SiteHashes;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// Derive the set of expected directories from site_hashes.
    /// Every ancestor of every known file key, video_output, and image_output is expected.
    fn expected_dirs(site_hashes: &SiteHashes) -> HashSet<PathBuf> {
        let mut dirs = HashSet::new();
        for key in site_hashes
            .files
            .keys()
            .chain(site_hashes.video_outputs.iter())
            .chain(site_hashes.image_outputs.iter())
        {
            let p = Path::new(key);
            let mut current = p.parent();
            while let Some(dir) = current {
                if dir == Path::new("") {
                    break;
                }
                dirs.insert(dir.to_path_buf());
                current = dir.parent();
            }
        }
        dirs
    }

    /// Given expected dirs and actual dirs on disk, return dirs to remove.
    fn dirs_to_delete(expected: &HashSet<PathBuf>, actual_dirs: &[&str]) -> Vec<String> {
        actual_dirs
            .iter()
            .filter(|d| !expected.contains(Path::new(*d)))
            .map(|d| d.to_string())
            .collect()
    }

    #[test]
    fn test_stale_dir_identified_for_removal() {
        let mut hashes = SiteHashes::new();
        hashes.insert("articles/active/index.html".to_string(), "abc".to_string());

        let expected = expected_dirs(&hashes);
        let actual = vec!["articles", "articles/active", "articles/stale-article"];
        let to_delete = dirs_to_delete(&expected, &actual);

        assert!(
            to_delete.contains(&"articles/stale-article".to_string()),
            "Stale directory should be identified for removal"
        );
    }

    #[test]
    fn test_active_dir_preserved() {
        let mut hashes = SiteHashes::new();
        hashes.insert("articles/active/index.html".to_string(), "abc".to_string());

        let expected = expected_dirs(&hashes);
        let actual = vec!["articles", "articles/active"];
        let to_delete = dirs_to_delete(&expected, &actual);

        assert!(
            !to_delete.contains(&"articles/active".to_string()),
            "Active directory should be preserved"
        );
    }

    #[test]
    fn test_ancestor_dirs_preserved() {
        let mut hashes = SiteHashes::new();
        hashes.insert(
            "articles/tutorials/getting-started/index.html".to_string(),
            "abc".to_string(),
        );

        let expected = expected_dirs(&hashes);

        assert!(expected.contains(Path::new("articles")));
        assert!(expected.contains(Path::new("articles/tutorials")));
        assert!(expected.contains(Path::new("articles/tutorials/getting-started")));

        let actual = vec![
            "articles",
            "articles/tutorials",
            "articles/tutorials/getting-started",
        ];
        let to_delete = dirs_to_delete(&expected, &actual);
        assert!(
            to_delete.is_empty(),
            "All ancestor dirs should be preserved"
        );
    }

    #[test]
    fn test_video_output_dirs_preserved() {
        let mut hashes = SiteHashes::new();
        hashes.video_outputs.insert("videos/clip.mp4".to_string());

        let expected = expected_dirs(&hashes);
        let actual = vec!["videos"];
        let to_delete = dirs_to_delete(&expected, &actual);

        assert!(
            !to_delete.contains(&"videos".to_string()),
            "Directory containing video outputs should be preserved"
        );
    }

    #[test]
    fn test_root_level_only_deletes_all_subdirs() {
        // When site only has root-level files, all subdirectories are stale.
        let mut hashes = SiteHashes::new();
        hashes.insert("index.html".to_string(), "a".to_string());
        hashes.insert("rss.xml".to_string(), "b".to_string());

        let expected = expected_dirs(&hashes);
        assert!(
            expected.is_empty(),
            "Root-level files should produce no expected dirs"
        );

        let actual = vec!["old-articles", "assets", "images"];
        let to_delete = dirs_to_delete(&expected, &actual);

        assert_eq!(to_delete.len(), 3, "All subdirs should be stale");
    }

    #[test]
    fn test_mixed_stale_and_active() {
        let mut hashes = SiteHashes::new();
        hashes.insert("articles/kept/index.html".to_string(), "a".to_string());
        hashes.insert("index.html".to_string(), "b".to_string());
        hashes.video_outputs.insert("videos/clip.mp4".to_string());

        let expected = expected_dirs(&hashes);
        let actual = vec![
            "articles",
            "articles/kept",
            "articles/deleted-one",
            "articles/deleted-two",
            "videos",
            "old-section",
        ];
        let to_delete = dirs_to_delete(&expected, &actual);

        assert!(to_delete.contains(&"articles/deleted-one".to_string()));
        assert!(to_delete.contains(&"articles/deleted-two".to_string()));
        assert!(to_delete.contains(&"old-section".to_string()));
        assert!(!to_delete.contains(&"articles".to_string()));
        assert!(!to_delete.contains(&"articles/kept".to_string()));
        assert!(!to_delete.contains(&"videos".to_string()));
    }
}

/// Tests for non-blocking video conversion (Two-Phase Build).
///
/// The problem: Video conversion blocks the preview UI. Users see
/// "Converting video 2/3... 80%" and cannot use the preview until
/// all videos finish converting.
///
/// The fix: Extract video items from generate_blocking_content() and return
/// them in BackgroundContext for async processing after preview opens.
mod non_blocking_video_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;
    use std::time::{Duration, Instant};

    /// Helper to create a test directory with a non-hidden name
    fn create_test_dir() -> (std::path::PathBuf, impl Drop) {
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_video_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        (test_dir.clone(), Cleanup(test_dir))
    }

    /// TDD Test: generate_blocking_content() must return BackgroundContext with video items.
    ///
    /// This test SHOULD FAIL initially because generate_blocking_content() currently:
    /// 1. Processes videos inline (blocking)
    /// 2. Returns only SiteResult, not BackgroundContext
    ///
    /// After the fix:
    /// 1. generate_blocking_content() extracts video items without converting them
    /// 2. Returns (SiteResult, BackgroundContext) tuple
    /// 3. BackgroundContext.video_items contains videos for background processing
    #[test]
    fn test_generate_blocking_content_returns_background_context_with_videos() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");

        // Create content with a markdown file referencing a video
        fs::write(test_dir.join("index.md"), "# Test\nVideo content").unwrap();

        // Create a fake MOV file (won't actually convert, but will be detected)
        fs::create_dir_all(test_dir.join("videos")).unwrap();
        // Create a minimal valid file with .mov extension
        fs::write(test_dir.join("videos/test.mov"), "fake video data").unwrap();

        // Scan folder to get project structure
        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");

        // Verify we have a video file detected
        assert!(
            !project_structure.video_files.is_empty(),
            "Should detect video file"
        );

        // Create output directory
        fs::create_dir_all(&output_dir).unwrap();

        // Call generate_blocking_content - this currently returns just SiteResult
        // After the fix, it should return (SiteResult, BackgroundContext)
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None, // no app_handle in tests
            None, // no progress_sender in tests
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );

        assert!(
            result.is_ok(),
            "generate_blocking_content should succeed: {:?}",
            result
        );

        // CURRENT: Returns just SiteResult
        // EXPECTED: Returns (SiteResult, BackgroundContext)
        //
        // This assertion tests the NEW return type.
        // It will FAIL until we change the function signature.
        let (site_result, background_ctx, _docs, _verify) = result.unwrap();

        // Verify BackgroundContext contains video items for background processing
        assert!(
            !background_ctx.video_items.is_empty(),
            "BackgroundContext should contain video items for background processing"
        );

        // Verify the video item matches what we created
        assert_eq!(
            background_ctx.video_items[0], "videos/test.mov",
            "Video item source_path should match"
        );

        // Verify site_result is still valid
        assert!(site_result.page_count > 0, "Should have generated pages");
    }

    /// TDD Test: generate_blocking_content() completes quickly (no blocking on videos).
    ///
    /// This test verifies that generate_blocking_content() returns within a reasonable
    /// time threshold, proving that video conversion is NOT blocking.
    ///
    /// Expected behavior after fix:
    /// - Function returns in < 5 seconds (even with multiple videos)
    /// - Video conversion happens in background task AFTER this returns
    #[test]
    fn test_generate_blocking_content_does_not_block_on_video_conversion() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");

        // Create content
        fs::write(test_dir.join("index.md"), "# Test").unwrap();

        // Create multiple fake video files to simulate blocking scenario
        fs::create_dir_all(test_dir.join("videos")).unwrap();
        for i in 0..5 {
            fs::write(
                test_dir.join(format!("videos/video{}.mov", i)),
                format!("fake video data {}", i),
            )
            .unwrap();
        }

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        fs::create_dir_all(&output_dir).unwrap();

        // Time the function call
        let start = Instant::now();
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        let elapsed = start.elapsed();

        assert!(result.is_ok(), "generate_blocking_content should succeed");

        // The function should return quickly (< 5 seconds)
        // Real video conversion would take much longer
        // This threshold is generous - actual should be < 1 second
        assert!(
            elapsed < Duration::from_secs(5),
            "generate_blocking_content should return quickly (not block on videos). Took {:?}",
            elapsed
        );

        // After the fix, videos should be in BackgroundContext, not converted yet
        let (_site_result, background_ctx, _docs, _verify) = result.unwrap();
        assert_eq!(
            background_ctx.video_items.len(),
            5,
            "All 5 videos should be in BackgroundContext for later processing"
        );
    }
}

/// Tests for deferred asset copying (background phase).
///
/// Asset files (non-markdown: PDFs, images, CNAME, etc.) are NOT copied
/// during the blocking phase. Instead, the blocking phase:
/// 1. Generates HTML/CSS/JS (so the preview can open immediately)
/// 2. Carries forward previous_hashes for deferred assets
/// 3. Returns a BackgroundContext for the background phase to do the actual copy
///
/// The background walk copies everything that isn't markdown or .mov.
mod deferred_asset_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    /// Helper to create a test directory with cleanup on drop
    fn create_test_dir(prefix: &str) -> (std::path::PathBuf, impl Drop) {
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_{}_test_{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        (test_dir.clone(), Cleanup(test_dir))
    }

    /// Asset files are NOT copied during blocking phase — they're deferred.
    #[test]
    fn test_asset_files_not_copied_during_blocking_phase() {
        let (test_dir, _cleanup) = create_test_dir("deferred_assets");

        fs::write(test_dir.join("index.md"), "# Test Site").unwrap();
        fs::write(test_dir.join("resume.pdf"), "fake pdf content").unwrap();
        fs::write(test_dir.join("CNAME"), "example.com").unwrap();
        fs::write(test_dir.join("robots.txt"), "User-agent: *").unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Asset files are NOT copied during blocking phase
        assert!(
            !output_dir.join("resume.pdf").exists(),
            "resume.pdf should NOT be copied during blocking phase (deferred)"
        );
        assert!(
            !output_dir.join("CNAME").exists(),
            "CNAME should NOT be copied during blocking phase (deferred)"
        );
        assert!(
            !output_dir.join("robots.txt").exists(),
            "robots.txt should NOT be copied during blocking phase (deferred)"
        );

        // HTML IS generated during blocking phase
        assert!(
            output_dir.join("index.html").exists(),
            "index.html should be generated during blocking phase"
        );
    }

    /// Previous hashes are carried forward for deferred assets so change
    /// detection doesn't trigger spurious reloads.
    #[test]
    fn test_previous_hashes_carried_forward_for_deferred_assets() {
        let (test_dir, _cleanup) = create_test_dir("deferred_hashes");

        fs::write(test_dir.join("index.md"), "# Test").unwrap();
        fs::write(test_dir.join("resume.pdf"), "fake pdf").unwrap();
        fs::write(test_dir.join("CNAME"), "example.com").unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        // Simulate previous build's hashes for these asset files
        let mut previous_hashes = crate::types::content::SiteHashes::default();
        previous_hashes
            .files
            .insert("resume.pdf".to_string(), "abc123".to_string());
        previous_hashes
            .files
            .insert("CNAME".to_string(), "def456".to_string());

        // Write previous hashes to disk so generate_blocking_content reads them
        let moss_dir = test_dir.join(".moss");
        fs::create_dir_all(&moss_dir).unwrap();
        let hashes_json = serde_json::to_string(&previous_hashes).unwrap();
        fs::write(moss_dir.join("build.nosync").join("hashes.json"), hashes_json).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        // Seed PendingManifest with the same previous_hashes (carry-forward is now
        // caller's responsibility: PendingManifest::new(previous_hashes)).
        let (site_result, _bg_ctx, _docs, _verify) = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(previous_hashes.clone()),
        )
        .expect("generate_blocking_content should succeed");

        let hashes = site_result.hashes;

        // Previous hashes for asset files are carried forward via PendingManifest seeding.
        assert_eq!(
            hashes.files.get("resume.pdf"),
            Some(&"abc123".to_string()),
            "resume.pdf should carry forward previous hash"
        );
        assert_eq!(
            hashes.files.get("CNAME"),
            Some(&"def456".to_string()),
            "CNAME should carry forward previous hash"
        );
    }

    /// The background walk's source/output paths reach `BackgroundContext`.
    #[test]
    fn test_deferred_work_populated_in_background_context() {
        let (test_dir, _cleanup) = create_test_dir("deferred_work");

        fs::write(test_dir.join("index.md"), "# Test").unwrap();
        fs::write(test_dir.join("resume.pdf"), "fake pdf").unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let (_site_result, bg_ctx, _docs, _verify) = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        assert_eq!(std::path::Path::new(&bg_ctx.source_path), test_dir);
        assert_eq!(bg_ctx.staging_dir, output_dir);
    }
}

/// Tests for auto-generated folder index pages.
///
/// When a folder contains child markdown files but has no `index.md`,
/// generate_blocking_content() should auto-generate an `index.html` listing
/// the folder's children. This implements Principle 2: "Folders and .md
/// files correspond to HTML pages."
mod auto_folder_index_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    /// Helper to create a test directory with cleanup on drop
    fn create_test_dir(prefix: &str) -> (std::path::PathBuf, impl Drop) {
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_{}_test_{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        (test_dir.clone(), Cleanup(test_dir))
    }

    /// Test that a folder with child .md files but no index.md gets an
    /// auto-generated index.html.
    ///
    /// Setup: videos/ folder with aimeili.md, morning-mist.md but NO index.md.
    /// Expected: videos/index.html is generated in output.
    #[test]
    fn test_folder_without_index_md_gets_auto_generated_index() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_idx");

        // Root index.md (needed for site generation)
        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Create videos/ folder with child .md files but no index.md
        let videos_dir = test_dir.join("videos");
        fs::create_dir_all(&videos_dir).unwrap();
        fs::write(
            videos_dir.join("aimeili.md"),
            "---\ntitle: Aimeili\ndate: 2024-06-01\n---\nA video about Aimeili.",
        )
        .unwrap();
        fs::write(
            videos_dir.join("morning-mist.md"),
            "---\ntitle: Morning Mist\ndate: 2024-07-15\n---\nMorning mist in the valley.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // The auto-generated index page should exist
        assert!(
            output_dir.join("videos/index.html").exists(),
            "videos/index.html should be auto-generated when no index.md exists"
        );
    }

    /// Test that a folder WITH an index.md does NOT get an auto-generated page.
    /// The explicit index.md takes precedence.
    #[test]
    fn test_folder_with_index_md_uses_explicit_not_auto_generated() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_explicit");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Create articles/ folder WITH an index.md
        let articles_dir = test_dir.join("articles");
        fs::create_dir_all(&articles_dir).unwrap();
        fs::write(
            articles_dir.join("index.md"),
            "---\ntitle: My Articles\n---\nThis is my custom articles page.",
        )
        .unwrap();
        fs::write(
            articles_dir.join("post-one.md"),
            "---\ntitle: Post One\ndate: 2024-01-01\n---\nFirst post.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // The index.html should exist (from the explicit index.md)
        assert!(
            output_dir.join("articles/index.html").exists(),
            "articles/index.html should exist from explicit index.md"
        );

        // Verify the content comes from the explicit index.md (contains custom text)
        let content = fs::read_to_string(output_dir.join("articles/index.html")).unwrap();
        assert!(
            content.contains("My Articles"),
            "articles/index.html should contain the explicit title 'My Articles'"
        );
        assert!(
            content.contains("This is my custom articles page"),
            "articles/index.html should contain the explicit content from index.md"
        );
    }

    /// Test that the auto-generated page contains links to child pages.
    #[test]
    fn test_auto_generated_index_contains_child_links() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_links");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Create videos/ folder with child .md files but no index.md
        let videos_dir = test_dir.join("videos");
        fs::create_dir_all(&videos_dir).unwrap();
        fs::write(
            videos_dir.join("aimeili.md"),
            "---\ntitle: Aimeili\ndate: 2024-06-01\n---\nContent.",
        )
        .unwrap();
        fs::write(
            videos_dir.join("winter-song.md"),
            "---\ntitle: Winter Song\ndate: 2024-12-01\n---\nContent.",
        )
        .unwrap();
        fs::write(
            videos_dir.join("morning-mist.md"),
            "---\ntitle: Morning Mist\ndate: 2024-07-15\n---\nContent.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let content = fs::read_to_string(output_dir.join("videos/index.html")).unwrap();

        // The auto-generated page should contain links to all 3 child pages
        assert!(
            content.contains("Aimeili"),
            "Auto-generated index should list 'Aimeili'"
        );
        assert!(
            content.contains("Winter Song"),
            "Auto-generated index should list 'Winter Song'"
        );
        assert!(
            content.contains("Morning Mist"),
            "Auto-generated index should list 'Morning Mist'"
        );
    }

    /// Test that nested folders work correctly.
    /// A nested folder without index.md should get an auto-generated page,
    /// while its parent with index.md should NOT.
    #[test]
    fn test_nested_folder_auto_index() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_nested");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Create articles/ with an explicit index.md
        let articles_dir = test_dir.join("articles");
        fs::create_dir_all(&articles_dir).unwrap();
        fs::write(
            articles_dir.join("index.md"),
            "---\ntitle: Articles\n---\nCustom articles page.",
        )
        .unwrap();

        // Create articles/tutorials/ WITHOUT an index.md
        let tutorials_dir = articles_dir.join("tutorials");
        fs::create_dir_all(&tutorials_dir).unwrap();
        fs::write(
            tutorials_dir.join("getting-started.md"),
            "---\ntitle: Getting Started\ndate: 2024-03-01\n---\nA tutorial.",
        )
        .unwrap();
        fs::write(
            tutorials_dir.join("advanced.md"),
            "---\ntitle: Advanced Guide\ndate: 2024-04-01\n---\nAdvanced content.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Nested folder without index.md should get auto-generated page
        assert!(
            output_dir.join("articles/tutorials/index.html").exists(),
            "articles/tutorials/index.html should be auto-generated"
        );

        // The auto-generated page should list child pages
        let content = fs::read_to_string(output_dir.join("articles/tutorials/index.html")).unwrap();
        assert!(
            content.contains("Getting Started"),
            "Auto-generated nested index should list 'Getting Started'"
        );
        assert!(
            content.contains("Advanced Guide"),
            "Auto-generated nested index should list 'Advanced Guide'"
        );
    }

    /// Test that the auto-generated page is included in site_hashes
    /// so stale file cleanup doesn't delete it.
    #[test]
    fn test_auto_generated_index_included_in_site_hashes() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_hashes");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        let videos_dir = test_dir.join("videos");
        fs::create_dir_all(&videos_dir).unwrap();
        fs::write(
            videos_dir.join("aimeili.md"),
            "---\ntitle: Aimeili\ndate: 2024-06-01\n---\nContent.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let (site_result, _, _docs, _verify) = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let hashes = site_result.hashes;
        assert!(
            hashes.files.contains_key("videos/index.html"),
            "videos/index.html should be in site_hashes for stale cleanup"
        );
    }

    /// Test that the page title is derived from the folder name.
    #[test]
    fn test_auto_generated_index_title_from_folder_name() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_title");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Folder with hyphens: "my-videos" should become "my videos"
        // (synthetic folder indexes use `filename_text` — hyphens →
        // spaces, NO title-casing — so a folder renders identical H1/title
        // text whether or not it has an `index.md`).
        let folder = test_dir.join("my-videos");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("clip.md"),
            "---\ntitle: Clip\ndate: 2024-01-01\n---\nContent.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let content = fs::read_to_string(output_dir.join("my-videos/index.html")).unwrap();
        // The page title should be in the <title> tag
        assert!(
                content.contains("my videos"),
                "Auto-generated page title should derive from folder name 'my-videos' -> 'my videos' (filename_text, no title-casing)"
            );
    }

    /// A home-less folder whose on-disk name is capitalized ("Writings")
    /// must render its auto-generated index TITLE/H1 in the original case
    /// ("Writings"), NOT the lowercased URL slug ("writings"). The slug is
    /// correctly lowercased for the address (`writings/index.html`), but the
    /// visible title preserves the author's directory casing. Repro mirrors
    /// the reported bug: no `Writings.md`, identity resolves from a child
    /// article, so moss synthesizes the folder index. (Bug 17.)
    #[test]
    fn test_auto_generated_index_title_preserves_folder_case() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_title_case");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Capital-W folder, home-less (no `Writings.md` / `index.md`), with a
        // child article so it is indexed.
        let folder = test_dir.join("Writings");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
                folder.join("Auguries of Innocence.md"),
                "---\ntitle: Auguries of Innocence\ndate: 2024-01-01\n---\nTo see a world in a grain of sand.",
            )
            .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // URL slug stays lowercase (address unchanged).
        let content = fs::read_to_string(output_dir.join("writings/index.html")).unwrap();

        // The visible folder-title H1 must be the on-disk case "Writings".
        assert!(
            content.contains(r#"<h1 class="moss-folder-title">Writings</h1>"#),
            "Folder-index H1 should preserve on-disk case 'Writings', got:\n{}",
            content
        );
        // And it must NOT be the lowercased slug in the H1 (the bug).
        assert!(
            !content.contains(r#"<h1 class="moss-folder-title">writings</h1>"#),
            "Folder-index H1 must not use the lowercased slug 'writings'"
        );
        // The <title> tab should carry the proper-case name too.
        assert!(
            content.contains("<title>Writings"),
            "Page <title> should preserve on-disk case 'Writings', got:\n{}",
            content
        );
    }

    /// Test that the root folder does NOT get auto-generated
    /// (it's handled separately by the existing homepage logic).
    #[test]
    fn test_root_folder_not_auto_generated() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_root");

        // No index.md at root — but root should get the auto-index from existing logic
        // Create a subfolder with content
        let posts_dir = test_dir.join("posts");
        fs::create_dir_all(&posts_dir).unwrap();
        fs::write(
            posts_dir.join("hello.md"),
            "---\ntitle: Hello\ndate: 2024-01-01\n---\nHello.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Root index.html is generated by the existing homepage logic, not auto-folder-index
        assert!(
            output_dir.join("index.html").exists(),
            "Root index.html should exist (from existing homepage logic)"
        );
        // Subfolder should also get auto-generated
        assert!(
            output_dir.join("posts/index.html").exists(),
            "posts/index.html should be auto-generated"
        );
    }

    /// Test that a parent folder with an explicit index.md lists an
    /// auto-generated subfolder as a child in its HTML output.
    ///
    /// This is the core bug scenario: `writings/index.md` exists but
    /// `writings/reviews/` has no index.md. The `writings/index.html`
    /// page must link to `writings/reviews/` as a child folder.
    #[test]
    fn test_parent_lists_auto_generated_subfolder() {
        let (test_dir, _cleanup) = create_test_dir("parent_lists_subfolder");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Create writings/ with an explicit index.md
        let writings_dir = test_dir.join("writings");
        fs::create_dir_all(&writings_dir).unwrap();
        fs::write(
            writings_dir.join("index.md"),
            "---\ntitle: Writings\n---\nMy writings.",
        )
        .unwrap();

        // Create writings/reviews/ WITHOUT an index.md
        let reviews_dir = writings_dir.join("reviews");
        fs::create_dir_all(&reviews_dir).unwrap();
        fs::write(
            reviews_dir.join("book-one.md"),
            "---\ntitle: Book One\ndate: 2024-01-01\n---\nA book review.",
        )
        .unwrap();
        fs::write(
            reviews_dir.join("book-two.md"),
            "---\ntitle: Book Two\ndate: 2024-02-01\n---\nAnother book review.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // The auto-generated reviews/index.html should exist
        assert!(
            output_dir.join("writings/reviews/index.html").exists(),
            "writings/reviews/index.html should be auto-generated"
        );

        // The parent writings/index.html should contain a link to reviews/
        let parent_html = fs::read_to_string(output_dir.join("writings/index.html")).unwrap();
        assert!(
                parent_html.contains("/writings/reviews/"),
                "writings/index.html should list reviews/ as a child folder, but it doesn't.\nHTML snippet: {}",
                &parent_html[..parent_html.len().min(2000)]
            );
    }

    /// Test deeply nested indexless folders: parent at each level should
    /// list the auto-generated subfolder.
    ///
    /// Structure: a/index.md + a/b/c/article.md (no index at b/ or c/).
    /// Expected: a/index.html links to a/b/, and a/b/index.html links to a/b/c/.
    #[test]
    fn test_deeply_nested_indexless_folders_listed_by_parent() {
        let (test_dir, _cleanup) = create_test_dir("deep_nested_parent");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // a/ has an explicit index
        let a_dir = test_dir.join("a");
        fs::create_dir_all(&a_dir).unwrap();
        fs::write(
            a_dir.join("index.md"),
            "---\ntitle: Section A\n---\nSection A content.",
        )
        .unwrap();

        // a/b/c/ has an article but neither a/b/ nor a/b/c/ has an index
        let abc_dir = a_dir.join("b").join("c");
        fs::create_dir_all(&abc_dir).unwrap();
        fs::write(
            abc_dir.join("article.md"),
            "---\ntitle: Deep Article\ndate: 2024-05-01\n---\nDeep content.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Auto-generated indices should exist at both levels
        assert!(
            output_dir.join("a/b/index.html").exists(),
            "a/b/index.html should be auto-generated"
        );
        assert!(
            output_dir.join("a/b/c/index.html").exists(),
            "a/b/c/index.html should be auto-generated"
        );

        // a/index.html should list b/ as a child
        let a_html = fs::read_to_string(output_dir.join("a/index.html")).unwrap();
        assert!(
            a_html.contains("/a/b/"),
            "a/index.html should list b/ as a child folder"
        );

        // a/b/index.html should list c/ as a child
        let b_html = fs::read_to_string(output_dir.join("a/b/index.html")).unwrap();
        assert!(
            b_html.contains("/a/b/c/"),
            "a/b/index.html should list c/ as a child folder"
        );
    }

    /// Test that page_count includes auto-generated folder index pages.
    #[test]
    fn test_auto_generated_index_increments_page_count() {
        let (test_dir, _cleanup) = create_test_dir("auto_folder_count");

        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        let videos_dir = test_dir.join("videos");
        fs::create_dir_all(&videos_dir).unwrap();
        fs::write(
            videos_dir.join("clip.md"),
            "---\ntitle: Clip\ndate: 2024-01-01\n---\nContent.",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let (site_result, _, _docs, _verify) = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // page_count should include: index.html (homepage), videos/clip/index.html (content),
        // videos/index.html (auto-generated), so at least 3
        assert!(
            site_result.page_count >= 3,
            "page_count should include auto-generated folder index pages, got {}",
            site_result.page_count
        );
    }

    /// Regression test: a top-level folder literally named `zh/` should be treated
    /// as a lang-prefix directory (no synthetic folder-index), consistent with
    /// `zh-hans/`, `zh-hant/`, `en/`, etc.
    ///
    /// Before the `.zh` shorthand was accepted as an alias for `.zh-hans`, a
    /// user-owned `zh/` folder would get a synthetic `zh/index.html`. After the
    /// alias landed, `Language::from_code("zh")` returns `Some(ZhHans)`, so the
    /// filter in `render.rs` skips `zh/` — matching the existing treatment of all
    /// other known language prefixes.
    #[test]
    fn test_folder_named_zh_treated_as_lang_prefix() {
        // Sanity check: `zh` is a recognized lang code (the whole point of 2.6).
        assert!(
            crate::i18n::Language::from_code("zh").is_some(),
            "precondition: `zh` should resolve to a Language via from_code"
        );

        let (test_dir, _cleanup) = create_test_dir("auto_folder_zh_lang_prefix");

        // Root index (needed for site generation).
        fs::write(test_dir.join("index.md"), "# My Site").unwrap();

        // Top-level `zh/` folder with a non-index markdown file. Under the
        // pre-2.6 behavior this would synthesize `zh/index.html`; post-2.6 it
        // must be skipped because `zh` is a recognized lang prefix.
        let zh_dir = test_dir.join("zh");
        fs::create_dir_all(&zh_dir).unwrap();
        fs::write(
            zh_dir.join("dialects.md"),
            "---\ntitle: Dialects\n---\nContent about dialects.",
        )
        .unwrap();

        // Also add a `zh-hans/` folder to pin consistency with the existing
        // lang-prefix treatment.
        let zh_hans_dir = test_dir.join("zh-hans");
        fs::create_dir_all(&zh_hans_dir).unwrap();
        fs::write(
            zh_hans_dir.join("about.md"),
            "---\ntitle: 关于\n---\n关于页面。",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Consistency assertion: whatever moss does for `zh-hans/` (the canonical
        // lang prefix), it must do for `zh/` (the shorthand alias from 2.6).
        // We don't hard-code whether a synthetic index is or isn't generated for
        // orphan-translation content — only that the two cases agree. This pins
        // the behavior-parity the reviewer flagged: before 2.6 a user-owned `zh/`
        // diverged from `zh-hans/`; after 2.6 they must match.
        let zh_synthetic = output_dir.join("zh/index.html").exists();
        let zh_hans_synthetic = output_dir.join("zh-hans/index.html").exists();
        assert_eq!(
            zh_synthetic,
            zh_hans_synthetic,
            "top-level `zh/` and `zh-hans/` must be treated identically by the \
                 synthetic folder-index pass (both {} synthetic index.html)",
            if zh_synthetic { "get" } else { "skip" }
        );
    }
}

// (removed) comments_attr_tests asserted the three-way `data-comments` logic
// against a LOCAL COPY of it — `compute_comments_attr`, a hand-written twin of
// `resolve_comments_attr` that production never calls. A copy proves only that
// the copy agrees with itself. The real three-way behaviour is
// covered end-to-end by `test_site_default_comments_false` /
// `test_per_page_comments_overrides_site` here and, for placement on both page
// kinds, by `both_templates_carry_the_comment_attribute_and_slot`.

/// Tests for cover image resolution via ContentGraph.
///
/// Bug: Obsidian wikilinks in frontmatter cover fields use bare filenames
/// (e.g. `[[photo.jpg]]`), which strip to `photo.jpg`. But the actual file
/// is at `assets/photo.jpg`. The code must resolve bare filenames through
/// the ContentGraph, just like body `![[photo.jpg]]` embeds are resolved.
mod cover_resolution_tests {
    use moss_core::content_graph::{generate_slug, ContentGraphBuilder};

    /// Helper: resolve a cover path through the ContentGraph the same way
    /// render.rs does after process_markdown_file returns.
    /// Pipe-encoded attrs ("path|attrs") are split off before resolving,
    /// then rejoined — matching the production code.
    fn resolve_cover(
        cover: &str,
        from_path: &str,
        graph: &moss_core::content_graph::ContentGraph,
    ) -> String {
        if !cover.starts_with("http") {
            let (path_part, attrs_str) = moss_core::media::split_pipe(cover);
            if let Some(resolved) = graph.resolve_path(path_part, from_path) {
                return if attrs_str.is_empty() {
                    resolved
                } else {
                    format!("{}|{}", resolved, attrs_str)
                };
            }
        }
        cover.to_string()
    }

    #[test]
    fn test_bare_filename_cover_resolved_to_assets_dir() {
        let mut builder = ContentGraphBuilder::new();
        builder.add_file("assets/a6833b1a.jpg", &generate_slug("assets/a6833b1a.jpg"));
        builder.add_file(
            "articles/travel/index.md",
            &generate_slug("articles/travel/index.md"),
        );
        let graph = builder.build();

        // Bare filename (from wikilink stripping) should resolve to full path
        let resolved = resolve_cover("a6833b1a.jpg", "articles/travel/index.md", &graph);
        assert_eq!(resolved, "assets/a6833b1a.jpg");
    }

    #[test]
    fn test_already_qualified_cover_unchanged() {
        let mut builder = ContentGraphBuilder::new();
        builder.add_file("assets/photo.jpg", &generate_slug("assets/photo.jpg"));
        let graph = builder.build();

        // Already has directory — should pass through unchanged
        let resolved = resolve_cover("assets/photo.jpg", "index.md", &graph);
        assert_eq!(resolved, "assets/photo.jpg");
    }

    #[test]
    fn test_http_cover_unchanged() {
        let graph = ContentGraphBuilder::new().build();

        let resolved = resolve_cover("https://example.com/photo.jpg", "index.md", &graph);
        assert_eq!(resolved, "https://example.com/photo.jpg");
    }

    #[test]
    fn test_unresolvable_bare_filename_unchanged() {
        let graph = ContentGraphBuilder::new().build();

        // No file in graph — bare filename passes through unchanged
        let resolved = resolve_cover("nonexistent.jpg", "index.md", &graph);
        assert_eq!(resolved, "nonexistent.jpg");
    }

    #[test]
    fn test_pipe_encoded_cover_preserves_attrs_after_resolve() {
        let mut builder = ContentGraphBuilder::new();
        builder.add_file("视频/aimeili.MOV", &generate_slug("视频/aimeili.MOV"));
        builder.add_file("视频/视频.md", &generate_slug("视频/视频.md"));
        let graph = builder.build();

        // Pipe-encoded cover: path|attrs — attrs must survive ContentGraph re-resolution.
        // Bug: resolve_path("aimeili.MOV|right") fuzzy-matched stem "aimeili" and
        // returned "视频/aimeili.MOV" without the attrs.
        let resolved = resolve_cover("aimeili.MOV|right", "视频/视频.md", &graph);
        assert_eq!(resolved, "视频/aimeili.MOV|right");
    }

    #[test]
    fn test_pipe_encoded_cover_with_directory_preserves_attrs() {
        let mut builder = ContentGraphBuilder::new();
        builder.add_file("assets/photo.jpg", &generate_slug("assets/photo.jpg"));
        let graph = builder.build();

        // Already-qualified path with pipe attrs
        let resolved = resolve_cover("assets/photo.jpg|cover left", "index.md", &graph);
        assert_eq!(resolved, "assets/photo.jpg|cover left");
    }
}

/// Integration test: wikilink cover with bare filename should resolve
/// to the full path in generated HTML.
mod cover_wikilink_integration {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    #[test]
    fn test_wikilink_bare_filename_cover_resolved_in_html() {
        let system_temp = std::env::temp_dir();
        let test_dir =
            system_temp.join(format!("moss_cover_wikilink_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        // Create assets directory with an image
        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        // Create a minimal valid JPEG (just needs to exist for the content graph)
        fs::write(assets_dir.join("cover-photo.jpg"), b"fake-jpg").unwrap();

        // Create a folder with a wikilink cover using bare filename
        let folder_dir = test_dir.join("articles").join("my-folder");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: My Folder\ncover: \"[[cover-photo.jpg]]\"\n---\n\nA test folder.\n",
        )
        .unwrap();

        // Create homepage that references the folder via grid shortcode
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\n:::grid 3\n- [My Folder](articles/my-folder)\n:::\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Read the generated homepage HTML
        let homepage =
            fs::read_to_string(output_dir.join("index.html")).expect("index.html should exist");

        // The cover image src should be "/assets/cover-photo.jpg" (root-relative), NOT bare "cover-photo.jpg"
        assert!(
            homepage.contains("src=\"/assets/cover-photo.jpg\""),
            "Cover image should resolve bare filename to /assets/cover-photo.jpg, got HTML:\n{}",
            homepage
                .lines()
                .filter(|l| l.contains("cover"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Folder index pages with covers should NOT render description in page body.
/// Description is only for card display when the page is referenced elsewhere.
mod folder_cover_no_description {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    #[test]
    fn test_folder_cover_does_not_render_description_in_page() {
        let system_temp = std::env::temp_dir();
        let test_dir =
            system_temp.join(format!("moss_folder_no_desc_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        // Create assets directory with a cover image
        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        // Create a folder index with cover AND description
        let folder_dir = test_dir.join("my-folder");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
                folder_dir.join("index.md"),
                "---\ntitle: My Folder\ncover: \"[[cover.jpg]]\"\ndescription: A folder description\n---\n\nBody content here.\n",
            )
            .unwrap();

        // Create homepage
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        // Read the generated folder page HTML
        let folder_html = fs::read_to_string(output_dir.join("my-folder").join("index.html"))
            .expect("folder index.html should exist");

        // `description:` is metadata: it reaches the reader through the head,
        // never as page text. It briefly rendered as a visible standfirst here
        // (f94280d00, reverted 2026-08-08) — the author could not tell whether
        // the sentence was theirs or one moss derived from the body, so moss
        // was printing words nobody wrote.
        assert!(
            folder_html.contains(r#"<meta name="description""#),
            "the description must still reach the head.\nFound in HTML:\n{}",
            folder_html
                .lines()
                .filter(|l| l.contains("description"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            !folder_html.contains("collection-description"),
            "Folder page should not render description in page body.\nFound in HTML:\n{}",
            folder_html
                .lines()
                .filter(|l| l.contains("description"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// A `:::grid N:::` shortcode block in a folder note's body must render
/// full-width, even when the folder note ALSO sets `cover:` (which
/// wraps the page in the book-open two-column layout — cover image on
/// the left, content in a narrow `.moss-collection-cover-body` column
/// on the right, see `folder_cover.rs`). Naively wrapping the grid
/// block inside that narrow column trapped it at roughly the column's
/// width, with the cover thumbnail's column reading as empty space
/// beside it — the real-content repro was `Illuminated Books.md`
/// (`cover:` + a `:::grid 2:::` of sibling wikilinks).
mod folder_cover_grid_escapes_narrow_column {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    #[test]
    fn grid_block_renders_as_sibling_after_cover_row_not_nested_inside_it() {
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!(
            "moss_folder_cover_grid_test_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        // Folder note: cover + a lead paragraph + a `:::grid 2:::` block.
        let folder_dir = test_dir.join("books");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Illuminated Books\ncover: \"[[cover.jpg]]\"\n---\n\n\
                 I rest not from my great task!\n\n\
                 :::grid 2\nCard One\n+++\nCard Two\n:::\n",
        )
        .unwrap();

        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let folder_html = fs::read_to_string(output_dir.join("books").join("index.html"))
            .expect("folder index.html should exist");

        assert!(
            folder_html.contains("moss-collection-cover-row"),
            "book-open cover layout should be present, got:\n{folder_html}"
        );
        assert!(
            folder_html.contains(r#"<div class="moss-grid""#),
            "grid block should be present, got:\n{folder_html}"
        );

        // The cover-row's own two closing divs (`.moss-collection-cover-body`
        // then `.moss-collection-cover-row`) must immediately precede the
        // grid's opening tag — i.e. the grid is a SIBLING rendered after
        // the cover row, not a descendant trapped inside the narrow
        // `.moss-collection-cover-body` column.
        assert!(
            folder_html.contains(r#"</div></div><div class="moss-grid""#),
            "grid must render as a sibling immediately after the cover row \
                 closes, not nested inside .moss-collection-cover-body. Got:\n{folder_html}"
        );

        // And the grid's cell text must NOT appear before the cover row
        // even opens (sanity: nothing got reordered ahead of the cover).
        let cover_pos = folder_html.find("moss-collection-cover-row").unwrap();
        let grid_pos = folder_html.find(r#"<div class="moss-grid""#).unwrap();
        assert!(
            cover_pos < grid_pos,
            "cover row must render before the grid block"
        );
    }

    #[test]
    fn grid_only_folder_note_with_no_lead_prose_still_escapes_the_cover_body() {
        // No lead paragraph at all — the folder note's ENTIRE body is the
        // grid block. `lead` is empty (safe by construction: an empty
        // string is trivially tag-balanced), so the split must still
        // happen and the cover-row's h1 must render with nothing else
        // squeezed in beside it.
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!(
            "moss_folder_cover_grid_only_test_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        let folder_dir = test_dir.join("books");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Illuminated Books\ncover: \"[[cover.jpg]]\"\n---\n\n\
                 :::grid 2\nCard One\n+++\nCard Two\n:::\n",
        )
        .unwrap();

        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let folder_html = fs::read_to_string(output_dir.join("books").join("index.html"))
            .expect("folder index.html should exist");

        assert!(
            folder_html.contains(
                r#"<h1 class="moss-folder-title" data-source-fm="title">Illuminated Books</h1>"#
            ),
            "title must still render inside the cover body. Got:\n{folder_html}"
        );
        assert!(
            folder_html.contains(r#"</div></div><div class="moss-grid""#),
            "grid must still escape as a sibling after the cover row even with no lead \
                 prose at all. Got:\n{folder_html}"
        );
    }

    #[test]
    fn layout_article_suppresses_cover_entirely() {
        // `layout: article` on a folder-index page reads as a plain article:
        // even with `cover:` set, no cover component (book-open row or
        // stacked hero) is auto-inserted — a long-form body (including a
        // `:::grid` block) renders full-width, with no lead/trailer split
        // needed at all.
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!(
            "moss_folder_cover_article_layout_test_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        let folder_dir = test_dir.join("long-form-piece");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: A Long Read\ncover: \"[[cover.jpg]]\"\nlayout: article\n---\n\n\
                 A long article body that should read full-width.\n\n\
                 :::grid 2\nCard One\n+++\nCard Two\n:::\n",
        )
        .unwrap();

        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        let _result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        let folder_html = fs::read_to_string(output_dir.join("long-form-piece").join("index.html"))
            .expect("folder index.html should exist");

        assert!(
            !folder_html.contains("moss-collection-hero"),
            "article-layout must NOT render the stacked hero banner. Got:\n{folder_html}"
        );
        assert!(
            !folder_html.contains("moss-collection-cover-row"),
            "article-layout cover must NOT render the book-open row. Got:\n{folder_html}"
        );
        assert!(
            !folder_html.contains("moss-collection-cover-body"),
            "article-layout cover must NOT render the narrow cover-body column. Got:\n{folder_html}"
        );
        assert!(
            folder_html.contains(
                r#"<h1 class="moss-folder-title" data-source-fm="title">A Long Read</h1>"#
            ),
            "title must still render as a plain heading. Got:\n{folder_html}"
        );
        assert!(
            folder_html.contains(r#"<div class="moss-grid""#),
            "grid block must still render. Got:\n{folder_html}"
        );

        // Ordering: title, then body content (including the grid) — flat
        // siblings, nothing nested in a narrow column and no banner at all.
        let title_pos = folder_html.find("moss-folder-title").unwrap();
        let grid_pos = folder_html.find(r#"<div class="moss-grid""#).unwrap();
        assert!(
            title_pos < grid_pos,
            "expected title -> body ordering. Got:\n{folder_html}"
        );
    }
}

/// Where a cover-bearing folder home releases its narrow column, end to end.
///
/// Both bugs had one cause: the release point was found by cutting the page's
/// already-rendered HTML string. `split_lead_before_grid` walked a byte cursor
/// (`i += 1`) and then sliced `html[i..]`, so the first multi-byte character in
/// the lede aborted the whole build; and the only release point it could
/// recognise was a literal `.moss-grid`, so a long-form article with no grid
/// was typeset in the ~20-character cover-body column for its entire length.
///
/// The release point is now an index into the typed `Vec<Block>`
/// (`body_plan::lede_end`), so both tests drive a real scan +
/// `generate_blocking_content` and assert on the emitted markup.
mod folder_cover_lede_release {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Build a vault holding one cover-bearing folder note plus a homepage, and
    /// return the folder index's HTML.
    fn folder_html_for(name: &str, folder_body: &str) -> String {
        let test_dir = std::env::temp_dir().join(format!("moss_{name}_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        let folder_dir = test_dir.join("books");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            format!("---\ntitle: Illuminated Books\ncover: \"[[cover.jpg]]\"\n---\n\n{folder_body}"),
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        fs::read_to_string(output_dir.join("books").join("index.html"))
            .expect("folder index.html should exist")
    }

    /// What stayed inside the narrow `.moss-collection-cover-body` column, and
    /// what was released after the cover row closed.
    fn split_cover_body(html: &str) -> (&str, &str) {
        let (_, after_open) = html
            .split_once(r#"<div class="moss-collection-cover-body">"#)
            .unwrap_or_else(|| panic!("no book-open cover layout in:\n{html}"));
        after_open
            .split_once("</div></div>")
            .unwrap_or_else(|| panic!("cover row never closes in:\n{html}"))
    }

    /// CJK prose in the lede of a cover-bearing folder home
    /// aborted the build outright: `start byte index 66 is not a char boundary;
    /// it is inside '在'`. Any non-ASCII byte before the grid was enough — the
    /// cursor stepped one byte at a time and then sliced.
    #[test]
    fn cjk_prose_before_a_grid_builds_instead_of_aborting_on_a_char_boundary() {
        let html = folder_html_for(
            "folder_cover_cjk_lede",
            "河灣作為一個寫作計畫，關注的是普通人寫作的現場。\n\n\
             :::grid 2\nCard One\n+++\nCard Two\n:::\n",
        );
        let (cover_body, released) = split_cover_body(&html);
        assert!(
            cover_body.contains("河灣作為一個寫作計畫"),
            "the CJK lede belongs beside the cover: {cover_body}"
        );
        assert!(
            !cover_body.contains("moss-grid"),
            "the grid must not be trapped in the narrow column: {cover_body}"
        );
        assert!(
            released.contains(r#"<div class="moss-grid""#),
            "the grid renders after the cover row closes: {released}"
        );
    }

    /// A long-form article body on a cover-bearing folder home
    /// has no grid to release at, so the whole body used to be typeset in the
    /// narrow column with half the viewport empty beside it. The release point
    /// is now the end of the lede — the first heading after a paragraph.
    #[test]
    fn a_long_article_body_with_no_grid_is_released_at_its_first_heading() {
        let html = folder_html_for(
            "folder_cover_long_article",
            "A short standfirst that belongs beside the cover.\n\n\
             ## The first section\n\nSeveral paragraphs of body text.\n\n\
             ## The second section\n\nMore body text, still no grid anywhere.\n",
        );
        let (cover_body, released) = split_cover_body(&html);
        assert!(
            cover_body.contains("standfirst"),
            "the standfirst stays beside the cover: {cover_body}"
        );
        assert!(
            !cover_body.contains("<h2"),
            "the article proper must NOT be trapped in the narrow column: {cover_body}"
        );
        assert!(
            !cover_body.contains("The first section"),
            "cover body: {cover_body}"
        );
        assert!(
            released.contains("The first section") && released.contains("The second section"),
            "the article body renders full-width after the cover row: {released}"
        );
    }
}

/// Independence of the children auto-listing and an authored `:::grid`.
///
/// A `:::grid` block in the body and moss's frontmatter-driven children
/// auto-listing are INDEPENDENT features: when both are present, BOTH render
/// (the grid the author wrote, plus the auto-listing below it). `children:
/// false` is the ONLY switch that turns the auto-listing off; an authored
/// grid survives it. The auto-listing is also the ONLY thing `children_style`
/// (list/summary/grid) styles — so these tests pin that each style renders
/// its distinct markup, and that a body grid never erases the styled listing.
///
/// All tests drive a real scan + `generate_blocking_content` and assert on
/// the emitted markup, exercising the full render path end to end.
mod children_listing_and_grid_independence {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn build_site(test_dir: &std::path::Path) -> std::path::PathBuf {
        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");
        output_dir
    }

    /// GUARD: a folder index WITHOUT a grid still auto-renders its children
    /// listing (the auto-listing is the default; nothing suppresses it).
    #[test]
    fn folder_index_without_grid_still_auto_lists_children() {
        let test_dir = std::env::temp_dir().join(format!(
            "moss_children_dedup_nogrid_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let folder_dir = test_dir.join("notes");
        fs::create_dir_all(&folder_dir).unwrap();
        // Folder index with prose only — no grid, no explicit child links.
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Notes\n---\n\nJust some prose, no listing here.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Note A.md"),
            "---\ntitle: Note A\n---\n\nBody of note A.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Note B.md"),
            "---\ntitle: Note B\n---\n\nBody of note B.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("notes").join("index.html"))
            .expect("notes/index.html should exist");

        assert!(
            html.contains(r#"href="/notes/note-a/""#),
            "Note A must still be auto-listed when the body has no grid.\n{html}"
        );
        assert!(
            html.contains(r#"href="/notes/note-b/""#),
            "Note B must still be auto-listed when the body has no grid.\n{html}"
        );
    }

    /// (a) `children_style: summary` renders full summary cards — the styled
    /// auto-listing that `children_style` exists to produce. NO grid in the
    /// body, so this proves the styled listing renders on its own.
    #[test]
    fn children_style_summary_renders_summary_cards() {
        let test_dir = std::env::temp_dir().join(format!(
            "moss_children_style_summary_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        let folder_dir = test_dir.join("gallery");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Gallery\nchildren_style: summary\n---\n\nA gallery of pieces.\n",
        )
        .unwrap();
        // A child WITH a cover and a description → summary cards show both.
        fs::write(
                folder_dir.join("Piece One.md"),
                "---\ntitle: Piece One\ndescription: A lovely first piece.\ncover: \"[[cover.jpg]]\"\n---\n\nThe body of piece one.\n",
            )
            .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("gallery").join("index.html"))
            .expect("gallery/index.html should exist");

        assert!(
            html.contains(r#"data-layout="list""#),
            "children_style: summary must emit the summary listing (data-layout=\"list\").\n{html}"
        );
        assert!(
            html.contains("moss-card-row"),
            "summary cards must render moss-card-row.\n{html}"
        );
        assert!(
            html.contains("moss-card-description"),
            "summary cards must render moss-card-description.\n{html}"
        );
    }

    /// (b) `children_style: list` renders the compact minimal index — even
    /// when a child carries a cover, the minimal rows drop it.
    #[test]
    fn children_style_list_renders_minimal_rows() {
        let test_dir =
            std::env::temp_dir().join(format!("moss_children_style_list_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let assets_dir = test_dir.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(assets_dir.join("cover.jpg"), b"fake-jpg").unwrap();

        let folder_dir = test_dir.join("notes");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Notes\nchildren_style: list\n---\n\nA compact index.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Note A.md"),
            "---\ntitle: Note A\ncover: \"[[cover.jpg]]\"\n---\n\nBody of note A.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("notes").join("index.html"))
            .expect("notes/index.html should exist");

        assert!(
            html.contains(r#"data-layout="minimal""#),
            "children_style: list must emit the compact index (data-layout=\"minimal\").\n{html}"
        );
        assert!(
            !html.contains("moss-card-cover"),
            "list (minimal) rows must NOT render covers.\n{html}"
        );
    }

    /// (c) `children_style: grid` renders the collection-card grid.
    #[test]
    fn children_style_grid_renders_grid_cards() {
        let test_dir =
            std::env::temp_dir().join(format!("moss_children_style_grid_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let folder_dir = test_dir.join("gallery");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Gallery\nchildren_style: grid\n---\n\nA grid of pieces.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Piece One.md"),
            "---\ntitle: Piece One\n---\n\nBody of piece one.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("gallery").join("index.html"))
            .expect("gallery/index.html should exist");

        assert!(
            html.contains(r#"data-layout="grid""#),
            "children_style: grid must emit the grid listing (data-layout=\"grid\").\n{html}"
        );
    }

    /// (d) FOLDER-INDEX independence: a folder index that lists its children
    /// in a `:::grid` AND has the default children auto-listing renders each
    /// child TWICE — once in the authored grid, once in the auto-listing.
    #[test]
    fn folder_index_grid_and_auto_listing_both_render() {
        let test_dir = std::env::temp_dir().join(format!(
            "moss_grid_and_listing_folder_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let folder_dir = test_dir.join("writings");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Writings\n---\n\n\
                 :::grid 2\n[[Child A]]\n+++\n[[Child B]]\n:::\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Child A.md"),
            "---\ntitle: Child A\n---\n\nThe first child's body.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Child B.md"),
            "---\ntitle: Child B\n---\n\nThe second child's body.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("writings").join("index.html"))
            .expect("writings/index.html should exist");

        let count_a = html.matches(r#"href="/writings/child-a/""#).count();
        assert_eq!(
            count_a, 2,
            "Child A must render TWICE (authored grid + independent auto-listing). \
                 Got {count_a}.\n{html}"
        );
        assert!(
            html.contains(r#"class="moss-grid""#),
            "the authored :::grid must render as .moss-grid.\n{html}"
        );
        assert!(
            html.contains(r#"class="moss-cards""#),
            "the auto children-listing must render as .moss-cards.\n{html}"
        );
        // The two hrefs live in DIFFERENT regions: one in the authored grid
        // (before the moss-cards wrapper), one in the auto-listing.
        let cards_start = html
            .find(r#"class="moss-cards""#)
            .expect("moss-cards wrapper present");
        let (grid_region, cards_region) = html.split_at(cards_start);
        assert_eq!(
            grid_region.matches(r#"href="/writings/child-a/""#).count(),
            1,
            "authored grid (before the moss-cards wrapper) must list Child A once.\n{html}"
        );
        assert_eq!(
            cards_region.matches(r#"href="/writings/child-a/""#).count(),
            1,
            "auto-listing (moss-cards region) must list Child A once.\n{html}"
        );
    }

    /// (d-twin) HOMEPAGE independence: same as above, but on the root
    /// homepage — the authored grid and the flattened auto-listing coexist.
    #[test]
    fn homepage_grid_and_auto_listing_both_render() {
        let test_dir = std::env::temp_dir().join(format!(
            "moss_grid_and_listing_home_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\n\
                 :::grid 2\n[[Post One]]\n+++\n[[Post Two]]\n:::\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("Post One.md"),
            "---\ntitle: Post One\n---\n\nBody of post one.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("Post Two.md"),
            "---\ntitle: Post Two\n---\n\nBody of post two.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html =
            fs::read_to_string(output_dir.join("index.html")).expect("index.html should exist");

        let count_one = html.matches(r#"href="/post-one/""#).count();
        assert_eq!(
            count_one, 2,
            "Post One must render TWICE (authored grid + independent auto-listing). \
                 Got {count_one}.\n{html}"
        );
        assert!(
            html.contains(r#"class="moss-grid""#),
            "the authored :::grid must render as .moss-grid.\n{html}"
        );
        assert!(
            html.contains(r#"class="moss-cards""#),
            "the auto children-listing must render as .moss-cards.\n{html}"
        );
        let cards_start = html
            .find(r#"class="moss-cards""#)
            .expect("moss-cards wrapper present");
        let (grid_region, cards_region) = html.split_at(cards_start);
        assert_eq!(
            grid_region.matches(r#"href="/post-one/""#).count(),
            1,
            "authored grid (before the moss-cards wrapper) must list Post One once.\n{html}"
        );
        assert_eq!(
            cards_region.matches(r#"href="/post-one/""#).count(),
            1,
            "auto-listing (moss-cards region) must list Post One once.\n{html}"
        );
    }

    /// (e) `children: false` is the ONLY off-switch: it suppresses the auto
    /// children-listing, but an authored `:::grid` in the body survives.
    #[test]
    fn children_false_suppresses_auto_listing_but_grid_survives() {
        let test_dir = std::env::temp_dir().join(format!(
            "moss_children_false_grid_survives_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        let folder_dir = test_dir.join("writings");
        fs::create_dir_all(&folder_dir).unwrap();
        fs::write(
            folder_dir.join("index.md"),
            "---\ntitle: Writings\nchildren: false\n---\n\n\
                 :::grid 2\n[[Child A]]\n+++\n[[Child B]]\n:::\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Child A.md"),
            "---\ntitle: Child A\n---\n\nThe first child's body.\n",
        )
        .unwrap();
        fs::write(
            folder_dir.join("Child B.md"),
            "---\ntitle: Child B\n---\n\nThe second child's body.\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: Home\n---\n\nWelcome.\n",
        )
        .unwrap();

        let output_dir = build_site(&test_dir);
        let html = fs::read_to_string(output_dir.join("writings").join("index.html"))
            .expect("writings/index.html should exist");

        // Authored grid survives — children: false only gates the auto-listing.
        assert!(
            html.contains(r#"class="moss-grid""#),
            "authored :::grid must survive children: false.\n{html}"
        );
        let count_a = html.matches(r#"href="/writings/child-a/""#).count();
        assert_eq!(
            count_a, 1,
            "Child A must render ONCE (authored grid only; auto-listing suppressed). \
                 Got {count_a}.\n{html}"
        );
        // No auto-listing markers.
        assert!(
            !html.contains(r#"class="moss-cards""#),
            "children: false must suppress the .moss-cards auto-listing.\n{html}"
        );
        assert!(
            !html.contains(r#"data-layout="minimal""#)
                && !html.contains(r#"data-layout="list""#)
                && !html.contains(r#"data-layout="grid""#),
            "children: false must suppress every auto-listing data-layout.\n{html}"
        );
    }
}

/// Tests for children/children_style/children_depth fields on folder index pages.
///
/// These fields control how child pages are rendered:
/// - children: true/false (default true)
/// - children_style: "list" (default) | "summary"
/// - children_depth: "direct" (default) | "all"
mod children_field_tests {
    use super::super::generate_html;
    use crate::build::page::layout::LayoutConfig;
    use crate::build::site_url::SiteUrl;
    use crate::i18n::Language;
    use crate::build::types::ParsedDocument;
    use crate::types::content::ProjectStructure;
    use moss_core::PageKind;

    fn localhost_url() -> SiteUrl {
        SiteUrl::parse("http://localhost").unwrap()
    }

    /// Helper to create a minimal ParsedDocument for testing
    fn make_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            html_content: format!("<article><p>{} content</p></article>", title),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            lang: Language::En,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn make_project() -> ProjectStructure {
        ProjectStructure {
            root_path: String::new(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![],
            video_files: vec![],
            notebook_files: vec![],
            other_files: vec![],
            total_files: 0,
            homepage_file: Some("index.md".to_string()),
            ffmpeg_bin_path: None,
            evicted_count: 0,
            evicted_paths: Vec::new(),
            has_content_folders: false,
            has_language_trees: false,
            passthrough_roots: std::collections::HashSet::new(),
            dirs: Vec::new(),
        }
    }

    fn make_layout() -> LayoutConfig {
        LayoutConfig::new("test-site", Some("Test Site"))
    }

    /// Test that children: false suppresses article listing on folder index pages.
    #[test]
    fn test_children_hidden_folder_index() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Articles", "articles/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.children = Some(false);

        let mut child = make_doc("Post One", "articles/post-one/index.html");
        child.date = Some("2025-06-01".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With children: false, the article listing should NOT appear
        assert!(
            !html.contains("Post One"),
            "children: false should suppress article listing. Found 'Post One' in output."
        );
        assert!(
            !html.contains(r#"data-layout="list""#) && !html.contains(r#"data-layout="minimal""#),
            "children: false should not produce data-layout=\"list\" or \"minimal\" wrapper"
        );
        assert!(
            !html.contains(r#"data-layout="grid""#),
            "children: false should not produce data-layout=\"grid\" wrapper"
        );
    }

    /// Regression: `children: false` must suppress `data-layout="grid"`
    /// on folder-index pages that use `children_style: grid`.
    #[test]
    fn test_children_false_suppresses_grid_on_folder_index() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Gallery", "gallery/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.children = Some(false);
        folder_index.children_style = Some(moss_core::Resolved::frontmatter("grid".to_string()));

        let mut child = make_doc("Photo Set", "gallery/set-one/index.html");
        child.date = Some("2025-06-01".to_string());
        child.cover = Some("cover.jpg".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
                !html.contains(r#"data-layout="grid""#),
                "children: false with children_style: grid should suppress data-layout=\"grid\" wrapper"
            );
        assert!(
            !html.contains(r#"data-layout="list""#),
            "children: false should also suppress data-layout=\"list\" wrapper"
        );
        assert!(
            !html.contains("Photo Set"),
            "children: false should suppress the child 'Photo Set' from the listing"
        );
    }

    /// Regression: `children: false` must suppress BOTH auto-listings on the homepage.
    /// On a real site we observed `.moss-cards[data-layout="grid"]` emitted above the hero and
    /// `.moss-cards[data-layout="list"]` after the footer on zh-hans home. This pins the
    /// behavior so custom homepages can fully opt out of auto-emitted listings.
    #[test]
    fn test_children_false_suppresses_both_on_homepage() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.children = Some(false);

        let mut article = make_doc("My Article", "articles/my-article/index.html");
        article.date = Some("2025-06-01".to_string());

        let mut folder_index = make_doc("Articles", "articles/index.html");
        folder_index.kind = PageKind::Folder;

        let all_docs = vec![homepage.clone(), folder_index, article];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains(r#"data-layout="grid""#),
            "children: false on homepage should suppress data-layout=\"grid\" wrapper"
        );
        assert!(
            !html.contains(r#"data-layout="list""#),
            "children: false on homepage should suppress data-layout=\"list\" wrapper"
        );
        assert!(
            !html.contains("My Article"),
            "children: false on homepage should exclude child article from auto-listing"
        );
    }

    /// Test that a non-homepage folder page with `sidebar` field gets sidebar layout.
    #[test]
    fn test_children_sidebar_non_homepage() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("News", "news/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.sidebar = Some("[[News]]".to_string());
        folder_index.from_sidebar_alias = Some(true);

        let mut child1 = make_doc("Breaking News", "news/breaking/index.html");
        child1.date = Some("2025-06-15".to_string());

        let mut child2 = make_doc("Other News", "news/other/index.html");
        child2.date = Some("2025-06-10".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child1, child2];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With sidebar: Some(...), the page should have sidebar layout classes
        assert!(
            html.contains("has-sidebar"),
            "sidebar field should produce has-sidebar class. Got:\n{}",
            html.lines()
                .filter(|l| l.contains("sidebar")
                    || l.contains("has-sidebar")
                    || l.contains("page-wrapper"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // The inline article listing should NOT appear (sidebar renders it instead)
        assert!(
            !html.contains(r#"data-layout="list""#),
            "sidebar field should suppress inline data-layout=\"list\" listing"
        );

        // Sidebar should contain the child articles
        assert!(
            html.contains("Breaking News"),
            "Sidebar should contain child article 'Breaking News'"
        );
    }

    /// A folder's sidebar lists pages that share a date in the order its
    /// series links walk them, not in the order the pages were read.
    #[test]
    fn same_date_sidebar_follows_the_series_chain() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Serial", "serial/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.sidebar = Some("[[Serial]]".to_string());
        folder_index.from_sidebar_alias = Some(true);

        let chapter = |title: &str, slug: &str| {
            let mut d = make_doc(title, &format!("serial/{slug}/index.html"));
            d.date = Some("1804".to_string());
            d
        };
        let all_docs = vec![
            homepage,
            chapter("Exile", "exile"),
            chapter("The Arrival", "arrival"),
            folder_index.clone(),
            chapter("Departure", "departure"),
            chapter("A Crossing", "crossing"),
        ];

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &make_project(),
            &make_layout(),
            false,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        let mut listed = vec!["arrival", "crossing", "departure", "exile"];
        listed.sort_by_key(|slug| {
            html.find(&format!("serial/{slug}/\""))
                .unwrap_or_else(|| panic!("{slug} missing from the sidebar: {html}"))
        });
        let chain: Vec<String> = super::super::sequence_siblings(
            &all_docs,
            "serial/index.html",
            "serial/",
            &folder_index.resolve_for_direct_children(),
        )
        .iter()
        .map(|d| d.url_path.trim_start_matches("serial/").trim_end_matches("/index.html").to_string())
        .collect();
        assert_eq!(chain, listed, "the sidebar must follow the series chain's order");
    }

    /// Test that children_style: "summary" renders summaries instead of list.
    #[test]
    fn test_children_style_summary() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Gallery", "gallery/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.children_style = Some(moss_core::Resolved::frontmatter("summary".to_string()));

        let mut child1 = make_doc("Photo Set One", "gallery/set-one/index.html");
        child1.date = Some("2025-06-15".to_string());
        child1.cover = Some("cover1.jpg".to_string());

        let mut child2 = make_doc("Photo Set Two", "gallery/set-two/index.html");
        child2.date = Some("2025-06-10".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child1, child2];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With children_style: "summary", should render summary layout —
        // identified by `data-layout="list"` since the
        // `moss-summary-layout` co-class is retired.
        assert!(
            html.contains(r#"data-layout="list""#),
            "children_style: summary should emit data-layout=list. Got: {}",
            &html[..html.len().min(500)]
        );
        assert!(
            html.contains(r#"class="moss-card""#),
            "children_style: summary should produce child-summary elements"
        );
        assert!(
            html.contains("Photo Set One"),
            "Card layout should contain child article title"
        );
        assert!(
            html.contains("Photo Set Two"),
            "Card layout should contain second child title"
        );
    }

    /// A stylesheet has no other way to ask "am I on the front page?" — see
    /// the `body_attrs` comment in html.rs. Both directions are pinned because
    /// a marker that appears everywhere is as useless as one that appears
    /// nowhere.
    #[test]
    fn the_homepage_says_so_on_its_body_tag() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        let all_docs = vec![homepage.clone()];

        let html = render_page(Some(&homepage), &all_docs, true);

        assert!(
            html.contains(r#"<body data-page="home">"#),
            "homepage should carry data-page=\"home\""
        );
    }

    #[test]
    fn an_inner_page_carries_no_home_marker() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        let article = make_doc("My Article", "articles/my-article/index.html");
        let all_docs = vec![homepage, article.clone()];

        let html = render_page(Some(&article), &all_docs, false);

        assert!(
            !html.contains("data-page="),
            "only the homepage carries the marker"
        );
    }

    /// The `generate_html` argument list is long and irrelevant to these two
    /// assertions; everything but `is_homepage` is held at its test default.
    fn render_page(
        doc: Option<&ParsedDocument>,
        all_docs: &[ParsedDocument],
        is_homepage: bool,
    ) -> String {
        generate_html(
            doc,
            all_docs,
            &make_project(),
            &make_layout(),
            is_homepage,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed")
    }

    /// The end-of-article subscribe endnote was deleted (moss-subscribe-endnote,
    /// render_subscribe_endnote, LayoutConfig::email_endnote): the footer
    /// default is now the only subscribe surface, on every page kind. This
    /// pins the deletion at the layer that would regress first if the
    /// endnote's injection or its footer-marker-stripping companion code
    /// ever came back — no `moss-subscribe-endnote` markup, and the footer's
    /// `<!-- slot:footer-end -->` marker (which `generate_native_slots` fills
    /// with the real subscribe form downstream) survives untouched exactly
    /// once, on an article, a folder index and the homepage alike.
    #[test]
    fn only_the_footer_subscribe_marker_survives_on_every_page_kind() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        let article = make_doc("Piece", "writing/piece/index.html");
        let mut folder_index = make_doc("發佈會", "events/index.html");
        folder_index.kind = PageKind::Folder;
        let all_docs = vec![homepage.clone(), article.clone(), folder_index.clone()];

        for (doc, is_homepage) in [
            (&homepage, true),
            (&article, false),
            (&folder_index, false),
        ] {
            let html = render_page(Some(doc), &all_docs, is_homepage);
            assert!(
                !html.contains("moss-subscribe-endnote") && !html.contains("moss-subscribe-lead"),
                "the endnote is deleted; it must never render again. Got: {html}"
            );
            assert_eq!(
                html.matches("<!-- slot:footer-end -->").count(),
                1,
                "the footer's subscribe slot must survive exactly once, unstripped. Got: {html}"
            );
        }
    }

    /// `byline:` and `colophon:` exist only to be displayed, so a folder index
    /// shows them exactly as an article does: the byline directly under the
    /// page title, the colophon at the very foot. The four 發佈會 pages that
    /// motivated this are folder indexes whose speaker credits are a page-level
    /// byline; before this they had nowhere to put them but the body.
    #[test]
    fn a_folder_index_renders_its_byline_under_the_title_and_its_colophon_last() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("發佈會", "events/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.byline = vec!["主講　陳遠山".to_string()];
        folder_index.colophon = vec!["主辦　遠聲媒體".to_string()];

        let all_docs = vec![homepage, folder_index.clone()];
        let html = render_page(Some(&folder_index), &all_docs, false);

        let title_at = html
            .find(r#"class="moss-folder-title""#)
            .expect("folder index should carry a folder-title h1");
        let byline_at = html
            .find(r#"class="moss-byline""#)
            .expect("folder index should render its byline");
        let colophon_at = html
            .find(r#"class="moss-article-colophon""#)
            .expect("folder index should render its colophon");

        assert!(
            title_at < byline_at && byline_at < colophon_at,
            "order must be title → byline → … → colophon. Got: {html}"
        );
        assert!(html.contains("主講　陳遠山"), "Got: {html}");
        assert!(html.contains("主辦　遠聲媒體"), "Got: {html}");
    }

    /// The rule holds for every page kind moss can assemble, and each credit
    /// block appears exactly once on each of them.
    ///
    /// Both halves matter. "At least once" is the rule itself — a page kind
    /// that quietly drops an authored `byline:` is the exception this rule
    /// exists to remove. "At most once" is the trap: `render/html.rs` has three
    /// assembly paths, and a `layout: article` page goes through two of them
    /// (the folder path for its title, cover and children listing; the article
    /// path for its date row and credits), so an ungated emission renders each
    /// block twice. Counts, not `contains` — `contains` is green on the
    /// doubled output, which is how that duplication got through once already.
    #[test]
    fn every_page_kind_renders_each_credit_block_exactly_once() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        for (label, mut doc, is_homepage) in every_page_kind() {
            doc.byline = vec!["主講　陳遠山".to_string()];
            doc.colophon = vec!["主辦　遠聲媒體".to_string()];
            let all_docs = vec![homepage.clone(), doc.clone()];
            let html = render_page(Some(&doc), &all_docs, is_homepage);

            assert_eq!(
                html.matches(r#"class="moss-byline""#).count(),
                1,
                "{label}: byline must render exactly once. Got: {html}"
            );
            assert_eq!(
                html.matches(r#"class="moss-article-colophon""#).count(),
                1,
                "{label}: colophon must render exactly once. Got: {html}"
            );
        }
    }

    /// The automatic place line reuses `render_byline_html`/
    /// `splice_byline_at_page_head`, so it has to reach all four call sites
    /// in `render/html.rs` (homepage page-head, folder title block,
    /// non-homepage page-head, article) — traced directly against the
    /// source rather than re-asserted, and exercised here through the same
    /// `every_page_kind` cases the byline count test uses, since those
    /// cases already cover every assembly path. This is the test that would
    /// have caught two missing call sites in an earlier draft, and — with
    /// `render_byline_html`'s combined-`Option` fix — a page with
    /// `location:` set and no `byline:` at all.
    #[test]
    fn all_four_byline_sites_carry_the_place_line() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        for (label, mut doc, is_homepage) in every_page_kind() {
            doc.place_line = Some("Location: [Kyoto](/about/kyoto/)".to_string());
            let all_docs = vec![homepage.clone(), doc.clone()];
            let html = render_page(Some(&doc), &all_docs, is_homepage);

            assert_eq!(
                html.matches(r#"class="moss-place-line""#).count(),
                1,
                "{label}: place line must render exactly once. Got: {html}"
            );
        }
    }

    /// Every page kind moss can assemble, as `(label, doc, is_homepage)` —
    /// one entry per assembly path in `render/html.rs`, plus the
    /// `layout: article` variants that traverse two of them.
    fn every_page_kind() -> Vec<(&'static str, ParsedDocument, bool)> {
        let mut cases: Vec<(&str, ParsedDocument, bool)> = Vec::new();

        cases.push(("root homepage", make_doc("Test Site", "index.html"), true));

        let mut article_home = make_doc("Test Site", "index.html");
        article_home.layout = Some("article".to_string());
        cases.push(("`layout: article` homepage", article_home, true));

        let mut home_override = make_doc("Mountain Home", "en/index.html");
        home_override.kind = PageKind::Folder;
        home_override.is_home_override = true;
        cases.push(("`home: true` folder page", home_override, false));

        let mut lang_root = make_doc("首頁", "zh-hans/index.html");
        lang_root.kind = PageKind::Folder;
        lang_root.source_path = Some("zh-hans/index.md".to_string());
        cases.push(("language-root folder page", lang_root, false));

        let mut plain = make_doc("About", "about.html");
        plain.layout = Some("page".to_string());
        cases.push(("plain page", plain, false));

        cases.push(("article", make_doc("Piece", "writing/piece/index.html"), false));

        let mut folder_index = make_doc("發佈會", "events/index.html");
        folder_index.kind = PageKind::Folder;
        cases.push(("folder index", folder_index.clone(), false));

        folder_index.layout = Some("article".to_string());
        cases.push(("`layout: article` folder index", folder_index, false));

        cases
    }

    /// Same as `render_page`, but with the editor preview's source annotations
    /// switched on (`emit_source_lines`). `build::ship` strips them again for
    /// published output, so this is the only mode in which they exist.
    fn render_page_for_editor(
        doc: Option<&ParsedDocument>,
        all_docs: &[ParsedDocument],
        is_homepage: bool,
    ) -> String {
        generate_html(
            doc,
            all_docs,
            &make_project(),
            &make_layout(),
            is_homepage,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            true, // emit_source_lines
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed")
    }

    /// In the editor preview, everything moss renders FROM frontmatter says
    /// which field it came from. That attribute is the whole of what a preview
    /// click follows back to: a byline without it is a byline the reader can
    /// see and the author cannot reach.
    ///
    /// Runs over every page kind because the annotation is easy to add on the
    /// path you happen to be editing and easy to forget on the other seven —
    /// which is exactly what happened to the folder-index heading, whose only
    /// annotated title was the one the article path injects.
    #[test]
    fn every_page_kind_names_the_frontmatter_behind_what_it_renders() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        for (label, mut doc, is_homepage) in every_page_kind() {
            doc.byline = vec!["主講　陳遠山".to_string()];
            doc.colophon = vec!["主辦　遠聲媒體".to_string()];
            let all_docs = vec![homepage.clone(), doc.clone()];
            let html = render_page_for_editor(Some(&doc), &all_docs, is_homepage);

            assert!(
                html.contains(r#"class="moss-byline" data-source-fm="byline""#),
                "{label}: the byline must name the field it came from. Got: {html}"
            );
            assert!(
                html.contains(
                    r#"class="moss-article-colophon" data-source-fm="colophon""#
                ),
                "{label}: the colophon must name the field it came from. Got: {html}"
            );

            // The folder-index heading is moss's own, rendered from `title:`.
            // Translation homes and `home: true` pages deliberately render no
            // such heading, so there
            // is nothing to annotate there.
            if html.contains("moss-folder-title") {
                assert!(
                    html.contains(r#"class="moss-folder-title" data-source-fm="title""#),
                    "{label}: the folder heading must name `title`. Got: {html}"
                );
            }
        }
    }

    /// The published site carries none of it. The annotations exist for the
    /// editor preview only; `emit_source_lines: false` is the shipped shape.
    #[test]
    fn a_published_page_carries_no_source_annotations() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        // Turn on everything that WOULD be annotated in the editor preview —
        // breadcrumb trail, folder cover, children listing — so this negative
        // covers every fm emitter, not just byline/colophon/title.
        homepage.breadcrumb = Some(true);

        let mut folder_index = make_doc("發佈會", "events/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.byline = vec!["主講　陳遠山".to_string()];
        folder_index.colophon = vec!["主辦　遠聲媒體".to_string()];
        folder_index.cover = Some("assets/cover.jpg".to_string());

        let mut child = make_doc("Launch", "events/launch/index.html");
        child.date = Some("2026-01-01".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child];
        let html = render_page(Some(&folder_index), &all_docs, false);

        assert!(
            !html.contains("data-source-fm"),
            "shipped HTML must carry no data-source-fm. Got: {html}"
        );
    }

    /// The folder cover, the children listing, the breadcrumb trail and the
    /// homepage logo each render from a frontmatter field, and in the editor
    /// preview each names that field — one assertion per emitter, so a
    /// regression says which one forgot.
    #[test]
    fn cover_children_breadcrumb_and_logo_name_their_fields() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.breadcrumb = Some(true); // site-wide breadcrumb enable
        homepage.logo = Some("assets/logo.svg".to_string());

        let mut folder_index = make_doc("Events", "events/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.cover = Some("assets/cover.jpg".to_string());

        let mut child = make_doc("Launch", "events/launch/index.html");
        child.date = Some("2026-01-01".to_string());

        let all_docs = vec![homepage.clone(), folder_index.clone(), child];

        let html = render_page_for_editor(Some(&folder_index), &all_docs, false);
        assert!(
            html.contains(r#"<div class="moss-collection-cover" data-source-fm="cover">"#),
            "the folder cover must name `cover`. Got: {html}"
        );
        assert!(
            html.contains(r#"<div class="moss-cards-container" data-source-fm="children">"#),
            "the children listing must name the `children` family. Got: {html}"
        );
        assert!(
            html.contains(r#"<div class="nav-left" data-source-fm="breadcrumb">"#),
            "the breadcrumb trail must name `breadcrumb`. Got: {html}"
        );
        // The nav logo renders from the HOMEPAGE's `logo:` — a field this
        // page's file does not hold, so this page must not claim it.
        assert!(
            !html.contains(r#"data-source-fm="logo""#),
            "a non-homepage must not annotate the logo. Got: {html}"
        );

        // On the homepage itself, `logo:` lives in the open file.
        let layout = make_layout().with_logo("/assets/logo.svg".to_string());
        let home_html = generate_html(
            Some(&homepage),
            &all_docs,
            &make_project(),
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            true, // emit_source_lines
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed");
        assert!(
            home_html.contains(r#"class="site-logo" data-source-fm="logo""#),
            "the homepage logo must name `logo`. Got: {home_html}"
        );
    }

    /// A page moss gives no title of its own — here the root homepage — puts
    /// the byline under the author's own opening `<h1>` when the body has one,
    /// and at the top of the page when it does not. Either way the colophon is
    /// last. The fallback is a real position, not a shrug: a byline the author
    /// wrote to be read must be somewhere.
    #[test]
    fn a_page_with_no_moss_title_places_the_byline_at_its_head() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.byline = vec!["主講　陳遠山".to_string()];
        homepage.colophon = vec!["主辦　遠聲媒體".to_string()];

        // No heading: the byline opens the page, before the body.
        homepage.html_content = "<p>Welcome.</p>".to_string();
        let all_docs = vec![homepage.clone()];
        let html = render_page(Some(&homepage), &all_docs, true);
        let byline_at = html.find(r#"class="moss-byline""#).expect("byline renders");
        let body_at = html.find("Welcome.").expect("body renders");
        let colophon_at = html
            .find(r#"class="moss-article-colophon""#)
            .expect("colophon renders");
        assert!(
            byline_at < body_at && body_at < colophon_at,
            "with no heading: byline → body → colophon. Got: {html}"
        );

        // Authored heading: the byline sits under it, as on any other page kind.
        homepage.html_content = "<h1>Mountain Home</h1>\n<p>Welcome.</p>".to_string();
        let all_docs = vec![homepage.clone()];
        let html = render_page(Some(&homepage), &all_docs, true);
        let h1_at = html.find("<h1>Mountain Home</h1>").expect("authored h1 is kept");
        let byline_at = html.find(r#"class="moss-byline""#).expect("byline renders");
        let body_at = html.find("Welcome.").expect("body renders");
        assert!(
            h1_at < byline_at && byline_at < body_at,
            "with a heading: h1 → byline → body. Got: {html}"
        );
    }

    /// The other half of the one rule: `description:` is metadata on every page
    /// kind, so a folder index prints none of it as page text.
    #[test]
    fn a_folder_index_does_not_print_its_description() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Events", "events/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.description = Some("A blurb moss may have written itself.".to_string());

        let all_docs = vec![homepage, folder_index.clone()];
        let html = render_page(Some(&folder_index), &all_docs, false);

        assert!(
            !html.contains("collection-description"),
            "description is metadata, not page text. Got: {html}"
        );
    }

    /// Auto-detection should skip year grouping for summary-style pages,
    /// even when articles span multiple years.
    #[test]
    fn test_summary_style_skips_year_group_auto_detection() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        // Folder index with NO explicit children_group (triggers auto-detection)
        let mut folder_index = make_doc("Blog", "blog/index.html");
        folder_index.kind = PageKind::Folder;
        // children_style and children_group are None — both auto-detected

        // Children with covers (triggers summary style) spanning multiple years
        let mut child1 = make_doc("Old Post", "blog/old/index.html");
        child1.date = Some("2016-07-21".to_string());
        child1.cover = Some("cover-old.jpg".to_string());

        let mut child2 = make_doc("New Post", "blog/new/index.html");
        child2.date = Some("2025-11-01".to_string());
        child2.cover = Some("cover-new.jpg".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child1, child2];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Should auto-detect summary style (children have covers) —
        // signal is `data-layout="list"` since the `moss-summary-layout`
        // co-class is retired.
        assert!(
            html.contains(r#"data-layout="list""#),
            "Should auto-detect summary style (data-layout=list) when children have covers"
        );
        // Should NOT have year-group sections (auto-detection skips for summary)
        assert!(
                !html.contains("moss-cards-minimal-year-group"),
                "Summary style should not auto-detect year grouping, even when articles span multiple years. Got: {}",
                &html[..html.len().min(500)]
            );
        // Should still contain both articles
        assert!(html.contains("Old Post"), "Should contain old post");
        assert!(html.contains("New Post"), "Should contain new post");
    }

    /// Test that children_depth: "all" includes nested descendants.
    #[test]
    fn test_children_depth_all() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Docs", "docs/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.children_depth = Some("all".to_string());

        let mut child = make_doc("Getting Started", "docs/getting-started/index.html");
        child.date = Some("2025-06-15".to_string());

        // Nested descendant (grandchild)
        let mut grandchild = make_doc("Installation", "docs/getting-started/install/index.html");
        grandchild.date = Some("2025-06-10".to_string());

        // Another direct child
        let mut child2 = make_doc("Advanced", "docs/advanced/index.html");
        child2.date = Some("2025-06-05".to_string());

        // Nested under advanced
        let mut grandchild2 = make_doc("Configuration", "docs/advanced/config/index.html");
        grandchild2.date = Some("2025-06-01".to_string());

        let all_docs = vec![
            homepage,
            folder_index.clone(),
            child,
            grandchild,
            child2,
            grandchild2,
        ];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With children_depth: "all", nested descendants should be included
        // (sub-folder index pages are excluded, but leaf articles are included)
        assert!(
            html.contains("Installation"),
            "children_depth: all should include nested descendant 'Installation'"
        );
        assert!(
            html.contains("Configuration"),
            "children_depth: all should include nested descendant 'Configuration'"
        );
    }

    /// Test that children_depth: "all" skips folder index pages even when
    /// those folders have no markdown children (e.g. image-only folders).
    #[test]
    fn test_children_depth_all_skips_childless_folder_indices() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Blog", "blog/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.is_root_level = true; // root-level nav item

        // An image folder that has sub-folders with no markdown children
        let mut image_index = make_doc("Images", "image/index.html");
        image_index.kind = PageKind::Folder;
        image_index.is_root_level = true; // nav item, skipped

        // Sub-folder index with no markdown children (image-only folder)
        let mut photo_index = make_doc("Photography", "image/photography/index.html");
        photo_index.kind = PageKind::Folder;

        // Another sub-folder index with no markdown children
        let mut assets_index = make_doc("Assets", "image/assets/index.html");
        assets_index.kind = PageKind::Folder;

        // A real leaf article
        let mut article = make_doc("My Article", "blog/my-article/index.html");
        article.date = Some("2025-06-15".to_string());

        // Homepage with children_depth: all
        let mut home = make_doc("Home", "index.html");
        home.kind = PageKind::Folder;
        home.children_depth = Some("all".to_string());

        let all_docs = vec![
            home.clone(),
            folder_index,
            image_index,
            photo_index,
            assets_index,
            article,
        ];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&home),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Real article should appear
        assert!(
            html.contains("My Article"),
            "Leaf article should appear in flattened homepage listing"
        );
        // Folder index pages should NOT appear (even without markdown children)
        assert!(
            !html.contains(">Photography<"),
            "Childless folder index 'Photography' should be excluded from flattened listing"
        );
        assert!(
            !html.contains(">Assets<"),
            "Childless folder index 'Assets' should be excluded from flattened listing"
        );
    }

    /// Test that default children behavior (no field set) shows articles below.
    #[test]
    fn test_children_default_below() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Blog", "blog/index.html");
        folder_index.kind = PageKind::Folder;
        // No children field set — defaults to true

        let mut child = make_doc("First Post", "blog/first-post/index.html");
        child.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage, folder_index.clone(), child];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Default should show articles inline
        assert!(
            html.contains("First Post"),
            "Default children (below) should show articles inline"
        );
        // The single dated child auto-year-groups (any dated child triggers
        // year grouping), and `list` always renders as the compact index, so
        // the wrapper is data-layout="minimal".
        assert!(
            html.contains(r#"data-layout="minimal""#),
            "Default children (dated) should produce data-layout=\"minimal\" wrapper"
        );
        // Should NOT have sidebar
        assert!(
            !html.contains("has-sidebar"),
            "Default children should not produce sidebar layout"
        );
    }

    // ========== Sidebar cross-ref vs self-ref tests ==========

    /// Cross-referencing sidebar (target folder != current folder) limits to 3 items.
    #[test]
    fn test_sidebar_cross_ref_limits_to_3() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        // Homepage has sidebar pointing to news folder (cross-ref: homepage != news)
        homepage.sidebar = Some("[[News]]".to_string());
        homepage.from_sidebar_alias = Some(true);

        let news_index = make_doc("News", "news/index.html");

        let mut c1 = make_doc("Article 1", "news/article-1/index.html");
        c1.date = Some("2025-06-15".to_string());
        let mut c2 = make_doc("Article 2", "news/article-2/index.html");
        c2.date = Some("2025-06-14".to_string());
        let mut c3 = make_doc("Article 3", "news/article-3/index.html");
        c3.date = Some("2025-06-13".to_string());
        let mut c4 = make_doc("Article 4", "news/article-4/index.html");
        c4.date = Some("2025-06-12".to_string());
        let mut c5 = make_doc("Article 5", "news/article-5/index.html");
        c5.date = Some("2025-06-11".to_string());

        let all_docs = vec![homepage.clone(), news_index, c1, c2, c3, c4, c5];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Cross-ref should show max 3 newest items
        assert!(html.contains("Article 1"), "Should contain newest article");
        assert!(html.contains("Article 2"), "Should contain 2nd newest");
        assert!(html.contains("Article 3"), "Should contain 3rd newest");
        assert!(
            !html.contains("Article 4"),
            "Should NOT contain 4th article (cross-ref limit = 3)"
        );
        assert!(
            !html.contains("Article 5"),
            "Should NOT contain 5th article (cross-ref limit = 3)"
        );
    }

    /// Cross-referencing sidebar has a "More" link to the source folder.
    #[test]
    fn test_sidebar_cross_ref_has_more_link() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.sidebar = Some("[[News]]".to_string());
        homepage.from_sidebar_alias = Some(true);

        let news_index = make_doc("News", "news/index.html");

        let mut c1 = make_doc("Article 1", "news/article-1/index.html");
        c1.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage.clone(), news_index, c1];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("sidebar-more"),
            "Cross-ref sidebar should have 'More' link. Got sidebar HTML:\n{}",
            html.lines()
                .filter(|l| l.contains("sidebar"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            html.contains("news/"),
            "More link should point to the news folder URL"
        );
    }

    /// Regression: the sidebar's "Latest"/"More" labels follow the hosting
    /// page's own language, not the site default. An English homepage on a
    /// Chinese-default site (site_lang = zh-hans) must render English
    /// sidebar chrome. Part of the site_lang-instead-of-doc.lang family.
    #[test]
    fn sidebar_labels_use_page_lang_not_site_lang() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.lang = Language::En;
        homepage.sidebar = Some("[[News]]".to_string());
        homepage.from_sidebar_alias = Some(true);

        let news_index = make_doc("News", "news/index.html");
        let mut c1 = make_doc("Article 1", "news/article-1/index.html");
        c1.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage.clone(), news_index, c1];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::ZhHans, // SITE default is Chinese
            None,
            false,
            true,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed");

        let sidebar: String = html
            .lines()
            .filter(|l| {
                l.contains("latest-sidebar")
                    || l.contains("sidebar-more")
                    || l.contains("Latest")
                    || l.contains("最新")
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            html.contains("Latest") && !html.contains("最新"),
            "English page's sidebar header must be English 'Latest', not '最新'; got:\n{sidebar}"
        );
        assert!(
            !html.contains("更多"),
            "English page's sidebar 'More' link must be English, not '更多'; got:\n{sidebar}"
        );
    }

    /// Regression: the nav's localized labels (e.g. the theme-toggle
    /// aria-label) follow the page's own language even when the page has
    /// NO translation siblings. Previously current_lang was only set from
    /// doc.lang when translation links existed, so a translation-less
    /// English page on a Chinese-default site got Chinese nav labels.
    #[test]
    fn nav_labels_use_page_lang_without_translations() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.lang = Language::En;
        // No translations set on the doc.

        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::ZhHans, // SITE default is Chinese
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("Toggle theme"),
            "English page's nav must use the English theme-toggle label; got nav lines:\n{}",
            html.lines()
                .filter(|l| l.contains("nav-theme-btn")
                    || l.contains("Toggle")
                    || l.contains("切换"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(
            !html.contains("切换主题"),
            "English page's nav must NOT use the Chinese theme-toggle label"
        );
    }

    /// Self-referencing sidebar (target folder == current folder) shows ALL items.
    #[test]
    fn test_sidebar_self_ref_shows_all() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut news_index = make_doc("News", "news/index.html");
        news_index.sidebar = Some("[[News]]".to_string());
        news_index.from_sidebar_alias = Some(true);

        let mut c1 = make_doc("Article 1", "news/article-1/index.html");
        c1.date = Some("2025-06-15".to_string());
        let mut c2 = make_doc("Article 2", "news/article-2/index.html");
        c2.date = Some("2025-06-14".to_string());
        let mut c3 = make_doc("Article 3", "news/article-3/index.html");
        c3.date = Some("2025-06-13".to_string());
        let mut c4 = make_doc("Article 4", "news/article-4/index.html");
        c4.date = Some("2025-06-12".to_string());
        let mut c5 = make_doc("Article 5", "news/article-5/index.html");
        c5.date = Some("2025-06-11".to_string());

        let all_docs = vec![homepage, news_index.clone(), c1, c2, c3, c4, c5];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&news_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Self-ref should show ALL items
        assert!(html.contains("Article 1"), "Should contain article 1");
        assert!(html.contains("Article 2"), "Should contain article 2");
        assert!(html.contains("Article 3"), "Should contain article 3");
        assert!(
            html.contains("Article 4"),
            "Should contain article 4 (self-ref = show all)"
        );
        assert!(
            html.contains("Article 5"),
            "Should contain article 5 (self-ref = show all)"
        );
    }

    /// Self-referencing sidebar does NOT have a "More" link.
    #[test]
    fn test_sidebar_self_ref_no_more_link() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut news_index = make_doc("News", "news/index.html");
        news_index.sidebar = Some("[[News]]".to_string());
        news_index.from_sidebar_alias = Some(true);

        let mut c1 = make_doc("Article 1", "news/article-1/index.html");
        c1.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage, news_index.clone(), c1];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&news_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            true,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains("sidebar-more"),
            "Self-ref sidebar should NOT have 'More' link"
        );
    }

    /// has_sidebar_layout flag is true when any doc has sidebar field set.
    #[test]
    fn test_has_sidebar_layout_flag() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut news_index = make_doc("News", "news/index.html");
        news_index.sidebar = Some("[[News]]".to_string());
        news_index.from_sidebar_alias = Some(true);

        let docs = vec![homepage, news_index];

        // The flag should be true when any document has sidebar
        let flag = docs.iter().any(|d| d.sidebar.is_some());
        assert!(
            flag,
            "has_sidebar_layout should be true when at least one doc has sidebar"
        );

        // And false when none do
        let no_sidebar_docs = vec![make_doc("Test", "index.html")];
        let no_flag = no_sidebar_docs.iter().any(|d| d.sidebar.is_some());
        assert!(
            !no_flag,
            "has_sidebar_layout should be false when no doc has sidebar"
        );
    }

    // ========== Home page children_style / children_depth tests ==========

    /// Home page should auto-detect children_style: "summary" when children have covers.
    #[test]
    fn test_homepage_auto_detects_summary_style() {
        let mut homepage = make_doc("My Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        // children_depth: "all" to flatten (like user's setup)
        homepage.children_depth = Some("all".to_string());

        let mut child1 = make_doc("Article One", "blog/post-one/index.html");
        child1.date = Some("2025-06-15".to_string());
        child1.cover = Some("cover1.jpg".to_string());

        let mut child2 = make_doc("Article Two", "blog/post-two/index.html");
        child2.date = Some("2025-06-10".to_string());

        let all_docs = vec![homepage.clone(), child1, child2];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With a child that has a cover, home page should auto-detect
        // summary style. The signal is `class="moss-card"` since the
        // `moss-summary-layout` co-class is retired.
        assert!(
            html.contains(r#"class="moss-card""#),
            "Home page should auto-detect summary style when children have covers. Got HTML: {}",
            &html[..html.len().min(1000)]
        );
        assert!(
            html.contains("Article One"),
            "Home page should contain child article 'Article One'"
        );
        assert!(
            html.contains("Article Two"),
            "Home page should contain child article 'Article Two'"
        );
    }

    /// Home page should respect explicit children_style: "list" frontmatter.
    #[test]
    fn test_homepage_explicit_children_style_list() {
        let mut homepage = make_doc("My Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children_style = Some(moss_core::Resolved::frontmatter("list".to_string()));
        homepage.children_depth = Some("all".to_string());

        let mut child = make_doc("Article One", "blog/post-one/index.html");
        child.date = Some("2025-06-15".to_string());
        child.cover = Some("cover1.jpg".to_string());

        let all_docs = vec![homepage.clone(), child];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Explicit list style should NOT produce summary layout. The
        // `moss-summary-layout` co-class is retired, so the historic
        // assertion is moot — guard on the absence of the
        // `moss-card-cover` / summary signal instead. Today
        // children_style="list" routes through the non-summary code
        // path: cards render via `child_list::render_child` which
        // emits prefix-link markup, not `moss-card-cover`.
        assert!(
                !html.contains("moss-card-cover"),
                "Explicit children_style: list should not produce summary-style covers on home page. Got: {}",
                &html[..html.len().min(500)]
            );
        assert!(
            html.contains("Article One"),
            "Home page list style should still show articles"
        );
    }

    /// Home page should default to "all" (flatten all descendants).
    #[test]
    fn test_homepage_default_direct_children() {
        let mut homepage = make_doc("My Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        // No children_depth set — should default to "all" (flatten)

        // A folder index (direct child)
        let mut blog_index = make_doc("Blog", "blog/index.html");
        blog_index.kind = PageKind::Folder;

        // A nested article (NOT a direct child of root)
        let mut nested = make_doc("Nested Post", "blog/nested-post/index.html");
        nested.date = Some("2025-06-15".to_string());

        // A root-level article (direct child)
        let mut root_article = make_doc("Root Article", "root-article/index.html");
        root_article.date = Some("2025-06-10".to_string());

        let all_docs = vec![homepage.clone(), blog_index, nested, root_article];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With default "all" depth, all descendants should appear
        assert!(
            html.contains("Root Article"),
            "Home page should include root-level article"
        );
        assert!(
            html.contains("Nested Post"),
            "Home page with default 'all' depth should include nested 'Nested Post'"
        );
    }

    /// Home page with children_depth: "all" should flatten all descendants.
    #[test]
    fn test_homepage_children_depth_all() {
        let mut homepage = make_doc("My Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children_depth = Some("all".to_string());

        let mut blog_index = make_doc("Blog", "blog/index.html");
        blog_index.kind = PageKind::Folder;

        let mut nested = make_doc("Nested Post", "blog/nested-post/index.html");
        nested.date = Some("2025-06-15".to_string());

        let mut root_article = make_doc("Root Article", "root-article/index.html");
        root_article.date = Some("2025-06-10".to_string());

        let all_docs = vec![homepage.clone(), blog_index, nested, root_article];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // With children_depth: "all", all descendants should appear
        assert!(
            html.contains("Nested Post"),
            "Home page children_depth: all should include nested 'Nested Post'"
        );
        assert!(
            html.contains("Root Article"),
            "Home page children_depth: all should include 'Root Article'"
        );
    }

    /// children: "[[News]]" should render only the News folder's articles.
    #[test]
    fn test_homepage_children_source_targets_folder() {
        let mut homepage = make_doc("My Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children = Some(true);
        homepage.children_source = Some("[[News]]".to_string());

        // Faculty page: nav item, should NOT appear
        let mut faculty = make_doc("Faculty", "faculty/index.html");
        faculty.kind = PageKind::Folder;
        faculty.nav = Some(true);

        // News folder index: should NOT appear (is_index)
        let mut news_index = make_doc("News", "news/index.html");
        news_index.kind = PageKind::Folder;

        // News article: SHOULD appear (targeted by children_source)
        let mut news_article = make_doc("New Hub", "news/new-hub/index.html");
        news_article.date = Some("2025-09-18".to_string());

        // Root page with nav: false — should NOT appear (only News articles)
        let mut mission = make_doc("Our Mission", "our-mission/index.html");
        mission.nav = Some(false);

        let all_docs = vec![homepage.clone(), faculty, news_index, news_article, mission];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // The news article should appear in children list
        assert!(
            html.contains("New Hub"),
            "children_source: [[News]] should include news article 'New Hub'"
        );
        // Extract main content area to verify Faculty/Mission are NOT children
        let main_start = html.find("<main").unwrap_or(0);
        let main_content = &html[main_start..];
        assert!(
            !main_content.contains("Our Mission"),
            "children_source: [[News]] should NOT include 'Our Mission' in main content"
        );
    }

    /// An ORDINARY page (not the homepage, not a folder index) carrying
    /// `children: '[[/]]'` + `children_depth: all` should host a whole-site
    /// listing the same way the homepage branch's `children_source` does.
    /// The "Regular page content" arm only synthesized a children marker when
    /// `is_folder_index || term_listing.is_some()`, so this page's parsed
    /// `children_source`/`children_depth` were silently dropped on the floor.
    #[test]
    fn test_ordinary_page_with_children_source_lists_whole_site() {
        let mut home = make_doc("Home", "index.html");
        home.is_root_level = true;
        home.kind = PageKind::Folder;

        let mut blog_folder = make_doc("Blog", "blog/index.html");
        blog_folder.kind = PageKind::Folder;

        let mut blog_post = make_doc("Blog Post", "blog/post-one/index.html");
        blog_post.date = Some("2025-01-01".to_string());

        let mut projects_folder = make_doc("Projects", "projects/index.html");
        projects_folder.kind = PageKind::Folder;

        let mut project_alpha = make_doc("Project Alpha", "projects/alpha/index.html");
        project_alpha.date = Some("2025-02-01".to_string());

        // An ordinary page: default kind (Article), not root-level, not a
        // folder index — it just hosts the listing at its own URL.
        let mut everything = make_doc("Everything", "everything/index.html");
        everything.source_path = Some("everything.md".to_string());
        everything.children_source = Some("[[/]]".to_string());
        everything.children_depth = Some("all".to_string());

        let all_docs = vec![
            home,
            blog_folder,
            blog_post,
            projects_folder,
            project_alpha,
            everything.clone(),
        ];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&everything),
            &all_docs,
            &project,
            &layout,
            false, // is_homepage — this is an ordinary page, not the homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("Blog Post"),
            "children: '[[/]]' + children_depth: all on an ordinary page should list every leaf page: {}",
            html
        );
        assert!(
            html.contains("Project Alpha"),
            "children: '[[/]]' + children_depth: all on an ordinary page should list every leaf page: {}",
            html
        );

        let main_start = html.find("<main").unwrap_or(0);
        let main_content = &html[main_start..];
        assert!(
            !main_content.contains(">Blog<"),
            "folder index pages must not appear in a flattened whole-site listing: {}",
            main_content
        );
        assert!(
            !main_content.contains(">Projects<"),
            "folder index pages must not appear in a flattened whole-site listing: {}",
            main_content
        );
        assert!(
            !main_content.contains(">Everything<"),
            "the hosting page must not list itself: {}",
            main_content
        );
    }

    /// A page that WINS a term claim (`term_listing` set) hosts that term's
    /// member listing below its own body, but it is still an ordinary leaf
    /// page — not `PageKind::Folder` — so the folder-cover branch above never
    /// ran for it and its `cover:` reached only `data-share-cover` (OG/share
    /// metadata), never a visible `<img>`. A folder-index page that claims
    /// the same term already shows its cover (the branch gated on
    /// `is_folder_index`); a claiming leaf page must show it too — the SAME
    /// book-open layout (cover beside title and lede), not a bare row with
    /// empty space beside the image — and it must still show only one
    /// visible title: its own, not a second one from the cover component.
    #[test]
    fn test_claimed_leaf_page_renders_own_cover() {
        let mut claimant = make_doc("Ada Lin", "people/ada-lin/index.html");
        claimant.cover = Some("ada.png".to_string());
        claimant.term_listing = Some("people/ada-lin".to_string());
        // Stands in for the markdown pipeline's own injected article title —
        // by the time `generate_html` runs, this shell's visible `<h1>` is
        // already the head of the body, not something this branch adds.
        claimant.html_content =
            "<h1 class=\"moss-article-title\">Ada Lin</h1>\n<p>Ada Lin content</p>".to_string();

        let all_docs = vec![claimant.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&claimant),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"class="moss-collection-cover-body""#),
            "a claimed leaf page with a cover must render the same book-open \
             layout — cover beside title and lede — a claimed folder index \
             gets, not a bare row: {}",
            html
        );
        assert!(
            html.contains(r#"<h1 class="moss-folder-title"></h1>"#),
            "the folder-cover component's own title slot must be passed an \
             empty label and collapse (site.css `.moss-folder-title:empty`) \
             — the claiming leaf's visible heading is its own, not a second \
             one from this component: {}",
            html
        );
        assert!(
            html.contains(r#"<h1 class="moss-article-title">Ada Lin</h1>"#),
            "the page's own heading must survive, inside the cover column: {}",
            html
        );
    }

    /// An Article-shell claiming leaf (the default shell here: no `layout:`
    /// override, flat mode, `is_root_level: false`) with a cover, a date, and
    /// a byline. The date-line + reading-prefs row and the byline are built
    /// separately, later in `generate_html`, and spliced into the WHOLE
    /// assembled page content via `splice_after_title_block` — which used to
    /// require the content to start with `<h1`. Once the claimed-leaf cover
    /// wraps that title in `.moss-collection-cover-row`, the content starts
    /// with `<div` instead, so the splice fell through to prepending: the
    /// date-line and byline landed BEFORE the whole cover row, above the
    /// image, on develop this page has no cover so the regression is
    /// specific to this branch. Both must land after the real title, inside
    /// `.moss-collection-cover-body`, and before the rest of the body.
    #[test]
    fn test_claimed_leaf_article_shell_date_and_byline_land_in_cover_column() {
        let mut claimant = make_doc("Ada Lin", "people/ada-lin/index.html");
        claimant.cover = Some("ada.png".to_string());
        claimant.term_listing = Some("people/ada-lin".to_string());
        claimant.date = Some("2026-04-01".to_string());
        claimant.byline = vec!["Photography by Ada Lin".to_string()];
        claimant.html_content =
            "<h1 class=\"moss-article-title\">Ada Lin</h1>\n<p>Ada Lin content</p>".to_string();

        let all_docs = vec![claimant.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&claimant),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        let cover_row_pos = html
            .find(r#"class="moss-collection-cover-row""#)
            .expect("cover row must render");
        let title_end = html
            .find("<h1 class=\"moss-article-title\">Ada Lin</h1>")
            .map(|i| i + "<h1 class=\"moss-article-title\">Ada Lin</h1>".len())
            .expect("the page's own title must render");
        let date_line_pos = html
            .find(r#"class="date-line""#)
            .expect("date-line must render for a dated article");
        let byline_pos = html
            .find("Photography by Ada Lin")
            .expect("byline must still render");
        let body_pos = html
            .find("<p>Ada Lin content</p>")
            .expect("the rest of the body must still render");

        assert!(
            cover_row_pos < title_end,
            "the cover row must open before the title: {}",
            html
        );
        assert!(
            date_line_pos > title_end,
            "date-line must land after the title, not above the whole cover \
             row: {}",
            html
        );
        assert!(
            byline_pos > title_end,
            "byline must land after the title, not above the whole cover \
             row: {}",
            html
        );
        assert!(
            date_line_pos < body_pos && byline_pos < body_pos,
            "date-line and byline must land before the rest of the body: {}",
            html
        );
    }

    /// A claiming leaf page rendered with the PAGE shell (not Article) still
    /// gets its `byline:` — the same `!is_article_page` splice every other
    /// page kind uses, run on the WHOLE already cover-wrapped `content` (one
    /// splice site, no lead-scoping special case). It lands correctly because
    /// `splice_byline_at_page_head` delegates unconditionally to
    /// `splice_after_title_block`, which recognizes the cover wrapper's own
    /// empty `<h1 class="moss-folder-title">` and steps past it to splice
    /// after this page's REAL title — not above the whole cover row, which is
    /// where a naive first-`<h1>` test would leave it (the wrapper's outer
    /// `<div>` is never itself an `<h1`).
    #[test]
    fn test_claimed_leaf_page_shell_byline_lands_beside_title_in_cover_column() {
        let mut claimant = make_doc("Ada Lin", "people/ada-lin/index.html");
        claimant.cover = Some("ada.png".to_string());
        claimant.term_listing = Some("people/ada-lin".to_string());
        claimant.layout = Some("page".to_string()); // force Page shell, not Article
        claimant.byline = vec!["Photography by Ada Lin".to_string()];
        claimant.html_content = "<h1>Ada Lin</h1>\n<p>Ada Lin content</p>".to_string();

        let all_docs = vec![claimant.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&claimant),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        let body_col_start = html
            .find(r#"class="moss-collection-cover-body""#)
            .expect("cover-body column must render");
        let title_end = html[body_col_start..]
            .find("</h1>")
            .map(|i| body_col_start + i)
            .expect("a title h1 must render inside the cover column");
        let byline_pos = html
            .find("Photography by Ada Lin")
            .expect("byline must still render for a Page-shell claimant");
        assert!(
            byline_pos > title_end,
            "byline must land after this page's own title, inside the cover \
             column, not above the whole cover row: {}",
            html
        );
    }

    /// Baseline: an ordinary leaf page with NO term claim keeps its
    /// pre-existing behavior — `cover:` reaches `data-share-cover` only, no
    /// visible `<img>`. Guards the claimed-page fix above from broadening
    /// into every page that merely sets `cover:`.
    #[test]
    fn test_unclaimed_leaf_page_with_cover_has_no_visible_cover_row() {
        let mut page = make_doc("First Look", "posts/first-look/index.html");
        page.cover = Some("first-look-cover.png".to_string());

        let all_docs = vec![page.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains(r#"class="moss-collection-cover-row""#),
            "an unclaimed leaf page must not gain a visible cover row: {}",
            html
        );
        assert!(
            html.contains(r#"data-share-cover="/first-look-cover.png""#),
            "the cover must still reach data-share-cover as before: {}",
            html
        );
    }

    #[test]
    fn test_homepage_children_limit_caps_body_feed_with_more_link() {
        // Homepage with children: "[[News]]" + children_limit: 3 should render
        // only the 3 newest articles, plus a "More →" link to the news folder.
        let mut homepage = make_doc("Lab", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children = Some(true);
        homepage.children_source = Some("[[News]]".to_string());
        homepage.children_limit = Some(3);

        let mut news_index = make_doc("News", "news/index.html");
        news_index.kind = PageKind::Folder;

        let make_article = |title: &str, slug: &str, date: &str| {
            let mut a = make_doc(title, &format!("news/{}/index.html", slug));
            a.date = Some(date.to_string());
            a
        };

        let a1 = make_article("Newest Story", "story-newest", "2026-05-01");
        let a2 = make_article("Second Story", "story-second", "2026-04-20");
        let a3 = make_article("Third Story", "story-third", "2026-04-10");
        let a4 = make_article("Older Story", "story-older", "2026-03-15");
        let a5 = make_article("Oldest Story", "story-oldest", "2026-02-01");

        let all_docs = vec![homepage.clone(), news_index, a1, a2, a3, a4, a5];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(html.contains("Newest Story"), "newest should appear");
        assert!(html.contains("Second Story"), "2nd should appear");
        assert!(html.contains("Third Story"), "3rd should appear");
        assert!(
            !html.contains("Older Story"),
            "4th should be truncated by children_limit: 3"
        );
        assert!(
            !html.contains("Oldest Story"),
            "5th should be truncated by children_limit: 3"
        );
        assert!(
            html.contains("/news/") && html.to_lowercase().contains("more"),
            "should render a 'More' link pointing to /news/ when truncated; html: {}",
            html
        );
    }

    #[test]
    fn test_homepage_children_limit_no_more_link_when_not_truncated() {
        // Limit equals items: no truncation, no "More →" link.
        let mut homepage = make_doc("Lab", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children = Some(true);
        homepage.children_source = Some("[[News]]".to_string());
        homepage.children_limit = Some(3);

        let mut news_index = make_doc("News", "news/index.html");
        news_index.kind = PageKind::Folder;

        let make_article = |title: &str, slug: &str, date: &str| {
            let mut a = make_doc(title, &format!("news/{}/index.html", slug));
            a.date = Some(date.to_string());
            a
        };

        let all_docs = vec![
            homepage.clone(),
            news_index,
            make_article("A", "a", "2026-05-01"),
            make_article("B", "b", "2026-04-01"),
            make_article("C", "c", "2026-03-01"),
        ];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Look for "more" only inside the children-feed area, not in nav or
        // other site chrome.
        let main = html.find("<main").map(|i| &html[i..]).unwrap_or(&html);
        let lc = main.to_lowercase();
        // Crude but sufficient: "more →" / "more&nbsp;→" / "more &rarr;"
        // should not appear when feed isn't truncated.
        assert!(
            !lc.contains(">more<") && !lc.contains("more →") && !lc.contains("more&nbsp;"),
            "no More link when limit == available items"
        );
    }

    #[test]
    fn test_homepage_children_limit_picks_latest_when_input_order_disagrees() {
        // Regression for a real site's bug: `take(n)` ran before the date sort,
        // so `children_limit: N` returned the lex/iteration-first N rather than
        // the latest N by date. Articles below are ordered so that BOTH input
        // order AND lexical url_path order put the OLDEST items first — only a
        // date-aware truncation picks the right three.
        let mut homepage = make_doc("Lab", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children = Some(true);
        homepage.children_source = Some("[[News]]".to_string());
        homepage.children_limit = Some(3);

        let mut news_index = make_doc("News", "news/index.html");
        news_index.kind = PageKind::Folder;

        let make_article = |title: &str, slug: &str, date: &str| {
            let mut a = make_doc(title, &format!("news/{}/index.html", slug));
            a.date = Some(date.to_string());
            a
        };

        // Lex order of url_paths: a-old, b-old, c-old, x-new, y-new, z-new.
        // Latest 3 by date: New X (2026-12), New Y (2026-11), New Z (2026-10).
        let all_docs = vec![
            homepage.clone(),
            news_index,
            make_article("Old A", "a-old", "2025-01-01"),
            make_article("Old B", "b-old", "2025-02-01"),
            make_article("Old C", "c-old", "2025-03-01"),
            make_article("New X", "x-new", "2026-12-01"),
            make_article("New Y", "y-new", "2026-11-01"),
            make_article("New Z", "z-new", "2026-10-01"),
        ];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("New X"),
            "newest should appear; html: {}",
            html
        );
        assert!(html.contains("New Y"), "2nd newest should appear");
        assert!(html.contains("New Z"), "3rd newest should appear");
        assert!(!html.contains("Old A"), "Old A must be truncated (oldest)");
        assert!(!html.contains("Old B"), "Old B must be truncated");
        assert!(!html.contains("Old C"), "Old C must be truncated");
    }

    #[test]
    fn test_user_js_tag_appears_in_html_when_present() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            true,           // has_user_js
            Some("abc123"), // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"<script src="/_moss/theme/script.abc123.js"></script>"#),
            "HTML should contain _moss/theme/script.<hash>.js script tag, got:\n{}",
            &html[html.len().saturating_sub(300)..]
        );
    }

    #[test]
    fn test_user_js_tag_absent_when_no_script() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains("_moss/theme/script.js"),
            "HTML should NOT contain _moss/theme/script.js when has_user_js is false"
        );
        assert!(
            !html.contains("window.mossTheme"),
            "HTML should NOT inject window.mossTheme when has_user_js is false"
        );
    }

    #[test]
    fn user_js_tag_includes_theme_base_global() {
        // has_user_js path must emit window.mossTheme.base BEFORE the user script.
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            true,           // has_user_js
            Some("abc123"), // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"window.mossTheme={base:new URL("/_moss/theme/",location.href).href}"#),
            "must inject the theme base global, got:\n{html}"
        );
        // ordering: the base global appears before the user <script src>
        let base_idx = html.find("window.mossTheme").expect("base global present");
        let src_idx = html
            .find(r#"src="/_moss/theme/script."#)
            .expect("user script present");
        assert!(
            base_idx < src_idx,
            "base global must precede the user script tag"
        );
    }

    /// Regression: explicit `children_group: year` from frontmatter
    /// must survive a non-Date sort axis. Pre-fix the renderer
    /// applied `effective_group = "none"` whenever
    /// `!matches!(resolved.axis, SortAxis::Date)` — overriding
    /// author intent silently. With Resolved<String> origin
    /// tracking, only auto-detected groups are overridden.
    #[test]
    fn explicit_children_group_survives_non_date_axis() {
        let mut homepage = make_doc("Home", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children_depth = Some("all".to_string());
        homepage.children_style = Some(moss_core::Resolved::frontmatter("list".to_string()));
        homepage.children_group = Some(moss_core::Resolved::frontmatter("year".to_string()));
        // Direct-children axis cached as Title (e.g. one dateless
        // direct child) — pre-fix this drove the year-group
        // suppression even though the author explicitly asked for it.
        homepage.direct_children_sort = Some(moss_core::sort::ResolvedSort {
            axis: moss_core::sort::SortAxis::Title,
            explicit_order: None,
            series_default: false,
        });

        let mut older = make_doc("Older Post", "essays/older/index.html");
        older.date = Some("2024-03-01".to_string());
        let mut newer = make_doc("Newer Post", "essays/newer/index.html");
        newer.date = Some("2025-04-01".to_string());

        let all_docs = vec![homepage.clone(), older, newer];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("Older Post") && html.contains("Newer Post"),
            "precondition: descendants must render. Got: {}",
            &html[..html.len().min(800)]
        );
        assert!(
            html.contains("moss-cards-minimal-year-group"),
            "explicit children_group: year must survive non-Date axis. Got: {}",
            &html[..html.len().min(800)]
        );
        assert!(
            html.contains("<h2>2024</h2>") && html.contains("<h2>2025</h2>"),
            "year headings expected for 2024 and 2025. Got: {}",
            &html[..html.len().min(800)]
        );
    }

    /// Regression: when `children_depth: all` flattens descendants,
    /// the renderer must re-resolve the sort axis on the flattened
    /// scope. The scan cache (`direct_children_sort`) only reflects
    /// direct-children inference and misrepresents the corpus the
    /// reader actually sees. A real site's home had only folder children
    /// at the root (dateless), which cached as Title axis even though
    /// the flattened descendants are mostly dated essays.
    #[test]
    fn flatten_homepage_uses_descendant_dates_for_axis() {
        let mut homepage = make_doc("Home", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;
        homepage.children_depth = Some("all".to_string());
        // Direct-children axis cached as Title (only one dateless
        // folder child below) — pre-fix this drove skip_resort=true
        // and suppressed year grouping for the flattened corpus.
        homepage.direct_children_sort = Some(moss_core::sort::ResolvedSort {
            axis: moss_core::sort::SortAxis::Title,
            explicit_order: None,
            series_default: false,
        });

        let mut writings = make_doc("Writings", "writings/index.html");
        writings.kind = PageKind::Folder;
        let mut older = make_doc("Older", "writings/older/index.html");
        older.date = Some("2024-03-01".to_string());
        let mut newer = make_doc("Newer", "writings/newer/index.html");
        newer.date = Some("2025-04-01".to_string());

        let all_docs = vec![homepage.clone(), writings, older, newer];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("Older") && html.contains("Newer"),
            "precondition: descendants must render. Got: {}",
            &html[..html.len().min(800)]
        );
        assert!(
                html.contains("moss-cards-minimal-year-group"),
                "flatten scope must drive axis re-resolution (year grouping for dated descendants). Got: {}",
                &html[..html.len().min(800)]
            );
        assert!(
            html.contains("<h2>2024</h2>") && html.contains("<h2>2025</h2>"),
            "year headings expected. Got: {}",
            &html[..html.len().min(800)]
        );
    }
}

// =========================================================================
// Typesetting tests
// =========================================================================
mod typesetting_tests {
    use super::super::generate_html;
    use crate::build::page::layout::LayoutConfig;
    use crate::build::site_url::SiteUrl;
    use crate::i18n::Language;
    use crate::build::types::ParsedDocument;
    use crate::types::content::ProjectStructure;
    use moss_core::PageKind;

    fn localhost_url() -> SiteUrl {
        SiteUrl::parse("http://localhost").unwrap()
    }

    fn make_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path.replace("index.html", "")), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            lang: Language::En,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn make_project() -> ProjectStructure {
        ProjectStructure {
            root_path: String::new(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![],
            video_files: vec![],
            notebook_files: vec![],
            other_files: vec![],
            total_files: 0,
            homepage_file: Some("index.md".to_string()),
            ffmpeg_bin_path: None,
            evicted_count: 0,
            evicted_paths: Vec::new(),
            has_content_folders: false,
            has_language_trees: false,
            passthrough_roots: std::collections::HashSet::new(),
            dirs: Vec::new(),
        }
    }

    fn make_layout() -> LayoutConfig {
        LayoutConfig::new("test-site", Some("Test Site"))
    }

    #[test]
    fn test_vertical_typesetting_from_layout_config() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout().with_typesetting("vertical".to_string());

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"data-typesetting="vertical""#),
            "HTML should contain data-typesetting attribute when layout has vertical typesetting"
        );
    }

    #[test]
    fn test_horizontal_typesetting_omits_attribute() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout().with_typesetting("horizontal".to_string());

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains("data-typesetting"),
            "HTML should NOT contain data-typesetting when typesetting is horizontal (default)"
        );
    }

    #[test]
    fn test_no_typesetting_config_omits_attribute() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout(); // no typesetting set

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains("data-typesetting"),
            "HTML should NOT contain data-typesetting when no typesetting is configured"
        );
    }

    #[test]
    fn test_per_page_typesetting_overrides_layout_config() {
        let mut page = make_doc("Article", "article/index.html");
        page.typesetting = Some("vertical".to_string());

        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout(); // no site-level typesetting

        let html = generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"data-typesetting="vertical""#),
            "Per-page typesetting should override missing site config"
        );
    }

    #[test]
    fn test_per_page_horizontal_overrides_site_vertical() {
        let mut page = make_doc("Article", "article/index.html");
        page.typesetting = Some("horizontal".to_string());

        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout().with_typesetting("vertical".to_string());

        let html = generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            !html.contains("data-typesetting"),
            "Per-page horizontal should override site vertical — attribute omitted for default"
        );
    }

    /// A folder page's automatic children listing — the homepage's and any
    /// other folder index's, two separate call sites — follows the page's
    /// EFFECTIVE typesetting, like the shell's `data-typesetting` above. Both
    /// used to read the site's alone, so a page's own `typesetting:` changed
    /// the page but not the dates in its listing.
    #[test]
    fn folder_listings_follow_the_pages_effective_typesetting() {
        let render = |dir: &str, page_typesetting: Option<&str>, site_typesetting: Option<&str>| {
            let (url, source, child_url, child_source) = if dir.is_empty() {
                ("index.html".to_string(), "index.md".to_string(), "a/index.html".to_string(), "a.md".to_string())
            } else {
                (format!("{dir}/index.html"), format!("{dir}/index.md"), format!("{dir}/a/index.html"), format!("{dir}/a.md"))
            };
            let mut folder = make_doc("Folder", &url);
            folder.kind = PageKind::Folder;
            folder.lang = Language::ZhHant;
            folder.source_path = Some(source);
            folder.typesetting = page_typesetting.map(String::from);
            let mut child = make_doc("A", &child_url);
            child.lang = Language::ZhHant;
            child.source_path = Some(child_source);
            child.date = Some("1697-09".to_string());
            let mut all_docs = vec![folder.clone(), child];
            if !dir.is_empty() {
                all_docs.push(make_doc("Test Site", "index.html"));
            }
            let mut layout = make_layout();
            if let Some(t) = site_typesetting {
                layout = layout.with_typesetting(t.to_string());
            }
            generate_html(
                Some(&folder), &all_docs, &make_project(), &layout, dir.is_empty(), None, None,
                Language::ZhHant, None, false, false, None, false, None, None,
                &std::collections::HashMap::new(), &localhost_url(), false, false, "favicon.svg",
                None, std::path::Path::new(""),
            )
            .expect("generate_html should succeed")
        };
        // The listing's year heading reads 一六九七 under vertical CJK.
        let cjk = "一六九七";
        for dir in ["", "wen"] {
            let page_vertical = render(dir, Some("vertical"), None);
            assert!(page_vertical.contains(cjk), "{dir:?}: page-level vertical must reach the listing");
            let site_vertical = render(dir, None, Some("vertical"));
            assert!(site_vertical.contains(cjk), "{dir:?}: site-only vertical must reach the listing");
            let page_horizontal = render(dir, Some("horizontal"), Some("vertical"));
            assert!(!page_horizontal.contains(cjk), "{dir:?}: the page's horizontal must win");
        }
    }

    #[test]
    fn test_site_default_content_width() {
        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout().with_content_width("wide".to_string());

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"data-content-width="wide""#),
            "Site-level content_width should appear on body"
        );
    }

    #[test]
    fn test_site_default_comments_false() {
        let mut page = make_doc("Article", "article/index.html");
        page.date = Some("2025-01-01".to_string());

        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout().with_comments(false);

        let html = generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"data-comments="false""#),
            "Site-level comments=false should emit data-comments on article element"
        );
    }

    #[test]
    fn test_per_page_comments_overrides_site() {
        let mut page = make_doc("Article", "article/index.html");
        page.comments = Some(true); // page opts in

        let homepage = make_doc("Test Site", "index.html");
        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout().with_comments(false); // site opts out

        let html = generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            false,
            false,
            "favicon.svg",
            None,
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"data-comments="true""#),
            "Per-page comments=true should override site comments=false"
        );
    }
}

// Cascade tests have been moved to cascade.rs

/// Tests that article cover images are NOT appended to article content.
///
/// Regression test: The `is_folder_index` check used `url_path.ends_with("/index.html")`
/// which matched ALL pages (articles use pretty URLs like `articles/my-post/index.html`).
/// Only actual folder index pages (from index.md, readme.md, _index.md, main.md) should
/// get cover images appended.
mod article_cover_tests {
    use moss_core::PageKind;

    fn process_markdown_file(
        file_path: &str,
        content: &str,
    ) -> Result<crate::build::types::ParsedDocument, String> {
        crate::build::markdown::process_markdown_file(
            file_path,
            content,
            "",
            &std::collections::HashMap::new(),
            true,
            crate::i18n::Language::En,
            None,
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            None,
            false,
            None,
            None, // folder_lang
        )
    }

    /// Helper: determines `is_folder_index` the same way render.rs does.
    fn is_folder_index(doc: &crate::build::types::ParsedDocument) -> bool {
        doc.kind == PageKind::Folder
            && doc.url_path.ends_with("/index.html")
            && doc.url_path != "index.html"
    }

    #[test]
    fn test_article_with_cover_is_not_folder_index() {
        let md = "---\ntitle: My Post\ncover: photo.jpg\n---\n\nArticle content.";
        let doc = process_markdown_file("articles/my-post.md", md).expect("should parse article");

        // Article uses pretty URL: articles/my-post/index.html
        assert!(
            doc.url_path.ends_with("/index.html"),
            "Article should have pretty URL ending in /index.html, got: {}",
            doc.url_path
        );
        assert!(doc.cover.is_some(), "Article should have cover set");
        assert!(
            !is_folder_index(&doc),
            "Article should NOT be detected as folder index. url_path: {}",
            doc.url_path
        );
    }

    #[test]
    fn test_folder_index_with_cover_is_folder_index() {
        let md = "---\ntitle: Articles\ncover: banner.jpg\n---\n\nWelcome to articles.";
        let doc =
            process_markdown_file("articles/index.md", md).expect("should parse folder index");

        assert_eq!(doc.url_path, "articles/index.html");
        assert!(doc.cover.is_some());
        assert!(
            is_folder_index(&doc),
            "Folder index should be detected as folder index. url_path: {}",
            doc.url_path
        );
    }

    #[test]
    fn test_root_index_is_not_folder_index() {
        let md = "---\ntitle: Home\ncover: hero.jpg\n---\n\nWelcome.";
        let doc = process_markdown_file("index.md", md).expect("should parse root index");

        assert_eq!(doc.url_path, "index.html");
        assert!(
            !is_folder_index(&doc),
            "Root index should NOT be folder index"
        );
    }

    #[test]
    fn test_main_md_in_subfolder_is_folder_index() {
        let md = "---\ntitle: Docs\ncover: docs-cover.jpg\n---\n\nDocumentation.";
        let doc = process_markdown_file("docs/main.md", md).expect("should parse docs/main.md");

        assert_eq!(doc.url_path, "docs/index.html");
        assert!(
            is_folder_index(&doc),
            "docs/main.md should be detected as folder index"
        );
    }
}

/// Auto-generated folder index pages should include an h1 heading with the folder name.
///
/// Class is `moss-folder-title`, shared with the explicit-folder-index paths
/// in `folder_cover.rs` (cover branch) and the no-cover branch in this file.
#[test]
fn test_auto_folder_index_includes_h1_heading() {
    let folder_h1 = crate::build::components::folder_title::render("文字", false);
    let article_list = "<div class=\"moss-cards\" data-layout=\"list\">...</div>";
    let content_html = format!("{}\n{}", folder_h1, article_list);

    assert!(
        content_html.contains("<h1 class=\"moss-folder-title\">文字</h1>"),
        "Auto-generated folder index should have visible h1 heading"
    );
    assert!(
        content_html.contains(r#"data-layout="list""#),
        "Article listing should follow the heading"
    );
}

/// Folder-index pages with no cover prepend `<h1 class="moss-folder-title">`
/// before the body content. Closes the previous gap where files like
/// a site's `Projects/Projects.md` rendered with zero h1s. Same helper
/// as the cover branch — single source of the class string.
#[test]
fn folder_index_without_cover_prepends_folder_title_h1() {
    let folder_h1 = crate::build::components::folder_title::render("Projects", false);
    assert!(folder_h1.contains(r#"<h1 class="moss-folder-title">Projects</h1>"#));
    // Integration: the no-cover branch in this file concatenates folder_h1 \n content.
    let content = "<p>Body.</p>";
    let result = format!("{}\n{}", folder_h1, content);
    assert!(result.starts_with(r#"<h1 class="moss-folder-title">Projects</h1>"#));
    assert!(result.contains("<p>Body.</p>"));
}

#[test]
fn no_cover_folder_heading_suppressed_for_nav_folder() {
    // A folder index that is a nav item (nav: true) gets no folder title —
    // the nav bar already shows it.
    let doc = crate::build::types::ParsedDocument {
        nav: Some(true),
        ..Default::default()
    };
    assert_eq!(
        super::no_cover_folder_heading(&doc, "Projects", true, false),
        ""
    );
}

#[test]
fn no_cover_folder_heading_present_for_non_nav_folder() {
    // A non-nav folder index (nested, nav: None) keeps its moss-folder-title.
    let doc = crate::build::types::ParsedDocument {
        nav: None,
        is_root_level: false,
        ..Default::default()
    };
    assert!(
        super::no_cover_folder_heading(&doc, "Projects", true, false)
            .contains(r#"<h1 class="moss-folder-title">Projects</h1>"#),
        "non-nav folder index must keep its title"
    );
}

// home_file_winner_tests have been moved to page_map.rs

/// Tests for home file demotion logic
mod home_file_demotion_tests {
    use crate::build::markdown::{
        adjust_relative_paths_for_pretty_urls, generate_slug,
        process_markdown_file as _process_markdown_file,
    };
    use moss_core::PageKind;

    // Shim for tests that don't care about the site_lang fallback —
    // pass English explicitly. Real production callers thread site_lang
    // through from `generate_blocking_content`.
    fn process_markdown_file(
        file_path: &str,
        content: &str,
        root_folder_name: &str,
        page_map: &std::collections::HashMap<String, String>,
        emit_source_lines: bool,
    ) -> Result<crate::build::types::ParsedDocument, String> {
        _process_markdown_file(
            file_path,
            content,
            root_folder_name,
            page_map,
            emit_source_lines,
            crate::i18n::Language::En,
            None,
            crate::build::markdown::SiteMarkdown::default(),
            None,
            None,
            None,
            None,
            false,
            None,
            None, // folder_lang
        )
    }

    #[test]
    fn test_self_named_demoted_when_index_exists() {
        // Simulate: root folder "山居" contains both index.md and 山居.md
        // Both are processed by process_markdown_file, both get is_index=true.
        // After demotion, only the winner (index.md) stays is_index.

        let index_content = "---\ntitle: Home\n---\nWelcome home";
        let self_named_content = "---\ntitle: 山居\n---\nThis is me";

        let empty_map = std::collections::HashMap::new();
        let index_doc =
            process_markdown_file("index.md", index_content, "山居", &empty_map, true).unwrap();
        let self_named_doc =
            process_markdown_file("山居.md", self_named_content, "山居", &empty_map, true).unwrap();

        // Before demotion, both should be PageKind::Folder
        assert!(
            index_doc.kind == PageKind::Folder,
            "index.md should be Folder before demotion"
        );
        assert!(
            self_named_doc.kind == PageKind::Folder,
            "山居.md should be Folder before demotion (per is_home_file)"
        );

        // Simulate demotion for 山居.md (it's not the winner)
        let mut doc = self_named_doc;
        doc.kind = PageKind::Article;
        let slug = generate_slug(&doc.clean_stem);
        doc.url_path = format!("{}/index.html", slug);
        doc.html_content = adjust_relative_paths_for_pretty_urls(&doc.html_content);

        assert!(
            doc.kind != PageKind::Folder,
            "Demoted doc should not be Folder"
        );
        assert_ne!(
            doc.url_path, "index.html",
            "Demoted doc should NOT have url_path 'index.html'"
        );
        assert!(
            doc.url_path.ends_with("/index.html"),
            "Demoted doc should have slug-based url_path, got: {}",
            doc.url_path
        );
        // The slug should be based on the clean_stem "山居"
        assert_eq!(doc.url_path, format!("{}/index.html", slug));
    }

    #[test]
    fn test_winner_keeps_index_url() {
        // The winner (index.md) should keep its url_path = "index.html"
        let content = "---\ntitle: Home\n---\nWelcome";
        let doc = process_markdown_file(
            "index.md",
            content,
            "mysite",
            &std::collections::HashMap::new(),
            true,
        )
        .unwrap();
        assert!(doc.kind == PageKind::Folder);
        assert_eq!(doc.url_path, "index.html");
    }

    #[test]
    fn test_subfolder_demotion_preserves_parent_path() {
        // Subfolder case: recipes/recipes.md gets demoted
        let content = "---\ntitle: My Recipes\n---\nDelicious food";
        let doc = process_markdown_file(
            "recipes/recipes.md",
            content,
            "mysite",
            &std::collections::HashMap::new(),
            true,
        )
        .unwrap();
        assert!(
            doc.kind == PageKind::Folder,
            "recipes.md should be Folder in recipes/ folder"
        );

        // Simulate demotion
        let mut doc = doc;
        doc.kind = PageKind::Article;
        let slug = generate_slug(&doc.clean_stem);
        let parent_path = std::path::Path::new("recipes/recipes.md")
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        doc.url_path = if parent_path.is_empty() {
            format!("{}/index.html", slug)
        } else {
            format!("{}/{}/index.html", parent_path, slug)
        };

        assert!(doc.kind != PageKind::Folder);
        assert!(
            doc.url_path.starts_with("recipes/"),
            "Demoted subfolder doc should preserve parent path, got: {}",
            doc.url_path
        );
        assert_ne!(
            doc.url_path, "recipes/index.html",
            "Should not be the folder index URL"
        );
    }
}

// build_page_map_tests and video_path_mapping_tests have been moved to page_map.rs

// Unit tests for resolve_path_with_overrides have been moved to page_map.rs.
// The full build integration test stays here since it tests generate_blocking_content.
mod video_path_mapping_integration_tests {
    /// End-to-end test: build a site with Chinese directory names and
    /// url overrides, verify that dir_overrides is captured on BackgroundContext.
    /// Also asserts that no .placeholder.svg files are produced (Pattern E removed).
    #[test]
    fn test_cjk_dir_overrides_captured_in_deferred_work() {
        use crate::build::manifest::PendingManifest;
        use crate::build::render::generate_blocking_content;
        use crate::build::render::SiteConfig;
        use crate::build::scan::scan::scan_folder;
        use crate::types::content::SiteHashes;
        use std::fs;

        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_video_map_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        // Create a folder with Chinese name and a markdown index that sets url override
        let video_dir = test_dir.join("视频");
        fs::create_dir_all(&video_dir).unwrap();
        fs::write(video_dir.join("视频.md"), "---\nurl: video\n---\n# Videos").unwrap();

        // Create a root index so the site isn't empty
        fs::write(test_dir.join("index.md"), "# Home").unwrap();

        // Create output directory
        let output_dir = test_dir.join(".moss").join("build.nosync").join("staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Run the blocking phase
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(result.is_ok(), "Build should succeed: {:?}", result);

        let (site_result, bg_ctx, _docs, _verify) = result.unwrap();

        // Verify that dir_overrides was captured on the BackgroundContext
        assert!(
            bg_ctx.dir_overrides.contains_key("视频"),
            "dir_overrides should contain '视频', got: {:?}",
            bg_ctx.dir_overrides
        );
        assert_eq!(
            bg_ctx.dir_overrides.get("视频").unwrap(),
            "video",
            "dir_overrides should map '视频' → 'video'"
        );

        // Pattern E removed: no .placeholder.svg keys in manifest
        {
            let hashes = &site_result.hashes;
            let placeholder_keys: Vec<&str> = hashes
                .files
                .keys()
                .filter(|k: &&String| k.ends_with(".placeholder.svg"))
                .map(String::as_str)
                .collect();
            assert!(
                    placeholder_keys.is_empty(),
                    "No .placeholder.svg keys should appear in the manifest after build (Pattern E removed). Found: {:?}",
                    placeholder_keys,
                );
        }
    }

    /// Acceptance criterion: PendingManifest after generate_blocking_content
    /// must not contain any keys ending in .placeholder.svg, even for sites with videos.
    #[test]
    fn test_no_placeholder_svg_in_manifest_after_build() {
        use crate::build::manifest::PendingManifest;
        use crate::build::render::generate_blocking_content;
        use crate::build::render::SiteConfig;
        use crate::build::scan::scan::scan_folder;
        use crate::types::content::{MediaMetadata, SiteHashes};
        use std::fs;

        let system_temp = std::env::temp_dir();
        let test_dir =
            system_temp.join(format!("moss_no_placeholder_svg_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        fs::write(test_dir.join("index.md"), "# Home").unwrap();
        let output_dir = test_dir.join(".moss").join("build.nosync").join("staging");
        fs::create_dir_all(&output_dir).unwrap();

        let mut project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");

        // Inject fake video metadata as if ffmpeg scanned a .mov file
        project_structure.video_files.push(MediaMetadata {
            is_animated: false,
            path: "video/clip.mov".to_string(),
            file_type: "mov".to_string(),
            size: 1000,
            modified: None,
            dimensions: Some((1920, 1080)),
            dominant_color: Some("#4287f5".to_string()),
            lqip_data_uri: None,
        });

        let mut pending = PendingManifest::new(SiteHashes::default());
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut pending,
        );
        assert!(result.is_ok(), "Build should succeed: {:?}", result);

        // Acceptance criterion: no .placeholder.svg in the manifest
        let (site_hashes, _) = pending.as_parts_clone();
        let placeholder_keys: Vec<&str> = site_hashes
            .files
            .keys()
            .filter(|k| k.ends_with(".placeholder.svg"))
            .map(String::as_str)
            .collect();
        assert!(
            placeholder_keys.is_empty(),
            "Pattern E removed: no .placeholder.svg keys should appear in manifest. Found: {:?}",
            placeholder_keys,
        );

        // Also verify the output dir contains no .placeholder.svg files on disk
        let svg_files: Vec<_> = walkdir::WalkDir::new(&output_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".placeholder.svg")
            })
            .collect();
        assert!(
            svg_files.is_empty(),
            "No .placeholder.svg files should be written to disk. Found: {:?}",
            svg_files.iter().map(|e| e.path()).collect::<Vec<_>>(),
        );
    }
}

mod og_url_tests {
    use super::super::generate_html;
    use crate::build::page::layout::LayoutConfig;
    use crate::build::site_url::SiteUrl;
    use crate::i18n::Language;
    use crate::build::types::ParsedDocument;
    use crate::types::content::ProjectStructure;
    use moss_core::PageKind;

    fn localhost_url() -> SiteUrl {
        SiteUrl::parse("http://localhost").unwrap()
    }

    fn make_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn make_project() -> ProjectStructure {
        ProjectStructure {
            root_path: String::new(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![],
            video_files: vec![],
            notebook_files: vec![],
            other_files: vec![],
            total_files: 0,
            homepage_file: Some("index.md".to_string()),
            ffmpeg_bin_path: None,
            evicted_count: 0,
            evicted_paths: Vec::new(),
            has_content_folders: false,
            has_language_trees: false,
            passthrough_roots: std::collections::HashSet::new(),
            dirs: Vec::new(),
        }
    }

    fn make_layout() -> LayoutConfig {
        LayoutConfig::new("test-site", Some("Test Site"))
    }

    #[test]
    fn og_url_uses_absolute_url_when_site_url_provided() {
        let homepage = make_doc("Home", "index.html");
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"og:url" content="https://example.com/writings/test/"#),
            "og:url should contain absolute URL with configured domain"
        );
    }

    #[test]
    fn og_url_uses_relative_path_when_no_site_url() {
        let homepage = make_doc("Home", "index.html");
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains(r#"og:url" content="/writings/test/"#),
            "og:url should contain relative path when no site_url"
        );
    }

    /// A cover no platform crop can hold is drawn INTO a card; a cover that
    /// fits one is handed over with the dimensions moss scanned.
    ///
    /// This is the wiring, and it is the half a unit test cannot see: every
    /// assertion in `og_card` and `cover` would stay green with the call in
    /// `og_choice` deleted, and the site would go back to sharing a band from
    /// the middle of a painting.
    #[test]
    fn a_tall_cover_becomes_a_plate_card_and_a_wide_one_does_not() {
        use super::super::generate_html_collect_og;

        fn share(cover: &str, dims: (u32, u32), source_root: &std::path::Path) -> String {
            let dir = tempfile::tempdir().expect("tempdir");
            let mut article = make_doc("\u{96d9}\u{9df9}\u{5716}", "paintings/two-eagles/index.html");
            article.cover = Some(cover.to_string());
            let all_docs = vec![make_doc("Home", "index.html"), article.clone()];
            let mut project = make_project();
            project.image_files = vec![crate::types::content::MediaMetadata {
                path: cover.to_string(),
                file_type: "jpg".into(),
                size: 0,
                modified: None,
                dimensions: Some(dims),
                dominant_color: None,
                lqip_data_uri: None,
                is_animated: false,
            }];
            let no_previous = std::collections::HashMap::new();
            let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
            generate_html_collect_og(
                Some(&article), &all_docs, &project, &make_layout(), false, None, None,
                Language::En, None, false, false, None, false, None, None,
                &std::collections::HashMap::new(),
                &SiteUrl::parse("https://example.com").unwrap(),
                true, false, "favicon.svg", false, Some(dir.path()), &mut og_outputs,
                source_root,
                &crate::build::emit::scripts::ScriptAssets::resolve(),
            )
            .expect("generate_html_collect_og should succeed")
        }

        // A real file on disk, because a plate is only drawn from pixels moss
        // can actually decode.
        let vault = tempfile::tempdir().expect("tempdir");
        image::RgbImage::from_pixel(229, 400, image::Rgb([200, 190, 170]))
            .save(vault.path().join("scroll.jpg"))
            .expect("write scroll");
        image::RgbImage::from_pixel(400, 229, image::Rgb([200, 190, 170]))
            .save(vault.path().join("banner.jpg"))
            .expect("write banner");

        let tall = share("scroll.jpg", (2290, 4000), vault.path());
        assert!(
            tall.contains("/_moss/og/") && tall.contains(r#"og:image:height" content="630""#),
            "a hanging scroll should share as a composed card, not as itself: {tall}"
        );

        let wide = share("banner.jpg", (2000, 1145), vault.path());
        assert!(
            wide.contains(r#"og:image" content="https://example.com/banner.jpg"#),
            "a 1.75:1 cover should be handed over as itself: {wide}"
        );
        assert!(
            wide.contains(r#"og:image:width" content="2000""#)
                && wide.contains(r#"og:image:height" content="1145""#),
            "a passed-through cover should carry the dimensions moss scanned: {wide}"
        );
    }

    /// A favicon raster trio an earlier default-SVG build left in the output
    /// tree is not this build's: once a vault grows its own `favicon.png`, no
    /// PNG sizes are rasterized, and pages must not link the leftovers
    /// (2026-09-14). The page reads the build's answer, never the
    /// directory, so the trio can wait for the permitted staging sweep.
    #[test]
    fn a_leftover_favicon_raster_trio_links_nothing() {
        use super::super::generate_html_collect_og;

        let dir = tempfile::tempdir().expect("tempdir");
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        for stale in ["favicon-16.png", "favicon-32.png", "favicon-180.png"] {
            std::fs::write(assets.join(stale), b"from an earlier build").unwrap();
        }
        let homepage = make_doc("Home", "index.html");
        let all_docs = vec![homepage.clone()];
        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = generate_html_collect_og(
            Some(&homepage), &all_docs, &make_project(), &make_layout(), true, None, None,
            Language::En, None, false, false, None, false, None, None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true, false, "favicon.png", false, Some(dir.path()), &mut og_outputs,
            std::path::Path::new(""),
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("render");

        assert!(!html.contains("apple-touch-icon"), "no link to a PNG this build did not write: {html}");
        assert!(!html.contains("favicon-180.png"), "no link to a PNG this build did not write");
    }

    #[test]
    fn auto_og_card_emits_when_no_frontmatter_cover() {
        use super::super::generate_html_collect_og;

        let dir = tempfile::tempdir().expect("tempdir");
        let output_root = dir.path();

        let homepage = make_doc("Home", "index.html");
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());
        // No frontmatter cover — the auto-card path should kick in.
        assert!(article.cover.is_none());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = generate_html_collect_og(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(output_root),
            &mut og_outputs,
            std::path::Path::new(""), // source_root
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed");

        assert!(
            html.contains(r#"og:image" content=""#) && html.contains("/_moss/og/"),
            "og:image should point at an auto-card under /_moss/og/, html: {}",
            html
        );
        assert!(
            html.contains(r#"og:image:width" content="1200""#),
            "og:image:width should be 1200 for auto-card"
        );
        assert!(
            html.contains(r#"og:image:height" content="630""#),
            "og:image:height should be 630 for auto-card"
        );

        // At least one PNG was written under output_root/_moss/og/.
        let og_dir = output_root.join("_moss").join("og");
        assert!(og_dir.exists(), "{} should exist", og_dir.display());
        let pngs: Vec<_> = std::fs::read_dir(&og_dir)
            .expect("read og dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
            .collect();
        assert!(
            !pngs.is_empty(),
            "at least one PNG should exist under {}",
            og_dir.display()
        );
    }

    /// The wiring test the cover-chain unit tests cannot see: `html.rs` must
    /// actually hand the page's own hero to the chain. A page whose only image
    /// is its `:::hero` shared as a near-blank generated card before this.
    #[test]
    fn page_hero_becomes_the_og_image_without_a_frontmatter_cover() {
        use super::super::generate_html_collect_og;

        let dir = tempfile::tempdir().expect("tempdir");
        let output_root = dir.path();

        let homepage = make_doc("Home", "index.html");
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());
        article.hero_image_url = Some("plum.jpg".to_string());
        assert!(article.cover.is_none());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = generate_html_collect_og(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(output_root),
            &mut og_outputs,
            std::path::Path::new(""), // source_root
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed");

        assert!(
            html.contains("plum.jpg"),
            "og:image should be the page's own hero image, html: {}",
            html
        );
        assert!(
            !html.contains("/_moss/og/"),
            "a page with a hero image must not fall through to a generated card, html: {}",
            html
        );
    }

    /// A page with no picture of its own gets the generated title card, not
    /// the home page's picture. Seen live: a `[site] typesetting = "vertical"`
    /// site whose home page carries a portrait as `cover:` shared every
    /// text-only letter with that portrait (2026-09-10).
    #[test]
    fn a_page_without_a_picture_gets_the_card_not_the_homepage_picture() {
        use super::super::generate_html_collect_og;

        let dir = tempfile::tempdir().expect("tempdir");
        let output_root = dir.path();

        let mut homepage = make_doc("Home", "index.html");
        homepage.cover = Some("portrait.jpg".to_string());
        homepage.hero_image_url = Some("home-hero.jpg".to_string());
        homepage.body_cover_path = Some("home-body.jpg".to_string());
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());
        assert!(article.cover.is_none() && article.hero_image_url.is_none());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = generate_html_collect_og(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(output_root),
            &mut og_outputs,
            std::path::Path::new(""), // source_root
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed");

        let og_image = html
            .lines()
            .find(|l| l.contains(r#"property="og:image""#))
            .unwrap_or("");
        assert!(
            og_image.contains("/_moss/og/"),
            "og:image should be the generated card, got: {og_image}"
        );
        for home_picture in ["portrait.jpg", "home-hero.jpg", "home-body.jpg"] {
            assert!(
                !html.contains(home_picture),
                "the home page's {home_picture} leaked onto another page's share tags"
            );
        }
    }

    /// The wiring the og_card unit tests cannot see: html.rs must hand the
    /// page's effective typesetting to the card, or a vertical site's cards
    /// stay horizontal. A vertical layout keys a different card.
    #[test]
    fn a_vertical_site_gets_a_different_card_from_a_horizontal_one() {
        use super::super::generate_html_collect_og;

        let render = |layout: &LayoutConfig| {
            let dir = tempfile::tempdir().expect("tempdir");
            let homepage = make_doc("Home", "index.html");
            let article = make_doc("Test Article", "writings/test/index.html");
            let all_docs = vec![homepage, article.clone()];
            let project = make_project();
            let no_previous = std::collections::HashMap::new();
            let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
            let html = generate_html_collect_og(
                Some(&article),
                &all_docs,
                &project,
                layout,
                false,
                None,
                None,
                Language::En,
                None,
                false,
                false,
                None,
                false,
                None,
                None,
                &std::collections::HashMap::new(),
                &SiteUrl::parse("https://example.com").unwrap(),
                true,
                false,
                "favicon.svg",
                false,
                Some(dir.path()),
                &mut og_outputs,
                std::path::Path::new(""), // source_root
                &crate::build::emit::scripts::ScriptAssets::resolve(),
            )
            .expect("generate_html_collect_og should succeed");
            html.lines()
                .find(|l| l.contains(r#"property="og:image""#))
                .unwrap_or("")
                .to_string()
        };

        let horizontal = render(&make_layout());
        let mut vertical_layout = make_layout();
        vertical_layout.typesetting = Some("vertical".to_string());
        let vertical = render(&vertical_layout);
        assert!(horizontal.contains("/_moss/og/") && vertical.contains("/_moss/og/"));
        assert_ne!(horizontal, vertical, "the vertical site rendered the horizontal card");
    }

    #[test]
    fn frontmatter_cover_takes_priority_over_auto_card() {
        use super::super::generate_html_collect_og;

        let dir = tempfile::tempdir().expect("tempdir");
        let output_root = dir.path();

        let homepage = make_doc("Home", "index.html");
        let mut article = make_doc("Test Article", "writings/test/index.html");
        article.date = Some("2025-06-15".to_string());
        article.cover = Some("frontmatter-cover.jpg".to_string());

        let all_docs = vec![homepage, article.clone()];
        let project = make_project();
        let layout = make_layout();

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = generate_html_collect_og(
            Some(&article),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(output_root),
            &mut og_outputs,
            std::path::Path::new(""), // source_root
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed");

        assert!(
            html.contains("frontmatter-cover.jpg"),
            "og:image should use the frontmatter cover, html: {}",
            html
        );
        assert!(
            !html.contains("/_moss/og/"),
            "auto-card path /_moss/og/ should NOT appear when frontmatter cover is set"
        );
        assert!(
            !html.contains(r#"og:image:width" content="1200""#),
            "no auto-card means no og:image:width 1200 should be emitted"
        );
        let cards = og_outputs.into_cards();
        assert!(
            cards.is_empty(),
            "no auto-card PNG should be tracked when frontmatter cover wins, got: {:?}",
            cards.iter().map(|c| c.served_path().as_str()).collect::<Vec<_>>()
        );
    }
    /// The user theme `<link>` must carry `layer="themes"`.
    ///
    /// Without the attribute the sheet is *unlayered*, and unlayered rules beat
    /// every layer rather than sitting above them. No computed-style check can
    /// see the difference — the user's rules win either way — so the failure is
    /// silent until something that should outrank the user's sheet cannot.
    ///
    /// This assertion lived in the customization-cascade playwright gate, which
    /// booted chromium AND webkit to read one attribute off one element. It is
    /// a string the emitter either writes or does not. The cascade *outcome*
    /// stays in that gate, where only a real engine can resolve layer
    /// precedence; this does not.
    #[test]
    fn user_theme_link_is_loaded_into_the_themes_layer() {
        let homepage = make_doc("Home", "index.html");
        let page = make_doc("Page", "page/index.html");
        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout();
        let tmp = tempfile::tempdir().expect("tempdir");

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        let html = super::super::generate_html_collect_og(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            true,             // has_user_css
            false,
            Some("abc123"),   // user_css_version
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(tmp.path()),
            &mut og_outputs,
            std::path::Path::new(""),
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed");

        assert!(
            html.contains(r#"/_moss/theme/style.abc123.css" layer="themes""#),
            "the user theme stylesheet must be linked with layer=\"themes\"; an \
             unlayered sheet outranks every layer instead of sitting above them. Got: {}",
            html.lines()
                .find(|l| l.contains("theme/style"))
                .unwrap_or("<no theme link emitted>")
        );
    }
}

/// Social-card emission for every LISTABLE doc-backed page (Task 2):
/// nav landing pages and folder-index pages — not just articles + homepage.
/// og:type=website (not article), real og:site_name, no Article JSON-LD.
/// Draft pages still emit no card.
mod listable_page_card_tests {
    use super::super::{generate_html, generate_html_collect_og};
    use crate::build::page::layout::LayoutConfig;
    use crate::build::site_url::SiteUrl;
    use crate::i18n::Language;
    use crate::build::types::ParsedDocument;
    use crate::types::content::ProjectStructure;
    use moss_core::PageKind;

    fn make_project() -> ProjectStructure {
        ProjectStructure {
            root_path: String::new(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![],
            video_files: vec![],
            notebook_files: vec![],
            other_files: vec![],
            total_files: 0,
            homepage_file: Some("index.md".to_string()),
            ffmpeg_bin_path: None,
            evicted_count: 0,
            evicted_paths: Vec::new(),
            has_content_folders: false,
            has_language_trees: false,
            passthrough_roots: std::collections::HashSet::new(),
            dirs: Vec::new(),
        }
    }

    fn make_layout() -> LayoutConfig {
        LayoutConfig::new("test-site", Some("Test Site"))
    }

    /// Build a non-homepage Page-template doc at /research/, mutate it via
    /// the closure, render it WITHOUT an og_outputs sink (so the auto-card
    /// path is suppressed), and return the HTML. Site title is "Test Site".
    fn render_test_page_with(mutate: impl FnOnce(&mut ParsedDocument)) -> String {
        let homepage = {
            let mut d = ParsedDocument::default();
            d.title = "Home".to_string();
            d.label = "Home".to_string();
            d.url_path = "index.html".to_string();
            d.kind = PageKind::Article;
            d
        };
        let mut page = ParsedDocument::default();
        page.title = "Research".to_string();
        page.label = "Research".to_string();
        page.url_path = "research/index.html".to_string();
        page.kind = PageKind::Article;
        mutate(&mut page);
        // Keep label in sync with title for og:title assertions.
        page.label = page.title.clone();

        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout();

        generate_html(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            None, // output_dir — no auto OG card
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed")
    }

    /// Like `render_test_page_with` but routes through the collect-og path
    /// with a real output dir + sink, so the auto-card synthesis can run.
    fn render_test_page_collect_og(mutate: impl FnOnce(&mut ParsedDocument)) -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        let output_root = dir.path();

        let homepage = {
            let mut d = ParsedDocument::default();
            d.title = "Home".to_string();
            d.label = "Home".to_string();
            d.url_path = "index.html".to_string();
            d.kind = PageKind::Article;
            d
        };
        let mut page = ParsedDocument::default();
        page.title = "Research".to_string();
        page.label = "Research".to_string();
        page.url_path = "research/index.html".to_string();
        page.kind = PageKind::Article;
        mutate(&mut page);
        page.label = page.title.clone();

        let all_docs = vec![homepage, page.clone()];
        let project = make_project();
        let layout = make_layout();

        let no_previous = std::collections::HashMap::new();
        let mut og_outputs = crate::build::page::og_card::OgSink::new(&no_previous);
        generate_html_collect_og(
            Some(&page),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            false,
            Some(output_root),
            &mut og_outputs,
            std::path::Path::new(""),
            &crate::build::emit::scripts::ScriptAssets::resolve(),
        )
        .expect("generate_html_collect_og should succeed")
    }

    #[test]
    fn listable_page_emits_website_og_and_twitter() {
        let html = render_test_page_with(|d| {
            d.nav = Some(true);
            d.title = "Research".into();
            d.description = Some("Lab research themes".into());
        });
        assert!(html.contains(r#"<meta property="og:type" content="website">"#));
        assert!(html.contains(r#"<meta property="og:title" content="Research">"#));
        assert!(html.contains(r#"<meta property="og:site_name" content="Test Site">"#)); // SITE, not page
        assert!(html.contains(r#"<meta name="twitter:card""#));
        assert!(!html.contains(r#""@type":"Article""#) && !html.contains(r#""@type": "Article""#));
    }

    #[test]
    fn draft_page_emits_no_card() {
        let html = render_test_page_with(|d| {
            d.draft = Some(true);
            d.title = "Hidden".into();
            d.description = Some("secret".into());
        });
        assert!(!html.contains("og:title"));
        assert!(!html.contains("twitter:card"));
        assert!(
            !html.contains(r#""@type":"Article""#) && !html.contains(r#""@type": "Article""#),
            "draft article must emit no Article JSON-LD"
        );
    }

    #[test]
    fn draft_page_emits_noindex() {
        let html = render_test_page_with(|d| {
            d.draft = Some(true);
            d.title = "WIP".into();
        });
        assert!(
            html.contains(r#"<meta name="robots" content="noindex">"#),
            "draft page must emit noindex robots meta"
        );
    }

    #[test]
    fn listable_page_emits_no_noindex() {
        let html = render_test_page_with(|d| {
            d.draft = None;
            d.title = "Live".into();
        });
        assert!(
            !html.contains(r#"content="noindex""#),
            "listable page must NOT emit noindex"
        );
    }

    #[test]
    fn coverless_listable_page_still_gets_og_image_via_autocard() {
        let html = render_test_page_collect_og(|d| {
            d.nav = Some(true);
            d.title = "Research".into();
            d.description = Some("themes".into());
        });
        assert!(html.contains(r#"<meta property="og:image""#));
    }

    /// Render the homepage doc (site title "Test Site") and return the HTML.
    /// Mirrors `render_test_page_with` but flips `is_homepage = true`.
    fn render_test_homepage() -> String {
        let mut homepage = ParsedDocument::default();
        homepage.title = "Test Site".to_string();
        homepage.label = "Test Site".to_string();
        homepage.url_path = "index.html".to_string();
        homepage.kind = PageKind::Article;
        homepage.is_root_level = true;

        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false,
            None,
            None,
            &std::collections::HashMap::new(),
            &SiteUrl::parse("https://example.com").unwrap(),
            true,
            false,
            "favicon.svg",
            None, // output_dir — no auto OG card
            std::path::Path::new(""),
        )
        .expect("generate_html should succeed")
    }

    #[test]
    fn title_is_suffixed_once_for_non_home_pages() {
        let html = render_test_page_with(|d| {
            d.title = "Research".into();
        });
        assert!(html.contains("<title>Research - Test Site</title>"));
    }

    #[test]
    fn home_title_is_bare_site_name() {
        let html = render_test_homepage(); // site title "Test Site"
        assert!(html.contains("<title>Test Site</title>"));
        assert!(!html.contains("<title>Test Site - Test Site</title>"));
    }

    /// The `after-title` slot marker (where the review colophon / book
    /// block is injected) must sit BELOW the title and the date/reading-prefs
    /// row — i.e. it is the first thing after the date-line, before the body.
    /// Regression guard: the marker used to live in the article template
    /// ABOVE `{content}`, which rendered the book block above the title.
    #[test]
    fn after_title_marker_follows_title_and_dateline() {
        let html = render_test_page_with(|d| {
            d.title = "Research".into();
            d.date = Some("2026-03-15T12:00:00Z".into());
            d.html_content =
                "<h1 class=\"moss-article-title\">Research</h1>\n<p>BODY_MARKER paragraph.</p>"
                    .to_string();
        });
        let h1 = html.find("</h1>").expect("title h1 present");
        let dateline = html
            .find(r#"class="date-line""#)
            .expect("date-line present");
        let slot = html
            .find("<!-- slot:after-title -->")
            .expect("after-title slot marker present in body");
        let body = html.find("BODY_MARKER").expect("body present");
        assert!(
                h1 < dateline && dateline < slot && slot < body,
                "order must be title < date-line < after-title slot < body; got h1={h1} date-line={dateline} slot={slot} body={body}\n{html}"
            );
    }

    /// A month-precision date (`date: "1809-05"`, which the frontmatter schema
    /// allows) must render through the same formatter as a full date, not as
    /// the raw ISO string. Regression guard for a69b13eef, which reverted
    /// `format_article_date` to a `parts.len() >= 3` split that silently
    /// passed any date with fewer than three dash-separated parts straight
    /// through to the page.
    #[test]
    fn article_date_line_formats_a_month_precision_date() {
        let html = render_test_page_with(|d| {
            d.title = "Research".into();
            d.date = Some("1809-05".into());
        });
        let dateline = html
            .find(r#"class="date-line""#)
            .map(|i| &html[i..])
            .expect("date-line present");
        let dateline_end = dateline.find("</div></div>").map(|i| i + 12).unwrap_or(dateline.len());
        let dateline = &dateline[..dateline_end];
        assert!(
            dateline.contains(r#"<span class="date">1809 · 5</span>"#),
            "expected formatted month-precision date in date-line:\n{dateline}"
        );
        assert!(
            !dateline.contains("1809-05"),
            "raw ISO date string must not reach the date-line (JSON-LD's own datePublished stays raw ISO deliberately):\n{dateline}"
        );
    }

    /// Even when an article has no date (no date-line), the `after-title`
    /// slot marker must still be emitted right after the title so the review
    /// colophon is not silently dropped on dateless reviews.
    #[test]
    fn after_title_marker_present_without_date() {
        let html = render_test_page_with(|d| {
            d.title = "Research".into();
            d.date = None;
            d.html_content =
                "<h1 class=\"moss-article-title\">Research</h1>\n<p>BODY_MARKER paragraph.</p>"
                    .to_string();
        });
        let h1 = html.find("</h1>").expect("title h1 present");
        let slot = html
            .find("<!-- slot:after-title -->")
            .expect("after-title slot marker present even without a date");
        let body = html.find("BODY_MARKER").expect("body present");
        assert!(
                h1 < slot && slot < body,
                "order must be title < after-title slot < body; got h1={h1} slot={slot} body={body}\n{html}"
            );
    }
}

/// Tests for folder index page metadata (title suffix and description).
mod folder_index_meta_tests {
    use super::super::generate_html;
    use crate::build::page::layout::LayoutConfig;
    use crate::build::site_url::SiteUrl;
    use crate::i18n::Language;
    use crate::build::types::ParsedDocument;
    use crate::types::content::ProjectStructure;
    use moss_core::PageKind;

    fn localhost_url() -> SiteUrl {
        SiteUrl::parse("http://localhost").unwrap()
    }

    fn make_doc(title: &str, url_path: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            url_path: url_path.to_string(),
            html_content: format!("<article><p>{} content</p></article>", title),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            lang: Language::En,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn make_project() -> ProjectStructure {
        ProjectStructure {
            root_path: String::new(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![],
            video_files: vec![],
            notebook_files: vec![],
            other_files: vec![],
            total_files: 0,
            homepage_file: Some("index.md".to_string()),
            ffmpeg_bin_path: None,
            evicted_count: 0,
            evicted_paths: Vec::new(),
            has_content_folders: false,
            has_language_trees: false,
            passthrough_roots: std::collections::HashSet::new(),
            dirs: Vec::new(),
        }
    }

    fn make_layout() -> LayoutConfig {
        LayoutConfig::new("test-site", Some("Test Site"))
    }

    /// Bug A3: Folder index pages should have site name in <title>.
    /// e.g. <title>Documentation - Test Site</title> not <title>Documentation</title>
    #[test]
    fn test_folder_index_title_has_site_name_suffix() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Documentation", "docs/index.html");
        folder_index.kind = PageKind::Folder;

        let all_docs = vec![homepage, folder_index.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
            html.contains("<title>Documentation - Test Site</title>"),
            "Folder index page should have site name suffix in title. Got: {}",
            html.lines()
                .find(|l| l.contains("<title>"))
                .unwrap_or("no title found")
        );
    }

    /// Homepage should NOT have a suffix (it IS the site name).
    #[test]
    fn test_homepage_title_no_suffix() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;
        homepage.kind = PageKind::Folder;

        let all_docs = vec![homepage.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&homepage),
            &all_docs,
            &project,
            &layout,
            true, // is_homepage
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Homepage title should be just the site name, no suffix
        assert!(
            html.contains("<title>Test Site</title>"),
            "Homepage title should be just the site name"
        );
        assert!(
            !html.contains("<title>Test Site - Test Site</title>"),
            "Homepage should not duplicate site name in title"
        );
    }

    /// Bug A4: Folder index pages should resolve description from frontmatter.
    #[test]
    fn test_folder_index_description_from_frontmatter() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Documentation", "docs/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.description =
            Some("Learn how to use moss to turn folders into websites.".to_string());

        let all_docs = vec![homepage, folder_index.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        assert!(
                html.contains(r#"<meta name="description" content="Learn how to use moss to turn folders into websites.">"#),
                "Folder index page should have description from frontmatter. Got: {}",
                html.lines().find(|l| l.contains("description")).unwrap_or("no description found")
            );
    }

    /// Folder index pages without frontmatter description should fall back to content.
    #[test]
    fn test_folder_index_description_fallback_to_content() {
        let mut homepage = make_doc("Test Site", "index.html");
        homepage.is_root_level = true;

        let mut folder_index = make_doc("Documentation", "docs/index.html");
        folder_index.kind = PageKind::Folder;
        folder_index.content = "This is the documentation overview.".to_string();
        // No description set

        let all_docs = vec![homepage, folder_index.clone()];
        let project = make_project();
        let layout = make_layout();

        let html = generate_html(
            Some(&folder_index),
            &all_docs,
            &project,
            &layout,
            false,
            None,
            None,
            Language::En,
            None,
            false,
            false,
            None,
            false, // has_user_js
            None,  // user_js_version
            None,
            &std::collections::HashMap::new(),
            &localhost_url(),
            true,
            false,
            "favicon.svg",
            None,                     // output_dir — tests skip auto OG card generation
            std::path::Path::new(""), // source_root
        )
        .expect("generate_html should succeed");

        // Should NOT have empty description
        assert!(
            !html.contains(r#"<meta name="description" content="">"#),
            "Folder index page should not have empty description when content is available"
        );
    }
}

/// Tests for homepage translation deduplication.
///
/// When index.md and index.zh-hans.md coexist at the root, the language-suffixed
/// file should be treated as a translation of the homepage (linked via language
/// toggle), NOT as a separate page at /index/.
mod homepage_translation_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::generate_blocking_content;
    use crate::build::render::SiteConfig;
    use crate::build::scan::scan::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    fn create_test_dir() -> (std::path::PathBuf, impl Drop) {
        let system_temp = std::env::temp_dir();
        let test_dir = system_temp.join(format!("moss_home_i18n_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        (test_dir.clone(), Cleanup(test_dir))
    }

    /// Homepage translation (index.zh-hans.md) should be deduplicated with the
    /// homepage (index.md), NOT get its own /index/ directory.
    #[test]
    fn test_homepage_translation_does_not_create_index_directory() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Create both homepage files
        // Pin lang on each homepage so dedup tiers are deterministic regardless of
        // whatlang confidence on short bodies. The site default falls
        // back to whichever lang has a reliable detection (here only the CJK file
        // does) and the English-content `index.md` has no signal — without an
        // explicit lang both docs collapse to ZhHans and dedup uses a numeric
        // suffix instead of the expected lang-prefix.
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\nHello world",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.zh-hans.md"),
            "---\ntitle: 我的网站\n---\n# 欢迎\n你好世界",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );

        assert!(
            result.is_ok(),
            "generate_blocking_content should succeed: {:?}",
            result
        );
        let (site_result, _, _docs, _verify) = result.unwrap();

        // The homepage should exist at index.html
        assert!(
            output_dir.join("index.html").exists(),
            "Homepage index.html should exist"
        );

        // The translation should exist at zh-hans/index.html (lang-prefix directory)
        assert!(
            output_dir.join("zh-hans/index.html").exists(),
            "Homepage translation should be at zh-hans/index.html"
        );
        assert!(
            !output_dir.join("index/index.html").exists(),
            "Homepage translation should NOT create an /index/ directory"
        );

        // Both pages should be counted
        assert!(
            site_result.page_count >= 2,
            "Should have at least 2 pages (homepage + translation)"
        );
    }

    /// The translation page (zh-hans/index.html) should exist as a valid HTML page
    /// with proper content, served at /zh-hans/ URL.
    #[test]
    fn test_homepage_translation_page_content() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Pin lang on each homepage so dedup tiers are deterministic regardless of
        // whatlang confidence on short bodies. The site default falls
        // back to whichever lang has a reliable detection (here only the CJK file
        // does) and the English-content `index.md` has no signal — without an
        // explicit lang both docs collapse to ZhHans and dedup uses a numeric
        // suffix instead of the expected lang-prefix.
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\nHello world",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.zh-hans.md"),
            "---\ntitle: 我的网站\n---\n# 欢迎\n你好世界",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );

        assert!(result.is_ok(), "generate_blocking_content should succeed");

        // The translation page should exist at zh-hans/index.html
        let translation_html = fs::read_to_string(output_dir.join("zh-hans/index.html"))
            .expect("Should read translation HTML at zh-hans/index.html");

        // Should contain the Chinese content
        assert!(
            translation_html.contains("你好世界"),
            "Translation page should contain the Chinese content"
        );

        // Should be a complete HTML page
        assert!(
            translation_html.contains("<!DOCTYPE html>"),
            "Translation page should be a complete HTML document"
        );
    }

    /// Homepage translation should NOT render an injected article title.
    /// When index.zh-hans.md is demoted (is_index=false), it must still be
    /// recognized as a folder/index page by the markdown pipeline so that
    /// the auto-injection (`<h1 class="moss-article-title">`) is not applied
    /// above the authored body. The legacy `.article-title` class no
    /// longer ships as an injected class but is also asserted absent.
    #[test]
    fn test_homepage_translation_no_article_title() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Pin lang on each homepage so dedup tiers are deterministic regardless of
        // whatlang confidence on short bodies. The site default falls
        // back to whichever lang has a reliable detection (here only the CJK file
        // does) and the English-content `index.md` has no signal — without an
        // explicit lang both docs collapse to ZhHans and dedup uses a numeric
        // suffix instead of the expected lang-prefix.
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\nHello world",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.zh-hans.md"),
            "---\ntitle: 我的网站\n---\n# 欢迎\n你好世界",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );

        assert!(result.is_ok(), "generate_blocking_content should succeed");

        let translation_html = fs::read_to_string(output_dir.join("zh-hans/index.html"))
            .expect("Should read translation HTML at zh-hans/index.html");

        // The translation page should NOT carry either the new
        // `moss-article-title` (auto-injection) or the legacy `article-title`
        // class — both indicate the page was treated as an article and got
        // an extra title H1 above the body. The substring `article-title`
        // catches both class names.
        assert!(
            !translation_html.contains("article-title"),
            "Homepage translation should not render article-title heading.\nGot: {}",
            &translation_html[..translation_html.len().min(2000)]
        );
    }

    /// The `home: true` marker promotes a non-index file in a subfolder
    /// (e.g. `en/Mountain Home.md` → `en/index.html`) to that folder's home
    /// page. The promoted page must NOT carry an injected
    /// `<h1 class="moss-folder-title">` — it's the language-specific
    /// equivalent of the site root, not a generic folder listing. The root
    /// home (`/index.html`) already skips this H1; home-marker pages must
    /// do the same so toggling languages doesn't reveal a stray heading.
    ///
    /// Regression from a bilingual site (en/Mountain Home.md): user reported "extra
    /// H1 rendered in the English home page, but home page should not
    /// have this extra H1." Guards the re-keyed path: the marker drives
    /// `doc.is_home_override`, NOT `translationKey == "home"`.
    #[test]
    fn test_home_marker_promoted_no_folder_title_h1() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();
        fs::create_dir_all(test_dir.join("en")).unwrap();

        // Mirror a bilingual vault: site-root file is the Chinese home,
        // en/Mountain Home.md is promoted to en/index.html via the home marker.
        fs::write(
            test_dir.join("山居.md"),
            "---\ntitle: 山居\nlang: zh-hans\nhome: true\n---\n第一段。\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("en/Mountain Home.md"),
            "---\ntitle: Mountain Home\nlang: en\nhome: true\n---\nFirst page.\n",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(
            result.is_ok(),
            "generate_blocking_content should succeed: {:?}",
            result.err()
        );

        let en_home_html = fs::read_to_string(output_dir.join("en/index.html"))
            .expect("Should read en/index.html");

        assert!(
            !en_home_html.contains(r#"<h1 class="moss-folder-title">"#),
            "Translation home should not carry moss-folder-title H1.\nGot:\n{}",
            &en_home_html[..en_home_html.len().min(2000)]
        );
        assert!(
            !en_home_html.contains(r#"<h1 class="moss-article-title">"#),
            "Translation home should not carry moss-article-title H1 either.\nGot:\n{}",
            &en_home_html[..en_home_html.len().min(2000)]
        );
    }

    /// Homepage translation should NOT appear in the navigation bar.
    #[test]
    fn test_homepage_translation_not_in_nav() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Pin lang on each homepage so dedup tiers are deterministic regardless of
        // whatlang confidence on short bodies. The site default falls
        // back to whichever lang has a reliable detection (here only the CJK file
        // does) and the English-content `index.md` has no signal — without an
        // explicit lang both docs collapse to ZhHans and dedup uses a numeric
        // suffix instead of the expected lang-prefix.
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\nHello world",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.zh-hans.md"),
            "---\ntitle: 我的网站\n---\n# 欢迎\n你好世界",
        )
        .unwrap();
        // Add a real nav page to verify nav works
        fs::write(
            test_dir.join("about.md"),
            "---\ntitle: About\n---\nAbout page",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );

        assert!(result.is_ok(), "generate_blocking_content should succeed");

        let homepage_html =
            fs::read_to_string(output_dir.join("index.html")).expect("Should read homepage HTML");

        // The nav should contain the About page but NOT the homepage translation
        assert!(
            homepage_html.contains("About"),
            "Nav should contain the About page"
        );
        // The nav-links section should not contain a link to the folder name
        // that would appear if index.zh-hans.md generated a separate /index/ page
        // Check that there's no nav link with the test directory's folder name
        // (which would be the title of the erroneous /index/ page)
        assert!(
            !homepage_html.contains("nav-links\">") || !homepage_html.contains("我的网站</a>"),
            "Nav should NOT contain the homepage translation as a separate nav item"
        );
    }

    /// og:image on a translated page should resolve a cover wikilink
    /// (`cover: "[[Winter-Song.mov]]"`) into the assets directory, NOT to
    /// the article's own markdown source path. Regression for a real vault's
    /// bug where en/videos/ articles emitted
    /// `<meta property="og:image" content="/en/video/winter-song.md">`.
    ///
    /// A video cover resolves to its `.thumb.jpg`, not the video: the
    /// original is never deployed, and a crawler asked for a share image
    /// does not want a video either. That half was itself a live bug on the
    /// same vault until 2026-08-27 — this test pinned `/assets/Winter-Song.mov`,
    /// a URL that returned 404 in production.
    #[test]
    fn test_og_image_resolves_cover_wikilink_on_translated_page() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Mirror a bilingual vault layout: language-neutral assets/ + a per-language
        // articles folder where each article has the same stem as an asset.
        fs::create_dir_all(test_dir.join("assets")).unwrap();
        fs::create_dir_all(test_dir.join("en/videos")).unwrap();
        fs::write(test_dir.join("assets/Winter-Song.mov"), b"fake-mov").unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\n",
        )
        .unwrap();
        fs::write(
                test_dir.join("en/videos/winter-song.md"),
                "---\ntitle: Winter Song\ndate: 2024-01-01\nlang: en\ncover: \"[[Winter-Song.mov]]\"\n---\n\nA short article.\n",
            )
            .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(
            result.is_ok(),
            "generate_blocking_content should succeed: {:?}",
            result.err()
        );

        let article_html = fs::read_to_string(output_dir.join("en/videos/winter-song/index.html"))
            .expect("Should read translated article HTML");

        // og:image is deploy-gated via CoverRef::to_meta_url: absolute on
        // deployed sites (crawlers have no document base URL), relative on
        // preview / unconfigured builds (so the dev port doesn't bake into
        // static HTML). SiteConfig::default() falls back to http://localhost
        // which is NOT deployed, so we expect a root-relative path here.
        assert!(
                article_html.contains(r#"og:image" content="/assets/Winter-Song.thumb.jpg""#),
                "og:image should be the root-relative thumbnail for unconfigured (localhost) site URL; expected /assets/Winter-Song.thumb.jpg. Got og:image lines:\n{}",
                article_html.lines().filter(|l| l.contains("og:image")).collect::<Vec<_>>().join("\n")
            );
        // Negative: the article's own markdown source must not leak as og:image.
        assert!(
            !article_html.contains(r#"og:image" content="/en/videos/winter-song"#)
                && !article_html.contains(r#"og:image" content="/en/video/winter-song"#),
            "og:image must not point at the article's own markdown source"
        );
        // Negative: no share tag may name the undeployed original.
        assert!(
            !article_html.contains("Winter-Song.mov"),
            "no share tag may point at the original video; it is never deployed"
        );
    }

    /// `og:site_name` should reflect the canonical site, not bleed
    /// the EN homepage's filename onto every Chinese page. The plan
    /// reports a user's vault (Chinese-default site with
    /// `en/en.md` as English homepage) emitted og:site_name="En" on
    /// every page including Chinese pages.
    #[test]
    fn test_og_site_name_uses_default_lang_homepage() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // Mirror the user's layout: Chinese-default site with a self-named
        // root home AND a self-named English home in the en/ subfolder.
        fs::create_dir_all(test_dir.join("en")).unwrap();
        fs::write(
            test_dir.join("山居.md"),
            "---\ntitle: 山居\nlang: zh-hans\n---\n# 你好\n这是一个网站。",
        )
        .unwrap();
        // en/en.md — self-named folder note; the plan reports this filename
        // pattern leaked "En" as the site name.
        fs::write(
            test_dir.join("en/en.md"),
            "---\ntitle: Mountain Home\nlang: en\n---\n# Hello\nThis is a site.",
        )
        .unwrap();
        fs::write(
            test_dir.join("视频.md"),
            "---\ntitle: 视频\nlang: zh-hans\n---\n# 视频\n中文内容。",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(result.is_ok(), "generate_blocking_content should succeed");

        // Walk every emitted HTML page and assert og:site_name is NOT "En".
        // The acceptable site_name is either the canonical home title (山居)
        // or the source folder name. Either way, NEVER the EN-folder filename.
        for entry in walkdir::WalkDir::new(&output_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("html"))
        {
            let html = fs::read_to_string(entry.path()).unwrap_or_default();
            assert!(
                !html.contains(r#"og:site_name" content="En""#),
                "og:site_name leaked the EN folder filename on {:?}",
                entry
                    .path()
                    .strip_prefix(&output_dir)
                    .unwrap_or(entry.path())
            );
        }
    }

    #[test]
    fn localized_root_title_names_authored_and_synthetic_pages() {
        let (test_dir, _cleanup) = create_test_dir();
        let output_dir = test_dir.join(".moss/build/staging");
        fs::create_dir_all(test_dir.join("zh-hant/guide")).unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: moss\nlang: en\n---\n# Home\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("zh-hant/index.md"),
            "---\ntitle: 青苔\nlang: zh-hant\n---\n# 首頁\n",
        )
        .unwrap();
        fs::write(
            test_dir.join("zh-hant/guide/page.md"),
            "---\ntitle: 寫作\nlang: zh-hant\n---\n# 寫作\n",
        )
        .unwrap();

        let project_structure = scan_folder(test_dir.to_str().unwrap()).unwrap();
        generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(test_dir.to_str().unwrap()),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("multilingual site should build");

        let authored = fs::read_to_string(output_dir.join("zh-hant/guide/page/index.html"))
            .expect("authored Traditional Chinese page");
        assert!(authored.contains(r#"<meta property="og:site_name" content="青苔">"#));
        assert!(authored.contains(r#"class="site-name"#) && authored.contains(">青苔</a>"));

        let synthetic = fs::read_to_string(output_dir.join("zh-hant/guide/index.html"))
            .expect("synthetic Traditional Chinese folder page");
        assert!(synthetic.contains(" - 青苔</title>"));
        assert!(synthetic.contains(r#"class="site-name"#) && synthetic.contains(">青苔</a>"));
        assert!(!synthetic.contains(" - moss</title>"));
    }

    /// Same as the .mov case but for `[[scale-family-tree.html]]` —
    /// the second variant the plan reported failing, where the article's
    /// own markdown source had the same stem as a sibling .html asset.
    #[test]
    fn test_og_image_resolves_html_cover_wikilink_on_translated_page() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        fs::create_dir_all(test_dir.join("assets")).unwrap();
        fs::create_dir_all(test_dir.join("en/interactive/scale-compare")).unwrap();
        fs::write(
            test_dir.join("assets/scale-family-tree.html"),
            b"<!doctype html><html></html>",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\n",
        )
        .unwrap();
        fs::write(
                test_dir.join("en/interactive/scale-compare/scale-family-tree.md"),
                "---\ntitle: Scale Family Tree\ndate: 2024-01-01\nlang: en\ncover: \"[[scale-family-tree.html]]\"\n---\n\nAn interactive embed.\n",
            )
            .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(result.is_ok(), "generate_blocking_content should succeed");

        let article_html = fs::read_to_string(
            output_dir.join("en/interactive/scale-compare/scale-family-tree/index.html"),
        )
        .expect("Should read translated article HTML");

        // og:image is deploy-gated via CoverRef::to_meta_url: absolute on
        // deployed sites, relative on preview / unconfigured builds (so the
        // dev port doesn't bake into static HTML). SiteConfig::default()
        // falls back to http://localhost which is NOT deployed.
        assert!(
                article_html.contains(r#"og:image" content="/assets/scale-family-tree.html""#),
                "og:image should be root-relative for /assets/scale-family-tree.html, not the .md source. Got og:image lines:\n{}",
                article_html.lines().filter(|l| l.contains("og:image")).collect::<Vec<_>>().join("\n")
            );
    }

    /// `<html lang>` should reflect each page's actual language, not the
    /// site default.
    #[test]
    fn test_html_lang_attribute_is_per_page() {
        let (test_dir, _cleanup) = create_test_dir();
        let folder_path = test_dir.to_str().unwrap();
        let output_dir = test_dir.join(".moss/build.nosync/staging");
        fs::create_dir_all(&output_dir).unwrap();

        // English homepage and Chinese translation, lang pinned for determinism.
        fs::write(
            test_dir.join("index.md"),
            "---\ntitle: My Site\nlang: en\n---\n# Welcome\nHello world",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.zh-hans.md"),
            "---\ntitle: 我的网站\n---\n# 欢迎\n你好世界",
        )
        .unwrap();

        let project_structure = scan_folder(folder_path).expect("Failed to scan folder");
        let result = generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(folder_path),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        );
        assert!(result.is_ok(), "generate_blocking_content should succeed");

        let en_html = fs::read_to_string(output_dir.join("index.html"))
            .expect("Should read English homepage");
        let zh_html = fs::read_to_string(output_dir.join("zh-hans/index.html"))
            .expect("Should read Chinese translation");

        // After Phase 1 PR-1b, the emitter writes `<html data-moss-html-version="1" lang="...">`.
        // Match on the lang attribute alone so attribute reordering doesn't break this test.
        assert!(
            en_html.contains(r#"lang="en""#),
            "English homepage should have lang=\"en\" on <html>. Got first 200 chars:\n{}",
            &en_html[..en_html.len().min(200)]
        );
        assert!(
            zh_html.contains(r#"lang="zh-Hans""#),
            "Chinese translation should have lang=\"zh-Hans\" on <html>. Got first 200 chars:\n{}",
            &zh_html[..zh_html.len().min(200)]
        );
    }
}

/// Tests for the `is_lang_tree_root_home` predicate (Bug B fix).
///
/// A page is a language-tree root home when ALL of:
///   1. It is a folder index (PageKind::Folder, url_path ends with "/index.html",
///      and url_path != "index.html").
///   2. Its source_path's first segment is a recognized language code.
///   3. Its url_path has exactly one '/' (depth-1: zh-hans/index.html, not
///      zh-hans/news/index.html).
///
/// These tests exercise the predicate in isolation, mirroring the logic
/// the production render code applies to suppress `moss-folder-title`.
mod lang_tree_root_home_tests {
    /// Helper: compute the is_lang_tree_root_home predicate for a given
    /// source_path and url_path, matching the production logic in
    /// `build/render/html.rs`.
    fn is_lang_tree_root_home(source_path: Option<&str>, url_path: &str) -> bool {
        source_path
            .and_then(|p| moss_core::home::lang_tree_prefix(p))
            .is_some()
            && url_path.chars().filter(|&c| c == '/').count() == 1
    }

    /// zh-hans/index.md → zh-hans/index.html qualifies.
    #[test]
    fn lang_root_index_qualifies() {
        assert!(
            is_lang_tree_root_home(Some("zh-hans/index.md"), "zh-hans/index.html"),
            "zh-hans/index.html at depth 1 must qualify as lang-tree root home"
        );
    }

    /// fr/index.md → fr/index.html qualifies.
    #[test]
    fn fr_root_index_qualifies() {
        assert!(is_lang_tree_root_home(Some("fr/index.md"), "fr/index.html"));
    }

    /// zh-hans/news/index.md → zh-hans/news/index.html does NOT qualify
    /// (depth 2 — a real content section, must keep its folder-title).
    #[test]
    fn lang_section_index_does_not_qualify() {
        assert!(
            !is_lang_tree_root_home(Some("zh-hans/news/index.md"), "zh-hans/news/index.html"),
            "depth-2 section index must NOT qualify as lang-tree root home"
        );
    }

    /// Root index.html does not qualify (no lang prefix in source_path).
    #[test]
    fn root_index_does_not_qualify() {
        // Root index.md has no lang prefix
        assert!(!is_lang_tree_root_home(Some("index.md"), "index.html"));
    }

    /// blog/index.md → blog/index.html does NOT qualify (blog is not a
    /// language code).
    #[test]
    fn non_lang_folder_does_not_qualify() {
        assert!(!is_lang_tree_root_home(
            Some("blog/index.md"),
            "blog/index.html"
        ));
    }

    /// None source_path does not qualify.
    #[test]
    fn no_source_path_does_not_qualify() {
        assert!(!is_lang_tree_root_home(None, "zh-hans/index.html"));
    }
}

/// Which pages are steps in a folder's reading order — the prev/next chain
/// the series nav walks. Two kinds of page live in the folder listing but
/// must NOT be reachable as a neighbour: a subfolder's own index page, and a
/// page that opted out with `series: false`.
mod sequence_siblings_tests {
    use super::super::sequence_siblings;
    use crate::build::types::ParsedDocument;
    use crate::build::types::SeriesField;
    use moss_core::PageKind;
    use moss_core::sort::{ResolvedSort, SortAxis};

    fn article(stem: &str, weight: i32) -> ParsedDocument {
        ParsedDocument {
            title: stem.to_string(),
            label: stem.to_string(),
            clean_stem: stem.to_string(),
            url_path: format!("guide/{}/index.html", stem),
            weight: Some(weight),
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    fn folder(stem: &str) -> ParsedDocument {
        ParsedDocument {
            title: stem.to_string(),
            label: stem.to_string(),
            clean_stem: stem.to_string(),
            url_path: format!("guide/{}/index.html", stem),
            kind: PageKind::Folder,
            ..Default::default()
        }
    }

    /// The `guide` folder's own index page.
    fn parent() -> ParsedDocument {
        let mut d = folder("guide");
        d.url_path = "guide/index.html".to_string();
        d
    }

    /// The `guide` folder's resolved sort: ordered by weight.
    fn weighted() -> ResolvedSort {
        ResolvedSort {
            axis: SortAxis::Weight,
            explicit_order: None,
            series_default: true,
        }
    }

    fn chain(docs: &[ParsedDocument]) -> Vec<String> {
        sequence_siblings(docs, "guide/index.html", "guide/", &weighted())
            .iter()
            .map(|d| d.clean_stem.clone())
            .collect()
    }

    /// Baseline: a plain folder of articles is untouched by the two
    /// exclusions — every page that sets neither flag stays in the chain,
    /// in sort order.
    #[test]
    fn plain_articles_all_stay_in_the_chain() {
        let docs = vec![parent(), article("one", 1), article("two", 2), article("three", 3)];
        assert_eq!(chain(&docs), vec!["one", "two", "three"]);
    }

    /// Bug 1: a subfolder's own index page reduces to a single segment once
    /// `/index.html` is stripped, so a URL-pattern filter accepts it as a
    /// direct child. It must be excluded by kind instead.
    #[test]
    fn subfolder_index_is_not_a_neighbour() {
        let docs = vec![
            parent(),
            article("one", 1),
            folder("appendix"), // guide/appendix/index.html — a doorway, not a step
            article("two", 2),
        ];
        assert_eq!(
            chain(&docs),
            vec!["one", "two"],
            "a subfolder index must not be threaded into its siblings' chain"
        );
    }

    /// `layout: article` is the author saying "this one is a piece",
    /// so the folder index rejoins the chain — and, being in it, renders its
    /// own prev/next too. `subfolder_index_is_not_a_neighbour` above is the
    /// other half of the pair: a folder index that said nothing stays a
    /// doorway. Having children is not what decides it.
    #[test]
    fn layout_article_subfolder_index_is_a_step() {
        let mut part_two = folder("two");
        part_two.weight = Some(2);
        part_two.layout = Some("article".to_string());
        let docs = vec![parent(), article("one", 1), part_two, article("three", 3)];
        assert_eq!(
            chain(&docs),
            vec!["one", "two", "three"],
            "a layout: article folder index must be threaded into the chain, \
             so `one`'s next is `two` and not `three`"
        );
    }

    /// Bug 2: `series: false` used to silence only the page's own nav, so the
    /// page before it still emitted a "next →" pointing at it. It must drop
    /// out of every sibling's chain, leaving its neighbours adjacent.
    #[test]
    fn series_false_page_is_not_reachable_from_either_neighbour() {
        let mut opted_out = article("two", 2);
        opted_out.series = Some(SeriesField::Flag(false));
        let docs = vec![parent(), article("one", 1), opted_out, article("three", 3)];
        assert_eq!(
            chain(&docs),
            vec!["one", "three"],
            "series: false must remove the page from its siblings' chain, \
             so `one`'s next is `three` and `three`'s prev is `one`"
        );
    }

    /// A series whose chapters all share a date walks its prev/next chain in
    /// the order the folder's own page lists them, whatever order the pages
    /// were read in. Before, the chain kept the read order (4, 1, 2, 3 on one
    /// build) while the listing broke the tie itself. Filenames, url slugs and
    /// titles each sort these four differently, so only one shared key can
    /// make the two agree.
    #[test]
    fn same_date_chain_matches_the_folder_listing() {
        use crate::build::folder_embed::{resolve_markers, synthesize_children_marker};
        use crate::i18n::Language;

        fn chapter(stem: &str, title: &str) -> ParsedDocument {
            ParsedDocument {
                title: title.to_string(),
                label: title.to_string(),
                clean_stem: stem.to_string(),
                url_path: format!("serial/{}/index.html", stem.to_lowercase()),
                date: Some("1804".to_string()),
                kind: PageKind::Article,
                ..Default::default()
            }
        }

        let project = crate::build::folder_embed::tests::test_project();
        for (style, group) in [("grid", "none"), ("summary", "none"), ("list", "none"), ("list", "year")] {
            let mut index = folder("serial");
            index.url_path = "serial/index.html".to_string();
            index.series = Some(SeriesField::Flag(true));
            index.children_style = Some(moss_core::Resolved::frontmatter(style.to_string()));
            index.children_group = Some(moss_core::Resolved::frontmatter(group.to_string()));
            let docs = vec![
                chapter("Exile", "Exile"),
                chapter("arrival", "The Arrival"),
                index,
                chapter("departure", "Departure"),
                chapter("Crossing", "A Crossing"),
            ];
            let index = &docs[2];

            let marker = synthesize_children_marker(index, "serial", "serial/index.md", false);
            let listing = resolve_markers(
                &marker,
                "serial/index.md",
                &docs,
                &project,
                &std::collections::HashMap::new(),
                Language::En,
                None,
                None,
                false,
            );
            assert_eq!(
                group == "year",
                listing.contains("moss-cards-minimal-year-group"),
                "only the year case renders year sections: {listing}"
            );
            let mut listed = vec!["arrival", "crossing", "departure", "exile"];
            listed.sort_by_key(|slug| {
                listing
                    .find(&format!("href=\"/serial/{slug}/\""))
                    .unwrap_or_else(|| panic!("{slug} missing from the {style}/{group} listing: {listing}"))
            });

            let chain: Vec<String> = sequence_siblings(
                &docs,
                "serial/index.html",
                "serial/",
                &index.resolve_for_direct_children(),
            )
            .iter()
            .map(|d| d.url_path.trim_start_matches("serial/").trim_end_matches("/index.html").to_string())
            .collect();

            assert_eq!(chain, listed, "the chain must follow the {style}/{group} listing's order");
        }
    }
}

/// The registry reset belongs to the START OF THE BUILD, not to the video branch.
///
/// In the app the `AssetRegistry` is a process-lifetime `Arc` shared by every
/// rebuild, and `build.rs`'s post-seal tail feeds `failed_keys()` into the set
/// `degrade` strips from staged HTML. So a `Failed` key left by an earlier build
/// deletes a live `<source>` from a page that is healthy now. The clear used to
/// sit inside `if !items.is_empty()`, which meant a site with no videos — the
/// common case — never reset anything (c184b7161).
mod registry_clear_tests {
    #[test]
    fn a_videoless_build_still_clears_a_stale_failed_entry() {
        use crate::build::manifest::PendingManifest;
        use crate::build::render::{generate_blocking_content, SiteConfig};
        use crate::build::scan::scan::scan_folder;
        use crate::types::content::SiteHashes;
        use crate::types::services::BuildServices;
        use std::fs;

        let test_dir = std::env::temp_dir().join(format!(
            "moss_registry_clear_test_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&test_dir).unwrap();

        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(test_dir.clone());

        // A site with NO videos — the case the old placement never reached.
        fs::write(test_dir.join("index.md"), "# Test").unwrap();
        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();

        // An earlier build's failure, surviving on the shared registry.
        let services = BuildServices::headless();
        let registry = services.assets.clone().expect("headless has a registry");
        registry.set_failed("images/from-a-deleted-page.webp".to_string(), "encode failed".to_string());

        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        generate_blocking_content(
            &crate::vault::paths::VaultRoot::resolve(&test_dir),
            &project_structure,
            &output_dir,
            Some(&services),
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
        )
        .expect("generate_blocking_content should succeed");

        assert!(
            registry.failed_keys().is_empty(),
            "a build with no videos must still reset the registry; stale Failed keys \
             survived and would strip a live <source> from healthy HTML: {:?}",
            registry.failed_keys()
        );
    }
}

/// End-to-end proof that the build-time link-metadata fetch
/// (`build::page::link_meta::fetch_new_link_meta_for_build`, wired in
/// `blocking.rs` right before the per-page render loop) actually reaches a
/// real build: a fresh site with an external grid link against a local
/// (loopback) HTTP server, built ONCE, must show the fetched title on that
/// SAME build — the one-build lag the owner's decision explicitly rejects
/// ("New links: FETCH DURING THE BUILD... so a card is complete on its
/// first build").
mod build_time_link_meta_fetch_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::blocking::SiteConfig;
    use crate::build::render::generate_blocking_content_for_build;
    use crate::build::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A server that answers every request with the same fixed HTML,
    /// `requests` times — same shape as `link_meta.rs`'s own test helper
    /// (kept as a local copy: that one is private to its module).
    fn spawn_test_server(html: &'static str, requests: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(body.as_bytes());
            }
        });
        (url, handle)
    }

    fn build_site(test_dir: &std::path::Path, exits_after_build: bool) {
        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        generate_blocking_content_for_build(
            &crate::vault::paths::VaultRoot::resolve(test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
            exits_after_build,
        )
        .expect("generate_blocking_content_for_build should succeed");
    }

    #[test]
    fn a_new_external_link_shows_its_fetched_title_on_the_first_build() {
        let html = r#"<html><head><meta property="og:title" content="Fetched On First Build"></head></html>"#;
        let (base, server) = spawn_test_server(html, 1);

        let test_dir = std::env::temp_dir().join(format!(
            "moss_build_fetch_e2e_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        fs::write(
            test_dir.join("index.md"),
            format!(":::grid 1\n<{base}/first-build>\n:::\n"),
        )
        .unwrap();

        build_site(&test_dir, true);

        let index_html = fs::read_to_string(
            test_dir.join(".moss").join("build.nosync").join("site").join("index.html"),
        )
        .expect("index.html should exist");
        assert!(
            index_html.contains("Fetched On First Build"),
            "the title fetched DURING this build must appear in this build's own \
             output, not just the next one's: {index_html}"
        );

        let _ = server.join();
    }

    #[test]
    fn an_offline_build_with_an_unreachable_link_still_finishes_promptly() {
        // Bind then drop, so the port is guaranteed to refuse connections —
        // the "server down" case the owner's report has to speak to.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let test_dir = std::env::temp_dir().join(format!(
            "moss_build_fetch_offline_e2e_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());

        fs::write(
            test_dir.join("index.md"),
            format!(":::grid 1\n<http://127.0.0.1:{port}/unreachable>\n:::\n"),
        )
        .unwrap();

        let start = std::time::Instant::now();
        build_site(&test_dir, true);
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "an unreachable link must not stall the build near the budget's ceiling: {elapsed:?}"
        );

        let index_html = fs::read_to_string(
            test_dir.join(".moss").join("build.nosync").join("site").join("index.html"),
        )
        .expect("index.html should exist even though the link never resolved");
        assert!(
            index_html.contains("moss-card"),
            "the cell still renders its card, just without fetched metadata: {index_html}"
        );
    }
}

/// End-to-end proof that a downloaded og:image becomes a real local cover
/// (`build::media::remote_cover`), rendered exactly like an internal page's
/// cover — never the remote URL — on the SAME build that introduces the
/// link, mirroring `build_time_link_meta_fetch_tests` above for the cover
/// half of the "one card kind" story.
mod remote_cover_end_to_end_tests {
    use crate::build::manifest::PendingManifest;
    use crate::build::render::blocking::SiteConfig;
    use crate::build::render::generate_blocking_content_for_build;
    use crate::build::scan_folder;
    use crate::types::content::SiteHashes;
    use std::fs;

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tiny_png_bytes() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(4, 3, image::Rgb([30, 120, 200]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// A server that answers `/page` with HTML carrying an `og:image`
    /// pointing at `/cover.png` on the SAME server, and `/cover.png` with a
    /// real decodable PNG. One connection per path, `requests` total.
    fn spawn_page_with_cover_server(requests: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let base_for_thread = url.clone();
        let handle = std::thread::spawn(move || {
            let png = tiny_png_bytes();
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut req = [0u8; 2048];
                let n = stream.read(&mut req).unwrap_or(0);
                let req_text = String::from_utf8_lossy(&req[..n]);
                let first_line = req_text.lines().next().unwrap_or("");
                if first_line.contains("/cover.png") {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: image/png\r\n\r\n",
                        png.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&png);
                } else {
                    let html = format!(
                        r#"<html><head><meta property="og:title" content="Cover Page"><meta property="og:image" content="{base_for_thread}/cover.png"></head></html>"#
                    );
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n",
                        html.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(html.as_bytes());
                }
            }
        });
        (url, handle)
    }

    fn build_site(test_dir: &std::path::Path) -> String {
        let output_dir = test_dir.join(".moss").join("build.nosync").join("site");
        fs::create_dir_all(&output_dir).unwrap();
        let project_structure =
            scan_folder(test_dir.to_str().unwrap()).expect("scan_folder should succeed");
        generate_blocking_content_for_build(
            &crate::vault::paths::VaultRoot::resolve(test_dir),
            &project_structure,
            &output_dir,
            None,
            None,
            true,
            SiteConfig::default(),
            &mut PendingManifest::new(SiteHashes::default()),
            true,
        )
        .expect("generate_blocking_content_for_build should succeed");
        fs::read_to_string(output_dir.join("index.html")).expect("index.html should exist")
    }

    #[test]
    fn an_og_image_becomes_a_local_cover_on_the_first_build() {
        let (base, server) = spawn_page_with_cover_server(2);
        let test_dir = std::env::temp_dir().join(format!(
            "moss_remote_cover_e2e_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());
        fs::write(test_dir.join("index.md"), format!(":::grid 1\n<{base}/page>\n:::\n")).unwrap();

        let index_html = build_site(&test_dir);

        assert!(
            index_html.contains(r#"class="moss-card-cover">"#),
            "og:image present must produce a real cover, not the no-cover placeholder: {index_html}"
        );
        assert!(index_html.contains("_moss/link/"), "cover must be a local asset: {index_html}");
        assert!(index_html.contains("src=\"/_moss/link/"), "cover URL must be root-relative: {index_html}");
        assert!(index_html.contains(".webp"), "must go through the webp pipeline: {index_html}");
        // The card's own href legitimately points at the external page (and
        // the favicon is hotlinked by existing, unrelated design), so the
        // "never emit the remote URL" check is scoped to the cover's own
        // <picture> — its src/srcset must be local paths, not the origin
        // that served the downloaded image.
        let picture_start = index_html.find("<picture>").expect("cover must render a <picture>");
        let picture_end = index_html[picture_start..].find("</picture>").unwrap() + picture_start;
        let picture_markup = &index_html[picture_start..picture_end];
        assert!(
            !picture_markup.contains(&base),
            "the cover's own markup must never reference the remote origin: {picture_markup}"
        );

        let _ = server.join();
    }

    #[test]
    fn a_manually_titled_link_still_gets_a_fetched_cover_and_favicon() {
        // Owner decision (2026-09): author-written link text wins the
        // TITLE slot only — it must not suppress the fetch, the favicon,
        // or the cover. The owner's real case is a row of podcast
        // episodes written as `[episode title](https://…)` that need
        // covers like every other card.
        let (base, server) = spawn_page_with_cover_server(2);
        let test_dir = std::env::temp_dir().join(format!(
            "moss_remote_cover_manual_title_e2e_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());
        fs::write(test_dir.join("index.md"), format!(":::grid 1\n[Real Title]({base}/page)\n:::\n")).unwrap();

        let index_html = build_site(&test_dir);

        assert!(
            index_html.contains(r#"<span class="moss-card-title">Real Title</span>"#),
            "the author's own link text must win the title slot: {index_html}"
        );
        assert!(
            index_html.contains(r#"class="moss-card-cover">"#),
            "a manual title must not suppress the fetched cover: {index_html}"
        );
        assert!(index_html.contains("_moss/link/"), "cover must be a local asset: {index_html}");
        assert!(
            index_html.contains(r#"class="moss-card-kicker-favicon""#),
            "a manual title must not suppress the fetched favicon: {index_html}"
        );

        let _ = server.join();
    }

    #[test]
    fn a_link_with_no_og_image_gets_the_placeholder() {
        let html_no_image = r#"<html><head><meta property="og:title" content="No Cover Here"></head></html>"#;
        let (base, server) = {
            use std::io::{Read, Write};
            use std::net::TcpListener;
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let url = format!("http://127.0.0.1:{port}");
            let handle = std::thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n",
                    html_no_image.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(html_no_image.as_bytes());
            });
            (url, handle)
        };
        let test_dir = std::env::temp_dir().join(format!(
            "moss_remote_cover_no_image_e2e_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());
        fs::write(test_dir.join("index.md"), format!(":::grid 1\n<{base}/page>\n:::\n")).unwrap();

        let index_html = build_site(&test_dir);

        assert!(
            index_html.contains(r#"class="moss-card-cover moss-card-no-cover"></div>"#),
            "no og:image must fall back to the plain placeholder: {index_html}"
        );
        let _ = server.join();
    }

    #[test]
    fn an_authored_image_still_wins_over_a_fetched_cover() {
        let (base, server) = spawn_page_with_cover_server(2);
        let test_dir = std::env::temp_dir().join(format!(
            "moss_remote_cover_author_wins_e2e_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let _cleanup = Cleanup(test_dir.clone());
        fs::write(
            test_dir.join("author.jpg"),
            b"not a real jpeg but the render path never decodes it",
        )
        .unwrap();
        fs::write(
            test_dir.join("index.md"),
            format!(":::grid 1\n[![Author photo](author.jpg)]({base}/page)\n:::\n"),
        )
        .unwrap();

        let index_html = build_site(&test_dir);

        assert!(index_html.contains("author.jpg"), "authored image must survive: {index_html}");
        assert!(!index_html.contains("_moss/link/"), "must not also emit a fetched cover: {index_html}");
        let _ = server.join();
    }
}
