//! Cover type detection and HTML rendering for card covers.
//!
//! Supports image, video, and iframe cover types with automatic detection
//! from file extensions or explicit override via frontmatter `cover_type`.

/// The type of media used for a cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverType {
    Image,
    Video,
    Iframe,
}

impl CoverType {
    pub fn resolve(cover: Option<&str>, explicit: Option<CoverType>) -> Option<Self> {
        cover.map(|_| explicit.unwrap_or(CoverType::Image))
    }
}

/// Detect the cover type from a URL's file extension, with optional explicit override.
///
/// If `explicit` is provided, it takes priority:
/// - `"video"` → `Video`
/// - `"iframe"` → `Iframe`
/// - anything else → `Image`
///
/// Otherwise, the extension of `url` is used:
/// - `.jpg`, `.jpeg`, `.png`, `.gif`, `.webp`, `.avif`, `.svg` → `Image`
/// - `.mp4`, `.webm`, `.mov` → `Video`
/// - `.html`, `.htm` → `Iframe`
/// - unknown or missing extension → `Image`
pub fn detect_cover_type(url: &str, explicit: Option<&str>) -> CoverType {
    if let Some(e) = explicit {
        return match e.to_ascii_lowercase().as_str() {
            "video" => CoverType::Video,
            "iframe" => CoverType::Iframe,
            _ => CoverType::Image,
        };
    }

    // Strip query string and fragment before extracting extension
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "svg" => CoverType::Image,
        "mp4" | "webm" | "mov" | "m4v" => CoverType::Video,
        "html" | "htm" => CoverType::Iframe,
        _ => CoverType::Image,
    }
}

/// Render the appropriate HTML for a cover element.
///
/// When `attrs` has non-default values, an inline `style` attribute is added to
/// `<img>` and `<video>` tags with `object-fit` / `object-position`.
/// Iframe covers ignore media attrs (iframes have their own sizing).
///
/// All parameters are HTML-escaped in the output.
pub fn render_cover_html(
    url: &str,
    cover_type: CoverType,
    alt: &str,
    css_class: &str,
    attrs: &moss_core::media::MediaAttrs,
    block_interaction: bool,
    // Step 6 of structural-html-emission: when `Some`, the `CoverType::Image`
    // branch routes the `<img>` through `image_render::synthesize_image_html`
    // with `ImageContext::FolderCardCover`. The synthesizer bakes
    // dimensions, LQIP, and the `<picture><source>` WebP wrap into the
    // emission at synthesis time. When `None`, falls back to the prior
    // raw `<img>` shape and the downstream regex pass retrofits the attrs
    // — preserves behavior for test/fragment-render contexts.
    //
    // When `eager` is true, the synthesizer emits
    // `loading="eager" fetchpriority="high"` instead of `loading="lazy"`.
    // Used for LCP optimization: grid_card's render_list_with_typesetting
    // marks the first card with a cover as eager so the browser fetches
    // the LCP image immediately. The pre-Step-6 implementation did this
    // via an `html.insert_str` of `loading="eager"` after the render —
    // Step 6 moves the decision into a typed parameter on the
    // synthesizer call.
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    eager: bool,
) -> String {
    let alt_escaped = html_escape(alt);
    let css_class = html_escape(css_class);
    let style_attr = attrs
        .to_inline_style()
        .map(|s| format!(r#" style="{}""#, html_escape(&s)))
        .unwrap_or_default();

    match cover_type {
        CoverType::Image => {
            if let Some(lookup) = media_lookup {
                let extra = if style_attr.is_empty() {
                    None
                } else {
                    Some(style_attr.trim_start().to_string())
                };
                let opts = moss_core::render::image::ImageRenderOptions {
                    eager,
                    extra_attrs: extra.as_deref(),
                    ..Default::default()
                };
                // Phase 1 B1 (2026-05-25): synthesizer takes `&AssetSnapshot`.
                // Memoized on the lookup — this built one snapshot per CARD
                // until 2026-08-24, which cost cards x images. No
                // `AssetRegistry` available at this boundary, so variant kinds
                // stay empty (the synthesizer still wraps raster originals in
                // `<picture>` via `is_raster_original` by file extension).
                //
                // BUG 6: the snapshot indexes keys under the build's REAL
                // `dir_overrides` (carried by the lookup), not an empty map.
                // Covers arrive under the OVERRIDE slug (e.g. a CJK folder
                // mapped to a custom English slug); an empty map would index
                // only source-key + base-slug, so the override-slug probe would
                // miss and re-fire the 800x600 fallback for card/folder covers.
                let assets = lookup.asset_snapshot();
                let img_html = moss_core::render::image::synthesize_image_html(
                    url,
                    alt,
                    assets,
                    moss_core::render::image::ImageContext::FolderCardCover,
                    &opts,
                );
                return format!(r#"<div class="{css_class}">{img_html}</div>"#);
            }
            let url = html_escape(url);
            // No-manifest fallback: emit bare <img>; downstream regex adds
            // attrs (data-placeholder-src, width, height, lazy/eager, LQIP).
            // The eager hint is emitted here so the regex pass's
            // `has_loading` short-circuit sees it and skips its own
            // `loading="lazy"` injection — and `fetchpriority="high"` is
            // added by the regex when it detects `loading="eager"`.
            // Pre-Step-6 the LCP hint was applied via a post-render
            // `html.insert_str("loading=\"eager\" ")` mutation in
            // `grid_card::render_list_with_typesetting`; Step 6 moves the
            // decision into the emitter where it belongs.
            let eager_attr = if eager { r#" loading="eager""# } else { "" };
            format!(r#"<div class="{css_class}"><img src="{url}" alt="{alt_escaped}"{eager_attr}{style_attr} /></div>"#)
        }
        CoverType::Video => {
            let src = html_escape(&moss_core::asset_paths::to_mp4(url));
            let poster = html_escape(&moss_core::asset_paths::to_thumb(url));
            // keep in sync with moss_core::render::video::AMBIENT_PLAYBACK_ATTRS
            // (covers intentionally omit `autoplay` — they are hover-played, not auto)
            format!(
                r#"<div class="{css_class}"><video src="{src}"{style_attr} muted loop playsinline preload="metadata"></video><img src="{poster}" alt="{alt_escaped}" class="cover-thumb"{style_attr} /></div>"#
            )
        }
        CoverType::Iframe => {
            let url = html_escape(url);
            // Defense-in-depth: carry `position:relative` inline alongside the
            // overlay so it anchors correctly even if a caller's CSS class
            // forgets to set a positioning context. Every current caller's CSS
            // (.moss-card-cover, .moss-collection-cover) already sets position:relative, so the
            // inline style is redundant for them — but cheap insurance for new
            // call sites and keeps existing HTML snapshots byte-stable.
            let (container_style, overlay) = if block_interaction {
                (
                    r#" style="position:relative""#,
                    r#"<div style="position:absolute;inset:0"></div>"#,
                )
            } else {
                ("", "")
            };
            format!(
                r#"<div class="{css_class}"{container_style}><iframe src="{url}" title="{alt_escaped}" loading="lazy" sandbox="allow-scripts"></iframe>{overlay}</div>"#
            )
        }
    }
}

pub(crate) use moss_core::media::html_escape;

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_cover_type ────────────────────────────────────────

    #[test]
    fn detect_image_extensions() {
        for ext in &["jpg", "jpeg", "png", "gif", "webp", "avif", "svg"] {
            let url = format!("photo.{ext}");
            assert_eq!(detect_cover_type(&url, None), CoverType::Image, "ext: {ext}");
        }
    }

    #[test]
    fn detect_video_extensions() {
        for ext in &["mp4", "webm", "mov", "m4v"] {
            let url = format!("clip.{ext}");
            assert_eq!(detect_cover_type(&url, None), CoverType::Video, "ext: {ext}");
        }
    }

    #[test]
    fn detect_iframe_extensions() {
        for ext in &["html", "htm"] {
            let url = format!("widget.{ext}");
            assert_eq!(detect_cover_type(&url, None), CoverType::Iframe, "ext: {ext}");
        }
    }

    #[test]
    fn detect_unknown_defaults_to_image() {
        assert_eq!(detect_cover_type("https://example.com/thing", None), CoverType::Image);
        assert_eq!(detect_cover_type("no-extension", None), CoverType::Image);
    }

    #[test]
    fn detect_case_insensitive() {
        assert_eq!(detect_cover_type("photo.JPG", None), CoverType::Image);
        assert_eq!(detect_cover_type("clip.MP4", None), CoverType::Video);
    }

    #[test]
    fn detect_strips_query_and_fragment() {
        assert_eq!(detect_cover_type("video.mp4?t=10", None), CoverType::Video);
        assert_eq!(detect_cover_type("photo.png#section", None), CoverType::Image);
    }

    #[test]
    fn explicit_override_takes_priority() {
        assert_eq!(detect_cover_type("photo.png", Some("video")), CoverType::Video);
        assert_eq!(detect_cover_type("clip.mp4", Some("iframe")), CoverType::Iframe);
        assert_eq!(detect_cover_type("widget.html", Some("image")), CoverType::Image);
    }

    #[test]
    fn explicit_override_case_insensitive() {
        assert_eq!(detect_cover_type("x", Some("Video")), CoverType::Video);
        assert_eq!(detect_cover_type("x", Some("IFRAME")), CoverType::Iframe);
    }

    #[test]
    fn explicit_unknown_defaults_to_image() {
        assert_eq!(detect_cover_type("x", Some("something")), CoverType::Image);
    }

    // ── render_cover_html ────────────────────────────────────────

    use moss_core::media::{Fit, MediaAttrs, Position};

    fn empty_attrs() -> MediaAttrs {
        MediaAttrs {
            fit: None,
            position: None,
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn render_image() {
        let html = render_cover_html("photo.jpg", CoverType::Image, "A photo", "cover", &empty_attrs(), false, None, false);
        assert_eq!(
            html,
            r#"<div class="cover"><img src="photo.jpg" alt="A photo" /></div>"#
        );
    }

    #[test]
    fn render_video() {
        let html = render_cover_html("clip.mp4", CoverType::Video, "A clip", "cover", &empty_attrs(), false, None, false);
        assert_eq!(
            html,
            r#"<div class="cover"><video src="clip.mp4" muted loop playsinline preload="metadata"></video><img src="clip.thumb.jpg" alt="A clip" class="cover-thumb" /></div>"#
        );
    }

    #[test]
    fn render_iframe() {
        let html = render_cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), true, None, false);
        assert_eq!(
            html,
            r#"<div class="cover" style="position:relative"><iframe src="widget.html" title="Widget" loading="lazy" sandbox="allow-scripts"></iframe><div style="position:absolute;inset:0"></div></div>"#
        );
    }

    #[test]
    fn render_iframe_has_title_attribute() {
        let html = render_cover_html("widget.html", CoverType::Iframe, "My Widget", "cover", &empty_attrs(), true, None, false);
        assert!(
            html.contains(r#"title="My Widget""#),
            "Iframe should have title attribute for accessibility. Got: {}",
            html
        );
    }

    #[test]
    fn render_iframe_escapes_title() {
        let html = render_cover_html("widget.html", CoverType::Iframe, "A <script> & \"test\"", "cover", &empty_attrs(), true, None, false);
        assert!(
            html.contains(r#"title="A &lt;script&gt; &amp; &quot;test&quot;""#),
            "Iframe title should be HTML-escaped. Got: {}",
            html
        );
    }

    #[test]
    fn render_image_with_eager_loading() {
        let html = render_cover_html("photo.jpg", CoverType::Image, "A photo", "cover", &empty_attrs(), false, None, false);
        // Default: no loading or fetchpriority attributes (placeholder.rs handles these)
        assert!(!html.contains("loading="), "Default image should not have loading attr. Got: {}", html);
    }

    #[test]
    fn render_escapes_html_in_url() {
        let html = render_cover_html("x.jpg?a=1&b=2", CoverType::Image, "alt", "c", &empty_attrs(), false, None, false);
        assert!(html.contains("a=1&amp;b=2"));
    }

    #[test]
    fn render_escapes_html_in_alt() {
        let html = render_cover_html("x.jpg", CoverType::Image, "<script>", "c", &empty_attrs(), false, None, false);
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn render_escapes_quotes_in_url() {
        let html = render_cover_html(r#"x"y.jpg"#, CoverType::Image, "alt", "c", &empty_attrs(), false, None, false);
        assert!(html.contains("x&quot;y.jpg"));
    }

    // ── BUG 6: cover under a dir-override resolves real dims ─────

    #[test]
    fn render_image_cover_under_dir_override_emits_real_dims_not_fallback() {
        // The reviewer's scenario: a folder mapped to a custom slug via
        // dir_overrides. The emitter produces the cover URL under the OVERRIDE
        // slug (`assets/euro/...`), while the scan inventory keys dims under the
        // raw source path (`assets/Europe - A Prophecy/...`). The lookup carries
        // the overrides so `build_asset_snapshot` additively indexes the dims
        // under the override-slug output-URL key the cover URL probes with.
        // Without threading the overrides (an empty map), the snapshot would
        // only hold source-key + base-slug (`europe-a-prophecy`), the
        // override-slug probe would MISS, and the 800x600 fallback would fire.
        use crate::build::media::dimensions::MediaDimensionLookup;
        use crate::types::content::MediaMetadata;
        let meta = MediaMetadata {
            is_animated: false,
            path: "assets/Europe - A Prophecy/e-006.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 0,
            modified: None,
            dimensions: Some((4515, 6158)),
            dominant_color: None,
            lqip_data_uri: None,
        };
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("assets/Europe - A Prophecy".to_string(), "euro".to_string());
        let lookup = MediaDimensionLookup::new(&[meta], &[], &overrides, None);

        // The emitter resolves the cover to the OVERRIDE slug.
        let html = render_cover_html(
            "assets/euro/e-006.jpg",
            CoverType::Image,
            "alt",
            "cover",
            &empty_attrs(),
            false,
            Some(&lookup),
            false,
        );
        assert!(
            html.contains(r#"width="4515""#) && html.contains(r#"height="6158""#),
            "override-slug cover must carry real dims, got: {}", html
        );
        assert!(
            !(html.contains(r#"width="800""#) && html.contains(r#"height="600""#)),
            "800x600 fallback must NOT fire for a dir-override cover, got: {}", html
        );
    }

    // ── render_cover_html with MediaAttrs ───────────────────────

    #[test]
    fn render_image_with_position_only() {
        let attrs = MediaAttrs {
            fit: None,
            position: Some(Position::Left),
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        };
        let html = render_cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, false);
        assert!(html.contains(r#"style="object-position:left""#), "Got: {}", html);
        assert!(html.contains(r#"<img src="photo.jpg""#));
    }

    #[test]
    fn render_image_with_fit_only() {
        let attrs = MediaAttrs {
            fit: Some(Fit::Contain),
            position: None,
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        };
        let html = render_cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, false);
        assert!(html.contains(r#"style="object-fit:contain""#), "Got: {}", html);
    }

    #[test]
    fn render_image_with_both_fit_and_position() {
        let attrs = MediaAttrs {
            fit: Some(Fit::Contain),
            position: Some(Position::Left),
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        };
        let html = render_cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, false);
        assert!(html.contains(r#"style="object-fit:contain;object-position:left""#), "Got: {}", html);
    }

    #[test]
    fn render_image_empty_attrs_no_style() {
        let html = render_cover_html("photo.jpg", CoverType::Image, "alt", "cover", &empty_attrs(), false, None, false);
        assert!(!html.contains("style="), "Empty attrs should not add style. Got: {}", html);
    }

    #[test]
    fn render_video_with_attrs() {
        let attrs = MediaAttrs {
            fit: None,
            position: Some(Position::Top),
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        };
        let html = render_cover_html("clip.mp4", CoverType::Video, "alt", "cover", &attrs, false, None, false);
        assert!(html.contains(r#"<video src="clip.mp4" style="object-position:top""#), "Got: {}", html);
        // Poster thumbnail should also get the same style
        assert!(html.contains(r#"class="cover-thumb" style="object-position:top""#), "Poster thumb should get style. Got: {}", html);
    }

    #[test]
    fn render_iframe_ignores_attrs() {
        let attrs = MediaAttrs {
            fit: Some(Fit::Cover),
            position: Some(Position::Center),
            align: None,
            color: None,
            class_names: Vec::new(),
            extra_attrs: std::collections::BTreeMap::new(),
        };
        let html = render_cover_html("widget.html", CoverType::Iframe, "alt", "cover", &attrs, true, None, false);
        // Iframe should NOT have object-fit/object-position
        assert!(!html.contains("object-fit"), "Iframe should ignore media attrs. Got: {}", html);
        assert!(!html.contains("object-position"), "Iframe should ignore media attrs. Got: {}", html);
    }

    // ── block_interaction ─────────────────────────────────────────

    #[test]
    fn render_iframe_blocked() {
        let html = render_cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), true, None, false);
        assert!(
            html.contains(r#"<div style="position:absolute;inset:0"></div>"#),
            "block_interaction=true should emit overlay. Got: {}", html
        );
    }

    #[test]
    fn render_iframe_interactive() {
        let html = render_cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), false, None, false);
        assert!(
            !html.contains(r#"position:absolute"#),
            "block_interaction=false should NOT emit overlay. Got: {}", html
        );
        // iframe itself should still be present
        assert!(html.contains("<iframe"), "iframe element should still render. Got: {}", html);
    }

    // ── CoverType::resolve ───────────────────────────────────────

    #[test]
    fn cover_type_resolve() {
        assert_eq!(CoverType::resolve(Some("photo.jpg"), None), Some(CoverType::Image));
        assert_eq!(CoverType::resolve(Some("clip.mp4"), Some(CoverType::Video)), Some(CoverType::Video));
        assert_eq!(CoverType::resolve(None, Some(CoverType::Video)), None);
        assert_eq!(CoverType::resolve(None, None), None);
    }
}
