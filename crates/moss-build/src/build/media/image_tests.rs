use super::*;
use crate::types::assets::{AssetRegistry, AssetState};
use crate::types::content::{MediaMetadata, SiteHashes};
use image::{DynamicImage, ImageBuffer, Rgb};
// The deployed raster fallback moved to the sibling `fallback_raster` module;
// its tests still live here, next to the WebP-pass tests they share fixtures
// with (`make_detailed_jpeg`, `make_transparent_png`).
use crate::build::media::fallback_raster::{
    encode_sized_raster, sized_raster_oid_for_original, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY,
};

// ----- Config params round-trip -----

#[test]
fn to_params_contains_encoding_fields() {
    let cfg = ImageCompressionConfig {
        quality: 75,
        max_edge: 1600,
        strip_exif: false,
        min_size_kb: 100,
    };
    let v = cfg.to_params();
    assert_eq!(v["quality"], 75);
    assert_eq!(v["max_edge"], 1600);
    assert_eq!(v["strip_exif"], false);
    assert_eq!(v["flatten_alpha"], true);
    // min_size_kb must NOT be in cache params.
    assert!(v.get("min_size_kb").is_none());
}

#[test]
fn to_params_deterministic_for_same_config() {
    let a = ImageCompressionConfig::default().to_params();
    let b = ImageCompressionConfig::default().to_params();
    assert_eq!(a.to_string(), b.to_string());
}

// ----- EXIF orientation — 8 cases -----

/// Build a 2×4 RGB test image where each pixel has a distinct color so
/// the 8 EXIF transforms are each individually distinguishable.
fn orientation_fixture() -> DynamicImage {
    // 2 columns × 4 rows. Encode coordinates into RGB so we can read
    // them back from the transformed image.
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(2, 4, |x, y| Rgb([x as u8 * 100, y as u8 * 50, 0]));
    DynamicImage::ImageRgb8(img)
}

fn dims(img: &DynamicImage) -> (u32, u32) {
    (img.width(), img.height())
}

#[test]
fn orientation_1_identity() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 1);
    assert_eq!(dims(&out), dims(&img));
    assert_eq!(out.to_rgb8().get_pixel(0, 0), img.to_rgb8().get_pixel(0, 0));
}

#[test]
fn orientation_2_flip_horizontal() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 2);
    // fliph: pixel at (x, y) comes from (w-1-x, y)
    let expected = img.to_rgb8().get_pixel(1, 0).0;
    assert_eq!(out.to_rgb8().get_pixel(0, 0).0, expected);
}

#[test]
fn orientation_3_rotate_180() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 3);
    let expected = img.to_rgb8().get_pixel(1, 3).0;
    assert_eq!(out.to_rgb8().get_pixel(0, 0).0, expected);
}

#[test]
fn orientation_4_flip_vertical() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 4);
    let expected = img.to_rgb8().get_pixel(0, 3).0;
    assert_eq!(out.to_rgb8().get_pixel(0, 0).0, expected);
}

#[test]
fn orientation_5_transpose() {
    // 2x4 → after transpose-like op: 4x2
    let img = orientation_fixture();
    let out = apply_exif_orientation(img, 5);
    assert_eq!(dims(&out), (4, 2));
}

#[test]
fn orientation_6_rotate_90_cw() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 6);
    assert_eq!(dims(&out), (4, 2));
    // rotate_90: (x,y) → (h-1-y, x). So new (0,0) = old (0,3).
    let expected = img.to_rgb8().get_pixel(0, 3).0;
    assert_eq!(out.to_rgb8().get_pixel(0, 0).0, expected);
}

#[test]
fn orientation_7_transverse() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img, 7);
    assert_eq!(dims(&out), (4, 2));
}

#[test]
fn orientation_8_rotate_90_ccw() {
    let img = orientation_fixture();
    let out = apply_exif_orientation(img.clone(), 8);
    assert_eq!(dims(&out), (4, 2));
    // rotate_270 aka rotate_90_ccw: (x,y) → (y, w-1-x). Old (1,0) → new (0,0).
    let expected = img.to_rgb8().get_pixel(1, 0).0;
    assert_eq!(out.to_rgb8().get_pixel(0, 0).0, expected);
}

// ----- validate_webp_output -----

#[test]
fn validate_webp_accepts_valid_output() {
    // Encode a real image to WebP then validate.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(8, 8, |_, _| Rgb([200, 100, 50]));
    let img = DynamicImage::ImageRgb8(buf);
    let bytes = encode_webp(&img, 80, None).unwrap();
    assert!(validate_webp_output(&bytes, (8, 8)).is_ok());
}

#[test]
fn validate_webp_rejects_truncated() {
    let bad = vec![0u8; 8];
    assert!(validate_webp_output(&bad, (10, 10)).is_err());
}

#[test]
fn validate_webp_rejects_dim_mismatch() {
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(8, 8, |_, _| Rgb([0, 0, 0]));
    let img = DynamicImage::ImageRgb8(buf);
    let bytes = encode_webp(&img, 80, None).unwrap();
    assert!(validate_webp_output(&bytes, (16, 16)).is_err());
}

// ----- Fingerprint determinism -----

#[test]
fn fingerprint_deterministic_same_input() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_path = root.join("a.jpg");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |_, _| Rgb([1, 2, 3]));
    let img = DynamicImage::ImageRgb8(buf);
    img.save_with_format(&img_path, image::ImageFormat::Jpeg)
        .unwrap();

    let cfg = ImageCompressionConfig::default();
    let source_str = root.to_string_lossy().to_string();
    let fp1 = compute_image_item_fingerprint(&source_str, Path::new("a.jpg"), &cfg);
    let fp2 = compute_image_item_fingerprint(&source_str, Path::new("a.jpg"), &cfg);
    assert_eq!(fp1, fp2);
    assert!(fp1.is_some(), "source exists and is stat-able");
}

/// An unstat-able source (never written) must not produce a fingerprint —
/// the caller (`dispatch_image_conversions`) treats `None` as "cannot prove
/// unchanged" and dispatches it rather than caching a bogus value.
#[test]
fn fingerprint_missing_source_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ImageCompressionConfig::default();
    let fp = compute_image_item_fingerprint(
        &tmp.path().to_string_lossy(),
        Path::new("missing.jpg"),
        &cfg,
    );
    assert!(fp.is_none(), "an unstat-able source must not produce a fingerprint");
}

/// A config change re-dispatches every image even when no file moved: the compression
/// params are part of what the gate compares.
#[test]
fn the_fingerprint_moves_with_the_compression_config() {
    let tmp = tempfile::tempdir().unwrap();
    make_big_jpeg(&tmp.path().join("a.jpg"), 40, 30);
    let root = tmp.path().to_string_lossy().to_string();
    let fp = |config: &ImageCompressionConfig| compute_image_item_fingerprint(&root, Path::new("a.jpg"), config);

    let default = ImageCompressionConfig::default();
    assert_ne!(fp(&default), fp(&ImageCompressionConfig { quality: default.quality - 10, ..default.clone() }));
}

/// What `compute_image_item_fingerprint` feeds the fingerprint: the file's whole stat
/// record, as `FileStat::of` reads it — not a subset of it.
#[test]
fn the_fingerprint_of_a_file_is_the_fingerprint_of_its_whole_stat_record() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a.jpg");
    make_big_jpeg(&file, 40, 30);
    let cfg = ImageCompressionConfig::default();

    let stat = crate::build::cache::FileStat::of(&fs::metadata(&file).unwrap());
    assert_eq!(
        compute_image_item_fingerprint(&tmp.path().to_string_lossy(), Path::new("a.jpg"), &cfg),
        Some(ImageFingerprint { stat, params: cfg.to_params() }),
    );
}

/// Replace-via-rename with size and mtime kept: only the inode says the image is a
/// different file, and the dispatch gate must not carry the old variant forward over it.
#[cfg(unix)]
#[test]
fn an_image_replaced_by_rename_with_its_size_and_mtime_kept_has_a_new_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a.jpg");
    make_big_jpeg(&file, 40, 30);
    let cfg = ImageCompressionConfig::default();
    let root = tmp.path().to_string_lossy().to_string();
    let before = compute_image_item_fingerprint(&root, Path::new("a.jpg"), &cfg);

    let mut bytes = fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    crate::build::cache::FileStat::replace_by_rename_keeping_mtime(&file, &bytes);

    assert_ne!(compute_image_item_fingerprint(&root, Path::new("a.jpg"), &cfg), before);
}

// ----- convert_single_image happy path + cache hit + legacy sentinel handling -----

/// Build an oversized JPEG that WebP will shrink.
fn make_big_jpeg(path: &Path, w: u32, h: u32) {
    // A mostly-solid color image compresses well as WebP but JPEG with
    // quality 90 on this size will produce a bigger file than a q80 WebP.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(w, h, |x, y| Rgb([(x as u8).wrapping_add(y as u8), 100, 50]));
    let img = DynamicImage::ImageRgb8(buf);
    img.save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

/// Write a real, non-animated WebP of the given pixel dimensions. A smooth
/// gradient compresses to a tiny WebP regardless of dimensions, so the file
/// stays comfortably under any reasonable `min_size_kb` gate — which lets a
/// large-*dimensioned* webp still be small-*file*, the exact combination the
/// rung-aware AlreadySmall carve-out targets. `image::image_dimensions`
/// reads the header back the same way the scan does.
fn make_webp(path: &Path, w: u32, h: u32) {
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(w, h, |x, y| Rgb([(x as u8).wrapping_add(y as u8), 100, 50]));
    let img = DynamicImage::ImageRgb8(buf);
    let bytes = encode_webp(&img, 80, None).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Build a tiny image that WebP will NOT shrink. The encode runs anyway
/// (no skip on size) — exercises the "write WebP regardless" path.
fn make_tiny_noise_jpeg(path: &Path) {
    // Very small, highly-noisy image → WebP header overhead dominates,
    // encoded size >= original JPEG.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |x, y| {
        Rgb([
            ((x * 53) ^ (y * 97)) as u8,
            ((x * 17) ^ (y * 31)) as u8,
            ((x * 11) ^ (y * 7)) as u8,
        ])
    });
    let img = DynamicImage::ImageRgb8(buf);
    img.save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

struct TestHarness {
    _tmp: tempfile::TempDir,
    objects: crate::build::cache::ObjectStore,
    transforms: crate::build::cache::TransformCache,
    staging: PathBuf,
    temp: PathBuf,
}

fn harness() -> TestHarness {
    use crate::build::cache::{ObjectStore, TransformCache};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let objects_root = root.join("objects");
    let transforms_root = root.join("transforms");
    let staging = root.join("staging");
    let temp = root.join("tmp");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(&temp).unwrap();
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(transforms_root, ObjectStore::new(objects_root));
    TestHarness {
        _tmp: tmp,
        objects,
        transforms,
        staging,
        temp,
    }
}

#[test]
fn convert_single_image_happy_path() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);

    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    assert!(
        outcome.webp_oid.is_some(),
        "expected WebP to shrink the JPEG"
    );
    let oid = outcome.webp_oid.unwrap();
    assert!(
        h.objects.get_path(&oid).is_some(),
        "blob should exist in CAS"
    );
    assert!(
        h.staging.join("photo.webp").exists(),
        "staging WebP missing"
    );

    // Transform record has an image/webp entry with that oid.
    let record = h.transforms.get(&source_oid).unwrap();
    let entry = record.transforms.get("image/webp").unwrap();
    assert_eq!(entry.oid, oid);
    assert_eq!(entry.size, outcome.webp_size);
}

// ----- Task 5: ladder rung encodes -----

/// Decoded (width, height) of a staged webp output file.
fn staged_webp_dims(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
        .unwrap_or_else(|e| panic!("decode {}: {e}", path.display()));
    (img.width(), img.height())
}

/// Test 1 (plan Task 5 Step 2): a 2000×1200 JPEG through the conversion
/// path produces the base webp AND both ladder rungs, each with EXACTLY
/// the rung width (arch review: `resize_exact`, never bounding-box
/// `resize`, which can undershoot on adversarial ratios).
#[test]
fn rung_encode_produces_exact_width_ladder_files() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 2000, 1200);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    // Base: 2000 ≤ 2400 cap → deployed at natural width.
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.webp")),
        (2000, 1200)
    );
    // Rungs: EXACT widths, aspect-preserving integer heights.
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.w800.webp")),
        (800, 480),
        "w800 rung must decode at EXACTLY 800px wide"
    );
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.w1600.webp")),
        (1600, 960),
        "w1600 rung must decode at EXACTLY 1600px wide"
    );

    // Rungs are cached under their own transform kinds (mirrors the
    // "image/sized-raster" idiom) so warm rebuilds link instead of
    // re-encoding.
    let record = h.transforms.get(&source_oid).unwrap();
    assert!(record.transforms.contains_key("image/webp-w800"));
    assert!(record.transforms.contains_key("image/webp-w1600"));

    // The outcome carries per-rung results for the dispatch layer
    // (set_ready / manifest registration per rung).
    assert_eq!(outcome.rungs.len(), 2);
    assert!(
        outcome
            .rungs
            .iter()
            .all(|r| r.oid.is_some() && r.error.is_none()),
        "all rungs must succeed: {:?}",
        outcome.rungs
    );
}

/// Test 2: a source below the ladder threshold produces NO rung files.
#[test]
fn rung_encode_below_threshold_produces_no_rung_files() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 700, 500);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    assert!(
        h.staging.join("photo.webp").exists(),
        "base webp must exist"
    );
    assert!(
        !h.staging.join("photo.w800.webp").exists(),
        "no w800 rung below the ladder threshold"
    );
    assert!(
        !h.staging.join("photo.w1600.webp").exists(),
        "no w1600 rung below the ladder threshold"
    );
    assert!(
        outcome.rungs.is_empty(),
        "outcome must carry no rungs: {:?}",
        outcome.rungs
    );
}

/// Test 3: portrait 1500×3000 — the 2400 cap binds on the HEIGHT, so the
/// deployed base is 1200×2400 and the ladder is [800] only (height-aware
/// `ladder_rungs(w, h, …)`, revised at Task-3 review — a `(w, w)` call
/// would wrongly include 1600 here).
#[test]
fn rung_encode_portrait_ladder_follows_deployed_width() {
    let h = harness();
    let src = h._tmp.path().join("tall.jpg");
    make_big_jpeg(&src, 1500, 3000);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "tall.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    assert_eq!(
        staged_webp_dims(&h.staging.join("tall.webp")),
        (1200, 2400),
        "base deploys height-capped at 2400"
    );
    assert_eq!(
        staged_webp_dims(&h.staging.join("tall.w800.webp")),
        (800, 1600),
        "w800 rung must be EXACT (3000·800/1500 = 1600 tall)"
    );
    assert!(
        !h.staging.join("tall.w1600.webp").exists(),
        "1600 is NOT below the deployed base width (1200) — no w1600 rung"
    );
    assert_eq!(outcome.rungs.len(), 1);
}

/// Phase B (Task 12): a WEBP SOURCE is a ladder source too. A 2000×1200
/// webp re-encodes its base webp→webp at the SAME relative name
/// (`to_webp("photo.webp") == "photo.webp"`) and produces both ladder
/// rungs from the decoded original — the EXACT rung width set (`[800,
/// 1600]` = `ladder_rungs(2000, 1200, false)`) that emission promises on
/// `<img srcset>` and registration promises in the AssetRegistry. This is
/// the encode leg of the emission↔registration↔encode agreement.
#[test]
fn rung_encode_produces_ladder_for_webp_source() {
    let h = harness();
    let src = h._tmp.path().join("photo.webp");
    make_webp(&src, 2000, 1200);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    // Sanity: the source really is a ladder source with a non-empty ladder.
    assert!(moss_core::asset_paths::is_ladder_source_ext("webp"));
    assert_eq!(
        moss_core::asset_paths::ladder_rungs(2000, 1200, false),
        &[800, 1600][..],
        "the width set encode/emission/registration all derive"
    );

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    // Base webp landed at the source's OWN relative path (webp→webp).
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.webp")),
        (2000, 1200)
    );
    // Rungs: EXACT widths — the same URLs `<img srcset>` references.
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.w800.webp")),
        (800, 480)
    );
    assert_eq!(
        staged_webp_dims(&h.staging.join("photo.w1600.webp")),
        (1600, 960)
    );
    assert_eq!(outcome.rungs.len(), 2);
    assert!(
        outcome
            .rungs
            .iter()
            .all(|r| r.oid.is_some() && r.error.is_none()),
        "all webp rungs must succeed: {:?}",
        outcome.rungs
    );
}

/// A rung-FREE webp source (600×400 → empty ladder) re-encodes the base
/// only — NO rung files, NO stranded promises. In production such a small
/// webp is AlreadySmall-skipped before `convert_single_image` (see
/// `should_skip_already_small_for_rungless_webp`); this pins the encode
/// leg's agreement for the case that DOES reach it (e.g. a webp just over
/// the size gate but under the first rung), matching emission's bare
/// `<img>` (no srcset) for the same dims.
#[test]
fn rung_encode_webp_source_below_threshold_produces_no_rungs() {
    let h = harness();
    let src = h._tmp.path().join("small.webp");
    make_webp(&src, 600, 400);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    assert!(
        moss_core::asset_paths::ladder_rungs(600, 400, false).is_empty(),
        "600×400 must have no rungs for this test's premise"
    );

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "small.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );
    assert!(
        h.staging.join("small.webp").exists(),
        "base webp must exist"
    );
    assert!(
        !h.staging.join("small.w800.webp").exists(),
        "no rung below the ladder threshold"
    );
    assert!(
        outcome.rungs.is_empty(),
        "outcome must carry no rungs: {:?}",
        outcome.rungs
    );
}

/// Test 4 (cross-check promised at Task-3 review): for synthetic
/// portraits, the DECODED base-webp width must agree with moss-core's
/// `deployed_width` integer math within the documented ±1px tolerance —
/// pinning the srcset `w`-descriptor against the real encoder.
///
/// Vectors (trimmed at review — was 3024×4032, an exact 3:4 ratio that
/// can't exercise truncation): 2411×3201 divides 2411·2400/3201 =
/// 1807.68 — truncation (1807) vs the encoder's rounding (1808) is
/// exactly the ±1 drift the tolerance documents; 1179×3200 lands at
/// 884.25.
#[test]
fn base_webp_width_matches_deployed_width_within_one_px() {
    for (w, hh) in [(2411u32, 3201u32), (1179, 3200)] {
        let h = harness();
        let src = h._tmp.path().join("p.jpg");
        make_big_jpeg(&src, w, hh);
        let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
        let cfg = ImageCompressionConfig::default();

        let outcome = convert_single_image(
            &src,
            &source_oid,
            "p.webp",
            &h.temp,
            &h.staging,
            &h.objects,
            &h.transforms,
            &cfg,
            None,
            None,
            &HashMap::new(),
        );
        assert!(outcome.error.is_none(), "{w}x{hh}: {:?}", outcome.error);

        let got = staged_webp_dims(&h.staging.join("p.webp")).0;
        let want = moss_core::asset_paths::deployed_width(w, hh);
        assert!(
            (got as i64 - want as i64).abs() <= 1,
            "{w}x{hh}: encoder produced {got}w, deployed_width says {want}w — \
                 drift beyond the documented ±1px tolerance"
        );
    }
}

/// Base-failure promise retraction (review fix): blocking.rs registers
/// the rung URLs as Pending BEFORE the worker runs; when the BASE
/// conversion fails, the worker must `set_failed` every registered rung
/// too — otherwise those URLs sit Pending forever behind the
/// LQIP/passthrough placeholder (honest-mirror Layer 5B stuck-Pending
/// class). Driven through `run_image_conversion` with a magic-valid but
/// undecodable JPEG whose SCAN dims claim a full ladder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn base_failure_fails_registered_rung_promises() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    // JPEG magic bytes + garbage: passes has_image_magic, fails decode.
    let mut corrupt = vec![0xFF, 0xD8, 0xFF, 0xE0];
    corrupt.extend_from_slice(&[0xAB; 4096]);
    fs::write(root.join(img_rel), &corrupt).unwrap();

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&root.join(img_rel)).unwrap();

    // Mirror blocking.rs: base + rungs registered Pending up front
    // (scan dims 2000×1200 → ladder [800, 1600]).
    let registry = std::sync::Arc::new(AssetRegistry::new());
    for url in ["photo.webp", "photo.w800.webp", "photo.w1600.webp"] {
        registry.set_pending(url.to_string(), Some((2000, 1200)), None);
    }

    let run_ctx = ImageRunContext {
        items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: Some((2000, 1200)),
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: None,
        rung_collisions: Default::default(),
    };

    let mut services = BuildServices::headless();
    services.assets = Some(registry.clone());
    let services = std::sync::Arc::new(services);
    services.begin_ui_bound();
    let services_clone = services.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    for url in ["photo.webp", "photo.w800.webp", "photo.w1600.webp"] {
        match registry.get(url) {
            Some(crate::types::assets::AssetState::Failed(_)) => {}
            other => panic!(
                "{url} must transition to Failed on base conversion failure \
                     (never left Pending), got: {other:?}"
            ),
        }
    }
}

/// A base-failure advisory's `item` must be the site-relative path a click
/// can actually open, not a bare filename. Before the fix, `base_failed`
/// stripped the source path down to `Path::file_name()`; a source nested
/// under a subdirectory (the field bug: a photo under `图片/摄影/`) then
/// pointed the "open this file" click at `<site_root>/<basename>`, which
/// exists nowhere. Same corrupt-JPEG trick as
/// `base_failure_fails_registered_rung_promises`, but nested, and captured
/// through the `BuildReporter` port — the only channel a headless worker
/// (no Job registry, no window) hands an advisory out through.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn base_failed_advisory_names_the_full_nested_source_path() {
    use crate::build::ports::reporter::BuildReporter;

    #[derive(Default)]
    struct RecordingReporter(std::sync::Mutex<Vec<PipelineEvent>>);
    impl BuildReporter for RecordingReporter {
        fn report(&self, event: &PipelineEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "图片/摄影/1570c6b2.jpg";
    fs::create_dir_all(root.join("图片/摄影")).unwrap();
    // JPEG magic bytes + garbage: passes has_image_magic, fails decode.
    let mut corrupt = vec![0xFF, 0xD8, 0xFF, 0xE0];
    corrupt.extend_from_slice(&[0xAB; 4096]);
    fs::write(root.join(img_rel), &corrupt).unwrap();

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&root.join(img_rel)).unwrap();

    let run_ctx = ImageRunContext {
        items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: None,
        rung_collisions: Default::default(),
    };

    let recorder = std::sync::Arc::new(RecordingReporter::default());
    let mut services = BuildServices::headless();
    services.reporter = recorder.clone();
    let services = std::sync::Arc::new(services);
    services.begin_ui_bound();
    let services_clone = services.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    let advisory = recorder
        .0
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
            PipelineEvent::BackgroundProgress { completed: true, advisories, .. } => {
                advisories.iter().find(|a| a.item.is_some()).cloned()
            }
            _ => None,
        })
        .expect("the completion event must carry the base-failure advisory");

    let item = advisory.item.expect("advisory names a file");
    assert_eq!(
        item, img_rel,
        "advisory item must be the full nested site-relative source path, not a basename"
    );
    // The guard for the whole class: whatever `item` names, it must actually
    // resolve under the site root — this is what the frontend's click-to-open
    // relies on (`resolveAgainstFolder`, which joins the folder + item verbatim).
    assert!(
        root.join(&item).exists(),
        "site_root.join(item) must resolve to the real source file on disk"
    );
}

/// A source moss cannot hash must retract its promises, not go quiet.
///
/// The hash-failure arm was the one failure in the worker that told nobody:
/// it logged, released its UiBound permit and returned, so the base `.webp`
/// and every rung `blocking.rs` had promised stayed Pending forever. The
/// author saw a placeholder that never resolved and no advisory explaining
/// it, and the published site referenced a `.webp` that was never staged —
/// a `<source>` 404 `<picture>` cannot recover from (ADR-013).
///
/// The cloud gate above this arm already claims the one transient reason a
/// readable file fails to hash, so what is left is a real I/O failure and
/// gets a real failure's answer. Driven through `run_image_conversion` with
/// an empty `source_oid` (forcing the deferred hash) over a path that is a
/// DIRECTORY: it exists, it is not in the cloud, and reading it fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unhashable_source_fails_its_promises_instead_of_going_quiet() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    fs::create_dir_all(root.join(img_rel)).unwrap();

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    // Mirror blocking.rs: base + rungs registered Pending up front
    // (scan dims 2000×1200 → ladder [800, 1600]).
    let registry = std::sync::Arc::new(AssetRegistry::new());
    for url in ["photo.webp", "photo.w800.webp", "photo.w1600.webp"] {
        registry.set_pending(url.to_string(), Some((2000, 1200)), None);
    }

    let run_ctx = ImageRunContext {
        items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            // Empty: the blocking collect defers hashing, so the worker
            // resolves it — and this is the source that cannot be read.
            source_oid: String::new(),
            ext: "jpg".to_string(),
            dimensions: Some((2000, 1200)),
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: None,
        rung_collisions: Default::default(),
    };

    let mut services = BuildServices::headless();
    services.assets = Some(registry.clone());
    let services = std::sync::Arc::new(services);
    services.begin_ui_bound();
    let services_clone = services.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    for url in ["photo.webp", "photo.w800.webp", "photo.w1600.webp"] {
        match registry.get(url) {
            Some(crate::types::assets::AssetState::Failed(_)) => {}
            other => panic!(
                "{url} must transition to Failed when the source cannot be hashed \
                 (never left Pending behind a placeholder that never resolves), got: {other:?}"
            ),
        }
    }
}

/// Test 5: collision — a vault file occupying a rung path survives the
/// build byte-for-byte (the worker must NOT overwrite it) while the
/// non-colliding rung is produced normally. Runs through the WORKER
/// (`run_image_conversion`) so the `ImageRunContext::rung_collisions`
/// threading is exercised, with a coordinator to prove the collided path
/// never enters the manifest as an image variant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rung_collision_keeps_user_file_and_produces_other_rungs() {
    use crate::build::coordinator::test_utils;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    make_big_jpeg(&root.join(img_rel), 2000, 1200);
    // The user's REAL file occupying the w800 rung path.
    let user_bytes: &[u8] = b"user-owned bytes at the reserved rung path";
    fs::write(root.join("photo.w800.webp"), user_bytes).unwrap();

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();
    // Simulate the asset-copy leg having landed the user's file in
    // staging (it runs independently of the image worker).
    fs::write(staging.join("photo.w800.webp"), user_bytes).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&root.join(img_rel)).unwrap();
    let mut rung_collisions: HashMap<String, PathBuf> = HashMap::new();
    rung_collisions.insert("photo.w800.webp".to_string(), root.join("photo.w800.webp"));

    let (tx, rx) = test_utils::build_test_coordinator();
    let run_ctx = ImageRunContext {
        items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: Some((2000, 1200)),
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: Some(tx.clone()),
        rung_collisions,
    };

    let services = std::sync::Arc::new(BuildServices::headless());
    services.begin_ui_bound();
    let services_clone = services.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    // The user's bytes survive untouched at the collided rung path …
    assert_eq!(
        fs::read(staging.join("photo.w800.webp")).unwrap(),
        user_bytes,
        "the worker must NOT overwrite the user's file at a collided rung path"
    );
    // … while the base and the non-colliding rung are produced normally.
    assert!(staging.join("photo.webp").exists(), "base webp must exist");
    assert_eq!(
        staged_webp_dims(&staging.join("photo.w1600.webp")),
        (1600, 960),
        "the non-colliding w1600 rung must be produced normally"
    );

    // Manifest: base + w1600 registered (stale-cleanup protection for
    // rung files); the collided w800 path must NOT be registered as an
    // image variant — the user's file is registered by the asset-copy
    // leg under its own bucket.
    drop(tx);
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(sealed.image_outputs().contains("photo.webp"));
    assert!(
        sealed.image_outputs().contains("photo.w1600.webp"),
        "rung outputs must be manifest-registered or the next rebuild's \
             stale cleanup deletes them: {:?}",
        sealed.image_outputs()
    );
    assert!(
        !sealed.image_outputs().contains("photo.w800.webp"),
        "a collided rung path must not be claimed as an image variant"
    );
}

/// The background encode caches the placeholder metadata (dimensions +
/// dominant color + LQIP) under the scan's stat key, computed from the
/// pixels it already decoded for WebP. This is what lets a later (warm)
/// scan return the real color after the blocking scan deliberately skipped
/// the full decode. The color then morphs into the preview.
#[test]
fn convert_single_image_caches_placeholder_meta_under_stat_key() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();
    let stat_key = "stat:photo.jpg:1000:2000";

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        Some(stat_key),
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    // media/meta is now cached under the stat key with real placeholder data.
    let record = h
        .transforms
        .get(stat_key)
        .expect("media/meta should be cached under the stat key");
    let entry = record
        .transforms
        .get("media/meta")
        .expect("media/meta transform entry should be present");
    let blob = h
        .objects
        .get_path(&entry.oid)
        .expect("meta blob should exist");
    let cached: crate::build::cache::CachedMediaMeta =
        serde_json::from_slice(&std::fs::read(blob).unwrap()).unwrap();
    assert_eq!(
        cached.dimensions,
        Some((400, 300)),
        "dimensions should come from the decoded pixels"
    );
    assert!(
        cached.dominant_color.is_some(),
        "dominant color should be computed and cached"
    );
    assert!(
        cached.lqip_data_uri.is_some(),
        "LQIP should be computed and cached"
    );
}

#[test]
fn convert_single_image_cache_hit() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);

    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let first = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        first.error.is_none(),
        "expected no error: {:?}",
        first.error
    );
    let first_oid = first.webp_oid.clone().unwrap();

    // Remove the source file — a real cache hit should not need it.
    fs::remove_file(&src).unwrap();

    let second = convert_single_image(
        // source_file is still referenced for stat, but open() would fail.
        // We allow convert_single_image to proceed because it early-returns
        // on the cache hit before touching source_file for decode. The
        // metadata() call at the top tolerates missing files (returns 0).
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        second.error.is_none(),
        "expected no error on cache hit: {:?}",
        second.error
    );

    assert_eq!(second.webp_oid.as_deref(), Some(first_oid.as_str()));
}

#[test]
fn convert_single_image_ignores_legacy_sentinel_and_writes_webp() {
    // Old caches may still hold sentinel records (`oid=""`, `params=Null`)
    // from before the "always write WebP" change. They must NOT short-
    // circuit conversion any more — the encoder runs and a real WebP
    // lands on disk so the `<picture>` source 404 goes away.
    use crate::build::cache::{TransformEntry, TransformRecord};

    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();

    let mut record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: fs::metadata(&src).unwrap().len(),
        transforms: Default::default(),
    };
    record.transforms.insert(
        "image/webp".to_string(),
        TransformEntry {
            oid: String::new(),
            size: 0,
            params: serde_json::Value::Null,
        },
    );
    h.transforms.put(&record).unwrap();

    let cfg = ImageCompressionConfig::default();
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    assert!(
        outcome.webp_oid.is_some(),
        "legacy sentinel must not short-circuit"
    );
    assert!(outcome.webp_size > 0);
    assert!(
        h.staging.join("photo.webp").exists(),
        "real WebP must land on disk"
    );

    // Sentinel record overwritten with a real entry.
    let record = h.transforms.get(&source_oid).unwrap();
    let entry = record.transforms.get("image/webp").unwrap();
    assert!(!entry.oid.is_empty(), "sentinel oid must be replaced");
    assert_ne!(entry.params, serde_json::Value::Null);
}

#[test]
fn convert_single_image_writes_webp_even_when_larger_than_source() {
    // Pathological input where WebP would be larger than JPEG (4x4 noise
    // with RIFF/VP8 overhead exceeding the JPEG baseline). The previous
    // implementation wrote a sentinel and produced no .webp; the
    // synthesizer's unconditional `<source srcset>` then 404'd in the
    // browser. Now WebP is always written.
    let h = harness();
    let src = h._tmp.path().join("tiny.jpg");
    make_tiny_noise_jpeg(&src);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();

    let cfg = ImageCompressionConfig::default();
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "tiny.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "expected no error: {:?}",
        outcome.error
    );

    assert!(
        outcome.webp_oid.is_some(),
        "WebP must be written regardless of size comparison"
    );
    assert!(outcome.webp_size > 0);
    assert!(
        h.staging.join("tiny.webp").exists(),
        "WebP file must land on disk"
    );

    let record = h.transforms.get(&source_oid).unwrap();
    let entry = record.transforms.get("image/webp").unwrap();
    assert!(
        !entry.oid.is_empty(),
        "transform record must hold a real CAS oid (no sentinel)"
    );
}

#[test]
fn convert_single_image_handles_mismatched_extension() {
    // Real-world case: Obsidian-style paste produced a static PNG saved
    // with a `.gif` extension. Previously `image::open` selected the GIF
    // decoder by extension and failed with "malformed GIF header",
    // surfacing an "Image conversion failed: foo.gif" advisory toast
    // on every preview build. The fix is content-based format detection
    // via `ImageReader::with_guessed_format`. Anything decodable by the
    // `image` crate should now convert regardless of the extension.
    let h = harness();
    let src = h._tmp.path().join("mislabeled.gif");

    // Write a real PNG to a .gif path.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(400, 300, |x, y| {
        Rgb([(x as u8).wrapping_add(y as u8), 100, 50])
    });
    DynamicImage::ImageRgb8(buf)
        .save_with_format(&src, image::ImageFormat::Png)
        .unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "mislabeled.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );

    assert!(
        outcome.error.is_none(),
        "mismatched extension must not block conversion: {:?}",
        outcome.error
    );
    assert!(
        outcome.webp_oid.is_some(),
        "WebP should be produced from PNG bytes"
    );
    assert!(
        h.staging.join("mislabeled.webp").exists(),
        "staging WebP missing"
    );
}

#[test]
fn convert_single_image_still_rejects_truly_undecodable_bytes() {
    // Belt-and-suspenders: should_skip catches HTML-as-image before
    // convert_single_image is ever called, but if someone calls
    // convert_single_image directly with garbage bytes it must still
    // surface as an error rather than silently succeeding.
    let h = harness();
    let src = h._tmp.path().join("notanimage.png");
    fs::write(&src, b"<!DOCTYPE html><html>not an image</html>").unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let outcome = convert_single_image(
        &src,
        &source_oid,
        "notanimage.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );

    assert!(
        outcome.error.is_some(),
        "undecodable bytes must surface as an error, not silently succeed"
    );
    assert!(outcome.webp_oid.is_none());
    assert!(!h.staging.join("notanimage.webp").exists());
}

// ----- should_skip combinations -----

#[test]
fn should_skip_svg() {
    let h = harness();
    let src = h._tmp.path().join("logo.svg");
    fs::write(&src, b"<svg></svg>").unwrap();
    let cfg = ImageCompressionConfig::default();
    let reason = should_skip(&src, "svg", 100, &cfg, &h.transforms, "fake_oid", false);
    assert_eq!(reason, Some(SkipReason::Svg));
}

#[test]
fn should_skip_heic() {
    let h = harness();
    let src = h._tmp.path().join("pic.heic");
    fs::write(&src, b"\x00\x00\x00\x20ftypheic").unwrap();
    let cfg = ImageCompressionConfig::default();
    let reason = should_skip(&src, "heic", 100, &cfg, &h.transforms, "fake_oid", false);
    assert_eq!(reason, Some(SkipReason::Heic));
}

#[test]
fn should_not_skip_small_jpg_png_for_already_small() {
    // png/jpg/jpeg always get encoded — the synthesizer's
    // `<picture><source srcset="…webp">` promise requires the file to
    // exist on disk regardless of source size.
    let h = harness();
    for ext in ["jpg", "png"] {
        let src = h._tmp.path().join(format!("small.{}", ext));
        make_big_jpeg(&src, 32, 32); // tiny dimensions, small file
        let cfg = ImageCompressionConfig {
            min_size_kb: 10,
            max_edge: 2400,
            ..Default::default()
        };
        let size = fs::metadata(&src).unwrap().len();
        assert!(size < 10 * 1024);
        assert_eq!(
            should_skip(&src, ext, size, &cfg, &h.transforms, "fake_oid", false),
            None,
            "ext={} must not be AlreadySmall-skipped (synthesizer emits <source srcset>)",
            ext,
        );
    }
}

#[test]
fn should_skip_already_small_for_non_picture_formats() {
    // Non-(png|jpg|jpeg) formats are emitted as bare <img> by the
    // synthesizer (no <picture><source>), so the AlreadySmall CPU
    // optimization still applies — no .webp file is required.
    let h = harness();
    let src = h._tmp.path().join("small.gif");
    // Synthesize a tiny GIF header (non-animated)
    fs::write(
        &src,
        b"GIF89a\x10\x00\x10\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
              \x21\xf9\x04\x00\x00\x00\x00\x00,\x00\x00\x00\x00\x10\x00\x10\x00\
              \x00\x02\x02D\x01\x00;",
    )
    .unwrap();
    let cfg = ImageCompressionConfig {
        min_size_kb: 10,
        max_edge: 2400,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    // Skip path for non-picture formats: AlreadySmall fires only if
    // image_dimensions() succeeds. The minimal GIF above may not parse
    // cleanly via `image` crate — if so, this test just asserts no panic
    // and that the result is either AlreadySmall or None.
    let reason = should_skip(&src, "gif", size, &cfg, &h.transforms, "fake_oid", false);
    assert!(
        matches!(reason, Some(SkipReason::AlreadySmall) | None),
        "got unexpected reason: {:?}",
        reason
    );
}

#[test]
fn should_skip_already_small_for_rungless_webp() {
    // A small non-animated webp (deployed base width ≤800 → empty ladder)
    // emits no rung URLs, so AlreadySmall stays a valid CPU optimization —
    // unchanged by Task 11. 500×400: deployed_width == 500, no rung < 500.
    let h = harness();
    let src = h._tmp.path().join("small.webp");
    make_webp(&src, 500, 400);
    let cfg = ImageCompressionConfig {
        min_size_kb: 500,
        max_edge: 2400,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    assert!(
        size < 500 * 1024,
        "gradient webp must fit under the size gate"
    );
    assert!(
        moss_core::asset_paths::ladder_rungs(500, 400, false).is_empty(),
        "500×400 must have no rungs for this test's premise",
    );
    assert_eq!(
        should_skip(&src, "webp", size, &cfg, &h.transforms, "fake_oid", false),
        Some(SkipReason::AlreadySmall),
        "rung-free webp keeps the AlreadySmall skip",
    );
}

#[test]
fn should_not_skip_webp_that_carries_rungs() {
    // THE Task 11 behavior change: a non-animated webp whose dimensions are
    // under max_edge (so the pre-Task-11 size+dimension rule WOULD have
    // marked it AlreadySmall) but whose deployed base width exceeds 800px
    // now carries ladder rungs. Phase B (Task 12) will emit those rung URLs
    // in `<img srcset>`, so the encode MUST run — should_skip returns None.
    // 1200×900: deployed_width == 1200, rung 800 < 1200 → ladder = [800].
    let h = harness();
    let src = h._tmp.path().join("wide.webp");
    make_webp(&src, 1200, 900);
    let cfg = ImageCompressionConfig {
        min_size_kb: 5000,
        max_edge: 2400,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    // Premise: file is small enough to trip the size gate, dims are under
    // max_edge (pre-Task-11 → AlreadySmall), yet a rung exists.
    assert!(
        size < 5000 * 1024,
        "gradient webp must fit under the size gate"
    );
    assert!(
        1200u32.max(900) < cfg.max_edge,
        "dims under max_edge (pre-fix AlreadySmall)"
    );
    assert!(
        !moss_core::asset_paths::ladder_rungs(1200, 900, false).is_empty(),
        "1200×900 must carry a rung for this test's premise",
    );
    assert_eq!(
        should_skip(&src, "webp", size, &cfg, &h.transforms, "fake_oid", false),
        None,
        "non-animated webp with ladder rungs must be encoded, not AlreadySmall-skipped",
    );
}

/// Wrap a solid `w`×`h` webp (built by the image crate) in EXTENDED VP8X
/// form carrying an `EXIF` chunk with `orientation`. `image::image_dimensions`
/// reads the STORED (unswapped) `w`×`h` from the VP8X canvas;
/// `read_exif_orientation` reads the tag — the same readers scan + encode
/// use. Test-local by design (mirrors the scan-module EXIF fixtures).
fn exif_oriented_webp(path: &Path, w: u32, h: u32, orientation: u16) {
    use std::io::Cursor;
    // little-endian TIFF with one IFD0 Orientation (0x0112, SHORT) entry.
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&(orientation as u32).to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());

    let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(w, h, Rgb([122, 139, 156])));
    let mut simple = Vec::new();
    img.write_to(&mut Cursor::new(&mut simple), image::ImageFormat::WebP)
        .unwrap();
    let mut fourcc = [0u8; 4];
    fourcc.copy_from_slice(&simple[12..16]);
    let clen = u32::from_le_bytes(simple[16..20].try_into().unwrap()) as usize;
    let cdata = simple[20..20 + clen].to_vec();

    let riff_chunk = |fourcc: &[u8; 4], data: &[u8]| -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(fourcc);
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(data);
        if data.len() & 1 == 1 {
            c.push(0);
        }
        c
    };
    let mut vp8x = vec![0x08u8, 0, 0, 0]; // EXIF flag + reserved
    vp8x.extend_from_slice(&(w - 1).to_le_bytes()[0..3]);
    vp8x.extend_from_slice(&(h - 1).to_le_bytes()[0..3]);
    let mut body = Vec::new();
    body.extend_from_slice(b"WEBP");
    body.extend_from_slice(&riff_chunk(b"VP8X", &vp8x));
    body.extend_from_slice(&riff_chunk(&fourcc, &cdata));
    body.extend_from_slice(&riff_chunk(b"EXIF", &tiff));
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    fs::write(path, out).unwrap();
}

#[test]
fn should_not_skip_exif_oriented_webp_whose_display_ladder_is_nonempty() {
    // Regression pin for design follow-up #1's should_skip leg. A small webp
    // stored 500×900 (portrait pixels) with EXIF orientation 6 DISPLAYS as
    // 900×500 (landscape). Scan/emission/registration now key off the
    // oriented display dims (900×500 → ladder [800]) and promise a w800 rung,
    // so the AlreadySmall carve-out must NOT skip it — skipping would strand
    // that promised rung (publish-404). This fails on the pre-fix should_skip
    // (which read the STORED 500×900 → empty ladder → AlreadySmall).
    let h = harness();
    let src = h._tmp.path().join("rotated_small.webp");
    exif_oriented_webp(&src, 500, 900, 6);
    // Fixture sanity: stored dims + tag exactly as scan/encode read them.
    assert_eq!(image::image_dimensions(&src).unwrap(), (500, 900));
    assert_eq!(read_exif_orientation(&src), 6);
    // Stored ladder empty; oriented ladder carries a rung — the crux.
    assert!(moss_core::asset_paths::ladder_rungs(500, 900, false).is_empty());
    assert!(!moss_core::asset_paths::ladder_rungs(900, 500, false).is_empty());
    let cfg = ImageCompressionConfig {
        min_size_kb: 500,
        max_edge: 2400,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    assert!(
        size < 500 * 1024,
        "gradient webp must fit under the size gate"
    );
    assert_eq!(
            should_skip(&src, "webp", size, &cfg, &h.transforms, "fake_oid", false),
            None,
            "EXIF-oriented webp whose DISPLAY ladder carries a rung must be encoded, not AlreadySmall-skipped",
        );
}

#[test]
fn should_skip_animated_webp_before_already_small() {
    // Animated webp short-circuits at the AnimatedWebp branch, BEFORE the
    // AlreadySmall carve-out — which is what makes passing is_animated=false
    // to ladder_rungs in that carve-out provably correct (no animated webp
    // ever reaches it). Hand-crafted VP8X + ANIM chunk (see
    // is_animated_webp_on_anim_chunk).
    let h = harness();
    let src = h._tmp.path().join("anim.webp");
    let mut bytes: Vec<u8> = b"RIFF".to_vec();
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.extend_from_slice(b"WEBP");
    bytes.extend_from_slice(b"VP8X");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(b"ANIM");
    bytes.extend_from_slice(&[0; 10]);
    fs::write(&src, &bytes).unwrap();
    let cfg = ImageCompressionConfig {
        min_size_kb: 5000,
        max_edge: 2400,
        ..Default::default()
    };
    assert_eq!(
        should_skip(&src, "webp", 100, &cfg, &h.transforms, "fake_oid", false),
        Some(SkipReason::AnimatedWebp),
        "animated webp must skip as AnimatedWebp, never reaching AlreadySmall",
    );
}

#[test]
fn should_skip_ignores_legacy_sentinel() {
    // Old caches may contain sentinels (`oid=""`, `params=Null`) written
    // by the prior "WebP > original, skip" optimization. With that
    // optimization removed, `should_skip` must NOT honor those sentinels —
    // they should fall through to a fresh encode so the .webp file lands
    // on disk and the `<picture>` source 404 goes away.
    use crate::build::cache::{TransformEntry, TransformRecord};
    let h = harness();
    let src = h._tmp.path().join("pic.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();

    let mut record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: fs::metadata(&src).unwrap().len(),
        transforms: Default::default(),
    };
    record.transforms.insert(
        "image/webp".to_string(),
        TransformEntry {
            oid: String::new(),
            size: 0,
            params: serde_json::Value::Null,
        },
    );
    h.transforms.put(&record).unwrap();

    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    assert_eq!(
        should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false),
        None
    );
}

#[test]
fn should_skip_returns_none_for_normal_jpeg() {
    let h = harness();
    let src = h._tmp.path().join("ok.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    // Use min_size_kb=0 so the size/dimension "AlreadySmall" rule cannot fire
    // and we verify only the "no skip reason" branch.
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    assert_eq!(
        should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false),
        None
    );
}

// ----- NotAnImage (magic-byte mismatch) -----

#[test]
fn should_skip_html_saved_as_png() {
    // The Yi-website case: server returned a 404 HTML page that was saved
    // with a .png extension.
    let h = harness();
    let src = h._tmp.path().join("Test.png");
    fs::write(
        &src,
        b"<!DOCTYPE HTML PUBLIC \"-//IETF//DTD HTML 2.0//EN\">\n\
              <html><head>\n<title>404 Not Found</title>\n</head></html>",
    )
    .unwrap();
    let cfg = ImageCompressionConfig::default();
    let reason = should_skip(&src, "png", 196, &cfg, &h.transforms, "fake_oid", false);
    assert_eq!(reason, Some(SkipReason::NotAnImage));
}

#[test]
fn should_skip_html_saved_as_jpg() {
    let h = harness();
    let src = h._tmp.path().join("broken.jpg");
    fs::write(&src, b"<html>not an image</html>").unwrap();
    let cfg = ImageCompressionConfig::default();
    let reason = should_skip(&src, "jpg", 24, &cfg, &h.transforms, "fake_oid", false);
    assert_eq!(reason, Some(SkipReason::NotAnImage));
}

#[test]
fn should_skip_not_an_image_real_png_passes() {
    // A real PNG must NOT be flagged as NotAnImage.
    let h = harness();
    let src = h._tmp.path().join("real.png");
    // Minimal valid 1×1 PNG
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR length + type
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth, color type, crc
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT length + type
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, // IDAT data
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, // CRC
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND length + type
        0x44, 0xAE, 0x42, 0x60, 0x82, // IEND data + CRC
    ];
    fs::write(&src, png_bytes).unwrap();
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    let reason = should_skip(&src, "png", size, &cfg, &h.transforms, "fake_oid", false);
    assert_ne!(
        reason,
        Some(SkipReason::NotAnImage),
        "valid PNG must not be flagged"
    );
}

#[test]
fn should_skip_not_an_image_real_jpeg_passes() {
    // A real JPEG (made by make_big_jpeg) must NOT be flagged.
    let h = harness();
    let src = h._tmp.path().join("real.jpg");
    make_big_jpeg(&src, 200, 200);
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };
    let size = fs::metadata(&src).unwrap().len();
    let reason = should_skip(&src, "jpg", size, &cfg, &h.transforms, "fake_oid", false);
    assert_ne!(
        reason,
        Some(SkipReason::NotAnImage),
        "valid JPEG must not be flagged"
    );
}

// ----- format-probe cache (moss#920) -----

/// Build a handcrafted CMYK JPEG: SOI, then SOF0 with a 4-component frame.
/// Mirrors `is_cmyk_jpeg_handcrafted_sof_components4`'s fixture — a real
/// decodable JPEG is not required since `is_cmyk_jpeg` only parses the SOF
/// marker, and CMYK is checked (and short-circuits) before the magic-byte
/// check in `probe_cmyk_magic_already_small`.
pub(crate) fn make_cmyk_jpeg(path: &Path) {
    let mut bytes: Vec<u8> = vec![0xFF, 0xD8];
    bytes.push(0xFF);
    bytes.push(0xC0); // SOF0
    bytes.extend_from_slice(&[0x00, 0x11]); // length = 17
    bytes.push(0x08); // precision
    bytes.extend_from_slice(&[0x00, 0x10]); // height
    bytes.extend_from_slice(&[0x00, 0x10]); // width
    bytes.push(0x04); // component count = 4 (CMYK)
    bytes.extend_from_slice(&[0; 12]); // padding to satisfy length
    fs::write(path, &bytes).unwrap();
}

#[test]
fn format_probe_cache_hit_survives_source_deletion() {
    // A cache HIT must never re-touch `source_path` — that's the entire
    // point of the cache (moss#920). Prove it: after the first call caches
    // a verdict, delete the source file entirely. A second call with the
    // same (source_oid, params) must still return the identical verdict
    // instead of erroring or silently recomputing (which would see a
    // missing file and behave unpredictably).
    let h = harness();
    let src = h._tmp.path().join("cmyk.jpg");
    make_cmyk_jpeg(&src);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();
    let size = fs::metadata(&src).unwrap().len();

    let first = should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false);
    assert_eq!(first, Some(SkipReason::Cmyk), "premise: fixture is CMYK");

    fs::remove_file(&src).unwrap();

    let second = should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false);
    assert_eq!(
        second,
        Some(SkipReason::Cmyk),
        "cache hit must return the cached verdict without re-reading the (now-deleted) source"
    );
}

// ----- moss#982: a source still in the cloud yields no verdict, and no
// CACHED verdict -----

#[test]
fn a_source_in_the_cloud_gets_its_own_verdict_not_not_an_image() {
    // The bug this pins: `has_image_magic` cannot distinguish an unreadable
    // file from a file that is not an image, so an evicted source used to be
    // classified `NotAnImage` — dropping it from `image_items`, which skips
    // `set_pending`, which strands the `<source>` the synthesizer emits anyway.
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();
    let size = fs::metadata(&src).unwrap().len();

    assert_eq!(
        should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, true),
        Some(SkipReason::SourceInTheCloud),
        "an evicted source must be named as such, never as NotAnImage"
    );
}

#[test]
fn a_verdict_about_a_source_in_the_cloud_is_never_cached() {
    // The severe half of moss#982. The format-probe cache is keyed by the
    // source's CONTENT oid, and the content of an evicted file never changes —
    // so a verdict computed while the bytes were absent would outlive the
    // eviction FOREVER, permanently breaking an image that had merely been
    // slow to download. `scan.rs` already refuses to cache non-answers for the
    // same reason; this asserts the format-probe cache now does too.
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();
    let size = fs::metadata(&src).unwrap().len();

    // Probe while the bytes are "in the cloud"...
    let cloud = should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, true);
    assert_eq!(cloud, Some(SkipReason::SourceInTheCloud));

    // ...then again once they have arrived. Same file, same oid, same config:
    // if the first call had written the cache, this would return the stale
    // verdict rather than looking at the (perfectly good) image.
    let arrived = should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false);
    assert_eq!(
        arrived, None,
        "the download must heal the image; a cached cloud-era verdict would make it permanent"
    );
}

#[test]
fn a_source_in_the_cloud_short_circuits_before_every_other_probe() {
    // Ordering matters: the guard has to precede the extension branches too,
    // because the cache read/write straddles them. An SVG is the cheapest
    // proof — it normally returns `Svg` from the very first branch.
    let h = harness();
    let src = h._tmp.path().join("logo.svg");
    fs::write(&src, b"<svg></svg>").unwrap();
    let cfg = ImageCompressionConfig::default();

    assert_eq!(
        should_skip(&src, "svg", 100, &cfg, &h.transforms, "fake_oid", true),
        Some(SkipReason::SourceInTheCloud),
    );
}

#[test]
fn format_probe_cache_miss_on_config_change_recomputes() {
    // A changed `min_size_kb`/`max_edge` must be a cache miss, not a stale
    // hit — `params` is part of the cache key precisely so a config change
    // can never serve a verdict computed under different settings.
    let h = harness();
    let src = h._tmp.path().join("small.webp");
    make_webp(&src, 500, 400);
    assert!(
        moss_core::asset_paths::ladder_rungs(500, 400, false).is_empty(),
        "premise: 500×400 webp carries no rungs"
    );
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let size = fs::metadata(&src).unwrap().len();

    let cfg_small_size_gate = ImageCompressionConfig {
        min_size_kb: 500,
        max_edge: 2400,
        ..Default::default()
    };
    assert_eq!(
        should_skip(&src, "webp", size, &cfg_small_size_gate, &h.transforms, &source_oid, false),
        Some(SkipReason::AlreadySmall),
        "premise: with a generous size gate this rung-free webp is AlreadySmall"
    );

    // Same source_oid, DIFFERENT params (min_size_kb: 0 — size gate can never
    // trip). If the cache incorrectly ignored params, this would wrongly
    // return the cached `AlreadySmall` from the call above.
    let cfg_no_size_gate = ImageCompressionConfig {
        min_size_kb: 0,
        max_edge: 2400,
        ..Default::default()
    };
    assert_eq!(
        should_skip(&src, "webp", size, &cfg_no_size_gate, &h.transforms, &source_oid, false),
        None,
        "config change (min_size_kb) must invalidate the cached AlreadySmall verdict"
    );
}

#[test]
fn format_probe_cache_key_includes_extension() {
    // Regression pin for a real review finding: without `ext` in the cache
    // key, two calls sharing a `source_oid` but differing only in extension
    // could leak one's verdict into the other's. `raster_with_picture`
    // forbids AlreadySmall for png/jpg/jpeg specifically (ADR-013 — the
    // synthesizer emits an unconditional `<picture><source>` for those), so a
    // webp's AlreadySmall verdict leaking into a png lookup would strand that
    // `<source>` — the exact 2026-05-19 failure class. Deliberately reuses
    // ONE literal `source_oid` across two different real files/extensions to
    // isolate the cache-key behavior from content-hash behavior.
    let h = harness();
    let shared_oid = "shared-oid-for-ext-collision-test";
    let cfg = ImageCompressionConfig {
        min_size_kb: 500,
        max_edge: 2400,
        ..Default::default()
    };

    let webp_src = h._tmp.path().join("small.webp");
    make_webp(&webp_src, 500, 400);
    assert!(moss_core::asset_paths::ladder_rungs(500, 400, false).is_empty());
    let webp_size = fs::metadata(&webp_src).unwrap().len();
    assert_eq!(
        should_skip(&webp_src, "webp", webp_size, &cfg, &h.transforms, shared_oid, false),
        Some(SkipReason::AlreadySmall),
        "premise: rung-free small webp is AlreadySmall and populates the cache under shared_oid"
    );

    // Real, valid, tiny PNG (from should_skip_not_an_image_real_png_passes) —
    // small enough to trip the size gate, but png is in raster_with_picture
    // so it must NEVER return AlreadySmall, cache or no cache.
    let png_src = h._tmp.path().join("tiny.png");
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    fs::write(&png_src, png_bytes).unwrap();
    let png_size = fs::metadata(&png_src).unwrap().len();
    assert_ne!(
        should_skip(&png_src, "png", png_size, &cfg, &h.transforms, shared_oid, false),
        Some(SkipReason::AlreadySmall),
        "a png lookup under the SAME source_oid as a cached webp AlreadySmall verdict must not \
         inherit that verdict — ext must be part of the cache key"
    );
}

#[test]
fn format_probe_cache_recomputes_on_corrupt_blob() {
    // The cached blob can outlive its usefulness (truncated write, disk
    // corruption, a future format change to the JSON shape). `should_skip`
    // must fall through and recompute the real verdict rather than error,
    // panic, or silently return a wrong/garbage answer.
    //
    // NOT asserted here: that this recompute repairs the on-disk blob for
    // next time. `ObjectStore::store_bytes` (cache.rs) skips writing when a
    // blob already exists at the destination content-hash path — it checks
    // bare existence, not whether the existing bytes are well-formed — so an
    // in-place-corrupted blob (same path, same apparent identity, bad
    // content) is NOT repaired by a later `store_bytes` call for the same
    // logical content. That's a pre-existing `ObjectStore` characteristic
    // shared by every cache built on it (`image/webp`, `image/sized-raster`),
    // not something introduced here — out of scope for this fix.
    let h = harness();
    let src = h._tmp.path().join("cmyk.jpg");
    make_cmyk_jpeg(&src);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();
    let size = fs::metadata(&src).unwrap().len();

    assert_eq!(
        should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false),
        Some(SkipReason::Cmyk),
        "premise: populates the format-probe cache entry"
    );

    let record = h.transforms.get(&source_oid).expect("cache entry must exist");
    let entry = record
        .transforms
        .get(FORMAT_PROBE_TRANSFORM)
        .expect("format-probe entry must exist");
    let blob_path = h.objects.get_path(&entry.oid).expect("blob must exist");
    fs::write(&blob_path, b"not valid json").unwrap();

    let recomputed = should_skip(&src, "jpg", size, &cfg, &h.transforms, &source_oid, false);
    assert_eq!(
        recomputed,
        Some(SkipReason::Cmyk),
        "a corrupt cached blob must not error or panic — recompute the real verdict"
    );
}

#[test]
fn format_probe_cache_ignores_empty_source_oid() {
    // An unresolved `source_oid` (`""`, the stat-match-miss fallback used by
    // both `collect_images_for_conversion` and `webp_converter_owns_base`)
    // must never be treated as a cache key: two different real files probed
    // with `""` in the same build must not leak one file's verdict into the
    // other's result.
    let h = harness();
    let cfg = ImageCompressionConfig::default();

    let cmyk_src = h._tmp.path().join("cmyk.jpg");
    make_cmyk_jpeg(&cmyk_src);
    let cmyk_size = fs::metadata(&cmyk_src).unwrap().len();

    let plain_src = h._tmp.path().join("real.jpg");
    make_big_jpeg(&plain_src, 200, 200);
    let plain_size = fs::metadata(&plain_src).unwrap().len();

    assert_eq!(
        should_skip(&cmyk_src, "jpg", cmyk_size, &cfg, &h.transforms, "", false),
        Some(SkipReason::Cmyk),
    );
    assert_ne!(
        should_skip(&plain_src, "jpg", plain_size, &cfg, &h.transforms, "", false),
        Some(SkipReason::Cmyk),
        "a plain RGB jpeg probed with an empty source_oid right after a CMYK \
         jpeg must not inherit the CMYK verdict from a shared \"\" cache key"
    );
    assert!(
        h.transforms.get("").is_none(),
        "an empty source_oid must never be written to the transform cache"
    );
}

// ----- Fingerprint cache helper -----

/// The image fingerprint cell is one process-global, so a test that expects
/// its own value back must not run beside another test that writes it.
///
/// That includes every test that runs `dispatch_image_conversions` with a
/// spawner (the GUI branch), whether or not it reads a value back: the branch
/// ends by pruning the cell to its own image set
/// (`retain_image_item_fingerprints`), which drops the entries a concurrent
/// test primed a moment ago. Two such tests took no lock, and
/// `a_new_image_only_dispatches_the_new_one…` then found one.jpg and two.jpg
/// re-dispatched in 3 of 12 runs of this module.
fn image_fingerprint_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// A fingerprint that differs from `tag`'s in one field, for the cache tests below.
fn test_fingerprint(tag: u64) -> ImageFingerprint {
    ImageFingerprint {
        stat: crate::build::cache::FileStat { size: tag, mtime: 1, mtime_nanos: Some(1), ctime: Some(1), inode: Some(1) },
        params: serde_json::json!({}),
    }
}

#[test]
fn fingerprint_cache_matches_only_what_was_recorded() {
    let _guard = image_fingerprint_test_lock().lock();
    // Use a unique path per run to avoid state from other tests.
    let path = format!("img-{}.jpg", uuid::Uuid::new_v4());
    let (a, b) = (test_fingerprint(1), test_fingerprint(2));
    // Nothing recorded for this path → not unchanged.
    assert!(!image_item_fingerprint_matches(&path, &a));
    record_image_item_fingerprint(&path, a.clone());
    assert!(image_item_fingerprint_matches(&path, &a));
    // A different value is not unchanged, and asking about it records nothing: the
    // dispatch that asks has not yet had its worker deliver the change.
    assert!(!image_item_fingerprint_matches(&path, &b));
    assert!(image_item_fingerprint_matches(&path, &a), "a check must not overwrite what a worker recorded");
}

/// The core per-item property: one path's fingerprint is independent of
/// another's. Before per-item fingerprinting a single whole-set fingerprint
/// covered every image, so checking path B always invalidated whatever path
/// A had just recorded. Only inserts (never removes) other paths' entries,
/// so — unlike a `retain` call — it is safe to run beside any other test
/// touching this process-global cache without taking `image_fingerprint_
/// test_lock()`.
#[test]
fn fingerprint_cache_is_independent_per_path() {
    let path_a = format!("a-{}.jpg", uuid::Uuid::new_v4());
    let path_b = format!("b-{}.jpg", uuid::Uuid::new_v4());
    let (a, b) = (test_fingerprint(1), test_fingerprint(2));
    record_image_item_fingerprint(&path_a, a.clone());
    record_image_item_fingerprint(&path_b, b.clone());
    // Both still match their own last-recorded fingerprint.
    assert!(image_item_fingerprint_matches(&path_a, &a));
    assert!(image_item_fingerprint_matches(&path_b, &b));
    // And a path nothing was recorded for is not vouched for by another's: the
    // fingerprint no longer carries the path, so the cell's key is all that says whose it is.
    assert!(!image_item_fingerprint_matches(&format!("c-{}.jpg", uuid::Uuid::new_v4()), &a));
}

// ======================================================================
// Task 3: Scan/render/build wiring
// ======================================================================

// ----- collect_images_for_conversion -----

/// Build a ProjectStructure backed by a temp dir.
///
/// Returns (structure, temp dir keeper). The dir must outlive the
/// structure since the images live in it.
// `pub(crate)` so the extracted `rungs` module's tests can reuse this
// shared fixture builder (Task 10.5 — the rung_collision_map tests moved
// to `rungs.rs`; this fixture stays here where its many other callers live).
pub(crate) fn build_project_with_images(
    entries: &[(&str, &str, Option<u64>, bool)],
) -> (crate::types::content::ProjectStructure, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let mut image_files = Vec::new();
    for (rel_path, ext, size_override, is_real_jpeg) in entries {
        let abs_path = root.join(rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        if *is_real_jpeg {
            make_big_jpeg(&abs_path, 400, 300);
        } else {
            // Dummy content for .svg / .heic / etc. — large enough to
            // avoid the AlreadySmall skip branch.
            fs::write(&abs_path, vec![0u8; 250 * 1024]).unwrap();
        }
        let size = size_override.unwrap_or_else(|| fs::metadata(&abs_path).unwrap().len());
        image_files.push(MediaMetadata {
            path: rel_path.to_string(),
            file_type: ext.to_string(),
            size,
            ..Default::default()
        });
    }

    let structure = crate::types::content::ProjectStructure {
        root_path: root.to_string_lossy().to_string(),
        markdown_files: vec![],
        html_files: vec![],
        image_files,
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

    (structure, tmp)
}

// The rung_collision_map tests moved to the sibling `rungs` module with
// `rung_collision_map` itself (Task 10.5). See `build/media/rungs.rs`.

#[test]
fn test_collect_images_filters_skipped() {
    let (mut structure, _tmp) = build_project_with_images(&[
        ("photo.jpg", "jpg", None, true),
        ("logo.svg", "svg", None, false),
        ("pic.heic", "heic", None, false),
    ]);

    // Add a .png whose content is an HTML 404 page (the Yi-website case).
    // build_project_with_images writes dummy zero bytes; overwrite with HTML.
    let html_png = _tmp.path().join("Test.png");
    fs::write(
        &html_png,
        b"<!DOCTYPE HTML PUBLIC \"-//IETF//DTD HTML 2.0//EN\">\n\
              <html><head><title>404 Not Found</title></head></html>",
    )
    .unwrap();
    let html_size = fs::metadata(&html_png).unwrap().len();
    structure.image_files.push(MediaMetadata {
        path: "Test.png".to_string(),
        file_type: "png".to_string(),
        size: html_size,
        ..Default::default()
    });

    let transforms_dir = _tmp.path().join("cache_transforms");
    let objects_dir = _tmp.path().join("cache_objects");
    let transforms = crate::build::cache::TransformCache::new(
        transforms_dir,
        crate::build::cache::ObjectStore::new(objects_dir),
    );
    let mut hash_index = crate::build::cache::HashIndex::new();

    // min_size_kb=0 so AlreadySmall doesn't interfere with the jpg fixture.
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };

    let items = collect_images_for_conversion(&structure, &transforms, &mut hash_index, &cfg);

    // svg and heic get a bare <img> and drop here; the html-as-png is a
    // promised `<picture>` source, so it is collected WITH its verdict for the
    // registration loop to settle (`ImageConversionItem::skip`).
    let by_path = |p: &str| {
        items
            .iter()
            .find(|i| i.source_path == PathBuf::from(p))
            .unwrap_or_else(|| panic!("{p} must be collected"))
    };
    assert_eq!(items.len(), 2, "svg and heic must be filtered: {items:?}");
    assert_eq!(by_path("Test.png").skip, Some(SkipReason::NotAnImage));
    assert_eq!(by_path("photo.jpg").skip, None);
    assert_eq!(items[0].source_path, PathBuf::from("photo.jpg"));
    assert_eq!(items[0].ext, "jpg");
    // Hashing is DEFERRED off the blocking path: with an empty hash index
    // (cold), collect stat-matches nothing, so source_oid is left empty for
    // the background worker to resolve. This is what removes the tens of
    // seconds of blocking SHA-256 on a large image folder.
    assert!(
        items[0].source_oid.is_empty(),
        "source_oid must be deferred (empty) on a cold blocking scan"
    );
}

/// On a warm build the hash index (persisted by the previous build's
/// background worker) has a stat-match, so collect reuses that hash WITHOUT
/// re-hashing — the blocking path stays cheap and the background can hit the
/// WebP cache immediately.
#[test]
fn test_collect_images_reuses_stat_matched_hash_without_rehashing() {
    let (structure, _tmp) = build_project_with_images(&[("photo.jpg", "jpg", None, true)]);

    let transforms = crate::build::cache::TransformCache::new(
        _tmp.path().join("cache_transforms"),
        crate::build::cache::ObjectStore::new(_tmp.path().join("cache_objects")),
    );
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };

    // Pre-populate the index with a stat-match for photo.jpg (as the
    // background worker would have persisted on a prior build).
    let file_path = _tmp.path().join("photo.jpg");
    let mut hash_index = crate::build::cache::HashIndex::new();
    hash_index.update(
        "photo.jpg".to_string(),
        &crate::build::cache::FileStat::of(&fs::metadata(&file_path).unwrap()),
        "deadbeef".to_string(),
    );

    let items = collect_images_for_conversion(&structure, &transforms, &mut hash_index, &cfg);

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].source_oid, "deadbeef",
        "collect must reuse the stat-matched hash from the index, never re-hash"
    );
}

/// What the blocking collect feeds the index: the file's whole stat record. An entry
/// the worker recorded for another size, mtime, sub-second mtime, ctime or inode is not
/// this file's, and the oid in it — planted here so a wrongly trusted entry is visible —
/// must not become the item's. The exact record is the control (the test above).
#[test]
fn collect_trusts_an_entry_only_for_the_files_whole_stat_record() {
    let (structure, tmp) = build_project_with_images(&[("photo.jpg", "jpg", None, true)]);
    let transforms = crate::build::cache::TransformCache::new(
        tmp.path().join("cache_transforms"),
        crate::build::cache::ObjectStore::new(tmp.path().join("cache_objects")),
    );
    let cfg = ImageCompressionConfig { min_size_kb: 0, ..Default::default() };
    let real = crate::build::cache::FileStat::of(&fs::metadata(tmp.path().join("photo.jpg")).unwrap());

    for (field, changed) in real.each_field_changed() {
        let mut hash_index = crate::build::cache::HashIndex::new();
        hash_index.update("photo.jpg".to_string(), &changed, "planted".to_string());

        let items = collect_images_for_conversion(&structure, &transforms, &mut hash_index, &cfg);

        assert_eq!(items.len(), 1);
        assert!(items[0].source_oid.is_empty(), "an entry recorded for another {field} was trusted: {:?}", items[0].source_oid);
    }
}

/// Replace-via-rename with size and mtime kept: the worker's recorded oid is the old
/// file's, and only the inode says so. Trusting it would encode the new image under the
/// old one's cache key and ship the previous picture's variant.
#[cfg(unix)]
#[test]
fn collect_does_not_take_the_oid_of_an_image_replaced_by_rename() {
    let (structure, tmp) = build_project_with_images(&[("photo.jpg", "jpg", None, true)]);
    let transforms = crate::build::cache::TransformCache::new(
        tmp.path().join("cache_transforms"),
        crate::build::cache::ObjectStore::new(tmp.path().join("cache_objects")),
    );
    let cfg = ImageCompressionConfig { min_size_kb: 0, ..Default::default() };
    let file = tmp.path().join("photo.jpg");
    let mut hash_index = crate::build::cache::HashIndex::new();
    hash_index.resolve(&file, "photo.jpg").unwrap();

    let mut bytes = fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    crate::build::cache::FileStat::replace_by_rename_keeping_mtime(&file, &bytes);

    let items = collect_images_for_conversion(&structure, &transforms, &mut hash_index, &cfg);

    assert_eq!(items.len(), 1);
    assert!(items[0].source_oid.is_empty(), "took the recorded oid of the file that was replaced: {:?}", items[0].source_oid);
}

#[test]
fn a_cmyk_jpeg_is_collected_with_its_verdict_so_its_source_can_be_settled() {
    // The synthesizer promises `<picture><source srcset="plate.webp">` for
    // every jpg from the extension alone. A CMYK source is never encoded, so
    // the collector used to drop it and the promised URL 404ed — which
    // `<picture>` does not recover from (ADR-013). It must reach the
    // registration loop carrying the verdict.
    let (mut structure, _tmp) = build_project_with_images(&[("plate.jpg", "jpg", None, true)]);
    let plate = _tmp.path().join("plate.jpg");
    make_cmyk_jpeg(&plate);
    structure.image_files[0].size = fs::metadata(&plate).unwrap().len();

    let transforms = crate::build::cache::TransformCache::new(
        _tmp.path().join("cache_transforms"),
        crate::build::cache::ObjectStore::new(_tmp.path().join("cache_objects")),
    );
    let mut hash_index = crate::build::cache::HashIndex::new();
    let cfg = ImageCompressionConfig {
        min_size_kb: 0,
        ..Default::default()
    };

    let items = collect_images_for_conversion(&structure, &transforms, &mut hash_index, &cfg);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].skip, Some(SkipReason::Cmyk));
}

#[test]
fn only_promised_variants_are_settled_for_a_skipped_source() {
    // The `<picture>` promise exists for png/jpg/jpeg only. A webp carries
    // its ladder on `<img srcset>` whose base URL IS the source file, so a
    // Failed mark there would replace the file with a warning tile.
    assert!(SkipReason::Cmyk.settles_a_promised_variant("JPG"));
    assert!(SkipReason::NotAnImage.settles_a_promised_variant("png"));
    assert!(!SkipReason::NotAnImage.settles_a_promised_variant("webp"));
    assert!(!SkipReason::NotAnImage.settles_a_promised_variant("gif"));
    assert!(!SkipReason::AlreadySmall.settles_a_promised_variant("jpg"));
    assert!(SkipReason::SourceInTheCloud.settles_a_promised_variant("gif"));
}

// ----- dispatch_image_conversions -----

#[test]
fn test_dispatch_image_conversions_empty_items() {
    // With empty image_items, dispatch must return without panicking and
    // must not touch the task tracker. No event sink / services expected.
    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![],
        source_path: "/nonexistent".to_string(),
        staging_dir: std::path::PathBuf::from("/nonexistent/staging"),
        moss_dir: std::path::PathBuf::from("/nonexistent/.moss"),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    // Pass None for services — dispatch returns early regardless.
    dispatch_image_conversions(None, &ctx, None);
}

#[test]
fn test_dispatch_image_conversions_with_items_produces_webp_headless() {
    // Integration-style: headless dispatch should run synchronously and
    // produce a .webp in the staging dir for one JPEG item.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    let img_path = root.join(img_rel);
    make_big_jpeg(&img_path, 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&img_path).unwrap();

    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    let services = BuildServices::headless();
    dispatch_image_conversions(Some(&services), &ctx, None);

    // Runner is synchronous in headless mode (event_sink is None), so by
    // the time dispatch returns, the .webp should exist.
    let webp_output = staging.join("photo.webp");
    assert!(
        webp_output.exists(),
        ".webp should have been produced in staging: {:?}",
        webp_output
    );

    // Task tracker for images should return to zero after conversion.
    assert!(
        !services.has_ui_bound(),
        "ui_bound counter must decrement after conversion finishes"
    );
}

#[test]
fn test_image_dispatch_applies_dir_overrides_to_served_path() {
    // Fix for I3: end-to-end check that `ctx.dir_overrides` flows
    // through dispatch → ImageRunContext → run_image_conversion and remaps
    // the staging output path. A future refactor that drops this threading
    // would leave the `.webp` at the file-tree location — this test catches
    // that regression by asserting on the actual output path.
    use std::collections::HashMap;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Source JPEG lives at source-folder/images/photo.jpg on disk.
    let img_rel = "source-folder/images/photo.jpg";
    let img_path = root.join(img_rel);
    fs::create_dir_all(img_path.parent().unwrap()).unwrap();
    make_big_jpeg(&img_path, 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&img_path).unwrap();

    // Override remaps the "source-folder" prefix segment to "published".
    // `resolve_path_with_overrides` matches on cumulative path segments,
    // so "source-folder" → "published" rewrites the first segment only,
    // leaving "images/photo.jpg" untouched.
    let mut overrides = HashMap::new();
    overrides.insert("source-folder".to_string(), "published".to_string());

    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        dir_overrides: overrides.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    let services = BuildServices::headless();
    dispatch_image_conversions(Some(&services), &ctx, None);

    // The override must remap the output path. Under overrides the .webp
    // should land at `published/images/photo.webp`, NOT at the file-tree
    // path `source-folder/images/photo.webp`.
    let mapped_webp = staging.join("published/images/photo.webp");
    let unmapped_webp = staging.join("source-folder/images/photo.webp");
    assert!(
        mapped_webp.exists(),
        "expected .webp at mapped page-tree path {:?}",
        mapped_webp
    );
    assert!(
        !unmapped_webp.exists(),
        "unexpected .webp at file-tree path {:?} — dir_overrides was not applied",
        unmapped_webp
    );
}

// =========================================================================
// Stale-cleanup protection (CRITICAL-1 regression tests)
// =========================================================================

/// Coordinator round-trip: image runner emits `ImageVariants` for each
/// produced `.webp`. The path appears in `SealedManifest::image_outputs`
/// and NOT in `files` / `blocking_keys` (per `HashBucket::ImageVariants`
/// semantics).
///
/// Pre-Track A this lived as `test_update_image_hashes_registers_outputs`
/// and asserted the on-disk `hashes.json` contained the .webp path after
/// `update_image_hashes(&ctx)`. The on-disk fallback is gone (#620 Item 2);
/// the runner now sends `EmitMessage::ImageVariants` through the
/// coordinator channel.
#[tokio::test]
async fn test_image_outputs_emitted_via_coordinator() {
    use crate::build::coordinator::{test_utils, EmitMessage};
    use crate::build::manifest::HashBucket;

    let (tx, rx) = test_utils::build_test_coordinator();
    tx.send(EmitMessage::File {
        rel_path: "images/hero.webp".to_string(),
        hash: "deadbeef00000001".to_string(),
        bucket: HashBucket::ImageVariants,
        oid: None,
    })
    .await
    .unwrap();
    drop(tx);

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.image_outputs().contains("images/hero.webp"),
        "image_outputs should contain 'images/hero.webp', got: {:?}",
        sealed.image_outputs()
    );
    // ImageVariants must NOT enter blocking_keys (stale-HTML cleanup must
    // not treat .webp variants as HTML to preserve).
    assert!(!sealed.blocking_keys().contains("images/hero.webp"));
    // ImageVariants must ALSO enter files (2026-05-15) so the deploy
    // wire manifest carries the path. Without this, the seta server
    // never asks for the .webp and live <picture><source srcset> 404s.
    assert!(
        sealed.files().contains_key("images/hero.webp"),
        "files must contain 'images/hero.webp' for deploy upload"
    );
}

/// Coordinator round-trip: the runner pre-maps file-tree paths to
/// page-tree paths via `resolve_path_with_overrides()` *before* sending
/// to the coordinator, so the wire format is always page-tree. This test
/// confirms a mapped path round-trips unchanged.
///
/// Pre-Track A this lived as `test_update_image_hashes_uses_mapped_paths`.
#[tokio::test]
async fn test_image_outputs_emitted_via_coordinator_mapped_path() {
    use crate::build::coordinator::{test_utils, EmitMessage};
    use crate::build::manifest::HashBucket;

    let (tx, rx) = test_utils::build_test_coordinator();
    // Mirror what the runner sends: the already-mapped page-tree path,
    // produced by `resolve_path_with_overrides("图片/hero.jpg", ...)`.
    tx.send(EmitMessage::File {
        rel_path: "images/hero.webp".to_string(),
        hash: String::new(),
        bucket: HashBucket::ImageVariants,
        oid: None,
    })
    .await
    .unwrap();
    drop(tx);

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.image_outputs().contains("images/hero.webp"),
        "image_outputs should contain mapped 'images/hero.webp', got: {:?}",
        sealed.image_outputs()
    );
    assert!(
        !sealed.image_outputs().contains("图片/hero.webp"),
        "image_outputs should NOT contain the unmapped file-tree path"
    );
}

/// End-to-end: dispatch_image_conversions (headless, with coordinator)
/// produces `.webp`, the runner emits `ImageVariants` to the coordinator,
/// and the resulting `SealedManifest::image_outputs` protects the file
/// from `remove_stale_files`. This is the exact bug the architecture
/// review flagged for CRITICAL-1.
///
/// Pre-Track A this used the on-disk `hashes.json` round-trip (the
/// `update_image_hashes` fallback). Now uses the coordinator path —
/// the only path post-#620 Item 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_webp_survives_stale_cleanup_after_dispatch() {
    use crate::build::media::pipeline::remove_stale_files;
    use crate::build::coordinator::test_utils;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    let img_path = root.join(img_rel);
    make_big_jpeg(&img_path, 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&img_path).unwrap();

    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    let services = BuildServices::headless();
    let (tx, rx) = test_utils::build_test_coordinator();

    // dispatch_image_conversions in headless mode (no event_sink) runs
    // synchronously. Run it on spawn_blocking so the coordinator can
    // drain on the runtime; tx is dropped after dispatch returns.
    let staging_for_blocking = staging.clone();
    let services_for_blocking = services;
    let ctx_for_blocking = ctx;
    let tx_for_blocking = tx.clone();
    let _ = staging_for_blocking; // keep handles alive in scope
    tokio::task::spawn_blocking(move || {
        dispatch_image_conversions(
            Some(&services_for_blocking),
            &ctx_for_blocking,
            Some(tx_for_blocking),
        );
    })
    .await
    .unwrap();
    drop(tx); // close all senders; coordinator can now seal

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    // Sanity: .webp materialized.
    let webp_output = staging.join("photo.webp");
    assert!(webp_output.exists(), ".webp should exist after dispatch");

    // The coordinator received the runner's ImageVariants emit.
    assert!(
        sealed.image_outputs().contains("photo.webp"),
        "image_outputs must contain 'photo.webp' after dispatch, got: {:?}",
        sealed.image_outputs()
    );

    // Build a SiteHashes view from the sealed manifest for stale cleanup.
    // remove_stale_files reads `image_outputs` to keep .webp alive.
    let mut hashes = SiteHashes::new();
    for k in sealed.image_outputs() {
        // Already-normalized strings from the sealed manifest. Wrap to satisfy the typed accessor.
        // .unwrap(): keys from sealed manifest are already-normalized, known valid.
        hashes.insert_image_output(&crate::build::served_path::ServedPath::from_source(k).unwrap());
    }
    remove_stale_files(&staging, &hashes, "test", &crate::build::lifecycle::permit_for_test());

    assert!(
        webp_output.exists(),
        ".webp must survive stale cleanup when registered in image_outputs"
    );
}

/// The data-loss guard per-item fingerprinting exists for: a vault with 3
/// already-converted images, adding a 4th, must dispatch ONLY the new one.
/// Before per-item fingerprinting, adding any one image invalidated the
/// WHOLE-SET fingerprint and re-dispatched all four; combined with
/// `emit_image_outputs_via_channel` only registering what a round actually
/// finished, a build that sealed before the async batch re-registered the
/// three untouched images would let the seal's own stale-file sweep delete
/// their already-finished, physically-present outputs (the same class of
/// bug `5323496908` fixed for video, now enforced per image).
///
/// Ablate by reverting `dispatch_image_conversions`'s GUI branch to the old
/// whole-set decision (`compute_image_set_fingerprint` +
/// `check_and_update_image_fingerprint`, dispatching every item whenever
/// the set doesn't match) and this goes red: every untouched image's
/// sentinel bytes are gone, overwritten by a real re-encode.
#[tokio::test]
async fn a_new_image_only_dispatches_the_new_one_others_survive_seal_and_stale_sweep() {
    use crate::build::coordinator::test_utils;
    use crate::build::media::pipeline::{compute_expected_dirs, remove_stale_dirs, remove_stale_files};

    /// Runs blocking work inline on whatever thread calls it — used here so
    /// the whole dispatch (including any actually-dispatched encode) is
    /// finished by the time `spawn_blocking` returns, with no separate wait.
    struct InlineSpawner;
    impl crate::build::ports::spawner::Spawner for InlineSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            task();
        }
        fn spawn(
            &self,
            _task: crate::build::ports::spawner::Task,
        ) -> crate::build::ports::spawner::Joining {
            Box::pin(async { Ok(()) })
        }
    }

    let _guard = image_fingerprint_test_lock().lock();

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let cfg = ImageCompressionConfig::default();

    // Three already-converted images: real source bytes on disk, a staged
    // `.webp` with SENTINEL bytes a real encode would never produce, and a
    // primed per-item fingerprint so dispatch takes the skip branch for
    // each. The sentinel surviving unchanged is the disk-level proof that
    // an image was NOT re-dispatched.
    let mut items: Vec<ImageConversionItem> = Vec::new();
    let mut untouched_webps: Vec<PathBuf> = Vec::new();
    for (i, name) in ["one.jpg", "two.jpg", "three.jpg"].iter().enumerate() {
        let path = root.join(name);
        make_big_jpeg(&path, 400, 300);
        let source_oid = crate::build::cache::ObjectStore::hash_file(&path).unwrap();
        let item = ImageConversionItem {
            source_path: PathBuf::from(*name),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        };
        let webp_rel = format!("{}.webp", name.trim_end_matches(".jpg"));
        let webp_path = staging.join(&webp_rel);
        fs::write(&webp_path, format!("SENTINEL-{}", i)).unwrap();
        let fp = compute_image_item_fingerprint(&root.to_string_lossy(), &item.source_path, &cfg)
            .expect("source exists and is stat-able");
        record_image_item_fingerprint(&item.source_path.to_string_lossy(), fp);
        untouched_webps.push(webp_path);
        items.push(item);
    }

    // A brand new, never-before-seen fourth image — no staged output, no
    // primed fingerprint.
    let new_path = root.join("four.jpg");
    make_big_jpeg(&new_path, 400, 300);
    let new_oid = crate::build::cache::ObjectStore::hash_file(&new_path).unwrap();
    items.push(ImageConversionItem {
        source_path: PathBuf::from("four.jpg"),
        source_oid: new_oid,
        ext: "jpg".to_string(),
        dimensions: None,
        skip: None,
    });

    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: items,
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    let services = BuildServices {
        spawner: Some(std::sync::Arc::new(InlineSpawner)),
        ..BuildServices::headless()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    // blocking_send (inside emit_image_outputs_via_channel) requires a
    // non-async thread — spawn_blocking, as in production where the
    // dispatcher runs from the blocking render phase.
    tokio::task::spawn_blocking(move || {
        dispatch_image_conversions(Some(&services), &ctx, Some(tx));
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    let view = sealed.site_hashes_view();
    remove_stale_files(&staging, view, "test", &crate::build::lifecycle::permit_for_test());
    remove_stale_dirs(&staging, &compute_expected_dirs(view), &crate::build::lifecycle::permit_for_test());

    for (i, webp_path) in untouched_webps.iter().enumerate() {
        assert!(
            webp_path.exists(),
            "'{}' (untouched image output) was deleted by the stale sweep after a \
             sibling image was added — sealed image_outputs: {:?}",
            webp_path.display(),
            sealed.image_outputs()
        );
        assert_eq!(
            fs::read(webp_path).unwrap(),
            format!("SENTINEL-{}", i).into_bytes(),
            "'{}' content changed — it was re-dispatched even though its own \
             fingerprint and output were unchanged",
            webp_path.display()
        );
    }

    // The new image WAS dispatched: no ffmpeg-style stand-in here — a real
    // JPEG decode+WebP encode ran, so its bytes are not the sentinel.
    let new_webp = staging.join("four.webp");
    assert!(
        new_webp.exists(),
        "the new image must have been dispatched and produced a .webp output"
    );
    assert_ne!(
        fs::read(&new_webp).unwrap(),
        b"SENTINEL-0".to_vec(),
        "the new image's output must be a real encode, not a carried-forward sentinel"
    );
}

// =========================================================================
// Cancel propagation through FolderSession (post-Track A)
// =========================================================================

// (Pre-Track A: test_image_not_cancelled_by_video_cancel asserted that the
// VideoConversionState and ImageConversionState had distinct AtomicBool
// flags, since `dispatch_video_conversions` would call `cancel()` on the
// video flag mid-build. Both flags are gone — there is no per-type cancel
// state to contaminate, so the regression cannot recur. Test removed.)

/// Cancelling the FolderSession halts the image runner.
/// Replaces the pre-Track A `test_image_cancel_aborts_image_runner` which
/// fired `services.image_cancellation.cancel()` directly. After A2 the
/// per-type cancel flag is gone; the cancel signal flows through
/// `session.cancel`, which the runner reads via a bridged AtomicBool.
///
/// Pre-2026-05-17 this test was flaky in batched runs: the async bridge
/// task spawned by `bridge_to_atomic` could starve under concurrent test
/// load and leave the local AtomicBool `false` when the runner's first
/// cancel check fired. The runner now seeds the AtomicBool synchronously
/// from `token.is_cancelled()` BEFORE installing the bridge, so a session
/// that's already cancelled at dispatch time deterministically aborts.
#[tokio::test]
async fn test_image_cancel_aborts_image_runner() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img_rel = "photo.jpg";
    let img_path = root.join(img_rel);
    make_big_jpeg(&img_path, 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();
    fs::write(
        moss_dir.join("build").join("hashes.json"),
        serde_json::to_string_pretty(&SiteHashes::new()).unwrap(),
    )
    .unwrap();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&img_path).unwrap();

    // Headless services + a real FolderSession so the runner's bridge has
    // a token to wire up.
    let mut services = BuildServices::headless();
    services.session = Some(crate::system::folder_session::FolderSession::new(
        root.to_path_buf(),
    ));

    // Pre-cancel the session BEFORE the runner installs its bridge. The
    // runner now reads `is_cancelled()` synchronously and seeds the
    // local AtomicBool, so this is deterministic — no yielding needed
    // for the bridge task to fire (which used to be the flake source).
    services.session.as_ref().unwrap().cancel.cancel();

    let run_ctx = ImageRunContext {
        items: vec![ImageConversionItem {
            source_path: PathBuf::from(img_rel),
            source_oid,
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        }],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: None,
        rung_collisions: Default::default(),
    };
    // Mirror dispatch's bookkeeping (runner decrements on early-exit).
    services.begin_ui_bound();

    // The runner is synchronous CPU work. Run it on a blocking thread so
    // the tokio runtime can poll the bridge task between calls (and so
    // the assertion below is awaited cleanly).
    let services_arc = std::sync::Arc::new(services);
    let services_clone = services_arc.clone();
    let staging_clone = staging.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    let webp_output = staging_clone.join("photo.webp");
    assert!(
        !webp_output.exists(),
        "image runner must abort and NOT produce .webp when session is cancelled"
    );
}

/// Regression: mid-batch cancellation must still register already-produced
/// `.webp` outputs in `hashes.json::image_outputs`. Without this, the next
/// rebuild's `remove_stale_files` treats them as unregistered and deletes
/// them — user-visible as "my images vanished after I switched folders
/// mid-conversion".
///
/// Approach: dispatch two images, spawn a watcher thread that flips the
/// image-cancel flag as soon as the first `.webp` appears on disk. The
/// main-thread runner then hits one of the two cancel early-return paths
/// (top-of-loop check OR per-item `Err("Cancelled")`), and the fix
/// ensures `update_image_hashes` runs before the return.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_image_hashes_updated_on_cancellation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let img1_rel = "photo1.jpg";
    let img2_rel = "photo2.jpg";
    make_big_jpeg(&root.join(img1_rel), 400, 300);
    make_big_jpeg(&root.join(img2_rel), 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();
    fs::write(
        moss_dir.join("build").join("hashes.json"),
        serde_json::to_string_pretty(&SiteHashes::new()).unwrap(),
    )
    .unwrap();

    let oid1 = crate::build::cache::ObjectStore::hash_file(&root.join(img1_rel)).unwrap();
    let oid2 = crate::build::cache::ObjectStore::hash_file(&root.join(img2_rel)).unwrap();

    use crate::build::coordinator::test_utils;

    let (tx, rx) = test_utils::build_test_coordinator();
    let mut services = BuildServices::headless();
    // Attach a real session so we can fire cancellation through the
    // post-Track A path (`session.cancel.cancel()`).
    let session = crate::system::folder_session::FolderSession::new(root.to_path_buf());
    services.session = Some(session.clone());

    let run_ctx = ImageRunContext {
        items: vec![
            ImageConversionItem {
                source_path: PathBuf::from(img1_rel),
                source_oid: oid1,
                ext: "jpg".to_string(),
                dimensions: None,
                skip: None,
            },
            ImageConversionItem {
                source_path: PathBuf::from(img2_rel),
                source_oid: oid2,
                ext: "jpg".to_string(),
                dimensions: None,
                skip: None,
            },
        ],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        config: ImageCompressionConfig::default(),
        dir_overrides: std::collections::HashMap::new(),
        tx: Some(tx.clone()),
        rung_collisions: Default::default(),
    };
    for _ in 0..2 {
        services.begin_ui_bound();
    }

    // Watcher thread: cancel the session as soon as the first webp
    // materializes. The runner's `bridge_to_atomic` flips a local
    // AtomicBool when the token fires; the runner will bail on the next
    // iteration (top-of-loop check or `convert_single_image`'s internal
    // cancel check — both early-return paths are covered).
    let first_webp = staging.join("photo1.webp");
    let cancel_session = session.clone();
    let watcher = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if first_webp.exists() {
                cancel_session.cancel.cancel();
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    });

    // Run the synchronous runner on a blocking thread so the tokio
    // runtime can poll the bridge task on a separate worker.
    let services_for_blocking = std::sync::Arc::new(services);
    let services_clone = services_for_blocking.clone();
    tokio::task::spawn_blocking(move || {
        run_image_conversion(&services_clone, &run_ctx);
    })
    .await
    .unwrap();

    let cancelled_in_time = watcher.join().unwrap();
    assert!(
        cancelled_in_time,
        "watcher should have observed photo1.webp and fired session.cancel.cancel()"
    );

    // First item must have produced its .webp before the cancel landed.
    assert!(
        staging.join("photo1.webp").exists(),
        "first image should have completed before cancellation"
    );

    // The fix: even though the runner bailed via an early-return cancel
    // path, `emit_image_outputs_via_channel` must still have fired —
    // so the already-produced photo1.webp key reaches the coordinator
    // and ends up in `SealedManifest::image_outputs`.
    drop(tx); // close all senders so the coordinator can seal
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.image_outputs().contains("photo1.webp"),
        "first item's .webp key must be registered in image_outputs \
             after mid-batch cancellation (got: {:?})",
        sealed.image_outputs()
    );
}

// ----- flatten_alpha_to_white -----

#[test]
fn flatten_alpha_to_white_on_opaque_rgba_preserves_color() {
    use image::{ImageBuffer, Rgba};
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(1, 1, |_, _| Rgba([200, 100, 50, 255]));
    let img = DynamicImage::ImageRgba8(buf);
    let out = flatten_alpha_to_white(img);
    assert!(
        !out.color().has_alpha(),
        "result must have no alpha channel"
    );
    let px = out.to_rgb8().get_pixel(0, 0).0;
    assert_eq!(px, [200, 100, 50], "fully opaque pixel must keep its color");
}

#[test]
fn flatten_alpha_to_white_on_fully_transparent_becomes_white() {
    use image::{ImageBuffer, Rgba};
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(1, 1, |_, _| Rgba([0, 128, 64, 0]));
    let img = DynamicImage::ImageRgba8(buf);
    let out = flatten_alpha_to_white(img);
    let px = out.to_rgb8().get_pixel(0, 0).0;
    assert_eq!(
        px,
        [255, 255, 255],
        "fully transparent pixel must become white"
    );
}

#[test]
fn flatten_alpha_to_white_on_semi_transparent_blends_correctly() {
    use image::{ImageBuffer, Rgba};
    // alpha=128 ≈ 0.502. Pixel (0,0,0) blended on white: each channel ≈ 127.
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(1, 1, |_, _| Rgba([0, 0, 0, 128]));
    let img = DynamicImage::ImageRgba8(buf);
    let out = flatten_alpha_to_white(img);
    let px = out.to_rgb8().get_pixel(0, 0).0;
    for ch in px {
        assert!(
            (ch as i32 - 127).abs() <= 1,
            "semi-transparent black on white should be ~127, got {}",
            ch
        );
    }
}

#[test]
fn flatten_alpha_to_white_on_rgb_image_passes_through() {
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(2, 2, |x, y| Rgb([x as u8 * 100, y as u8 * 100, 50]));
    let img = DynamicImage::ImageRgb8(buf);
    let out = flatten_alpha_to_white(img);
    assert!(!out.color().has_alpha());
    assert_eq!(out.width(), 2);
    assert_eq!(out.height(), 2);
}

/// Producer-side coherence: when the staged file does not exist on disk,
/// `emit_image_outputs_via_channel` must skip the registration. Without
/// this guard, a silent failure in `convert_single_image`'s staging-link
/// step would still register the path in `image_outputs` and `inner.files`,
/// breaking deploy with `"Manifest claims '<path>' exists but it's missing
/// on disk"`. Regression for the CPHS faculty-webp deploy failure
/// (2026-05-18).
#[tokio::test]
async fn emit_image_outputs_skips_missing_staged_files() {
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;

    let staging = tempfile::tempdir().unwrap();
    // Write one of two expected webps. The other path is missing from disk.
    std::fs::write(staging.path().join("present.webp"), b"present-bytes").unwrap();

    let (tx, rx) = test_utils::build_test_coordinator();
    let paths = vec![("present.webp".to_string(), None), ("missing.webp".to_string(), None)];
    let staging_path = staging.path().to_path_buf();
    // `emit_image_outputs_via_channel` uses `blocking_send` and must run
    // on a non-async thread (matching production, which dispatches it via
    // `tokio::task::spawn_blocking` from `run_image_conversion`).
    let registry = std::sync::Arc::new(AssetRegistry::new());
    // Both variants were promised by the emitter, as `promise.rs` does.
    registry.set_pending("present.webp".to_string(), None, None);
    registry.set_pending("missing.webp".to_string(), None, None);
    let reg = registry.clone();
    tokio::task::spawn_blocking(move || {
        emit_image_outputs_via_channel(
            &Some(tx),
            &paths,
            &staging_path,
            &std::collections::HashSet::new(),
            Some(reg.as_ref()),
        );
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.image_outputs().contains("present.webp"),
        "present.webp must register (got: {:?})",
        sealed.image_outputs()
    );
    assert!(
        !sealed.image_outputs().contains("missing.webp"),
        "missing.webp must NOT register — producer-side coherence violation \
             (got: {:?})",
        sealed.image_outputs()
    );
    assert!(
        !sealed.files().contains_key("missing.webp"),
        "missing.webp must NOT appear in files map either (got keys: {:?})",
        sealed.files().keys().collect::<Vec<_>>()
    );
    // Skipping registration is only half the job: the promise must settle
    // `Failed` too, or `degrade` leaves a live <source> for a URL that never
    // shipped and `<picture>` renders blank (ADR-013 amendment 2026-09-09).
    assert!(
        matches!(registry.get("missing.webp"), Some(AssetState::Failed(_))),
        "a variant missing at emit must settle Failed so degrade can strip its \
         <source> (got: {:?})",
        registry.get("missing.webp")
    );
    assert!(
        !matches!(registry.get("present.webp"), Some(AssetState::Failed(_))),
        "a variant that landed must not be marked Failed (got: {:?})",
        registry.get("present.webp")
    );
}

/// moss#1085 gated the self-heal on the ship-time prune's suppressed set but
/// not the violation report, so every settled vault logged a permanent false
/// "coherence violation: staged .webp missing" for each orphan-pruned
/// variant. A suppressed path absent from staging is the *expected* state
/// (the prune deleted it on purpose) — it must produce no violation line.
#[tokio::test]
async fn emit_image_outputs_suppressed_absent_produces_no_violation() {
    let staging = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    let paths = vec![("pruned.webp".to_string(), None)];
    let mut suppressed = std::collections::HashSet::new();
    suppressed.insert("pruned.webp".to_string());
    let staging_path = staging.path().to_path_buf();

    let lines = tokio::task::spawn_blocking(move || {
        emit_image_outputs_via_channel(&Some(tx), &paths, &staging_path, &suppressed, None)
    })
    .await
    .unwrap();

    assert!(
        lines.is_empty(),
        "a suppressed, absent path must not produce a violation line: {:?}",
        lines
    );
}

/// The other half of the same guard: an absent path that is NOT in the
/// suppressed set is still a genuine coherence violation (an upstream
/// staging-link step claimed success without landing the bytes) and must
/// still be reported, with the count reflecting only the unsuppressed misses.
#[tokio::test]
async fn emit_image_outputs_unsuppressed_absent_still_violates() {
    let staging = tempfile::tempdir().unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    let paths = vec![("pruned.webp".to_string(), None), ("genuinely-missing.webp".to_string(), None)];
    let mut suppressed = std::collections::HashSet::new();
    suppressed.insert("pruned.webp".to_string());
    let staging_path = staging.path().to_path_buf();

    let lines = tokio::task::spawn_blocking(move || {
        emit_image_outputs_via_channel(&Some(tx), &paths, &staging_path, &suppressed, None)
    })
    .await
    .unwrap();

    assert_eq!(
        lines.len(),
        1,
        "the unsuppressed absence must still produce exactly one violation line: {:?}",
        lines
    );
    assert!(
        lines[0].contains("(1×") && lines[0].contains("genuinely-missing.webp"),
        "violation must count only the unsuppressed miss, not the suppressed one: {}",
        lines[0]
    );
}

/// The trap: a suppressed path can still exist on disk (suppression is
/// computed from THIS build's staging HTML, which is incomplete when images
/// run — a plugin/notebook/feed page written later can reference it). If
/// registration were skipped for every suppressed path regardless of
/// existence, `remove_stale_files` would delete a file a page points at.
/// Pin: registration depends only on presence, never on suppression.
#[tokio::test]
async fn emit_image_outputs_registers_suppressed_path_that_exists() {
    use crate::build::coordinator::test_utils;
    use crate::types::content::SiteHashes;

    let staging = tempfile::tempdir().unwrap();
    std::fs::write(staging.path().join("referenced-later.webp"), b"real-bytes").unwrap();

    let (tx, rx) = test_utils::build_test_coordinator();
    let paths = vec![("referenced-later.webp".to_string(), None)];
    let mut suppressed = std::collections::HashSet::new();
    suppressed.insert("referenced-later.webp".to_string());
    let staging_path = staging.path().to_path_buf();

    tokio::task::spawn_blocking(move || {
        emit_image_outputs_via_channel(&Some(tx), &paths, &staging_path, &suppressed, None);
    })
    .await
    .unwrap();

    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
    assert!(
        sealed.image_outputs().contains("referenced-later.webp"),
        "a suppressed path that exists on disk must still register — \
             otherwise stale-cleanup deletes a file a page points at (got: {:?})",
        sealed.image_outputs()
    );
}

// ----- coherence-violation log collapse (send-logs noise taming) -----

#[test]
fn coherence_summary_collapses_many_missing_into_one_line() {
    let missing: Vec<String> = (0..5967).map(|i| format!("img{i}.webp")).collect();
    let lines = summarize_coherence_violations(&missing, &[]);
    assert_eq!(
        lines.len(),
        1,
        "5,967 missing files must collapse to ONE line, got {}",
        lines.len()
    );
    assert!(
        lines[0].contains("5967"),
        "summary must report the suppressed count: {}",
        lines[0]
    );
    assert!(
        lines[0].contains("img0.webp"),
        "summary must include a representative sample: {}",
        lines[0]
    );
}

#[test]
fn coherence_summary_empty_when_no_violations() {
    assert!(summarize_coherence_violations(&[], &[]).is_empty());
}

#[test]
fn coherence_summary_one_line_per_class() {
    let lines = summarize_coherence_violations(
        &["a.webp".to_string()],
        &[("b.webp".to_string(), "permission denied".to_string())],
    );
    assert_eq!(lines.len(), 2, "missing + unreadable are distinct classes");
    assert!(
        lines[0].contains("missing"),
        "first line is the missing class: {}",
        lines[0]
    );
    assert!(
        lines[1].contains("unreadable") && lines[1].contains("permission denied"),
        "second line is the unreadable class with the error sample: {}",
        lines[1]
    );
}

// ----- Sized-raster (deployed raster originals) -----

/// High-entropy JPEG so downscale-then-reencode is unambiguously smaller
/// (noise is the worst case to compress, so 4000px ≫ 2400px in bytes).
///
/// The 4000×3000 call sites are the two slowest tests in the crate (~5s each,
/// generating and JPEG-encoding 12MP of noise) and look like obvious fixtures
/// to shrink. They are not: `encode_sized_raster` only wins bytes when the
/// source is far enough above `max_edge` (1600) that the smaller pixel count
/// beats the re-encode, and `sized_raster_oid_for_original` only stores a blob
/// when that win happens. Measured at 2400×1800 the sized output came out
/// LARGER than the source (4.77MB vs 4.45MB) and three tests went red. The
/// size is the precondition, not padding — shrink it and you delete the
/// coverage instead of speeding it up.
fn make_detailed_jpeg(path: &Path, w: u32, h: u32) {
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([
            ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) & 0xff) as u8,
            ((x.wrapping_mul(97) ^ y.wrapping_mul(193)) & 0xff) as u8,
            ((x.wrapping_mul(389) ^ y.wrapping_mul(769)) & 0xff) as u8,
        ])
    });
    DynamicImage::ImageRgb8(buf)
        .save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

/// Write an RGBA PNG whose LEFT half (x < w/2) is fully transparent
/// (alpha 0) with a red tint, and whose RIGHT half is opaque blue. The
/// transparent region is large so a pixel sampled well inside it stays
/// alpha 0 through a Lanczos3 downscale (the filter support never reaches
/// the opaque boundary).
fn make_transparent_png(path: &Path, w: u32, h: u32) {
    use image::Rgba;
    let buf: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, _| {
        if x < w / 2 {
            Rgba([255, 0, 0, 0]) // transparent
        } else {
            Rgba([0, 0, 255, 255]) // opaque
        }
    });
    DynamicImage::ImageRgba8(buf)
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn encode_sized_raster_jpeg_caps_dimensions_and_shrinks() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("big.jpg");
    make_detailed_jpeg(&src, 4000, 3000);
    let src_len = fs::metadata(&src).unwrap().len();

    // FALLBACK_MAX_EDGE, not cfg.max_edge: the deployed raster fallback is
    // capped at the top ladder rung, which is what the production caller
    // (`sized_raster_oid_for_original`) passes.
    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY).expect("encode");

    // Output is a JPEG (source format preserved), decodes, and is capped.
    assert_eq!(
        image::guess_format(&bytes).ok(),
        Some(image::ImageFormat::Jpeg),
        "a .jpg source must stay JPEG"
    );
    let img = image::load_from_memory(&bytes).expect("output must be a valid image");
    assert!(
        img.width().max(img.height()) <= FALLBACK_MAX_EDGE,
        "output max edge {} must be <= {}",
        img.width().max(img.height()),
        FALLBACK_MAX_EDGE
    );
    assert!(
        (bytes.len() as u64) < src_len,
        "sized {} bytes must be < source {} bytes",
        bytes.len(),
        src_len
    );
}

#[test]
fn encode_sized_raster_leaves_small_images_uncapped_but_still_jpeg() {
    // A source already under max_edge is not upsized; it is still a valid
    // re-encoded JPEG (never the raw original passthrough).
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("small.jpg");
    make_detailed_jpeg(&src, 300, 200);
    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY).expect("encode");
    let img = image::load_from_memory(&bytes).expect("valid image");
    assert_eq!((img.width(), img.height()), (300, 200));
}

#[test]
fn encode_sized_raster_png_preserves_transparency_and_caps_dimensions() {
    // The core bug fix: a transparent PNG larger than max_edge must come out
    // as a real PNG that is (a) still a PNG at the byte level, (b) has an
    // alpha channel, (c) keeps a known-transparent pixel transparent — NOT
    // flattened to a white box — and (d) is dimension-capped.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("logo.png");
    make_transparent_png(&src, 3000, 2000);

    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY).expect("encode");

    // (a) Real PNG bytes at the .png path (correct Content-Type on servers).
    assert_eq!(
        image::guess_format(&bytes).ok(),
        Some(image::ImageFormat::Png),
        "a .png source must stay PNG (not become JPEG)"
    );

    let img = image::load_from_memory(&bytes).expect("output must be a valid image");
    // (b) Alpha channel survives.
    assert!(
        img.color().has_alpha(),
        "sized PNG must retain an alpha channel"
    );
    // (d) Dimension-capped.
    assert!(
        img.width().max(img.height()) <= FALLBACK_MAX_EDGE,
        "output max edge {} must be <= {}",
        img.width().max(img.height()),
        FALLBACK_MAX_EDGE
    );

    // (c) A pixel well inside the fully-transparent left region is still
    // transparent (alpha 0), not an opaque white box.
    let rgba = img.to_rgba8();
    let sample = rgba.get_pixel(rgba.width() / 10, rgba.height() / 2);
    assert_eq!(
        sample[3], 0,
        "transparent region must stay transparent (alpha 0), got {:?} — a white \
             flatten would be [255,255,255,255]",
        sample.0
    );
}

/// The gate that makes the lossy PNG path safe. Restored (and upgraded) from
/// `encode_sized_raster_small_png_stays_png_with_alpha`, which used to assert
/// only "still a PNG, still has alpha" — true of a quantized PNG with a `tRNS`
/// chunk too, so it could not have caught a palette encode eating the alpha.
/// This asserts the ROAD, not just the outcome: a PNG with any transparent
/// pixel must come out TRUECOLOUR RGBA (IHDR colour type 6), i.e. it never
/// entered the quantizer at all.
///
/// Transparency is a real contract, not a regenerable expectation: transparent
/// PNGs are logos and diagrams, hard-edged artwork is exactly where palette +
/// error diffusion shows, and a flattened logo is a visible bug.
/// A 1x1 opaque PNG must encode rather than abort the build.
///
/// `is_photographic` asks whether error diffusion would help, and its ratio
/// test is `distinct >= ceil(pixels * 0.02)` — which is 1 for any image under
/// 50 pixels. So a single-pixel image clears the bar with its one colour and
/// was classified photographic, and `image` 0.25's Floyd-Steinberg indexes
/// (x + 1, y) unconditionally: "Image index (1, 0) out of bounds (1, 1)",
/// panicking mid-encode.
///
/// This is asserted here rather than left to `snapshot_media_site`, which is
/// where it actually surfaced: that fixture has a 1x1 logo.png, so the suite
/// reported a missing output file and a panic in a tokio worker several frames
/// away from the cause. A unit test names it.
#[test]
fn encode_sized_raster_survives_a_single_pixel_png() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("dot.png");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Rgb([12, 34, 56]));
    DynamicImage::ImageRgb8(buf).save(&src).unwrap();

    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY)
        .expect("a 1x1 png must encode, not panic");
    let img = image::load_from_memory(&bytes).expect("valid image");
    assert_eq!((img.width(), img.height()), (1, 1), "1x1 must stay 1x1");
}

#[test]
fn encode_sized_raster_keeps_transparent_png_off_the_palette_road() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("icon.png");
    make_transparent_png(&src, 100, 80);

    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY).expect("encode");

    // IHDR colour type lives at byte 25 (8 signature + 4 length + 4 "IHDR" +
    // 4 width + 4 height + 1 bit depth). 6 = RGBA truecolour, 3 = indexed.
    assert_eq!(
        bytes[25], 6,
        "a transparent PNG must stay TRUECOLOUR RGBA — colour type {} means it \
         went through the palette quantizer, which would eat the alpha",
        bytes[25]
    );

    let img = image::load_from_memory(&bytes).expect("valid image");
    assert_eq!((img.width(), img.height()), (100, 80), "not upsized");
    assert_eq!(
        img.to_rgba8().get_pixel(0, 0)[3],
        0,
        "the transparent corner pixel must stay transparent"
    );
}

#[test]
fn sized_raster_oid_reuses_transform_cache() {
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    let src = tmp.path().join("big.jpg");
    make_detailed_jpeg(&src, 4000, 3000);
    let source_oid = ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let oid1 = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("first sizing must succeed");

    // Remove the source: a genuine re-encode would now fail to decode and
    // return None. A warm cache must return the same oid WITHOUT reading the
    // source at all — proving the second build does not re-encode.
    fs::remove_file(&src).unwrap();
    let oid2 = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("second call must be a cache hit (no source read, no re-encode)");

    assert_eq!(
        oid1, oid2,
        "warm cache must return the same sized-raster oid"
    );
}

#[test]
fn sized_raster_oid_keeps_source_when_reencode_not_smaller() {
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    // A small (under max_edge, so NO downscale) but heavily compressed source
    // JPEG (q15) with high-frequency detail: its bytes are already tiny, so a
    // q82 re-encode is strictly larger. The keep-smaller guard must then keep
    // the SOURCE verbatim (return the source oid) rather than bloat the output.
    let src = tmp.path().join("already-optimized.jpg");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(300, 300, |x, y| {
        Rgb([
            ((x * 7) % 256) as u8,
            ((y * 11) % 256) as u8,
            (((x + y) * 13) % 256) as u8,
        ])
    });
    let img = DynamicImage::ImageRgb8(buf);
    let mut low_q: Vec<u8> = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut low_q, 15)
        .encode(
            img.to_rgb8().as_raw(),
            300,
            300,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    fs::write(&src, &low_q).unwrap();
    // Store the source blob in the CAS, exactly as the pipeline caller does
    // (store_file, pipeline.rs) — the keep-smaller decision points the
    // transform at this source oid, so its blob must exist for the cached
    // verbatim decision to resolve on the next build.
    let source_oid = objects.store_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let out = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("must return Some (verbatim source oid)");
    assert_eq!(
        out, source_oid,
        "keep-smaller guard must return the source oid when the re-encode is not smaller"
    );

    // The keep-verbatim decision is cached: removing the source, a second call
    // still returns the source oid without re-encoding (no per-build churn).
    fs::remove_file(&src).unwrap();
    let out2 = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("cache hit must return the cached verbatim decision");
    assert_eq!(out2, source_oid);
}

/// Write an OPAQUE, photograph-like truecolour PNG: smooth diagonal gradients
/// (the part a palette has to approximate) plus fine per-pixel noise (the part
/// that defeats lossless compression). This is the shape of the real corpus's
/// expensive files — 潮汐's largest deployed assets are 2000×2500 photographs
/// stored as PNG.
fn make_photographic_png(path: &Path, w: u32, h: u32) {
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
        let noise = ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) & 0x0f) as u8;
        Rgb([
            ((x * 255 / w.max(1)) as u8).saturating_add(noise),
            ((y * 255 / h.max(1)) as u8).saturating_add(noise),
            (((x + y) * 255 / (w + h).max(1)) as u8).saturating_add(noise),
        ])
    });
    DynamicImage::ImageRgb8(buf)
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn sized_raster_oid_shrinks_an_opaque_photographic_png() {
    // THE regression this exists to pin. A photographic PNG that already fits
    // inside the fallback cap has nothing to gain from resizing, and a
    // TRUECOLOUR re-encode of it is always BIGGER than the author's own file —
    // so the keep-smaller guard used to win every time and moss shipped the
    // original bytes verbatim. Measured on 潮汐: 469 PNG originals, 133.5 MB of
    // source, 134.0 MB deployed — a 0% saving that looked deliberate but was
    // just a lost size comparison.
    //
    // The fix is a LOSSY PNG: quantize to an 8-bit palette. It must stay a PNG,
    // because the `<img src>` fallback keeps the original filename
    // (`render/image.rs::synthesize_inner`) — JPEG bytes at a `.png` URL would
    // be a Content-Type lie.
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    // Deliberately UNDER FALLBACK_MAX_EDGE: no downscale is available, so the
    // only way to win bytes is the palette encode.
    let src = tmp.path().join("photo.png");
    make_photographic_png(&src, 1200, 900);
    let src_len = fs::metadata(&src).unwrap().len();
    let source_oid = objects.store_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let oid = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("photographic png must size");

    assert_ne!(
        oid, source_oid,
        "an opaque photographic PNG must NOT be kept verbatim — that is the bug"
    );

    let bytes = fs::read(objects.get_path(&oid).expect("blob must exist")).unwrap();
    assert_eq!(
        image::guess_format(&bytes).ok(),
        Some(image::ImageFormat::Png),
        "the fallback URL keeps the .png extension, so the bytes must stay PNG"
    );
    let img = image::load_from_memory(&bytes).expect("output must decode");
    assert_eq!(
        (img.width(), img.height()),
        (1200, 900),
        "already inside the cap — must not be resized"
    );
    assert!(
        (bytes.len() as u64) * 5 < src_len * 4,
        "palette PNG must be MATERIALLY smaller: {} bytes vs source {} ({:.2}x)",
        bytes.len(),
        src_len,
        bytes.len() as f64 / src_len as f64
    );
}

/// Write an OPAQUE, POSTER-like PNG: a grid of `block`-sized uniform squares,
/// each a slightly different colour. 1,024 distinct colours over 147,456
/// pixels is a ratio of 0.007 — the flat-artwork side of
/// `PHOTOGRAPHIC_COLOR_RATIO`, and close to the 潮汐 corpus's PNG median.
fn make_flat_art_png(path: &Path, blocks: u32, block: u32) {
    let side = blocks * block;
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(side, side, |x, y| {
        let (bx, by) = (x / block, y / block);
        let n = by * blocks + bx;
        Rgb([(n / blocks * 7) as u8, (n % blocks * 7) as u8, 128])
    });
    DynamicImage::ImageRgb8(buf)
        .save_with_format(path, image::ImageFormat::Png)
        .unwrap();
}

#[test]
fn flat_art_is_quantized_without_dither_speckle() {
    // The palette encode's risk is not transparency — `is_fully_opaque` covers
    // that — it is error diffusion. Dithering is right for a photograph, where
    // 256 colours cannot express a sky, and wrong for a poster, where the
    // diffused error lands in large uniform regions and reads as speckle.
    //
    // 潮汐 is the case that made this concrete: all 469 of its PNGs are
    // pngquant'd posters, comics and diagrams (every one already <=256 source
    // colours), so gating on opacity alone would have dithered the entire PNG
    // corpus of a site whose PNGs are exclusively flat artwork.
    //
    // Pinning the OUTPUT rather than the predicate: each source block is one
    // colour, so each output block must still be one colour. Diffusion is
    // exactly what would break that.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("poster.png");
    let (blocks, block) = (32, 12);
    make_flat_art_png(&src, blocks, block);

    let bytes = encode_sized_raster(&src, FALLBACK_MAX_EDGE, SIZED_JPEG_QUALITY).expect("encode");
    let img = image::load_from_memory(&bytes)
        .expect("output must decode")
        .to_rgba8();

    // Interior pixels only: Lanczos is a no-op here (384 < FALLBACK_MAX_EDGE),
    // but sampling away from the seams keeps the assertion about diffusion
    // rather than about edge handling.
    for by in 0..blocks {
        for bx in 0..blocks {
            let (x0, y0) = (bx * block + 2, by * block + 2);
            let first = *img.get_pixel(x0, y0);
            for dy in 0..block - 4 {
                for dx in 0..block - 4 {
                    assert_eq!(
                        *img.get_pixel(x0 + dx, y0 + dy),
                        first,
                        "block ({}, {}) is uniform in the source; a second colour at \
                         +({}, {}) means error diffusion ran on flat artwork",
                        bx,
                        by,
                        dx,
                        dy
                    );
                }
            }
        }
    }
}

#[test]
fn sized_raster_oid_keeps_png_verbatim_when_palette_cannot_beat_it() {
    // The other half of the guard: lossy must never mean "bigger". A tiny flat
    // graphic compresses to a few hundred bytes losslessly, while ANY indexed
    // PNG pays a fixed 768-byte PLTE chunk — so the palette encode loses, and
    // the source must be kept verbatim (real: 潮汐 has 3.7 KB PNGs whose
    // palette encode is 13 KB).
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    let src = tmp.path().join("swatch.png");
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(32, 32, |x, _| Rgb([(x * 8) as u8, 64, 128]));
    DynamicImage::ImageRgb8(buf)
        .save_with_format(&src, image::ImageFormat::Png)
        .unwrap();
    let source_oid = objects.store_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let out = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("must return Some (verbatim source oid)");
    assert_eq!(
        out, source_oid,
        "the palette encode is bigger here — the source must be kept verbatim"
    );
}

#[test]
fn sized_raster_oid_caps_the_fallback_at_fallback_max_edge() {
    // The deployed raster fallback is sized to FALLBACK_MAX_EDGE — since
    // moss#976 B1 a literal (1200), independent of both DEPLOY_MAX_EDGE
    // (2400) and the ladder's top rung (1600). At 2400 the fallback was the
    // highest-resolution asset in the build — bigger than every webp a
    // modern browser actually fetches — while being served only to the ~4%
    // of installs without WebP support; B1 measured 1200 as a further
    // near-zero-risk cut (~39.5% smaller JPEG, ~29.5% smaller PNG) on top of
    // that first fix.
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    // Between FALLBACK_MAX_EDGE (1200) and DEPLOY_MAX_EDGE (2400): the old
    // (pre-B1) cap would have left this untouched at 1800px.
    let src = tmp.path().join("wide.jpg");
    make_detailed_jpeg(&src, 1800, 1200);
    let source_oid = objects.store_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let oid = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("must size");
    let bytes = fs::read(objects.get_path(&oid).expect("blob must exist")).unwrap();
    let img = image::load_from_memory(&bytes).expect("output must decode");
    assert_eq!(
        img.width().max(img.height()),
        FALLBACK_MAX_EDGE,
        "the fallback must be capped at FALLBACK_MAX_EDGE, not DEPLOY_MAX_EDGE or the ladder"
    );
}

#[test]
fn sized_raster_oid_none_for_non_decodable() {
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    let bogus = tmp.path().join("broken.png");
    fs::write(&bogus, b"<!doctype html><title>404</title>").unwrap();
    let oid = ObjectStore::hash_file(&bogus).unwrap();
    let cfg = ImageCompressionConfig::default();

    assert!(
        sized_raster_oid_for_original(
            &bogus,
            &oid,
            &objects,
            &transforms,
            &cfg,
            SIZED_JPEG_QUALITY
        )
        .is_none(),
        "a non-decodable raster must yield None so the caller keeps the verbatim original"
    );
}

#[test]
fn sized_raster_oid_png_stores_transparent_png() {
    // End-to-end at the cache layer: a transparent PNG original produces a
    // stored blob that is a transparent PNG (source format + alpha kept).
    use crate::build::cache::{ObjectStore, TransformCache};

    let tmp = tempfile::tempdir().unwrap();
    let objects_root = tmp.path().join("objects");
    let objects = ObjectStore::new(objects_root.clone());
    let transforms = TransformCache::new(
        tmp.path().join("transforms"),
        ObjectStore::new(objects_root),
    );

    let src = tmp.path().join("logo.png");
    make_transparent_png(&src, 3000, 2000);
    let source_oid = ObjectStore::hash_file(&src).unwrap();
    let cfg = ImageCompressionConfig::default();

    let oid = sized_raster_oid_for_original(
        &src,
        &source_oid,
        &objects,
        &transforms,
        &cfg,
        SIZED_JPEG_QUALITY,
    )
    .expect("png sizing must succeed");
    let blob = objects.get_path(&oid).expect("blob must exist in CAS");
    let bytes = fs::read(&blob).unwrap();

    assert_eq!(
        image::guess_format(&bytes).ok(),
        Some(image::ImageFormat::Png),
        "the stored raster blob for a .png source must be a PNG"
    );
    let img = image::load_from_memory(&bytes).expect("valid image");
    assert!(img.color().has_alpha(), "stored PNG must keep alpha");
    assert!(img.width().max(img.height()) <= FALLBACK_MAX_EDGE, "capped");
}
// ----- Self-heal: re-materialize an orphaned staged .webp from CAS -----

/// Encode `photo.jpg` through the real pipeline, then DELETE the staged
/// `.webp` to simulate a staging swap / stale-cleanup orphaning an in-flight
/// encode's output. `lifecycle::cas_heal::rematerialize` must re-link the surviving
/// CAS blob back into staging (the bytes are recoverable; only the staging
/// LINK was lost), self-healing the coherence violation.
#[test]
fn rematerialize_relinks_orphaned_staged_webp() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let cfg = ImageCompressionConfig::default();

    // Encode: populates the CAS blob + transform record (keyed by the real
    // source oid) + stages the webp — exactly what a first build does.
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "encode failed: {:?}",
        outcome.error
    );
    let webp_oid = outcome.webp_oid.clone().unwrap();
    let staged = h.staging.join("photo.webp");
    assert!(
        staged.exists(),
        "precondition: staged webp present after encode"
    );
    let cas_bytes = fs::read(h.objects.get_path(&webp_oid).unwrap()).unwrap();

    // Orphan it: the staged link is gone, but the CAS blob survives.
    fs::remove_file(&staged).unwrap();
    assert!(!staged.exists(), "precondition: staged webp orphaned");
    assert!(
        h.objects.get_path(&webp_oid).is_some(),
        "CAS blob must survive"
    );

    // Self-heal — unchanged source (same stat record) ⇒ HashIndex::resolve
    // recovers the same oid, find_cached_output hits, link_to restores it.
    let params = cfg.to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    rematerialize(
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &src,
        "photo.jpg",
        &staged,
        "image/webp",
        HashPolicy::HashOnMiss,
    );

    assert!(
        staged.exists(),
        "self-heal must re-materialize the staged webp"
    );
    assert_eq!(
        fs::read(&staged).unwrap(),
        cas_bytes,
        "re-linked staged bytes must equal the CAS blob"
    );
}

/// Rung-kind lookup: the same self-heal must recover a LADDER RUNG from
/// its own transform kind (`image/webp-w{N}`), not just the base
/// `image/webp`. Pins the `transform` parameter wiring the fingerprint-skip
/// heal relies on: a caller passing the rung kind gets the RUNG's blob,
/// never the base variant's bytes.
#[test]
fn rematerialize_recovers_rung_via_rung_kind() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    // 1000px wide → ladder = [800] → one rung encode + an
    // "image/webp-w800" transform record beside the base's "image/webp".
    make_big_jpeg(&src, 1000, 750);
    let cfg = ImageCompressionConfig::default();

    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "encode failed: {:?}",
        outcome.error
    );
    let rung = outcome
        .rungs
        .iter()
        .find(|r| r.width == 800)
        .expect("1000px source must produce the w800 rung");
    let rung_oid = rung.oid.clone().expect("rung encode must store a CAS blob");
    let staged_rung = h.staging.join("photo.w800.webp");
    assert!(
        staged_rung.exists(),
        "precondition: staged rung present after encode"
    );
    let cas_bytes = fs::read(h.objects.get_path(&rung_oid).unwrap()).unwrap();

    // Orphan the RUNG only; the base webp stays put.
    fs::remove_file(&staged_rung).unwrap();
    assert!(
        h.staging.join("photo.webp").exists(),
        "base variant must be untouched"
    );

    let params = cfg.to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    rematerialize(
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &src,
        "photo.jpg",
        &staged_rung,
        "image/webp-w800",
        HashPolicy::HashOnMiss,
    );

    assert!(
        staged_rung.exists(),
        "self-heal must re-materialize the staged rung"
    );
    assert_eq!(
        fs::read(&staged_rung).unwrap(),
        cas_bytes,
        "re-linked bytes must equal the RUNG's CAS blob (rung-kind lookup, not the base)"
    );
}

/// No CAS blob for the source (never successfully encoded) ⇒ the guard must
/// NOT fabricate a phantom staged file. Re-creating bytes that don't exist
/// would re-introduce the register-without-write class of bugs.
#[test]
fn rematerialize_noop_without_cas_blob() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 64, 64);
    let staged = h.staging.join("photo.webp");
    assert!(!staged.exists());

    let params = ImageCompressionConfig::default().to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    rematerialize(
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &src,
        "photo.jpg",
        &staged,
        "image/webp",
        HashPolicy::HashOnMiss,
    );

    assert!(
        !staged.exists(),
        "no CAS blob ⇒ nothing to re-link; must not fabricate a phantom"
    );
}

/// A present, non-empty staged file is left byte-for-byte untouched — the
/// steady-state text-edit rebuild pays nothing (no needless reflink).
#[test]
fn rematerialize_leaves_present_file_untouched() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 200, 200);
    let staged = h.staging.join("photo.webp");
    fs::write(&staged, b"sentinel-existing-bytes").unwrap();

    let params = ImageCompressionConfig::default().to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    rematerialize(
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &src,
        "photo.jpg",
        &staged,
        "image/webp",
        HashPolicy::HashOnMiss,
    );

    assert_eq!(
        fs::read(&staged).unwrap(),
        b"sentinel-existing-bytes",
        "present staged file must be left untouched"
    );
}

/// End-to-end mechanism: after self-heal the coherence guard in
/// `emit_image_outputs_via_channel` REGISTERS the re-linked variant (present
/// on disk), whereas without self-heal it SKIPS (staged file absent). This is
/// the difference between the variant reaching `sealed.files` (→ AssetsSettled
/// → live swap) and being permanently missing.
#[test]
fn self_heal_then_emit_registers_relinked_webp() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let cfg = ImageCompressionConfig::default();
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "encode failed: {:?}",
        outcome.error
    );
    let staged = h.staging.join("photo.webp");
    fs::remove_file(&staged).unwrap();

    // Control: emit with the staged file still absent ⇒ SKIP (no File msg).
    {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
        emit_image_outputs_via_channel(
            &Some(tx),
            &[("photo.webp".to_string(), None)],
            &h.staging,
            &std::collections::HashSet::new(),
            None,
        );
        assert!(
            rx.try_recv().is_err(),
            "orphaned (absent) staged webp must NOT be registered"
        );
    }

    // Self-heal, then emit ⇒ REGISTERS the variant with a real content hash.
    let params = cfg.to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    rematerialize(
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &src,
        "photo.jpg",
        &staged,
        "image/webp",
        HashPolicy::HashOnMiss,
    );
    assert!(staged.exists(), "self-heal must restore the staged webp");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    emit_image_outputs_via_channel(
        &Some(tx),
        &[("photo.webp".to_string(), None)],
        &h.staging,
        &std::collections::HashSet::new(),
        None,
    );
    match rx.try_recv() {
        Ok(EmitMessage::File {
            rel_path,
            hash,
            bucket,
            ..
        }) => {
            assert_eq!(rel_path, "photo.webp");
            assert!(
                !hash.is_empty(),
                "registered variant must carry a content hash"
            );
            assert!(matches!(bucket, HashBucket::ImageVariants));
        }
        other => panic!(
            "expected a File registration after self-heal, got {:?}",
            other
        ),
    }
}

// ----- Commit 2: self-heal a just-dispatched batch's own output before registration -----

/// The exact coherence violation real uploads showed: `[ERROR] [image]
/// coherence violation: staged .webp missing (3× this build); skipping
/// manifest registration. … Upstream staging-link reported success but the
/// bytes are not on disk.` `convert_single_image`'s `link_to` really did
/// succeed — this test encodes through the real pipeline first, so the
/// staged bytes are genuinely present at that moment — but
/// `run_image_conversion` registers a whole batch's outputs only ONCE,
/// after every item finishes. On a cloud-synced vault the exclusion marker
/// on `.moss/build/staging` can silently fail to stick (moss#964 measured
/// it ABSENT on `.moss/build` while present on `.moss/cache` on a real
/// vault), so the provider can evict an early-finished item's `.webp`
/// before that shared registration pass reads it back.
///
/// `self_heal_before_registration` closes that gap by re-verifying (and
/// here, re-materializing from the still-intact CAS blob) every one of a
/// batch's own outputs immediately before registration. This is a
/// different call site from `self_heal_then_emit_registers_relinked_webp`
/// above, which covers the OLDER, already-existing self-heal for a
/// *carried-forward* (skip-branch) image; this one covers an image that
/// was *just dispatched and encoded in this very round*.
#[test]
fn an_evicted_batch_output_is_healed_before_registration_not_silently_dropped() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let cfg = ImageCompressionConfig::default();
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();

    // Encode through the real pipeline: this IS the "Upstream staging-link
    // reported success" moment — link_to really did put good bytes at
    // photo.webp, and the CAS blob it came from is independently intact.
    let outcome = convert_single_image(
        &src,
        &source_oid,
        "photo.webp",
        &h.temp,
        &h.staging,
        &h.objects,
        &h.transforms,
        &cfg,
        None,
        None,
        &HashMap::new(),
    );
    assert!(
        outcome.error.is_none(),
        "encode failed: {:?}",
        outcome.error
    );
    let staged = h.staging.join("photo.webp");
    assert!(
        staged.exists(),
        "precondition: staged webp present after encode"
    );

    let item = ImageConversionItem {
        source_path: PathBuf::from("photo.jpg"),
        source_oid: source_oid.clone(),
        ext: "jpg".to_string(),
        dimensions: None,
        skip: None,
    };

    // Simulate the eviction race: the provider zeroes the staged file to a
    // dataless placeholder sometime between this item's own successful
    // link_to (above) and the batch-wide registration pass about to run
    // below — the 0-byte-stub convention `output_present` (and this
    // codebase's own tests) already use to stand in for `SF_DATALESS`.
    fs::write(&staged, b"").unwrap();
    assert_eq!(
        fs::metadata(&staged).unwrap().len(),
        0,
        "precondition: staged webp evicted (0 bytes)"
    );

    // Control: registering now, WITHOUT the self-heal this fix adds, must
    // silently drop the path — reproducing the exact bug (a coherence
    // violation logged, no File message, the image vanishes from the built
    // site with no user-visible error).
    {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
        emit_image_outputs_via_channel(
            &Some(tx),
            &[("photo.webp".to_string(), None)],
            &h.staging,
            &std::collections::HashSet::new(),
            None,
        );
        assert!(
            rx.try_recv().is_err(),
            "control: an evicted staged webp must NOT be registered without self-heal — \
             confirms this scenario reproduces the coherence violation"
        );
    }

    // The fix: verify (and here, re-materialize from the surviving CAS
    // blob) every one of this batch's own outputs before registration ever
    // runs.
    let params = cfg.to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    let healed = self_heal_before_registration(
        &[item],
        h._tmp.path(),
        &HashMap::new(),
        &h.staging,
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &HashMap::new(),
        &std::collections::HashSet::new(),
    );
    assert_eq!(
        healed, 1,
        "self-heal must recover exactly the one evicted output"
    );
    assert!(
        fs::metadata(&staged).unwrap().len() > 0,
        "staged webp must be restored to real bytes, not left as a 0-byte stub"
    );

    // Registering again now succeeds: the image is present, not silently
    // missing — the outcome this fix requires.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    emit_image_outputs_via_channel(
        &Some(tx),
        &[("photo.webp".to_string(), None)],
        &h.staging,
        &std::collections::HashSet::new(),
        None,
    );
    match rx.try_recv() {
        Ok(EmitMessage::File {
            rel_path,
            hash,
            bucket,
            ..
        }) => {
            assert_eq!(rel_path, "photo.webp");
            assert!(
                !hash.is_empty(),
                "registered variant must carry a content hash"
            );
            assert!(matches!(bucket, HashBucket::ImageVariants));
        }
        other => panic!(
            "expected a File registration after self-heal, got {:?}",
            other
        ),
    }
}

/// No CAS blob for the source (genuinely never encoded, not merely
/// evicted) ⇒ `self_heal_before_registration` must not fabricate a phantom
/// file — mirrors `rematerialize_noop_without_cas_blob`, at this new call
/// site.
#[test]
fn self_heal_before_registration_does_not_fabricate_without_a_cas_blob() {
    let h = harness();
    let src = h._tmp.path().join("never-encoded.jpg");
    make_big_jpeg(&src, 200, 200);
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let item = ImageConversionItem {
        source_path: PathBuf::from("never-encoded.jpg"),
        source_oid,
        ext: "jpg".to_string(),
        dimensions: None,
        skip: None,
    };
    let cfg = ImageCompressionConfig::default();
    let params = cfg.to_params();
    let mut index = crate::build::cache::HashIndex::load(&h._tmp.path().join("hash_index"));
    let healed = self_heal_before_registration(
        &[item],
        h._tmp.path(),
        &HashMap::new(),
        &h.staging,
        &h.objects,
        &h.transforms,
        &params,
        &mut index,
        &HashMap::new(),
        &std::collections::HashSet::new(),
    );
    assert_eq!(
        healed, 0,
        "no CAS blob exists — nothing to heal, and nothing fabricated"
    );
    assert!(
        !h.staging.join("never-encoded.webp").exists(),
        "must not create a phantom staged file"
    );
}

// ----- Phase 1b: thread the just-encoded CAS oid through to the manifest -----

/// Inline no-op `Spawner`, shared shape with the other full-`dispatch_image_
/// conversions` tests in this file: it puts `dispatch_image_conversions` on
/// the GUI (fingerprint-skip) path but runs the encode honestly, so the whole
/// call finishes synchronously and the test needs no extra wait.
struct OidTestInlineSpawner;
impl crate::build::ports::spawner::Spawner for OidTestInlineSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        task();
    }
    fn spawn(
        &self,
        _task: crate::build::ports::spawner::Task,
    ) -> crate::build::ports::spawner::Joining {
        Box::pin(async { Ok(()) })
    }
}

/// Mirrors `ship_phase_ships_correct_bytes_from_cas_despite_stage_dir_being_
/// overwritten` (`ship.rs`) for the image worker's own encode path:
/// `run_image_conversion` now records the just-encoded webp's CAS oid instead
/// of throwing it away (the mechanical `oid: None` this whole change closes
/// the gap on), so `ship_phase` can read the immutable blob instead of the
/// mutable stage path a concurrent build can rewrite between seal and ship.
/// Driven through the real `dispatch_image_conversions` producer end to end —
/// a hand-built manifest never acquires a `staged_oid` in the first place and
/// would prove nothing about this fix.
#[tokio::test]
async fn ship_phase_ships_a_freshly_encoded_webp_from_cas_despite_stage_overwrite() {
    use crate::build::coordinator::test_utils;

    let _guard = image_fingerprint_test_lock().lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    make_big_jpeg(&root.join("photo.jpg"), 400, 300);
    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let item = ImageConversionItem {
        source_path: PathBuf::from("photo.jpg"),
        source_oid: crate::build::cache::ObjectStore::hash_file(&root.join("photo.jpg")).unwrap(),
        ext: "jpg".to_string(),
        dimensions: None,
        skip: None,
    };
    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![item],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };
    let services = BuildServices {
        spawner: Some(std::sync::Arc::new(OidTestInlineSpawner)),
        ..BuildServices::headless()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        dispatch_image_conversions(Some(&services), &ctx, Some(tx));
    })
    .await
    .unwrap();
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    let oid = sealed
        .staged_oid("photo.webp")
        .expect("a freshly encoded webp must carry a live staged_oid")
        .to_string();

    let object_store = crate::build::cache::ObjectStore::new(
        MossPaths::from_moss_dir(moss_dir.clone()).cache_objects(),
    );
    let cas_bytes = fs::read(object_store.get_path(&oid).expect("the oid must name a live CAS blob")).unwrap();

    // A concurrent build rewrites the mutable stage copy after this
    // manifest's oid was sealed.
    fs::write(staging.join("photo.webp"), b"CONCURRENT-OVERWRITE").unwrap();

    let site = root.join("site");
    crate::build::ship::ship_phase(&staging, &site, &sealed, Some(&object_store), None)
        .expect("ship_phase should succeed");

    let shipped = fs::read(site.join("photo.webp")).unwrap();
    assert_eq!(
        shipped, cas_bytes,
        "ship_phase must ship the CAS blob's bytes, not the concurrently \
         overwritten mutable stage copy"
    );
    assert_ne!(
        shipped,
        b"CONCURRENT-OVERWRITE".to_vec(),
        "premise: the overwrite really changed the stage bytes"
    );
}

/// Regression guard: if a future refactor drops the oid on `delivered`'s way
/// into `emit_image_outputs_via_channel`, this must fail loudly rather than
/// quietly reverting every image entry to the pre-fix, fingerprint-only ship.
/// Covers both the base variant and a ladder rung, since both `ensure_staged`
/// call sites in the main encode path carry their own oid.
#[tokio::test]
async fn run_image_conversion_always_records_a_staged_oid_for_base_and_rungs() {
    use crate::build::coordinator::test_utils;

    let _guard = image_fingerprint_test_lock().lock();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // 1000×750 lands one ladder rung (w800): deployed_width(1000,750) = 1000,
    // and LADDER = [800, 1600] keeps only 800 below that.
    make_big_jpeg(&root.join("wide.jpg"), 1000, 750);
    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();

    let item = ImageConversionItem {
        source_path: PathBuf::from("wide.jpg"),
        source_oid: crate::build::cache::ObjectStore::hash_file(&root.join("wide.jpg")).unwrap(),
        ext: "jpg".to_string(),
        dimensions: Some((1000, 750)),
        skip: None,
    };
    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![item],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };
    let services = BuildServices {
        spawner: Some(std::sync::Arc::new(OidTestInlineSpawner)),
        ..BuildServices::headless()
    };

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        dispatch_image_conversions(Some(&services), &ctx, Some(tx));
    })
    .await
    .unwrap();
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    assert!(
        sealed.files().contains_key("wide.webp"),
        "premise: the base variant was registered at all"
    );
    assert!(
        sealed.files().contains_key("wide.w800.webp"),
        "premise: the rung variant was registered at all"
    );
    assert!(
        sealed.staged_oid("wide.webp").is_some(),
        "the main encode path must record a staged_oid for the base variant"
    );
    assert!(
        sealed.staged_oid("wide.w800.webp").is_some(),
        "the main encode path must record a staged_oid for each ladder rung too"
    );
}

/// The carry-forward skip path's other half: `dispatch_image_conversions`'s
/// per-item fingerprint match never calls `convert_single_image` at all, so
/// it has no fresh oid to offer — self-heal relinks a cached blob without
/// surfacing which one. It must register with `oid: None` and lean on
/// `ship_phase`'s fingerprint fallback, never fabricate or resurrect a stale
/// one.
#[tokio::test]
async fn skip_path_carry_forward_registers_with_no_oid_not_a_stale_one() {
    let _guard = image_fingerprint_test_lock().lock();
    use crate::build::coordinator::test_utils;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    make_big_jpeg(&root.join("kept.jpg"), 400, 300);
    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();
    fs::write(staging.join("kept.webp"), b"kept-bytes").unwrap();

    let item = ImageConversionItem {
        source_path: PathBuf::from("kept.jpg"),
        source_oid: crate::build::cache::ObjectStore::hash_file(&root.join("kept.jpg")).unwrap(),
        ext: "jpg".to_string(),
        dimensions: None,
        skip: None,
    };
    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: vec![item.clone()],
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };
    let services = BuildServices {
        spawner: Some(std::sync::Arc::new(OidTestInlineSpawner)),
        ..BuildServices::headless()
    };

    // Prime the fingerprint so dispatch takes the self-heal/skip path instead
    // of a real encode — the branch that has no fresh oid at hand.
    let cfg = ImageCompressionConfig::default();
    let fp = compute_image_item_fingerprint(&ctx.source_path, &item.source_path, &cfg)
        .expect("source exists and is stat-able");
    record_image_item_fingerprint(&item.source_path.to_string_lossy(), fp);

    let (tx, rx) = test_utils::build_test_coordinator();
    tokio::task::spawn_blocking(move || {
        dispatch_image_conversions(Some(&services), &ctx, Some(tx));
    })
    .await
    .unwrap();
    let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

    assert_eq!(
        fs::read(staging.join("kept.webp")).unwrap(),
        b"kept-bytes",
        "premise: this took the skip path, not a real re-encode"
    );
    assert!(
        sealed.files().contains_key("kept.webp"),
        "premise: still registered despite taking the skip path"
    );
    assert!(
        sealed.staged_oid("kept.webp").is_none(),
        "the carry-forward skip path has no fresh oid at hand and must fall \
         back to fingerprint protection, not a fabricated or stale oid"
    );
}

/// Per-item skip/dispatch, exercised through two siblings with the SAME
/// recorded (matching) fingerprint but different on-disk state. `kept.jpg`'s
/// variant is already staged, so it takes the cheap skip path: self-heal
/// re-registers it WITHOUT ever entering `run_image_conversion` (proven by
/// its sentinel bytes surviving unchanged). `pending.jpg`'s variant was
/// never actually produced — a matched fingerprint was never proof the
/// LAST run that considered it actually left the file on disk — so it must
/// be dispatched and encoded this round rather than silently staying
/// unregistered forever (mirrors `dispatch_video_conversions`'s identical
/// self-heal-by-redispatch rule for a missing output, video.rs). Before
/// this per-item fix, this file's `self_heal_leaves_a_not_yet_encoded_
/// variant_pending_rather_than_failed` asserted `pending.webp` stayed
/// `Pending` forever under a matching WHOLE-SET fingerprint; the never-
/// mark-Failed half of that concern (moss#1044) still holds and is
/// asserted below, now satisfied by actually encoding the image instead of
/// leaving it stuck.
#[test]
fn a_missing_output_is_dispatched_while_its_unaffected_sibling_takes_the_skip_path() {
    /// Runs blocking work inline. Only its presence matters here — it is what
    /// puts `dispatch_image_conversions` on the GUI (fingerprint-skip) path —
    /// but it runs the task honestly so a missed skip encodes rather than
    /// silently doing nothing.
    struct InlineSpawner;
    impl crate::build::ports::spawner::Spawner for InlineSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            task();
        }
        fn spawn(
            &self,
            _task: crate::build::ports::spawner::Task,
        ) -> crate::build::ports::spawner::Joining {
            Box::pin(async { Ok(()) })
        }
    }

    let _guard = image_fingerprint_test_lock().lock();

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Two images from an earlier build: one whose staged variant is on disk,
    // one whose variant has not been encoded into this staging generation yet.
    make_big_jpeg(&root.join("kept.jpg"), 400, 300);
    make_big_jpeg(&root.join("pending.jpg"), 400, 300);

    let moss_dir = root.join(".moss");
    let staging = moss_dir.join("build").join("staging");
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("objects")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("transforms")).unwrap();
    fs::create_dir_all(moss_dir.join("build").join("cache").join("tmp")).unwrap();
    fs::write(staging.join("kept.webp"), b"kept-bytes").unwrap();

    let items: Vec<ImageConversionItem> = ["kept.jpg", "pending.jpg"]
        .iter()
        .map(|rel| ImageConversionItem {
            source_path: PathBuf::from(rel),
            source_oid: crate::build::cache::ObjectStore::hash_file(&root.join(rel)).unwrap(),
            ext: "jpg".to_string(),
            dimensions: None,
            skip: None,
        })
        .collect();

    let ctx = BackgroundContext {
        video_items: vec![],
        image_items: items.clone(),
        source_path: root.to_string_lossy().to_string(),
        staging_dir: staging.clone(),
        moss_dir: moss_dir.clone(),
        notebook_files: vec![],
        rung_collisions: Default::default(),
        ..BackgroundContext::for_test()
    };

    // Both variants were promised by the emitter, as `promise.rs` does.
    let registry = std::sync::Arc::new(AssetRegistry::new());
    registry.set_pending("kept.webp".to_string(), None, None);
    registry.set_pending("pending.webp".to_string(), None, None);

    let services = BuildServices {
        spawner: Some(std::sync::Arc::new(InlineSpawner)),
        assets: Some(registry.clone()),
        ..BuildServices::headless()
    };

    // Prime BOTH images' own fingerprints, which is what a rebuild that
    // changed neither image's bytes looks like to dispatch.
    let cfg = ImageCompressionConfig::default();
    for item in &items {
        let rel = item.source_path.to_string_lossy().to_string();
        let fp = compute_image_item_fingerprint(&ctx.source_path, &item.source_path, &cfg)
            .expect("source exists and is stat-able");
        record_image_item_fingerprint(&rel, fp);
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    dispatch_image_conversions(Some(&services), &ctx, Some(tx));

    let registered: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|msg| match msg {
            EmitMessage::File { rel_path, .. } => Some(rel_path),
            _ => None,
        })
        .collect();

    // kept.jpg took the skip path: self-heal found the variant already on
    // disk and re-registered it WITHOUT re-encoding — its sentinel bytes
    // prove no real encode touched it, even though its SIBLING dispatched.
    assert!(
        registered.contains(&"kept.webp".to_string()),
        "self-heal must register the already-staged variant; got {:?}",
        registered
    );
    assert_eq!(
        fs::read(staging.join("kept.webp")).unwrap(),
        b"kept-bytes",
        "kept.jpg must not be re-encoded just because a sibling image's output was missing"
    );

    // pending.jpg's matched fingerprint was never proof the last run that
    // considered it actually left the file on disk: it must be dispatched
    // and registered once encoded, not left permanently unregistered.
    assert!(
        registered.contains(&"pending.webp".to_string()),
        "a missing output behind a matched fingerprint must be dispatched and \
         registered once encoded; got {:?}",
        registered
    );
    assert!(
        matches!(registry.get("pending.webp"), Some(AssetState::Ready)),
        "the missing variant must reach Ready once dispatch encodes it; got {:?}",
        registry.get("pending.webp")
    );
    assert!(
        registry.failed_keys().is_empty(),
        "neither variant may settle Failed — on a synced vault an evicted \
         file reads absent too, and `degrade` would strip a healthy <source> \
         if a merely-not-yet-encoded variant were ever marked Failed; \
         failed: {:?}",
        registry.failed_keys()
    );
}

/// The same minimal 1×1 PNG `should_skip_not_an_image_real_png_passes` uses,
/// hoisted so the moss#985 tests below assert about a file that really is a
/// valid image — the whole point being that it was classified as not one.
#[cfg(test)]
fn real_png_bytes() -> &'static [u8] {
    &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR length + type
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth, color type, crc
        0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT length + type
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, // IDAT data
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, // CRC
        0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND length + type
        0x44, 0xAE, 0x42, 0x60, 0x82, // IEND data + CRC
    ]
}

// ---------------------------------------------------------------------------
// An unreadable source is not a verdict about its content (moss#985)
//
// Every other NotAnImage test above feeds the probe a READABLE file, which is
// why a suite of 8,608 tests stayed green while a valid PNG could be cached as
// "not an image" forever. The branch that produced that verdict — the read
// failing — had no test at all.
//
// `chmod 000` is the portable stand-in for eviction: it produces the same
// `Err` from the same `File::open`/`read` that `SF_DATALESS` produces under
// moss's fail-fast policy, on a box with no cloud provider. So this runs on
// every platform, unlike the eviction path it stands for.
// ---------------------------------------------------------------------------

/// Root ignores mode bits, so this cannot assert anything there. Skipping is
/// honest; asserting a false green is not.
#[cfg(unix)]
fn make_unreadable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
    fs::File::open(path).is_err()
}

#[cfg(unix)]
#[test]
fn an_unreadable_source_is_never_cached_as_not_an_image() {
    let h = harness();
    let src = h._tmp.path().join("evicted.png");
    fs::write(&src, real_png_bytes()).unwrap();
    if !make_unreadable(&src) {
        eprintln!("skipped: this process can read a 0o000 file (running as root?)");
        return;
    }

    let cfg = ImageCompressionConfig::default();
    let oid = "oid_for_an_unreadable_source";
    let reason = should_skip(&src, "png", 70, &cfg, &h.transforms, oid, false);

    assert_eq!(
        reason,
        Some(SkipReason::SourceInTheCloud),
        "an unreadable file says nothing about its content — the caller must \
         still register the variant promise, which NotAnImage would skip"
    );
    assert!(
        h.transforms.get(oid).is_none(),
        "nothing about content nobody read may reach the cache: the verdict is \
         keyed by content oid, and an evicted file's content never changes, so \
         a cached verdict here would outlive the eviction forever"
    );
}

/// The end of the story the previous test starts, and the one that actually
/// bit: the bytes arrive, and the image converts normally. Before the fix this
/// build emitted zero `.webp` variants while the synthesizer went on emitting
/// `<source srcset="…webp">` for the same file — ADR-013's unrecoverable
/// chosen-source 404, reached permanently through a transient cloud state.
#[cfg(unix)]
#[test]
fn a_source_that_becomes_readable_again_converts_normally() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness();
    let src = h._tmp.path().join("arrives.png");
    fs::write(&src, real_png_bytes()).unwrap();
    let cfg = ImageCompressionConfig::default();
    let oid = "oid_for_a_source_that_arrives";

    if !make_unreadable(&src) {
        eprintln!("skipped: this process can read a 0o000 file (running as root?)");
        return;
    }
    should_skip(&src, "png", 70, &cfg, &h.transforms, oid, false);

    // The download lands. Same path, same bytes, same content oid.
    fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();

    assert_ne!(
        should_skip(&src, "png", 70, &cfg, &h.transforms, oid, false),
        Some(SkipReason::NotAnImage),
        "the verdict must be recomputed from the bytes that have now arrived, \
         not served from what the failed read concluded"
    );
}

/// The guard must not overreach: a file that IS readable and genuinely is not
/// an image still gets the cached NotAnImage verdict it always did. Without
/// this, "never cache a failure" would quietly become "never cache", and every
/// HTML-saved-as-.png would re-probe on every build forever.
#[test]
fn a_readable_non_image_is_still_cached() {
    let h = harness();
    let src = h._tmp.path().join("404.png");
    fs::write(&src, b"<html>not an image</html>").unwrap();
    let cfg = ImageCompressionConfig::default();
    let oid = "oid_for_a_readable_non_image";

    assert_eq!(
        should_skip(&src, "png", 25, &cfg, &h.transforms, oid, false),
        Some(SkipReason::NotAnImage)
    );
    assert!(
        h.transforms.get(oid).is_some(),
        "a verdict read from real bytes is still worth keeping"
    );
}

/// An unreadable staged variant is neither present nor missing. Settling it
/// `Failed` would have `degrade` strip its `<source>` from a page whose bytes
/// may be fine; registration reports it `Unverified` instead, and the
/// generation that carries the mark is withheld.
#[cfg(unix)]
#[test]
fn registration_over_an_unreadable_variant_reports_it_and_never_fails_it() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness();
    let locked_dir = h.staging.join("locked");
    fs::create_dir_all(&locked_dir).unwrap();
    fs::write(locked_dir.join("photo.webp"), b"webp bytes").unwrap();
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked_dir).is_ok() {
        fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: this process can read a 0o000 directory (running as root?)");
        return;
    }

    let registry = AssetRegistry::new();
    registry.set_pending("locked/photo.webp".to_string(), None, None);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<EmitMessage>(16);
    emit_image_outputs_via_channel(
        &Some(tx),
        &[("locked/photo.webp".to_string(), None)],
        &h.staging,
        &std::collections::HashSet::new(),
        Some(&registry),
    );
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        !matches!(registry.get("locked/photo.webp"), Some(AssetState::Failed(_))),
        "an unreadable variant must not settle Failed — degrade would strip a healthy <source>"
    );
    match rx.try_recv() {
        Ok(EmitMessage::Unverified { rel_path, .. }) => assert_eq!(rel_path, "locked/photo.webp"),
        other => panic!("expected an Unverified report, got {other:?}"),
    }
}

// ----- a same-size rewrite in the same second is a different file -----

/// A vault with one image, and an image-side rebuild run the way a real one runs
/// it: the blocking phase collects against the index the last worker persisted,
/// then the worker encodes whatever it was handed.
struct RewriteVault {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    moss_dir: PathBuf,
}

impl RewriteVault {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let moss_dir = root.join(".moss");
        for dir in ["staging", "cache/objects", "cache/transforms", "cache/tmp"] {
            fs::create_dir_all(moss_dir.join("build").join(dir)).unwrap();
        }
        Self { _tmp: tmp, root, moss_dir }
    }

    fn pic(&self) -> PathBuf {
        self.root.join("pic.png")
    }

    /// The staged variant, as the last rebuild left it.
    fn webp(&self) -> Vec<u8> {
        fs::read(self.moss_dir.join("build/staging/pic.webp")).expect("the rebuild staged pic.webp")
    }

    /// Two solid colours whose PNGs are the same number of bytes, so a rewrite
    /// from one to the other is a same-size rewrite.
    fn write_pic(&self, rgb: [u8; 3]) {
        image::RgbImage::from_pixel(24, 24, Rgb(rgb)).save(self.pic()).unwrap();
    }

    /// Replace the image with same-size different bytes, stamped with the whole
    /// second the previous write carries (see `FileStat::stamp_in_the_second_of`).
    fn rewrite_in_the_same_second(&self, rgb: [u8; 3]) {
        let before = fs::metadata(self.pic()).unwrap();
        self.write_pic(rgb);
        assert_eq!(fs::metadata(self.pic()).unwrap().len(), before.len(), "precondition: a same-size rewrite");
        crate::build::cache::FileStat::stamp_in_the_second_of(&self.pic(), before.modified().unwrap());
    }

    /// What the blocking phase hands the worker: the images that need encoding, by the
    /// index the last worker persisted.
    fn collect(&self) -> Vec<ImageConversionItem> {
        let paths = MossPaths::from_moss_dir(self.moss_dir.clone());
        let structure = crate::types::content::ProjectStructure {
            root_path: self.root.to_string_lossy().to_string(),
            markdown_files: vec![],
            html_files: vec![],
            image_files: vec![MediaMetadata {
                path: "pic.png".to_string(),
                file_type: "png".to_string(),
                size: fs::metadata(self.pic()).unwrap().len(),
                ..Default::default()
            }],
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
        let transforms = crate::build::cache::TransformCache::new(
            paths.cache_transforms(),
            crate::build::cache::ObjectStore::new(paths.cache_objects()),
        );
        let mut index = crate::build::cache::HashIndex::load(&paths.cache_hash_index());
        collect_images_for_conversion(&structure, &transforms, &mut index, &ImageCompressionConfig::default())
            .into_iter()
            .filter(|item| item.skip.is_none())
            .collect()
    }

    /// The desktop app's services: a spawner, so each image is compared with the
    /// fingerprint of the last build that considered it. `moss build` has none.
    fn app_services() -> BuildServices {
        BuildServices {
            spawner: Some(std::sync::Arc::new(OidTestInlineSpawner) as std::sync::Arc<dyn crate::build::ports::spawner::Spawner>),
            ..BuildServices::headless()
        }
    }

    /// The same, with the user having cancelled the folder's work before the worker runs.
    fn cancelled_app_services(&self) -> BuildServices {
        let session = crate::system::folder_session::FolderSession::new(self.root.clone());
        session.cancel.cancel();
        BuildServices { session: Some(session), ..Self::app_services() }
    }

    /// Dispatch `items` the way a build's background phase does.
    async fn dispatch(&self, items: Vec<ImageConversionItem>, services: BuildServices) {
        let ctx = BackgroundContext {
            image_items: items,
            source_path: self.root.to_string_lossy().to_string(),
            staging_dir: self.moss_dir.join("build/staging"),
            moss_dir: self.moss_dir.clone(),
            ..BackgroundContext::for_test()
        };
        tokio::task::spawn_blocking(move || dispatch_image_conversions(Some(&services), &ctx, None))
            .await
            .unwrap();
    }

    /// One rebuild of the image side. `with_fingerprint_gate` runs it the way the
    /// desktop app does (see `app_services`); without, the way `moss build` does.
    async fn rebuild(&self, with_fingerprint_gate: bool) {
        let services = if with_fingerprint_gate { Self::app_services() } else { BuildServices::headless() };
        self.dispatch(self.collect(), services).await;
    }
}

/// The image worker's oid comes from the persisted index, and the index used to be
/// keyed by (path, size, whole-second mtime). A rewrite of the same size in the same
/// second matched the previous write's entry, so the old oid was reused, the cached
/// encode was found under it, and `pic.webp` kept the old picture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_size_rewrite_in_the_same_second_is_re_encoded() {
    let vault = RewriteVault::new();
    vault.write_pic([200, 30, 30]);
    vault.rebuild(false).await;
    let first = vault.webp();

    vault.rewrite_in_the_same_second([30, 30, 200]);
    vault.rebuild(false).await;

    assert_ne!(vault.webp(), first, "pic.png changed on disk but its variant is still the old encode");
}

/// The desktop app's rebuild has a second whole-second key in front of the index:
/// an image whose fingerprint (path, size, mtime, config) matches the last build's is
/// carried forward without ever reaching the worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_same_size_rewrite_in_the_same_second_gets_past_the_fingerprint_gate() {
    let _guard = image_fingerprint_test_lock().lock();
    let vault = RewriteVault::new();
    vault.write_pic([200, 30, 30]);
    vault.rebuild(true).await;
    let first = vault.webp();

    vault.rewrite_in_the_same_second([30, 30, 200]);
    vault.rebuild(true).await;

    assert_ne!(vault.webp(), first, "the fingerprint gate carried the old variant forward over a changed source");
}

// ----- the fingerprint gate vouches for an image only once its worker delivered it -----

/// The desktop app skips an image whose fingerprint matches the one recorded the last
/// time it was considered, when its variant is still in staging. If the fingerprint is
/// recorded at dispatch, a worker that leaves without encoding (the user cancelled,
/// the source went back to the cloud) has already vouched for a source it never
/// encoded, and the variant of the OLD bytes — kept in staging by the stale-output
/// pass — is what the next build's gate finds present and skips.
///
/// Build 1 encodes red. The source becomes blue. Build 2 is dispatched over blue and its
/// worker `leaves`. Build 3 finds blue unchanged since build 2, and must still encode it.
async fn a_source_changed_under_a_worker_that_left_is_encoded_by_the_next_build(
    leave: impl FnOnce(&RewriteVault) -> (BuildServices, Option<crate::build::icloud::pretend::Guard>),
) {
    let _guard = image_fingerprint_test_lock().lock();
    let vault = RewriteVault::new();
    vault.write_pic([200, 30, 30]);
    vault.rebuild(true).await;
    let first = vault.webp();

    vault.rewrite_in_the_same_second([30, 30, 200]);
    let items = vault.collect();
    let (services, still_holding) = leave(&vault);
    vault.dispatch(items, services).await;
    drop(still_holding);
    assert_eq!(vault.webp(), first, "premise: the worker that left encoded nothing, so the old variant is still staged");

    vault.rebuild(true).await;

    assert_ne!(vault.webp(), first, "pic.png is blue on disk but its variant is still the red encode: the gate skipped it");
}

/// The other half: a worker that delivers vouches for its source, so the next build
/// carries the variant forward (registered without an oid — only a worker's own
/// delivery has one) instead of dispatching it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_image_a_worker_delivered_is_carried_forward_by_the_next_build() {
    use crate::build::coordinator::test_utils;

    let _guard = image_fingerprint_test_lock().lock();
    let vault = RewriteVault::new();
    vault.write_pic([200, 30, 30]);
    let mut oids = Vec::new();
    for _ in 0..2 {
        let (tx, rx) = test_utils::build_test_coordinator();
        let ctx = BackgroundContext {
            image_items: vault.collect(),
            source_path: vault.root.to_string_lossy().to_string(),
            staging_dir: vault.moss_dir.join("build/staging"),
            moss_dir: vault.moss_dir.clone(),
            ..BackgroundContext::for_test()
        };
        let services = RewriteVault::app_services();
        tokio::task::spawn_blocking(move || dispatch_image_conversions(Some(&services), &ctx, Some(tx))).await.unwrap();
        let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;
        oids.push(sealed.staged_oid("pic.webp").map(str::to_string));
    }

    assert!(oids[0].is_some(), "premise: the first build's worker delivered the variant");
    assert_eq!(oids[1], None, "the second build dispatched an image its worker had already delivered");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_changed_under_a_cancelled_worker_is_encoded_by_the_next_build() {
    a_source_changed_under_a_worker_that_left_is_encoded_by_the_next_build(|vault| (vault.cancelled_app_services(), None)).await;
}

/// The other exit that leaves the old variant: the source is in the cloud when the worker
/// gets to it (it was local when the blocking phase collected it).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_changed_under_a_worker_that_deferred_it_to_the_cloud_is_encoded_by_the_next_build() {
    a_source_changed_under_a_worker_that_left_is_encoded_by_the_next_build(|vault| {
        (RewriteVault::app_services(), Some(crate::build::icloud::pretend::evicted(&vault.pic())))
    })
    .await;
}

/// Self-heal relinks a cached output by the source's oid. A same-size rewrite in the
/// same second has a new oid, so the previous picture's variant is not its output and
/// must not be linked into staging under its name.
#[test]
fn rematerialize_does_not_relink_the_previous_versions_output_over_a_same_size_rewrite() {
    let h = harness();
    let src = h._tmp.path().join("pic.png");
    let write = |rgb: [u8; 3]| image::RgbImage::from_pixel(24, 24, Rgb(rgb)).save(&src).unwrap();
    write([200, 30, 30]);
    let cfg = ImageCompressionConfig::default();

    // The last build: hashed through the index, encoded under that oid.
    let mut index = crate::build::cache::HashIndex::new();
    let oid = index.resolve(&src, "pic.png").unwrap();
    let outcome = convert_single_image(
        &src, &oid, "pic.webp", &h.temp, &h.staging, &h.objects, &h.transforms, &cfg, None, None, &HashMap::new(),
    );
    assert!(outcome.error.is_none(), "encode failed: {:?}", outcome.error);
    let staged = h.staging.join("pic.webp");
    fs::remove_file(&staged).unwrap();

    let before = fs::metadata(&src).unwrap();
    write([30, 30, 200]);
    assert_eq!(fs::metadata(&src).unwrap().len(), before.len(), "precondition: a same-size rewrite");
    crate::build::cache::FileStat::stamp_in_the_second_of(&src, before.modified().unwrap());

    let outcome = rematerialize(
        &h.objects, &h.transforms, &cfg.to_params(), &mut index, &src, "pic.png", &staged, "image/webp",
        HashPolicy::HashOnMiss,
    );
    assert!(matches!(outcome, HealOutcome::NotCached), "healed a rewritten source from the old one's output: {outcome:?}");
    assert!(!staged.exists(), "the previous version's variant was linked into staging");
}

// ----- a source still in the cloud is never read to learn its hash -----

/// The heal runs over every item of a batch, including the ones the worker
/// deferred to the cloud. A provider re-materialising a source changes its ctime
/// and inode, so the index no longer vouches for it and `HashOnMiss` would hash
/// it — a read that can block for the whole download or fail. Not reading is
/// `NotCached`, whatever the store holds under the source's oid.
#[test]
fn rematerialize_never_reads_a_source_that_is_still_in_the_cloud() {
    let h = harness();
    let src = h._tmp.path().join("photo.jpg");
    make_big_jpeg(&src, 400, 300);
    let cfg = ImageCompressionConfig::default();
    let source_oid = crate::build::cache::ObjectStore::hash_file(&src).unwrap();
    let outcome = convert_single_image(
        &src, &source_oid, "photo.webp", &h.temp, &h.staging, &h.objects, &h.transforms, &cfg, None, None,
        &HashMap::new(),
    );
    assert!(outcome.error.is_none(), "encode failed: {:?}", outcome.error);
    let staged = h.staging.join("photo.webp");

    // The index knows the file — as it stood before the provider touched it.
    let mut index = crate::build::cache::HashIndex::new();
    index.update(
        "photo.jpg".to_string(),
        &crate::build::cache::FileStat { ctime: Some(1), inode: Some(1), ..crate::build::cache::FileStat::of(&fs::metadata(&src).unwrap()) },
        source_oid,
    );

    fs::remove_file(&staged).unwrap();
    let cloud = crate::build::icloud::pretend::evicted(&src);
    let outcome = rematerialize(
        &h.objects, &h.transforms, &cfg.to_params(), &mut index, &src, "photo.jpg", &staged, "image/webp",
        HashPolicy::HashOnMiss,
    );
    assert!(matches!(outcome, HealOutcome::NotCached), "read a source that is in the cloud: {outcome:?}");
    assert!(!staged.exists(), "linked an output for a source it could not identify");

    // The same heal once the bytes are local: it hashes, and the store answers.
    drop(cloud);
    let outcome = rematerialize(
        &h.objects, &h.transforms, &cfg.to_params(), &mut index, &src, "photo.jpg", &staged, "image/webp",
        HashPolicy::HashOnMiss,
    );
    assert!(matches!(outcome, HealOutcome::Healed), "a local source must still heal: {outcome:?}");
}
