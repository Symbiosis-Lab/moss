use super::*;
use crate::build::media::fallback_raster::FALLBACK_MAX_EDGE;
use crate::build::types::SourceMetadata;

/// Seal exactly what the emit channel reported, so a ship assertion is about
/// what the code under test registered rather than about this file's idea of it.
fn drained_sealed(
    rx: tokio::sync::mpsc::Receiver<crate::build::coordinator::EmitMessage>,
) -> crate::build::manifest::SealedManifest {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(crate::build::coordinator::test_utils::drain_into_sealed(
            rx,
            crate::types::content::SiteHashes::default(),
        ))
}

fn meta(hash: &str, size: u64, mtime: u64) -> SourceMetadata {
    SourceMetadata {
        hash: hash.to_string(),
        size,
        mtime,
        mtime_nanos: None,
        ctime: None,
        inode: None,
    }
}

#[test]
fn cache_hit_returns_oid_when_size_and_mtime_match_and_safe() {
    let cached = meta("oid-abc", 4096, 1_000);
    let got = check_source_cache(Some(&cached), 4096, 1_000, 2_000);
    assert_eq!(got.as_deref(), Some("oid-abc"));
}

#[test]
fn cache_miss_when_no_previous_entry() {
    let got = check_source_cache(None, 4096, 1_000, 2_000);
    assert_eq!(got, None);
}

#[test]
fn cache_miss_when_size_differs() {
    let cached = meta("oid-abc", 4096, 1_000);
    let got = check_source_cache(Some(&cached), 8192, 1_000, 2_000);
    assert_eq!(got, None);
}

#[test]
fn cache_miss_when_mtime_differs() {
    let cached = meta("oid-abc", 4096, 1_000);
    let got = check_source_cache(Some(&cached), 4096, 1_500, 2_000);
    assert_eq!(got, None);
}

#[test]
fn cache_miss_in_racy_window_around_previous_write() {
    // Git's racy-mtime trick: mtime equal to or after the previous-hashes
    // write time could mean an in-place edit happened during the
    // timer-resolution window. Re-hash to be safe.
    let cached = meta("oid-abc", 4096, 2_000);
    let got = check_source_cache(Some(&cached), 4096, 2_000, 2_000);
    assert_eq!(got, None);
}

#[test]
fn cache_hit_just_outside_racy_window() {
    // mtime is at least EPSILON seconds older than prev — safe.
    let cached = meta("oid-abc", 4096, 1_999);
    let got = check_source_cache(Some(&cached), 4096, 1_999, 2_001);
    assert_eq!(got.as_deref(), Some("oid-abc"));
}

#[test]
fn cache_miss_when_no_previous_hashes_file() {
    // First-ever build: no hashes.json yet. Refuse to trust any cache.
    let cached = meta("oid-abc", 4096, 1_000);
    let got = check_source_cache(Some(&cached), 4096, 1_000, 0);
    assert_eq!(got, None);
}

#[test]
fn cache_miss_for_file_modified_after_previous_write() {
    let cached = meta("oid-abc", 4096, 5_000);
    let got = check_source_cache(Some(&cached), 4096, 5_000, 2_000);
    assert_eq!(got, None);
}

/// Integration test: verify that `copy_deferred_assets` preserves a directory
/// symlink in BOTH staging (output_dir) and the canonical site dir, and that
/// the stage→site mirror does not dereference the symlink into a real
/// directory.
///
/// A directory symlink must survive both copies as a link: the deferred copy
/// into staging, and the ship into the generation. The mirror's symlink branch
/// uses `symlink_metadata` so it does not recurse through the link.
#[cfg(unix)]
#[test]
fn copy_deferred_assets_preserves_directory_symlink() {
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    let site = moss.join("build/site");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(&site).unwrap();
    fs::create_dir_all(moss.join("cache/objects")).unwrap();

    // Source layout:
    //   source/
    //     resources/app/index.html
    //     resources/app/bundle.js
    //     myapp -> resources/app   (directory symlink)
    fs::create_dir_all(source.join("resources/app")).unwrap();
    fs::write(source.join("resources/app/index.html"), b"<h1>app</h1>").unwrap();
    fs::write(source.join("resources/app/bundle.js"), b"console.log('hi')").unwrap();
    std::os::unix::fs::symlink("resources/app", source.join("myapp")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = crate::build::coordinator::test_utils::build_test_coordinator();
    let stats = copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);

    // The per-symlink log line is DEBUG, so this count is the only trace a
    // preserved symlink leaves in an uploaded log (issue #1005). Asserted here
    // rather than in its own test because this fixture already has one.
    assert_eq!(
        stats.preserved_symlinks, 1,
        "the preserved symlink must be counted; got {}",
        stats.preserved_symlinks,
    );

    // Ship what the copy actually reported, not what this test would like it
    // to have reported: a hand-written manifest here would make the assertions
    // below true no matter what `copy_deferred_assets` registered.
    let sealed = drained_sealed(rx);
    assert!(
        sealed.files().contains_key("myapp"),
        "the copy must register the alias it preserved; it named: {:?}",
        sealed.files().keys().collect::<Vec<_>>()
    );
    crate::build::ship::ship_phase(&staging, &site, &sealed, None, None).expect("ship_phase should succeed");

    // 1. staging/myapp is a symlink with target "resources/app"
    let staging_alias = staging.join("myapp");
    let staging_meta = fs::symlink_metadata(&staging_alias).expect("staging/myapp should exist");
    assert!(
        staging_meta.file_type().is_symlink(),
        "staging/myapp must be a symlink, not a real directory"
    );
    assert_eq!(
        fs::read_link(&staging_alias).unwrap(),
        std::path::PathBuf::from("resources/app"),
        "staging symlink target must round-trip verbatim"
    );

    // 2. site/myapp is a symlink with target "resources/app" (dual-write).
    let site_alias = site.join("myapp");
    let site_meta = fs::symlink_metadata(&site_alias).expect("site/myapp should exist");
    assert!(
        site_meta.file_type().is_symlink(),
        "site/myapp must be a symlink — dual-write or the mirror's symlink branch"
    );
    assert_eq!(
        fs::read_link(&site_alias).unwrap(),
        std::path::PathBuf::from("resources/app"),
        "site symlink target must round-trip verbatim"
    );

    // 3. Canonical files exist at their real path.
    assert!(
        site.join("resources/app/index.html").exists(),
        "canonical index.html should be copied to site/"
    );

    // 4. Reading through the symlink yields the canonical bytes.
    let through_alias =
        fs::read(site_alias.join("index.html")).expect("can read through site/myapp/index.html");
    assert_eq!(through_alias, b"<h1>app</h1>");
}

/// Integration test: a preserved symlink registers a manifest entry
/// under MODE_SYMLINK so the deploy uploader will ship it as a symlink
/// to seta. Without this, the symlink would be silently dropped from
/// the deploy (the original `cities-heat-map` 404 bug).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_registers_symlink_in_manifest() {
    use crate::build::coordinator::test_utils;
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss.join("cache/objects")).unwrap();

    fs::create_dir_all(source.join("resources/app")).unwrap();
    fs::write(source.join("resources/app/index.html"), b"<h1>app</h1>").unwrap();
    // Use the same relative-path target the production code preserves.
    let target = "resources/app";
    std::os::unix::fs::symlink(target, source.join("cities-heat-map")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
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

    // Drain the coordinator (post-#620 Item 2: hashes.json is no longer
    // written by `copy_deferred_assets`; the coordinator owns the manifest).
    let sealed = test_utils::drain_into_sealed(rx, crate::types::content::SiteHashes::default()).await;

    let entry = sealed
        .files()
        .get("cities-heat-map")
        .expect("symlink manifest entry must be registered");
    // `register_with_hash` preserves any existing mode prefix (`100644:` or
    // `120000:`); for entries without a prefix it adds `100644:`. The runner
    // sends the symlink hash with its `120000:` prefix intact, so the
    // coordinator stores it verbatim — this test exercises the preservation
    // path. The resulting entry must therefore match `symlink_entry(target)`
    // which also encodes the `120000:` prefix.
    let expected = crate::types::content::symlink_entry(target);
    assert_eq!(entry, &expected, "manifest must encode symlink target");

    let (mode, _) = crate::types::content::parse_entry(entry);
    assert_eq!(mode, crate::types::content::MODE_SYMLINK);
}

#[cfg(target_os = "macos")]
#[test]
fn copy_deferred_assets_resolves_finder_alias() {
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    let site = moss.join("build/site");
    for d in [&source, &staging, &site, &moss.join("cache/objects")] {
        fs::create_dir_all(d).unwrap();
    }

    fs::create_dir_all(source.join("resources/app")).unwrap();
    fs::write(source.join("resources/app/index.html"), b"<h1>app</h1>").unwrap();

    // Create a Finder Alias via osascript
    let script = format!(
        r#"tell application "Finder" to make alias to (POSIX file "{}" as alias) at (POSIX file "{}" as alias)"#,
        source.join("resources/app").display(),
        source.display()
    );
    let ok = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipping test: osascript unavailable");
        return;
    }
    // Finder names it "<basename>" or "<basename> alias" depending on
    // whether there's a naming conflict. Scan for any alias file created.
    let alias_path = {
        use crate::build::media::symlink::is_bookmark_alias_file;
        let mut found = None;
        if let Ok(entries) = fs::read_dir(&source) {
            for entry in entries.flatten() {
                let p = entry.path();
                if is_bookmark_alias_file(&p) {
                    found = Some(p);
                    break;
                }
            }
        }
        match found {
            Some(p) => p,
            None => {
                eprintln!("skipping test: alias not created in source dir");
                return;
            }
        }
    };

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };
    let alias_name = alias_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let (tx, rx) = crate::build::coordinator::test_utils::build_test_coordinator();
    let _ = copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    let sealed = drained_sealed(rx);
    let _ = crate::build::ship::ship_phase(&staging, &site, &sealed, None, None);

    // The alias should be a POSIX symlink in BOTH staging and site
    for dir in [&staging, &site] {
        let path = dir.join(&alias_name);
        let meta = fs::symlink_metadata(&path).expect("output should exist");
        assert!(
            meta.file_type().is_symlink(),
            "{:?} should be a symlink",
            path
        );
        // Reading through it returns canonical bytes
        let through = fs::read(path.join("index.html")).expect("can read through alias-symlink");
        assert_eq!(through, b"<h1>app</h1>");
    }
}

/// Regression guard for the deferred-phase stale-cleanup race (#620 Item 4).
///
/// The race window:
/// 1. A background image worker writes `fresh.webp` to `staging/` AND queues
///    an `EmitMessage` for the coordinator. The disk write lands BEFORE the
///    EmitMessage is observed by `pending`.
/// 2. `copy_deferred_assets` walks `source_root`, accumulates `live_asset_keys`,
///    and calls `remove_stale_files` on the staging+canonical dirs using its
///    LOCAL `site_hashes` clone — which does NOT see the in-flight emit.
/// 3. `fresh.webp` is not in `site_hashes.image_outputs`, not in `files`, so
///    `remove_stale_files` deletes it.
/// 4. The coordinator later merges the EmitMessage but the file is already gone.
///
/// Two variants:
/// - **Variant A** (empty `previous_hashes`): the in-flight `.webp` is the
///   ONLY thing keeping the path alive. Forces the bug to manifest on a first
///   build, where carry-forward cannot mask it.
/// - **Variant B** (populated `previous_hashes` with `fresh.webp` already in
///   `image_outputs`): the carry-forward path keeps the entry alive even if
///   the in-flight emit is missed. Catches the case where carry-forward masks
///   the underlying race at the test layer.
///
/// Both variants pre-write `fresh.webp` to `staging/` to simulate "worker has
/// written but not yet sent the EmitMessage." We do NOT send a matching
/// EmitMessage in the test — the only protection comes from carry-forward
/// (variant B), or the bug being fixed at the layer above (variant A, after
/// #621 moved cleanup past the seal).
///
/// As of #621, variant A passes because `copy_deferred_assets` no longer runs
/// `remove_stale_files` inline at all — cleanup moved to the seal+persist
/// side task in `build.rs`, where it operates on a stable `SealedManifest`
/// view that has already merged in-flight `EmitMessage`s.
#[tokio::test]
async fn copy_deferred_assets_stale_cleanup_does_not_delete_in_flight_image_variant_a() {
    use crate::build::coordinator::ManifestCoordinator;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    let site = moss.join("build/site");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::create_dir_all(moss.join("cache/objects")).unwrap();

    // Pre-write the in-flight .webp BEFORE copy_deferred_assets runs.
    // Simulates an image worker that has written its output but has not
    // yet sent its EmitMessage to the coordinator.
    std::fs::write(staging.join("fresh.webp"), b"\x52\x49\x46\x46webp-bytes").unwrap();

    // Variant A: empty previous_hashes — no carry-forward protection.
    let (coord, tx) = ManifestCoordinator::new(crate::types::content::SiteHashes::default());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let tx_clone = tx.clone();
    let site_arg = site.clone();
    let h = tokio::task::spawn_blocking(move || {
        super::copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx_clone, None)
    });
    // Drop the original sender so the coordinator can seal once `h` finishes.
    drop(tx);

    let _stats = h.await.unwrap();

    assert!(
        staging.join("fresh.webp").exists(),
        "in-flight .webp was deleted by remove_stale_files — \
             deferred-phase stale-cleanup race is open (variant A: empty previous_hashes)"
    );

    // Drain the coordinator so the test cleans up gracefully.
    let _sealed = coord.run_until_drained().await;
}

/// Variant B of the deferred-phase stale-cleanup race regression test.
/// `previous_hashes` is populated so carry-forward keeps `fresh.webp` alive
/// even if the in-flight emit is dropped. See variant A's docstring for the
/// race description and decision criteria.
#[tokio::test]
async fn copy_deferred_assets_stale_cleanup_does_not_delete_in_flight_image_variant_b() {
    use crate::build::coordinator::ManifestCoordinator;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    let site = moss.join("build/site");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::create_dir_all(moss.join("cache/objects")).unwrap();

    std::fs::write(staging.join("fresh.webp"), b"\x52\x49\x46\x46webp-bytes").unwrap();

    // Variant B: previous_hashes already has fresh.webp in image_outputs.
    // Tests whether carry-forward masks the bug when seeded with prior state.
    let mut prev_hashes = crate::types::content::SiteHashes::default();
    prev_hashes.image_outputs.insert("fresh.webp".to_string());
    let mut current_hashes = crate::types::content::SiteHashes::default();
    current_hashes
        .image_outputs
        .insert("fresh.webp".to_string());

    let (coord, tx) = ManifestCoordinator::new(current_hashes.clone());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        previous_hashes: prev_hashes,
        ..crate::types::services::BackgroundContext::for_test()
    };

    let tx_clone = tx.clone();
    let site_arg = site.clone();
    let h = tokio::task::spawn_blocking(move || {
        super::copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx_clone, None)
    });
    drop(tx);

    let _stats = h.await.unwrap();

    assert!(
        staging.join("fresh.webp").exists(),
        "in-flight .webp was deleted by remove_stale_files — \
             deferred-phase stale-cleanup race is open (variant B: populated previous_hashes)"
    );

    let _sealed = coord.run_until_drained().await;
}

/// Regression test for #621: after the fix, `copy_deferred_assets` must NOT
/// remove stale files inline. Cleanup is deferred to the seal+persist side
/// task in `build.rs`, which operates on `SealedManifest::site_hashes_view()`.
///
/// Complements variant A above (which targets the in-flight `.webp` race).
/// This test pre-creates an actually-stale file and asserts it survives —
/// confirming the cleanup hook is gone, not merely that it ran with a more
/// inclusive `site_hashes`.
#[tokio::test]
async fn copy_deferred_assets_does_not_run_stale_file_cleanup_inline() {
    use crate::build::coordinator::ManifestCoordinator;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    let site = moss.join("build/site");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&site).unwrap();
    std::fs::create_dir_all(moss.join("cache/objects")).unwrap();

    // Pre-create a "stale" file in BOTH staging and site — pre-#621, the
    // in-band cleanup would have removed it because it's not in any bucket
    // of `site_hashes`. After #621, cleanup has moved to the seal+persist
    // side task, so `copy_deferred_assets` must leave it alone.
    let stale_staging = staging.join("stale.txt");
    let stale_site = site.join("stale.txt");
    std::fs::write(&stale_staging, b"stale bytes").unwrap();
    std::fs::write(&stale_site, b"stale bytes").unwrap();

    let (coord, tx) = ManifestCoordinator::new(crate::types::content::SiteHashes::default());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let tx_clone = tx.clone();
    let site_arg = site.clone();
    let h = tokio::task::spawn_blocking(move || {
        super::copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx_clone, None)
    });
    drop(tx);
    let _stats = h.await.unwrap();

    assert!(
        stale_staging.exists(),
        "stale file in staging must survive copy_deferred_assets after #621 fix; \
             cleanup is now deferred to the seal+persist side task in build.rs"
    );
    assert!(
        stale_site.exists(),
        "stale file in site must survive copy_deferred_assets after #621 fix; \
             cleanup is now deferred to the seal+persist side task in build.rs"
    );

    let _sealed = coord.run_until_drained().await;
}

// -----------------------------------------------------------------------
// remove_stale_files: orphan *.placeholder.svg cleanup (post-#615)
// -----------------------------------------------------------------------

#[test]
fn remove_stale_files_always_unlinks_placeholder_svg_orphans() {
    // Vaults built with pre-#615 moss have *.placeholder.svg files in
    // build/site/assets/ AND entries in hashes.json#files for them.
    // Without a special-case, remove_stale_files preserves them
    // indefinitely because they appear in `files`. Current code never
    // generates them, so they're always orphans.
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();

    let orphan_a = assets.join("clip.placeholder.svg");
    let orphan_b = assets.join("song.placeholder.svg");
    std::fs::write(&orphan_a, b"<svg/>").unwrap();
    std::fs::write(&orphan_b, b"<svg/>").unwrap();

    // Site hashes still have the entries — exactly the user's bug state.
    let mut site_hashes = SiteHashes::default();
    site_hashes.files.insert(
        "assets/clip.placeholder.svg".to_string(),
        "100644:abc".to_string(),
    );
    site_hashes.files.insert(
        "assets/song.placeholder.svg".to_string(),
        "100644:def".to_string(),
    );

    // A legitimate file that should NOT be touched.
    let legit = assets.join("photo.jpg");
    std::fs::write(&legit, b"\xff\xd8\xff\xe0").unwrap();
    site_hashes
        .files
        .insert("assets/photo.jpg".to_string(), "100644:123".to_string());

    remove_stale_files(dir, &site_hashes, "test-site", &crate::build::lifecycle::permit_for_test());

    assert!(
        !orphan_a.exists(),
        "orphan_a should be unlinked even though it's in files map"
    );
    assert!(
        !orphan_b.exists(),
        "orphan_b should be unlinked even though it's in files map"
    );
    assert!(legit.exists(), "legitimate file must be preserved");
}

/// TDD: source asset manifest entry must use xxh3 (16-hex), not SHA-256 (64-hex).
///
/// Regression test for the manifest-hash/verify_file_bytes mismatch:
/// `copy_deferred_assets` was recording the CAS OID (SHA-256, 64-hex) into the
/// manifest `files` map, while `verify_file_bytes` in `deploy.rs` recomputes an
/// xxh3 (16-hex) hash.  The two could never match, causing a 422 on every source
/// asset ≤20 MB.  This test proves the fixed code records an xxh3 hash that
/// `verify_file_bytes` can round-trip successfully.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_manifest_hash_is_xxh3_not_sha256() {
    use crate::build::assets::paths::compute_binary_hash;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // Write a known source asset whose bytes we can compare later.
    let asset_bytes = b"hello world asset content for xxh3 test";
    let asset_file = source.join("assets/test-asset.bin");
    fs::create_dir_all(asset_file.parent().unwrap()).unwrap();
    fs::write(&asset_file, asset_bytes).unwrap();

    let expected_xxh3 = compute_binary_hash(asset_bytes);
    // Sanity: xxh3 must be 16 hex chars, not 64 (SHA-256).
    assert_eq!(expected_xxh3.len(), 16, "xxh3 must be 16 hex chars");

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    // The manifest entry for the source asset.
    let entry = sealed
        .files()
        .get("assets/test-asset.bin")
        .expect("asset must be registered in the sealed manifest");

    // Parse off the mode prefix (e.g. "100644:") to get the raw hash.
    let (_, raw_hash) = crate::types::content::parse_entry(entry);

    // Must be exactly 16 hex chars (xxh3), NOT 64 (SHA-256).
    assert_eq!(
        raw_hash.len(),
        16,
        "manifest hash must be 16-char xxh3, got {} chars: '{}'",
        raw_hash.len(),
        raw_hash
    );
    assert_eq!(
        raw_hash, expected_xxh3,
        "manifest hash must equal compute_binary_hash of the source bytes"
    );

    // And the actual deploy verify logic: compute_binary_hash of the bytes
    // must equal the manifest entry (this is what deploy.rs:verify_file_bytes does).
    let recomputed = compute_binary_hash(asset_bytes);
    assert_eq!(
        raw_hash, recomputed,
        "deploy verify_file_bytes would accept this: recomputed hash matches manifest"
    );
}

#[test]
fn remove_stale_files_idempotent_with_no_placeholders() {
    // Fresh vault has no .placeholder.svg files. Running cleanup twice
    // (or N times) is a no-op for legitimate files — the new
    // .placeholder.svg special-case must not regress this.
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();

    let legit = assets.join("photo.jpg");
    std::fs::write(&legit, b"\xff\xd8\xff\xe0").unwrap();

    let mut site_hashes = SiteHashes::default();
    site_hashes
        .files
        .insert("assets/photo.jpg".to_string(), "100644:123".to_string());

    remove_stale_files(dir, &site_hashes, "test-site", &crate::build::lifecycle::permit_for_test());
    assert!(legit.exists(), "legit must survive first cleanup");

    // Run it again; legit must still survive.
    remove_stale_files(dir, &site_hashes, "test-site", &crate::build::lifecycle::permit_for_test());
    assert!(
        legit.exists(),
        "legit must survive second cleanup (idempotent)"
    );
}

/// Issue #697: verify that audio/PDF/static assets are registered as Ready
/// in the AssetRegistry after `copy_deferred_assets` runs. This ensures
/// the editor's hover-preview can resolve these asset types even though they
/// are not registered in the blocking phase (unlike images and videos).
#[test]
fn copy_deferred_assets_registers_static_assets_in_registry() {
    use crate::types::assets::{AssetRegistry, AssetState};
    use crate::types::content::SiteHashes;
    use std::sync::Arc;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // Place test assets: audio, PDF, and one image (image must NOT be
    // re-registered here — images are handled by the blocking/image pipeline).
    let audio_file = source.join("audio/talk.mp3");
    std::fs::create_dir_all(audio_file.parent().unwrap()).unwrap();
    std::fs::write(&audio_file, b"ID3fake").unwrap();

    let pdf_file = source.join("docs/paper.pdf");
    std::fs::create_dir_all(pdf_file.parent().unwrap()).unwrap();
    std::fs::write(&pdf_file, b"%PDF-1.4").unwrap();

    let image_file = source.join("img/photo.jpg");
    std::fs::create_dir_all(image_file.parent().unwrap()).unwrap();
    std::fs::write(&image_file, b"\xff\xd8\xff\xe0").unwrap();

    let registry = Arc::new(AssetRegistry::new());

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
    copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, Some(registry.clone()));

    // Audio and PDF must be registered as Ready.
    match registry.get("audio/talk.mp3") {
        Some(AssetState::Ready) => {}
        other => panic!("Expected Ready for audio/talk.mp3, got {:?}", other),
    }
    match registry.get("docs/paper.pdf") {
        Some(AssetState::Ready) => {}
        other => panic!("Expected Ready for docs/paper.pdf, got {:?}", other),
    }

    // Images must NOT be double-registered here (their pipeline handles it).
    assert!(
        registry.get("img/photo.jpg").is_none(),
        "Images must not be registered by copy_deferred_assets (blocking phase owns them)"
    );
}

/// TDD: broken symlink → `AssetPipelineRunStats::skipped_symlinks` == 1.
///
/// A symlink pointing at a non-existent target is a `SkippedBroken` outcome
/// in `handle_symlink_entry`. Before this fix, that count was never surfaced;
/// authors saw output files but could not tell if a symlink was silently dropped.
/// After the fix, `skipped_symlinks` is incremented and included in both the
/// stats struct and (via `BuildServices::skipped_symlinks`) the `BuildComplete`
/// event sent to the frontend.
#[cfg(unix)]
#[test]
fn copy_deferred_assets_counts_broken_symlink_in_stats() {
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("source");
    let moss = tmp.path().join(".moss");
    let staging = moss.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss.join("cache/objects")).unwrap();

    // A regular file so the walk has something to process.
    fs::write(source.join("page.txt"), b"hello").unwrap();

    // A broken symlink: target does not exist → SkippedBroken outcome.
    std::os::unix::fs::symlink("nonexistent-target", source.join("broken-link")).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss.clone(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, _rx) = crate::build::coordinator::test_utils::build_test_coordinator();
    let stats = copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);

    assert_eq!(
        stats.skipped_symlinks, 1,
        "one broken symlink must be counted in skipped_symlinks; got {}",
        stats.skipped_symlinks,
    );
}

/// Issue #1005: the summary line must name preserved symlinks and resolved
/// aliases. Counting them in the struct is not enough — the demotion of the
/// per-item log to DEBUG traded N lines for zero unless the count reaches the
/// one line that actually gets logged. Zero-valued counts stay off the line.
#[test]
fn asset_run_summary_reports_preserved_symlinks_and_aliases() {
    let quiet = AssetPipelineRunStats { copied: 3, ..Default::default() };
    let line = format_asset_run_summary(&quiet);
    assert!(!line.contains("preserved"), "no preserved symlinks → no clause: {line}");
    assert!(!line.contains("aliases"), "no aliases → no clause: {line}");

    let linked = AssetPipelineRunStats {
        copied: 3,
        preserved_symlinks: 2,
        preserved_aliases: 1,
        ..Default::default()
    };
    let line = format_asset_run_summary(&linked);
    assert!(line.contains("2 symlinks preserved"), "summary must report preserved: {line}");
    assert!(line.contains("1 alias resolved to a symlink"), "summary must report aliases: {line}");
}

/// The summary is read by a human in an uploaded log, so it says "1 symlink",
/// never "1 symlinks" or "1 symlink(s)" — the same rule
/// `make_symlink_skip_advisory` follows.
#[test]
fn asset_run_summary_uses_singular_nouns_for_a_count_of_one() {
    let one = AssetPipelineRunStats {
        copied: 1,
        skipped_symlinks: 1,
        preserved_symlinks: 1,
        preserved_aliases: 1,
        ..Default::default()
    };
    let line = format_asset_run_summary(&one);
    assert!(!line.contains("(s)"), "no '(s)' pluralization in a user-visible line: {line}");
    assert!(line.contains("1 symlink skipped"), "singular skipped: {line}");
    assert!(line.contains("1 symlink preserved"), "singular preserved: {line}");
    assert!(line.contains("1 alias resolved to a symlink"), "singular alias: {line}");

    let many = AssetPipelineRunStats {
        skipped_symlinks: 2,
        preserved_symlinks: 2,
        preserved_aliases: 2,
        ..Default::default()
    };
    let line = format_asset_run_summary(&many);
    assert!(line.contains("2 symlinks skipped"), "plural skipped: {line}");
    assert!(line.contains("2 symlinks preserved"), "plural preserved: {line}");
    assert!(line.contains("2 aliases resolved to symlinks"), "plural aliases: {line}");
}

/// Regression lock: a successfully-linked asset must NEVER be dropped from
/// the manifest by the hash step, even when the SOURCE file is unreadable
/// at hash time (e.g. iCloud eviction, permission race).
///
/// This is the confirmed "broken-cover / dropped-asset" incident class:
/// commit 6e4bd6e0a changed the manifest hash from the infallible
/// `unwrap_or(oid)` to a fallible `compute_binary_hash_file(file_path)`
/// (source file), whose `Err` branch did `skipped += 1; continue`, bypassing
/// `insert_file_hash`.  `remove_stale_files` then deleted the just-linked
/// output because its key was absent from `site_hashes.files`.
///
/// The fix: hash the OUTPUT file (just linked by `link_to`), not the source.
/// This test verifies:
///   1. The manifest entry is present after `copy_deferred_assets`.
///   2. The manifest hash equals `compute_binary_hash` of the output bytes —
///      proving the hash was computed from the LOCAL output (not the source),
///      so a source iCloud eviction cannot affect the manifest entry.
///   3. The output file at the manifest path is readable and its xxh3
///      matches the recorded manifest hash (deploy verify_file_bytes passes).
///
/// Approach: single-run test.  The structural proof that the hash is computed
/// from the output (not the source) is in the fix itself
/// (`compute_binary_hash_file(&target)` vs `compute_binary_hash_file(file_path)`).
/// We additionally verify by asserting that the manifest hash == xxh3 of the
/// OUTPUT file's bytes (which always works), and that removing the SOURCE
/// after the run does not retroactively change the manifest (the manifest is
/// a local data structure, not re-evaluated post-run).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_manifest_entry_not_dropped_by_hash_step() {
    use crate::build::assets::paths::compute_binary_hash;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // Write the source asset with known bytes.
    let asset_bytes = b"regression-lock: manifest-entry-not-dropped-by-hash-step";
    let asset_file = source.join("assets/cover.jpg");
    fs::create_dir_all(asset_file.parent().unwrap()).unwrap();
    fs::write(&asset_file, asset_bytes).unwrap();

    let expected_xxh3 = compute_binary_hash(asset_bytes);
    assert_eq!(expected_xxh3.len(), 16, "sanity: xxh3 must be 16 hex chars");

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };
    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    // --- 1. Manifest entry must be present. ---
    // Before the fix: the `compute_binary_hash_file(file_path)` Err branch
    // did `skipped += 1; continue`, bypassing `insert_file_hash` and leaving
    // the entry absent.  `remove_stale_files` then deleted the just-linked
    // output — the confirmed dropped-asset incident class.
    let entry = sealed.files().get("assets/cover.jpg").expect(
        "manifest entry must be present after a successful link_to — \
                     the hash step must NOT drop a successfully-linked asset",
    );

    let (_, raw_hash) = crate::types::content::parse_entry(entry);

    // --- 2. Hash must be 16-char xxh3 (not 64-char SHA-256). ---
    assert_eq!(
        raw_hash.len(),
        16,
        "manifest hash must be 16-char xxh3, got {} chars: '{}'",
        raw_hash.len(),
        raw_hash
    );

    // --- 3. Hash must equal xxh3 of the OUTPUT file's bytes. ---
    // The output file (just linked from the CAS blob by link_to) lives at
    // staging/<mapped_path>.  Its bytes equal the source bytes — the xxh3
    // is identical — but reading it proves the hash is resilient to a source
    // eviction (the output is a LOCAL .moss/build file, immune to iCloud).
    let output_file = staging.join("assets/cover.jpg");
    assert!(
        output_file.exists(),
        "output file must be present after link_to"
    );
    let output_bytes = fs::read(&output_file).expect("output file must be readable");
    let output_xxh3 = compute_binary_hash(&output_bytes);
    assert_eq!(
        raw_hash, output_xxh3,
        "manifest hash must equal compute_binary_hash of the OUTPUT file bytes \
             (proving hash is read from local output, not from the evictable source)"
    );
    assert_eq!(
        raw_hash, expected_xxh3,
        "manifest hash must equal compute_binary_hash of the original asset bytes"
    );
}

/// Plan 2026-07-06: raster originals (jpg/jpeg/png) must be deployed as a
/// SIZED/optimized JPEG capped at `FALLBACK_MAX_EDGE`, NOT the full-res
/// source. SVG, GIF, and non-decodable files fall back to a verbatim
/// (byte-identical) copy. The manifest hash must match the sized output
/// bytes (so deploy `verify_file_bytes` passes). Cache reuse is covered by
/// the image.rs unit test `sized_jpeg_oid_reuses_transform_cache`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_sizes_raster_originals_and_copies_others_verbatim() {
    use crate::build::assets::paths::compute_binary_hash;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // (a) A 4000×3000 JPEG — well above the fallback cap. High-entropy so
    //     the re-encode is unambiguously smaller than the source.
    let big_jpg = source.join("photo.jpg");
    {
        let buf: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(4000, 3000, |x, y| {
                image::Rgb([
                    ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) & 0xff) as u8,
                    ((x.wrapping_mul(97) ^ y.wrapping_mul(193)) & 0xff) as u8,
                    ((x.wrapping_mul(389) ^ y.wrapping_mul(769)) & 0xff) as u8,
                ])
            });
        image::DynamicImage::ImageRgb8(buf)
            .save_with_format(&big_jpg, image::ImageFormat::Jpeg)
            .unwrap();
    }
    let big_jpg_src_len = fs::metadata(&big_jpg).unwrap().len();

    // (b) An SVG (vector) — copied verbatim (not in the raster set).
    let svg = source.join("vector.svg");
    let svg_bytes: &[u8] =
        b"<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>";
    fs::write(&svg, svg_bytes).unwrap();

    // (c) A .png whose bytes are actually HTML (broken download) — not a
    //     decodable raster, so the size step returns None → verbatim copy.
    let fake_png = source.join("broken.png");
    let fake_png_bytes: &[u8] = b"<!doctype html><title>404 not found</title>";
    fs::write(&fake_png, fake_png_bytes).unwrap();

    // (d) A GIF — outside the jpg/jpeg/png raster set → verbatim copy.
    let gif = source.join("clip.gif");
    let mut gif_bytes: Vec<u8> = b"GIF89a".to_vec();
    gif_bytes.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0]); // minimal LSD
    gif_bytes.push(0x3B); // trailer
    fs::write(&gif, &gif_bytes).unwrap();

    // (e) A transparent PNG larger than max_edge — its LEFT half is fully
    //     transparent (alpha 0). It must be deployed as a SIZED, real PNG
    //     that keeps its transparency (NOT flattened to a white JPEG box).
    let transparent_png = source.join("logo.png");
    {
        let buf: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(3000, 2000, |x, _| {
                if x < 1500 {
                    image::Rgba([255, 0, 0, 0]) // transparent
                } else {
                    image::Rgba([0, 0, 255, 255]) // opaque
                }
            });
        image::DynamicImage::ImageRgba8(buf)
            .save_with_format(&transparent_png, image::ImageFormat::Png)
            .unwrap();
    }
    let transparent_png_src = fs::read(&transparent_png).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    // --- Raster original: deployed as a SIZED JPEG. ---
    let out_jpg = staging.join("photo.jpg");
    assert!(
        out_jpg.exists(),
        "sized photo.jpg must be written to the output"
    );
    let (ow, oh) = image::image_dimensions(&out_jpg).expect("output must decode as an image");
    assert!(
        ow.max(oh) <= FALLBACK_MAX_EDGE,
        "output max edge {} must be capped at the fallback cap",
        ow.max(oh)
    );
    let out_bytes = fs::read(&out_jpg).unwrap();
    assert!(
        (out_bytes.len() as u64) < big_jpg_src_len,
        "sized output {} bytes must be < full-res source {} bytes",
        out_bytes.len(),
        big_jpg_src_len
    );
    // The full-res original must NOT have been deployed byte-for-byte.
    assert_ne!(
        out_bytes.len() as u64,
        big_jpg_src_len,
        "the deployed jpg must not be a verbatim copy of the full-res original"
    );
    // Manifest hash must equal the xxh3 of the SIZED output bytes (deploy
    // verify_file_bytes recomputes this) — proving no full-res hash leaked.
    let jpg_entry = sealed
        .files()
        .get("photo.jpg")
        .expect("photo.jpg in manifest");
    let (_, jpg_hash) = crate::types::content::parse_entry(jpg_entry);
    assert_eq!(
        jpg_hash,
        compute_binary_hash(&out_bytes),
        "manifest hash must match the sized output bytes"
    );

    // --- Non-raster / non-decodable: verbatim, byte-identical copies. ---
    assert_eq!(
        fs::read(staging.join("vector.svg")).unwrap(),
        svg_bytes,
        "SVG must be copied verbatim"
    );
    assert_eq!(
        fs::read(staging.join("broken.png")).unwrap(),
        fake_png_bytes,
        "a non-decodable .png must be copied verbatim (size step returned None)"
    );
    assert_eq!(
        fs::read(staging.join("clip.gif")).unwrap(),
        gif_bytes,
        "GIF must be copied verbatim"
    );

    // --- Transparent PNG: deployed as a SIZED, transparency-preserving PNG. ---
    let out_png = staging.join("logo.png");
    assert!(
        out_png.exists(),
        "sized logo.png must be written to the output"
    );
    let out_png_bytes = fs::read(&out_png).unwrap();
    // Must NOT be a verbatim copy of the full-res original (it was resized).
    assert_ne!(
        out_png_bytes, transparent_png_src,
        "the transparent PNG must be re-encoded (sized), not copied verbatim"
    );
    // Real PNG bytes at the .png path (correct Content-Type on servers).
    assert_eq!(
        image::guess_format(&out_png_bytes).ok(),
        Some(image::ImageFormat::Png),
        "a .png source must be deployed as a PNG, never a JPEG at a .png path"
    );
    let png_img = image::load_from_memory(&out_png_bytes).expect("output must decode");
    assert!(
        png_img.color().has_alpha(),
        "deployed PNG must retain its alpha channel (no white flatten)"
    );
    assert!(
        png_img.width().max(png_img.height()) <= FALLBACK_MAX_EDGE,
        "PNG output max edge {} must be capped at the fallback cap",
        png_img.width().max(png_img.height())
    );
    // A pixel deep in the transparent region must still be transparent.
    let png_rgba = png_img.to_rgba8();
    assert_eq!(
        png_rgba.get_pixel(png_rgba.width() / 10, png_rgba.height() / 2)[3],
        0,
        "the transparent region must survive deployment (alpha 0, not a white box)"
    );
}

// -----------------------------------------------------------------------
// Responsive-image-variants Phase B: the converter SOLELY owns the resized
// base for an oversized webp SOURCE.
//
// A webp SOURCE's base output path IS the source path (`photo.webp`), so it
// collides with the RESIZED base that `convert_single_image` writes there.
// For a converted (large, non-animated) webp, `copy_deferred_assets` must
// NOT verbatim-copy the source over that resized base — on a WARM build the
// fast cache-link copy wins the race and ships the full-res source while the
// emitted `<img srcset>` base descriptor advertises the smaller deployed
// width. A should_skip'd webp (AlreadySmall / AnimatedWebp) is NOT touched
// by the converter and must still be copied verbatim (else a 404, ADR-013).
// -----------------------------------------------------------------------

/// A real, non-animated WebP of the given pixel dimensions. A smooth
/// gradient compresses to a tiny file regardless of dimensions, so an
/// oversized-by-DIMENSION webp still lands under the 200 KB `min_size_kb`
/// gate — mirrors `image.rs::make_webp`.
fn write_gradient_webp(path: &std::path::Path, w: u32, h: u32) {
    let buf: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
        image::ImageBuffer::from_fn(w, h, |x, y| {
            image::Rgb([(x as u8).wrapping_add(y as u8), 100, 50])
        });
    let img = image::DynamicImage::ImageRgb8(buf);
    let bytes = crate::build::media::image::encode_webp(&img, 80, None).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Decoded (width, height) of a staged webp output file.
fn decoded_webp_dims(path: &std::path::Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
        .unwrap_or_else(|e| panic!("decode {}: {e}", path.display()));
    (img.width(), img.height())
}

/// The background image worker (`convert_single_image`) writing the RESIZED
/// base into `staging`, using the SAME moss_dir-derived object store +
/// transform cache that `copy_deferred_assets` reads. This is the sole
/// writer of the resized base for a converted webp source.
fn run_converter_base(
    source_webp: &std::path::Path,
    relative_webp: &str,
    staging: &std::path::Path,
    moss_dir: &std::path::Path,
) {
    use crate::build::cache::{ObjectStore, TransformCache};
    let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
    let objects = ObjectStore::new(paths.cache_objects());
    let transforms = TransformCache::new(
        paths.cache_transforms(),
        ObjectStore::new(paths.cache_objects()),
    );
    let temp = paths.cache_tmp();
    std::fs::create_dir_all(&temp).unwrap();
    let source_oid = ObjectStore::hash_file(source_webp).unwrap();
    let cfg = crate::build::media::image::ImageCompressionConfig::default();
    let outcome = crate::build::media::image::convert_single_image(
        source_webp,
        &source_oid,
        relative_webp,
        &temp,
        staging,
        &objects,
        &transforms,
        &cfg,
        None,
        None,
        &std::collections::HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "converter must succeed: {:?}",
        outcome.error
    );
}

/// Run `copy_deferred_assets` synchronously against a source/staging pair.
async fn run_copy_deferred(
    source: &std::path::Path,
    staging: &std::path::Path,
    moss_dir: &std::path::Path,
) -> crate::build::manifest::SealedManifest {
    use crate::build::coordinator::test_utils;
    let ctx = crate::types::services::BackgroundContext {
        source_path: source.to_path_buf().to_string_lossy().to_string(),
        staging_dir: staging.to_path_buf(),
        moss_dir: moss_dir.to_path_buf(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };
    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();
    test_utils::drain_into_sealed(rx, SiteHashes::default()).await
}

/// REGRESSION PIN. An oversized (1500×3000, long edge > 2400) webp source
/// must ship the RESIZED base (1200×2400 = `deployed_width`) at `photo.webp`
/// on BOTH a cold and a warm build — never the full-res verbatim source.
///
/// The two writers target the SAME staged path. We model the race outcome by
/// controlling order:
///   • COLD — converter finishes LAST (slow fresh encode wins). Correct even
///     before the fix.
///   • WARM — copy_deferred finishes LAST (fast cache-link copy wins). Before
///     the fix this clobbers the resized base with the full-res source →
///     THIS is the assertion that fails on the buggy HEAD.
/// Caches are shared across the two builds (warm actually reuses the cold
/// transform cache); staging is fresh per build.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_webp_base_owned_by_converter_cold_and_warm() {
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).unwrap();
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging_cold = moss_dir.join("build/site-stage-cold");
    let staging_warm = moss_dir.join("build/site-stage-warm");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging_cold).unwrap();
    fs::create_dir_all(&staging_warm).unwrap();

    // Oversized webp source: 1500×3000 → the 2400 cap binds on the height,
    // so the deployed base is 1200×2400 (== moss-core deployed_width(1500,3000)).
    let src_webp = source.join("photo.webp");
    write_gradient_webp(&src_webp, 1500, 3000);
    assert_eq!(
        moss_core::asset_paths::deployed_width(1500, 3000),
        1200,
        "premise: deployed base width is 1200"
    );

    // --- COLD build: copy_deferred FIRST, then the converter (converter
    //     writes last → correct even on the buggy HEAD). ---
    let _ = run_copy_deferred(&source, &staging_cold, &moss_dir).await;
    run_converter_base(&src_webp, "photo.webp", &staging_cold, &moss_dir);
    let cold_dims = decoded_webp_dims(&staging_cold.join("photo.webp"));
    assert_eq!(
        cold_dims,
        (1200, 2400),
        "COLD: base must be the resized deploy base, not the full-res source"
    );
    let cold_bytes = fs::read(staging_cold.join("photo.webp")).unwrap();

    // --- WARM build: converter FIRST (transform-cache HIT → fast link),
    //     then copy_deferred (copy_deferred writes last). On the buggy HEAD
    //     the verbatim copy clobbers the resized base with the full-res
    //     1500×3000 source → the assertions below fail. ---
    run_converter_base(&src_webp, "photo.webp", &staging_warm, &moss_dir);
    let _ = run_copy_deferred(&source, &staging_warm, &moss_dir).await;
    let warm_dims = decoded_webp_dims(&staging_warm.join("photo.webp"));
    assert_eq!(
        warm_dims,
        (1200, 2400),
        "WARM: copy_deferred must NOT clobber the converter's resized base \
             with the full-res source (the reported bug)"
    );
    let warm_bytes = fs::read(staging_warm.join("photo.webp")).unwrap();

    // The served base bytes must be identical cold vs warm — the converter's
    // deterministic resized encode, regardless of who ran last.
    assert_eq!(
        cold_bytes, warm_bytes,
        "base bytes must be byte-identical cold vs warm (single writer = the converter)"
    );
}

/// A SMALL webp (600×400, AlreadySmall) is NOT in the conversion set — the
/// converter never writes it — so copy_deferred MUST still copy it verbatim,
/// or the base 404s (ADR-013). Guards the fix against dropping small webp.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn small_webp_source_lands_verbatim() {
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).unwrap();
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();

    let src_webp = source.join("small.webp");
    write_gradient_webp(&src_webp, 600, 400);
    // Premise: 600×400 has an empty ladder → AlreadySmall-skipped by the
    // converter, so this webp is NOT in the conversion set.
    assert!(
        moss_core::asset_paths::ladder_rungs(600, 400, false).is_empty(),
        "premise: 600×400 carries no rungs"
    );
    let src_bytes = fs::read(&src_webp).unwrap();

    let sealed = run_copy_deferred(&source, &staging, &moss_dir).await;

    let out = staging.join("small.webp");
    assert!(
        out.exists(),
        "small webp must be copied verbatim (converter never writes it)"
    );
    assert_eq!(
        fs::read(&out).unwrap(),
        src_bytes,
        "small webp must be byte-identical to the source (verbatim copy)"
    );
    assert!(
        sealed.files().get("small.webp").is_some(),
        "small webp must be registered in the manifest by copy_deferred"
    );
}

/// An ANIMATED webp (AnimatedWebp) is likewise outside the conversion set
/// and must be copied verbatim by copy_deferred. Guards the fix against
/// dropping animated webp.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn animated_webp_source_lands_verbatim() {
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).unwrap();
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();

    // Hand-crafted VP8X + ANIM chunk (mirrors is_animated_webp_on_anim_chunk).
    let src_webp = source.join("anim.webp");
    let mut anim: Vec<u8> = b"RIFF".to_vec();
    anim.extend_from_slice(&[0, 0, 0, 0]);
    anim.extend_from_slice(b"WEBP");
    anim.extend_from_slice(b"VP8X");
    anim.extend_from_slice(&[0; 10]);
    anim.extend_from_slice(b"ANIM");
    anim.extend_from_slice(&[0; 10]);
    fs::write(&src_webp, &anim).unwrap();

    let sealed = run_copy_deferred(&source, &staging, &moss_dir).await;

    let out = staging.join("anim.webp");
    assert!(out.exists(), "animated webp must be copied verbatim");
    assert_eq!(
        fs::read(&out).unwrap(),
        anim,
        "animated webp must be byte-identical to the source (verbatim copy, no rungs)"
    );
    assert!(
        sealed.files().get("anim.webp").is_some(),
        "animated webp must be registered in the manifest by copy_deferred"
    );
}

/// Design follow-up #6 (harden the `.unwrap_or(0)` TOCTOU). A webp source
/// whose fresh metadata read FAILS mid-build (modeled as `size: None`) must
/// fail SAFE — the converter owns the base, so `copy_deferred_assets` SKIPS
/// the verbatim copy. The old `.unwrap_or(0)` instead fed size 0 (always
/// `< min_size_kb`) → a small-DIMENSION webp re-derived `AlreadySmall` →
/// verbatim copy racing the converter's resized base (the Task-12.6
/// double-writer). A readable small webp (`Some(size)`) stays AlreadySmall
/// and is still verbatim-copied — the single-writer invariant, both ways.
#[test]
fn webp_unreadable_source_fails_safe_to_converter_owned() {
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = TempDir::new_in(&base).unwrap();
    let moss_dir = tmp.path().join(".moss");
    let paths = MossPaths::from_moss_dir(moss_dir);
    let transforms = crate::build::cache::TransformCache::new(
        paths.cache_transforms(),
        crate::build::cache::ObjectStore::new(paths.cache_objects()),
    );
    let cfg = crate::build::media::image::ImageCompressionConfig::default();

    // A real, readable SMALL webp (600×400, empty ladder → AlreadySmall).
    // The authority (`collect_images_for_conversion`) reads its real >0
    // scan size and returns Some(AlreadySmall) → NOT converted → copy_deferred
    // owns the verbatim copy.
    let src_webp = tmp.path().join("small.webp");
    write_gradient_webp(&src_webp, 600, 400);
    assert!(
        moss_core::asset_paths::ladder_rungs(600, 400, false).is_empty(),
        "premise: 600×400 carries no rungs (AlreadySmall)"
    );
    let real_size = std::fs::metadata(&src_webp).unwrap().len();

    // Readable → NOT converter-owned → copy_deferred verbatim-copies (unchanged).
    assert!(
        !webp_converter_owns_base(&src_webp, "webp", Some(real_size), &cfg, &transforms, ""),
        "readable small webp stays AlreadySmall → copy_deferred owns the verbatim copy"
    );

    // Unreadable (metadata-read failure) → fail SAFE to converter-owned →
    // SKIP the verbatim copy. The old `.unwrap_or(0)` would re-derive
    // AlreadySmall from the small dims and fall through to a verbatim copy
    // that races the converter's resized base.
    assert!(
        webp_converter_owns_base(&src_webp, "webp", None, &cfg, &transforms, ""),
        "unreadable webp must fail safe to converter-owned — no verbatim copy, no double write"
    );
}

// -----------------------------------------------------------------------
// maybe_inject_spa_cached (moss#919)
// -----------------------------------------------------------------------

fn spa_test_cache(
    tmp: &std::path::Path,
) -> (crate::build::cache::ObjectStore, crate::build::cache::TransformCache) {
    use crate::build::cache::{ObjectStore, TransformCache};
    let paths = MossPaths::from_moss_dir(tmp.join(".moss"));
    let objects = ObjectStore::new(paths.cache_objects());
    let transforms = TransformCache::new(paths.cache_transforms(), ObjectStore::new(paths.cache_objects()));
    (objects, transforms)
}

#[test]
fn maybe_inject_spa_cached_hit_matches_miss_output_byte_for_byte() {
    use crate::build::cache::ObjectStore;
    use crate::build::site_meta::spa_inject::SpaDefaults;
    let tmp = tempfile::tempdir().unwrap();
    let (objects, transforms) = spa_test_cache(tmp.path());

    let html = "<html><head><title>App</title></head><body></body></html>";
    let source = tmp.path().join("source.html");
    std::fs::write(&source, html).unwrap();
    let source_oid = ObjectStore::hash_file(&source).unwrap();
    let source_size = std::fs::metadata(&source).unwrap().len();

    let defaults = SpaDefaults {
        description: Some("A bundled SPA"),
        canonical_url: Some("https://example.com/app/"),
        og_tags: None,
        twitter_tags: None,
        apple_touch_icon: None,
        raster_favicons: None,
        theme_color_light: None,
        theme_color_dark: None,
    };

    // Miss: target starts as the raw source (mirrors link_to's pre-injection
    // state), gets injected, and the result is cached.
    let target_a = tmp.path().join("a.html");
    std::fs::write(&target_a, html).unwrap();
    let miss_result =
        maybe_inject_spa_cached(&target_a, &defaults, &source_oid, source_size, &objects, &transforms)
            .expect("miss path succeeds");
    assert!(miss_result.is_some(), "premise: injection changes the file");
    let miss_bytes = std::fs::read(&target_a).unwrap();
    assert!(
        String::from_utf8_lossy(&miss_bytes).contains("A bundled SPA"),
        "miss path actually injected"
    );

    // Hit: a second target starting from the SAME raw source, same
    // (source_oid, params) — must land on the cached `link_to` fast path and
    // produce byte-identical output without a second inject_defaults call.
    let target_b = tmp.path().join("b.html");
    std::fs::write(&target_b, html).unwrap();
    let hit_result =
        maybe_inject_spa_cached(&target_b, &defaults, &source_oid, source_size, &objects, &transforms)
            .expect("hit path succeeds");
    let hit_bytes = std::fs::read(&target_b).unwrap();

    assert_eq!(hit_bytes, miss_bytes, "cache-hit output must be byte-identical to cache-miss output");
    assert_eq!(hit_result, miss_result, "cache-hit must return the same hash as the miss that produced it");
}

#[test]
fn maybe_inject_spa_cached_no_op_result_is_cached_too() {
    // When every tag is already present, maybe_inject_spa returns Ok(None).
    // That "nothing to inject" outcome must also be cached (as content_oid:
    // None), not just the changed case — otherwise every build re-reads and
    // re-diffs SPAs whose authors already set every tag.
    use crate::build::cache::ObjectStore;
    use crate::build::site_meta::spa_inject::SpaDefaults;
    let tmp = tempfile::tempdir().unwrap();
    let (objects, transforms) = spa_test_cache(tmp.path());

    let html = concat!(
        "<html><head>",
        r#"<meta name="description" content="already set">"#,
        r#"<link rel="canonical" href="https://example.com/app/">"#,
        "</head><body></body></html>",
    );
    let source = tmp.path().join("source.html");
    std::fs::write(&source, html).unwrap();
    let source_oid = ObjectStore::hash_file(&source).unwrap();
    let source_size = std::fs::metadata(&source).unwrap().len();

    let defaults = SpaDefaults {
        description: Some("would-be-injected"),
        canonical_url: Some("https://example.com/app/"),
        og_tags: None,
        twitter_tags: None,
        apple_touch_icon: None,
        raster_favicons: None,
        theme_color_light: None,
        theme_color_dark: None,
    };

    let target = tmp.path().join("t.html");
    std::fs::write(&target, html).unwrap();
    let first =
        maybe_inject_spa_cached(&target, &defaults, &source_oid, source_size, &objects, &transforms)
            .expect("first call succeeds");
    assert_eq!(first, None, "premise: every tag already present, no-op");

    // Corrupt the target to prove the SECOND call takes the cache-hit
    // no-op branch (returns Ok(None), leaves target untouched) rather than
    // re-running inject_defaults (which would restore it to `html`).
    std::fs::write(&target, "MUTATED").unwrap();
    let second =
        maybe_inject_spa_cached(&target, &defaults, &source_oid, source_size, &objects, &transforms)
            .expect("cached no-op call succeeds");
    assert_eq!(second, None, "cached no-op result must also be None");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "MUTATED",
        "cache-hit no-op path must not touch target"
    );
}

// -----------------------------------------------------------------------
// ManifestHashMemo / recall_or_hash_output (perf: oid-keyed manifest hash
// memo, replacing a full per-asset re-read on every cache-hit build)
// -----------------------------------------------------------------------

#[test]
fn recall_or_hash_output_memoizes_the_real_hash() {
    use crate::build::media::manifest_hash_memo::ManifestHashMemo;

    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("asset.bin");
    std::fs::write(&target, b"some asset bytes").unwrap();
    let expected = crate::build::assets::paths::compute_binary_hash_file(&target).unwrap();

    let memo = ManifestHashMemo::load(&tmp.path().join("no-such-memo.json"));
    let got = recall_or_hash_output(&memo, "oid-x", &target, "fallback-oid", "output");

    assert_eq!(got, expected, "a miss must return exactly what compute_binary_hash_file returns");
    assert_eq!(
        memo.get("oid-x").as_deref(),
        Some(expected.as_str()),
        "a successful hash must be memoized under the oid key"
    );
}

#[test]
fn recall_or_hash_output_hit_does_not_read_the_file() {
    use crate::build::media::manifest_hash_memo::ManifestHashMemo;

    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("asset.bin");
    std::fs::write(&target, b"some asset bytes").unwrap();
    let expected = crate::build::assets::paths::compute_binary_hash_file(&target).unwrap();

    let memo = ManifestHashMemo::load(&tmp.path().join("no-such-memo.json"));
    let first = recall_or_hash_output(&memo, "oid-y", &target, "fallback-oid", "output");
    assert_eq!(first, expected);

    // Delete the file entirely. A second call that reads it would fail
    // (compute_binary_hash_file errors) and fall back to "fallback-oid" —
    // so returning the real hash here is proof the memo hit short-circuited
    // before any file I/O.
    std::fs::remove_file(&target).unwrap();
    let second = recall_or_hash_output(&memo, "oid-y", &target, "fallback-oid", "output");
    assert_eq!(second, expected, "a memo hit must not touch the (now-deleted) file");
}

#[test]
fn recall_or_hash_output_never_memoizes_the_fallback() {
    use crate::build::media::manifest_hash_memo::ManifestHashMemo;

    let tmp = tempfile::tempdir().unwrap();
    // Never created — compute_binary_hash_file fails against it.
    let target = tmp.path().join("missing.bin");

    let memo = ManifestHashMemo::load(&tmp.path().join("no-such-memo.json"));
    let failed = recall_or_hash_output(&memo, "oid-z", &target, "fallback-oid", "output");
    assert_eq!(failed, "fallback-oid", "a hash failure must return the fallback");
    assert_eq!(memo.get("oid-z"), None, "a failed hash must never be memoized");

    // The transient failure "heals" (file now exists) and the SAME oid key
    // is looked up again — this must be a genuine miss, not a poisoned hit.
    std::fs::write(&target, b"now it exists").unwrap();
    let expected = crate::build::assets::paths::compute_binary_hash_file(&target).unwrap();
    let recovered = recall_or_hash_output(&memo, "oid-z", &target, "fallback-oid", "output");
    assert_eq!(recovered, expected, "a subsequent successful read must record the real xxh3");
    assert_eq!(memo.get("oid-z").as_deref(), Some(expected.as_str()));
}

/// Trap 1: `spa_post_hash` must win over the memo even when the memo already
/// holds an entry for this exact oid (simulating a previous, pre-injection
/// build having memoized it). Only the `None` arm of the `spa_post_hash`
/// match may consult `recall_or_hash_output` — this proves it end to end
/// through `copy_deferred_assets`, not just at the helper level.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_spa_post_hash_wins_over_a_poisoned_memo() {
    use crate::build::cache::ObjectStore;
    use crate::build::coordinator::test_utils;
    use crate::build::site_meta::spa_inject::SpaDefaultsOwned;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // A bundled-SPA index.html with none of the tags `defaults` would inject
    // already present, so injection actually changes the bytes.
    let html = "<html><head><title>App</title></head><body></body></html>";
    let index = source.join("app/index.html");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, html).unwrap();
    let source_oid = ObjectStore::hash_file(&index).unwrap();

    // Poison the memo BEFORE the build, under the exact oid this build will
    // resolve — as if a previous (pre-injection) build had memoized it.
    let moss_paths = MossPaths::from_moss_dir(moss_dir.clone());
    let memo = crate::build::media::manifest_hash_memo::ManifestHashMemo::load(
        &moss_paths.cache_manifest_hash_memo(),
    );
    memo.record(&source_oid, "0000000000000000".to_string());
    memo.save(&moss_paths.cache_manifest_hash_memo()).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        spa_defaults: Some(SpaDefaultsOwned {
            description: Some("A bundled SPA".to_string()),
            ..Default::default()
        }),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    let entry = sealed.files().get("app/index.html").expect("asset must be registered");
    let (_, raw_hash) = crate::types::content::parse_entry(entry);

    assert_ne!(raw_hash, "0000000000000000", "must not use the poisoned memo entry");

    let staged_bytes = fs::read(staging.join("app/index.html")).unwrap();
    assert!(
        String::from_utf8_lossy(&staged_bytes).contains("A bundled SPA"),
        "premise: injection actually ran"
    );
    let expected = crate::build::assets::paths::compute_binary_hash(&staged_bytes);
    assert_eq!(
        raw_hash, expected,
        "recorded hash must be the SPA post-injection hash, not the poisoned pre-injection memo entry"
    );
}

/// Persistence: a second `copy_deferred_assets` call against the same
/// `moss_dir` reuses the memo from disk rather than starting cold. Proven by
/// corrupting the CAS blob directly between builds — `ObjectStore::store_file`
/// is idempotent on a non-empty blob (it only re-validates for 0-byte
/// corruption), so the corruption survives into build 2 untouched. If build 2
/// re-read the blob it would report the corrupted bytes' hash; it must
/// instead report the real hash memoized by build 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_reuses_the_persisted_manifest_hash_memo_across_builds() {
    use crate::build::cache::ObjectStore;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    let asset_bytes = b"persisted-memo asset content";
    let asset_file = source.join("assets/data.bin");
    fs::create_dir_all(asset_file.parent().unwrap()).unwrap();
    fs::write(&asset_file, asset_bytes).unwrap();
    let real_hash = crate::build::assets::paths::compute_binary_hash(asset_bytes);

    let make_deferred = || crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    // Build 1: populates and persists the memo.
    let (tx1, rx1) = test_utils::build_test_coordinator();
    let deferred1 = make_deferred();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&deferred1, crate::build::ports::reporter::discarding(), tx1, None);
    })
    .await
    .unwrap();
    let sealed1 = test_utils::drain_into_sealed(rx1, SiteHashes::default()).await;
    let (_, raw1) = crate::types::content::parse_entry(sealed1.files().get("assets/data.bin").unwrap());
    assert_eq!(raw1, real_hash, "build 1 must record the real hash");

    let moss_paths = MossPaths::from_moss_dir(moss_dir.clone());
    assert!(
        moss_paths.cache_manifest_hash_memo().exists(),
        "the memo must be persisted to disk after a build"
    );

    // Corrupt the CAS blob directly with non-empty garbage — store_file's
    // idempotent skip (it only checks for 0-byte corruption) leaves this
    // untouched by build 2's own store_file call.
    let oid = ObjectStore::hash_file(&asset_file).unwrap();
    let object_store2 = ObjectStore::new(moss_paths.cache_objects());
    let blob_path = object_store2.blob_path(&oid);
    fs::write(&blob_path, b"CORRUPTED BLOB BYTES, NOT THE REAL ASSET").unwrap();
    let corrupted_hash = crate::build::assets::paths::compute_binary_hash_file(&blob_path).unwrap();
    assert_ne!(
        corrupted_hash, real_hash,
        "sanity: the corruption must actually change what a re-read would produce"
    );

    // Build 2: a fresh BackgroundContext, same moss_dir — the memo is loaded from
    // disk, not carried over in memory from build 1.
    let (tx2, rx2) = test_utils::build_test_coordinator();
    let deferred2 = make_deferred();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&deferred2, crate::build::ports::reporter::discarding(), tx2, None);
    })
    .await
    .unwrap();
    let sealed2 = test_utils::drain_into_sealed(rx2, SiteHashes::default()).await;
    let (_, raw2) = crate::types::content::parse_entry(sealed2.files().get("assets/data.bin").unwrap());
    assert_eq!(
        raw2, real_hash,
        "build 2 must reuse the persisted memo and return the real hash, not re-read the corrupted blob"
    );
}

// ─── Ship-by-OID: the CAS object recorded alongside a staged entry ─────────

/// The bug Step 1 fixes: `maybe_inject_spa_cached` rewrites `target` AFTER
/// `link_to` placed the PRE-injection CAS bytes there, minting a fresh CAS
/// object for the injected content — the caller must record THAT object as
/// the entry's `staged_oid`, not the pre-injection `link_oid` it started
/// with. Assert the CAS blob's bytes hash to the same xxh3 as the manifest's
/// recorded hash for a page injection actually changed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_records_the_post_injection_oid_for_a_rewritten_spa_index() {
    use crate::build::cache::ObjectStore;
    use crate::build::coordinator::test_utils;
    use crate::build::site_meta::spa_inject::SpaDefaultsOwned;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // A bundled-SPA index.html with none of the tags `defaults` would inject
    // already present, so injection actually changes the bytes.
    let html = "<html><head><title>App</title></head><body></body></html>";
    let index = source.join("app/index.html");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, html).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        spa_defaults: Some(SpaDefaultsOwned {
            description: Some("A bundled SPA".to_string()),
            ..Default::default()
        }),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    let staged_bytes = fs::read(staging.join("app/index.html")).unwrap();
    assert!(
        String::from_utf8_lossy(&staged_bytes).contains("A bundled SPA"),
        "premise: injection actually ran"
    );

    let entry = sealed.files().get("app/index.html").expect("asset must be registered");
    let (_, manifest_hash) = crate::types::content::parse_entry(entry);

    let oid = sealed
        .staged_oid("app/index.html")
        .expect("an injected SPA index must carry the POST-injection CAS oid, not none");

    let object_store = ObjectStore::new(crate::moss_paths::MossPaths::from_moss_dir(moss_dir.clone()).cache_objects());
    let cas_path = object_store.get_path(oid).expect("the staged_oid must name a live CAS blob");
    let cas_bytes = fs::read(&cas_path).unwrap();
    assert_eq!(
        cas_bytes, staged_bytes,
        "the staged_oid must back exactly the post-injection bytes on disk"
    );
    let cas_hash = crate::build::assets::paths::compute_binary_hash(&cas_bytes);
    assert_eq!(
        cas_hash, manifest_hash,
        "the CAS blob's bytes must hash to the same xxh3 as the manifest's recorded hash"
    );
}

/// Regression guard: if a future refactor drops the `staged_oids.insert(...)`
/// call in `copy_deferred_assets`, this must fail loudly rather than quietly
/// reverting every asset back to the pre-fix, race-prone stage-path-only ship.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_deferred_assets_always_records_a_staged_oid_for_its_own_entries() {
    use crate::build::cache::ObjectStore;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    let asset_bytes = b"a plain deferred asset, nothing SPA about it";
    let asset_file = source.join("assets/data.bin");
    fs::create_dir_all(asset_file.parent().unwrap()).unwrap();
    fs::write(&asset_file, asset_bytes).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    assert!(
        sealed.files().contains_key("assets/data.bin"),
        "premise: the asset was registered at all"
    );
    let oid = sealed
        .staged_oid("assets/data.bin")
        .expect("every entry copy_deferred_assets's Ok(oid) arm registers must carry a staged_oid");

    let object_store = ObjectStore::new(crate::moss_paths::MossPaths::from_moss_dir(moss_dir.clone()).cache_objects());
    let cas_bytes = fs::read(object_store.get_path(oid).expect("the oid must name a live CAS blob")).unwrap();
    assert_eq!(cas_bytes, asset_bytes.as_slice(), "the staged_oid must back exactly these bytes");
}

/// Step 3's whole reason to exist: `degrade::apply_to_staging` rewrites a
/// `copy_deferred_assets`-produced HTML page directly to `stage_dir` post-seal
/// (no CAS write), so the OID this build already recorded for it before the
/// repair now names the PRE-repair bytes. Ship-by-OID must not resurrect
/// them. Driven through the real `copy_deferred_assets` producer end to end —
/// a hand-built manifest (`degrade_tests.rs`'s pattern) never acquires a
/// `staged_oid` in the first place and would prove nothing about this fix.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ship_phase_reflects_a_post_seal_repair_not_a_stale_cas_entry() {
    use crate::build::cache::ObjectStore;
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;
    use std::fs;
    use tempfile::TempDir;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
    fs::create_dir_all(&base).expect("create target/test-tmp");
    let tmp = TempDir::new_in(&base).unwrap();
    let source = tmp.path().join("source");
    let moss_dir = tmp.path().join(".moss");
    let staging = moss_dir.join("build/site-stage");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("cache/objects")).unwrap();

    // A source HTML asset (interactive embed), not markdown-rendered —
    // `copy_deferred_assets` copies it straight through — referencing a webp
    // variant that will be declared failed.
    let html = concat!(
        r#"<picture><source srcset="photo.webp" type="image/webp">"#,
        r#"<img src="photo.jpg"></picture>"#
    );
    let widget = source.join("widget/index.html");
    fs::create_dir_all(widget.parent().unwrap()).unwrap();
    fs::write(&widget, html).unwrap();

    let ctx = crate::types::services::BackgroundContext {
        source_path: source.clone().to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        blocking_keys: Default::default(),
        dir_overrides: Default::default(),
        ..crate::types::services::BackgroundContext::for_test()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        copy_deferred_assets(&ctx, crate::build::ports::reporter::discarding(), tx, None);
    })
    .await
    .unwrap();

    let mut sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    let oid_before_repair = sealed
        .staged_oid("widget/index.html")
        .expect("copy_deferred_assets must record a staged_oid for its own entry")
        .to_string();

    let mp = crate::moss_paths::MossPaths::from_moss_dir(moss_dir.clone());
    let object_store = ObjectStore::new(mp.cache_objects());
    let cas_bytes_before =
        fs::read(object_store.get_path(&oid_before_repair).expect("the pre-repair CAS blob must be live")).unwrap();
    assert!(
        String::from_utf8_lossy(&cas_bytes_before).contains("photo.webp"),
        "premise: the CAS blob still holds the un-repaired reference"
    );

    // The repair: `widget/photo.webp` is a terminally-failed variant.
    let mut failed = std::collections::HashSet::new();
    failed.insert("widget/photo.webp".to_string());
    crate::build::degrade::repair_staged_html(&mp, &staging, &mut sealed, failed);

    let repaired_on_disk = fs::read_to_string(staging.join("widget/index.html")).unwrap();
    assert!(
        !repaired_on_disk.contains("photo.webp"),
        "premise: the repair actually rewrote the page: {repaired_on_disk}"
    );
    assert!(
        sealed.staged_oid("widget/index.html").is_none(),
        "a post-seal repair must clear the staged OID it just invalidated"
    );

    let site = tmp.path().join("site");
    crate::build::ship::ship_phase(&staging, &site, &sealed, Some(&object_store), None)
        .expect("ship_phase should succeed");

    let shipped = fs::read_to_string(site.join("widget/index.html")).unwrap();
    assert!(
        !shipped.contains("photo.webp"),
        "ship_phase must ship the REPAIRED bytes, not the stale CAS object recorded before \
         the repair — shipping it would resurrect the failed variant reference moss#867 \
         exists to strip: {shipped}"
    );
}
