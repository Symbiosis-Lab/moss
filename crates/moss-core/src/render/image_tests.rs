use super::*;
use crate::asset_snapshot::VariantKindSet;

/// Build an AssetSnapshot with a single entry — the per-test fixture
/// shape after the Phase 1 B1 migration (2026-05-25). Replaces the prior
/// `MediaDimensionLookup`-based `lookup(vec![img_meta(...)])` builder.
/// The snapshot keys are `PathBuf` per `AssetSnapshot`'s contract; the
/// stem-derived `variants` entry mirrors what
/// `AssetRegistry::iter_registered_variants` would populate when a WebP
/// variant is registered.
fn snapshot_with(
    path: &str,
    dims: Option<(u32, u32)>,
    color: Option<&str>,
    lqip: Option<&str>,
    webp: bool,
) -> AssetSnapshot {
    let mut s = AssetSnapshot::new();
    let key = PathBuf::from(path);
    if let Some(d) = dims {
        s.dimensions.insert(key.clone(), d);
    }
    if let Some(c) = color {
        s.dominant_color.insert(key.clone(), c.to_string());
    }
    if let Some(l) = lqip {
        s.lqip.insert(key.clone(), l.to_string());
    }
    if webp {
        let stem = crate::asset_snapshot::path_strip_extension(&key);
        s.variants.insert(
            stem,
            VariantKindSet { webp: true, avif: false, hls: false },
        );
    }
    s
}

fn snapshot_dims(path: &str, w: u32, h: u32) -> AssetSnapshot {
    snapshot_with(path, Some((w, h)), None, None, false)
}

// --- BUG 6: output-URL-form lookups must hit real dims, not 800x600 ---

/// A body/wikilink image arrives percent-encoded (`Europe%20-%20A%20Prophecy`).
/// `probe_paths` must percent-decode and reverse `../` so it hits the RAW
/// source dims key, instead of missing → 800x600 fallback.
#[test]
fn probe_paths_percent_decodes_src() {
    let snap = snapshot_dims("assets/Europe - A Prophecy/e-006.jpg", 4515, 6158);
    assert_eq!(
        lookup_dims(&snap, "../../assets/Europe%20-%20A%20Prophecy/e-006.jpg"),
        Some((4515, 6158))
    );
}

/// A cover arrives as a slugified output URL (`/assets/europe-a-prophecy/...`).
/// With the snapshot additively indexed under the slug key (Bug6.1), the
/// synthesized `<img>` must carry the real portrait dims, NOT the fallback.
#[test]
fn cover_slug_url_emits_real_dimensions_not_fallback() {
    let mut snap = snapshot_dims("assets/Europe - A Prophecy/e-006.jpg", 4515, 6158);
    snap.dimensions.insert(
        PathBuf::from("assets/europe-a-prophecy/e-006.jpg"),
        (4515, 6158),
    );
    let html = synthesize_image_html(
        "/assets/europe-a-prophecy/e-006.jpg",
        "cover",
        &snap,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"width="4515" height="6158""#),
        "expected real dims, got: {html}"
    );
    assert!(
        !html.contains(r#"width="800" height="600""#),
        "800x600 fallback fired: {html}"
    );
}

// --- <picture>-wrapped shape (raster originals always wrapped 2026-05-20) ---

#[test]
fn picture_wrap_for_raster_original_no_lqip() {
    // After 2026-05-20: synthesizer always emits <picture> for png/jpg/jpeg
    // originals. The preview server's AssetRegistry intercept ensures the
    // webp URL resolves (LQIP bytes for Pending, real bytes for Ready);
    // publish-mode synchronous encoding ensures it never 404s in
    // production. data-placeholder-src is gone — iframe-bridge matches
    // by URL substring now.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let html = synthesize_image_html(
        "photo.jpg",
        "A cat",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<picture><source srcset="photo.webp" type="image/webp"><img src="photo.jpg" width="800" height="600" loading="lazy" alt="A cat" /></picture>"#
    );
}

#[test]
fn picture_wrap_for_raster_original_with_lqip() {
    // LQIP inline style is preserved (legitimate production technique;
    // cf. Vercel `blurDataURL`). Shown to the user instantly while the
    // actual bytes are being decoded.
    let s = snapshot_with(
        "photo.jpg",
        Some((800, 600)),
        None,
        Some("data:image/jpeg;base64,abc"),
        false,
    );
    let html = synthesize_image_html(
        "photo.jpg",
        "A cat",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<picture><source srcset="photo.webp" type="image/webp"><img src="photo.jpg" width="800" height="600" loading="lazy" style="background-image:url(data:image/jpeg;base64,abc);background-size:cover" alt="A cat" /></picture>"#
    );
}

#[test]
fn bare_img_with_dominant_color_no_lqip() {
    let s = snapshot_with("photo.jpg", Some((800, 600)), Some("#aabbcc"), None, false);
    let html = synthesize_image_html(
        "photo.jpg",
        "Cat",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"style="background-color:#aabbcc""#),
        "Got: {html}"
    );
}

// --- <picture> wrap (WebP variant present) ----------------------------

#[test]
fn picture_wrap_when_webp_exists() {
    let s = snapshot_with("photo.jpg", Some((800, 600)), None, None, true);
    let html = synthesize_image_html(
        "photo.jpg",
        "Cat",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    // Outer: <picture>...</picture> with <source> first
    assert!(
        html.starts_with(r#"<picture><source srcset="photo.webp" type="image/webp"><img"#),
        "Got: {html}"
    );
    assert!(html.ends_with(r#"alt="Cat" /></picture>"#), "Got: {html}");
}

#[test]
fn picture_srcset_uses_to_webp_of_src_not_stored_variant() {
    // CJK-path case: <img src> already carries dir_overrides + relative
    // prefix (e.g. ../assets/photo.jpg). srcset must derive from src
    // (../assets/photo.webp), not the manifest's stored value.
    // See wrap_img_in_picture's rationale at placeholder.rs:660.
    let s = snapshot_with("../assets/photo.jpg", Some((800, 600)), None, None, true);
    let html = synthesize_image_html(
        "../assets/photo.jpg",
        "",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"srcset="../assets/photo.webp""#),
        "srcset should inherit src's prefix; got: {html}"
    );
}

// --- eager loading ----------------------------------------------------

#[test]
fn eager_swaps_loading_attr_and_adds_fetchpriority() {
    let s = snapshot_dims("photo.jpg", 1, 1);
    let html = synthesize_image_html(
        "photo.jpg",
        "Hero",
        &s,
        ImageContext::Hero,
        &ImageRenderOptions {
            eager: true,
            ..Default::default()
        },
    );
    assert!(
        html.contains(r#"loading="eager" fetchpriority="high""#),
        "Got: {html}"
    );
    assert!(!html.contains(r#"loading="lazy""#), "Got: {html}");
}

// --- favicon short-circuit --------------------------------------------

#[test]
fn favicon_is_bare_16x16_with_class() {
    // No manifest data for the favicon URL — synthesizer must NOT probe
    // the snapshot (favicons are not registered as moss assets).
    let s = AssetSnapshot::new();
    let html = synthesize_image_html(
        "https://example.com/favicon.ico",
        "",
        &s,
        ImageContext::Favicon,
        &ImageRenderOptions {
            class: Some("link-preview-favicon"),
            ..Default::default()
        },
    );
    assert_eq!(
        html,
        r#"<img class="link-preview-favicon" src="https://example.com/favicon.ico" width="16" height="16" alt="">"#
    );
}

// --- site-logo short-circuit (Phase 2B carve-out) --------------------

#[test]
fn synthesize_site_logo_basic_shape() {
    let html = synthesize_image_html(
        "assets/logo.png",
        "",
        &AssetSnapshot::new(),
        ImageContext::SiteLogo,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img class="site-logo" src="assets/logo.png" alt="" aria-hidden="true">"#
    );
}

#[test]
fn synthesize_site_logo_escapes_src() {
    let html = synthesize_image_html(
        r#"logo "with quotes".png"#,
        "",
        &AssetSnapshot::new(),
        ImageContext::SiteLogo,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"logo &quot;with quotes&quot;.png"#),
        "got: {html}"
    );
}

#[test]
fn synthesize_site_logo_does_not_emit_picture_or_lqip() {
    // Even when the snapshot would normally drive LQIP/<picture> for a
    // png, SiteLogo must short-circuit and emit a bare <img>.
    let mut snap = AssetSnapshot::new();
    snap.lqip
        .insert("logo.png".into(), "data:image/jpeg;base64,xxx".into());
    let html = synthesize_image_html(
        "logo.png",
        "moss",
        &snap,
        ImageContext::SiteLogo,
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("<picture"),
        "logo should not get picture wrap: {html}"
    );
    assert!(
        !html.contains("background-image"),
        "logo should not get LQIP: {html}"
    );
    assert!(
        !html.contains("loading="),
        "logo above-the-fold, no lazy-load: {html}"
    );
}

// --- HTML-escape contract --------------------------------------------

#[test]
fn alt_with_quotes_is_escaped() {
    let s = snapshot_dims("photo.jpg", 1, 1);
    let html = synthesize_image_html(
        "photo.jpg",
        r#"Quote: "hi""#,
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    // moss_core::media::html_escape escapes " → &quot; — never leave a
    // raw quote inside an attribute value.
    assert!(
        html.contains(r#"alt="Quote: &quot;hi&quot;""#),
        "Got: {html}"
    );
}

#[test]
fn extra_attrs_passed_through_verbatim() {
    // :::hero passes `style="object-fit:cover;object-position:50% 50%"`
    // via MediaAttrs::to_inline_style — the caller pre-escapes, so we
    // just append.
    let s = snapshot_dims("hero.jpg", 1920, 1080);
    let html = synthesize_image_html(
        "hero.jpg",
        "",
        &s,
        ImageContext::Hero,
        &ImageRenderOptions {
            eager: true,
            extra_attrs: Some(r#"data-cover-fit="cover""#),
            ..Default::default()
        },
    );
    assert!(html.contains(r#"data-cover-fit="cover""#), "Got: {html}");
    // Extra attrs come AFTER alt — matches the current regex-pass order
    // where the regex preserves the original post-src attributes.
    let alt_pos = html.find("alt=").unwrap();
    let extra_pos = html.find("data-cover-fit=").unwrap();
    assert!(extra_pos > alt_pos, "extra attrs should come after alt");
}

/// Regression test for impl-review finding 2026-05-16: when extra_attrs
/// already carries a `style=` attribute (the `:::hero {attrs=...}` case
/// when MediaAttrs::to_inline_style() returns Some), the synthesizer
/// must NOT also emit its own LQIP-derived `style=`. The legacy regex
/// pass had a `has_style` guard at `placeholder.rs:413-422` that did the
/// same suppression. Without this, browsers see two `style=` attributes
/// on one element, honor the last one, and drop the LQIP placeholder.
#[test]
fn lqip_style_suppressed_when_extra_attrs_has_style() {
    let s = snapshot_with(
        "hero.jpg",
        Some((1920, 1080)),
        None,
        Some("data:image/jpeg;base64,abc"),
        false,
    );
    let html = synthesize_image_html(
        "hero.jpg",
        "",
        &s,
        ImageContext::Hero,
        &ImageRenderOptions {
            eager: true,
            extra_attrs: Some(r#"style="object-fit:cover;object-position:50% 50%""#),
            ..Default::default()
        },
    );
    // Exactly one `style=` substring — the one the caller passed.
    assert_eq!(
        html.matches("style=").count(),
        1,
        "expected exactly one style= attribute; got: {html}"
    );
    // Confirm the caller's style is what survived (not the LQIP).
    assert!(
        html.contains(r#"style="object-fit:cover"#),
        "caller-supplied style must survive; got: {html}"
    );
    assert!(
        !html.contains(r#"background-image:url(data:"#),
        "LQIP must be suppressed when extra_attrs carries style=; got: {html}"
    );
}

// --- Step 8: figure wrapper for MarkdownStandalone --------------------

/// Phase 1 C1 test helper: build an empty extras BTreeMap.
fn empty_extras() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

#[test]
fn markdown_standalone_no_caption_wraps_in_figure() {
    // No caption → `<figure class="moss-image">…</figure>` around the
    // synthesized `<picture>`/`<img>` with no `<figcaption>`.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt text",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.starts_with(r#"<figure class="moss-image">"#));
    assert!(html.ends_with("</figure>"));
    assert!(!html.contains("<figcaption>"));
    assert!(html.contains("<img"));
}

#[test]
fn markdown_standalone_with_caption_adds_figcaption() {
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt text",
        &s,
        ImageContext::MarkdownStandalone {
            caption: Some("A nice photo"),
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.starts_with(r#"<figure class="moss-image">"#));
    assert!(html.contains("<figcaption>A nice photo</figcaption>"));
    assert!(html.ends_with("</figure>"));
}

#[test]
fn markdown_standalone_caption_is_html_escaped() {
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: Some(r#"Q&A "best" of <em>2024</em>"#),
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(
            "<figcaption>Q&amp;A &quot;best&quot; of &lt;em&gt;2024&lt;/em&gt;</figcaption>"
        ),
        "caption text must be HTML-escaped at the boundary; got: {html}"
    );
}

#[test]
fn markdown_standalone_wraps_picture_when_webp_present() {
    // When the manifest carries a WebP variant, the synthesizer emits
    // a `<picture>` wrap inside the figure: the structural figure
    // and the responsive picture compose without conflict.
    let s = snapshot_with("photo.jpg", Some((1200, 800)), None, None, true);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt",
        &s,
        ImageContext::MarkdownStandalone {
            caption: Some("Cap"),
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    // Structural order: figure > picture > source + img > figcaption
    let fig_idx = html.find(r#"<figure class="moss-image">"#).expect("figure");
    let pic_idx = html.find("<picture>").expect("picture");
    let src_idx = html.find("<source").expect("source");
    let img_idx = html.find("<img").expect("img");
    let cap_idx = html.find("<figcaption>").expect("figcaption");
    let fig_close = html.find("</figure>").expect("figure close");
    assert!(fig_idx < pic_idx, "<picture> must be inside <figure>");
    assert!(pic_idx < src_idx);
    assert!(src_idx < img_idx);
    assert!(img_idx < cap_idx, "<figcaption> follows <picture>");
    assert!(cap_idx < fig_close);
}

#[test]
fn markdown_inline_does_not_wrap_in_figure() {
    // Inline images NEVER get a figure wrapper — they sit in prose.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(!html.contains("<figure"));
    assert!(!html.contains("<figcaption>"));
}

// --- spec § P9 width: `data-width` on the figure wrapper -------------

#[test]
fn markdown_standalone_width_screen_emits_data_width_on_figure() {
    // Width pipe-alias `![[photo.jpg|full]]` → `screen` lands on the
    // figure wrapper, not the inner img. The image-side test below
    // pins the "absent by default" half of the contract.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: Some("screen"),
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        html.starts_with(r#"<figure class="moss-image" data-width="screen">"#),
        "data-width must sit on the figure wrapper; got: {html}"
    );
    // The inner img must NOT carry data-width — the attribute is the
    // wrapper's responsibility per spec.
    assert!(
        !html.contains(r#"<img"#) || !html[html.find("<img").unwrap()..].contains("data-width="),
        "inner <img> must not carry data-width; got: {html}"
    );
}

#[test]
fn markdown_standalone_width_wide_with_caption() {
    // width + caption compose: both attributes / children appear in
    // the wrapper.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt",
        &s,
        ImageContext::MarkdownStandalone {
            caption: Some("A nice photo"),
            width: Some("wide"),
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.contains(r#"data-width="wide""#), "got: {html}");
    assert!(
        html.contains("<figcaption>A nice photo</figcaption>"),
        "got: {html}"
    );
}

#[test]
fn markdown_standalone_width_none_omits_data_width() {
    // Negative test: the default (no width) must not emit the attribute,
    // so theme authors can target the absence via `:not([data-width])`.
    let s = snapshot_dims("photo.jpg", 800, 600);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(!html.contains("data-width="), "got: {html}");
}

#[test]
fn wrap_in_figure_width_emits_data_width_attribute() {
    // Direct-call contract test: `wrap_in_figure` is the single source
    // of truth for the figure wrapper byte shape; the raw-HTML branch
    // of `emit_standalone_figure_image` calls it with the lifted width.
    let html = wrap_in_figure(r#"<img src="x" />"#, None, Some("page"));
    assert_eq!(
        html,
        r#"<figure class="moss-image" data-width="page"><img src="x" /></figure>"#
    );
}

#[test]
fn wrap_in_figure_width_with_caption() {
    let html = wrap_in_figure(r#"<img src="x" />"#, Some("hello"), Some("screen"));
    assert_eq!(
        html,
        r#"<figure class="moss-image" data-width="screen"><img src="x" /><figcaption>hello</figcaption></figure>"#
    );
}

#[test]
fn wrap_in_figure_no_width_no_attribute() {
    let html = wrap_in_figure(r#"<img src="x" />"#, None, None);
    assert_eq!(
        html,
        r#"<figure class="moss-image"><img src="x" /></figure>"#
    );
}

#[test]
fn markdown_standalone_no_manifest_still_wraps_in_figure() {
    // The wrapper is structural identity (Step 8 contract), not
    // manifest-dependent. Even with an empty AssetSnapshot (test/
    // fragment-render path), `<figure class="moss-image">` still wraps
    // the synthesized `<img>`. The Phase 1 B1 migration (2026-05-25)
    // replaced the `Option<&MediaDimensionLookup>` parameter with
    // `&AssetSnapshot`; the empty-snapshot path is now the test
    // equivalent of the prior `None` lookup.
    let s = AssetSnapshot::new();
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "Alt",
        &s,
        ImageContext::MarkdownStandalone {
            caption: Some("Cap"),
            width: None,
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.starts_with(r#"<figure class="moss-image">"#));
    assert!(html.contains("<img"));
    assert!(html.contains("<figcaption>Cap</figcaption>"));
    assert!(html.ends_with("</figure>"));
}

// --- size fallback ----------------------------------------------------

#[test]
fn missing_dimensions_fall_back_to_800x600() {
    let s = AssetSnapshot::new();
    let html = synthesize_image_html(
        "ghost.jpg",
        "",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    // FALLBACK_WIDTH / FALLBACK_HEIGHT (still 800×600, sourced from
    // `moss_core::asset_snapshot::FALLBACK_WIDTH` so the synthesizer and
    // the surviving regex pass agree on the absent-dims default).
    assert!(html.contains(r#"width="800" height="600""#), "Got: {html}");
}

// --- regex-pass idempotency on synthesizer output ----------------------
//
// Phase 2E v5 PR5 (2026-05-26) retired the Stage 3 regex post-pass; the
// image synthesizer in this module is now the sole emitter of width /
// height / loading / LQIP / dominant-color attributes for moss-emitted
// <img> tags. The three idempotency tests at
// `src-tauri/tests/image_synth_regex_parity.rs` that guarded the
// regex+synth byte-shape parity were deleted alongside the regex.

// --- TrackingPixel (Phase 2C, 2026-05-25) ---
//
// The RSS read-tracking pixel is a 1×1 invisible <img>. It must fire on
// read (so NO loading="lazy"), carry empty alt (decorative), and never
// be wrapped in <picture> / decorated with LQIP. Self-closing form
// because the call site embeds it in CDATA-wrapped RSS XML.

#[test]
fn synthesize_tracking_pixel_basic() {
    let html = synthesize_image_html(
        "https://api.mosspub.com/pixel.gif?u=abc",
        "",
        &AssetSnapshot::new(),
        ImageContext::TrackingPixel,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img src="https://api.mosspub.com/pixel.gif?u=abc" alt="" width="1" height="1" />"#
    );
}

#[test]
fn synthesize_tracking_pixel_escapes_url() {
    let html = synthesize_image_html(
        r#"x.gif?u=a&b="c""#,
        "",
        &AssetSnapshot::new(),
        ImageContext::TrackingPixel,
        &ImageRenderOptions::default(),
    );
    assert!(html.contains("&amp;"), "& must be escaped");
    assert!(html.contains("&quot;"), "\" must be escaped");
}

#[test]
fn synthesize_tracking_pixel_no_lazy_no_lqip() {
    // Even when the snapshot contains LQIP / dimensions for the pixel
    // path, the TrackingPixel short-circuit must ignore them — pixels
    // are tracking beacons, not images.
    let mut snap = AssetSnapshot::new();
    snap.lqip
        .insert("pixel.gif".into(), "data:image/jpeg;base64,xxx".into());
    let html = synthesize_image_html(
        "pixel.gif",
        "",
        &snap,
        ImageContext::TrackingPixel,
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("loading=\"lazy\""),
        "must NOT lazy-load (must fire on read)"
    );
    assert!(!html.contains("background-image"), "must NOT carry LQIP");
    assert!(!html.contains("<picture"), "must NOT wrap in picture");
}

// --- ImageContext::EmailBody (Phase 2D, 2026-05-25) -------------------
//
// Email-client-safe carve-out: no <picture>, no data-*, no loading=lazy.
// Inline `style="display:block;max-width:100%;height:auto;"` is the
// cross-client responsive pattern. width/height attrs are Option<u32>:
// emitted when known, omitted when None.

#[test]
fn synthesize_email_body_with_dims() {
    let html = synthesize_image_html(
        "https://media.example.com/photo.jpg",
        "Cover",
        &AssetSnapshot::new(),
        ImageContext::EmailBody {
            width: Some(600),
            height: Some(400),
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.contains(r#"width="600""#));
    assert!(html.contains(r#"height="400""#));
    assert!(html.contains(r#"style="display:block;max-width:100%;height:auto;""#));
}

#[test]
fn synthesize_email_body_without_dims() {
    let html = synthesize_image_html(
        "x.jpg",
        "alt",
        &AssetSnapshot::new(),
        ImageContext::EmailBody {
            width: None,
            height: None,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("width="),
        "width should be omitted when None"
    );
    assert!(
        !html.contains("height="),
        "height should be omitted when None"
    );
}

#[test]
fn synthesize_email_body_no_picture_no_data() {
    let html = synthesize_image_html(
        "photo.jpg",
        "alt",
        &AssetSnapshot::new(),
        ImageContext::EmailBody {
            width: None,
            height: None,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("<picture"),
        "email images must not use <picture>"
    );
    assert!(!html.contains("<source"), "no <source>");
    assert!(!html.contains("data-"), "no data-* (email clients strip)");
    assert!(
        !html.contains("loading="),
        "email clients ignore loading attr"
    );
    assert!(
        !html.contains("background-image"),
        "email clients strip inline style URLs"
    );
}

#[test]
fn synthesize_email_body_escapes() {
    let html = synthesize_image_html(
        r#"https://x.com/photo.jpg?a=1&b=2"#,
        r#"alt with "quotes""#,
        &AssetSnapshot::new(),
        ImageContext::EmailBody {
            width: None,
            height: None,
        },
        &ImageRenderOptions::default(),
    );
    assert!(html.contains("&amp;"), "& must be escaped");
    assert!(html.contains("&quot;"), "\" must be escaped");
}

// --- ImageContext::GalleryThumb (Phase 2E v5 PR3, 2026-05-26) ---------
//
// Gallery body images: below-the-fold thumbnail, same inner byte
// shape as MarkdownInline (`<picture><source srcset=*.webp><img
// loading="lazy" ...></picture>` for raster, bare `<img>` for
// non-raster). The outer `.moss-gallery-item` wrapper is owned by
// `DefaultHooks::render_shortcode`'s Gallery arm; this variant
// emits only the inner image. Distinguishing it from
// MarkdownInline at the type level keeps per-item passthrough
// attributes (object-position from MediaAttrs) typed for future
// evolution.

#[test]
fn synthesize_gallery_thumb_emits_picture_with_lazy() {
    let p = ImageRenderOptions::default();
    let mut snap = AssetSnapshot::new();
    snap.dimensions
        .insert(PathBuf::from("photo.jpg"), (1200, 800));
    let out = synthesize_image_html("photo.jpg", "alt", &snap, ImageContext::GalleryThumb, &p);
    assert!(out.contains("<picture"), "{out}");
    // 1200px-wide fixture → ladder rung at 800 + capped base descriptor,
    // with the gallery grid-cell sizes (Task 3, responsive-image-variants).
    assert!(
            out.contains(
                r#"srcset="photo.w800.webp 800w, photo.webp 1200w" type="image/webp" sizes="(min-width: 48rem) 33vw, 100vw""#
            ),
            "{out}"
        );
    assert!(out.contains(r#"loading="lazy""#), "{out}");
    assert!(out.contains(r#"width="1200""#), "{out}");
    assert!(out.contains(r#"height="800""#), "{out}");
    assert!(out.contains(r#"alt="alt""#), "{out}");
}

#[test]
fn synthesize_gallery_thumb_non_raster_bare_img() {
    // SVG / .webp originals don't trigger the <picture> wrap (no
    // variant exists). Falls back to bare <img> with lazy loading +
    // dims from the snapshot.
    let p = ImageRenderOptions::default();
    let mut snap = AssetSnapshot::new();
    snap.dimensions.insert(PathBuf::from("icon.svg"), (64, 64));
    let out = synthesize_image_html("icon.svg", "", &snap, ImageContext::GalleryThumb, &p);
    assert!(!out.contains("<picture"), "{out}");
    assert!(!out.contains("<source"), "{out}");
    assert!(out.contains(r#"loading="lazy""#), "{out}");
    assert!(out.contains(r#"width="64""#), "{out}");
}

#[test]
fn synthesize_gallery_thumb_threads_extra_attrs() {
    // The Gallery hook builds a `style="object-position:..."` fragment
    // from MediaAttrs and passes it via extra_attrs. The synthesizer
    // suppresses its own LQIP-derived style= when extra_attrs already
    // carries one — verify that suppression engages here too
    // (parity with Hero / MarkdownInline).
    let snap = snapshot_with(
        "photo.jpg",
        Some((1200, 800)),
        None,
        Some("data:image/jpeg;base64,abc"),
        false,
    );
    let opts = ImageRenderOptions {
        extra_attrs: Some(r#"style="object-position:50% 50%""#),
        ..Default::default()
    };
    let out = synthesize_image_html("photo.jpg", "", &snap, ImageContext::GalleryThumb, &opts);
    assert_eq!(
        out.matches("style=").count(),
        1,
        "expected exactly one style= attribute; got: {out}"
    );
    assert!(
        out.contains(r#"style="object-position:50% 50%""#),
        "caller-supplied style must survive; got: {out}"
    );
}

// --- ImageContext::HeroBare (Phase 2E PR2, 2026-05-26) ----------------
//
// The no-snapshot hero fallback. Emits a bare `<img>` with no
// `<picture>`, no `<source>`, no LQIP, no dims, no `loading` attr.
// The asset-publish invariant rules out emitting a
// `<source srcset="*.webp">` for an unregistered variant — this
// variant is the explicit opt-out for code paths that run before
// `AssetRegistry::set_pending` has been called for the source's
// `.webp` companion (test/fragment-render paths). The byte shape
// mirrors the pre-PR2 fallback at
// `typed_renderers.rs::render_hero_html_typed` lines 554-557.

#[test]
fn synthesize_hero_bare_basic_shape() {
    let out = synthesize_image_html(
        "cover.jpg",
        "",
        &AssetSnapshot::new(),
        ImageContext::HeroBare,
        &ImageRenderOptions::default(),
    );
    // No <picture>, no <source>, no class, no loading, no LQIP, no
    // width/height attrs. Exact byte shape with empty alt.
    assert_eq!(out, r#"<img src="cover.jpg" alt="" />"#, "got: {}", out);
}

// --- srcset ladder + sizes (responsive-image-variants Task 3) ---------
//
// Sources wider than the first ladder rung (800px) gain width
// descriptors for each rung below the deployed base plus the base
// itself (capped at DEPLOY_MAX_EDGE), and a per-context sizes=
// attribute from contract::sizes. Sources at/below the first rung —
// and unknown-dims sources — keep the legacy single-URL shape
// byte-identical (no descriptors, no sizes).

#[test]
fn ladder_srcset_emitted_for_wide_raster() {
    let assets = snapshot_dims("photo.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "photo.jpg",
        "alt",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
            html.contains(
                r#"<source srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 2000w" type="image/webp" sizes="(min-width: 48rem) 47.25rem, 100vw">"#
            ),
            "got: {html}"
        );
}

#[test]
fn base_descriptor_caps_at_deploy_max_edge() {
    let assets = snapshot_dims("photo.jpg", 4000, 3000);
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(html.contains("photo.webp 2400w"), "got: {html}");
}

#[test]
fn no_ladder_below_first_rung_keeps_legacy_shape() {
    let assets = snapshot_dims("photo.jpg", 800, 600);
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    // Byte-identical to today's single-URL source: no descriptors, no sizes.
    assert!(
        html.contains(r#"<source srcset="photo.webp" type="image/webp">"#),
        "got: {html}"
    );
    assert!(!html.contains("sizes="), "got: {html}");
}

#[test]
fn unknown_dims_keep_legacy_shape() {
    // Empty snapshot → fallback 800×600 → no rungs → legacy single-URL shape.
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &AssetSnapshot::new(),
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"<source srcset="photo.webp" type="image/webp">"#),
        "got: {html}"
    );
    assert!(!html.contains("sizes="), "got: {html}");
}

#[test]
fn portrait_below_first_rung_keeps_legacy_shape() {
    // 1200×3600 portrait: the encoder caps the longest EDGE, so the
    // deployed base is only 800 wide — no rung below it (strict `<`),
    // legacy single-URL shape.
    let assets = snapshot_dims("photo.jpg", 1200, 3600);
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"<source srcset="photo.webp" type="image/webp">"#),
        "got: {html}"
    );
    assert!(!html.contains("sizes="), "got: {html}");
}

#[test]
fn portrait_base_descriptor_uses_post_resize_width() {
    // 3024×4032 portrait deploys at 1800×2400 — the base descriptor
    // must be the POST-RESIZE width (1800w), never min(w, 2400).
    let assets = snapshot_dims("photo.jpg", 3024, 4032);
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 1800w""#),
        "got: {html}"
    );
}

#[test]
fn folder_card_cover_uses_card_sizes() {
    let assets = snapshot_dims("cover.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "cover.jpg",
        "",
        &assets,
        ImageContext::FolderCardCover,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"sizes="(min-width: 48rem) 24rem, 100vw""#),
        "got: {html}"
    );
}

#[test]
fn percent_encoded_src_gets_encoded_rung_urls() {
    // Body/wikilink srcs arrive percent-encoded while snapshot keys are
    // the RAW source path: the dims probe decodes (BUG 6.2), but the
    // emitted rung URLs stay in the src's encoded URL space — same
    // derivation rule as the base to_webp(src).
    let assets = snapshot_dims("a b/photo.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "a%20b/photo.jpg",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
            html.contains(
                r#"srcset="a%20b/photo.w800.webp 800w, a%20b/photo.w1600.webp 1600w, a%20b/photo.webp 2000w""#
            ),
            "got: {html}"
        );
}

// --- comma-in-filename srcset encoding (follow-up #4) -----------------
//
// srcset is a comma-separated candidate list. A source file whose NAME
// contains a comma (`a,b.jpg`) yields candidate URLs with literal commas
// (`percent_encode_path_segments` keeps `,` literal — see fuzzy_path.rs).
// In a LADDER srcset those literal commas mis-split the list (both in
// browsers and in the html_post `rewrite_srcset_candidates` splitter), so
// the synthesizer `%2C`-encodes commas in EACH candidate URL. The `<img
// src>` base attribute keeps its literal comma (a single unambiguous URL).
// A static file server decodes `%2C` → `,` when resolving, so the deployed
// file `a,b.w800.webp` (literal comma on disk) is found — verified against
// the preview server (tower-http ServeDir + urlencoding::decode) in
// preview/server/router.rs's `%2C` decode test.

#[test]
fn comma_named_raster_encodes_srcset_commas_but_not_base_src() {
    // jpg `<picture>` path: every `<source srcset>` candidate URL has its
    // comma encoded; the inner `<img src>` keeps the literal comma.
    let assets = snapshot_dims("a,b.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "a,b.jpg",
        "cat",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<picture><source srcset="a%2Cb.w800.webp 800w, a%2Cb.w1600.webp 1600w, a%2Cb.webp 2000w" type="image/webp" sizes="(min-width: 48rem) 47.25rem, 100vw"><img src="a,b.jpg" width="2000" height="1200" loading="lazy" alt="cat" /></picture>"#
    );
}

#[test]
fn comma_named_webp_source_encodes_img_srcset_commas_but_not_base_src() {
    // webp `<img srcset>` path: candidate URLs comma-encoded; the base
    // `<img src>` keeps the literal comma.
    let assets = snapshot_dims("a,b.webp", 2000, 1200);
    let html = synthesize_image_html(
        "a,b.webp",
        "cat",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img src="a,b.webp" srcset="a%2Cb.w800.webp 800w, a%2Cb.w1600.webp 1600w, a%2Cb.webp 2000w" sizes="(min-width: 48rem) 47.25rem, 100vw" width="2000" height="1200" loading="lazy" alt="cat" />"#
    );
}

#[test]
fn comma_named_raster_with_no_ladder_encodes_single_url_srcset() {
    // Bug B (docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md):
    // when dims are unknown (snapshot miss → `resolve_ladder` returns
    // `None`), `synthesize_inner` takes the LEGACY single-URL `<source
    // srcset="X.webp">` branch instead of the ladder branch. That branch
    // used to skip `encode_srcset_url` entirely, so a comma-named source
    // shipped a raw `,` in `srcset` — the browser splits one candidate
    // into two bogus ones and both 404 (confirmed on the live site).
    let assets = AssetSnapshot::new(); // no dims registered for "a,b.jpg"
    let html = synthesize_image_html(
        "a,b.jpg",
        "cat",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<picture><source srcset="a%2Cb.webp" type="image/webp"><img src="a,b.jpg" width="800" height="600" loading="lazy" alt="cat" /></picture>"#
    );
    // srcset carries a single candidate — no comma anywhere in it, so a
    // spec-compliant candidate-list split yields exactly one entry.
    let srcset_start = html.find("srcset=\"").unwrap() + "srcset=\"".len();
    let srcset_end = html[srcset_start..].find('"').unwrap() + srcset_start;
    assert!(
        !html[srcset_start..srcset_end].contains(','),
        "srcset candidate must not contain a literal comma; got: {html}"
    );
    // The base `<img src>` keeps its literal comma — safe there, and
    // required so the deployed source path still round-trips.
    assert!(html.contains(r#"src="a,b.jpg""#), "got: {html}");
}

#[test]
fn no_comma_filename_srcset_is_not_over_encoded() {
    // Over-encoding guard: a comma-free filename must emit a byte-identical
    // srcset with NO `%2C` anywhere — the comma encoding only ever touches
    // real commas.
    let assets = snapshot_dims("photo.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "photo.jpg",
        "cat",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(!html.contains("%2C"), "no comma to encode; got: {html}");
    assert_eq!(
        html,
        r#"<picture><source srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 2000w" type="image/webp" sizes="(min-width: 48rem) 47.25rem, 100vw"><img src="photo.jpg" width="2000" height="1200" loading="lazy" alt="cat" /></picture>"#
    );
}

#[test]
fn hero_context_uses_full_bleed_sizes() {
    let assets = snapshot_dims("hero.jpg", 2400, 985);
    let html = synthesize_image_html(
        "hero.jpg",
        "",
        &assets,
        ImageContext::Hero,
        &ImageRenderOptions::default(),
    );
    assert!(html.contains(r#"sizes="100vw""#), "got: {html}");
}

#[test]
fn markdown_standalone_width_wide_uses_wide_band_sizes() {
    // ADR-021 Corollary 2 content-width escape (2026-07-30): site.css now
    // sizes data-width bands, so a wide figure declares the wide band —
    // min(63rem, 100vw) above the 48rem breakpoint — not the content
    // column. (Pre-escape this mapped to SIZES_BODY because the attribute
    // had no width rule and 100vw would have over-fetched.)
    let s = snapshot_dims("photo.jpg", 2000, 1200);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: Some("wide"),
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        html.starts_with(r#"<figure class="moss-image" data-width="wide">"#),
        "got: {html}"
    );
    assert!(
        html.contains(r#"sizes="(min-width: 48rem) min(63rem, 100vw), 100vw""#),
        "wide declares the wide escape band; got: {html}"
    );
}

#[test]
fn markdown_standalone_width_page_uses_page_band_sizes() {
    // page = --moss-site-max-width (1200px), clamped to the container.
    let s = snapshot_dims("photo.jpg", 2000, 1200);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: Some("page"),
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"sizes="(min-width: 48rem) min(1200px, 100vw), 100vw""#),
        "page declares the page escape band; got: {html}"
    );
}

#[test]
fn explicit_sizes_option_overrides_context() {
    // The options.sizes channel (figure data-width tokens and grid-cell
    // scoping thread through it) must win over the context default.
    let s = snapshot_dims("photo.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownInline,
        &ImageRenderOptions {
            sizes: Some("(min-width: 48rem) calc(min(1200px, 100vw) / 3), 100vw"),
            ..Default::default()
        },
    );
    assert!(
        html.contains(r#"sizes="(min-width: 48rem) calc(min(1200px, 100vw) / 3), 100vw""#),
        "options.sizes must override the context default; got: {html}"
    );
}

#[test]
fn markdown_standalone_width_screen_uses_full_bleed_sizes() {
    // screen/full = the full container band (100cqw in site.css's
    // content-width escape); 100vw is the closest sizes= can express.
    // Mirrors markdown_standalone_width_screen_emits_data_width
    // _on_figure but with ladder-triggering dims.
    let s = snapshot_dims("photo.jpg", 2000, 1200);
    let extras = empty_extras();
    let html = synthesize_image_html(
        "photo.jpg",
        "",
        &s,
        ImageContext::MarkdownStandalone {
            caption: None,
            width: Some("screen"),
            align: None,
            class_names: &[],
            extra_attrs: &extras,
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        html.starts_with(r#"<figure class="moss-image" data-width="screen">"#),
        "got: {html}"
    );
    assert!(html.contains(r#"sizes="100vw""#), "got: {html}");
}

#[test]
fn email_body_output_unchanged_by_ladder() {
    let assets = snapshot_dims("photo.jpg", 2000, 1200);
    let html = synthesize_image_html(
        "photo.jpg",
        "a",
        &assets,
        ImageContext::EmailBody {
            width: Some(2000),
            height: Some(1200),
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("srcset"),
        "email must never carry srcset: {html}"
    );
    assert!(!html.contains("<picture"), "got: {html}");
}

#[test]
fn synthesize_hero_bare_with_style_via_extra_attrs() {
    // The legacy fallback passed an inline `style="..."` fragment
    // built from MediaAttrs::to_inline_style(). PR2 threads that
    // fragment through `ImageRenderOptions::extra_attrs` — the
    // synthesizer prepends a single space, matching the legacy byte
    // shape.
    let out = synthesize_image_html(
        "cover.jpg",
        "",
        &AssetSnapshot::new(),
        ImageContext::HeroBare,
        &ImageRenderOptions {
            extra_attrs: Some(r#"style="object-fit:cover;object-position:50% 50%""#),
            ..Default::default()
        },
    );
    assert_eq!(
        out, r#"<img src="cover.jpg" alt="" style="object-fit:cover;object-position:50% 50%" />"#,
        "got: {}",
        out
    );
}

#[test]
fn synthesize_hero_bare_with_lqip_in_snapshot_still_bare() {
    // Even when the snapshot has LQIP / dims for the source path,
    // the HeroBare variant must ignore them — the no-snapshot signal
    // is structural (this variant exists precisely because no
    // AssetRegistry has been primed), not data-driven.
    let mut snap = AssetSnapshot::new();
    snap.lqip
        .insert("cover.jpg".into(), "data:image/jpeg;base64,xxx".into());
    snap.dimensions.insert("cover.jpg".into(), (1920, 1080));
    let out = synthesize_image_html(
        "cover.jpg",
        "",
        &snap,
        ImageContext::HeroBare,
        &ImageRenderOptions::default(),
    );
    assert!(!out.contains("<picture"), "got: {}", out);
    assert!(!out.contains("<source"), "got: {}", out);
    assert!(!out.contains("background-image"), "got: {}", out);
    assert!(!out.contains("width="), "got: {}", out);
    assert!(!out.contains("height="), "got: {}", out);
    assert!(!out.contains("loading="), "got: {}", out);
}

#[test]
fn synthesize_hero_bare_escapes_url() {
    // The synthesizer's html_escape covers `&` and `"`; the legacy
    // fallback used the same `html_escape` from build::media::cover.
    let out = synthesize_image_html(
        r#"x.jpg?a=1&b="c""#,
        "",
        &AssetSnapshot::new(),
        ImageContext::HeroBare,
        &ImageRenderOptions::default(),
    );
    assert!(out.contains("&amp;"), "& must be escaped; got: {}", out);
    assert!(out.contains("&quot;"), "\" must be escaped; got: {}", out);
}

#[test]
fn synthesize_hero_bare_byte_shape_matches_legacy_fallback() {
    // Pin the byte shape against a literal reconstruction of the
    // pre-PR2 emission so a future "tidy" of the synthesizer's
    // HeroBare branch can't drift away from the legacy fallback
    // without flipping this assertion deliberately.
    //
    // Legacy line:
    //   format!("<img src=\"{}\" alt=\"\"{} />", html_escape(href), style)
    // where `style` was `""` or ` style="..."` (with leading space).
    let href = "covers/img.jpg";
    let style_fragment = r#" style="object-fit:contain""#;
    let legacy = format!(
        "<img src=\"{}\" alt=\"\"{} />",
        html_escape(href),
        style_fragment,
    );

    // PR2 path: thread the style through extra_attrs minus the leading
    // space (matches typed_renderers.rs migration).
    let pr2 = synthesize_image_html(
        href,
        "",
        &AssetSnapshot::new(),
        ImageContext::HeroBare,
        &ImageRenderOptions {
            extra_attrs: Some(style_fragment.trim_start()),
            ..Default::default()
        },
    );
    assert_eq!(
        pr2, legacy,
        "byte shape divergence: pr2={} legacy={}",
        pr2, legacy
    );
}

// --- Phase B: webp SOURCE responsive ladder (Task 12) -----------------
//
// A webp source is already webp (to_webp(src) == src), so the ladder rides
// the `<img>` directly via `srcset`+`sizes` — NO `<picture>` wrap (a
// `<source>` identical to the img would be pointless). A wide, non-animated
// webp gains the ladder; small / animated / unknown-dims webp stays
// byte-identical to the pre-Task-12 bare `<img>`. The `sizes=` value is the
// SAME per-context mapping png/jpg/jpeg use.

/// Build a snapshot carrying dims AND an explicit animated flag for the
/// source path — the animated map is keyed like `dimensions`.
fn snapshot_dims_animated(path: &str, w: u32, h: u32, animated: bool) -> AssetSnapshot {
    let mut s = snapshot_dims(path, w, h);
    s.animated.insert(PathBuf::from(path), animated);
    s
}

#[test]
fn webp_source_wide_emits_img_srcset_no_picture() {
    // The yinlab.io case: a non-animated 1866×1866 webp original. Exact
    // byte shape — srcset on the <img>, no <picture>, base descriptor is
    // `photo.webp` itself at the deployed width (1866, under the cap).
    let assets = snapshot_dims("photo.webp", 1866, 1866);
    let html = synthesize_image_html(
        "photo.webp",
        "alt",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img src="photo.webp" srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 1866w" sizes="(min-width: 48rem) 47.25rem, 100vw" width="1866" height="1866" loading="lazy" alt="alt" />"#
    );
    assert!(
        !html.contains("<picture"),
        "webp source must NOT be wrapped in <picture>: {html}"
    );
}

#[test]
fn webp_source_small_is_byte_identical_bare_img() {
    // 600×400: deployed_width == 600, no rung < 600 → empty ladder →
    // byte-identical to today's bare <img> for a small webp (no srcset,
    // no <picture>, no sizes).
    let assets = snapshot_dims("photo.webp", 600, 400);
    let html = synthesize_image_html(
        "photo.webp",
        "alt",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img src="photo.webp" width="600" height="400" loading="lazy" alt="alt" />"#
    );
}

#[test]
fn webp_source_animated_is_byte_identical_bare_img() {
    // A 1866×1866 webp FLAGGED animated in the snapshot (Task 9/10 scan)
    // gets NO ladder — animation-preserving multi-size re-encode is out of
    // scope, and the pipeline's should_skip drops animated webp, so
    // emitting rungs would 404. Byte-identical to the bare <img>, despite
    // dims that would otherwise ladder.
    let assets = snapshot_dims_animated("loop.webp", 1866, 1866, true);
    let html = synthesize_image_html(
        "loop.webp",
        "alt",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert_eq!(
        html,
        r#"<img src="loop.webp" width="1866" height="1866" loading="lazy" alt="alt" />"#
    );
    assert!(
        !html.contains("srcset"),
        "animated webp must not carry srcset: {html}"
    );
}

#[test]
fn webp_source_explicit_non_animated_flag_ladders() {
    // Symmetry with the animated test: an explicit `false` in the snapshot
    // (present-false key) ladders exactly like a missing key.
    let assets = snapshot_dims_animated("photo.webp", 2000, 1200, false);
    let html = synthesize_image_html(
        "photo.webp",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 2000w""#),
        "got: {html}"
    );
}

#[test]
fn webp_source_card_context_uses_card_sizes() {
    let assets = snapshot_dims("cover.webp", 2000, 1200);
    let html = synthesize_image_html(
        "cover.webp",
        "",
        &assets,
        ImageContext::FolderCardCover,
        &ImageRenderOptions::default(),
    );
    assert!(
            html.contains(
                r#"srcset="cover.w800.webp 800w, cover.w1600.webp 1600w, cover.webp 2000w" sizes="(min-width: 48rem) 24rem, 100vw""#
            ),
            "got: {html}"
        );
    assert!(!html.contains("<picture"), "got: {html}");
}

#[test]
fn webp_source_gallery_context_uses_gallery_sizes() {
    let assets = snapshot_dims("g.webp", 2000, 1200);
    let html = synthesize_image_html(
        "g.webp",
        "",
        &assets,
        ImageContext::GalleryThumb,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"sizes="(min-width: 48rem) 33vw, 100vw""#),
        "got: {html}"
    );
    assert!(!html.contains("<picture"), "got: {html}");
}

#[test]
fn webp_source_hero_context_uses_full_bleed_sizes() {
    let assets = snapshot_dims("hero.webp", 2400, 985);
    let html = synthesize_image_html(
        "hero.webp",
        "",
        &assets,
        ImageContext::Hero,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(
            r#"srcset="hero.w800.webp 800w, hero.w1600.webp 1600w, hero.webp 2400w" sizes="100vw""#
        ),
        "got: {html}"
    );
}

#[test]
fn webp_source_portrait_base_descriptor_uses_post_resize_width() {
    // 3024×4032 portrait deploys at 1800×2400 — the base descriptor must be
    // the POST-RESIZE width (1800w), and width/height attrs stay natural
    // (aspect ratio hint), exactly like the png/jpg portrait picture path.
    let assets = snapshot_dims("photo.webp", 3024, 4032);
    let html = synthesize_image_html(
        "photo.webp",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 1800w""#),
        "got: {html}"
    );
    assert!(
        html.contains(r#"width="3024" height="4032""#),
        "got: {html}"
    );
}

#[test]
fn webp_source_unknown_dims_is_bare_img() {
    // Empty snapshot → dims miss → no ladder → bare <img> (fallback dims,
    // no srcset, no <picture>). Matches the test/fragment-render path.
    let html = synthesize_image_html(
        "photo.webp",
        "",
        &AssetSnapshot::new(),
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(!html.contains("srcset"), "got: {html}");
    assert!(!html.contains("<picture"), "got: {html}");
    assert!(html.starts_with(r#"<img src="photo.webp""#), "got: {html}");
}

#[test]
fn webp_source_uppercase_ext_ladders_and_uses_src_case() {
    // is_webp_source is case-insensitive; the emitted rung/base URLs derive
    // from `src` (to_webp_rung lowercases only the extension it appends).
    let assets = snapshot_dims("photo.WEBP", 2000, 1200);
    let html = synthesize_image_html(
        "photo.WEBP",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("<picture"),
        "uppercase .WEBP must ladder as <img srcset>: {html}"
    );
    assert!(
        html.contains(r#"srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.WEBP 2000w""#),
        "got: {html}"
    );
}

#[test]
fn webp_source_lqip_style_survives_on_laddered_img() {
    // The LQIP inline style is emitted on the webp <img> exactly as it is
    // for the png/jpg inner <img> — the srcset addition doesn't suppress it.
    let assets = snapshot_with(
        "photo.webp",
        Some((2000, 1200)),
        None,
        Some("data:image/jpeg;base64,abc"),
        false,
    );
    let html = synthesize_image_html(
        "photo.webp",
        "",
        &assets,
        ImageContext::MarkdownInline,
        &ImageRenderOptions::default(),
    );
    assert!(
        html.contains(r#"srcset="photo.w800.webp 800w"#),
        "got: {html}"
    );
    assert!(
        html.contains(
            r#"style="background-image:url(data:image/jpeg;base64,abc);background-size:cover""#
        ),
        "LQIP must survive on the laddered webp <img>: {html}"
    );
}

#[test]
fn webp_source_in_email_body_never_leaks_srcset() {
    // EmailBody short-circuits BEFORE synthesize_inner, so even a >800px
    // webp (which WOULD ladder in a page context) emits the flat, email-
    // client-safe <img> — no srcset, no <picture>. Email clients support
    // neither; the webp ladder must never leak into an email body.
    let assets = snapshot_dims("photo.webp", 2000, 1200);
    let html = synthesize_image_html(
        "photo.webp",
        "alt",
        &assets,
        ImageContext::EmailBody {
            width: Some(2000),
            height: Some(1200),
        },
        &ImageRenderOptions::default(),
    );
    assert!(
        !html.contains("srcset"),
        "email webp must never carry srcset: {html}"
    );
    assert!(
        !html.contains("<picture"),
        "email webp must never wrap in <picture>: {html}"
    );
    assert!(
        html.contains(r#"style="display:block;max-width:100%;height:auto;""#),
        "email <img> shape preserved: {html}"
    );
}
