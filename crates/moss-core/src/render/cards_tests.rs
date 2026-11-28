use super::*;
use crate::page::card::detect_cover_type;


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

use crate::media::{Fit, MediaAttrs, Position};

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
    let html = cover_html("photo.jpg", CoverType::Image, "A photo", "cover", &empty_attrs(), false, None, Loading::Lazy, None);
    assert_eq!(
        html,
        r#"<div class="cover"><img src="photo.jpg" alt="A photo" /></div>"#
    );
}

#[test]
fn render_video() {
    let html = cover_html("clip.mp4", CoverType::Video, "A clip", "cover", &empty_attrs(), false, None, Loading::Lazy, None);
    assert_eq!(
        html,
        r#"<div class="cover"><video src="clip.mp4" muted loop playsinline preload="metadata"></video><img src="clip.thumb.jpg" alt="A clip" class="cover-thumb" /></div>"#
    );
}

#[test]
fn render_iframe() {
    let html = cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), true, None, Loading::Lazy, None);
    assert_eq!(
        html,
        r#"<div class="cover" style="position:relative"><iframe src="widget.html" title="Widget" loading="lazy" sandbox="allow-scripts"></iframe><div style="position:absolute;inset:0"></div></div>"#
    );
}

#[test]
fn render_iframe_has_title_attribute() {
    let html = cover_html("widget.html", CoverType::Iframe, "My Widget", "cover", &empty_attrs(), true, None, Loading::Lazy, None);
    assert!(
        html.contains(r#"title="My Widget""#),
        "Iframe should have title attribute for accessibility. Got: {}",
        html
    );
}

#[test]
fn render_iframe_escapes_title() {
    let html = cover_html("widget.html", CoverType::Iframe, "A <script> & \"test\"", "cover", &empty_attrs(), true, None, Loading::Lazy, None);
    assert!(
        html.contains(r#"title="A &lt;script&gt; &amp; &quot;test&quot;""#),
        "Iframe title should be HTML-escaped. Got: {}",
        html
    );
}

#[test]
fn a_lazy_cover_without_a_manifest_carries_no_loading_attribute() {
    let html = cover_html("photo.jpg", CoverType::Image, "A photo", "cover", &empty_attrs(), false, None, Loading::Lazy, None);
    assert!(!html.contains("loading="), "Default image should not have loading attr. Got: {}", html);
}

#[test]
fn render_escapes_html_in_url() {
    let html = cover_html("x.jpg?a=1&b=2", CoverType::Image, "alt", "c", &empty_attrs(), false, None, Loading::Lazy, None);
    assert!(html.contains("a=1&amp;b=2"));
}

#[test]
fn render_escapes_html_in_alt() {
    let html = cover_html("x.jpg", CoverType::Image, "<script>", "c", &empty_attrs(), false, None, Loading::Lazy, None);
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn render_escapes_quotes_in_url() {
    let html = cover_html(r#"x"y.jpg"#, CoverType::Image, "alt", "c", &empty_attrs(), false, None, Loading::Lazy, None);
    assert!(html.contains("x&quot;y.jpg"));
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
    let html = cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, Loading::Lazy, None);
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
    let html = cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, Loading::Lazy, None);
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
    let html = cover_html("photo.jpg", CoverType::Image, "alt", "cover", &attrs, false, None, Loading::Lazy, None);
    assert!(html.contains(r#"style="object-fit:contain;object-position:left""#), "Got: {}", html);
}

#[test]
fn render_image_empty_attrs_no_style() {
    let html = cover_html("photo.jpg", CoverType::Image, "alt", "cover", &empty_attrs(), false, None, Loading::Lazy, None);
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
    let html = cover_html("clip.mp4", CoverType::Video, "alt", "cover", &attrs, false, None, Loading::Lazy, None);
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
    let html = cover_html("widget.html", CoverType::Iframe, "alt", "cover", &attrs, true, None, Loading::Lazy, None);
    // Iframe should NOT have object-fit/object-position
    assert!(!html.contains("object-fit"), "Iframe should ignore media attrs. Got: {}", html);
    assert!(!html.contains("object-position"), "Iframe should ignore media attrs. Got: {}", html);
}

// ── block_interaction ─────────────────────────────────────────

#[test]
fn render_iframe_blocked() {
    let html = cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), true, None, Loading::Lazy, None);
    assert!(
        html.contains(r#"<div style="position:absolute;inset:0"></div>"#),
        "block_interaction=true should emit overlay. Got: {}", html
    );
}

#[test]
fn render_iframe_interactive() {
    let html = cover_html("widget.html", CoverType::Iframe, "Widget", "cover", &empty_attrs(), false, None, Loading::Lazy, None);
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
