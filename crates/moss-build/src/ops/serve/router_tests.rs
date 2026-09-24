use super::*;
// `is_port_available` is no longer needed by the production server-start
// path (the bind-with-scan helper is the authoritative check), but tests
// still use it as a non-binding "did the port get freed?" probe.
use super::super::port::is_port_available;
use std::sync::Arc;

/// Sync test: the moss-health body MUST contain `MOSS_HEALTH_MARKER`.
/// Locks the producer/consumer contract so a future typo-fix to the
/// constant cannot silently make every moss-vs-foreign reuse check
/// reject moss's own server.
#[test]
fn moss_health_body_contains_marker() {
    let body = moss_health_body();
    assert!(
        body.contains(MOSS_HEALTH_MARKER),
        "moss_health_body() must contain MOSS_HEALTH_MARKER ({}); got: {}",
        MOSS_HEALTH_MARKER,
        body
    );
    // Spot-check the schema field landed too — readers don't parse it
    // yet but its presence is part of the wire-format contract.
    assert!(
        body.contains("\"schema\":1"),
        "moss_health_body() must include schema:1; got: {}",
        body
    );
}

#[tokio::test]
async fn test_server_starts_without_index_html() {
    // Test that start_server can start even for empty directories
    // (without index.html). This is important for before_build plugins that
    // download content after the server starts.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Directory is empty - no index.html
    assert!(!temp_dir.path().join("index.html").exists());

    // Create site directory state
    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));

    // Use a high port range (55000+) to avoid conflicts with other tests
    // and production port 8080
    // Pass None for asset_registry (CLI mode behavior)
    let result =
        start_server(ServeConfig {
            ..ServeConfig::new(site_dir_state, 55000)
        }).await;
    assert!(
        result.is_ok(),
        "Server should start without index.html: {:?}",
        result.err()
    );

    let (port, shutdown_tx) = result.unwrap();
    assert!(
        port >= 55000,
        "Should return a valid port in the test range"
    );

    // Clean up: gracefully shut down the server via the oneshot channel
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_server_with_asset_registry_starts() {
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let asset_registry = Arc::new(AssetRegistry::new());

    // Test that server starts with asset registry (integrated functionality)
    let result = start_server(ServeConfig {
        asset_registry: Some(asset_registry),
        ..ServeConfig::new(site_dir_state, 56000)
    })
    .await;
    assert!(
        result.is_ok(),
        "Server with asset registry should start: {:?}",
        result.err()
    );

    // Clean up via graceful shutdown
    let (_port, shutdown_tx) = result.unwrap();
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_server_graceful_shutdown_frees_port() {
    // Test that sending shutdown signal actually stops the server and frees the port
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));

    let (port, shutdown_tx) =
        start_server(ServeConfig {
            ..ServeConfig::new(site_dir_state, 57000)
        })
            .await
            .expect("Server should start");

    // Server should be running
    assert!(
        !is_port_available(port),
        "Port should be occupied while server runs"
    );

    // Send shutdown signal
    let _ = shutdown_tx.send(());

    // Give the server a moment to shut down
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Port should now be free
    assert!(
        is_port_available(port),
        "Port should be free after graceful shutdown"
    );
}

// ===== Content wrapper integration tests =====

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_image_file_returns_wrapped_html() {
    // Verify that requesting an image file returns an HTML wrapper with <img> tag,
    // __moss_raw=1 reference, and the injected bridge script.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Write a fake PNG file (content doesn't matter for wrapping logic)
    std::fs::write(temp_dir.path().join("test.png"), b"PNG fake data").unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58000)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/test.png", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "document")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");

    // Content-Type should be text/html (wrapped), not image/png
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("text/html"),
        "Content-Type should be text/html, got: {}",
        content_type
    );

    let body = response.into_string().expect("Should read body");

    // Should contain an <img> tag referencing the original file with __moss_raw=1
    assert!(
        body.contains("<img"),
        "Wrapped HTML should contain <img> tag"
    );
    assert!(
        body.contains("__moss_raw=1"),
        "Wrapped HTML should reference raw URL via __moss_raw=1"
    );

    // Bridge script should have been injected into the wrapped HTML
    assert!(
        body.contains("moss-rpc-call"),
        "Bridge script should be injected (contains moss-rpc-call)"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_raw_param_serves_original_file() {
    // Verify that ?__moss_raw=1 bypasses the wrapper and serves the original file bytes.
    use std::io::Read;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let original_bytes: &[u8] = b"PNG fake image data for raw test";
    std::fs::write(temp_dir.path().join("test.png"), original_bytes).unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58500)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/test.png?__moss_raw=1", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");

    // Content-Type should be image/png (original, not wrapped)
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("image/png"),
        "Content-Type should be image/png for raw request, got: {}",
        content_type
    );

    // Body should be the exact original file bytes
    let mut body_bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut body_bytes)
        .expect("Should read body bytes");
    assert_eq!(
        body_bytes, original_bytes,
        "Raw response body should match original file bytes"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_html_file_passes_through() {
    // Verify that HTML files are served as-is (not wrapped), with bridge script injected.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<html><body>Hello</body></html>",
    )
    .unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 59000)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/index.html", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");

    let body = response.into_string().expect("Should read body");

    // Original content should be present
    assert!(
        body.contains("Hello"),
        "HTML body should contain original content 'Hello'"
    );

    // Bridge script should be injected
    assert!(
        body.contains("moss-rpc-call"),
        "Bridge script should be injected into HTML response"
    );

    // Should NOT be wrapped (no __moss_raw=1 reference)
    assert!(
        !body.contains("__moss_raw=1"),
        "HTML file should not be wrapped (no __moss_raw=1 reference)"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_start_serves_generation_through_current_symlink() {
    // Regression (preview blank→404 on cold start): a previously-built site
    // is reached through `current → generations/<id>/`, not through any
    // directory the build populates in place. A server seeded with
    // MossPaths::initial_serve_dir() must serve that frozen generation
    // through the `current` symlink — NOT 404. Guards the symlink-follow +
    // cold-start seeding contract behind the build.rs cold-start fix.
    use crate::moss_paths::MossPaths;
    use tempfile::TempDir;

    // Repo-local temp (project rule: never /tmp).
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let proj = TempDir::new_in(&base).expect("temp project");

    let mp = MossPaths::new(proj.path());
    mp.ensure_dirs().unwrap();
    // Seal a previous build: generations/gen001/index.html + current → gen001.
    let gen = mp.generation_dir("gen001");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(
        gen.join("index.html"),
        "<html><body>FROZEN-GEN</body></html>",
    )
    .unwrap();
    mp.set_current_ptr("gen001").unwrap();

    // site/ is empty — the regression rested here and 404'd.
    let serve_dir = mp.initial_serve_dir();
    assert_eq!(
        serve_dir,
        mp.current_ptr(),
        "cold start must rest on the current generation"
    );

    let site_dir_state = Arc::new(std::sync::RwLock::new(serve_dir.clone()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 59500)
    })
    .await
    .expect("Server should start");

    // The iframe loads the server root on cold start — must 200, not 404.
    let url = format!("http://localhost:{}/", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("cold-start request to / should succeed (not 404)");
    assert_eq!(
        response.status(),
        200,
        "cold start must serve the frozen generation, not 404"
    );
    let body = response.into_string().expect("read body");
    assert!(
        body.contains("FROZEN-GEN"),
        "must serve the frozen generation through the current symlink, got: {body}"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frozen_generation_page_regains_preview_gate_attribute() {
    // Simulates the zero-flicker window: during rebuilds (and on cold
    // start) the server serves the previous SHIPPED generation, where
    // ship_phase removed the no-preview markers (keeping the beacon
    // script for deploy) and stripped data-moss-preview from <body>.
    // Runtime preview gates — the beacon's self-gate, subscribe.ts's
    // no-real-POST gate — all read that attribute, so the middleware
    // must re-guarantee it on every served HTML response. This is the
    // regression test for the beacon firing from a local preview.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    std::fs::write(
        temp_dir.path().join("index.html"),
        concat!(
            "<html><head>",
            "<script id=\"moss-beacon\">/* gates on data-moss-preview */</script>",
            "</head><body class=\"page\"><p>frozen</p></body></html>",
        ),
    )
    .unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 61700)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/index.html", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");
    let body = response.into_string().expect("Should read body");

    assert!(
        body.contains("<body data-moss-preview class=\"page\">"),
        "served frozen page must regain data-moss-preview on <body>, got: {body}"
    );

    let _ = shutdown_tx.send(());
}

/// The seal tail runs DETACHED, and it runs against the very directory the
/// preview server is reading: `pipeline::run` points the server at
/// `staging/` when the render finishes, and nothing moves it off until the
/// NEXT build starts. So any pass in that tail that unlinks a staged file
/// takes the file out from under a live reader — and the frontend asks for
/// pages at exactly that moment, because `refresh-preview` fires as soon as
/// the build returns.
///
/// Driven step by step rather than raced: the repair pass is called directly
/// and the served tree is fetched on both sides of it, so a regression is a
/// deterministic 404 instead of a timing window that passes on a fast box.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_seal_tail_leaves_the_served_staging_tree_alone() {
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;
    use crate::moss_paths::MossPaths;
    use crate::types::content::SiteHashes;

    // Repo-local temp (project rule: never /tmp).
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let vault = tempfile::TempDir::new_in(&base).expect("temp vault");

    let mp = MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("assets")).unwrap();
    let html = concat!(
        "<html><body data-moss-preview>",
        r#"<picture><source srcset="assets/kept.webp" type="image/webp">"#,
        r#"<img src="assets/kept.jpg"></picture>"#,
        "</body></html>\n",
    );
    std::fs::write(stage.join("index.html"), html).unwrap();
    std::fs::write(stage.join("assets/kept.webp"), b"kept webp bytes").unwrap();
    // Registered, on disk, and referenced by nothing: the orphan prune's arm.
    std::fs::write(stage.join("assets/orphan.webp"), b"orphan webp bytes").unwrap();

    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(
        &ServedPath::from_source("index.html").unwrap(),
        html.as_bytes(),
        HashBucket::Files,
    );
    for (rel, bytes) in [
        ("assets/kept.webp", b"kept webp bytes".as_slice()),
        ("assets/orphan.webp", b"orphan webp bytes".as_slice()),
    ] {
        pending.register(
            &ServedPath::from_source(rel).unwrap(),
            bytes,
            HashBucket::ImageVariants,
        );
    }
    let mut sealed = pending.seal();

    let (port, shutdown_tx, _token) = serve_bound(stage.clone(), 59700).await;
    let get = |rel: &str| {
        let url = format!("http://localhost:{}/{}", port, rel);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(5)).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("transport error fetching {rel}: {e}"),
        }
    };

    // Control: everything the tail is about to walk over is reachable now.
    assert_eq!(get("index.html"), 200, "control: the page must be served before the tail runs");
    assert_eq!(get("assets/orphan.webp"), 200, "control: the orphan must be on disk before the tail runs");

    crate::build::degrade::repair_staged_html(
        &mp,
        &stage,
        &mut sealed,
        std::collections::HashSet::new(),
    );

    assert_eq!(
        get("index.html"),
        200,
        "the tail must not take the page out from under the reader"
    );
    assert_eq!(
        get("assets/orphan.webp"),
        200,
        "an unreferenced variant leaves the GENERATION by leaving the manifest — \
         unlinking it from the tree the preview is serving is a live 404"
    );
    assert!(
        !sealed.files().contains_key("assets/orphan.webp"),
        "it must still be dropped from the manifest, or ship_phase copies it into the generation"
    );

    // `remove_stale_html` unlinks from staging, so it needs a permit, and the
    // permit comes from a lifecycle that has caught up: the render on screen
    // is on `current`, so the park moves the server there first. Driven
    // against a second real server whose cell the lifecycle moves.
    use crate::build::lifecycle;
    use crate::build::media::pipeline::remove_stale_html;

    std::fs::create_dir_all(stage.join("old-page")).unwrap();
    std::fs::write(stage.join("old-page/index.html"), "<html>old</html>").unwrap();
    let blocking_keys: std::collections::HashSet<String> =
        std::iter::once("index.html".to_string()).collect();

    let _record = lifecycle::lock_for(&mp);
    std::fs::create_dir_all(mp.generation_dir("g1")).unwrap();
    let site_dir_cell = Arc::new(std::sync::RwLock::new(std::path::PathBuf::new()));
    lifecycle::adopt_server(&mp, &site_dir_cell);
    let (shown, _) = lifecycle::show_render(&mp, true);
    assert!(lifecycle::promote(&mp, crate::build::ship::next_promotion_epoch(), Some(shown), "g1").unwrap());
    let (port2, shutdown_tx2) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_cell.clone(), 59750)
    })
    .await
    .expect("second server should start");
    let get2 = |rel: &str| {
        let url = format!("http://localhost:{}/{}", port2, rel);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(5)).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("transport error fetching {rel}: {e}"),
        }
    };

    let permit = lifecycle::park_for_rebuild(&mp, false, Default::default()).expect("a caught-up lifecycle permits the sweep");
    assert_eq!(*site_dir_cell.read().unwrap(), mp.current_ptr(), "and parks the server off staging first");
    remove_stale_html(&stage, &blocking_keys, &std::collections::HashSet::new(), &permit);

    // Only the next render moves the server back onto `stage`.
    lifecycle::show_render(&mp, true);

    assert_eq!(
        get2("old-page/index.html"),
        404,
        "the deleted source's page must already be gone by the moment the server can reach stage_dir"
    );
    assert_eq!(
        get2("index.html"),
        200,
        "the kept page must be servable the instant the switch lands"
    );

    let _ = shutdown_tx2.send(());
    let _ = shutdown_tx.send(());
}

/// A rebuild that starts before the last render's generation is promoted
/// must not move the preview to `current`: `current` is older than what the
/// author is looking at, so every page that render added would 404 for the
/// length of the rebuild. It stays on staging and that build sweeps nothing.
/// Once the render's generation is promoted, the next rebuild parks on it and
/// may sweep, because `current` now holds everything staging showed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rebuild_never_parks_the_preview_on_a_generation_older_than_the_render_on_screen() {
    use crate::build::lifecycle;
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;
    use crate::build::ship::{materialize_and_promote, next_promotion_epoch, Promotion, ShipVerdict};
    use crate::moss_paths::MossPaths;
    use crate::types::content::SiteHashes;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let vault = tempfile::TempDir::new_in(&base).expect("temp vault");
    let mp = MossPaths::new(vault.path());
    let _record = lifecycle::lock_for(&mp);
    let stage = mp.staging_dir();
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::create_dir_all(mp.generation_dir("g1")).unwrap();
    std::fs::write(mp.generation_dir("g1").join("index.html"), "<html>g1</html>").unwrap();

    // Render 1 is shown and promoted as g1, which has no `fresh/`.
    let cell = Arc::new(std::sync::RwLock::new(std::path::PathBuf::new()));
    lifecycle::adopt_server(&mp, &cell);
    let (r1, _) = lifecycle::show_render(&mp, true);
    assert!(lifecycle::promote(&mp, next_promotion_epoch(), Some(r1), "g1").unwrap());

    // Render 2 adds `fresh/` and is on screen; its seal tail has not run.
    let pages = [("index.html", "<html>home</html>"), ("fresh/index.html", "<html>fresh</html>")];
    for (rel, html) in pages {
        std::fs::create_dir_all(stage.join(rel).parent().unwrap()).unwrap();
        std::fs::write(stage.join(rel), html).unwrap();
    }
    let (r2, _) = lifecycle::show_render(&mp, true);

    let (port, shutdown_tx) = start_server(ServeConfig::new(cell.clone(), 59800)).await.expect("server");
    let get = |rel: &str| {
        let url = format!("http://localhost:{}/{}", port, rel);
        match ureq::get(&url).timeout(std::time::Duration::from_secs(5)).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("transport error fetching {rel}: {e}"),
        }
    };

    assert!(lifecycle::park_for_rebuild(&mp, false, Default::default()).is_none(), "a rebuild ahead of the promotion may not sweep");
    assert_eq!(get("fresh/"), 200, "the page render 2 added must stay served through the rebuild");

    let mut pending = PendingManifest::new(SiteHashes::default());
    for (rel, html) in pages {
        pending.register(&ServedPath::from_source(rel).unwrap(), html.as_bytes(), HashBucket::Files);
    }
    let sealed = pending.seal();
    let promotion =
        materialize_and_promote(&sealed, &mp, &stage, None, next_promotion_epoch(), Some(r2), ShipVerdict::Ship);
    assert_eq!(promotion, Ok(Promotion::Promoted));

    assert!(lifecycle::park_for_rebuild(&mp, false, Default::default()).is_some(), "caught up, the next rebuild may sweep");
    assert_eq!(*cell.read().unwrap(), mp.current_ptr(), "and it parks on current");
    assert_eq!(get("fresh/"), 200, "which now holds render 2's pages");

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pdf_file_returns_unsupported_page() {
    // Verify that unsupported file types (PDF) get the "open in system viewer" page.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Write a fake PDF file
    std::fs::write(temp_dir.path().join("doc.pdf"), b"%PDF-1.4 fake").unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 59500)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/doc.pdf", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "document")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");

    // Content-Type should be text/html (wrapped)
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("text/html"),
        "Content-Type should be text/html for unsupported file, got: {}",
        content_type
    );

    let body = response.into_string().expect("Should read body");

    // Should contain the "open in system viewer" postMessage call
    assert!(
        body.contains("moss-open-in-system"),
        "Unsupported page should contain moss-open-in-system postMessage"
    );

    // Should reference the filename
    assert!(
        body.contains("doc.pdf"),
        "Unsupported page should contain the filename 'doc.pdf'"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sub_resource_image_not_wrapped() {
    // Verify that sub-resource loads (e.g. <img src>) are NOT wrapped.
    // Without Sec-Fetch-Dest: document, the middleware should pass through raw bytes.
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let image_bytes: &[u8] = b"PNG fake image data";
    std::fs::write(temp_dir.path().join("photo.png"), image_bytes).unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 60000)
    })
    .await
    .expect("Server should start");

    // Request with Sec-Fetch-Dest: image (like a browser's <img> tag load)
    let url = format!("http://localhost:{}/photo.png", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Request should succeed");

    // Content-Type should be image/png (NOT wrapped to text/html)
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("image/png"),
        "Sub-resource image should be served raw as image/png, got: {}",
        content_type
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_dir_decodes_percent_2c_to_comma_named_file() {
    // Follow-up #4: the synthesizer emits ladder srcset candidate URLs with
    // commas `%2C`-encoded (`a%2Cb.w800.webp`), while the deployed file is
    // written to disk with a LITERAL comma (`a,b.w800.webp`). This proves
    // the round-trip: a browser requesting the `%2C`-encoded URL resolves to
    // the literal-comma file — i.e. tower-http `ServeDir` percent-decodes
    // `%2C` → `,` before hitting disk (the same repair the `%20`/CJK paths
    // already rely on). Without this, the fix would trade a mis-split for a
    // different 404.
    use std::io::Read;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // The deployed rung lands on disk with a LITERAL comma in its name.
    let image_bytes: &[u8] = b"WEBP fake rung bytes";
    std::fs::write(temp_dir.path().join("a,b.w800.webp"), image_bytes).unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 60400)
    })
    .await
    .expect("Server should start");

    // The browser requests the srcset URL with the comma percent-encoded.
    let url = format!("http://localhost:{}/a%2Cb.w800.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("%2C-encoded URL must resolve to the literal-comma file (200, not 404)");
    assert_eq!(response.status(), 200);

    let mut body = Vec::new();
    response.into_reader().read_to_end(&mut body).unwrap();
    assert_eq!(
        body, image_bytes,
        "ServeDir must decode %2C → , and serve the on-disk `a,b.w800.webp` bytes"
    );

    let _ = shutdown_tx.send(());
}

// ===== Bug 1: source-passthrough (instant sharp preview) =====

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_passthrough_serves_full_original_for_pending_webp() {
    // A promised .webp variant that has NOT been encoded yet (absent from
    // the served root) but IS registered as a source passthrough → the
    // server returns the FULL ORIGINAL bytes (sharp), not the tiny LQIP
    // stub and not a 404. The source lives OUTSIDE the served root, so
    // ServeDir alone would 404 — only the passthrough serves it.
    use crate::types::assets::AssetRegistry;
    use std::io::Read;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    // A distinctive, non-trivial original body (> the 50-byte stub).
    let original_bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let source_file = src.path().join("hero.jpg");
    std::fs::write(&source_file, &original_bytes).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("assets/hero.webp".to_string(), Some((100, 100)), None);
    registry.set_source_passthrough("assets/hero.webp".to_string(), source_file.clone());

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60100)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/hero.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("passthrough must serve 200, not 404");
    assert_eq!(response.status(), 200);

    let mut body = Vec::new();
    response.into_reader().read_to_end(&mut body).unwrap();
    assert_eq!(
        body, original_bytes,
        "passthrough must serve the FULL original bytes (not the LQIP stub)"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passthrough_response_forbids_caching_so_the_encoded_variant_can_replace_it() {
    // These bytes are a STAND-IN occupying the URL the encoded variant will
    // take. `ServeFile` dates `Last-Modified` from the SOURCE file, so a photo
    // shot last year earns a heuristic freshness lifetime of weeks (RFC 9111
    // § 4.2.2) — the browser would keep serving the uncompressed original from
    // cache and the author would never see the encode land. Belt to the
    // iframe-bridge's `?_t=` cache-bust braces.
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let source_file = src.path().join("hero.jpg");
    std::fs::write(&source_file, vec![7u8; 512]).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("assets/hero.webp".to_string(), Some((100, 100)), None);
    registry.set_source_passthrough("assets/hero.webp".to_string(), source_file);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60150)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/hero.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("passthrough must serve 200");
    assert_eq!(
        response.header("cache-control").unwrap_or(""),
        "no-cache, no-store, must-revalidate",
        "a passthrough stand-in must never be cached at the variant's own URL"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passthrough_matches_a_percent_encoded_request_against_its_decoded_key()
{
    // Registry keys come from source file paths and are raw UTF-8
    // (`images/冬.webp`); the browser sends `images/%E5%86%AC.webp`. The router
    // decodes before the passthrough lookup, so the two meet. Without that a
    // CJK-named cover would miss the passthrough, fall to the 1×1 stub, and the
    // author would see nothing at all where their photo should be.
    use crate::types::assets::AssetRegistry;
    use std::io::Read;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let original_bytes: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
    let source_file = src.path().join("winter.jpg");
    std::fs::write(&source_file, &original_bytes).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("images/冬.webp".to_string(), Some((100, 100)), None);
    registry.set_source_passthrough("images/冬.webp".to_string(), source_file);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60160)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/images/%E5%86%AC.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("percent-encoded variant must reach the passthrough");
    assert_eq!(response.status(), 200);
    let mut body = Vec::new();
    response.into_reader().read_to_end(&mut body).unwrap();
    assert_eq!(
        body, original_bytes,
        "the decoded request path must resolve the raw-UTF-8 registry key"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn passthrough_range_request_on_video_returns_206() {
    // Video seeking needs Range. A registered .mp4 → source .mov passthrough
    // is served via ServeFile, which honors a Range header → 206 Partial
    // Content + Content-Range (Content-Length too).
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let original_bytes: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
    let source_file = src.path().join("clip.mov");
    std::fs::write(&source_file, &original_bytes).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("videos/clip.mp4".to_string(), Some((640, 480)), None);
    registry.set_source_passthrough("videos/clip.mp4".to_string(), source_file.clone());

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60200)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/videos/clip.mp4", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "video")
        .set("Range", "bytes=0-15")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("range passthrough must succeed");
    assert_eq!(
        response.status(),
        206,
        "Range request must yield 206 Partial Content"
    );
    assert!(
        response.header("content-range").is_some(),
        "206 response must carry a Content-Range header"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_variant_returns_warning_svg_despite_passthrough_registration() {
    // A terminally FAILED variant must STILL surface the warning SVG even
    // though a source passthrough was registered at pending time — the
    // encode failure stays visible (locked answer #1).
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let source_file = src.path().join("broken.jpg");
    std::fs::write(&source_file, vec![1u8; 4096]).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    // Registered at pending time, then the encode failed terminally.
    registry.set_source_passthrough("assets/broken.webp".to_string(), source_file.clone());
    registry.set_failed("assets/broken.webp".to_string(), "decode error".to_string());

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60300)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/broken.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("failed variant returns 200 warning svg");
    assert_eq!(response.status(), 200);
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("image/svg+xml"),
        "Failed variant must return the warning SVG, not the passthrough original; got: {}",
        content_type
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_passthrough_with_unreadable_source_falls_back_to_transparent_stub() {
    // iCloud-dataless / deleted source: the passthrough source path is
    // registered but the file is gone. The server must fail GRACEFULLY to
    // the 1×1 transparent-webp stub (200) — never 404 a chosen <source>.
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    // Register a passthrough to a path that does NOT exist.
    let missing = src.path().join("gone.jpg");

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("assets/gone.webp".to_string(), Some((10, 10)), None);
    registry.set_source_passthrough("assets/gone.webp".to_string(), missing);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60400)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/gone.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("unreadable passthrough must fall back to stub, not 404");
    assert_eq!(response.status(), 200, "must never 404 a chosen <source>");
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("image/webp"),
        "unreadable source falls back to the transparent-webp stub; got: {}",
        content_type
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cloud_evicted_source_is_never_opened_for_passthrough() {
    // The source is READABLE here — the file is right there, full of bytes. Only
    // the eviction probe says otherwise, which is exactly the macOS case this
    // branch exists for and the one no Linux filesystem can stage: under the
    // dataless-fail-fast policy a plain `open` of an evicted file SUCCEEDS, so
    // `ServeFile` would answer 200 with a full Content-Length and a body that
    // dies on first poll. A truncated 200 is as unrecoverable for `<picture>` as
    // the 404 the promise model forbids, so the probe has to be asked BEFORE the open.
    //
    // Injecting the verdict is not faking the test: the bytes, the registry, the
    // router and the response are all real, and the assertion is that a `true`
    // verdict diverts the request. Without the gate this serves the original and
    // the test fails.
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let source_file = src.path().join("hero.jpg");
    std::fs::write(&source_file, vec![9u8; 4096]).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("assets/hero.webp".to_string(), Some((100, 100)), None);
    registry.set_source_passthrough("assets/hero.webp".to_string(), source_file);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        is_evicted: |_path| true, // the provider has evicted this file's data
        ..ServeConfig::new(site_dir_state, 60500)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/hero.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("an evicted source must still answer, never 404");
    assert_eq!(response.status(), 200, "must never 404 a chosen <source>");
    assert_eq!(
        response.header("content-type").unwrap_or(""),
        "image/webp",
        "an evicted source diverts to the 1x1 transparent stub"
    );
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut body).unwrap();
    assert!(
        body.len() < 100,
        "the 50-byte stub, not the 4096-byte original: got {} bytes",
        body.len()
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_readable_source_still_passes_through_when_the_probe_says_no() {
    // The control for the test above: same shape, `false` verdict, real bytes.
    // Pins that the gate diverts ONLY on an eviction — a probe wired to reject
    // everything would pass the test above and silently disable passthrough.
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let original_bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let source_file = src.path().join("hero.jpg");
    std::fs::write(&source_file, &original_bytes).unwrap();

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("assets/hero.webp".to_string(), Some((100, 100)), None);
    registry.set_source_passthrough("assets/hero.webp".to_string(), source_file);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        is_evicted: |_path| false,
        ..ServeConfig::new(site_dir_state, 60550)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/assets/hero.webp", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "image")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("a readable source must pass through");
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut body).unwrap();
    assert_eq!(body, original_bytes, "the author gets their real photo");

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreadable_video_passthrough_falls_back_to_the_sized_svg_not_an_image_stub() {
    // The webp sibling of this test cannot tell the two fallback shapes apart:
    // for an image URL both the bare stub and the placeholder handler answer
    // `image/webp`. A VIDEO is where they diverge. Handing the failure to
    // `handle_asset_request` gets a `<rect>` SVG at the scanned dimensions —
    // right media type, right box, no layout shift — instead of a 50-byte 1x1
    // WebP served as `image/webp` at a `.mp4` URL, which is not a video at all.
    use crate::types::assets::AssetRegistry;
    use tempfile::TempDir;

    let site = TempDir::new().unwrap();
    let src = TempDir::new().unwrap();
    let missing = src.path().join("gone.mov");

    let registry = Arc::new(AssetRegistry::new());
    registry.set_pending("videos/clip.mp4".to_string(), Some((640, 480)), None);
    registry.set_source_passthrough("videos/clip.mp4".to_string(), missing);

    let site_dir_state = Arc::new(std::sync::RwLock::new(site.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        asset_registry: Some(registry),
        ..ServeConfig::new(site_dir_state, 60450)
    })
    .await
    .expect("Server should start");

    let url = format!("http://localhost:{}/videos/clip.mp4", port);
    let response = ureq::get(&url)
        .set("Sec-Fetch-Dest", "video")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("unreadable video passthrough must not 404");
    assert_eq!(response.status(), 200, "must never 404 a chosen source");
    assert!(
        response.header("content-type").unwrap_or("").starts_with("image/svg+xml"),
        "a Pending video falls back to the dimension-sized SVG, not the image stub; got: {}",
        response.header("content-type").unwrap_or("")
    );
    let body = response.into_string().unwrap();
    assert!(
        body.contains(r#"width="640""#) && body.contains(r#"height="480""#),
        "the placeholder reserves the scanned box so nothing shifts: {body}"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_404_returns_html_with_bridge_script() {
    // Verify that requesting a non-existent page returns an HTML 404 page
    // with the iframe-bridge script injected. This is critical because:
    // 1. Without HTML body, the bridge script can't be injected
    // 2. Without bridge script, navigation is dead (no back button, no link clicks)
    // 3. The user is stuck on a blank page with no way to recover
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Write a real HTML file so the server has something to serve
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<html><body>Home</body></html>",
    )
    .unwrap();
    // Do NOT create "nonexistent.html" — this is what we're testing

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 60500)
    })
    .await
    .expect("Server should start");

    // Request a page that doesn't exist
    let url = format!("http://localhost:{}/nonexistent", port);
    let result = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call();

    // ureq treats 4xx as errors, so we need to handle that
    let response = match result {
        Ok(resp) => resp,
        Err(ureq::Error::Status(status, resp)) => {
            assert_eq!(status, 404, "Should return 404 status");
            resp
        }
        Err(e) => panic!("Request failed unexpectedly: {}", e),
    };

    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("text/html"),
        "404 response should be text/html so bridge script is injected, got: {}",
        content_type
    );

    let body = response.into_string().expect("Should read body");

    // Should contain a proper HTML page with </body> tag
    assert!(
        body.contains("</body>"),
        "404 page should have </body> tag for bridge script injection point"
    );

    // Should contain "404" indicator
    assert!(
        body.contains("404"),
        "404 page should indicate the status code"
    );

    // Bridge script should be injected by the middleware
    assert!(
        body.contains("moss-rpc-call"),
        "Bridge script should be injected into 404 HTML page"
    );

    // Machine-readable 404 marker: the bridge reports this to the shell so a
    // rename-navigation that raced the generation swap can be retried
    // (preview-actions.ts `decideRenameRetry`). Matching the marker, not the
    // "Not Found" title, avoids false positives from real pages so-titled.
    assert!(
        body.contains(r#"<meta name="moss-not-found" content="1">"#),
        "404 page must carry the moss-not-found marker for the rename retry"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_moss_health_endpoint_returns_marker() {
    // Verify the /__moss_health/ endpoint returns the moss-specific marker.
    // This is what `verify_server_ready` checks to distinguish a moss
    // server from a foreign dev server (eleventy, vite, jekyll, ...).
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Plant a real index.html so ServeDir has something to serve at /
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<html><body>Hi</body></html>",
    )
    .unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 61000)
    })
    .await
    .expect("Server should start");

    // Hit the health endpoint via 127.0.0.1 literal (matches verify_server_ready)
    let url = format!("http://127.0.0.1:{}/__moss_health/", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Health endpoint should respond");

    assert_eq!(response.status(), 200, "Health endpoint should return 200");
    let content_type = response.header("content-type").unwrap_or("");
    assert!(
        content_type.starts_with("application/json"),
        "Health response should be application/json, got: {}",
        content_type
    );

    let body = response.into_string().expect("Should read body");
    assert!(
        body.contains("moss-preview-server"),
        "Health body must contain the moss-preview-server marker, got: {}",
        body
    );
    assert!(
        body.contains(env!("CARGO_PKG_VERSION")),
        "Health body should include CARGO_PKG_VERSION, got: {}",
        body
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_moss_health_route_wins_over_servedir() {
    // Verify that even if the user's site has a file at
    // `__moss_health/index.html`, the health route still wins (because
    // it's registered before `.fallback(ServeDir)`).
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    // Plant a colliding file: try to shadow the health endpoint
    let shadow_dir = temp_dir.path().join("__moss_health");
    std::fs::create_dir_all(&shadow_dir).unwrap();
    std::fs::write(shadow_dir.join("index.html"), "SHADOWED-BY-USER").unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 61500)
    })
    .await
    .expect("Server should start");

    let url = format!("http://127.0.0.1:{}/__moss_health/", port);
    let response = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("Health endpoint should respond");

    let body = response.into_string().expect("Should read body");
    assert!(
        body.contains("moss-preview-server"),
        "Health route must win over user file; got: {}",
        body
    );
    assert!(
        !body.contains("SHADOWED-BY-USER"),
        "User file must NOT shadow the health endpoint; got: {}",
        body
    );

    let _ = shutdown_tx.send(());
}

/// Synthesizes the dual-stack-collision bug at the bind layer and asserts
/// the bind-with-scan helper hops to the next port without leaving moss
/// in the half-bound state that the old `is_port_available → bind` path
/// could produce (the original TOCTOU window).
///
/// `#[ignore]`-gated for the same reason as the port-level IPv6 test:
/// sandboxed CI runners may not grant IPv6 loopback bind. Run locally:
///   cargo test -p moss --lib preview::server::router::tests::test_bind_dual_stack_scan_skips_v6_collision -- --ignored
#[tokio::test]
#[ignore = "requires IPv6 ::1 bind capability; not all sandboxed CI runners grant it"]
async fn test_bind_dual_stack_scan_skips_v6_collision() {
    // Pick a candidate port that's fully free on both stacks at start of test.
    let start_port = (62000..62100)
        .find(|&candidate| {
            let v4 = std::net::TcpListener::bind(("127.0.0.1", candidate));
            let v6 = std::net::TcpListener::bind(("::1", candidate));
            v4.is_ok() && v6.is_ok()
        })
        .expect(
            "no port in 62000..62100 was bindable on both IPv4 and IPv6; \
                 IPv6 loopback may be disabled on this host (re-run without --ignored)",
        );

    // Hold ONLY [::1]:start_port to simulate eleventy's `*:port` IPv6
    // wildcard scenario. IPv4 127.0.0.1:start_port is intentionally free —
    // this is exactly the configuration where a naive is_port_available
    // that probed only IPv4 would have returned `true`, the bind-IPv4
    // step would succeed, the bind-IPv6 step inside the spawned task
    // would fail, and the outer Result was Err with the original code.
    let _v6_holder = std::net::TcpListener::bind(("::1", start_port))
        .expect("Should bind IPv6 loopback for the test");

    // The bind-with-scan helper must skip `start_port` and return a
    // later port with both listeners cleanly bound.
    let (bound_port, v4, v6) = bind_dual_stack_with_scan(start_port)
        .await
        .expect("bind_dual_stack_with_scan must succeed by hopping past the IPv6-held port");

    assert!(
        bound_port > start_port,
        "Expected bound_port > start_port ({}), got {} — scan didn't hop past the collision",
        start_port,
        bound_port
    );
    assert_eq!(
        v4.local_addr().unwrap().port(),
        bound_port,
        "IPv4 listener should be on the bound port"
    );
    assert_eq!(
        v6.local_addr().unwrap().port(),
        bound_port,
        "IPv6 listener should be on the bound port"
    );

    // Drop listeners explicitly so we can verify the held port is still
    // held by `_v6_holder` and nothing else changed about it.
    drop(v4);
    drop(v6);
    assert!(
        std::net::TcpListener::bind(("::1", start_port)).is_err(),
        "IPv6 [::1]:{} must still be held by _v6_holder",
        start_port
    );
}

/// A page URL names a directory, not a file. `is_still_in_the_cloud` is false
/// for a directory, so checking the joined path answered "not evicted" for
/// every page on the site — including `/` — and let tower-http's bodyless 500
/// through. That is the single most likely way a user meets this code path:
/// the sync client evicting moss's own `index.html` out of `.moss/build.nosync/`.
#[cfg(target_os = "macos")]
#[test]
fn a_directory_url_is_resolved_to_its_index_before_the_cloud_check() {
    use std::io::Write;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("about")).unwrap();
    for rel in ["index.html", "about/index.html"] {
        let mut f = std::fs::File::create(root.join(rel)).unwrap();
        f.write_all(b"<html></html>").unwrap();
    }

    // Nothing is evicted, so every shape must decline — this is the guard that
    // proves the assertions below measure eviction and not merely "returns
    // Some for a directory".
    for url in ["/", "/about/", "/index.html"] {
        assert!(
            super::cloud_offline_response(root, url, http::StatusCode::INTERNAL_SERVER_ERROR)
                .is_none(),
            "{url} is fully local; nothing to report"
        );
    }

    // A non-500 status is never this function's business, whatever the path.
    assert!(
        super::cloud_offline_response(root, "/", http::StatusCode::NOT_FOUND).is_none()
    );
}

/// One assertion for the invariant that `/__moss/source/` serves the VAULT's
/// bytes, never the build output's.
///
/// The fixture puts a different file at the same relative path in both places,
/// so the test can tell them apart. Wiring this route to the served site dir —
/// the obvious mistake, since that is the only path the router already had —
/// returns "BUILT" and fails here. A test that only checked "some bytes came
/// back" would pass in both worlds and prove nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_route_serves_vault_bytes_not_build_output() {
    // Rooted in the crate dir, not /tmp: `resolve_scoped` canonicalizes, and on
    // macOS /tmp is a symlink to /private/tmp (the sibling test in
    // source_asset_protocol.rs does the same for the same reason).
    let vault = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let vault_path = vault.path();

    // `.moss/` is the vault marker VaultRoot::find_containing walks up to.
    let site_dir = vault_path.join(".moss/build.nosync/current");
    std::fs::create_dir_all(site_dir.join("assets")).unwrap();
    std::fs::create_dir_all(vault_path.join("assets")).unwrap();
    std::fs::write(vault_path.join("assets/photo.txt"), b"SOURCE").unwrap();
    std::fs::write(site_dir.join("assets/photo.txt"), b"BUILT").unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(site_dir.clone()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58600)
    })
    .await
    .expect("Server should start");

    let body = ureq::get(&format!("http://localhost:{}/__moss/source/assets/photo.txt", port))
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("source route should answer")
        .into_string()
        .expect("body should read");

    assert_eq!(
        body, "SOURCE",
        "/__moss/source/ must serve the vault file; got the build artifact instead"
    );

    let _ = shutdown_tx.send(());
}

/// The containment check survives the HTTP carrier.
///
/// `%2e%2e` rather than a literal `..`: a literal one is normalized away by the
/// client or the router before any moss code sees it, so it would test the
/// stack's path handling instead of `resolve_scoped`'s. Percent-encoded, the
/// `..` reaches the resolver intact — which is the code path that has to refuse.
///
/// The positive control before it is not decoration. A 404 is also what an
/// UNMATCHED route returns here (the request falls through to `ServeDir`), so
/// asserting only the escape would pass just as well if the route had never
/// been registered — a green that proves nothing. The control uses a
/// percent-encoded in-vault path, so one request establishes both that the
/// wildcard route matches and that decoding reaches the resolver; only then
/// does the 404 below mean "the resolver refused."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_route_rejects_escape_above_the_vault() {
    let parent = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::write(parent.path().join("secret.txt"), b"SECRET").unwrap();

    let vault_path = parent.path().join("vault");
    let site_dir = vault_path.join(".moss/build.nosync/current");
    std::fs::create_dir_all(&site_dir).unwrap();
    std::fs::write(vault_path.join("inside.txt"), b"INSIDE").unwrap();

    let site_dir_state = Arc::new(std::sync::RwLock::new(site_dir.clone()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58700)
    })
    .await
    .expect("Server should start");

    // Positive control: `%69` is 'i'. If this does not return INSIDE, the
    // wildcard route is not matching and the escape assertion below is vacuous.
    let control = ureq::get(&format!("http://localhost:{}/__moss/source/%69nside.txt", port))
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("control: the source route should answer a percent-encoded in-vault path")
        .into_string()
        .expect("control body should read");
    assert_eq!(
        control, "INSIDE",
        "control: route matched but served the wrong bytes — the escape check below would be meaningless"
    );

    let result = ureq::get(&format!(
        "http://localhost:{}/__moss/source/%2e%2e/secret.txt",
        port
    ))
    .timeout(std::time::Duration::from_secs(5))
    .call();

    match result {
        Err(ureq::Error::Status(404, _)) => {}
        Ok(resp) => panic!(
            "escape must 404; got {} with body {:?}",
            resp.status(),
            resp.into_string().unwrap_or_default()
        ),
        Err(e) => panic!("expected a 404, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

/// The trust boundary rejects a rebound `Host` end-to-end (the pure logic is
/// unit-tested in `trust_boundary.rs`; this proves the layer is actually wired
/// into the router). Raw TCP, because a rebound browser sends the attacker's
/// hostname in `Host` while the socket still lands on our loopback port — and
/// ureq manages `Host` from the URL, so only a hand-written request reproduces
/// what the attack looks like on the wire.
///
/// The `localhost` request is the positive control: without it, a 421 on every
/// request would pass just as well if the layer were refusing everything.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trust_boundary_refuses_a_rebound_host_but_serves_localhost() {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let temp_dir = tempfile::TempDir::new().unwrap();
    std::fs::write(temp_dir.path().join("index.html"), b"<html>ok</html>").unwrap();
    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58800)
    })
    .await
    .expect("Server should start");

    let status_line = |host: &str| -> String {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream
            .write_all(
                format!("GET /__moss_health/ HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        resp.lines().next().unwrap_or("").to_string()
    };

    // Positive control: the socket and the route both work for a loopback Host.
    assert!(
        status_line("localhost").contains("200"),
        "control: a loopback Host must be served; the refusal below would be vacuous otherwise"
    );

    // The rebinding attack: same socket, attacker's hostname in Host → 421.
    assert!(
        status_line("evil.com").contains("421"),
        "a rebound (non-loopback) Host must be refused with 421"
    );

    let _ = shutdown_tx.send(());
}

/// The trust boundary rejects a foreign `Origin` with 403 — an ordinary
/// cross-site request, before rebinding is even attempted. ureq lets us set
/// `Origin` freely (unlike `Host`), so this exercises the Origin arm directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trust_boundary_refuses_a_foreign_origin() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    std::fs::write(temp_dir.path().join("index.html"), b"<html>ok</html>").unwrap();
    let site_dir_state = Arc::new(std::sync::RwLock::new(temp_dir.path().to_path_buf()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        ..ServeConfig::new(site_dir_state, 58900)
    })
    .await
    .expect("Server should start");

    let base = format!("http://localhost:{}/__moss_health/", port);

    // Positive control: no Origin (a top-level navigation / health check) passes.
    ureq::get(&base)
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .expect("control: an origin-less request must pass");

    // A foreign Origin is refused with 403.
    match ureq::get(&base)
        .set("Origin", "http://evil.com")
        .timeout(std::time::Duration::from_secs(5))
        .call()
    {
        Err(ureq::Error::Status(403, _)) => {}
        Ok(resp) => panic!("foreign Origin must 403; got {}", resp.status()),
        Err(e) => panic!("expected a 403, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

// ===== Read-only HTTP command carrier: POST /__moss/invoke/<cmd> =====

/// A read-only command invoked over `POST /__moss/invoke/<cmd>` with a JSON
/// body returns 200 + the command's real JSON result. `scan_shortcodes` is
/// pure, so once the carrier has a vault bound this isolates the plumbing:
/// route → args deserialize → SAME command body → JSON out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invoke_carrier_runs_a_read_only_command_over_http() {
    let (_vault, site_dir) = served_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir, 62200).await;

    let payload = serde_json::json!({ "text": "# Title\n\nplain body\n" }).to_string();
    let url = format!("http://localhost:{}/__moss/invoke/scan_shortcodes", port);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
        .expect("invoke of a read-only command must return 200");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("read body")).expect("json body");
    assert!(
        body.is_object(),
        "scan_shortcodes returns an EditorScanResult object; got: {body}"
    );

    let _ = shutdown_tx.send(());
}

/// A `State<'_, AppState>` command works over the carrier too: the project root
/// is derived from the server's live `site_dir` (`<vault>/.moss/build.nosync/current`),
/// so `editor_resolve_asset` resolves an asset that lives in the vault — proving
/// the InvokeCtx project-root threading, not just pure-args plumbing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invoke_carrier_resolves_an_asset_using_the_derived_project_root() {
    use tempfile::TempDir;

    // Rooted in the crate dir (not /tmp): resolve_asset canonicalizes, and on
    // macOS /tmp is a symlink — the sibling source-route test does the same.
    let vault = TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let site_dir = vault.path().join(".moss/build.nosync/current");
    std::fs::create_dir_all(&site_dir).unwrap();
    std::fs::create_dir_all(vault.path().join("assets")).unwrap();
    std::fs::write(vault.path().join("assets/hero.jpg"), b"JPGBYTES").unwrap();
    std::fs::write(vault.path().join("index.md"), b"# home").unwrap();

    let (port, shutdown_tx, _token) = serve_bound(site_dir.clone(), 62300).await;

    let from_file = vault.path().join("index.md");
    let payload = serde_json::json!({
        "target": "hero.jpg",
        "fromFile": from_file.to_string_lossy(),
    })
    .to_string();

    let url = format!("http://localhost:{}/__moss/invoke/editor_resolve_asset", port);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
        .expect("editor_resolve_asset over the carrier must return 200");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("read body")).expect("json body");
    assert!(
        !body.is_null(),
        "the asset lives in the vault, so resolution must be Some — a null answer \
         means the project root was NOT derived from site_dir; got: {body}"
    );

    let _ = shutdown_tx.send(());
}

/// A command that is NOT in the read-only allowlist is ABSENT from the carrier:
/// the router returns 404 (unexposed is not gated, it does not
/// exist here), never a runtime 403. `reveal_entry` is a real registered command,
/// so this proves the *allowlist* subsets the registry, not merely that unknown
/// strings 404.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invoke_carrier_404s_a_command_not_on_the_allowlist() {
    let (_vault, site_dir) = served_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir, 62400).await;

    let url = format!("http://localhost:{}/__moss/invoke/reveal_entry", port);
    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(r#"{}"#)
    {
        Err(ureq::Error::Status(404, _)) => {}
        Ok(resp) => panic!("an unexposed command must 404; got {}", resp.status()),
        Err(e) => panic!("expected a 404, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

/// The invoke route is genuinely behind the outermost trust-boundary layer: a
/// foreign `Origin` is refused with 403 BEFORE the handler (or the allowlist)
/// ever runs. The positive control (a scan over the carrier) proves the refusal
/// is not vacuous.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invoke_carrier_is_behind_the_trust_boundary() {
    let (_vault, site_dir) = served_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir, 62500).await;

    let url = format!("http://localhost:{}/__moss/invoke/scan_shortcodes", port);

    // Positive control: without a foreign Origin the carrier answers 200.
    let ok = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(r#"{"text":"x"}"#)
        .expect("control: the carrier must answer an origin-less request");
    assert_eq!(ok.status(), 200);

    // A foreign Origin is refused with 403 before the handler runs.
    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Origin", "http://evil.com")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(r#"{"text":"x"}"#)
    {
        Err(ureq::Error::Status(403, _)) => {}
        Ok(resp) => panic!("foreign Origin must 403 on the invoke route; got {}", resp.status()),
        Err(e) => panic!("expected a 403, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

// ===== Token-gated HTTP mutation carrier: POST /__moss/mutate/<cmd> =====

/// Stand up a vault fixture whose site dir is `<vault>/.moss/build.nosync/current`, so
/// `VaultRoot::find_containing` (walked up by the carrier to derive the project
/// root) resolves to the vault. Returns the TempDir (kept alive by the caller)
/// and the site_dir path. Rooted in the crate dir, not /tmp: create_files /
/// resolve canonicalize, and on macOS /tmp is a symlink (the source-route tests
/// do the same).
#[cfg(test)]
fn served_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let vault = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let site_dir = vault.path().join(".moss/build.nosync/current");
    std::fs::create_dir_all(&site_dir).unwrap();
    (vault, site_dir)
}

/// Start a server over `site_dir` with the carrier mounted, and return the
/// token the router minted when it bound the served vault at start-up.
async fn serve_bound(
    site_dir: std::path::PathBuf,
    port: u16,
) -> (u16, tokio::sync::oneshot::Sender<()>, String) {
    let ctx = crate::ops::serve::invoke::InvokeCtx::standalone();
    let (port, shutdown_tx) = start_server(ServeConfig {
        invoke: Some(ctx.clone()),
        ..ServeConfig::new(Arc::new(std::sync::RwLock::new(site_dir)), port)
    })
    .await
    .expect("Server should start");
    let token = ctx.token().expect("start_server binds the served vault").to_string();
    (port, shutdown_tx, token)
}

/// A save with a VALID token persists the exact serialized bytes to disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutate_carrier_with_valid_token_persists_editor_bytes() {
    let (vault, site_dir) = served_vault();
    std::fs::write(vault.path().join("index.md"), "---\nx: 1\n---\nold").unwrap();

    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 62700).await;

    let target = vault.path().join("index.md");
    let payload = serde_json::json!({
        "filePath": target.to_string_lossy(),
        "frontmatter": { "title": "Hello" },
        "body": "New **body** text.",
    })
    .to_string();
    let url = format!("http://localhost:{}/__moss/mutate/save_editor_content", port);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
        .expect("save_editor_content over the mutation carrier must return 200");
    assert_eq!(resp.status(), 200);

    let on_disk = std::fs::read_to_string(&target).unwrap();
    assert!(
        on_disk.contains("New **body** text."),
        "the saved body must be on disk, got: {on_disk}"
    );
    assert!(
        on_disk.contains("title: Hello"),
        "the saved frontmatter must be on disk, got: {on_disk}"
    );

    let _ = shutdown_tx.send(());
}

/// No token → 401. Wrong token → 401. The floor is loopback and same-origin, so
/// these requests pass the trust boundary; the token gate is what stops them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutate_carrier_without_or_with_wrong_token_is_401() {
    let (vault, site_dir) = served_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir, 62800).await;

    let url = format!("http://localhost:{}/__moss/mutate/create_files", port);
    let payload = serde_json::json!({
        "files": [{ "dir": vault.path().to_string_lossy(), "name": "should-not-exist", "frontmatter": {} }],
    })
    .to_string();

    // No token at all.
    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
    {
        Err(ureq::Error::Status(401, _)) => {}
        Ok(resp) => panic!("missing token must 401; got {}", resp.status()),
        Err(e) => panic!("expected a 401, got transport error: {e}"),
    }

    // A wrong token.
    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", "not-the-real-token")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
    {
        Err(ureq::Error::Status(401, _)) => {}
        Ok(resp) => panic!("wrong token must 401; got {}", resp.status()),
        Err(e) => panic!("expected a 401, got transport error: {e}"),
    }

    // The gate must have prevented the write.
    assert!(
        !vault.path().join("should-not-exist.md").exists(),
        "a 401'd mutation must not have created any file"
    );

    let _ = shutdown_tx.send(());
}

/// A token is bound to the vault it was minted for. Point the
/// SAME server at a second vault — the reused-server folder switch the app
/// performs by rewriting `site_dir` — and the first vault's token is 401 on the
/// very next request, while the token the switch published under the second
/// vault's `.moss/build.nosync/` is accepted. Before this, the token was minted once
/// per server while the root moved underneath it, so a forgotten tab followed
/// the server into whatever folder it was next pointed at.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn folder_switch_retires_the_previous_vaults_token() {
    let (vault_a, site_a) = served_vault();
    let (vault_b, site_b) = served_vault();
    let ctx = crate::ops::serve::invoke::InvokeCtx::standalone();

    let site_dir_state = Arc::new(std::sync::RwLock::new(site_a.clone()));
    let (port, shutdown_tx) = start_server(ServeConfig {
        invoke: Some(ctx.clone()),
        ..ServeConfig::new(site_dir_state.clone(), 63950)
    })
    .await
    .expect("Server should start");
    let token_a = ctx.token().expect("start_server binds the served vault").to_string();
    // The handle the app switches folders through.
    let handle = crate::types::runtime::ServerHandle {
        port,
        shutdown_tx: Some(shutdown_tx),
        site_dir: site_dir_state,
        carrier: Some(ctx),
    };

    let url = format!("http://localhost:{}/__moss/mutate/create_files", port);
    let create_in = |vault: &tempfile::TempDir, token: &str| {
        ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("X-Moss-Token", token)
            .timeout(std::time::Duration::from_secs(5))
            .send_string(
                &serde_json::json!({
                    "files": [{ "dir": vault.path().to_string_lossy(), "name": "note", "frontmatter": {} }]
                })
                .to_string(),
            )
    };

    // Sanity: A's token works while A is served.
    create_in(&vault_a, &token_a).expect("A's token must be accepted while A is served");
    assert!(vault_a.path().join("note.md").exists());

    // An SSE subscriber admitted under A. Its stream must END on the switch
    // itself, not keep delivering B's events to a holder of A's token until
    // some later carrier request happens to observe the switch. Response
    // headers arrive only after the handler has subscribed to the bus, so
    // "headers received" is the proof the subscription landed — no sleep and
    // no process-global subscriber count, which a sibling test could move.
    let events_url = format!("http://localhost:{}/__moss/events", port);
    let (subscribed_tx, subscribed_rx) = std::sync::mpsc::channel();
    let subscriber = {
        let token_a = token_a.clone();
        std::thread::spawn(move || {
            let resp = ureq::get(&events_url)
                .set("X-Moss-Token", &token_a)
                .timeout(std::time::Duration::from_secs(10))
                .call()
                .expect("A's subscriber is admitted while A is served");
            subscribed_tx.send(()).expect("test body is waiting");
            let mut body = String::new();
            std::io::Read::read_to_string(&mut resp.into_reader(), &mut body).map(|_| body)
        })
    };
    subscribed_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the subscription never landed");

    // The folder switch: the server now serves B.
    handle.point_at(site_b.clone());

    let ended = subscriber.join().expect("subscriber thread");
    assert!(ended.is_ok(), "A's SSE stream must end on the switch, got {ended:?}");

    match create_in(&vault_b, &token_a) {
        Err(ureq::Error::Status(401, _)) => {}
        Ok(resp) => panic!("A's token must be refused once B is served; got {}", resp.status()),
        Err(e) => panic!("expected a 401, got transport error: {e}"),
    }
    assert!(
        !vault_b.path().join("note.md").exists(),
        "the stale token must not have written into the new vault"
    );

    // The switch published B's token where a local client reads it.
    let token_b = std::fs::read_to_string(crate::ops::serve::carrier_token::token_path(vault_b.path()))
        .expect("the switch publishes B's token under B's build dir");
    assert_ne!(token_a, token_b);
    create_in(&vault_b, &token_b).expect("B's token must be accepted");
    assert!(vault_b.path().join("note.md").exists());

    drop(handle);
}

/// A VALID token but a FOREIGN Origin → 403, NOT 401. Proves the trust boundary
/// runs BEFORE the token gate: the cross-origin request is refused before the
/// token is ever inspected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutate_carrier_with_valid_token_but_foreign_origin_is_403() {
    let (vault, site_dir) = served_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 62900).await;

    let url = format!("http://localhost:{}/__moss/mutate/create_files", port);
    let payload = serde_json::json!({
        "files": [{ "dir": vault.path().to_string_lossy(), "name": "origin-blocked", "frontmatter": {} }],
    })
    .to_string();

    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .set("Origin", "http://evil.com")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
    {
        Err(ureq::Error::Status(403, _)) => {}
        Ok(resp) => panic!("a foreign Origin must 403 even with a valid token; got {}", resp.status()),
        Err(e) => panic!("expected a 403, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

/// Method + content-type gating: a GET to a mutation route is 405 (method
/// routing), and a POST with a non-JSON body is 415 (the content-type gate),
/// even with a valid token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutate_carrier_rejects_get_and_wrong_content_type() {
    let (_vault, site_dir) = served_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63000).await;

    let url = format!("http://localhost:{}/__moss/mutate/create_files", port);

    // GET → 405 Method Not Allowed (the route is POST-only).
    match ureq::get(&url)
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .call()
    {
        Err(ureq::Error::Status(405, _)) => {}
        Ok(resp) => panic!("GET on a mutation route must 405; got {}", resp.status()),
        Err(e) => panic!("expected a 405, got transport error: {e}"),
    }

    // POST with a valid token but a non-JSON Content-Type → 415.
    match ureq::post(&url)
        .set("Content-Type", "text/plain")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string("not json")
    {
        Err(ureq::Error::Status(415, _)) => {}
        Ok(resp) => panic!("a non-JSON body must 415; got {}", resp.status()),
        Err(e) => panic!("expected a 415, got transport error: {e}"),
    }

    let _ = shutdown_tx.send(());
}

/// No regression: a read-only command still works WITHOUT any token, on a server
/// that ALSO exposes the token-gated mutation carrier. Proves the read tier
/// stays public even once mutations exist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_only_carrier_needs_no_token_even_with_mutations_enabled() {
    let (_vault, site_dir) = served_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir.clone(), 63100).await;

    let url = format!("http://localhost:{}/__moss/invoke/scan_shortcodes", port);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&serde_json::json!({ "text": "# hi" }).to_string())
        .expect("a read-only command must answer 200 with no token");
    assert_eq!(resp.status(), 200);

    let _ = shutdown_tx.send(());
}

// ===== Token-gated HTTP authed-read carrier: POST /__moss/read/<cmd> =====

/// `validate_content` over the read carrier returns real diagnostics, not just
/// a 200.
///
/// The falsifier can only see this command 404 — it asserts zero console
/// errors, and the consumer (`ChipDiagnostics`) paints whatever comes back
/// with no error surface, so a 200 carrying the wrong answer is invisible
/// there. This is the layer that can see the answer.
///
/// Not asserted here: an absent file. It 500s, because the arm propagates the
/// read error exactly as the desktop command does — the `Ok(None)` early
/// return is for a CLOUD-EVICTED source, which needs a dataless file to
/// reproduce. Writing a browser-only "absent is empty" rule would be the fork
/// this tier exists to prevent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_carrier_validate_content_returns_diagnostics_for_a_bad_field() {
    let (vault, site_dir) = served_vault();
    // `date` is a builtin field typed as a date; a bare string is wrong, and
    // `bogus` is in no schema at all.
    std::fs::write(
        vault.path().join("index.md"),
        "---\ntitle: Boot\nbogus: 1\n---\nbody",
    )
    .unwrap();

    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63900).await;

    let url = format!("http://localhost:{}/__moss/read/validate_content", port);
    let source = vault.path().join("index.md").to_string_lossy().to_string();
    let body = serde_json::json!({ "filePath": source }).to_string();

    // Token-gated like every other authed read.
    match ureq::post(&url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&body)
    {
        Err(ureq::Error::Status(401, _)) => {}
        Ok(resp) => panic!("validate_content must 401 without a token; got {}", resp.status()),
        Err(e) => panic!("expected a 401, got transport error: {e}"),
    }

    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&body)
        .expect("validate_content over the read carrier must return 200");
    let diags: Vec<serde_json::Value> = resp.into_json().expect("a diagnostic list");
    assert!(
        diags.iter().any(|d| d["message"].as_str().is_some_and(|m| m.contains("bogus"))),
        "the unknown field must reach the browser as a diagnostic: {diags:?}"
    );

    let _ = shutdown_tx.send(());
}

/// The editor's boot reads are gated behind the SAME token as mutations, and a
/// valid token boots the real editor payload. This is the security-relevant
/// claim of the authed-read tier (the whole vault tree is reachable through it,
/// so it is not a public read) AND the functional claim (the boot payload flows
/// and the initial file is pre-parsed by `bootstrap_for_carrier`).
///
/// One server, three requests: no token → 401 (the gate); a valid token →
/// `editor_bootstrap` returns the boot payload with the initial file parsed; and
/// `list_directory` (the file-tree read the editor boots against) returns the
/// vault's entries. `list_directory`'s presence here proves the whole editor
/// boot+open read set is reachable, not just bootstrap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_carrier_gates_editor_boot_reads_behind_the_token() {
    let (vault, site_dir) = served_vault();
    std::fs::write(
        vault.path().join("index.md"),
        "---\ntitle: Boot\n---\nhello from the vault",
    )
    .unwrap();

    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63400).await;

    let source = vault.path().join("index.md").to_string_lossy().to_string();

    // 1) No token → 401. The editor boot read is NOT public.
    let boot_url = format!("http://localhost:{}/__moss/read/editor_bootstrap", port);
    match ureq::post(&boot_url)
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&serde_json::json!({ "sourcePath": source }).to_string())
    {
        Err(ureq::Error::Status(401, _)) => {}
        Ok(resp) => panic!("an editor boot read must 401 without a token; got {}", resp.status()),
        Err(e) => panic!("expected a 401, got transport error: {e}"),
    }

    // 2) Valid token → 200 with the boot payload; the initial file is pre-parsed.
    let resp = ureq::post(&boot_url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&serde_json::json!({ "sourcePath": source }).to_string())
        .expect("editor_bootstrap over the read carrier must return 200 with a valid token");
    assert_eq!(resp.status(), 200);
    let boot: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("read body")).expect("json body");
    assert!(boot.get("schema").is_some(), "boot payload must carry a schema: {boot}");
    assert_eq!(
        boot["initial_file"]["frontmatter"]["title"], "Boot",
        "bootstrap must pre-parse the initial file's frontmatter: {boot}"
    );

    // 3) Valid token → 200 for list_directory, the file-tree boot read.
    let list_url = format!("http://localhost:{}/__moss/read/list_directory", port);
    let resp = ureq::post(&list_url)
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(
            &serde_json::json!({
                "path": vault.path().to_string_lossy(),
                "projectPath": vault.path().to_string_lossy(),
                "showInternal": false,
            })
            .to_string(),
        )
        .expect("list_directory over the read carrier must return 200 with a valid token");
    assert_eq!(resp.status(), 200);
    let entries = resp.into_string().expect("read body");
    assert!(
        entries.contains("index.md"),
        "the vault listing must include the file we wrote, got: {entries}"
    );

    let _ = shutdown_tx.send(());
}

// ===== THE HARD GATE: dual-path parity (pure core vs HTTP arm) =====

/// `create_files`: the HTTP arm produces the byte-for-byte same on-disk effect as
/// the pure core the Tauri command uses (`vault::fs::create_files_inner`). Two
/// identical fresh vaults, the SAME args applied through each path, and the
/// resulting file tree is asserted byte-identical.
///
/// CAVEAT (documented, not a gap): a TRUE dual-invocation — calling the actual
/// `#[tauri::command] create_files` — is impossible here because it takes
/// `State<'_, AppState>`, and `tauri::State` has a private field so it is
/// unconstructible outside a running Tauri runtime (the identical constraint
/// slice 1 documented for the read-only `State` arms). The command body is a
/// one-line passthrough to `create_files_inner`, so asserting the HTTP
/// arm's effect equals that core's effect is the strongest available parity:
/// it pins that the hand-wired arm matches the exact core the command runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dual_path_parity_create_files_effect_matches_the_pure_core() {
    use crate::vault::fs::{create_files_inner, NewFile};
    let note = |vault: &std::path::Path| NewFile {
        dir: vault.to_string_lossy().to_string(),
        name: "parity-note".into(),
        frontmatter: serde_json::Map::new(),
    };
    // Path A — the pure core the Tauri command calls, invoked directly.
    let (vault_a, _site_a) = served_vault();
    let core_created = create_files_inner(vault_a.path(), &[note(vault_a.path())])
        .expect("core create_files_inner must succeed")
        .remove(0);

    // Path B — the HTTP mutation arm, same args, in an identical fresh vault.
    let (vault_b, site_b) = served_vault();
    let (port, shutdown_tx, token) = serve_bound(site_b.clone(), 63200).await;
    let payload = serde_json::json!({ "files": [note(vault_b.path())] }).to_string();
    let resp = ureq::post(&format!("http://localhost:{}/__moss/mutate/create_files", port))
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
        .expect("HTTP create_files must return 200");
    assert_eq!(resp.status(), 200);
    // The arm's RETURN VALUE, not just its effect: the file tree selects the
    // new row by the path the command hands back, so a wrong string is a real
    // bug that an exists-check cannot see.
    let returned: String = resp.into_json::<Vec<String>>().expect("arm returns the created paths").remove(0);
    let _ = shutdown_tx.send(());
    assert_eq!(
        std::path::Path::new(&returned)
            .strip_prefix(vault_b.path())
            .expect("returned path is under vault_b"),
        std::path::Path::new("parity-note.md"),
    );

    // Equal EFFECT: the same relative path exists under each vault, with
    // byte-identical content.
    let rel_a = std::path::Path::new(&core_created)
        .strip_prefix(vault_a.path())
        .expect("core path is under vault_a");
    let http_created = vault_b.path().join("parity-note.md");
    assert!(http_created.exists(), "HTTP arm must have created the file");
    assert_eq!(
        rel_a,
        std::path::Path::new("parity-note.md"),
        "both paths append .md to the same relative name"
    );
    assert_eq!(
        std::fs::read(&core_created).unwrap(),
        std::fs::read(&http_created).unwrap(),
        "the two carriers must write byte-identical file content"
    );
}

/// `create_folder`'s arm against the same core the desktop command calls.
/// Same shape as the `create_files` parity test above: a folder the editor can
/// make in the app but not in a browser is the half-carried surface the tier
/// expansion exists to close.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dual_path_parity_create_folder_effect_matches_the_pure_core() {
    // Path A — the pure core the Tauri command calls, invoked directly.
    let (vault_a, _site_a) = served_vault();
    let core_created = crate::vault::fs::create_folder_inner(
        vault_a.path(),
        &vault_a.path().to_string_lossy(),
        "parity-folder",
    )
    .expect("core create_folder_inner must succeed");

    // Path B — the HTTP mutation arm, same args, in an identical fresh vault.
    let (vault_b, site_b) = served_vault();
    let (port, shutdown_tx, token) = serve_bound(site_b.clone(), 63800).await;
    // The wire spelling is camelCase (`parentDir`), as bindings.ts sends it.
    let payload = serde_json::json!({
        "parentDir": vault_b.path().to_string_lossy(),
        "name": "parity-folder",
    })
    .to_string();
    let resp = ureq::post(&format!("http://localhost:{}/__moss/mutate/create_folder", port))
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(&payload)
        .expect("HTTP create_folder must return 200");
    assert_eq!(resp.status(), 200);
    let returned: String = resp.into_json().expect("arm returns the created path");
    let _ = shutdown_tx.send(());
    assert_eq!(
        std::path::Path::new(&returned)
            .strip_prefix(vault_b.path())
            .expect("returned path is under vault_b"),
        std::path::Path::new("parity-folder"),
    );

    // Equal EFFECT: the same relative directory exists under each vault.
    let rel_a = std::path::Path::new(&core_created)
        .strip_prefix(vault_a.path())
        .expect("core path is under vault_a");
    assert_eq!(rel_a, std::path::Path::new("parity-folder"));
    let http_created = vault_b.path().join("parity-folder");
    assert!(
        http_created.is_dir(),
        "the HTTP arm must have created a directory, not a file"
    );
}

// The `save_editor_content` dual-path parity test (HTTP arm vs the REAL Tauri
// command inner, `save_editor_content_with_task`) lives app-side in
// the desktop app's preview server tests: the command inner and its
// TaskRegistry wiring stay in the app crate, and the parity claim is about
// THAT code, so the test follows it.

// ===== Path confinement: the carrier is a trust boundary =====
//
// Every arm taking a path-shaped argument routes it through `invoke::confine`.
// These tests prove the boundary holds for the two primitives that matter most:
// the token-FREE read (the only unauthenticated path-taker) and the token-gated
// write. Each carries a positive control first — an escape assertion against a
// route that is not matching at all would be vacuous.

/// A vault with a secret file sitting NEXT to it, plus a real page inside.
/// Returns (parent, vault_path, site_dir) — `parent` must stay alive.
fn confinement_vault() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let parent = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::write(parent.path().join("secret.md"), "---\ntitle: SECRET\n---\ntop secret\n")
        .unwrap();

    let vault_path = parent.path().join("vault");
    let site_dir = vault_path.join(".moss/build.nosync/current");
    std::fs::create_dir_all(&site_dir).unwrap();
    std::fs::write(vault_path.join("inside.md"), "---\ntitle: INSIDE\n---\nin the vault\n")
        .unwrap();
    (parent, vault_path, site_dir)
}

/// The token-FREE tier must not read outside the vault.
///
/// `parse_frontmatter` is the only path-taking arm on `/__moss/invoke`, and its
/// shared core (`read_vault_text`) deliberately does not confine — the desktop
/// caller has no need to. So this arm's `confine` is the whole defence, and an
/// unauthenticated same-origin caller is exactly who it defends against.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_free_carrier_refuses_to_read_a_file_outside_the_vault() {
    let (parent, vault_path, site_dir) = confinement_vault();
    let (port, shutdown_tx, _token) = serve_bound(site_dir.clone(), 63400).await;

    let post = |file: std::path::PathBuf| {
        ureq::post(&format!("http://localhost:{}/__moss/invoke/parse_frontmatter", port))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_string(&serde_json::json!({ "filePath": file.to_string_lossy() }).to_string())
    };

    // Positive control: an IN-vault read must succeed, or the refusal below
    // proves nothing about confinement.
    let inside = post(vault_path.join("inside.md"))
        .expect("control: an in-vault read must succeed")
        .into_string()
        .unwrap();
    assert!(
        inside.contains("INSIDE"),
        "control: the arm answered but did not return the in-vault file's frontmatter: {inside}"
    );

    // The escape: a sibling of the vault, named absolutely.
    let escaped = post(parent.path().join("secret.md"));
    match escaped {
        Err(ureq::Error::Status(status, _)) => assert_eq!(
            status, 500,
            "an out-of-vault read must be refused as a command error"
        ),
        Ok(r) => panic!(
            "token-free carrier read a file OUTSIDE the vault: {}",
            r.into_string().unwrap_or_default()
        ),
        Err(e) => panic!("unexpected transport error: {e}"),
    }

    // And the traversal spelling of the same escape.
    let traversal = post(vault_path.join("../secret.md"));
    assert!(
        traversal.is_err(),
        "a `..` traversal out of the vault must be refused"
    );

    let _ = shutdown_tx.send(());
}

/// The mutation tier must not WRITE outside the vault. A valid session token
/// authenticates a caller; it does not widen where that caller may write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutate_carrier_refuses_to_write_outside_the_vault_even_with_a_valid_token() {
    let (parent, vault_path, site_dir) = confinement_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63500).await;

    let save = |file: std::path::PathBuf, body: &str| {
        ureq::post(&format!("http://localhost:{}/__moss/mutate/save_editor_content", port))
            .set("Content-Type", "application/json")
            .set("X-Moss-Token", &token)
            .timeout(std::time::Duration::from_secs(5))
            .send_string(
                &serde_json::json!({
                    "filePath": file.to_string_lossy(),
                    "frontmatter": { "title": "T" },
                    "body": body,
                })
                .to_string(),
            )
    };

    // Positive control: an in-vault write must land.
    let target = vault_path.join("inside.md");
    save(target.clone(), "rewritten by the carrier").expect("control: in-vault save must succeed");
    assert!(
        std::fs::read_to_string(&target).unwrap().contains("rewritten by the carrier"),
        "control: the save returned 200 but the bytes did not land"
    );

    // The escape: overwrite a file next to the vault.
    let outside = parent.path().join("secret.md");
    let before = std::fs::read_to_string(&outside).unwrap();
    let escaped = save(outside.clone(), "CLOBBERED");
    assert!(
        escaped.is_err(),
        "the mutation carrier wrote OUTSIDE the vault with only a session token"
    );
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        before,
        "the out-of-vault file must be byte-for-byte untouched"
    );

    // The second escape, which the lexical check above cannot see: the path is
    // spelled inside the vault, but a symlinked directory resolves it out.
    // The file does not exist yet, which is exactly the case `confine` used to
    // wave through.
    #[cfg(unix)]
    {
        let escape_dir = parent.path().join("escape-target");
        std::fs::create_dir_all(&escape_dir).unwrap();
        std::os::unix::fs::symlink(&escape_dir, vault_path.join("drafts")).unwrap();

        let via_symlink = vault_path.join("drafts").join("planted.md");
        let planted = save(via_symlink, "PLANTED");
        assert!(
            planted.is_err(),
            "the mutation carrier followed a symlinked vault directory out of the vault"
        );
        assert!(
            !escape_dir.join("planted.md").exists(),
            "a file was created outside the vault through a symlinked directory"
        );
    }

    let _ = shutdown_tx.send(());
}

/// `list_tree` and `list_directory` used to take the project root FROM THE
/// CALLER — so a containment check inside the core would have validated the
/// input against itself. Both now ignore the caller's root and walk the
/// carrier's own, which means a caller naming an outside directory gets the
/// vault, never that directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_carrier_walks_its_own_root_not_a_caller_supplied_one() {
    let (parent, _vault_path, site_dir) = confinement_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63600).await;

    // Ask for the PARENT directory (which holds secret.md) as the tree root.
    let body = ureq::post(&format!("http://localhost:{}/__moss/read/list_tree", port))
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", &token)
        .timeout(std::time::Duration::from_secs(5))
        .send_string(
            &serde_json::json!({
                "path": parent.path().to_string_lossy(),
                "showInternal": false,
            })
            .to_string(),
        )
        .expect("list_tree must answer")
        .into_string()
        .unwrap();

    assert!(
        body.contains("inside.md"),
        "the walk should have covered the carrier's own vault: {body}"
    );
    assert!(
        !body.contains("secret.md"),
        "list_tree honoured a caller-supplied root and walked OUTSIDE the vault: {body}"
    );

    let _ = shutdown_tx.send(());
}

// ── The event carrier (`GET /__moss/events`) ─────────────────────────────────

/// Read one SSE record (up to the blank line) off a blocking reader, or give up.
///
/// Deliberately hand-rolled rather than pulled from a crate: the frame format
/// is the contract under test, so a parser that shares code with the one in
/// `listen.ts` would be marking its own homework.
fn read_one_sse_record(mut reader: impl std::io::Read) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < 64 * 1024 {
        match reader.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\n\n") {
                    return Some(String::from_utf8_lossy(&buf).into_owned());
                }
            }
            Err(_) => return None,
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn event_stream_refuses_a_subscriber_with_no_token() {
    let (_parent, _vault_path, site_dir) = confinement_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63410).await;

    let url = format!("http://localhost:{port}/__moss/events");

    // Positive control FIRST: with the token the stream opens, so the refusal
    // below cannot be passing because the route is simply absent.
    let good = tokio::task::spawn_blocking({
        let url = url.clone();
        let token = token.clone();
        move || {
            ureq::get(&url)
                .set("X-Moss-Token", &token)
                .timeout(std::time::Duration::from_secs(5))
                .call()
        }
    })
    .await
    .unwrap()
    .expect("control: a token-bearing subscriber must be accepted");
    assert_eq!(good.status(), 200);
    assert!(
        good.header("content-type").unwrap_or("").starts_with("text/event-stream"),
        "the stream must announce itself as SSE, got {:?}",
        good.header("content-type")
    );
    drop(good);

    for (label, req) in [
        ("no header at all", None),
        ("a wrong token", Some("not-the-token")),
    ] {
        let attempt = tokio::task::spawn_blocking({
            let url = url.clone();
            move || {
                let r = ureq::get(&url).timeout(std::time::Duration::from_secs(5));
                match req {
                    Some(t) => r.set("X-Moss-Token", t).call(),
                    None => r.call(),
                }
            }
        })
        .await
        .unwrap();
        match attempt {
            Err(ureq::Error::Status(401, _)) => {}
            Ok(_) => panic!("the event stream accepted a subscriber with {label}"),
            Err(e) => panic!("expected 401 for {label}, got {e}"),
        }
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test(flavor = "multi_thread")]
async fn event_stream_delivers_a_published_event_to_a_browser() {
    use crate::ops::serve::events;

    let (_parent, _vault_path, site_dir) = confinement_vault();
    let (port, shutdown_tx, token) = serve_bound(site_dir.clone(), 63420).await;

    let url = format!("http://localhost:{port}/__moss/events");
    let reader = tokio::task::spawn_blocking({
        let token = token.clone();
        move || {
            ureq::get(&url)
                .set("X-Moss-Token", &token)
                .timeout(std::time::Duration::from_secs(10))
                .call()
                .expect("the stream must open")
                .into_reader()
        }
    })
    .await
    .unwrap();
    // Response headers arrive only after the handler has subscribed to the
    // bus, so the receiver exists before this publish: the event cannot be
    // dropped for a reason that is not the code's.

    events::publish(&crate::types::events::MossEvent::BuildComplete(
        crate::build::progress::BuildComplete {
            videos_converted: 0,
            total_time_ms: 42,
            skipped_symlinks: 0,
        },
    ));

    let record = tokio::task::spawn_blocking(move || read_one_sse_record(reader))
        .await
        .unwrap()
        .expect("a published event must arrive as one SSE record");

    // The frame carries the name OUTSIDE the payload, because one stream serves
    // every event name and `listen.ts` demultiplexes on it.
    assert!(
        record.starts_with("data:"),
        "an SSE record must be a `data:` line, got: {record:?}"
    );
    let json: serde_json::Value =
        serde_json::from_str(record.trim_start_matches("data:").trim()).expect("frame is JSON");
    assert_eq!(json["name"], crate::types::events::MOSS_EVENT_CHANNEL);
    assert_eq!(json["payload"]["kind"], "BuildComplete");
    assert_eq!(json["payload"]["payload"]["total_time_ms"], 42);

    let _ = shutdown_tx.send(());
}
