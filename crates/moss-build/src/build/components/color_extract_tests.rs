use super::*;

#[test]
fn test_rgb_to_hsl_red() {
    let (h, s, l) = rgb_to_hsl(255, 0, 0);
    assert!((h - 0.0).abs() < 1.0);
    assert!((s - 1.0).abs() < 0.01);
    assert!((l - 0.5).abs() < 0.01);
}

#[test]
fn test_rgb_to_hsl_green() {
    let (h, s, l) = rgb_to_hsl(0, 255, 0);
    assert!((h - 120.0).abs() < 1.0);
    assert!((s - 1.0).abs() < 0.01);
    assert!((l - 0.5).abs() < 0.01);
}

#[test]
fn test_rgb_to_hsl_blue() {
    let (h, s, l) = rgb_to_hsl(0, 0, 255);
    assert!((h - 240.0).abs() < 1.0);
    assert!((s - 1.0).abs() < 0.01);
    assert!((l - 0.5).abs() < 0.01);
}

#[test]
fn test_rgb_to_hsl_gray() {
    let (h, s, l) = rgb_to_hsl(128, 128, 128);
    let _ = h;
    assert!((s - 0.0).abs() < 0.01);
    assert!((l - 0.5).abs() < 0.05);
}

#[test]
fn test_rgb_to_hsl_white() {
    let (_, s, l) = rgb_to_hsl(255, 255, 255);
    assert!((s - 0.0).abs() < 0.01);
    assert!((l - 1.0).abs() < 0.01);
}

#[test]
fn test_rgb_to_hsl_black() {
    let (_, s, l) = rgb_to_hsl(0, 0, 0);
    assert!((s - 0.0).abs() < 0.01);
    assert!((l - 0.0).abs() < 0.01);
}

// ── WCAG contrast tests ──────────────────────────────────

#[test]
fn white_on_black_max_contrast() {
    let ratio = contrast_ratio(1.0, 0.0);
    assert!((ratio - 21.0).abs() < 0.1);
}

#[test]
fn identical_colors_min_contrast() {
    let ratio = contrast_ratio(0.5, 0.5);
    assert!((ratio - 1.0).abs() < 0.01);
}

#[test]
fn darken_already_dark_is_noop() {
    // Pure blue at L=20% is already very dark
    let l = darken_for_contrast(240.0, 1.0, 0.20);
    assert!((l - 0.20).abs() < 0.02, "Should stay near 0.20, got {}", l);
}

#[test]
fn darken_bright_yellow_reduces_lightness() {
    // Yellow (H=60) at L=70% is very light — must darken significantly
    let l = darken_for_contrast(60.0, 1.0, 0.70);
    assert!(
        l < 0.40,
        "Bright yellow should darken below 40%, got {}%",
        l * 100.0
    );

    // Verify the result actually passes WCAG AA
    let (r, g, b) = hsl_to_rgb(60.0, 1.0, l);
    let lum = relative_luminance(r, g, b);
    let ratio = contrast_ratio(1.0, lum);
    assert!(ratio >= 4.5, "Should pass AA, got ratio {}", ratio);
}

#[test]
fn darken_preserves_highest_viable_lightness() {
    // Mid-blue at L=50% — should darken but not to black
    let l = darken_for_contrast(220.0, 0.8, 0.50);
    assert!(
        l > 0.10,
        "Should not darken to near-black, got {}%",
        l * 100.0
    );
    assert!(l <= 0.50, "Should darken from original 50%");

    let (r, g, b) = hsl_to_rgb(220.0, 0.8, l);
    let lum = relative_luminance(r, g, b);
    let ratio = contrast_ratio(1.0, lum);
    assert!(ratio >= 4.5, "Should pass AA, got ratio {}", ratio);
}

#[test]
fn hsl_to_rgb_roundtrip() {
    // Red: HSL(0, 1, 0.5) → RGB(1, 0, 0)
    let (r, g, b) = hsl_to_rgb(0.0, 1.0, 0.5);
    assert!((r - 1.0).abs() < 0.01);
    assert!(g.abs() < 0.01);
    assert!(b.abs() < 0.01);
}

#[test]
fn relative_luminance_white() {
    let lum = relative_luminance(1.0, 1.0, 1.0);
    assert!((lum - 1.0).abs() < 0.01);
}

#[test]
fn relative_luminance_black() {
    let lum = relative_luminance(0.0, 0.0, 0.0);
    assert!(lum.abs() < 0.01);
}

// ── resolve_color_source_path ──────────────────────────────────
//
// Regression coverage for the asymmetry bug where one folder-card
// renderer handled video covers (rewriting `.mp4` → `.thumb.jpg`
// before reading) while the other passed the `.mp4` path straight
// to `image::open()`, which silently returned `None`. Both
// renderers now route through this helper; these tests pin the
// resolution rules so a future copy-paste cannot recreate the
// divergence.

use std::path::PathBuf;

fn paths(source: &str, output: &str) -> (PathBuf, PathBuf) {
    (PathBuf::from(source), PathBuf::from(output))
}

#[test]
fn resolve_video_cover_uses_thumbnail_in_output_root() {
    let (src, out) = paths("/site", "/site/.moss/build/current");
    let resolved = resolve_color_source_path("音乐/cover.mp4", &src, &out)
        .expect("video cover should resolve");
    assert_eq!(
        resolved,
        PathBuf::from("/site/.moss/build/current/音乐/cover.thumb.jpg")
    );
}

#[test]
fn resolve_video_cover_handles_uppercase_extensions() {
    // iPhone .MOV, GoPro .MP4 — case must not matter.
    let (src, out) = paths("/site", "/site/.moss/build/current");
    for cover in ["clip.MOV", "clip.MP4", "clip.Mp4", "clip.WEBM"] {
        let resolved = resolve_color_source_path(cover, &src, &out)
            .unwrap_or_else(|| panic!("uppercase {cover} should resolve"));
        assert_eq!(
            resolved,
            PathBuf::from("/site/.moss/build/current/clip.thumb.jpg"),
            "{cover} should map to .thumb.jpg",
        );
    }
}

#[test]
fn resolve_video_cover_strips_leading_slash() {
    // Cover URLs from frontmatter often start with "/" (root-relative).
    let (src, out) = paths("/site", "/site/.moss/build/current");
    let resolved = resolve_color_source_path("/videos/clip.mov", &src, &out)
        .expect("leading-slash video should resolve");
    assert_eq!(
        resolved,
        PathBuf::from("/site/.moss/build/current/videos/clip.thumb.jpg")
    );
}

#[test]
fn resolve_image_cover_prefers_source_root_when_present() {
    // Use a real temp directory so .exists() returns true for the source path.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    std::fs::create_dir_all(src.join("assets")).unwrap();
    std::fs::write(src.join("assets/cover.jpg"), b"fake").unwrap();
    let out = src.join(".moss/build/current");

    let resolved = resolve_color_source_path("assets/cover.jpg", src, &out)
        .expect("image cover should resolve");
    assert_eq!(resolved, src.join("assets/cover.jpg"));
}

#[test]
fn resolve_image_cover_falls_back_to_output_when_source_missing() {
    // When the source file does not exist (e.g. cover written by a
    // build hook into output only), fall back to the output root.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    let out = src.join(".moss/build/current");

    let resolved = resolve_color_source_path("assets/missing.jpg", src, &out)
        .expect("image cover should resolve to fallback");
    assert_eq!(resolved, out.join("assets/missing.jpg"));
}

#[test]
fn resolve_external_url_returns_none() {
    let (src, out) = paths("/site", "/site/.moss/build/current");
    assert_eq!(
        resolve_color_source_path("https://example.com/cover.jpg", &src, &out),
        None,
    );
    assert_eq!(
        resolve_color_source_path("http://example.com/cover.jpg", &src, &out),
        None,
    );
}

#[test]
fn resolve_cover_decodes_a_percent_encoded_url_back_to_the_on_disk_name() {
    // `PathResolver::resolve_url` percent-encodes every segment, so a CJK
    // cover arrives here as escape sequences. Joining those onto the source
    // root verbatim looks for a file literally named `%E5%B0%81…` and misses,
    // silently dropping the card's dominant color.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    std::fs::create_dir_all(src.join("獎項")).unwrap();
    std::fs::write(src.join("獎項/封面.jpg"), b"fake").unwrap();
    let out = src.join(".moss/build/current");

    let resolved =
        resolve_color_source_path("/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.jpg", src, &out)
            .expect("encoded cover should resolve");
    assert_eq!(resolved, src.join("獎項/封面.jpg"));
}

#[test]
fn resolve_cover_decode_leaves_a_literal_percent_filename_alone() {
    // A lone `%` is a legal filename byte, not a truncated escape.
    let (src, out) = paths("/site", "/site/.moss/build/current");
    let resolved =
        resolve_color_source_path("assets/100%.jpg", &src, &out).expect("should resolve");
    assert_eq!(resolved, out.join("assets/100%.jpg"));
}

// ── prepare_cover_color ──────────────────────────────
//
// Normalizes scan-cached colors for use as folder-card backgrounds.
// Tests pin the format-handling rules so the two storage formats
// (raw hex from image scan, darkened HSL from video scan) both
// produce a card-ready color.

#[test]
fn prepare_hex_image_color_darkens_for_wcag() {
    // Bright yellow #FFFF00 — un-darkened it has 1.07:1 contrast vs
    // white (unreadable). Must be darkened.
    let color = prepare_cover_color("#FFFF00").expect("hex must produce color");
    assert!(
        color.starts_with("hsla(60,"),
        "expected yellow hue, got {color}"
    );

    // Verify the result actually passes WCAG AA against white. Parse
    // back the HSL, get RGB, compute contrast.
    let inner = color
        .strip_prefix("hsla(")
        .and_then(|s| s.strip_suffix(", 1)"))
        .unwrap();
    let parts: Vec<&str> = inner.split(", ").collect();
    let h: f32 = parts[0].parse().unwrap();
    let s: f32 = parts[1].trim_end_matches('%').parse::<f32>().unwrap() / 100.0;
    let l: f32 = parts[2].trim_end_matches('%').parse::<f32>().unwrap() / 100.0;
    let (r, g, b) = hsl_to_rgb(h, s, l);
    assert!(contrast_ratio(1.0, relative_luminance(r, g, b)) >= 4.5);
}

#[test]
fn prepare_hex_short_form_works() {
    // 3-digit hex from CSS shorthand like `#0F0`.
    let color = prepare_cover_color("#0F0").expect("short hex");
    // Pure green hue.
    assert!(
        color.starts_with("hsla(120,"),
        "expected green hue, got {color}"
    );
}

#[test]
fn prepare_hsla_video_color_passes_through_unchanged() {
    // The video scan path stores already-darkened HSL. Passing it
    // through `prepare_cover_color` again must NOT re-darken.
    let already_darkened = "hsla(240, 100%, 25%, 1)";
    assert_eq!(
        prepare_cover_color(already_darkened).as_deref(),
        Some(already_darkened),
    );
}

#[test]
fn prepare_hsl_without_alpha_also_passes_through() {
    // Defensive: scan format may evolve. `hsl(...)` is also a valid
    // CSS color and should pass through as-is.
    let color = "hsl(120, 50%, 30%)";
    assert_eq!(prepare_cover_color(color).as_deref(), Some(color));
}

#[test]
fn prepare_invalid_input_returns_none() {
    // Empty, garbage, hex of wrong length all return None — caller
    // falls back to runtime extraction.
    assert_eq!(prepare_cover_color(""), None);
    assert_eq!(prepare_cover_color("   "), None);
    assert_eq!(prepare_cover_color("not a color"), None);
    assert_eq!(prepare_cover_color("#XYZXYZ"), None);
    assert_eq!(prepare_cover_color("#12345"), None); // wrong length
}

#[test]
fn prepare_hex_with_whitespace_trims() {
    // Defensive against trailing newlines / leading whitespace from
    // serde.
    let color = prepare_cover_color("  #336699  \n").expect("trimmed hex");
    assert!(
        color.starts_with("hsla(210,"),
        "expected blue hue, got {color}"
    );
}

#[test]
fn cache_path_and_runtime_extraction_produce_equivalent_colors() {
    // Structural pin: the cached `MediaMetadata.dominant_color` for
    // an image (raw `#RRGGBB` average) and the runtime
    // `extract_dominant_color` on the same image must yield the
    // same WCAG-darkened HSL after passing the cached value through
    // `prepare_cover_color`. If a future refactor changes
    // the averaging algorithm in one path but not the other, this
    // test catches the de-sync.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cover.jpg");

    // 4×4 solid red. Both paths average → pure red `#FF0000`.
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([255u8, 0, 0]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();

    // Runtime path: read the JPEG and darken in one shot.
    let runtime_color = extract_dominant_color(&path).expect("runtime extraction must succeed");

    // Cache path: read the same JPEG, store as raw hex (the scan
    // format), then normalize via prepare_cover_color.
    let cached_hex = {
        let img = image::open(&path).unwrap();
        let thumb = img.thumbnail(100, 100).to_rgb8();
        let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
        for px in thumb.pixels() {
            r += px[0] as u64;
            g += px[1] as u64;
            b += px[2] as u64;
            n += 1;
        }
        format!(
            "#{:02X}{:02X}{:02X}",
            (r / n) as u8,
            (g / n) as u8,
            (b / n) as u8
        )
    };
    let cache_color = prepare_cover_color(&cached_hex).expect("cached hex must normalize");

    assert_eq!(
        runtime_color, cache_color,
        "cache path and runtime path must agree on the same image",
    );
}

#[test]
fn video_cover_resolves_and_extracts_end_to_end() {
    // The strongest regression guard: bypass the asymmetry by going
    // through the helper, then prove the resolved path actually
    // produces a non-None color. If a future refactor changes the
    // thumbnail naming convention, output-tree layout, or video
    // detection, this test fails — the unit tests on the helper
    // alone wouldn't catch a renaming of `.thumb.jpg`.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    let out_videos = src.join(".moss/build/current/videos");
    std::fs::create_dir_all(&out_videos).unwrap();

    // Synthesize a 4×4 solid-blue JPEG at the conventional .thumb.jpg
    // location. extract_dominant_color resizes to 50×50 (pure
    // upscale), averages, darkens for WCAG — the result must be a
    // blue HSL string.
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([0u8, 0, 255]));
    let thumb_path = src.join(".moss/build/current/videos/clip.thumb.jpg");
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(&thumb_path, image::ImageFormat::Jpeg)
        .unwrap();

    let resolved =
        resolve_color_source_path("/videos/clip.mp4", src, &src.join(".moss/build/current"))
            .expect("video cover should resolve");
    assert_eq!(resolved, thumb_path);

    let color = extract_dominant_color(&resolved)
        .expect("resolved thumb must yield a color, not silent None");
    // Pure blue darkened for WCAG against white text.
    assert!(
        color.starts_with("hsla(240,")
            || color.starts_with("hsla(239,")
            || color.starts_with("hsla(241,"),
        "expected blue hue, got {color}",
    );
}

#[test]
fn resolve_video_cover_never_falls_back_to_source_root() {
    // Even if a `.mp4` file exists at the source root, we still resolve
    // to the output `.thumb.jpg`: the source video can't be decoded by
    // the `image` crate. Test with a temp source containing a real .mp4.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path();
    std::fs::create_dir_all(src.join("音乐")).unwrap();
    std::fs::write(src.join("音乐/cover.mp4"), b"fake mp4 bytes").unwrap();
    let out = src.join(".moss/build/current");

    let resolved =
        resolve_color_source_path("音乐/cover.mp4", src, &out).expect("video should resolve");
    assert_eq!(
        resolved,
        out.join("音乐/cover.thumb.jpg"),
        "video must never resolve to the source .mp4",
    );
}

// ── prepare_cover_color_muted ─────────────────────────────────────
//
// Hero mobile background with 0.8x desaturation for atmospheric feel.
// Same WCAG AA floor as prepare_cover_color; differs only in
// the saturation multiplier before darkening.

#[test]
fn prepare_cover_color_muted_from_hex_produces_hsla() {
    // Bright cyan #00FFFF is a vivid, saturated color. The function
    // should desaturate it (0.8x) and darken it to WCAG AA, returning
    // an hsla() string with the cyan hue preserved.
    let color = prepare_cover_color_muted("#00FFFF").expect("cyan must produce color");
    assert!(
        color.starts_with("hsla(180,"),
        "expected cyan hue, got {color}"
    );
    let inner = color
        .strip_prefix("hsla(")
        .and_then(|s| s.strip_suffix(", 1)"))
        .unwrap();
    let parts: Vec<&str> = inner.split(", ").collect();
    assert_eq!(parts.len(), 3, "should have 3 hsla components");
    let h: f32 = parts[0].parse().unwrap();
    let s: f32 = parts[1].trim_end_matches('%').parse::<f32>().unwrap();
    let l: f32 = parts[2].trim_end_matches('%').parse::<f32>().unwrap();
    assert!(
        (h - 180.0).abs() < 1.0,
        "hue should be ~180° (cyan), got {h}"
    );
    assert!(
        s > 0.0 && s < 100.0,
        "saturation should be between 0-100%, got {s}%"
    );
    assert!(
        l > 0.0 && l < 100.0,
        "lightness should be between 0-100%, got {l}%"
    );
}

#[test]
fn prepare_cover_color_muted_desaturates_relative_to_card() {
    let card = prepare_cover_color("#FF4400").expect("card color");
    let hero = prepare_cover_color_muted("#FF4400").expect("hero color");
    fn saturation(s: &str) -> f32 {
        let inner = s
            .strip_prefix("hsla(")
            .and_then(|s| s.strip_suffix(", 1)"))
            .unwrap();
        let parts: Vec<&str> = inner.split(", ").collect();
        parts[1].trim_end_matches('%').parse::<f32>().unwrap()
    }
    assert!(
        saturation(&hero) <= saturation(&card),
        "hero bg ({hero}) should be <= saturated vs card ({card})"
    );
}

#[test]
fn prepare_cover_color_muted_hsla_passthrough() {
    let already = "hsla(210, 45%, 20%, 1)";
    assert_eq!(prepare_cover_color_muted(already).as_deref(), Some(already));
}

#[test]
fn prepare_cover_color_muted_empty_returns_none() {
    assert_eq!(prepare_cover_color_muted(""), None);
    assert_eq!(prepare_cover_color_muted("not a color"), None);
}

#[test]
fn prepare_cover_color_muted_rgb_passthrough() {
    let color = "rgb(0, 100, 200)";
    assert_eq!(prepare_cover_color_muted(color).as_deref(), Some(color));
}

// ── parse_css_color ─────────────────────────────────────────────
//
// CSS color-literal parser feeding the cover-color ladder. A miss is
// always benign (caller falls through to the next signal), so the
// named-color table deliberately covers only the basic-16 + common
// extended names.

#[test]
fn parse_css_color_hex_forms() {
    assert_eq!(parse_css_color("#336699"), Some((0x33, 0x66, 0x99)));
    assert_eq!(parse_css_color("#FFF"), Some((255, 255, 255)));
    assert_eq!(parse_css_color("  #000  "), Some((0, 0, 0)));
}

#[test]
fn parse_css_color_rgb_forms() {
    assert_eq!(parse_css_color("rgb(10,20,30)"), Some((10, 20, 30)));
    assert_eq!(parse_css_color("rgb(10, 20, 30)"), Some((10, 20, 30)));
    assert_eq!(parse_css_color("rgba(10, 20, 30, 0.5)"), Some((10, 20, 30)));
    assert_eq!(parse_css_color("rgb(0 128 255 / 50%)"), Some((0, 128, 255)));
    assert_eq!(parse_css_color("rgb(100%, 0%, 50%)"), Some((255, 0, 128)));
}

#[test]
fn parse_css_color_hsl_forms() {
    // hsl(120, 100%, 50%) = pure green
    assert_eq!(parse_css_color("hsl(120, 100%, 50%)"), Some((0, 255, 0)));
    assert_eq!(parse_css_color("hsla(0, 100%, 50%, 1)"), Some((255, 0, 0)));
    assert_eq!(parse_css_color("hsl(240deg, 100%, 50%)"), Some((0, 0, 255)));
}

#[test]
fn parse_css_color_named() {
    assert_eq!(parse_css_color("black"), Some((0, 0, 0)));
    assert_eq!(parse_css_color("White"), Some((255, 255, 255)));
    assert_eq!(parse_css_color("rebeccapurple"), Some((0x66, 0x33, 0x99)));
    assert_eq!(parse_css_color("midnightblue"), Some((0x19, 0x19, 0x70)));
}

#[test]
fn parse_css_color_rejects_non_colors() {
    assert_eq!(parse_css_color("transparent"), None); // deliberately absent
    assert_eq!(parse_css_color("var(--bg)"), None);
    assert_eq!(parse_css_color("url(bg.png)"), None);
    assert_eq!(parse_css_color("linear-gradient(red, blue)"), None);
    assert_eq!(parse_css_color("notacolor"), None);
    assert_eq!(parse_css_color(""), None);
    assert_eq!(parse_css_color("#12345"), None); // wrong hex length
}

// ── scan_webpage_color / extract_webpage_color ──────────────────
//
// Rungs 2–3 of the cover-color ladder: theme-color meta preferred,
// else a tightly scoped static html/body background scan. Pure
// static parsing — the page is NEVER rendered (spec: rejected
// approaches). Precedence pins below are the spec's exact rules.

/// Repo-internal temp dir (test artifacts must live inside the repo,
/// not $HOME or bare /tmp). Self-creates target/test-tmp.
fn repo_tmp() -> tempfile::TempDir {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp");
    std::fs::create_dir_all(base).unwrap();
    tempfile::Builder::new().tempdir_in(base).unwrap()
}

#[test]
fn webpage_theme_color_meta_wins() {
    let html = r##"<html><head>
            <meta name="theme-color" content="#112233">
            <style>body { background: #aabbcc; }</style>
        </head><body></body></html>"##;
    assert_eq!(scan_webpage_color(html), Some((0x11, 0x22, 0x33)));
}

#[test]
fn webpage_media_less_theme_color_preferred() {
    // moss-built pages emit a light/dark media'd pair; a media-less
    // meta (when present) is the deterministic pick.
    let html = r##"<html><head>
            <meta name="theme-color" content="#111111" media="(prefers-color-scheme: light)">
            <meta name="theme-color" content="#222222">
        </head><body></body></html>"##;
    assert_eq!(scan_webpage_color(html), Some((0x22, 0x22, 0x22)));
}

#[test]
fn webpage_first_theme_color_when_all_have_media() {
    let html = r##"<html><head>
            <meta name="theme-color" content="#111111" media="(prefers-color-scheme: light)">
            <meta name="theme-color" content="#222222" media="(prefers-color-scheme: dark)">
        </head><body></body></html>"##;
    assert_eq!(scan_webpage_color(html), Some((0x11, 0x11, 0x11)));
}

#[test]
fn webpage_unparseable_theme_color_falls_to_background() {
    let html = r##"<html><head>
            <meta name="theme-color" content="var(--brand)">
            <style>body { background-color: #334455; }</style>
        </head><body></body></html>"##;
    assert_eq!(scan_webpage_color(html), Some((0x33, 0x44, 0x55)));
}

#[test]
fn webpage_body_beats_html_and_style_attr_beats_style_block() {
    let html = r##"<html style="background: #010101"><head><style>
            html { background: #020202; }
            body { background: #030303; }
        </style></head><body style="background-color: #040404"></body></html>"##;
    // body style attr > body <style> rule > html style attr > html rule
    assert_eq!(scan_webpage_color(html), Some((0x04, 0x04, 0x04)));
}

#[test]
fn webpage_last_style_declaration_wins() {
    let html = r##"<html><head><style>
            body { background: #111111; }
            html, body { background-color: #222222; }
        </style></head><body></body></html>"##;
    assert_eq!(scan_webpage_color(html), Some((0x22, 0x22, 0x22)));
}

#[test]
fn webpage_skips_complex_selectors_and_values() {
    let html = r##"<html><head><style>
            body.dark { background: #111111; }
            body { background: linear-gradient(#000, #fff); }
            body { background: url(bg.png); }
            body { background: transparent; }
        </style></head><body></body></html>"##;
    // .dark-scoped rule out of scope; gradient/url/transparent are not
    // single color literals → no signal at all.
    assert_eq!(scan_webpage_color(html), None);
}

#[test]
fn webpage_no_signals_returns_none() {
    // Mirrors the motivating cover: a p5.js page whose color lives in
    // JS (background(0) in the sketch) — statically invisible.
    let html = r#"<html><head><script src="p5.js"></script>
            <style>html, body { padding: 0; margin: 0; overflow: hidden; }</style>
        </head><body></body></html>"#;
    assert_eq!(scan_webpage_color(html), None);
}

#[test]
fn extract_webpage_color_darkens_for_wcag() {
    // A white page must yield a band passing ≥ 4.5:1 against white
    // text — webpage colors are author-side values, never pre-darkened.
    let tmp = repo_tmp();
    let page = tmp.path().join("cover.html");
    std::fs::write(
        &page,
        r##"<html><head>
            <meta name="theme-color" content="#ffffff">
        </head><body></body></html>"##,
    )
    .unwrap();
    let color = extract_webpage_color(&page).expect("white page must yield a color");
    let inner = color
        .strip_prefix("hsla(")
        .and_then(|s| s.strip_suffix(", 1)"))
        .unwrap();
    let parts: Vec<&str> = inner.split(", ").collect();
    let h: f32 = parts[0].parse().unwrap();
    let s: f32 = parts[1].trim_end_matches('%').parse::<f32>().unwrap() / 100.0;
    let l: f32 = parts[2].trim_end_matches('%').parse::<f32>().unwrap() / 100.0;
    let (r, g, b) = hsl_to_rgb(h, s, l);
    assert!(contrast_ratio(1.0, relative_luminance(r, g, b)) >= 4.5);
}

#[test]
fn extract_webpage_color_missing_file_returns_none() {
    let tmp = repo_tmp();
    assert_eq!(extract_webpage_color(&tmp.path().join("absent.html")), None);
}

// ── resolve_card_color ──────────────────────────────────────────
//
// The shared ladder entry point BOTH folder-card renderers
// (grid_cells::apply_collection_cards, folder_embed) must call — same
// anti-divergence rule as resolve_color_source_path above.
// CoverType re-exported via `use super::*` (module-level import).

#[test]
fn card_color_override_wins_for_iframe() {
    let color = resolve_card_color(
        Some("widget.html|color=#336699"),
        Some(CoverType::Iframe),
        None,
        None,
    );
    assert!(
        color.as_deref().is_some_and(|c| c.starts_with("hsla(210,")),
        "override hue must win, got {color:?}"
    );
}

#[test]
fn card_color_override_wins_for_image_too() {
    // The override is uniform across cover types — one obvious way.
    let color = resolve_card_color(
        Some("photo.jpg|color=black"),
        Some(CoverType::Image),
        None,
        None,
    );
    assert_eq!(color.as_deref(), Some("hsla(0, 0%, 0%, 1)"));
}

#[test]
fn card_color_bad_override_falls_through_to_iframe_default() {
    let color = resolve_card_color(
        Some("widget.html|color=notacolor"),
        Some(CoverType::Iframe),
        None,
        None,
    );
    assert_eq!(color.as_deref(), Some(IFRAME_COVER_FALLBACK));
}

#[test]
fn card_color_iframe_reads_theme_color_from_source_root() {
    let tmp = repo_tmp();
    std::fs::write(
        tmp.path().join("cover.html"),
        r##"<html><head><meta name="theme-color" content="#00FFFF"></head><body></body></html>"##,
    )
    .unwrap();
    let color = resolve_card_color(
        Some("cover.html"),
        Some(CoverType::Iframe),
        Some(tmp.path()),
        None,
    );
    assert!(
        color.as_deref().is_some_and(|c| c.starts_with("hsla(180,")),
        "expected cyan hue from theme-color, got {color:?}"
    );
}

#[test]
fn card_color_iframe_without_signals_gets_dark_default() {
    let tmp = repo_tmp();
    std::fs::write(
        tmp.path().join("sketch.html"),
        // Mirrors the motivating p5.js cover: color only in JS.
        r#"<html><head><script src="p5.js"></script></head><body></body></html>"#,
    )
    .unwrap();
    let color = resolve_card_color(
        Some("sketch.html"),
        Some(CoverType::Iframe),
        Some(tmp.path()),
        None,
    );
    assert_eq!(color.as_deref(), Some(IFRAME_COVER_FALLBACK));
}

#[test]
fn card_color_iframe_missing_file_gets_dark_default() {
    let tmp = repo_tmp();
    let color = resolve_card_color(
        Some("absent.html"),
        Some(CoverType::Iframe),
        Some(tmp.path()),
        None,
    );
    assert_eq!(color.as_deref(), Some(IFRAME_COVER_FALLBACK));
}

#[test]
fn card_color_iframe_external_url_gets_dark_default() {
    // External pages are never fetched (resolve_color_source_path
    // rule 1) — the dark default still applies.
    let color = resolve_card_color(
        Some("https://example.com/widget.html"),
        Some(CoverType::Iframe),
        None,
        None,
    );
    assert_eq!(color.as_deref(), Some(IFRAME_COVER_FALLBACK));
}

#[test]
fn card_color_image_runtime_extraction_preserved() {
    // Existing image behavior must survive the collapse onto the
    // shared helper: no cache entry + root present → decode + darken.
    let tmp = repo_tmp();
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([255u8, 0, 0]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(tmp.path().join("cover.jpg"), image::ImageFormat::Jpeg)
        .unwrap();
    let color = resolve_card_color(
        Some("cover.jpg"),
        Some(CoverType::Image),
        Some(tmp.path()),
        None,
    );
    assert!(
        color
            .as_deref()
            .is_some_and(|c| c.starts_with("hsla(0,") || c.starts_with("hsla(359,")),
        "expected red hue from runtime extraction, got {color:?}"
    );
}

#[test]
fn card_color_image_without_root_stays_none() {
    // Pre-existing behavior pin: image covers with no lookup hit and
    // no root resolve to None (light default), NOT the iframe fallback.
    let color = resolve_card_color(Some("photo.jpg"), Some(CoverType::Image), None, None);
    assert_eq!(color, None);
}

#[test]
fn card_color_no_cover_is_none() {
    assert_eq!(resolve_card_color(None, None, None, None), None);
}

#[test]
fn card_color_video_resolves_thumbnail_through_ladder() {
    // CoverType::Video must route through resolve_color_source_path's
    // .mp4 → .thumb.jpg rewrite from the shared entry point too —
    // same pin as video_cover_resolves_and_extracts_end_to_end, but
    // through resolve_card_color.
    let tmp = repo_tmp();
    let out_dir = tmp.path().join(".moss/build/current/videos");
    std::fs::create_dir_all(&out_dir).unwrap();
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([0u8, 0, 255]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(out_dir.join("clip.thumb.jpg"), image::ImageFormat::Jpeg)
        .unwrap();
    let color = resolve_card_color(
        Some("videos/clip.mp4"),
        Some(CoverType::Video),
        Some(tmp.path()),
        None,
    );
    assert!(
        color.as_deref().is_some_and(|c| {
            c.starts_with("hsla(240,") || c.starts_with("hsla(239,") || c.starts_with("hsla(241,")
        }),
        "expected blue hue from video thumbnail, got {color:?}"
    );
}

/// The placeholder half of `card_color_video_resolves_thumbnail_through_ladder`:
/// a page rendered BEFORE the background video phase writes `.thumb.jpg` gets
/// no color (the video branch has no source-file fallback — see
/// `resolve_color_source_path`'s doc), and — this is the fix this test backs
/// — a SECOND call with nothing else changed except the file landing on disk
/// resolves it correctly. `moss-build/src/build.rs`'s
/// `trigger_media_settle_rerender` leans on exactly this: it doesn't patch
/// the placeholder in place, it schedules the SAME render to run again once
/// the poster settles, and this is the proof that a plain re-render is
/// sufficient — no second mechanism is needed.
#[test]
fn card_color_video_self_heals_once_the_poster_lands() {
    let tmp = repo_tmp();
    let out_dir = tmp.path().join(".moss/build/current/videos");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Before: background video conversion hasn't produced the thumbnail yet —
    // the exact state a page renders in while the poster is still converting.
    let before = resolve_card_color(
        Some("videos/clip.mp4"),
        Some(CoverType::Video),
        Some(tmp.path()),
        None,
    );
    assert_eq!(
        before, None,
        "no color before the poster exists — this is the bare placeholder a \
         page ships while the poster is still converting"
    );

    // The background phase lands the poster — same path, same bytes shape as
    // `video_cover_resolves_and_extracts_end_to_end`.
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([0u8, 0, 255]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(out_dir.join("clip.thumb.jpg"), image::ImageFormat::Jpeg)
        .unwrap();

    // After: a render that runs AFTER the poster landed — nothing else
    // changed — now resolves the color. No cache entry, no special-cased
    // "second call" behavior: it is the same function reading the same path,
    // which is what makes triggering a plain re-render the correct fix.
    let after = resolve_card_color(
        Some("videos/clip.mp4"),
        Some(CoverType::Video),
        Some(tmp.path()),
        None,
    );
    assert!(
        after.as_deref().is_some_and(|c| {
            c.starts_with("hsla(240,") || c.starts_with("hsla(239,") || c.starts_with("hsla(241,")
        }),
        "expected blue hue once the poster exists, got {after:?}"
    );
}

/// The image-cover counterpart: a page rendered while the source is
/// dataless/not-yet-materialized (the source path does not exist yet) bakes
/// no color, and a render that runs after the source lands resolves it — no
/// cache entry, same function, same path. `resolve_color_source_path` falls
/// back to `output_root` when the source is absent (not the video-only "never
/// falls back to source" rule), so the "before" state here has NEITHER file
/// present, matching a genuinely unreadable source rather than a warm cache.
/// This is the premise `trigger_media_settle_rerender`'s image/newly-added
/// branch (build.rs) leans on for the brand-new-image case.
#[test]
fn card_color_image_self_heals_once_the_source_materializes() {
    let tmp = repo_tmp();
    let source_dir = tmp.path().join("assets");
    std::fs::create_dir_all(&source_dir).unwrap();
    // Before: neither the source nor any output exists — the dataless state.

    let before = resolve_card_color(
        Some("assets/cover.jpg"),
        Some(CoverType::Image),
        Some(tmp.path()),
        None,
    );
    assert_eq!(
        before, None,
        "no color before the source is readable — the bare placeholder a \
         page ships while iCloud hasn't materialized the file yet"
    );

    // iCloud (or any other cloud provider) materializes the real bytes.
    let img = image::ImageBuffer::from_fn(4, 4, |_, _| image::Rgb([255u8, 0, 0]));
    image::DynamicImage::ImageRgb8(img)
        .save_with_format(source_dir.join("cover.jpg"), image::ImageFormat::Jpeg)
        .unwrap();

    let after = resolve_card_color(
        Some("assets/cover.jpg"),
        Some(CoverType::Image),
        Some(tmp.path()),
        None,
    );
    assert!(
        after.as_deref().is_some_and(|c| {
            c.starts_with("hsla(0,") || c.starts_with("hsla(359,") || c.starts_with("hsla(1,")
        }),
        "expected red hue once the source is readable, got {after:?}"
    );
}
