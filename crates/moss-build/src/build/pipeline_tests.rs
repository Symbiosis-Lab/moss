use super::super::media::pipeline::{
    compute_expected_dirs, copy_deferred_assets, is_image_extension,
    remove_stale_dirs, remove_stale_files, stage_copy, stage_write,
};
use super::super::video::{compute_video_item_fingerprint, run_video_conversion};
use super::*;
use crate::types::services::BackgroundContext;
use crate::build::scan::scan::scan_folder;
use tempfile::TempDir;

/// Cache keys for a test build.
///
/// A fixed builder fingerprint and no plugins: tests assert on what a build
/// emits, and a real fingerprint (a stat of the test binary) would differ
/// between runs for reasons no assertion is about.
/// The served paths in a receipt list, for assertions that are about WHAT was
/// written rather than what it hashed to.
fn receipt_paths(receipts: &[(crate::build::served_path::ServedPath, String)]) -> Vec<String> {
    receipts.iter().map(|(sp, _)| sp.as_str().to_string()).collect()
}

fn test_cache_keys() -> crate::build::ports::CacheKeyInputs {
    crate::build::ports::CacheKeyInputs {
        builder: "test-builder-fingerprint".to_string(),
    }
}





#[test]
fn test_load_previous_hashes_nonexistent() {
    let temp = TempDir::new().unwrap();
    let hashes = load_previous_hashes(temp.path().to_str().unwrap());
    assert!(hashes.files.is_empty());
}

#[test]
fn test_load_previous_hashes_valid_json() {
    let temp = TempDir::new().unwrap();
    let moss_dir = temp.path().join(".moss");
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    let hashes_json = r#"{"files":{"index.html":"abc123","about.html":"def456"}}"#;
    fs::write(moss_dir.join("build").join("hashes.json"), hashes_json).unwrap();

    let hashes = load_previous_hashes(temp.path().to_str().unwrap());
    assert_eq!(hashes.files.len(), 2);
    assert_eq!(hashes.files.get("index.html"), Some(&"abc123".to_string()));
}

#[test]
fn test_load_previous_hashes_invalid_json() {
    let temp = TempDir::new().unwrap();
    let moss_dir = temp.path().join(".moss");
    fs::create_dir_all(moss_dir.join("build")).unwrap();

    fs::write(moss_dir.join("build").join("hashes.json"), "not valid json").unwrap();

    // Should return default (empty) on parse error
    let hashes = load_previous_hashes(temp.path().to_str().unwrap());
    assert!(hashes.files.is_empty());
}

fn create_test_dir() -> (std::path::PathBuf, impl Drop) {
    let system_temp = std::env::temp_dir();
    let test_dir = system_temp.join(format!("moss_build_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_dir).unwrap();

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    (test_dir.clone(), Cleanup(test_dir))
}

/// Test helper: scans folder and calls build() in one step.
/// Avoids repeating scan_folder() + build() in every test.
///
/// Returns only `is_empty` (drops the `BackgroundHandle`) — tests that need
/// to exercise the background phase should call `run()` directly and await the handle.
/// Test wrapper around `run`. Constructs a single-thread tokio runtime
/// per call, runs the build, awaits the `BackgroundHandle` (so the
/// coordinator drains and seals), and persists the resulting
/// `SealedManifest` to `.moss/build/hashes.json` so the next build's
/// `load_previous_hashes` sees a complete manifest.
///
/// Pre-#620 Item 2 the synchronous path was supported by:
///   1. a no-tokio-runtime fallback in `build_inner` (gone — sees
///      `debug_assert!` on tokio::runtime::Handle);
///   2. a legacy on-disk `hashes.json` write inside the runners (gone —
///      the coordinator owns the manifest).
///
/// The replacement is this wrapper: drive the same await+persist flow
/// production uses (`build.rs:964-986`) but inline for tests so the
/// existing `#[test]` callers don't all need to migrate to
/// `#[tokio::test]`.
fn build_test(
    folder_path: &str,
    site_dir_state: Option<&SiteDirectoryState>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    server_port: Option<u16>,
    services: Option<&BuildServices>,
    resolved_slots: &ResolvedSlots,
) -> Result<bool, String> {
    build_test_full(
        folder_path,
        site_dir_state,
        progress_sender,
        server_port,
        services,
        resolved_slots,
    )
    .map(|(is_empty, _missing)| is_empty)
}

/// `build_test`, but also handing back what the build could not find — the
/// evidence `deploy::refuse_publish` decides on. Only the publish-gate
/// tests need it; everything else wants the `is_empty` boolean.
fn build_test_full(
    folder_path: &str,
    site_dir_state: Option<&SiteDirectoryState>,
    progress_sender: Option<&dyn crate::build::ports::reporter::BuildReporter>,
    server_port: Option<u16>,
    services: Option<&BuildServices>,
    resolved_slots: &ResolvedSlots,
) -> Result<(bool, Vec<crate::build::types::MissingMedia>), String> {
    let ps = scan_folder(folder_path)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("test runtime build failed: {}", e))?;
    // Resolve once here so the 40-odd test callers keep their `&str` argument.
    let root = crate::vault::paths::VaultRoot::resolve(folder_path);
    // A fixed answer: no `ServerState` exists in a test, so the build's own
    // port — usually none — is the whole truth. The one test where the answer
    // CHANGES mid-build drives `run_pipeline` instead (`build_tests.rs`,
    // `every_completion_reports_the_port_that_came_up_during_the_build`).
    let preview_port = crate::build::ports::port_of_this_build(server_port, None);
    rt.block_on(async move {
        // PR7b (moss#599): `PipelineRunOutput::build_documents` carries the parsed
        // page slice. This test path doesn't consume it (snapshot tests inspect
        // the on-disk output), so we discard it here.
        let slots = resolved_slots.clone();
        let PipelineRunOutput {
            is_empty,
            bg_handle,
            build_documents: _documents,
            content_hashes: _content_hashes,
            missing_media,
            cancelled: _cancelled,
            home_ready: _home_ready,
            publishable: _publishable,
        } = run(
            &root,
            site_dir_state,
            progress_sender,
            &preview_port,
            services,
            Some(Box::new(move |_, _, _| Ok(slots))),
            &ps,
            None,
            crate::build::render::IncrementalGates::default(),
            crate::build::feeds::search_lane::Freshness::Now,
            &test_cache_keys(),
        )
        .map_err(crate::build::outcome::BuildStopped::into_message)?;
        if let Some(handle) = bg_handle {
            match handle.await_completion().await {
                Ok(sealed) => {
                    let hashes_path = crate::moss_paths::MossPaths::new(root.path()).hashes();
                    if let Err(e) = sealed.write_to_disk(&hashes_path) {
                        log::warn!("test seal+persist: failed to write hashes.json: {}", e);
                    }
                }
                Err(e) => {
                    return Err(format!("test seal+persist failed: {}", e));
                }
            }
        }
        Ok((is_empty, missing_media))
    })
}

#[test]
fn test_build_creates_output_directory() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create a simple markdown file
    fs::write(test_dir.join("index.md"), "# Hello\nTest content").unwrap();

    // Build without progress channel or state
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());

    assert!(result.is_ok(), "Build should succeed: {:?}", result);
    assert!(!result.unwrap(), "Site should not be empty");

    // Verify output was created (staging/ is sole output post-T2)
    assert!(test_dir.join(".moss/build/staging/index.html").exists());
}

#[test]
fn test_build_empty_folder_returns_true() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Build with no content files
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());

    assert!(result.is_ok());
    assert!(result.unwrap(), "Empty folder should return is_empty=true");
}

#[test]
fn test_build_with_site_dir_state() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create content
    fs::write(test_dir.join("index.md"), "# Test").unwrap();

    // Create SiteDirectoryState seeded to staging (post-generations: site/ is empty)
    let staging_path = test_dir.join(".moss/build/staging");
    fs::create_dir_all(&staging_path).unwrap();
    let site_dir_state = SiteDirectoryState::new(staging_path.clone());

    // Build with state
    let result = build_test(
        folder_path,
        Some(&site_dir_state),
        None,
        None,
        None,
        &ResolvedSlots::empty(),
    );

    assert!(result.is_ok());
    // Post-T2: staging/ is sole build output
    assert!(test_dir.join(".moss/build/staging/index.html").exists());
}

#[test]
fn test_send_progress_with_none_channel() {
    // Should not panic when channel is None
    send_progress(None, "test", "message", 50, false, None, None);
}

// =========================================================================
// Site-stage Cleanup Tests
// =========================================================================

#[test]
fn test_site_stage_persists_after_successful_build() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create a simple markdown file
    fs::write(test_dir.join("index.md"), "# Hello\nTest content").unwrap();

    // Build — always uses staging, even for first build
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    // Staging directory persists for mtime/size cache optimization
    let stage_dir = test_dir.join(".moss/build/staging");
    assert!(stage_dir.exists(), "site-stage should persist after build");

    // Post-T2: staging/ is sole output (ship_phase removed)
    assert!(test_dir.join(".moss/build/staging/index.html").exists());
}

#[test]
fn test_site_stage_persists_after_empty_build() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Build with no content (empty folder)
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Even empty builds use staging — site-stage/ is created
    assert!(
        test_dir.join(".moss/build/staging").exists(),
        "site-stage should exist even for empty builds"
    );
}

#[test]
fn test_site_stage_persists_after_rebuild() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# First").unwrap();
    let _ = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());

    // Second build (with different content to trigger changes)
    fs::write(test_dir.join("index.md"), "# Second").unwrap();
    // Clear caches to avoid mtime-based false cache hits on CI
    let _ = fs::remove_dir_all(test_dir.join(".moss/build/cache"));
    let _ = fs::remove_file(test_dir.join(".moss/build/hashes.json"));
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Verify site-stage persists for incremental rebuilds
    let stage_dir = test_dir.join(".moss/build/staging");
    assert!(
        stage_dir.exists(),
        "site-stage directory should persist for incremental rebuilds"
    );
}

#[test]
fn test_rebuild_site_populated_after_build() {
    // Post-T2: staging/ is the sole build output. The test verifies that
    // staging/ is fully populated when build() returns.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build (via staging)
    fs::write(test_dir.join("index.md"), "# First").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // Modify content to trigger staging path on second build
    fs::write(test_dir.join("index.md"), "# Second").unwrap();
    let _ = fs::remove_dir_all(test_dir.join(".moss/build/cache"));
    let _ = fs::remove_file(test_dir.join(".moss/build/hashes.json"));

    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Rebuild should succeed: {:?}", result);

    // Immediately after build() returns, staging/ must contain the latest content.
    let staging_content =
        fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        staging_content.contains("Second"),
        "staging/ must contain the latest content after rebuild"
    );
    assert!(
        !staging_content.is_empty(),
        "staging/ index.html must not be empty after rebuild"
    );
}

#[test]
fn test_site_stage_persists_when_no_changes() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# Same content").unwrap();
    let _ = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());

    // Second build with same content (should detect no changes)
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Verify site-stage persists
    let stage_dir = test_dir.join(".moss/build/staging");
    assert!(
        stage_dir.exists(),
        "site-stage directory should persist for incremental rebuilds"
    );
}

// =========================================================================
// Enhance-before-copy Tests
// =========================================================================

#[test]
fn test_slots_injected_before_copy_to_site() {
    // Post-T2: staging/ is sole output. Slot injection happens on staging/
    // and the result is verified there. ship_phase to site/ removed.
    use crate::build::enhance::{EnhanceContent, EnhanceResult};

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello\nTest content").unwrap();

    // Create ResolvedSlots with a test slot
    let mut slots = ResolvedSlots::empty();
    let mut result_slots = std::collections::HashMap::new();
    result_slots.insert(
        "footer-left".to_string(),
        EnhanceContent::Static {
            html: "<form class=\"test-subscribe\">Subscribe</form>".to_string(),
        },
    );
    let enhance_result = EnhanceResult {
        success: true,
        slots: result_slots,
    };
    slots.merge(&enhance_result, 0, "test-plugin");

    let result = build_test(folder_path, None, None, None, None, &slots);
    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    let stage_html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();

    assert!(
        stage_html.contains("test-subscribe"),
        "site-stage/ should have injected slot content"
    );

    // No raw slot markers should remain
    assert!(
        !stage_html.contains("<!-- slot:footer-left"),
        "site-stage/ should not have raw slot markers"
    );

    // staging/ is the annotated PREVIEW copy with data-source-* annotations
    assert!(
        stage_html.contains("data-moss-preview") && stage_html.contains(r#"data-source-line=""#),
        "site-stage/ (preview) must keep data-moss-preview + data-source-* annotations"
    );
}

#[test]
fn test_empty_slots_leave_no_markers() {
    // Even with ResolvedSlots::empty(), raw slot markers should be replaced
    // (with empty string, effectively removing them).
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();

    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Post-T2: check staging/ (sole output)
    let stage_html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        !stage_html.contains("<!-- slot:"),
        "No raw slot markers should remain in staging/ even with empty slots"
    );
}

#[test]
fn test_slots_injected_on_rebuild() {
    // Slot injection must work on both first build and rebuild
    use crate::build::enhance::{EnhanceContent, EnhanceResult};

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build without slots
    fs::write(test_dir.join("index.md"), "# First").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // Rebuild with slots
    fs::write(test_dir.join("index.md"), "# Second").unwrap();
    let _ = fs::remove_file(test_dir.join(".moss/build/hashes.json"));

    let mut slots = ResolvedSlots::empty();
    let mut result_slots = std::collections::HashMap::new();
    result_slots.insert(
        "head-end".to_string(),
        EnhanceContent::Static {
            html: "<style class=\"test-style\">.test{}</style>".to_string(),
        },
    );
    let enhance_result = EnhanceResult {
        success: true,
        slots: result_slots,
    };
    slots.merge(&enhance_result, 0, "test-plugin");

    let result = build_test(folder_path, None, None, None, None, &slots);
    assert!(result.is_ok(), "Rebuild should succeed: {:?}", result);

    // Post-T2: check staging/ (sole output)
    let stage_html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        stage_html.contains("test-style"),
        "Rebuild should have injected head-end slot content"
    );
}

#[test]
fn test_slot_injection_manifest_hash_matches_site_file() {
    // Slot injection is part of the build artifact boundary: marked HTML is
    // rendered into staging, slots are injected, then the manifest records
    // the production `site/` derivative that deploy uploads.
    use crate::build::assets::paths::compute_binary_hash;
    use crate::build::enhance::{EnhanceContent, EnhanceResult};

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello\nSome content.").unwrap();

    let mut slots = ResolvedSlots::empty();
    let mut result_slots = std::collections::HashMap::new();
    result_slots.insert(
        "body-end".to_string(),
        EnhanceContent::Static {
            html: "<div id=\"slot-injection-marker\">injected</div>".to_string(),
        },
    );
    let enhance_result = EnhanceResult {
        success: true,
        slots: result_slots,
    };
    slots.merge(&enhance_result, 0, "test-plugin");

    build_test(folder_path, None, None, None, None, &slots).expect("build with slots must succeed");

    // Read the actual staging file bytes after slot injection.
    let staging_html = fs::read(test_dir.join(".moss/build/staging/index.html"))
        .expect("staging/index.html must exist after build");

    // The injected content must be present in staging.
    let staging_str = std::str::from_utf8(&staging_html).unwrap();
    assert!(
        staging_str.contains("slot-injection-marker"),
        "staging/index.html must contain the injected slot content"
    );

    // Post-T2: manifest hashes the deploy-ready bytes (staging/ bytes with
    // data-source-line annotations stripped via apply_transform). staging/ holds
    // annotated HTML; the manifest records the stripped derivative that deploy uploads.
    // site/ is not updated until T4 (seal+persist materialization).
    let staging_html_for_hash = fs::read(test_dir.join(".moss/build/staging/index.html"))
        .expect("staging/index.html must exist after build");
    let manifest_bytes = crate::build::ship::apply_transform(
        crate::build::ship::transform_for("index.html"),
        &staging_html_for_hash,
    );
    let expected_hash = format!("100644:{}", compute_binary_hash(&manifest_bytes));

    // Read hashes.json and check the index.html entry.
    let hashes_path = test_dir.join(".moss/build/hashes.json");
    let hashes_json =
        fs::read_to_string(&hashes_path).expect("hashes.json must be written by build_test");
    let hashes: crate::types::content::SiteHashes =
        serde_json::from_str(&hashes_json).expect("hashes.json must be valid JSON");

    let manifest_hash = hashes
        .files
        .get("index.html")
        .expect("hashes.json must contain an entry for index.html");

    assert_eq!(
        manifest_hash, &expected_hash,
        "manifest hash for index.html must match the post-slot-injection staging/ bytes"
    );
}

// =========================================================================
// Unified Staging Tests (first build, empty site, missing site)
// =========================================================================

#[test]
fn test_first_build_uses_staging() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create a simple markdown file
    fs::write(test_dir.join("index.md"), "# Hello").unwrap();

    // First build - uses staging just like rebuilds
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    // Post-T2: staging/ is sole output
    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "index.html should be generated in staging/"
    );

    // Staging directory persists for mtime/size cache optimization
    assert!(
        test_dir.join(".moss/build/staging").exists(),
        "site-stage should persist after build"
    );
}

#[test]
fn test_rebuild_when_site_emptied() {
    // Post-T2: site/ is not updated by the build (ship_phase removed).
    // staging/ is the sole output; emptying staging/ between builds
    // verifies the pipeline recovers correctly.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create content
    fs::write(test_dir.join("index.md"), "# Hello").unwrap();

    // First build
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());
    // staging/ has the output
    assert!(test_dir.join(".moss/build/staging/index.html").exists());

    // Empty staging/ to force the pipeline to recreate it.
    // (site/ is no longer written by the pipeline; clearing staging/ is the
    // equivalent setup for verifying the "recover from empty output" path.)
    let staging = test_dir.join(".moss/build/staging");
    if staging.exists() {
        fs::remove_dir_all(&staging).unwrap();
    }
    fs::create_dir_all(&staging).unwrap();

    // Verify staging is now empty (input condition)
    assert!(
        staging.read_dir().unwrap().next().is_none(),
        "Staging should be empty before second build"
    );

    // Second build - should populate staging/ again
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Rebuild should succeed: {:?}", result);

    // staging/ should have content (sole output post-T2)
    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "index.html should be in staging/ after build"
    );
}

#[test]
fn test_rebuild_when_staging_deleted() {
    // Post-generations: pipeline always writes to staging/, materializes to generations/.
    // Verify rebuild succeeds even when staging/ is deleted between builds.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create content
    fs::write(test_dir.join("index.md"), "# Hello").unwrap();

    // First build
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());
    assert!(test_dir.join(".moss/build/staging/index.html").exists());

    // Delete staging directory completely (simulates a partial cleanup)
    if test_dir.join(".moss/build/staging").exists() {
        fs::remove_dir_all(test_dir.join(".moss/build/staging")).unwrap();
    }

    // Verify staging doesn't exist
    assert!(
        !test_dir.join(".moss/build/staging").exists(),
        "Staging should not exist before second build"
    );

    // Second build - should recreate staging and build
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Rebuild should succeed: {:?}", result);

    // staging/ has the output
    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "index.html should be in staging/ after build"
    );
}

/// C1 (stage/site repair): a build with `server_port=None` — the condition of
/// EVERY watch rebuild, publish, and CLI build — must STILL annotate `/stage`
/// with `data-source-*` (the preview rests on `/stage`, so click-to-source needs
/// annotations on every build).
///
/// Post-T2: ship_phase to site/ removed. Only staging/ is checked.
#[test]
fn build_annotates_stage_and_strips_site_without_server_port() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();
    // Neutral prose — must NOT contain the literal "data-source-line" (we assert
    // on the ATTRIBUTE form `data-source-line="` so body text can't false-match).
    fs::write(
        test_dir.join("index.md"),
        "# Title\n\nAn ordinary body paragraph of prose.\n",
    )
    .unwrap();

    // server_port = None — the watch-rebuild / publish / CLI condition.
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "build should succeed: {:?}", result);

    let stage = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();

    assert!(
            stage.contains(r#"data-source-line=""#),
            "/stage must be annotated even when server_port is None (preview rests on /stage); got:\n{}",
            stage
        );
    assert!(
        stage.contains("data-moss-preview"),
        "/stage <body> must carry the data-moss-preview marker"
    );
}

#[test]
fn test_staging_used_for_rebuild_with_existing_content() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# First").unwrap();
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Verify first content (in staging/ post-T2)
    let content = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        content.contains("First"),
        "First build should contain 'First'"
    );

    // Modify content
    fs::write(test_dir.join("index.md"), "# Second").unwrap();

    // Clear caches to ensure rebuild detects content change (avoids
    // flaky failures when both builds run within the same second on CI,
    // causing mtime-based cache lookups to return stale hashes)
    let _ = fs::remove_dir_all(test_dir.join(".moss/build/cache"));
    let _ = fs::remove_file(test_dir.join(".moss/build/hashes.json"));

    // Second build - should use staging pattern (site has content)
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Verify new content from site-stage (the staging directory).
    // Note: site/ is updated by ship_phase which runs synchronously before
    // build() returns, but we check site-stage/ here for the annotated HTML.
    let stage_content =
        fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        stage_content.contains("Second"),
        "Staging build should contain 'Second'"
    );

    // Staging persists for incremental rebuilds
    assert!(
        test_dir.join(".moss/build/staging").exists(),
        "site-stage should persist for incremental rebuilds"
    );
}

// =========================================================================
// Zero-Flicker Staging Pattern Tests
// =========================================================================

#[test]
fn test_pointer_switches_during_rebuild() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# First").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // Set up state for second build — seed to staging (post-generations: site/ is empty)
    let staging_seed = test_dir.join(".moss/build/staging");
    fs::create_dir_all(&staging_seed).unwrap();
    let state = SiteDirectoryState::new(staging_seed.clone());

    // Modify content to trigger changes
    fs::write(test_dir.join("index.md"), "# Second").unwrap();

    // Before rebuild, pointer should be at staging/
    assert_eq!(state.current_dir.read().unwrap().as_path(), staging_seed);

    // Rebuild with state
    build_test(
        folder_path,
        Some(&state),
        None,
        None,
        None,
        &ResolvedSlots::empty(),
    )
    .unwrap();

    // After build returns, pointer rests on site-stage/ (preview has annotations)
    let stage_dir = test_dir.join(".moss/build/staging");
    assert_eq!(state.current_dir.read().unwrap().as_path(), stage_dir);

    // Post-T2: staging/ is the sole output. site/ is NOT updated.
    // Check that staging/ has the new content.
    let content = fs::read_to_string(stage_dir.join("index.html")).unwrap();
    assert!(
        content.contains("Second"),
        "staging/ should contain the new content"
    );
}

#[test]
fn test_content_unchanged_when_hashes_match() {
    // Post-T2: staging/ is the sole build output. When hashes match (no-changes
    // rebuild), staging/ must still have identical content.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# Same content").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    let staging_file = test_dir.join(".moss/build/staging/index.html");
    let content_before = fs::read_to_string(&staging_file).unwrap();

    // Small delay so any new write would be detectable.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Second build with same content.
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // The staging file's CONTENT must be byte-identical.
    let content_after = fs::read_to_string(&staging_file).unwrap();
    assert_eq!(
        content_before, content_after,
        "Staging content must be unchanged when hashes match"
    );
}

#[test]
fn test_multiple_sequential_rebuilds() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# Version 1").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // Create state seeded to staging (post-generations: site/ is empty)
    let staging_seed = test_dir.join(".moss/build/staging");
    fs::create_dir_all(&staging_seed).unwrap();
    let state = SiteDirectoryState::new(staging_seed);

    // Multiple rebuilds with changes
    let stage_dir = test_dir.join(".moss/build/staging");
    for i in 2..=5 {
        fs::write(test_dir.join("index.md"), format!("# Version {}", i)).unwrap();
        build_test(
            folder_path,
            Some(&state),
            None,
            None,
            None,
            &ResolvedSlots::empty(),
        )
        .unwrap();

        // Pointer rests on site-stage/ (preview with annotations)
        assert_eq!(state.current_dir.read().unwrap().as_path(), stage_dir);

        // Post-T2: verify content in staging/ (sole output)
        let content = fs::read_to_string(stage_dir.join("index.html")).unwrap();
        assert!(
            content.contains(&format!("Version {}", i)),
            "Build {} should contain 'Version {}'",
            i,
            i
        );

        // Verify staging persists for incremental rebuilds
        assert!(
            test_dir.join(".moss/build/staging").exists(),
            "site-stage should persist after build {}",
            i
        );
    }
}

#[test]
fn test_pointer_sequence_during_rebuild() {
    // Post-generations: documents expected state transitions:
    // 1. Before rebuild: pointer at staging/ (seed for zero-flicker)
    // 2. After rebuild: pointer at staging/ (preview with annotations)
    // 3. staging/ has the new content; generations/<id>/ is the frozen output

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // First build
    fs::write(test_dir.join("index.md"), "# Old").unwrap();
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // Seed state to staging (post-generations: site/ is empty)
    let staging_seed = test_dir.join(".moss/build/staging");
    fs::create_dir_all(&staging_seed).unwrap();
    let state = SiteDirectoryState::new(staging_seed.clone());

    // Verify initial state
    let initial_path = state.current_dir.read().unwrap().clone();
    assert_eq!(initial_path, staging_seed);

    // Trigger rebuild with changes
    fs::write(test_dir.join("index.md"), "# New").unwrap();
    build_test(
        folder_path,
        Some(&state),
        None,
        None,
        None,
        &ResolvedSlots::empty(),
    )
    .unwrap();

    // Pointer rests on site-stage/ (preview with annotations)
    let stage_dir = test_dir.join(".moss/build/staging");
    assert_eq!(state.current_dir.read().unwrap().as_path(), stage_dir);

    // staging/ has the new content (sole output post-T2)
    let content = fs::read_to_string(stage_dir.join("index.html")).unwrap();
    assert!(
        content.contains("New"),
        "Content should be updated at staging/ (the sole output)"
    );
}

// =========================================================================
// Two-Phase Build Tests (ADR-001: Staged Build)
// =========================================================================
// These tests verify the two-phase build architecture:
// - Blocking phase (~1s): scan, markdown->HTML, document_setup, server start
// - Background phase (async): RSS, assets copying, video conversion
//
// Key invariants:
// 1. build() returns BEFORE background tasks complete
// 2. HTML files are available immediately after build() returns
// 3. Background tasks emit progress events
// 4. Asset ready events are emitted as each asset is copied

#[test]
fn test_build_returns_before_assets_copied() {
    // ADR-001: build() should return quickly after blocking phase
    // Assets copying happens in background
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create markdown and an assets directory with files
    fs::write(test_dir.join("index.md"), "# Hello\nTest content").unwrap();
    fs::create_dir_all(test_dir.join("assets")).unwrap();
    fs::write(test_dir.join("assets/test.txt"), "asset content").unwrap();

    // Measure time for build() to return
    let start = std::time::Instant::now();
    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    // Blocking phase should complete quickly for simple content.
    // Use a generous 10s timeout to avoid flaky failures on loaded CI runners
    // while still catching genuine regressions (e.g., accidental sync video conversion).
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "Build should return quickly (blocking phase): {:?}",
        elapsed
    );

    // HTML should be immediately available after build() returns (in staging/ post-T2)
    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "HTML should be generated in blocking phase"
    );

    // Note: In the current implementation, assets are copied synchronously.
    // After implementing ADR-001, assets may still be copying in background.
    // The test verifies that build() returns before assets are fully copied.
}

#[test]
fn test_html_available_immediately_after_build() {
    // ADR-001: Core HTML content must be available immediately
    // This is the key user-facing requirement - browser can open right away
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create multiple markdown files
    fs::write(test_dir.join("index.md"), "# Home\nWelcome").unwrap();
    fs::create_dir_all(test_dir.join("about")).unwrap();
    fs::write(test_dir.join("about/index.md"), "# About\nAbout page").unwrap();

    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // All HTML files must be available immediately (in staging/ post-T2)
    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "index.html should be ready immediately"
    );
    assert!(
        test_dir
            .join(".moss/build/staging/about/index.html")
            .exists(),
        "about/index.html should be ready immediately"
    );

    // CSS and JS must also be available (needed for page rendering).
    // Files are now emitted with content-hash names (e.g. _moss/style.<hash>.css).
    let moss_dir = test_dir.join(".moss/build/staging/_moss");
    let has_hashed_css = fs::read_dir(&moss_dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("style.") && s.ends_with(".css")
            })
        })
        .unwrap_or(false);
    assert!(
        has_hashed_css,
        "_moss/style.<hash>.css should be ready immediately"
    );

    let js_dir = test_dir.join(".moss/build/staging/_moss/js");
    let has_hashed_theme_js = fs::read_dir(&js_dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("theme.") && s.ends_with(".js")
            })
        })
        .unwrap_or(false);
    assert!(
        has_hashed_theme_js,
        "_moss/js/theme.<hash>.js should be ready immediately"
    );
}

#[test]
fn test_blocking_phase_includes_essential_files() {
    // ADR-001: Blocking phase must include all files needed for initial page load
    // This means: HTML, CSS, JS, favicon (if exists)
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Create content with favicon
    fs::write(test_dir.join("index.md"), "# Test").unwrap();
    fs::create_dir_all(test_dir.join("assets")).unwrap();
    fs::write(test_dir.join("assets/favicon.svg"), "<svg></svg>").unwrap();

    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok());

    // Essential files for page rendering (in staging/ post-T2)
    assert!(test_dir.join(".moss/build/staging/index.html").exists());
    // CSS and JS are now emitted with content-hash names.
    let moss_dir = test_dir.join(".moss/build/staging/_moss");
    let has_hashed_css = fs::read_dir(&moss_dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("style.") && s.ends_with(".css")
            })
        })
        .unwrap_or(false);
    assert!(
        has_hashed_css,
        "_moss/style.<hash>.css must exist in blocking output"
    );
    let js_dir = test_dir.join(".moss/build/staging/_moss/js");
    let has_hashed_theme_js = fs::read_dir(&js_dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("theme.") && s.ends_with(".js")
            })
        })
        .unwrap_or(false);
    assert!(
        has_hashed_theme_js,
        "_moss/js/theme.<hash>.js must exist in blocking output"
    );
    assert!(
        test_dir
            .join(".moss/build/staging/assets/favicon.svg")
            .exists(),
        "Favicon should be copied in blocking phase (above the fold)"
    );
}

// =========================================================================
// Background Video Conversion Tests (ADR-001: Phase 2)
// =========================================================================

#[test]
fn test_background_ctx_is_not_ignored() {
    // Smoke test: a markdown file referencing a `.mov` video file builds
    // successfully and produces an output HTML.
    //
    // Phase 2E v5 PR5 (2026-05-26) retired the Stage 3 regex post-pass
    // that used to rewrite `.mov → .mp4` on raw `<video>` HTML embedded
    // in markdown source. Raw HTML is opaque pulldown-cmark pass-through
    // and no longer reaches a moss-controlled emitter. The `.mov → .mp4`
    // rewrite now lives exclusively at
    // `moss_core::render::video::synthesize_video_html` (via
    // `asset_paths::to_mp4`) and only fires for wikilink-syntax video
    // embeds (`![[clip.mov]]`) — which are covered by the unit tests at
    // `crates/moss-core/src/render/video.rs` (search for
    // `video_mov_extension_swaps_to_mp4`). This test no longer asserts
    // the .mp4 substring (the prior assertion only worked because the
    // regex post-pass ran over raw HTML); the named invariant
    // "background context is not ignored" is just that the build
    // succeeds end-to-end with a video file in the project.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(
        test_dir.join("index.md"),
        r#"# Test
<video src="./videos/clip.mov" controls></video>
"#,
    )
    .unwrap();

    fs::create_dir_all(test_dir.join("videos")).unwrap();
    fs::write(test_dir.join("videos/clip.mov"), "fake video data").unwrap();

    let result = build_test(folder_path, None, None, None, None, &ResolvedSlots::empty());
    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    assert!(
        test_dir.join(".moss/build/staging/index.html").exists(),
        "build should produce an output index.html"
    );
}

/// A ladder left in staging by a previous build is emitted by THIS build,
/// from a registry that starts empty — the `moss build` case.
///
/// The ordering inside `generate_blocking_content` is the whole invariant:
/// `hls::register_existing_ladders` reads the ladder off disk into the
/// registry, and `MediaDimensionLookup::new` copies the registry's variants
/// into a snapshot once, by value. Register after that line and the snapshot
/// never learns the ladder exists, so `has_hls_for_source` answers false and
/// every site silently emits a progressive `<video src>` instead — seventeen
/// encoded files referenced by nothing. Nothing else fails: the ladder is
/// still on disk, still linked into staging, still uploaded.
#[test]
fn a_ladder_already_in_staging_reaches_this_build_s_markup() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Test\n\n![[clip.mp4]]\n").unwrap();
    fs::write(test_dir.join("clip.mp4"), "fake video data").unwrap();

    // What the previous build's encoder left behind. `master.m3u8` is the
    // gate — a directory without one is a half-written ladder.
    let ladder = test_dir.join(".moss/build/staging/clip.hls");
    fs::create_dir_all(&ladder).unwrap();
    for member in moss_core::asset_paths::hls_members(&moss_core::asset_paths::VIDEO_LADDER) {
        fs::write(ladder.join(member), "fake ladder member").unwrap();
    }

    // Fresh registry, as `moss build` gets: one process, one registry, no
    // rebuild afterwards to pick the ladder up.
    let services = BuildServices::headless();
    let result = build_test(
        folder_path,
        None,
        None,
        None,
        Some(&services),
        &ResolvedSlots::empty(),
    );
    assert!(result.is_ok(), "Build should succeed: {:?}", result);

    let html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        html.contains("clip.hls/master.m3u8"),
        "a staged ladder must reach the emitted markup; got:\n{}",
        html
    );
}

// =========================================================================
// BuildComplete
// =========================================================================
//
// Four tests lived here that named the right scenarios — "emitted when all
// tasks done", "immediate if no videos", "on ffmpeg missing", "on cancellation"
// — and asserted nothing about any of them: each built a `BuildComplete` struct
// by hand and checked its own fields back. They were green for the entire time
// the event was never emitted at all on an image-only site
// (LOG-C2CF-T0935-08-14). Emission is now owned by `BuildTerminalBarrier` and
// tested where it is emitted, in `build/background.rs`:
// `a_build_with_no_workers_at_all_still_reaches_its_terminal_receipt`,
// `the_receipt_reports_the_counters_the_workers_published`, and
// `a_superseded_build_owes_no_completion_receipt`.

// =========================================================================
// Legacy Video Cache Cleanup Tests
// =========================================================================

#[test]
fn test_legacy_video_cache_cleanup() {
    let temp = TempDir::new().unwrap();
    let moss_dir = temp.path().join(".moss");
    let legacy = moss_dir.join("build").join("cache").join("videos");
    let cas = moss_dir.join("build").join("cache").join("objects");

    // Create both directories
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(&cas).unwrap();
    fs::write(legacy.join("test.mp4"), "old cached video").unwrap();

    // After cleanup runs, legacy should be removed
    cleanup_legacy_video_cache(&moss_dir);

    assert!(!legacy.exists(), "Legacy video cache should be removed");
    assert!(cas.exists(), "CAS objects dir should still exist");
}

#[test]
fn test_legacy_video_cache_not_removed_without_cas() {
    let temp = TempDir::new().unwrap();
    let moss_dir = temp.path().join(".moss");
    let legacy = moss_dir.join("build").join("cache").join("videos");

    // Create only legacy directory (no CAS yet)
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("test.mp4"), "old cached video").unwrap();

    // Cleanup should NOT remove legacy when CAS doesn't exist
    cleanup_legacy_video_cache(&moss_dir);

    assert!(
        legacy.exists(),
        "Legacy video cache should be preserved when CAS doesn't exist"
    );
}

#[test]
fn test_legacy_video_cache_noop_when_neither_exists() {
    let temp = TempDir::new().unwrap();
    let moss_dir = temp.path().join(".moss");

    // Neither directory exists -- should not panic
    cleanup_legacy_video_cache(&moss_dir);
}

// =========================================================================
// Unified run_video_conversion() Tests
// =========================================================================

#[test]
fn test_run_video_conversion_headless_empty_videos() {
    let services = BuildServices::headless();
    let ctx = BackgroundContext {
        video_items: vec![],
        source_path: "/nonexistent".to_string(),
        staging_dir: std::path::PathBuf::from("/nonexistent/.moss/build/staging"),
        moss_dir: std::path::PathBuf::from("/nonexistent/.moss"),
        start_time: std::time::Instant::now(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    run_video_conversion(&services, &ctx, 0, None);
}

// (Pre-Track A: test_run_video_conversion_cancel_flag_stays_false_headless
// verified `services.cancellation.cancelled_flag()` was false in headless
// mode. The flag is gone — cancellation now lives on FolderSession::cancel,
// and headless mode has `services.session = None`. The runner reads from
// a fresh local AtomicBool seeded `false`; trivially false in headless.)

// -------------------------------------------------------------------------
// One outcome per video: the census, the URL, and the UiBound budget
// -------------------------------------------------------------------------

/// A vault holding `names`, with a stand-in for ffmpeg built from `script` — so
/// the encoder's behaviour is the test's to choose, on a box where ffmpeg may
/// not be installed at all. `{temp}` in the script expands to the temp root.
fn vault_with_videos(names: &[&str], script: &str) -> (TempDir, BackgroundContext) {
    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("source");
    let staging_dir = temp.path().join("stage");
    let cache = temp.path().join(".moss/build/cache");
    std::fs::create_dir_all(&staging_dir).unwrap();
    for name in names {
        std::fs::create_dir_all(source_dir.join(name).parent().unwrap()).unwrap();
    }
    for sub in ["objects", "transforms", "tmp"] {
        std::fs::create_dir_all(cache.join(sub)).unwrap();
    }
    for name in names {
        std::fs::write(source_dir.join(name), b"original video bytes").unwrap();
    }

    let fake_ffmpeg = temp.path().join("not-ffmpeg");
    std::fs::write(
        &fake_ffmpeg,
        script.replace("{temp}", &temp.path().to_string_lossy()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_ffmpeg, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let ctx = BackgroundContext {
        video_items: names.iter().map(|n| n.to_string()).collect(),
        source_path: source_dir.to_string_lossy().to_string(),
        staging_dir,
        moss_dir: temp.path().join(".moss"),
        start_time: std::time::Instant::now(),
        ffmpeg_bin_path: Some(fake_ffmpeg.to_string_lossy().to_string()),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };
    (temp, ctx)
}

/// The common case: one video and an encoder that fails every invocation.
fn vault_with_one_unencodable_video() -> (TempDir, BackgroundContext) {
    vault_with_videos(&["videos/clip.mov"], "#!/bin/sh\nexit 1\n")
}

/// Headless services with a folder session attached, so the UiBound counter the
/// progress bar reads is observable, and `begin_ui_bound` called `n` times the
/// way `dispatch_video_conversions` does before spawning the worker.
fn services_with_budget(n: u32) -> BuildServices {
    let mut services = BuildServices::headless();
    services.session = Some(crate::system::folder_session::FolderSession::new(
        std::path::PathBuf::from("/tmp/moss-video-outcome-test"),
    ));
    for _ in 0..n {
        services.begin_ui_bound();
    }
    services
}

/// Run the worker the way production does: on a blocking thread, inside a
/// runtime. `run_video_conversion` needs both — it bridges the session's cancel
/// token with `tokio::spawn`, and it registers outputs with `blocking_send`.
async fn run_worker(
    services: &std::sync::Arc<BuildServices>,
    ctx: &std::sync::Arc<BackgroundContext>,
    epoch: u64,
    tx: Option<tokio::sync::mpsc::Sender<crate::build::coordinator::EmitMessage>>,
) {
    let (services, ctx) = (services.clone(), ctx.clone());
    tokio::task::spawn_blocking(move || run_video_conversion(&services, &ctx, epoch, tx))
        .await
        .unwrap();
}

/// Every video output path the worker registered with the coordinator.
fn drain_registered_paths(
    rx: &mut tokio::sync::mpsc::Receiver<crate::build::coordinator::EmitMessage>,
) -> Vec<String> {
    let mut paths = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        if let crate::build::coordinator::EmitMessage::File { rel_path, bucket, .. } = msg {
            assert_eq!(
                bucket,
                crate::build::manifest::HashBucket::VideoOutputs,
                "the video worker registers into video_outputs only"
            );
            paths.push(rel_path);
        }
    }
    paths
}

/// A video the encoder could not convert still ships its original bytes — and
/// ships them AT THE URL THE PAGE ASKS FOR.
///
/// Both halves were wrong before the per-item outcome. The three fallback arms
/// each copied the original to the SOURCE path (`videos/clip.mov`) while
/// announcing `to_mp4`'s `videos/clip.mp4`, so a published site referenced a
/// file that existed nowhere — invisible in preview, where
/// `set_source_passthrough` serves the original at the `.mp4` URL anyway. And
/// none of them pushed to the census, so the stray bytes belonged to no
/// manifest bucket and `remove_stale_files` deleted them on the next build.
///
/// A unit test over the outcome enum cannot see either: both are properties of
/// what reached the disk and the coordinator.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_encode_ships_original_at_the_mp4_url_and_registers_it() {
    let (_temp, ctx) = vault_with_one_unencodable_video();
    let ctx = std::sync::Arc::new(ctx);
    let services = std::sync::Arc::new(services_with_budget(1));
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    run_worker(&services, &ctx, 0, Some(tx)).await;

    let mp4 = ctx.staging_dir.join("videos/clip.mp4");
    assert!(
        mp4.exists(),
        "the fallback must land bytes at the .mp4 URL the page references, not at the source path"
    );
    assert_eq!(
        std::fs::read(&mp4).unwrap(),
        b"original video bytes",
        "the bytes at the .mp4 URL are the original's"
    );
    assert!(
        !ctx.staging_dir.join("videos/clip.mov").exists(),
        "no stray copy at the source path: nothing references it and the stale sweep would delete it"
    );

    assert_eq!(
        drain_registered_paths(&mut rx),
        vec!["videos/clip.mp4".to_string()],
        "the shipped video must be in the census the manifest's video_outputs is built from"
    );

    assert!(
        !services.has_ui_bound(),
        "the item's UiBound permit is released on the failure path too — an unpaired \
         begin leaves the progress bar running forever"
    );
}

/// A dispatch superseded WHILE the encoder is running abandons the item — it
/// does not ship a fallback and does not report the video as handled.
///
/// This is the stop-versus-failure boundary, at the one site where the two used
/// to be blurred. The encode-failure arm discriminated on the cancel flag only,
/// and `start_new_conversion` is a bare `fetch_add` that never sets that flag —
/// so a superseded encode fell through to the failure path, wrote fallback
/// bytes into the staging tree the fresh epoch was already rewriting, and told
/// the user its video had "shipped without optimizing" while that video was
/// being re-encoded. The typed outcome makes the two endings different values;
/// this test is what proves the right one is chosen.
///
/// The epoch is bumped only once ffmpeg has actually been entered (the worker
/// blocks inside it until this test releases it), so the supersession lands
/// mid-encode rather than at the loop-top check that always handled it.
#[cfg(unix)] // the fake ffmpeg is a /bin/sh script; Windows cannot run it (3 real-Windows runs, 2026-09-10/11)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn superseded_mid_encode_abandons_instead_of_shipping_a_fallback() {
    // Enters, announces itself, waits for the test, then fails the encode.
    let (temp, ctx) = vault_with_videos(
        &["videos/clip.mov", "videos/second.mov"],
        "#!/bin/sh\n\
         touch '{temp}/entered'\n\
         while [ ! -f '{temp}/release' ]; do sleep 0.02; done\n\
         exit 1\n",
    );
    let entered = temp.path().join("entered");
    let release = temp.path().join("release");

    let ctx = std::sync::Arc::new(ctx);
    let services = std::sync::Arc::new(services_with_budget(2));
    // A sentinel the run must not touch: the receipt belongs to the epoch that
    // superseded this one, and that count is still growing.
    services
        .videos_converted
        .store(42, std::sync::atomic::Ordering::Relaxed);

    let epoch = services.cancellation.current_id();
    let worker = {
        let (services, ctx) = (services.clone(), ctx.clone());
        tokio::task::spawn_blocking(move || run_video_conversion(&services, &ctx, epoch, None))
    };

    // Wait for the encoder to be entered — no fixed sleep: the child holds the
    // worker there until `release` appears, so this cannot race ahead.
    for _ in 0..1000 {
        if entered.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(entered.exists(), "the worker never reached the encoder");

    services.cancellation.start_new_conversion();
    std::fs::write(&release, b"go").unwrap();
    worker.await.unwrap();

    assert!(
        !ctx.staging_dir.join("videos/clip.mp4").exists(),
        "a superseded encode must not ship a fallback: the fresh epoch owns that output path"
    );
    assert!(
        !services.has_ui_bound(),
        "abandoning at item 0 of 2 releases BOTH permits — releasing only the current \
         item's leaves the progress bar running forever, because there is no next \
         iteration to notice"
    );
    assert_eq!(
        services
            .videos_converted
            .load(std::sync::atomic::Ordering::Relaxed),
        42,
        "a superseded run leaves the receipt to the fresh epoch — the one thing that \
         distinguishes it from a cancel, and the one thing a collapsed abandonment \
         arm would lose"
    );
}

#[test]
fn test_run_video_conversion_headless_has_no_shell() {
    let services = BuildServices::headless();
    assert!(
        !services.reporter.shell_listening(),
        "Headless services have no shell to signal"
    );
    assert!(
        services.reporter.is_terminal(),
        "Headless services report to the terminal, not a shell"
    );
    assert!(
        services.assets.is_some(),
        "Headless services carry a per-build asset registry so failed \
         encodes are recorded and the seal tail can degrade them"
    );
}

#[test]
fn test_run_video_conversion_singleflight_dedup_headless() {
    use crate::build::media::video::VideoConversionOutcome;
    let services = BuildServices::headless();

    let test_oid = "abc123".to_string();
    let (result, shared) =
        services
            .in_flight_videos
            .do_work(&test_oid, || VideoConversionOutcome {
                error: None,
                hls_rungs: 0,
                poster: false,
            });
    assert!(result.is_some());
    assert!(result.unwrap().error.is_none(), "First call should succeed");
    assert!(!shared, "First call should not be shared");

    let (result2, shared2) =
        services
            .in_flight_videos
            .do_work(&test_oid, || VideoConversionOutcome {
                error: None,
                hls_rungs: 0,
                poster: false,
            });
    assert!(result2.is_some());
    assert!(
        result2.unwrap().error.is_none(),
        "Sequential call should succeed"
    );
    assert!(!shared2, "Sequential call should execute independently");
}

#[test]
fn test_run_video_conversion_task_tracker_with_session() {
    // With a session attached, begin/end accumulate as expected.
    // This documents the production wiring: BuildServices::from_app
    // attaches a FolderSession; begin_ui_bound/end_ui_bound increment
    // and decrement the session's counter. (Headless mode has no
    // session, so the counter is permanently 0 — see
    // test_build_services_headless_tracker_works.)
    let mut services = BuildServices::headless();
    services.session = Some(crate::system::folder_session::FolderSession::new(
        std::path::PathBuf::from("/tmp/test"),
    ));

    for _ in 0..5 {
        services.begin_ui_bound();
    }
    assert!(services.has_ui_bound());

    for _ in 0..3 {
        services.end_ui_bound();
    }
    assert!(services.has_ui_bound());
}

// =========================================================================
// Tests for Video Item Fingerprinting (per-item, not per-set)
// =========================================================================

#[test]
fn test_compute_video_item_fingerprint_deterministic() {
    use crate::build::media::ffmpeg::VideoCompressionConfig;

    let dir = tempfile::tempdir().unwrap();
    let video_path = dir.path().join("test.mov");
    std::fs::write(&video_path, b"fake video data").unwrap();

    let config = VideoCompressionConfig::default();

    let fp1 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config);
    let fp2 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config);
    assert_eq!(fp1, fp2, "Same inputs should produce same fingerprint");
    assert!(fp1.is_some());
}

#[test]
fn test_compute_video_item_fingerprint_changes_with_content() {
    use crate::build::media::ffmpeg::VideoCompressionConfig;

    let dir = tempfile::tempdir().unwrap();
    let video_path = dir.path().join("test.mov");
    std::fs::write(&video_path, b"video data v1").unwrap();

    let config = VideoCompressionConfig::default();

    let fp1 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config);

    std::fs::write(
        &video_path,
        b"video data v2 - different content with more bytes",
    )
    .unwrap();

    let fp2 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config);
    assert_ne!(
        fp1, fp2,
        "Different content/size should produce different fingerprint"
    );
}

#[test]
fn test_compute_video_item_fingerprint_changes_with_config() {
    use crate::build::media::ffmpeg::VideoCompressionConfig;

    let dir = tempfile::tempdir().unwrap();
    let video_path = dir.path().join("test.mov");
    std::fs::write(&video_path, b"fake video data").unwrap();

    let config1 = VideoCompressionConfig::default();
    let config2 = VideoCompressionConfig {
        max_size_mb: 50,
        ..Default::default()
    };

    let fp1 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config1);
    let fp2 = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "test.mov", &config2);
    assert_ne!(
        fp1, fp2,
        "Different compression config should produce different fingerprint"
    );
}

#[test]
fn test_compute_video_item_fingerprint_missing_source_returns_none() {
    use crate::build::media::ffmpeg::VideoCompressionConfig;

    let dir = tempfile::tempdir().unwrap();
    let config = VideoCompressionConfig::default();

    // No file written at "missing.mov" — the caller (dispatch_video_conversions)
    // must treat `None` as "cannot prove unchanged" and dispatch it, never as
    // a distinct, cacheable fingerprint value of its own.
    let fp = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "missing.mov", &config);
    assert!(fp.is_none(), "An unstat-able source must not produce a fingerprint");
}

#[test]
fn test_compute_video_item_fingerprint_differs_by_path_even_with_identical_content() {
    use crate::build::media::ffmpeg::VideoCompressionConfig;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.mov"), b"same bytes").unwrap();
    std::fs::write(dir.path().join("b.mov"), b"same bytes").unwrap();

    let config = VideoCompressionConfig::default();

    let fp_a = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "a.mov", &config);
    let fp_b = compute_video_item_fingerprint(dir.path().to_str().unwrap(), "b.mov", &config);
    assert_ne!(
        fp_a, fp_b,
        "two different videos with identical bytes must not alias to the same per-item fingerprint"
    );
}

#[test]
fn test_dispatch_skips_when_item_fingerprint_unchanged() {
    let services = BuildServices::headless();
    let fingerprint = "test_fingerprint_abc123";

    assert!(!services
        .cancellation
        .check_and_update_item_fingerprint("videos/a.mov", fingerprint));
    assert!(services
        .cancellation
        .check_and_update_item_fingerprint("videos/a.mov", fingerprint));
}

// =========================================================================
// Image AssetReady emission tests
// =========================================================================

#[test]
fn test_is_image_extension_recognizes_common_formats() {
    assert!(is_image_extension("jpg"));
    assert!(is_image_extension("jpeg"));
    assert!(is_image_extension("png"));
    assert!(is_image_extension("gif"));
    assert!(is_image_extension("webp"));
    assert!(is_image_extension("svg"));
}

#[test]
fn test_is_image_extension_rejects_non_images() {
    assert!(!is_image_extension("css"));
    assert!(!is_image_extension("js"));
    assert!(!is_image_extension("html"));
    assert!(!is_image_extension("woff2"));
    assert!(!is_image_extension("mp4"));
    assert!(!is_image_extension("mov"));
    assert!(!is_image_extension(""));
}

#[test]
fn test_copy_deferred_assets_accepts_event_sink() {
    use std::collections::HashSet;

    let temp_dir =
        std::env::temp_dir().join(format!("moss_test_asset_ready_{}", std::process::id()));
    let source = temp_dir.join("source");
    let output = temp_dir.join("output");
    let moss = temp_dir.join(".moss");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    std::fs::write(source.join("photo.jpg"), b"fake image data").unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output,
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_copy_deferred_assets_copies_source_html_files() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    let interactive = source.join("interactive");
    fs::create_dir_all(&interactive).unwrap();
    fs::write(
        interactive.join("sketch.html"),
        "<html><body>interactive sketch</body></html>",
    )
    .unwrap();
    fs::write(interactive.join("p5.js"), "// p5 library").unwrap();
    fs::write(interactive.join("article.md"), "# Article").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());
    blocking_keys.insert("interactive/my-article/index.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output.join("interactive/sketch.html").exists(),
        "Source HTML file interactive/sketch.html must be copied to output"
    );
    assert!(
        output.join("interactive/p5.js").exists(),
        "JS file interactive/p5.js must be copied to output"
    );
    assert!(
        !output.join("interactive/article.md").exists(),
        "Markdown files should not be copied by background phase"
    );
}

#[test]
fn test_copy_deferred_assets_skips_html_when_in_blocking_keys() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    let interactive = source.join("interactive");
    fs::create_dir_all(&interactive).unwrap();
    fs::write(interactive.join("sketch.html"), "<html>sketch</html>").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());
    blocking_keys.insert("interactive/sketch.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        !output.join("interactive/sketch.html").exists(),
        "Source HTML is skipped when blocking_keys is polluted (bug behavior)"
    );
}

// =========================================================================
// Stale Directory Cleanup Tests
// =========================================================================

#[test]
fn test_stale_directory_removed_after_article_deletion() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&moss).unwrap();

    let active_dir = output.join("articles/active-article");
    let stale_dir = output.join("articles/stale-article");
    fs::create_dir_all(&active_dir).unwrap();
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(active_dir.join("index.html"), "<html>active</html>").unwrap();

    let mut site_hashes = SiteHashes::new();
    site_hashes.insert(
        "articles/active-article/index.html".to_string(),
        "hash123".to_string(),
    );

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("articles/active-article/index.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    // Stale-dir cleanup moved to the seal+persist side task in build.rs
    // (#621). The tx=None path no longer runs cleanup inline. Simulate
    // the new flow by calling cleanup explicitly with the same site_hashes
    // the in-band call would have used.
    let expected_dirs = compute_expected_dirs(&site_hashes);
    remove_stale_dirs(&output, &expected_dirs);

    assert!(
        !stale_dir.exists(),
        "Stale directory articles/stale-article/ should be removed"
    );
    assert!(
        active_dir.exists(),
        "Active directory articles/active-article/ should be preserved"
    );
    assert!(
        output.join("articles").exists(),
        "Parent directory articles/ should be preserved"
    );
}

#[test]
fn test_stale_directory_with_stale_files_removed() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&moss).unwrap();

    let stale_dir = output.join("old-section/leftover");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(stale_dir.join("orphan.html"), "<html>orphan</html>").unwrap();

    let mut site_hashes = SiteHashes::new();
    site_hashes.insert("index.html".to_string(), "hash".to_string());

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    // Stale-file + stale-dir cleanup moved to seal+persist side task (#621);
    // simulate that step here so this test continues to exercise the
    // cleanup logic.
    remove_stale_files(&output, &site_hashes, "test");
    let expected_dirs = compute_expected_dirs(&site_hashes);
    remove_stale_dirs(&output, &expected_dirs);

    assert!(
        !output.join("old-section").exists(),
        "Entire stale directory tree should be removed"
    );
}

#[test]
fn test_video_output_directories_preserved() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(moss.join("build")).unwrap();

    let videos_dir = output.join("videos");
    fs::create_dir_all(&videos_dir).unwrap();
    fs::write(videos_dir.join("clip.mp4"), "fake video").unwrap();

    let mut site_hashes = SiteHashes::new();
    site_hashes.insert("index.html".to_string(), "hash".to_string());
    site_hashes
        .video_outputs
        .insert("videos/clip.mp4".to_string());

    let hashes_json = serde_json::to_string_pretty(&site_hashes).unwrap();
    fs::write(moss.join("build").join("hashes.json"), &hashes_json).unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        videos_dir.exists(),
        "videos/ directory should be preserved (contains video_outputs)"
    );
}

// =========================================================================
// Stale Staging Cleanup Tests
// =========================================================================

#[test]
fn test_remove_stale_files_deletes_unlisted_files() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    fs::create_dir_all(&dir).unwrap();

    fs::write(dir.join("index.html"), "<html>keep</html>").unwrap();
    fs::write(dir.join("custom.css"), "body { color: red }").unwrap();

    let mut hashes = SiteHashes::new();
    hashes.insert("index.html".to_string(), "hash1".to_string());

    remove_stale_files(&dir, &hashes, "test");

    assert!(dir.join("index.html").exists(), "index.html should be kept");
    assert!(
        !dir.join("custom.css").exists(),
        "custom.css should be removed as stale"
    );
}

#[test]
fn test_remove_stale_files_preserves_video_outputs() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let videos = dir.join("videos");
    fs::create_dir_all(&videos).unwrap();

    fs::write(videos.join("clip.mp4"), "fake video").unwrap();

    let mut hashes = SiteHashes::new();
    hashes.video_outputs.insert("videos/clip.mp4".to_string());

    remove_stale_files(&dir, &hashes, "test");

    assert!(
        videos.join("clip.mp4").exists(),
        "video output should be preserved"
    );
}

/// Mirrors `test_remove_stale_files_preserves_video_outputs` for WebP
/// outputs. This is the regression test for CRITICAL-1: without
/// `image_outputs` in `SiteHashes`, every freshly-produced `.webp` is
/// deleted on the next rebuild because it has no entry in `files`
/// (the source is .jpg/.png, not .webp).
#[test]
fn test_remove_stale_files_preserves_image_outputs() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let images = dir.join("images");
    fs::create_dir_all(&images).unwrap();

    fs::write(images.join("hero.webp"), "fake webp bytes").unwrap();

    let mut hashes = SiteHashes::new();
    hashes.image_outputs.insert("images/hero.webp".to_string());

    remove_stale_files(&dir, &hashes, "test");

    assert!(
        images.join("hero.webp").exists(),
        "image output should be preserved"
    );
}

/// Files not in any tracking set still get deleted — sanity check that
/// image_outputs preservation is scoped to the tracked set.
#[test]
fn test_remove_stale_files_deletes_untracked_webp() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let images = dir.join("images");
    fs::create_dir_all(&images).unwrap();

    fs::write(images.join("tracked.webp"), "tracked").unwrap();
    fs::write(images.join("orphan.webp"), "orphan").unwrap();

    let mut hashes = SiteHashes::new();
    hashes
        .image_outputs
        .insert("images/tracked.webp".to_string());

    remove_stale_files(&dir, &hashes, "test");

    assert!(
        images.join("tracked.webp").exists(),
        "tracked webp preserved"
    );
    assert!(
        !images.join("orphan.webp").exists(),
        "untracked webp should be deleted"
    );
}

/// ADR-030 §3.4 append-only retention: a math PNG whose equation was
/// edited or deleted has NO entry in the new build's hashes, but its URL
/// is baked into already-sent emails (Gmail's proxy caches it forever).
/// Stale-file cleanup must never touch `_moss/math/`.
#[test]
fn remove_stale_files_never_deletes_math_pngs() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let math = dir.join("_moss").join("math");
    fs::create_dir_all(&math).unwrap();

    // Untracked on purpose — the equation is gone from every page.
    fs::write(math.join("aaaaaaaaaaaaaaaa.png"), "png bytes").unwrap();

    let hashes = SiteHashes::new();
    remove_stale_files(&dir, &hashes, "test");

    assert!(
        math.join("aaaaaaaaaaaaaaaa.png").exists(),
        "math PNGs are append-only and must survive stale cleanup"
    );
}

/// The directory-level counterpart: a build with no math pages computes
/// no expected `_moss/math` dir, but the dir holds append-only PNGs.
#[test]
fn remove_stale_dirs_never_deletes_math_dir() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let math = dir.join("_moss").join("math");
    fs::create_dir_all(&math).unwrap();
    fs::write(math.join("aaaaaaaaaaaaaaaa.png"), "png bytes").unwrap();

    // Expected dirs cover _moss (other artifacts) but NOT _moss/math.
    let mut expected = std::collections::HashSet::new();
    expected.insert(std::path::PathBuf::from("_moss"));
    remove_stale_dirs(&dir, &expected);

    assert!(
        math.join("aaaaaaaaaaaaaaaa.png").exists(),
        "_moss/math must survive dir-level stale cleanup"
    );
}

/// Regression: a notebook hash entry the asset walk does not own must reach
/// the sealed deploy manifest untouched.
///
/// The bug: notebook artifacts are hashed into the manifest for the deploy
/// wire, but the background asset walk cannot see them (they live under
/// `jupyter/**` and their sources are `.ipynb` → a different extension). The
/// walk used to hold a snapshot clone of the whole manifest and re-emit it,
/// so anything it failed to recognise was swept before persistence — the
/// deploy manifest shipped to seta missed those paths and prod 404'd
/// `/resources/habitable-zone.html` and `/jupyter/**`.
///
/// The walk now owns only the static-asset slice, seeded from the PREVIOUS
/// build's manifest (moss#618), so it has no way to sweep an entry belonging
/// to another phase. This test holds that: the notebook entry is registered
/// into the pending manifest the coordinator carries — as
/// `run_notebook_processing` registers it in `build_inner` — and must still be
/// there after the walk has run and the manifest is sealed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_copy_deferred_assets_preserves_notebook_hash_entries_for_deploy() {
    use crate::build::coordinator::test_utils;
    use crate::build::manifest::{HashBucket, PendingManifest};

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let nb_path = "resources/habitable-zone.html".to_string();
    let nb_hash = "deadbeefdeadbeef".to_string();

    // What `run_notebook_processing` does in `build_inner`: register the
    // output into the live pending manifest, NOT into anything the walk holds.
    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.apply_message(nb_path.clone(), &nb_hash, HashBucket::NotebookOutputs);

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    // copy_deferred_assets uses `tx.blocking_send` which cannot run on a
    // tokio worker thread; spawn_blocking moves it to a dedicated blocking
    // pool thread.
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into(rx, pending).await;

    assert!(
        sealed.files().contains_key(&nb_path),
        "notebook hash must survive the asset walk and reach the deploy manifest. \
             got files={:?}",
        sealed.files().keys().collect::<Vec<_>>()
    );
    assert!(
        sealed.notebook_outputs().contains(&nb_path),
        "notebook_outputs set must persist too"
    );
}

/// Mirrors `test_remove_stale_files_preserves_image_outputs` for notebook
/// outputs. Without `notebook_outputs` carry-forward, JupyterLite-generated
/// files (jupyter/**, notebook.html viewer wrappers, .ipynb copies) are
/// deleted on every rebuild because their source is .ipynb but the outputs
/// have different extensions — matching the webp pattern exactly.
///
/// Regression test for a 404 on /resources/habitable-zone.html observed in
/// test-sites/chps-site: rebuild → stale cleanup deletes the viewer HTML →
/// iframe 404 until the next full notebook regeneration completes.
#[test]
fn test_remove_stale_files_preserves_notebook_outputs() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let resources = dir.join("resources");
    let jupyter_lab = dir.join("jupyter").join("lab");
    fs::create_dir_all(&resources).unwrap();
    fs::create_dir_all(&jupyter_lab).unwrap();

    // Viewer HTML wrapper (sibling of .ipynb at its natural path)
    fs::write(resources.join("habitable-zone.html"), "viewer").unwrap();
    // JupyterLite asset file
    fs::write(jupyter_lab.join("index.html"), "lab").unwrap();
    // .ipynb copy (for direct access / JupyterLite load)
    fs::write(resources.join("habitable-zone.ipynb"), "nb").unwrap();
    // Orphan: not in any preserve-set, must be deleted (proves the
    // function is actually scanning, not no-op'ing).
    fs::write(jupyter_lab.join("orphan.js"), "stale").unwrap();

    let mut hashes = SiteHashes::new();
    hashes
        .notebook_outputs
        .insert("resources/habitable-zone.html".to_string());
    hashes
        .notebook_outputs
        .insert("jupyter/lab/index.html".to_string());
    hashes
        .notebook_outputs
        .insert("resources/habitable-zone.ipynb".to_string());

    remove_stale_files(&dir, &hashes, "test");

    assert!(
        resources.join("habitable-zone.html").exists(),
        "notebook viewer HTML should be preserved"
    );
    assert!(
        jupyter_lab.join("index.html").exists(),
        "jupyter asset should be preserved"
    );
    assert!(
        resources.join("habitable-zone.ipynb").exists(),
        ".ipynb copy should be preserved"
    );
    assert!(
        !jupyter_lab.join("orphan.js").exists(),
        "untracked file should still be deleted"
    );
}

/// `remove_stale_html` also needs notebook_outputs protection because
/// JupyterLite's SPA shell produces dozens of `index.html` files under
/// `jupyter/{lab,tree,doc,repl,consoles,edit,notebooks}/...` — all
/// matching the generated-page pattern this function targets.
///
/// These live in `notebook_outputs`, not `blocking_keys` (background-phase
/// emission), so the function must accept a second preserve-set.
#[test]
fn test_remove_stale_html_preserves_notebook_outputs() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    fs::create_dir_all(dir.join("jupyter/lab")).unwrap();
    fs::create_dir_all(dir.join("jupyter/tree")).unwrap();
    fs::create_dir_all(dir.join("jupyter/stale-section")).unwrap();

    fs::write(dir.join("index.html"), "<html>home</html>").unwrap();
    fs::write(dir.join("jupyter/lab/index.html"), "<html>lab</html>").unwrap();
    fs::write(dir.join("jupyter/tree/index.html"), "<html>tree</html>").unwrap();
    // Orphan index.html not in any preserve-set — must be deleted
    // (proves stale-html is actually sweeping, not no-op'ing).
    fs::write(
        dir.join("jupyter/stale-section/index.html"),
        "<html>stale</html>",
    )
    .unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());

    let mut notebook_outputs = HashSet::new();
    notebook_outputs.insert("jupyter/lab/index.html".to_string());
    notebook_outputs.insert("jupyter/tree/index.html".to_string());

    remove_stale_html(&dir, &blocking_keys, &notebook_outputs);

    assert!(dir.join("index.html").exists(), "home kept");
    assert!(
        dir.join("jupyter/lab/index.html").exists(),
        "jupyter/lab/index.html protected by notebook_outputs"
    );
    assert!(
        dir.join("jupyter/tree/index.html").exists(),
        "jupyter/tree/index.html protected by notebook_outputs"
    );
    assert!(
        !dir.join("jupyter/stale-section/index.html").exists(),
        "orphan index.html should still be swept"
    );
}

#[test]
fn test_remove_stale_dirs_removes_empty_stale_directories() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    let keep_dir = dir.join("js");
    let stale_dir = dir.join("old-section");
    fs::create_dir_all(&keep_dir).unwrap();
    fs::create_dir_all(&stale_dir).unwrap();

    let mut expected_dirs = std::collections::HashSet::new();
    expected_dirs.insert(std::path::PathBuf::from("js"));

    remove_stale_dirs(&dir, &expected_dirs);

    assert!(keep_dir.exists(), "js/ should be preserved");
    assert!(!stale_dir.exists(), "old-section/ should be removed");
}

#[test]
fn test_compute_expected_dirs_includes_qr_directory() {
    let mut hashes = SiteHashes::default();
    hashes
        .files
        .insert("qr/some-article.svg".to_string(), "hash1".to_string());
    hashes
        .files
        .insert("qr/another-article.svg".to_string(), "hash2".to_string());
    // QR keys mirror the page's whole url_path, so a language edition nests
    // (`qr/zh-hans/about.svg`). The sweeper must keep that subdirectory too or
    // it deletes a live QR right after the build wrote it.
    hashes
        .files
        .insert("qr/zh-hans/about.svg".to_string(), "hash4".to_string());
    hashes
        .files
        .insert("_moss/js/theme.js".to_string(), "hash3".to_string());

    let dirs = compute_expected_dirs(&hashes);

    assert!(
        dirs.contains(&std::path::PathBuf::from("qr")),
        "qr/ should be in expected_dirs when QR files are registered"
    );
    assert!(
        dirs.contains(&std::path::PathBuf::from("qr/zh-hans")),
        "a nested language QR directory should be in expected_dirs"
    );
    assert!(
        dirs.contains(&std::path::PathBuf::from("_moss/js")),
        "_moss/js/ should be in expected_dirs"
    );
    assert!(
        dirs.contains(&std::path::PathBuf::from("_moss")),
        "_moss/ should be in expected_dirs"
    );
}

#[test]
fn test_copy_deferred_assets_cleans_staging_dir() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let staging = temp.path().join("site-stage");
    let site = temp.path().join("site");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(&site).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    fs::write(staging.join("custom.css"), "body { old }").unwrap();
    fs::write(site.join("custom.css"), "body { old }").unwrap();

    let mut site_hashes = SiteHashes::new();
    site_hashes.insert("index.html".to_string(), "hash1".to_string());

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    // Stale-file cleanup moved to the seal+persist side task in build.rs
    // (#621). Simulate that step here so this test continues to exercise
    // the staging+site cleanup logic.
    remove_stale_files(&site, &site_hashes, "site");
    remove_stale_files(&staging, &site_hashes, "staging");

    assert!(
        !site.join("custom.css").exists(),
        "Stale custom.css should be removed from site/"
    );
    assert!(
        !staging.join("custom.css").exists(),
        "Stale custom.css should be removed from site-stage/"
    );
}

#[test]
fn test_remove_stale_html_deletes_orphaned_index_pages() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    fs::create_dir_all(dir.join("文字/article")).unwrap();
    fs::create_dir_all(dir.join("视频/old-video")).unwrap();

    fs::write(dir.join("index.html"), "<html>home</html>").unwrap();
    fs::write(dir.join("文字/article/index.html"), "<html>article</html>").unwrap();
    fs::write(dir.join("视频/old-video/index.html"), "<html>stale</html>").unwrap();
    fs::write(dir.join("index-2.html"), "<html>stale page 2</html>").unwrap();
    fs::write(dir.join("视频/old-video/sketch.html"), "<html>embed</html>").unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());
    blocking_keys.insert("文字/article/index.html".to_string());

    remove_stale_html(&dir, &blocking_keys, &HashSet::new());

    assert!(dir.join("index.html").exists(), "current index.html kept");
    assert!(
        dir.join("文字/article/index.html").exists(),
        "current article kept"
    );
    assert!(
        !dir.join("视频/old-video/index.html").exists(),
        "stale index.html removed"
    );
    assert!(
        !dir.join("index-2.html").exists(),
        "stale paginated page removed"
    );
    assert!(
        dir.join("视频/old-video/sketch.html").exists(),
        "source HTML asset preserved"
    );
}

#[test]
fn test_remove_stale_html_preserves_non_html_files() {
    use std::collections::HashSet;
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("output");
    fs::create_dir_all(&dir).unwrap();

    fs::write(dir.join("index.html"), "<html>home</html>").unwrap();
    fs::write(dir.join("style.css"), "body {}").unwrap();
    fs::write(dir.join("photo.jpg"), "fake jpg").unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("index.html".to_string());

    remove_stale_html(&dir, &blocking_keys, &HashSet::new());

    assert!(dir.join("index.html").exists());
    assert!(dir.join("style.css").exists(), "CSS preserved");
    assert!(dir.join("photo.jpg").exists(), "image preserved");
}

#[test]
fn test_resolve_source_hash_uses_cached_hash_on_mtime_match() {
    use crate::build::cache::HashIndex;

    let temp = TempDir::new().unwrap();
    let video_file = temp.path().join("video.mov");
    fs::write(&video_file, b"fake video content for hashing test").unwrap();

    let meta = fs::metadata(&video_file).unwrap();
    let size = meta.len();
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut hash_index = HashIndex::new();
    hash_index.update(
        "video.mov".to_string(),
        size,
        mtime,
        "cached_hash_abc123".to_string(),
    );

    let result =
        super::super::video::resolve_source_hash(&video_file, "video.mov", &mut hash_index);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cached_hash_abc123");
}

#[test]
fn test_resolve_source_hash_rehashes_on_mtime_mismatch() {
    use crate::build::cache::HashIndex;

    let temp = TempDir::new().unwrap();
    let video_file = temp.path().join("video.mov");
    fs::write(&video_file, b"fake video content").unwrap();

    let meta = fs::metadata(&video_file).unwrap();
    let size = meta.len();

    let mut hash_index = HashIndex::new();
    hash_index.update(
        "video.mov".to_string(),
        size,
        1000,
        "stale_hash".to_string(),
    );

    let result =
        super::super::video::resolve_source_hash(&video_file, "video.mov", &mut hash_index);
    assert!(result.is_ok());
    let hash = result.unwrap();
    assert_ne!(hash, "stale_hash", "Should NOT return stale hash");
    assert_eq!(hash.len(), 64, "Should be a SHA-256 hex hash");
}

#[test]
fn test_resolve_source_hash_updates_index_on_miss() {
    use crate::build::cache::HashIndex;

    let temp = TempDir::new().unwrap();
    let video_file = temp.path().join("video.mov");
    fs::write(&video_file, b"content for index update test").unwrap();

    let mut hash_index = HashIndex::new();
    assert!(hash_index.entries.is_empty());

    let result =
        super::super::video::resolve_source_hash(&video_file, "video.mov", &mut hash_index);
    assert!(result.is_ok());

    assert!(
        hash_index.entries.contains_key("video.mov"),
        "HashIndex should be updated with new entry"
    );
}

// =========================================================================
// Fast-path 0-byte corruption detection
// =========================================================================

#[test]
fn test_fast_path_rejects_zero_byte_canonical_files() {
    use crate::build::cache::HashIndex;


    let temp = TempDir::new().unwrap();
    let source_dir = temp.path().join("source");
    let staging_dir = temp.path().join("site-stage");
    let canonical_dir = temp.path().join("site");
    let moss_dir = temp.path().join(".moss");

    fs::create_dir_all(source_dir.join("videos")).unwrap();
    fs::create_dir_all(staging_dir.join("videos")).unwrap();
    fs::create_dir_all(canonical_dir.join("videos")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    fs::write(
        source_dir.join("videos/clip.mov"),
        b"fake video source data",
    )
    .unwrap();

    fs::write(canonical_dir.join("videos/clip.mp4"), b"").unwrap();
    fs::write(canonical_dir.join("videos/clip.thumb.jpg"), b"").unwrap();

    let source_meta = fs::metadata(source_dir.join("videos/clip.mov")).unwrap();
    let source_size = source_meta.len();
    let source_mtime = source_meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut hash_index = HashIndex::new();
    hash_index.update(
        "videos/clip.mov".to_string(),
        source_size,
        source_mtime,
        "fake_hash_123".to_string(),
    );
    let hash_index_path = moss_dir.join("build").join("cache").join("hash-index.json");
    let hash_json = serde_json::to_string_pretty(&hash_index).unwrap();
    fs::write(&hash_index_path, hash_json).unwrap();

    let services = BuildServices::headless();
    let ctx = BackgroundContext {
        video_items: vec!["videos/clip.mov".to_string()],
        source_path: source_dir.to_string_lossy().to_string(),
        staging_dir: staging_dir.clone(),
        moss_dir: moss_dir.clone(),
        start_time: std::time::Instant::now(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    run_video_conversion(&services, &ctx, 0, None);

    let staging_mp4 = staging_dir.join("videos/clip.mp4");
    if staging_mp4.exists() {
        let size = fs::metadata(&staging_mp4).unwrap().len();
        assert!(
            size > 0,
            "Fast-path should NOT propagate 0-byte corrupt mp4 to staging (got {} bytes)",
            size
        );
    }
}

#[test]
fn test_has_output_changes_false_when_hashes_carried_forward() {
    let mut previous = SiteHashes::new();
    previous.insert("index.html".to_string(), "html_hash_1".to_string());
    previous.insert("style.css".to_string(), "css_hash_1".to_string());
    previous.insert("images/photo.jpg".to_string(), "asset_hash_1".to_string());
    previous.insert("videos/clip.mp4".to_string(), "asset_hash_2".to_string());

    let mut new_hashes = SiteHashes::new();
    new_hashes.files = previous.files.clone();
    new_hashes.insert("index.html".to_string(), "html_hash_1".to_string());
    new_hashes.insert("style.css".to_string(), "css_hash_1".to_string());

    let has_output_changes = !new_hashes.get_changed_files(&previous).is_empty()
        || !new_hashes.get_new_files(&previous).is_empty()
        || !new_hashes.get_deleted_files(&previous).is_empty();

    assert!(
        !has_output_changes,
        "Should detect no changes when same folder reopened with carry-forward"
    );
}

#[test]
fn test_has_output_changes_true_without_carry_forward() {
    let mut previous = SiteHashes::new();
    previous.insert("index.html".to_string(), "html_hash_1".to_string());
    previous.insert("images/photo.jpg".to_string(), "asset_hash_1".to_string());

    let mut new_hashes = SiteHashes::new();
    new_hashes.insert("index.html".to_string(), "html_hash_1".to_string());

    let deleted = new_hashes.get_deleted_files(&previous);
    assert_eq!(deleted, vec!["images/photo.jpg".to_string()]);
}

// =========================================================================
// Asset Path Mapping via dir_overrides Tests
// =========================================================================

#[test]
fn test_copy_deferred_assets_maps_paths_through_dir_overrides() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    let chinese_dir = source.join("交互");
    fs::create_dir_all(&chinese_dir).unwrap();
    fs::write(
        chinese_dir.join("particle-universe.html"),
        "<html>particles</html>",
    )
    .unwrap();
    fs::write(chinese_dir.join("p5.js"), "// p5 library").unwrap();
    fs::write(chinese_dir.join("sketch.js"), "// sketch code").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut dir_overrides = HashMap::new();
    dir_overrides.insert("交互".to_string(), "interactive".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        dir_overrides,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output.join("interactive/particle-universe.html").exists(),
        "particle-universe.html should be at interactive/, not 交互/"
    );
    assert!(
        output.join("interactive/p5.js").exists(),
        "p5.js should be at interactive/, not 交互/"
    );
    assert!(
        output.join("interactive/sketch.js").exists(),
        "sketch.js should be at interactive/, not 交互/"
    );
    assert!(
        !output.join("交互/particle-universe.html").exists(),
        "particle-universe.html should NOT be at original 交互/ path"
    );
}

#[test]
fn test_copy_deferred_assets_maps_nested_dir_overrides() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    let nested_dir = source.join("文字").join("游记");
    fs::create_dir_all(&nested_dir).unwrap();
    fs::write(nested_dir.join("photo.jpg"), "fake image").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut dir_overrides = HashMap::new();
    dir_overrides.insert("文字".to_string(), "writings".to_string());
    dir_overrides.insert("文字/游记".to_string(), "travel".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        dir_overrides,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output.join("writings/travel/photo.jpg").exists(),
        "photo.jpg should be at writings/travel/, not 文字/游记/"
    );
    assert!(
        !output.join("文字/游记/photo.jpg").exists(),
        "photo.jpg should NOT be at original 文字/游记/ path"
    );
}

#[test]
fn test_copy_deferred_assets_no_overrides_unchanged() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("image.png"), "fake png").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output.join("image.png").exists(),
        "image.png should be copied to output unchanged"
    );
}

#[test]
fn test_copy_deferred_assets_skips_root_style_css() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("style.css"), "body { color: red; }").unwrap();

    fs::create_dir_all(&output).unwrap();
    let default_css = ":root { --moss-color-accent: green; }";
    fs::write(output.join("style.css"), default_css).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("style.css".to_string());
    let mut site_hashes = SiteHashes::default();
    site_hashes.insert("style.css".to_string(), "css_hash".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    let content = fs::read_to_string(output.join("style.css")).unwrap();
    assert!(
        content.contains("--moss-color-accent"),
        "Root style.css should NOT be overwritten by user's style.css. Got: {}",
        content
    );
}

#[test]
fn test_copy_deferred_assets_skips_root_script_js() {
    use std::collections::HashMap;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("script.js"), "console.log('user script');").unwrap();

    fs::create_dir_all(&output).unwrap();
    let default_script = "console.log('moss default script');";
    fs::write(output.join("script.js"), default_script).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let mut blocking_keys = HashSet::new();
    blocking_keys.insert("script.js".to_string());
    let mut site_hashes = SiteHashes::default();
    site_hashes.insert("script.js".to_string(), "js_hash".to_string());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys,
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    let content = fs::read_to_string(output.join("script.js")).unwrap();
    assert!(
        content.contains("moss default script"),
        "Root script.js should NOT be overwritten by user's script.js. Got: {}",
        content
    );
}

// =========================================================================
// Video Path Mapping Tests (file-tree → page-tree)
// =========================================================================

/// Coordinator round-trip: video runner emits `VideoOutputs` for each
/// .mp4 + .thumb.jpg through the manifest channel. Both keys are
/// recorded under `SealedManifest::video_outputs` and NOT under
/// `files` / `blocking_keys`.
///
/// Pre-Track A this lived as `test_update_video_hashes_uses_mapped_paths`
/// and asserted the on-disk hashes.json contained the mapped paths after
/// `update_video_hashes(&ctx)`. The on-disk fallback is gone (#620 Item 2);
/// the runner now sends EmitMessage::VideoOutputs through the coordinator
/// channel. The mapping itself (file-tree → page-tree) happens inside the
/// runner's path-derivation; this test exercises the coordinator wiring
/// for already-mapped paths (the exact wire format the runner uses).
#[tokio::test]
async fn test_video_outputs_emitted_via_coordinator() {
    use crate::build::coordinator::{test_utils, EmitMessage};
    use crate::build::manifest::HashBucket;

    let (tx, rx) = test_utils::build_test_coordinator();

    // Mirror the runner: send mapped page-tree paths for both .mp4 and .thumb.jpg.
    for path in &["video/aimeili.mp4", "video/aimeili.thumb.jpg"] {
        tx.send(EmitMessage::File {
            rel_path: path.to_string(),
            hash: String::new(),
            bucket: HashBucket::VideoOutputs,
        })
        .await
        .unwrap();
    }
    drop(tx);

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    assert!(
        sealed.video_outputs().contains("video/aimeili.mp4"),
        "sealed manifest should contain mapped 'video/aimeili.mp4', got: {:?}",
        sealed.video_outputs()
    );
    assert!(
        sealed.video_outputs().contains("video/aimeili.thumb.jpg"),
        "sealed manifest should contain mapped 'video/aimeili.thumb.jpg', got: {:?}",
        sealed.video_outputs()
    );
    // VideoOutputs appear in `files` (the deploy wire manifest — every bucket does)
    // but must NOT appear in `blocking_keys` per HashBucket semantics
    // (manifest.rs module table: `inner.files` is always yes; `blocking_keys` is no
    // for VideoOutputs).
    assert!(
        sealed.files().contains_key("video/aimeili.mp4"),
        "VideoOutputs must reach files (deploy wire); got: {:?}",
        sealed.files().keys().collect::<Vec<_>>()
    );
    assert!(!sealed.blocking_keys().contains("video/aimeili.mp4"));
}

/// Coordinator round-trip: a no-overrides path round-trips through the
/// coordinator unchanged. Pre-Track A this lived as
/// `test_update_video_hashes_no_overrides`.
#[tokio::test]
async fn test_video_outputs_emitted_via_coordinator_no_overrides() {
    use crate::build::coordinator::{test_utils, EmitMessage};
    use crate::build::manifest::HashBucket;

    let (tx, rx) = test_utils::build_test_coordinator();
    for path in &["videos/clip.mp4", "videos/clip.thumb.jpg"] {
        tx.send(EmitMessage::File {
            rel_path: path.to_string(),
            hash: String::new(),
            bucket: HashBucket::VideoOutputs,
        })
        .await
        .unwrap();
    }
    drop(tx);

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.video_outputs().contains("videos/clip.mp4"),
        "sealed manifest should contain 'videos/clip.mp4', got: {:?}",
        sealed.video_outputs()
    );
    assert!(
        sealed.video_outputs().contains("videos/clip.thumb.jpg"),
        "sealed manifest should contain 'videos/clip.thumb.jpg', got: {:?}",
        sealed.video_outputs()
    );
}

#[test]
fn test_video_served_path_derivation_with_overrides() {
    use crate::build::render::resolve_path_with_overrides;
    use moss_core::asset_paths;

    let mut dir_overrides = std::collections::HashMap::new();
    dir_overrides.insert("视频".to_string(), "video".to_string());
    dir_overrides.insert("交互".to_string(), "interactive".to_string());

    let source_path = "视频/aimeili.mov";
    let mapped = resolve_path_with_overrides(source_path, &dir_overrides);
    assert_eq!(mapped, "video/aimeili.mov");

    let mp4 = asset_paths::to_mp4(&mapped);
    let thumb = asset_paths::to_thumb(&mapped);
    assert_eq!(mp4, "video/aimeili.mp4");
    assert_eq!(thumb, "video/aimeili.thumb.jpg");

    let nested_source = "交互/sketches/demo.mov";
    let nested_mapped = resolve_path_with_overrides(nested_source, &dir_overrides);
    assert_eq!(nested_mapped, "interactive/sketches/demo.mov");
    assert_eq!(
        asset_paths::to_mp4(&nested_mapped),
        "interactive/sketches/demo.mp4"
    );
    assert_eq!(
        asset_paths::to_thumb(&nested_mapped),
        "interactive/sketches/demo.thumb.jpg"
    );
}

// =========================================================================
// Notebook processing integration tests
// =========================================================================

#[test]
fn test_notebook_processing_noop_when_empty() {
    let outputs = run_notebook_processing(None, &[], "/test", Path::new("/test/output"), None);
    assert!(outputs.is_empty(), "No notebooks = no outputs");
}

#[test]
fn test_notebook_processing_copies_ipynb_and_generates_viewer() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&output).unwrap();

    let notebook_json = r#"{"cells":[{"cell_type":"code","source":["print('hello')"]}],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
    fs::write(source.join("test.ipynb"), notebook_json).unwrap();

    let files = vec![crate::types::content::FileInfo {
        path: "test.ipynb".to_string(),
        file_type: "ipynb".to_string(),
        size: notebook_json.len() as u64,
        modified: None,
    }];

    let outputs = run_notebook_processing(None, &files, &source.to_string_lossy(), &output, None);

    assert!(
        output.join("test.ipynb").exists(),
        "Notebook file should be copied to output"
    );
    assert!(
        receipt_paths(&outputs).contains(&"test.ipynb".to_string()),
        "Outputs should include the notebook path"
    );
}

#[test]
fn test_data_files_copied_to_jupyter_files() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let staging = temp.path().join("staging");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();

    // Create a notebook with a sibling data file
    let notebook_json = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
    fs::write(source.join("analysis.ipynb"), notebook_json).unwrap();
    fs::write(source.join("data.csv"), "name,value\nfoo,42\n").unwrap();
    // Hidden file should NOT be copied
    fs::write(source.join(".DS_Store"), "hidden").unwrap();

    let files = vec![crate::types::content::FileInfo {
        path: "analysis.ipynb".to_string(),
        file_type: "ipynb".to_string(),
        size: notebook_json.len() as u64,
        modified: None,
    }];

    let outputs = run_notebook_processing(None, &files, &source.to_string_lossy(), &staging, None);

    // Data file should be in jupyter/files/ (fallback path without JupyterLite)
    // Note: without JupyterLite assets, the function takes the fallback path
    // and only copies .ipynb files. Data file co-location runs in the full path.
    // This test verifies the notebook itself is copied.
    assert!(receipt_paths(&outputs).contains(&"analysis.ipynb".to_string()));
}

#[test]
fn test_notebook_processing_cancel_flag_returns_early() {
    use std::sync::atomic::AtomicBool;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&output).unwrap();

    let notebook_json = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
    fs::write(source.join("nb1.ipynb"), notebook_json).unwrap();
    fs::write(source.join("nb2.ipynb"), notebook_json).unwrap();

    let files = vec![
        crate::types::content::FileInfo {
            path: "nb1.ipynb".to_string(),
            file_type: "ipynb".to_string(),
            size: notebook_json.len() as u64,
            modified: None,
        },
        crate::types::content::FileInfo {
            path: "nb2.ipynb".to_string(),
            file_type: "ipynb".to_string(),
            size: notebook_json.len() as u64,
            modified: None,
        },
    ];

    // Set cancel flag before starting
    let cancel = AtomicBool::new(true);
    let outputs =
        run_notebook_processing(None, &files, &source.to_string_lossy(), &output, Some(&cancel));

    assert!(
        outputs.is_empty(),
        "Cancelled notebook processing should return empty: got {:?}",
        outputs
    );
}

// (Pre-Track A: test_notebook_cancel_isolated_from_video_cancel asserted
// that VideoConversionState's `cancelled` AtomicBool was distinct from
// NotebookConversionState's. Both flags are gone — cancellation lives on
// FolderSession::cancel and is a single source of truth shared across all
// pipelines. The test is vacuous post-A2.)

// =========================================================================
// stage_copy / stage_write helper tests
// =========================================================================

#[test]
fn stage_copy_writes_the_file_and_returns_its_hash() {
    let temp = TempDir::new().unwrap();
    let source_file = temp.path().join("input.txt");
    let staging = temp.path().join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::write(&source_file, "hello").unwrap();

    let path = crate::build::served_path::ServedPath::from_source("sub/output.txt").unwrap();
    let hash = stage_copy(&source_file, &path, &staging).unwrap();

    assert_eq!(
        fs::read_to_string(staging.join("sub/output.txt")).unwrap(),
        "hello"
    );
    // The receipt is the manifest's hash of the bytes written — computed from
    // the SOURCE, so registration never depends on reading the stage back.
    assert_eq!(hash, crate::build::assets::paths::compute_binary_hash(b"hello"));
}

#[test]
fn stage_write_creates_parents_normalizes_dirs_and_returns_its_hash() {
    // Regression test for the chps-site bug: a source dir with capital case
    // (Resources/) emits to the lowercase form. Per the lowercase-only dir rule
    // (a21573395), directory segments are lowercased but the basename is
    // preserved verbatim so JupyterLite assets like `MathJax_Main-Bold.woff`
    // round-trip without rewriting.
    let temp = TempDir::new().unwrap();
    let staging = temp.path().join("staging");

    let path =
        crate::build::served_path::ServedPath::from_source("Resources/a/b/Habitable Zone.html")
            .unwrap();
    let hash = stage_write("<html>nb</html>", &path, &staging).unwrap();

    assert!(staging.join("resources/a/b/Habitable Zone.html").exists());
    assert_eq!(
        hash,
        crate::build::assets::paths::compute_binary_hash(b"<html>nb</html>")
    );
}

#[test]
fn test_copy_dir_recursive_copies_all_files() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");

    fs::create_dir_all(src.join("sub")).unwrap();
    fs::write(src.join("a.txt"), "content-a").unwrap();
    fs::write(src.join("sub/b.txt"), "content-b").unwrap();

    let mut receipts = copy_dir_recursive(&src, &dst).unwrap();
    receipts.sort();

    assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "content-a");
    assert_eq!(
        fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
        "content-b"
    );
    // The receipt names exactly what was written — no more (a walk of `dst`
    // would also find anything else that happens to be there) and no less —
    // and each hash is of the source bytes, so nothing has to read `dst`.
    assert_eq!(
        receipts,
        vec![
            (
                "a.txt".to_string(),
                crate::build::assets::paths::compute_binary_hash(b"content-a")
            ),
            (
                "sub/b.txt".to_string(),
                crate::build::assets::paths::compute_binary_hash(b"content-b")
            ),
        ]
    );
}

/// The incident this whole design exists for, at the seam it happened on.
///
/// `staging/jupyter/` held Dropbox conflicted-copy twins. Registration walked
/// that directory, so the twins entered the manifest as moss's own output; they
/// were cloud-evicted, and reading one back to hash it returned EDEADLK, which
/// classified as a fatal stop and killed every build of that vault
/// (the CPHS vault, 2026-08-30).
///
/// The receipt is what makes that unrepresentable: `copy_dir_recursive` reports
/// what it copied, so whatever else is sitting in the destination — a twin, a
/// 0-byte eviction stub, an unreadable file — is simply not in the list, and
/// nothing downstream ever opens the destination to find out. This asserts it
/// against a destination deliberately seeded with all three. It is written here
/// rather than through `run_notebook_processing` because that function needs a
/// resolved JupyterLite bundle to reach the copy at all, and without one it
/// takes a fallback branch on which the assertion would pass vacuously.
#[test]
fn receipts_never_name_a_file_the_build_did_not_write() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("bundle");
    let dst = temp.path().join("staging/jupyter");

    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("bootstrap.js"), "ours").unwrap();

    // Left in the destination by something other than this build.
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("index (Conflicted copy 2026-08-30).html"), "twin").unwrap();
    fs::write(dst.join("evicted-stub.js"), "").unwrap();
    let unreadable = dst.join("unreadable.js");
    fs::write(&unreadable, "secret").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    }

    let receipts = copy_dir_recursive(&src, &dst).expect("copy must not fail on foreign files");

    assert_eq!(
        receipts,
        vec![(
            "bootstrap.js".to_string(),
            crate::build::assets::paths::compute_binary_hash(b"ours")
        )],
        "only the file this build wrote may be named"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    }
}

// Regression: notebook outputs registered in site_hashes survive stale cleanup.
// JupyterLite assets and viewer HTML are registered by run_notebook_processing
// in site_hashes.files with actual SHA-256 hashes so that remove_stale_files
// doesn't delete them and deploy detects content changes.
#[test]
fn test_stale_cleanup_preserves_registered_notebook_outputs() {
    use sha2::{Digest, Sha256};
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("site");
    fs::create_dir_all(dir.join("jupyter")).unwrap();
    fs::write(dir.join("jupyter/config-utils.js"), "jl asset").unwrap();
    fs::write(dir.join("jupyter/index.html"), "<html>jl</html>").unwrap();
    fs::create_dir_all(dir.join("resources")).unwrap();
    fs::write(dir.join("resources/analysis.html"), "viewer").unwrap();
    fs::write(dir.join("stale-file.txt"), "should be removed").unwrap();
    fs::write(dir.join("style.css"), "kept").unwrap();

    // Compute real hashes matching what run_notebook_processing now does.
    let hash_of = |content: &[u8]| format!("{:x}", Sha256::digest(content));

    let mut hashes = SiteHashes::default();
    hashes.insert("style.css".to_string(), "hash".to_string());
    // Notebook processing registers these paths with actual SHA-256 hashes.
    hashes.insert("jupyter/config-utils.js".to_string(), hash_of(b"jl asset"));
    hashes.insert(
        "jupyter/index.html".to_string(),
        hash_of(b"<html>jl</html>"),
    );
    hashes.insert("resources/analysis.html".to_string(), hash_of(b"viewer"));

    remove_stale_files(&dir, &hashes, "test");

    // Registered notebook outputs survive
    assert!(
        dir.join("jupyter/config-utils.js").exists(),
        "Registered JupyterLite asset must survive"
    );
    assert!(
        dir.join("jupyter/index.html").exists(),
        "Registered JupyterLite index must survive"
    );
    assert!(
        dir.join("resources/analysis.html").exists(),
        "Registered viewer HTML must survive"
    );
    // Unregistered files are removed
    assert!(
        !dir.join("stale-file.txt").exists(),
        "Stale files must be removed"
    );
    // Tracked files survive
    assert!(dir.join("style.css").exists(), "Tracked files must survive");
}

// Regression: unregistered jupyter/ files ARE cleaned up (no blanket skip).
#[test]
fn test_stale_cleanup_removes_unregistered_jupyter_files() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("site");
    fs::create_dir_all(dir.join("jupyter")).unwrap();
    fs::write(dir.join("jupyter/stale-old-file.js"), "old").unwrap();

    let hashes = SiteHashes::default();
    remove_stale_files(&dir, &hashes, "test");

    assert!(
        !dir.join("jupyter/stale-old-file.js").exists(),
        "Unregistered jupyter/ files should be cleaned up"
    );
}

// =========================================================================
// .moss/ custom CSS/JS discovery tests
// =========================================================================

#[test]
fn test_user_css_from_moss_dir() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();
    let moss_dir = test_dir.join(".moss");
    fs::create_dir_all(moss_dir.join("theme")).unwrap();
    fs::write(
        moss_dir.join("theme").join("style.css"),
        "body { color: blue; }",
    )
    .unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // User CSS is now emitted with a content-hash filename; scan to find it.
    let theme_dir = test_dir.join(".moss/build/staging/_moss/theme");
    let hashed_css_path = fs::read_dir(&theme_dir)
        .unwrap()
        .flatten()
        .find(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.starts_with("style.") && s.ends_with(".css")
        })
        .map(|e| e.path())
        .expect("_moss/theme/style.<hash>.css must exist after build");
    let custom_css = fs::read_to_string(&hashed_css_path).unwrap();
    assert_eq!(custom_css, "body { color: blue; }");

    let html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        html.contains("_moss/theme/style."),
        "HTML should link to _moss/theme/style.<hash>.css when .moss/theme/style.css exists"
    );
}

#[test]
fn test_user_css_root_ignored() {
    // Root-level style.css is no longer picked up. Only .moss/theme/style.css
    // is canonical; check_misplaced_theme_files() in render.rs warns users
    // when style.css is found at the project root.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();
    fs::write(test_dir.join("style.css"), "body { color: red; }").unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    assert!(!test_dir.join(".moss/build/staging/_moss/theme/style.css").exists(),
            "Root style.css should NOT produce _moss/theme/style.css — only .moss/theme/style.css is canonical");
}

#[test]
fn test_no_user_css() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    assert!(!test_dir
        .join(".moss/build/staging/_moss/theme/style.css")
        .exists());
}

#[test]
fn test_theme_style_edit_regenerates_custom_css() {
    use std::thread::sleep;
    use std::time::Duration;

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Minimal site
    fs::create_dir_all(test_dir.join(".moss/theme")).unwrap();
    fs::write(test_dir.join("index.md"), "# Hello\n").unwrap();
    fs::write(
        test_dir.join(".moss/theme/style.css"),
        ":root { --moss-color-accent: red; }\n",
    )
    .unwrap();

    // Helper: collect content of all hashed CSS files in theme_dir.
    let theme_dir = test_dir.join(".moss/build/staging/_moss/theme");
    let read_all_hashed_css = |theme_dir: &std::path::Path| -> Vec<String> {
        fs::read_dir(theme_dir)
            .unwrap_or_else(|_| panic!("_moss/theme dir must exist"))
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.starts_with("style.") && s.ends_with(".css")
            })
            .map(|e| fs::read_to_string(e.path()).unwrap())
            .collect()
    };

    // First build — at least one style.<hash>.css should contain "red"
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();
    let v1_files = read_all_hashed_css(&theme_dir);
    assert!(
        !v1_files.is_empty(),
        "first build: _moss/theme/style.<hash>.css must exist"
    );
    assert!(
        v1_files.iter().any(|c| c.contains("red")),
        "first build: _moss/theme/style.<hash>.css should contain theme content; got: {:?}",
        v1_files
    );

    // Edit the theme only — bump mtime so any mtime-based dedup notices
    sleep(Duration::from_secs(1));
    fs::write(
        test_dir.join(".moss/theme/style.css"),
        ":root { --moss-color-accent: blue; }\n",
    )
    .unwrap();

    // Second build — a new style.<hash>.css must now contain "blue".
    // (Note: build_test doesn't run stale cleanup, so both hashes may coexist.)
    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();
    let v2_files = read_all_hashed_css(&theme_dir);
    assert!(
        v2_files.iter().any(|c| c.contains("blue")),
        "second build: a new style.<hash>.css should reflect the theme edit; got: {:?}",
        v2_files
    );
}

#[test]
fn test_user_js_from_moss_dir() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();
    let moss_dir = test_dir.join(".moss");
    fs::create_dir_all(moss_dir.join("theme")).unwrap();
    fs::write(
        moss_dir.join("theme").join("script.js"),
        "console.log('moss')",
    )
    .unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    // User JS is now emitted with a content-hash filename; scan to find it.
    let theme_dir = test_dir.join(".moss/build/staging/_moss/theme");
    let hashed_js_path = fs::read_dir(&theme_dir)
        .unwrap()
        .flatten()
        .find(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.starts_with("script.") && s.ends_with(".js")
        })
        .map(|e| e.path())
        .expect("_moss/theme/script.<hash>.js must exist after build");
    let custom_js = fs::read_to_string(&hashed_js_path).unwrap();
    assert_eq!(custom_js, "console.log('moss')");

    let html = fs::read_to_string(test_dir.join(".moss/build/staging/index.html")).unwrap();
    assert!(
        html.contains("_moss/theme/script."),
        "HTML should include _moss/theme/script.<hash>.js when .moss/theme/script.js exists"
    );
}

#[test]
fn test_user_js_root_ignored() {
    // Root-level script.js is no longer picked up. Only .moss/theme/script.js
    // is canonical; check_misplaced_theme_files() in render.rs warns users
    // when script.js is found at the project root.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Hello").unwrap();
    fs::write(test_dir.join("script.js"), "console.log('root')").unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    assert!(!test_dir.join(".moss/build/staging/_moss/theme/script.js").exists(),
            "Root script.js should NOT produce _moss/theme/script.js — only .moss/theme/script.js is canonical");
}

// =========================================================================
// .moss/assets/ copy tests
// =========================================================================

#[test]
fn test_moss_assets_copied_to_output() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = source.join(".moss");

    fs::create_dir_all(moss.join("theme")).unwrap();
    fs::write(moss.join("theme").join("favicon.svg"), "<svg/>").unwrap();
    fs::write(moss.join("theme").join("logo.png"), b"fake png").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output.join("_moss/theme/favicon.svg").exists(),
        ".moss/theme/favicon.svg should be copied to _moss/theme/favicon.svg"
    );
    assert!(
        output.join("_moss/theme/logo.png").exists(),
        ".moss/theme/logo.png should be copied to _moss/theme/logo.png"
    );
}

#[test]
fn test_moss_assets_nested_directories() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = source.join(".moss");

    fs::create_dir_all(moss.join("theme").join("fonts")).unwrap();
    fs::write(
        moss.join("theme").join("fonts").join("inter.woff2"),
        b"fake font",
    )
    .unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    assert!(
        output
            .join("_moss")
            .join("theme")
            .join("fonts")
            .join("inter.woff2")
            .exists(),
        ".moss/theme/fonts/inter.woff2 should appear at _moss/theme/fonts/inter.woff2 in output"
    );
}

/// Regression test: theme assets (video overlays, textures) must appear in output.
///
/// This was the original bug: `leaves.mp4` placed in `.moss/` root was not
/// picked up because the pipeline only walked `.moss/assets/` (now `.moss/theme/`).
#[test]
fn test_theme_video_overlay_copied_to_output() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = source.join(".moss");

    // Place video overlay and texture in theme/
    fs::create_dir_all(moss.join("theme")).unwrap();
    fs::write(moss.join("theme").join("leaves.mp4"), b"fake video overlay").unwrap();
    fs::write(moss.join("theme").join("grain.png"), b"fake texture").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(
        &ctx,
        crate::build::ports::reporter::discarding(), tx, None);
    }

    // Theme assets land under _moss/theme/ in the one output directory.
    assert!(
        output.join("_moss/theme/leaves.mp4").exists(),
        "leaves.mp4 must be copied to _moss/theme/ in output"
    );
    assert!(
        output.join("_moss/theme/grain.png").exists(),
        "grain.png must be copied to _moss/theme/ in output"
    );
}

/// Regression test: theme walk skips the entry files (style.css/script.js emitted by blocking phase)
/// but copies all other assets through verbatim under _moss/theme/.
#[test]
fn test_theme_walk_skips_entry_files_copies_rest() {
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = source.join(".moss");

    fs::create_dir_all(moss.join("theme")).unwrap();
    fs::write(moss.join("theme").join("style.css"), "body { color: red }").unwrap();
    fs::write(moss.join("theme").join("custom.css"), ".custom {}").unwrap();
    fs::write(moss.join("theme").join("custom.js"), "console.log('hi')").unwrap();
    fs::write(moss.join("theme").join("allowed.svg"), "<svg/>").unwrap();

    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss,
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    {
        let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    }

    // style.css is still skipped by name (the blocking phase emits it; the
    // asset walk does not). With copy_deferred_assets run alone here, it is
    // simply absent from the output.
    assert!(
        !output.join("_moss/theme/style.css").exists(),
        "style.css is emitted by the blocking phase, not the asset walk"
    );
    // custom.css / custom.js are no longer reserved names — under the
    // verbatim /_moss/theme/ mirror they copy through like any other asset.
    assert!(
        output.join("_moss/theme/custom.css").exists(),
        ".moss/theme/custom.css copies through to _moss/theme/custom.css"
    );
    assert!(
        output.join("_moss/theme/custom.js").exists(),
        ".moss/theme/custom.js copies through to _moss/theme/custom.js"
    );
    // Other files copy under _moss/theme/.
    assert!(
        output.join("_moss/theme/allowed.svg").exists(),
        "allowed.svg should be copied to _moss/theme/"
    );
}

/// Regression test: notebook outputs are registered with real SHA-256 hashes, not sentinel values.
///
/// Before the fix, notebook outputs were registered with the constant string
/// "notebook-generated" regardless of file content, preventing the deploy
/// manifest differ from detecting actual content changes.
#[test]
fn test_notebook_outputs_get_real_hashes() {
    use sha2::{Digest, Sha256};
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let site_dir = dir.path();

    // Create fake notebook output files (simulating what the notebook plugin produces)
    let files: Vec<(&str, &[u8])> = vec![
        ("resources/test.ipynb", b"notebook content"),
        ("jupyter/files/test.ipynb", b"notebook content"),
        ("resources/test.html", b"<html>viewer</html>"),
    ];

    let mut site_hashes = SiteHashes::default();

    for (path, content) in &files {
        let full_path = site_dir.join(path);
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        fs::write(&full_path, content).unwrap();

        // Compute hash the same way as the fixed code path
        let bytes = fs::read(&full_path).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        site_hashes.insert(path.to_string(), hash);
    }

    // Verify all hashes are real SHA-256 (64 lowercase hex characters)
    for (path, _) in &files {
        let hash = site_hashes
            .files
            .get(*path)
            .unwrap_or_else(|| panic!("missing hash for {}", path));
        assert_eq!(
            hash.len(),
            64,
            "Hash for {} should be 64 hex chars, got {}",
            path,
            hash.len()
        );
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash for {} should be all hex digits, got: {}",
            path,
            hash
        );
        assert_ne!(
            hash.as_str(),
            "notebook-generated",
            "Hash for {} must not be the sentinel value",
            path
        );
    }

    // Verify hash matches the actual file content deterministically
    let expected_notebook_hash = format!("{:x}", Sha256::digest(b"notebook content"));
    assert_eq!(
        site_hashes.files.get("resources/test.ipynb").unwrap(),
        &expected_notebook_hash,
        "Hash must match SHA-256 of actual file bytes"
    );
    assert_eq!(
        site_hashes.files.get("jupyter/files/test.ipynb").unwrap(),
        &expected_notebook_hash,
        "Identical content must produce identical hash"
    );

    let expected_html_hash = format!("{:x}", Sha256::digest(b"<html>viewer</html>"));
    assert_eq!(
        site_hashes.files.get("resources/test.html").unwrap(),
        &expected_html_hash,
        "HTML viewer hash must match SHA-256 of its content"
    );
}

/// Regression test: different notebook content produces different hashes.
///
/// With a sentinel value ("notebook-generated"), all notebook outputs looked
/// identical to the deploy differ — content changes were invisible. Real
/// SHA-256 hashes must be unique for distinct content.
#[test]
fn test_notebook_hash_changes_with_content() {
    use sha2::{Digest, Sha256};

    let content_v1 = b"version 1 content";
    let content_v2 = b"version 2 content";

    let hash_v1 = format!("{:x}", Sha256::digest(content_v1));
    let hash_v2 = format!("{:x}", Sha256::digest(content_v2));

    // Both hashes must be well-formed SHA-256 hex strings
    assert_eq!(hash_v1.len(), 64, "SHA-256 hex must be 64 chars");
    assert_eq!(hash_v2.len(), 64, "SHA-256 hex must be 64 chars");
    assert!(hash_v1.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(hash_v2.chars().all(|c| c.is_ascii_hexdigit()));

    // Different content must yield different hashes (deploy differ can detect changes)
    assert_ne!(
            hash_v1,
            hash_v2,
            "Different notebook content must produce different hashes so the deploy differ can detect updates"
        );

    // Same content must yield the same hash (idempotent rebuilds don't re-upload unchanged files)
    let hash_v1_again = format!("{:x}", Sha256::digest(content_v1));
    assert_eq!(
        hash_v1, hash_v1_again,
        "Same content must always produce the same hash"
    );
}

// =========================================================================
// Task 5: Image Compression Pipeline — End-to-End Integration Test
// =========================================================================
//
// Exercises the full `build()` entry point for a project that contains real
// JPEGs, verifying the seams across scan → render → dispatch → runner →
// HTML rewrite. Unit coverage for edge cases (EXIF, sentinel, skip rules,
// ancestor check, dir_overrides, cancel isolation, stale-cleanup) lives in
// image.rs / generator/placeholder.rs / media/pipeline.rs. This test keeps
// the integration story honest: a JPEG on disk really does produce a
// smaller .webp, the HTML really does get wrapped in <picture>, and
// hashes.json really does list the WebP under `image_outputs`.

/// Build a JPEG large enough to clear `ImageCompressionConfig::min_size_kb`
/// (default 200 KB) so the scan pipeline doesn't skip it with
/// `SkipReason::AlreadySmall`. Dimensions + modest pixel variation keep
/// it well above the threshold while still compressing better as WebP
/// than as JPEG, so the conversion produces a real (smaller) .webp.
fn make_big_jpeg_at(path: &std::path::Path, w: u32, h: u32) {
    use image::{DynamicImage, ImageBuffer, Rgb};
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
        // Smooth gradient — compresses well in both formats but WebP wins
        // at this size + default quality.
        Rgb([(x as u8).wrapping_add(y as u8), (x / 7 + y / 11) as u8, 50])
    });
    let img = DynamicImage::ImageRgb8(buf);
    img.save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

#[test]
fn test_image_compression_pipeline_end_to_end() {
    // Full pipeline:
    //   scan → render (HTML + image_items + webp_variant on MediaMetadata)
    //        → dispatch_image_conversions (runs synchronously in headless)
    //          ↳ run_image_conversion writes .webp + updates hashes.json
    //        → dispatch_background_assets (runs synchronously in headless)
    //          ↳ copies non-markdown assets (svg passthrough)
    //
    // `compute_webp_variant_for_scan` gates `<picture>` emission on
    // transform-cache presence (a 404'd <source> is not browser-
    // recoverable, see media/image.rs). Encoding lands during build 1
    // but scan reads the cache *before* that — so build 1 emits a bare
    // `<img src="photo.jpg">` even though `photo.webp` does end up on
    // disk by the time build 1 returns. Build 2 sees the cache hit and
    // upgrades the HTML to `<picture><source srcset="photo.webp">`.
    //
    // Assertions prove the seams are wired correctly:
    //   1. JPEGs on disk become .webp in .moss/build/staging/ (sole output post-T2)
    //   2. .webp is smaller than the source JPEG (real encoding happened)
    //   3. SVG passes through unchanged (skip rules honoured)
    //   4. After build 2 — Built HTML wraps the markdown <img> in
    //      <picture> with <source srcset="X.webp" type="image/webp">
    //   5. hashes.json lists the .webp outputs under `image_outputs`
    //      (protects freshly-produced WebPs from the next rebuild's
    //      stale-cleanup pass).

    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // index.md references two images — one via markdown syntax, one via
    // raw <img> tag. Both should end up wrapped in <picture>.
    fs::write(
        test_dir.join("index.md"),
        "# Hero\n\n\
             Markdown image: ![Alt text](photo.jpg)\n\n\
             HTML image: <img src=\"photo2.jpg\" alt=\"second\">\n\n\
             Icon: ![logo](skip.svg)\n",
    )
    .unwrap();

    // Real JPEGs — dimensions chosen to clear the default
    // `ImageCompressionConfig::min_size_kb` threshold (200 KB) so
    // `collect_images_for_conversion` picks them up instead of skipping
    // with `SkipReason::AlreadySmall`. Smaller fixtures pass through the
    // pipeline without producing a .webp and the seam test cannot verify
    // the conversion.
    make_big_jpeg_at(&test_dir.join("photo.jpg"), 2400, 1800);
    make_big_jpeg_at(&test_dir.join("photo2.jpg"), 2400, 1800);

    // A tiny SVG — passes through because the dispatcher only considers
    // raster formats that benefit from WebP.
    fs::write(
            test_dir.join("skip.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#,
        )
        .unwrap();

    // Headless services → image/asset dispatch runs synchronously, so by
    // the time build() returns everything is on disk.
    let services = BuildServices::headless();
    let result = build_test(
        folder_path,
        None,
        None,
        None,
        Some(&services),
        &ResolvedSlots::empty(),
    );
    assert!(result.is_ok(), "Build 1 should succeed: {:?}", result);

    // ---- 1. WebP outputs exist (in staging/ — the sole build output post-T2) ----
    let staging_dir = test_dir.join(".moss/build/staging");
    let photo_webp = staging_dir.join("photo.webp");
    let photo2_webp = staging_dir.join("photo2.webp");
    assert!(
        photo_webp.exists(),
        "photo.webp should exist in staging/ after build 1, got files: {:?}",
        fs::read_dir(&staging_dir).ok().map(|d| d
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>())
    );
    assert!(
        photo2_webp.exists(),
        "photo2.webp should exist in staging/ after build 1"
    );

    // Build 2 — scan now sees the cached WebP, so the synthesizer
    // upgrades the markdown <img> to <picture>. Re-uses the same dir,
    // mimicking a normal incremental rebuild.
    let result2 = build_test(
        folder_path,
        None,
        None,
        None,
        Some(&services),
        &ResolvedSlots::empty(),
    );
    assert!(result2.is_ok(), "Build 2 should succeed: {:?}", result2);

    // ---- 2. .webp must be smaller than source JPEG (real encoding) ----
    let jpeg_size = fs::metadata(test_dir.join("photo.jpg")).unwrap().len();
    let webp_size = fs::metadata(&photo_webp).unwrap().len();
    assert!(
        webp_size < jpeg_size,
        "photo.webp ({} bytes) should be smaller than photo.jpg ({} bytes)",
        webp_size,
        jpeg_size
    );

    // ---- 3. SVG passes through to staging/ ----
    assert!(
        staging_dir.join("skip.svg").exists(),
        "skip.svg should pass through to staging/"
    );

    // ---- 4. After build 2 — markdown-syntax image gets <picture>;
    //         raw HTML <img> does not (Step 7 contract) ----
    //
    // Post-Step-7 the structural <picture> wrap is emitted at parse
    // time by `image_render::synthesize_image_html` when the variant
    // manifest reports a cached WebP companion. Cold-build scan sees
    // an empty transform cache and the synthesizer skips the wrap
    // (avoids a 404'd <source>); the warm-build scan (this is run 2)
    // sees the cache hit and wraps. Raw HTML `<img>` baked into the
    // markdown source bypasses every typed layer (pulldown-cmark
    // passes it through as opaque Event::Html bytes) and intentionally
    // falls into the bare-img carve-out: it gets attribute injection
    // from the surviving regex pass but not a <picture> wrap. This
    // matches the carve-out list at `image_render.rs:56-93` (site
    // logo, RSS pixel, email body, and raw-HTML `<img>` in markdown
    // source). The review colophon joined the synthesizer in
    // 2026-05-16.
    let html = fs::read_to_string(staging_dir.join("index.html")).expect("index.html should exist");
    assert!(
        html.contains("<picture>"),
        "Markdown-syntax image must emit <picture> via synthesizer, got: {}",
        html
    );
    // 2400×1800 fixture → srcset ladder (responsive-image-variants
    // Task 3): rungs below the deployed base (2400) + base descriptor,
    // content-column sizes for an inline markdown image. Rung ENCODES
    // land in Tasks 4-5; this seam test only pins the emitted HTML +
    // the base webp on disk.
    assert!(
            html.contains(
                r#"<source srcset="/photo.w800.webp 800w, /photo.w1600.webp 1600w, /photo.webp 2400w" type="image/webp" sizes="(min-width: 48rem) 47.25rem, 100vw">"#
            ),
            "Built HTML should contain <source> ladder for photo.webp (markdown image), got: {}",
            html
        );
    // photo2.jpg comes from a raw HTML <img> tag in markdown source —
    // post-Step-7, no <picture> wrap. The <img> still gets attribute
    // injection (loading="lazy", dims, data-placeholder-src) via the
    // surviving regex pass.
    assert!(
        html.contains(r#"<img src="photo2.jpg""#),
        "Raw HTML <img> for photo2.jpg should survive, got: {}",
        html
    );
    // data-placeholder-src removed 2026-05-20 from both the synthesizer
    // and the regex pass. iframe-bridge matches by URL substring now;
    // AssetRegistry + preview server handle the placeholder lifecycle.
    // See docs/archive/2026-05-20-image-variant-honest-mirror.md.
    assert!(
        !html.contains("data-placeholder-src"),
        "data-placeholder-src must not be emitted post-2026-05-20, got: {}",
        html
    );
    // Exactly one <picture> element (for the markdown-syntax image only).
    assert_eq!(
        html.matches("<picture>").count(),
        1,
        "Expected exactly one <picture> wrapper (markdown image, not raw HTML), got {} in: {}",
        html.matches("<picture>").count(),
        html
    );

    // ---- 5. hashes.json registers both .webp outputs ----
    let hashes = load_previous_hashes(folder_path);
    assert!(
        hashes.image_outputs.contains("photo.webp"),
        "hashes.json::image_outputs should contain photo.webp, got: {:?}",
        hashes.image_outputs
    );
    assert!(
        hashes.image_outputs.contains("photo2.webp"),
        "hashes.json::image_outputs should contain photo2.webp, got: {:?}",
        hashes.image_outputs
    );

    // Task tracker must return to zero after headless conversion.
    // (In headless mode, has_ui_bound() is always false because no
    // session is attached. Asserting it stays false confirms no
    // panic / inconsistent state during conversion.)
    assert!(
        !services.has_ui_bound(),
        "ui_bound counter must drain after headless build"
    );
}

/// Regression: a no-edit republish must hit the source-metadata cache
/// for every file. If a future refactor accidentally removes the
/// `sources` write-back or the `check_source_cache` call, this test
/// fails.
///
/// The win it locks in: on the William Blake recordings site this took
/// `copy_deferred_assets` from ~35s to ~1s. Without this assertion any
/// well-meaning cleanup that drops the `sources` map writes would
/// silently regress to the slow path.
///
/// Pre-#620 Item 2: this test ran two `copy_deferred_assets(... None)`
/// calls and roundtripped the persisted `hashes.json` between them.
/// Post-#620 Item 2: the on-disk fallback is gone. The test now seals
/// the run-1 manifest from the coordinator, then feeds the sealed
/// `SiteHashes` as `previous_hashes` to run 2 — mirroring the
/// production flow where `seal+persist` writes to disk and the next
/// build's `load_previous_hashes` reads it back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_warm_cache_zero_misses() {
    use crate::build::media::pipeline::copy_deferred_assets;
    use crate::build::coordinator::test_utils;
    use std::collections::HashSet;

    let temp = TempDir::new().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    let moss = temp.path().join(".moss");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::create_dir_all(moss.join("build").join("cache").join("objects")).unwrap();

    // Two assets covering different MIME paths through the walker.
    std::fs::write(
        source.join("photo.jpg"),
        b"fake jpg bytes -- content does not matter",
    )
    .unwrap();
    std::fs::write(source.join("script.user.js"), b"console.log('hi')").unwrap();
    // Sleep past RACY_MTIME_EPSILON_SECS (1s) so source mtimes are strictly
    // older than the moment we capture the manifest's source-cache state.
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    let deferred1 = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: output.clone(),
        moss_dir: moss.clone(),
        blocking_keys: HashSet::new(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    // Run 1 — cold cache, every file should miss + populate `sources`.
    // Only the cold half is asserted here. The warm half needs run 2's
    // `previous_hashes.sources` to hold run 1's entries, which is a full
    // seal → persist → load round-trip; that lives in `tests/`.
    let (tx1, _rx1) = test_utils::build_test_coordinator();
    // copy_deferred_assets uses `tx.blocking_send` which cannot run on a
    // tokio worker thread; spawn_blocking moves it to a dedicated blocking
    // pool thread.
    let stats1 = tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&deferred1, crate::build::ports::reporter::discarding(), tx1, None)
    })
    .await
    .unwrap();
    assert_eq!(stats1.cache_hits, 0, "first run must have no cache hits");
    assert!(
        stats1.cache_misses >= 2,
        "first run must miss every asset (got {} misses)",
        stats1.cache_misses,
    );
}

// =========================================================================
// Regression: text-only sites must produce a sealed manifest
// =========================================================================

/// Regression guard for the deploy hard-error on text-only (no-media) sites.
///
/// When `has_background_work == false`, the old code did `drop(pending); None`,
/// leaving `bg_handle = None`.  The seal+persist side task in `build.rs` only
/// runs inside `if let Some(handle) = _bg_handle { ... }`, so
/// `current_sealed_manifest` was never populated → deploy hard-errored with
/// "Deploy artifacts are not sealed yet."
///
/// The first fix added an `else` branch making a zero-worker handle. moss#618
/// removed the gate instead: every build now dispatches the asset walk, so
/// there is always a worker and always a seal, and the branch this test was
/// written against no longer exists. The assertion is kept because it states
/// the guarantee deploy depends on — a markdown-only folder still seals.
#[tokio::test]
async fn text_only_build_returns_bg_handle_for_deploy_seal() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    // Text-only site: one markdown file, no images, no videos, no deferred assets.
    fs::write(
        test_dir.join("index.md"),
        "# Hello\nText-only content, no media.",
    )
    .unwrap();

    let ps = scan_folder(folder_path).expect("scan_folder failed");
    let PipelineRunOutput { bg_handle, .. } = run(
        &crate::vault::paths::VaultRoot::resolve(folder_path),
        None,
        None,
        &crate::build::ports::port_of_this_build(None, None),
        None,
        Some(Box::new(|_, _, _| Ok(ResolvedSlots::empty()))),
        &ps,
        None,
        crate::build::render::IncrementalGates::default(),
        crate::build::feeds::search_lane::Freshness::Now,
        &test_cache_keys(),
    )
    .expect("run() failed");

    assert!(
        bg_handle.is_some(),
        "text-only build must return Some(BackgroundHandle) so the seal+persist \
             side task can populate current_sealed_manifest; deploy hard-errors when None"
    );

    // Also verify the handle actually seals cleanly (zero workers → immediate drain).
    let sealed = bg_handle
        .unwrap()
        .await_completion()
        .await
        .expect("await_completion failed on zero-worker handle");

    // A text-only build has no deferred assets, but the render-phase pages
    // (index.html) must be present in the sealed manifest's files map.
    assert!(
        !sealed.files().is_empty(),
        "sealed manifest must contain at least index.html from the render phase"
    );
}

/// `build_test`, plus the seal-time cleanup production runs in
/// `advertise_sealed` step 4 (`build.rs`) — `remove_stale_files` against the
/// sealed manifest.
///
/// It exists because the difference is not cosmetic. `build_test` stops at
/// `await_completion`, so a test that only checks the staging directory after it
/// cannot see the three passes that actually decide a page's fate:
/// `stale_carried_forward`, `PendingManifest::seal`, and `remove_stale_files`.
/// The eviction test below passed for exactly that reason while production was
/// deleting the page — and, because `sealed.files()` is the deploy manifest,
/// un-publishing it. Returns the sealed manifest's keys so a test can assert on
/// the wire manifest, not just on disk.
fn build_test_sealed(folder_path: &str) -> Result<Vec<String>, String> {
    let ps = scan_folder(folder_path)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("test runtime build failed: {}", e))?;
    let root = crate::vault::paths::VaultRoot::resolve(folder_path);
    let preview_port = crate::build::ports::port_of_this_build(None, None);
    rt.block_on(async move {
        let PipelineRunOutput { bg_handle, .. } = run(
            &root,
            None,
            None,
            &preview_port,
            None,
            Some(Box::new(move |_, _, _| Ok(ResolvedSlots::empty()))),
            &ps,
            None,
            crate::build::render::IncrementalGates::default(),
            crate::build::feeds::search_lane::Freshness::Now,
            &test_cache_keys(),
        )
        .map_err(crate::build::outcome::BuildStopped::into_message)?;
        let Some(handle) = bg_handle else { return Ok(Vec::new()) };
        let sealed = handle
            .await_completion()
            .await
            .map_err(|e| format!("test seal+persist failed: {}", e))?;
        let mp = crate::moss_paths::MossPaths::new(root.path());
        let _ = sealed.write_to_disk(&mp.hashes());
        let stage_dir = mp.staging_dir();
        let view = sealed.site_hashes_view();
        let mut keys: Vec<String> = view.files.keys().cloned().collect();
        keys.sort();
        crate::build::media::pipeline::remove_stale_files(&stage_dir, view, "staging");
        Ok(keys)
    })
}

/// Drive the full production SHIP TAIL, not just the build: seal → prune
/// orphaned `.webp` → reclaim what the prune orphaned → write `hashes.json`
/// → promote. Mirrors `build.rs`'s `exits_after_build` branch, which is the
/// only place all of these run in order — this harness has no next build in
/// its own process either, so it calls `ship::reclaim_staging_now` exactly
/// where that branch does (moss#… the 2026-09-16 regression: without this
/// call a single call here left an orphan's `.webp` sitting in staging, which
/// every assertion below would have missed since none of them ran a SECOND
/// build to let `pipeline::sweep_staging` cover for it).
///
/// `build_test` and `build_test_sealed` both stop at the seal, so neither can
/// observe anything the tail decides — and the tail is where the staging tree
/// is complete, where the reference set is therefore complete, and where the
/// prune runs. Returns the keys the prune condemned so a caller can assert a
/// converged build condemns nothing.
///
/// It promotes, too, for the multi-call case: a caller that runs this
/// harness again on the same folder still exercises `pipeline::sweep_staging`
/// at that next build's start, reading the `hashes.json` this call just
/// wrote — which needs a promoted generation to be the one the server is
/// considered "on" (see `sweep_staging`'s `served_from_current` doc).
fn build_test_shipped(
    folder_path: &str,
) -> Result<std::collections::HashSet<String>, String> {
    let ps = scan_folder(folder_path)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("test runtime build failed: {}", e))?;
    let root = crate::vault::paths::VaultRoot::resolve(folder_path);
    let preview_port = crate::build::ports::port_of_this_build(None, None);
    // Headless services, not `None`: with no services at all the media
    // dispatch never runs, so no `.webp` is ever produced and every
    // assertion about the image lifecycle passes vacuously. Headless also
    // runs the conversion synchronously, which is what makes the tail
    // deterministic.
    let services = BuildServices::headless();
    rt.block_on(async move {
        let PipelineRunOutput { bg_handle, .. } = run(
            &root,
            None,
            None,
            &preview_port,
            Some(&services),
            Some(Box::new(move |_, _, _| Ok(ResolvedSlots::empty()))),
            &ps,
            None,
            crate::build::render::IncrementalGates::default(),
            crate::build::feeds::search_lane::Freshness::Now,
            &test_cache_keys(),
        )
        .map_err(crate::build::outcome::BuildStopped::into_message)?;
        let Some(handle) = bg_handle else {
            return Ok(std::collections::HashSet::new());
        };
        let mut sealed = handle
            .await_completion()
            .await
            .map_err(|e| format!("test seal+persist failed: {}", e))?;
        let mp = crate::moss_paths::MossPaths::new(root.path());
        let stage_dir = mp.staging_dir();
        let scan = crate::build::media::orphan_prune::extract_referenced_tails(&stage_dir);
        let pruned =
            crate::build::ship::prune_orphaned_webp_before_ship(&mp, &mut sealed, &scan);
        // Mirrors `advertise_sealed`'s `reclaim_stage_dir_when_done = true`
        // arm: this harness never has a next build in the same process
        // either, so without this call staging would hold orphaned bytes no
        // test here could ever observe going away.
        crate::build::ship::reclaim_staging_now(&stage_dir, &sealed);
        let _ = sealed.write_to_disk(&mp.hashes());
        crate::build::ship::materialize_and_promote(
            &sealed,
            &mp,
            &stage_dir,
            None,
            crate::build::ship::next_promotion_epoch(),
            true,
        )?;
        Ok(pruned)
    })
}

/// Every file under `dir`, as relative path → content hash. The instrument a
/// null-build assertion needs: "did anything move" over a whole tree, in a
/// form whose failure message names the file.
fn stage_snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<String, u64> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(rd) = fs::read_dir(&next) else { continue };
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = fs::read(&path) {
                let rel = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().to_string();
                out.insert(rel, xxhash_rust::xxh3::xxh3_64(&bytes));
            }
        }
    }
    out
}

/// A build that changes nothing must DO nothing.
///
/// The general shape of moss#1085: one part of the build derives a set by
/// walking the disk, another derives "the same" set by reading what the render
/// referenced, the two disagree, and every build redoes work that can never
/// settle. On harbor that ran for sixteen consecutive builds — 96 `.webp`
/// files re-materialized from the CAS and the same 96 deleted again, 3.5 MB
/// each pass, 1–2 s on the critical path between `seal+persist` and the prune
/// finishing.
///
/// **Byte-identity alone cannot catch it**, which is why this test asserts a
/// counter as well. Heal-then-prune is self-cancelling: whether the two halves
/// agree or fight, staging ends each build holding exactly the referenced set,
/// so consecutive snapshots match either way. The disagreement is only visible
/// as work — a converged build's prune removes zero files.
///
/// `orphan.jpg` is the fixture that makes the two sets differ at all: a real
/// image in the vault that no page references. Without it both derivations
/// return the same thing and the test proves nothing.
///
/// Builds three times deliberately. Build 1 encodes with a cold transform
/// cache, so `compute_webp_variant_for_scan` cannot yet emit `<picture>` and
/// the HTML references no `.webp` at all; build 2 is the first with a
/// steady-state reference set. Convergence is a claim about builds 2→3.
///
/// A fourth build then makes the opposite claim on the same fixture: the
/// suppression that produced convergence must lift the moment a page points
/// at the image, or the fix trades a wasteful loop for a 404.
#[test]
fn a_build_that_changes_nothing_does_nothing() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(
        test_dir.join("index.md"),
        "---\ntitle: Hero\ndate: 2026-01-02\n---\n\n\
         # Hero\n\nReferenced: ![Alt](photo.jpg)\n\n\
         ```rust\nfn main() { println!(\"hi\"); }\n```\n",
    )
    .unwrap();
    make_big_jpeg_at(&test_dir.join("photo.jpg"), 2400, 1800);
    // Referenced by nothing. This is the whole point of the fixture.
    make_big_jpeg_at(&test_dir.join("orphan.jpg"), 2400, 1800);

    // A second page one directory down, with its own asset and a passthrough
    // file beside it. The width is the point: this stops being a regression
    // test for the one instance that earned it and becomes a detector for the
    // class. Every producer the fixture reaches — nested output dir, asset
    // copy, syntax highlighting, feed, sitemap — now has to agree with itself
    // across two builds, or the byte-identity assertion below names the file
    // that moved and hands over the next instance for free.
    fs::create_dir_all(test_dir.join("posts/assets")).unwrap();
    fs::write(
        test_dir.join("posts/one.md"),
        "---\ntitle: One\ndate: 2026-01-03\n---\n\n# One\n\n![Inline](assets/inline.jpg)\n",
    )
    .unwrap();
    make_big_jpeg_at(&test_dir.join("posts/assets/inline.jpg"), 2400, 1800);
    fs::write(
        test_dir.join("posts/assets/diagram.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 4 4\">\
         <rect width=\"4\" height=\"4\"/></svg>",
    )
    .unwrap();

    // The colocated case from moss#1085, stated rather than stumbled into. On
    // harbor that was `0ba912901f865c42.jpg` colocated in
    // `awards/film/s1/assets/` and `awards/film/s2/assets/`: one referrer, two
    // staged copies. `make_big_jpeg_at` is deterministic, so this file is
    // byte-identical to `posts/assets/inline.jpg` and the two share ONE cache
    // object while only one of them is referenced — which is what `orphan.jpg`
    // above cannot express. Suppression is keyed by STAGING PATH, not by
    // object, and this is the fixture that can tell the difference: an
    // implementation keyed by content hash would suppress the referenced twin
    // too, and only the pair of assertions below would notice.
    fs::create_dir_all(test_dir.join("gallery/assets")).unwrap();
    make_big_jpeg_at(&test_dir.join("gallery/assets/inline.jpg"), 2400, 1800);

    build_test_shipped(folder_path).expect("build 1");
    build_test_shipped(folder_path).expect("build 2");

    let stage_dir = test_dir.join(".moss/build/staging");
    let before = stage_snapshot(&stage_dir);

    // Guard the fixture itself. Everything below is a claim about `.webp`
    // lifecycle, so a fixture that quietly stopped producing any — a raised
    // `min_size_kb`, a changed skip rule — would turn all of it green while
    // testing nothing.
    assert!(
        before.keys().any(|k| k.ends_with(".webp")),
        "fixture produced no .webp variants at all; staging holds {:?}",
        before.keys().collect::<Vec<_>>()
    );

    let pruned = build_test_shipped(folder_path).expect("build 3");
    let after = stage_snapshot(&stage_dir);

    assert!(
        pruned.is_empty(),
        "a converged build must condemn nothing — {pruned:?} means something \
         re-materialized variants the previous build had already dropped \
         (moss#1085)"
    );

    let changed: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(|k| before.get(*k) != after.get(*k))
        .collect();
    assert!(
        changed.is_empty(),
        "a rebuild with no input change must leave staging byte-identical; these moved: {changed:?}"
    );

    // Convergence must be reached by keeping the right file, not by dropping
    // both: the referenced copy of the shared object ships, the colocated
    // copy nobody points at does not.
    assert!(
        stage_dir.join("posts/assets/inline.webp").is_file(),
        "the referenced copy of the shared object must survive the prune; staging holds {:?}",
        after.keys().collect::<Vec<_>>()
    );
    assert!(
        !stage_dir.join("gallery/assets/inline.webp").exists(),
        "the colocated copy nobody references must stay gone, not be re-healed \
         from the object its referenced twin keeps alive (moss#1085)"
    );

    // ---- and suppression is a latch, not a one-way door ----------------
    //
    // Declining to re-make a variant the last complete scan judged
    // unreferenced is only safe if pointing a page at that image brings it
    // straight back. Otherwise the build ships a `<source srcset>` for a file
    // it deliberately did not write, and `<picture>` has no fallback from a
    // chosen source that 404s (ADR-013). The escape hatch is unit-tested on
    // `suppressed_variants` alone; only here does the whole chain have to
    // agree — persisted verdict → suppression lifted by HTML already staged
    // this build → encode → the offered URL naming real bytes.
    //
    // It continues this fixture rather than standing up its own: the
    // colocated copy above is already in exactly the state the transition
    // needs (unreferenced, pruned, verdict persisted), and a second fixture
    // would pay three more full pipeline builds to reach it.
    fs::write(
        test_dir.join("gallery/index.md"),
        "---\ntitle: Gallery\ndate: 2026-01-04\n---\n\n# Gallery\n\n![Late](assets/inline.jpg)\n",
    )
    .unwrap();
    build_test_shipped(folder_path).expect("build 4");

    // Anti-vacuity, and the link between the two halves: the URL the page
    // offers must name the file asserted below. moss emits site-root-absolute
    // `<source>` URLs, so the promised URL and the staging key differ only by
    // the leading `/`. Checking the two halves independently would pass even
    // if the page pointed somewhere else entirely.
    let page = fs::read_to_string(stage_dir.join("gallery/index.html"))
        .expect("the new page must be staged");
    assert!(
        page.contains("/gallery/assets/inline.webp"),
        "the new page offers no <source> naming exactly the file the assertion \
         below checks — a looser substring would also match the OTHER staged \
         copy of this same image, and the assertion would pass vacuously"
    );
    assert!(
        stage_dir.join("gallery/assets/inline.webp").is_file(),
        "a suppressed variant a page now references must be produced again on \
         that same build, not one build later — the page already promises it, \
         and <picture> does not recover from a chosen source that 404s (ADR-013)"
    );
}

/// A one-shot build's own orphan prune must remove the orphaned `.webp`
/// BYTES from `stage_dir`, not just its `sealed` entry.
///
/// The 2026-09-16 regression this test would have caught: `f003326` moved the
/// physical unlink out of `prune_orphaned_webp_before_ship` (mid-flight 404s
/// under a live preview server — a real bug, correctly fixed) and deferred it
/// to `pipeline::sweep_staging`, which only runs at the START of a NEXT
/// build. `build_test_shipped` here, a real `moss build`, and every
/// moss-desktop snapshot-test fixture are all one-shot: the process exits
/// after this one build, so a next build that would do the sweeping never
/// comes, and the orphaned bytes shipped in anything that read `stage_dir`
/// directly. `ship::reclaim_staging_now` closes that gap for exactly the
/// build shape that proves nobody is left to read `stage_dir` afterward.
///
/// Ablate by commenting out the `reclaim_staging_now` call in
/// `build_test_shipped` above: `gone.webp` then survives on disk after build
/// 2 and the last assertion here goes red.
#[test]
fn a_dropped_reference_reclaims_its_webp_bytes_on_the_same_build() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(
        test_dir.join("index.md"),
        "---\ntitle: Home\ndate: 2026-01-02\n---\n\n# Home\n\n![Cover](gone.jpg)\n",
    )
    .unwrap();
    make_big_jpeg_at(&test_dir.join("gone.jpg"), 2400, 1800);

    build_test_shipped(folder_path).expect("build 1: image referenced");

    let stage_dir = test_dir.join(".moss/build/staging");
    assert!(
        stage_dir.join("gone.webp").is_file(),
        "fixture guard: the referenced image must produce a staged .webp, or \
         nothing below tests anything"
    );

    // Drop the reference. The source image stays in the vault — this is the
    // orphan prune's case to act on, not the deleted-source path
    // (`drop_absent_outputs`) or a removed-source fingerprint retention.
    fs::write(
        test_dir.join("index.md"),
        "---\ntitle: Home\ndate: 2026-01-02\n---\n\n# Home\n\nNo image now.\n",
    )
    .unwrap();

    let pruned = build_test_shipped(folder_path).expect("build 2: reference dropped");
    assert!(
        pruned.contains("gone.webp"),
        "the prune must condemn the now-orphaned variant: {pruned:?}"
    );
    assert!(
        !stage_dir.join("gone.webp").exists(),
        "the orphaned .webp must be gone from staging after THIS build — a \
         real `moss build` (and every moss-desktop snapshot-test fixture) is \
         a one-shot process with no next build to defer the reclaim to"
    );
}

/// A page the OS moved to the cloud must not lose its published HTML — on disk
/// or in the manifest deploy uploads.
///
/// Pre-Sonoma eviction (macOS 12–13, still moss's `minimumSystemVersion`) does
/// not flag the file `SF_DATALESS` — it *replaces* it with a hidden
/// `.name.icloud` placeholder, so the real filename vanishes from the scan. By
/// filename alone that is indistinguishable from the user deleting the page,
/// and four separate passes act on "this page is gone": `remove_stale_html`,
/// `stale_carried_forward`, `PendingManifest::seal`, and `remove_stale_files`.
/// Guarding only the first is what let the page be deleted anyway.
///
/// The last third is the other half of the invariant: the protection must not
/// simply be always-on, or stale cleanup silently stops working.
#[cfg(target_os = "macos")]
#[test]
fn a_cloud_evicted_page_survives_the_seal_time_sweep_and_the_deploy_manifest() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();
    fs::write(test_dir.join("index.md"), "# Home").unwrap();
    fs::write(test_dir.join("keeper.md"), "# Keeper\n\nStill a real page.").unwrap();

    let keys = build_test_sealed(folder_path).expect("first build");
    let published = test_dir.join(".moss/build/staging/keeper/index.html");
    assert!(published.is_file(), "first build must publish the page");
    assert!(keys.iter().any(|k| k == "keeper/index.html"), "keys: {keys:?}");

    // Pre-Sonoma eviction: the real name is gone from the walk, a hidden
    // placeholder stands in for it.
    fs::remove_file(test_dir.join("keeper.md")).unwrap();
    fs::write(test_dir.join(".keeper.md.icloud"), "").unwrap();

    let keys = build_test_sealed(folder_path).expect("build with the page evicted");
    assert!(
        published.is_file(),
        "an evicted page's HTML must survive the seal-time sweep too"
    );
    assert!(
        keys.iter().any(|k| k == "keeper/index.html"),
        "and it must stay in the deploy manifest, or publishing un-publishes the \
         live page. keys: {keys:?}"
    );

    // Now delete it for real — no placeholder standing in for it.
    fs::remove_file(test_dir.join(".keeper.md.icloud")).unwrap();

    let keys = build_test_sealed(folder_path).expect("build after a real deletion");
    assert!(
        !published.exists(),
        "a genuinely deleted page's HTML must still be cleaned up"
    );
    assert!(!keys.iter().any(|k| k == "keeper/index.html"), "keys: {keys:?}");
}


// ---------------------------------------------------------------------------
// The cloud gate: does this build's home page belong to the user?
//
// These exist because the gate's first implementation asked
// `stage_dir.join("index.html").is_file()`, which is *always* true — the render
// pass writes a real home page, an auto-index, or the empty-folder artifact, but
// it always writes one. The gate could therefore never raise, and every consumer
// downstream of it was dead code. Each case below fails against that version.
// ---------------------------------------------------------------------------

fn gate_structure(root: &std::path::Path, homepage: Option<&str>, evicted: &[&str]) -> ProjectStructure {
    ProjectStructure {
        root_path: root.to_string_lossy().to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 1,
        homepage_file: homepage.map(|s| s.to_string()),
        ffmpeg_bin_path: None,
        evicted_count: evicted.len(),
        evicted_paths: evicted.iter().map(|p| root.join(p)).collect(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    }
}

#[test]
fn gate_raises_when_sonoma_evicts_the_elected_home_page() {
    // Sonoma+ leaves the real name in the walk, so the scan elects it and then
    // the render pass defers it: `homepage_file` and `evicted_paths` agree.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("index.md"), &["index.md"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_raises_when_pre_sonoma_hides_the_home_page_and_a_runner_up_wins() {
    // macOS 12–13 replaces index.md with a hidden `.index.md.icloud` sibling, so
    // the scan never sees the name and elects about.md instead. The published
    // home page is a page the user never nominated.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("about.md"), &["index.md"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_raises_when_every_page_is_evicted() {
    // Nothing parsed, so the render pass emitted the empty-folder onboarding
    // artifact — for a vault that is not empty.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), None, &["index.md", "about.md"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_raises_when_the_alphabetical_fallback_home_is_evicted() {
    // No index.md anywhere: the home election falls through to "first document
    // alphabetically", and that file is the one still in the cloud.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("zebra.md"), &["about.md"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_stays_down_when_the_home_page_is_readable() {
    // A note deep in the vault is evicted. The home page is fine, so the site
    // the user sees is the site they have.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("index.md"), &["notes/deep.md"]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_stays_down_for_an_evicted_image() {
    // The old `icloud_count > 0` half of the condition would have raised the
    // cloud screen over a perfectly good site because one photo was evicted.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("index.md"), &["cover.jpg"]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_stays_down_when_nothing_is_evicted() {
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), None, &[]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

/// The election the gate re-runs is filename-only, so it cannot see a
/// `home: true` marker on an evicted file — but it must still see one on the
/// file that WAS published, or an unrelated evicted note wins Priority 5 and
/// raises the screen over a site that is entirely correct.
#[test]
fn gate_stays_down_when_the_published_home_page_carries_the_marker() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("zulu.md"), "---\nhome: true\n---\n# Home").unwrap();
    // `alpha.md` is an ordinary note that happens to sort first. Without the
    // marker check it wins the alphabetical fallback and, being evicted, is
    // read as "the home page has not arrived."
    let ps = gate_structure(tmp.path(), Some("zulu.md"), &["alpha.md"]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

/// Same shape, no marker: the alphabetical fallback genuinely does elect the
/// evicted file, so the gate must raise. This is what proves the test above is
/// measuring the marker and not the shape.
#[test]
fn gate_raises_when_an_unmarked_home_loses_to_an_evicted_alphabetical_winner() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("zulu.md"), "# Home").unwrap();
    let ps = gate_structure(tmp.path(), Some("zulu.md"), &["alpha.md"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

// ----- moss#982: the gate is monotonic, and sees post-scan evictions -----

#[test]
fn the_gate_holds_on_a_cold_open_with_files_still_in_the_cloud() {
    assert!(
        cloud_gate_should_hold(false, false, 553, false),
        "no home page, nothing sealed, files outstanding — this is what the screen is for"
    );
}

#[test]
fn the_gate_never_holds_once_a_generation_is_sealed() {
    // Monotonicity. This is what makes a screen with no dismissal control safe:
    // `home_waiting` is re-emitted by every build and arrivals trigger builds,
    // so a re-armable gate can slam a full-window screen over a site the user is
    // already reading (2026-08-05-cloud-waiting-screen-redesign.md §1).
    assert!(
        !cloud_gate_should_hold(false, true, 553, false),
        "a sealed generation is servable — blocking it is never right"
    );
}

#[test]
fn the_gate_does_not_hold_over_a_site_that_built() {
    assert!(!cloud_gate_should_hold(true, false, 553, false));
    assert!(!cloud_gate_should_hold(true, true, 553, false));
}

#[test]
fn the_gate_is_silent_when_nothing_is_in_the_cloud() {
    // A build that failed to produce a home page for a non-cloud reason must
    // not get the cloud screen — it would be a lie, and no arrival can lower it.
    assert!(!cloud_gate_should_hold(false, false, 0, false));
}

#[test]
fn a_post_scan_eviction_still_raises_the_gate() {
    // The blindness moss#982 measured: the scan count is taken before the build
    // and prunes dot-directories, so it is 0 for anything evicted afterwards.
    // The caller folds the ledger into `cloud_outstanding` precisely so this
    // case reaches the gate at all; here that is the difference between the
    // third argument being 0 and being non-zero.
    assert!(!cloud_gate_should_hold(false, false, 0, false), "scan count alone");
    assert!(
        cloud_gate_should_hold(false, false, 1, false),
        "one file the ledger recorded during the build is enough"
    );
}

// ----- structural sources: a site rendered without them is not servable -----

/// The reported symptom, pinned. `home_ready` alone said "servable" for a build
/// whose stylesheet and page sources were still downloading, so the first
/// preview of a Google Drive vault painted directory names, `Unknown` dates and
/// no CSS, and called it ready.
#[test]
fn the_gate_holds_when_the_site_built_without_its_own_sources() {
    assert!(
        cloud_gate_should_hold(true, false, 553, true),
        "an index.html rendered from files that were not there is not the author's site"
    );
}

/// Monotonicity survives the structural input. It may not become a second way to
/// slam a full-window screen over a site the user is already reading — "optimize
/// storage" can evict a source at any time, and every build re-emits.
#[test]
fn a_structural_source_in_the_cloud_never_re_arms_the_gate_over_a_sealed_site() {
    assert!(
        !cloud_gate_should_hold(true, true, 553, true),
        "a sealed generation is servable; the download belongs to the corner panel"
    );
    assert!(!cloud_gate_should_hold(false, true, 553, true));
}

/// A structural absence is a reason to hold, not a reason to invent a cloud
/// episode. With nothing outstanding there is no arrival coming, so a gate
/// raised here would never come down.
#[test]
fn a_structural_flag_alone_does_not_raise_the_gate() {
    assert!(!cloud_gate_should_hold(true, false, 0, true));
}

/// The ordinary case must be untouched: a fully-local vault is servable the
/// moment the home page exists.
#[test]
fn a_fully_local_build_leaves_the_gate_exactly_where_it_was() {
    assert!(!cloud_gate_should_hold(true, false, 553, false));
}

// ----- moss#1042: the publish decision is not the screen decision -----

/// The bug, stated as the smallest possible assertion. A sealed generation is
/// what made `cloud_gate_should_hold` return `false` — correctly, there was
/// something to look at — and the same fact was then used to justify replacing
/// it with a build rendered from files that were not there. `should_publish`
/// does not take that fact at all, which is the whole fix.
#[test]
fn a_sealed_generation_does_not_license_publishing_over_it() {
    assert!(
        !cloud_gate_should_hold(true, true, 553, true),
        "the screen stays down — the sealed generation is worth serving"
    );
    assert!(
        !should_publish(true),
        "and the build that could not read its sources still may not replace it"
    );
}

/// Media is decoration: a site whose images are still arriving is still the
/// user's site, and publishing it is right (the placeholders are ADR-013's
/// job). Only a structural absence withholds.
#[test]
fn media_still_downloading_does_not_withhold_a_publish() {
    assert!(should_publish(false), "553 images outstanding is not a reason to withhold");
}

/// Withholding on a cold vault rolls nothing back — there is nothing sealed to
/// roll back to — and the screen covers the same condition. Asserted together
/// because the pair is the safety argument: the user is never left with neither
/// a site nor an explanation.
#[test]
fn a_cold_vault_withholds_and_shows_the_screen() {
    assert!(!should_publish(true));
    assert!(cloud_gate_should_hold(false, false, 553, true));
}

/// The gate must stay clearable. Vaults hold files moss never opens, so the
/// provider never downloads them and their eviction is permanent — if one
/// withheld the publish, the user would never see their site again.
#[test]
fn an_evicted_file_moss_never_reads_does_not_withhold_forever() {
    let junk = vec![
        std::path::PathBuf::from("/v/archive.zip"),
        std::path::PathBuf::from("/v/art.psd"),
    ];
    assert_eq!(super::super::cloud_ledger::structural_missing_count(&junk, 0), 0);
    assert!(should_publish(false));
}

/// `initial-build-complete` does not report the serving directory, it *sets*
/// it — its listener in `lib.rs` calls `switch_to` with the payload path. So
/// naming staging on a withheld build would undo the withholding through the
/// back door, and this is the assertion that says it does not.
#[test]
fn a_withheld_build_never_names_staging_as_the_serving_directory() {
    let stage = std::path::Path::new("/v/.moss/build/staging");
    let current = std::path::Path::new("/v/.moss/build/current");

    assert_eq!(served_dir(true, true, stage, current), Some(stage), "published: staging");
    assert_eq!(served_dir(true, false, stage, current), Some(stage), "first build, published");
    assert_eq!(
        served_dir(false, true, stage, current),
        Some(current),
        "withheld with something sealed — the user keeps their real site"
    );
    assert_eq!(
        served_dir(false, false, stage, current),
        None,
        "withheld with nothing sealed — no directory to name; the screen owns the window"
    );
}

/// The invariant that keeps the two decisions in step: a build moss withholds
/// is always one the gate can explain. `cloud_ledger::structural_missing_count` only ever
/// sees paths still in the cloud (the caller filters), and each of its two
/// inputs is a subset of what `cloud_outstanding` counts — so `!should_publish`
/// implies `cloud_outstanding > 0`, which is exactly what
/// `cloud_gate_should_hold` needs to raise the screen when nothing is sealed.
/// Without it the user gets neither a site nor an explanation.
#[test]
fn a_withheld_build_can_always_raise_the_screen() {
    for (evicted, ledger) in [
        (vec![std::path::PathBuf::from("/v/index.md")], 0usize),
        (vec![], 1usize),
    ] {
        let missing = super::super::cloud_ledger::structural_missing_count(&evicted, ledger) > 0;
        assert!(missing, "precondition of this case");
        assert!(!should_publish(missing));
        // The caller's `cloud_outstanding` is `icloud_count.max(ledger)`, and
        // both inputs above contribute to it — so it is at least 1 here.
        let cloud_outstanding = evicted.len().max(ledger);
        assert!(cloud_outstanding > 0);
        assert!(
            cloud_gate_should_hold(false, false, cloud_outstanding, missing),
            "nothing sealed and nothing servable — the screen must be available"
        );
    }
}

/// Priority 3 of the home election is `index.pages` / `index.docx` — first-class
/// home pages that are not markdown at all. Filtering candidates to markdown
/// extensions let an evicted Pages home through the gate silently.
#[test]
fn gate_raises_when_an_evicted_pages_home_is_the_winner() {
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("notes.md"), &["index.pages"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));

    let ps = gate_structure(tmp.path(), Some("notes.md"), &["index.docx"]);
    assert!(home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

/// The converse of the above: `.markdown` is a page extension the scan accepts
/// but the election can never elect, so an evicted one is not a home-page problem.
#[test]
fn gate_stays_down_for_an_evicted_file_the_election_cannot_elect() {
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("index.md"), &["aaa.markdown"]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

#[test]
fn gate_stays_down_when_an_evicted_page_loses_the_election() {
    // index.md is readable and outranks the evicted about.md, so the home page
    // moss published is the real one.
    let tmp = TempDir::new().unwrap();
    let ps = gate_structure(tmp.path(), Some("index.md"), &["about.md"]);
    assert!(!home_page_is_a_substitute(&ps, tmp.path().to_str().unwrap()));
}

/// The direct regression guard for the discovery that motivated page hashing:
/// `SiteHashes.sources` used to be asset-only, because the one production
/// writer is the deferred media walk and that walk `continue`s past markdown
/// before it ever inserts. Publish-time classification diffs
/// (`sources`, `source_to_output`), so an asset-only `sources` means no page
/// can ever be told from another — every page reads as added or deleted.
///
/// Asserted through a real build and the persisted `hashes.json`, because the
/// failure lived in the wiring between the render phase and the manifest, not
/// in either one alone.
#[test]
fn markdown_pages_appear_in_sealed_sources() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Home\nwelcome").unwrap();
    fs::create_dir_all(test_dir.join("posts")).unwrap();
    fs::write(test_dir.join("posts").join("hello.md"), "# Hello\nbody").unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    let hashes = load_previous_hashes(folder_path);
    let pages: Vec<&String> = hashes.sources.keys().filter(|k| k.ends_with(".md")).collect();
    assert!(
        !pages.is_empty(),
        "sealed `sources` carries no markdown entries at all — page-source hashing is not wired up; got {:?}",
        hashes.sources.keys().collect::<Vec<_>>()
    );

    // Lockstep with the mapping: a page with an output and no hash is
    // unclassifiable at publish time.
    for src in hashes.source_to_output.keys() {
        let meta = hashes.sources.get(src).unwrap_or_else(|| {
            panic!("{src} has an output mapping but no source hash")
        });
        assert_eq!(meta.hash.len(), 64, "page hashes share the asset SHA-256 domain");
    }
}

/// The same wiring, one population over: SLOT-ONLY sources (`footer.md` and its
/// per-language siblings) render into every page's chrome and own no output, so
/// both registration loops — each keyed on an emitted output — used to skip
/// them and their hash never reached `sources`.
///
/// That is not cosmetic. The sweep's drift compare reads this map, and a walked
/// file with no baseline entry reads as NEW, so every 2s pass judged both
/// footers as drift, dispatched a full rebuild that could not clear them, then
/// blamed the watcher for missing an event and recreated it — after which the
/// folder degraded to sweep-only and the event-driven partial build stopped
/// happening at all (harbor, 2026-08-19: `n=2` every pass, always
/// `en/footer.md`).
///
/// `everything_the_walk_judges_the_scan_consumes` did not catch it: it asserts
/// walk ⊆ SCAN, and the scan does consume `footer.md`. The invariant drift
/// actually needs is walk ⊆ MANIFEST, which only a real build can show.
#[test]
fn slot_only_sources_appear_in_sealed_sources() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();

    fs::write(test_dir.join("index.md"), "# Home\nwelcome").unwrap();
    fs::write(test_dir.join("footer.md"), "site chrome").unwrap();
    fs::create_dir_all(test_dir.join("en")).unwrap();
    fs::write(test_dir.join("en").join("en.md"), "# English").unwrap();
    fs::write(test_dir.join("en").join("footer.md"), "english chrome").unwrap();

    build_test(folder_path, None, None, None, None, &ResolvedSlots::empty()).unwrap();

    let hashes = load_previous_hashes(folder_path);
    for slot in ["footer.md", "en/footer.md"] {
        let meta = hashes.sources.get(slot).unwrap_or_else(|| {
            panic!(
                "slot-only source {slot} is absent from sealed `sources`, so the sweep \
                 reads it as a new file every pass and drifts forever; got {:?}",
                hashes.sources.keys().collect::<Vec<_>>()
            )
        });
        assert_eq!(meta.hash.len(), 64, "slot hashes share the asset SHA-256 domain");
        // Hash WITHOUT a mapping is the point: a slot file owns no output, and
        // the publish classifier builds its page set from `source_to_output`,
        // so an entry here must not make it look like a page.
        assert!(
            !hashes.source_to_output.contains_key(slot),
            "{slot} must not gain an output mapping — it renders into chrome, not a page"
        );
    }
}

/// The publish gate's evidence must survive a real build.
///
/// `deploy::refuse_publish` is the only thing between an author and a
/// published site with a 404 image in it, and its evidence is whatever the
/// build recorded. moss-core's own tests prove the resolver *produces* a
/// `MissingAsset` diagnostic for these references; only a pipeline run proves
/// the build *keeps* it. It did not: the standard-markdown spelling is
/// resolved in the AST pass, whose diagnostics were logged and dropped.
#[test]
fn a_reference_to_a_file_that_is_not_there_reaches_the_publish_gate() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();
    fs::write(test_dir.join("index.md"), "# Home\n\nHome body.\n").unwrap();
    fs::write(
        test_dir.join("article.md"),
        // Both spellings an author can reach a missing file through.
        "# Article\n\n![missing](nowhere.png)\n\n![[gone.jpg]]\n",
    )
    .unwrap();

    let (_is_empty, missing) = build_test_full(
        folder_path,
        None,
        None,
        None,
        None,
        &ResolvedSlots::empty(),
    )
    .expect("build");

    let refs: Vec<&str> = missing.iter().map(|m| m.reference.as_str()).collect();
    assert!(
        refs.contains(&"nowhere.png"),
        "a standard-markdown image with no file behind it must reach the \
         publish gate; got {refs:?}"
    );
    assert!(
        refs.contains(&"gone.jpg"),
        "a wikilink embed with no file behind it must reach the publish \
         gate; got {refs:?}"
    );
}

/// The other half of the same contract: an empty verdict must mean "nothing
/// is missing", not "nobody looked". A gate that blocks every publish is as
/// useless as one that blocks none.
#[test]
fn a_vault_whose_media_all_exists_leaves_the_publish_gate_open() {
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();
    fs::write(test_dir.join("index.md"), "# Home\n\nHome body.\n").unwrap();
    // A real 1x1 PNG, so the asset phase has bytes it can decode.
    fs::write(
        test_dir.join("there.png"),
        [
            0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ],
    )
    .unwrap();
    fs::write(
        test_dir.join("article.md"),
        "# Article\n\n![there](there.png)\n",
    )
    .unwrap();

    let (_is_empty, missing) = build_test_full(
        folder_path,
        None,
        None,
        None,
        None,
        &ResolvedSlots::empty(),
    )
    .expect("build");

    assert!(
        missing.is_empty(),
        "a resolvable image must not block a publish; got {missing:?}"
    );
}

/// The presence pass is the last owner of "the manifest and the generation
/// agree". Four entries, four fates, one call — a carried entry whose file a
/// sync client evicted between builds is dropped; a 0-byte stub is dropped
/// too, so the next build's pre-render sweep unlinks it rather than a producer
/// trusting it; a `_moss/math/`
/// entry survives unreadable because the published site still serves it
/// (ADR-030) and un-promising one deletes it from a live site; a symlink
/// entry survives, since `output_present` reads the link, not the target.
#[cfg(unix)]
#[test]
fn the_presence_pass_drops_only_the_outputs_that_are_really_gone() {
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;

    let tmp = TempDir::new().unwrap();
    let stage = tmp.path().join("staging");
    fs::create_dir_all(stage.join("_moss/math")).unwrap();

    let real = ServedPath::from_source("page/index.html").unwrap();
    let stub = ServedPath::from_source("stub.css").unwrap();
    let gone = ServedPath::from_source("evicted.css").unwrap();
    let math = ServedPath::for_math_png("87ba30f2b3c09ca9").unwrap();
    let link = ServedPath::from_source("myapp").unwrap();

    fs::create_dir_all(stage.join("page")).unwrap();
    fs::write(real.to_disk(&stage), b"<h1>hi</h1>").unwrap();
    fs::write(stub.to_disk(&stage), b"").unwrap();
    // `gone` and `math` are deliberately never written — `math` stands for the
    // evicted PNG whose bytes a read would fault on.
    std::os::unix::fs::symlink("page", link.to_disk(&stage)).unwrap();

    let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
    pending.register(&real, b"<h1>hi</h1>", HashBucket::Files);
    pending.register_hashed(&stub, &crate::types::content::file_entry("aaaa"), HashBucket::Files);
    pending.register_hashed(&gone, &crate::types::content::file_entry("bbbb"), HashBucket::NotebookOutputs);
    pending.register_hashed(&math, &crate::types::content::file_entry("cccc"), HashBucket::Files);
    pending.register_hashed(
        &link,
        &crate::types::content::symlink_entry("page"),
        HashBucket::Files,
    );
    let mut sealed = pending.seal();
    let id_before = sealed.generation_id().to_string();

    crate::build::ship::drop_absent_outputs(&stage, &mut sealed);

    assert!(sealed.files().contains_key(real.as_str()), "a real output stays");
    assert!(
        sealed.files().contains_key(math.as_str()),
        "an unreadable math PNG keeps its entry — the live site still serves it"
    );
    assert!(
        sealed.files().contains_key(link.as_str()),
        "a symlink entry is present by the link, not by reading through it"
    );
    assert!(!sealed.files().contains_key(stub.as_str()), "a 0-byte stub is not an output");
    assert!(!sealed.files().contains_key(gone.as_str()), "a missing output is dropped");
    assert!(
        !sealed.notebook_outputs().contains(gone.as_str()),
        "the drop must reach every bucket, not just files — an entry left in one \
         names a path the generation does not contain, and deploy refuses the upload"
    );
    assert!(
        stub.to_disk(&stage).exists(),
        "the pass is read-only against staging — that tree is what the preview \
         server is reading while the seal tail runs. The stub goes at the next \
         build's start, via `pipeline::sweep_staging`, which finds it because \
         this drop kept it out of `hashes.json`"
    );
    assert_ne!(
        sealed.generation_id(),
        id_before,
        "a manifest that lost an entry after seal needs a new identity"
    );

    // The pair is the point. Keeping the math entry is only safe if ship also
    // declines to read it: a copy of absent bytes counts a failure, returns
    // Err, and `current` is never repointed — the stale preview this change
    // exists to end.
    let site = tmp.path().join("gen");
    crate::build::ship::ship_phase(&stage, &site, &sealed, None)
        .expect("an entry the presence pass kept unreadable must not fail the generation");
    assert!(
        !math.to_disk(&site).exists(),
        "the entry is kept for the live site; the local generation simply lacks it"
    );
    assert!(real.to_disk(&site).exists(), "the real output still ships");
}

#[test]
fn a_cmyk_jpeg_ships_as_the_original_with_no_webp_source() {
    // A CMYK JPEG is never encoded (`SkipReason::Cmyk`), but the synthesizer
    // promises `<picture><source srcset="plate.webp">` for every jpg from the
    // extension alone. Until 2026-09-05 the collector dropped the source, the
    // promise was never registered, and the published page carried a
    // `<source>` that 404ed — which `<picture>` does not recover from
    // (ADR-013): zhu-da's 河上花圖 rendered as nothing. Now the verdict rides
    // to the registration loop, the variant settles `Failed`, and the
    // post-seal degrade pass (moss#867) removes the `<source>` so the page
    // falls through to the original `<img>`.
    let (test_dir, _cleanup) = create_test_dir();
    let folder_path = test_dir.to_str().unwrap();
    fs::write(test_dir.join("index.md"), "# Plate\n\n![plate](plate.jpg)\n").unwrap();
    crate::build::media::image::tests::make_cmyk_jpeg(&test_dir.join("plate.jpg"));

    let services = BuildServices::headless();
    let result = build_test(
        folder_path,
        None,
        None,
        None,
        Some(&services),
        &ResolvedSlots::empty(),
    );
    assert!(result.is_ok(), "build should succeed: {:?}", result);

    // The build's own verdict: the promise the synthesizer made is settled,
    // not left unregistered.
    let registry = services.assets.as_deref().expect("headless carries a registry");
    assert!(
        matches!(registry.get("plate.webp"), Some(crate::types::assets::AssetState::Failed(_))),
        "the never-encoded variant must settle Failed, got {:?}",
        registry.get("plate.webp")
    );

    // `build_test` stops where the seal tail (`advertise_sealed`) begins, so
    // run the tail's degrade step over the real staging page. This is the
    // same call the tail makes, against the HTML the build emitted.
    let staging = test_dir.join(".moss/build/staging");
    let html = fs::read_to_string(staging.join("index.html")).unwrap();
    assert!(html.contains("plate.webp"), "premise: the page promised the variant:\n{html}");
    let mut pending = crate::build::manifest::PendingManifest::new(Default::default());
    pending.register(
        &crate::build::served_path::ServedPath::from_source("index.html").unwrap(),
        html.as_bytes(),
        crate::build::manifest::HashBucket::Files,
    );
    let mut sealed = pending.seal();
    crate::build::degrade::apply_to_staging(&staging, &mut sealed, &registry.failed_keys());

    let html = fs::read_to_string(staging.join("index.html")).unwrap();
    assert!(
        !html.contains("<source"),
        "no variant of a CMYK JPEG may survive to the published page:\n{html}"
    );
    assert!(html.contains(r#"src="/plate.jpg""#), "the original stays the image:\n{html}");
    assert!(staging.join("plate.jpg").exists(), "the original ships");
}
