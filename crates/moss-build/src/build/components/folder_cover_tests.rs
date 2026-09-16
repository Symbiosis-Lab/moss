use super::*;
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
fn test_render_image_cover_wraps_content_in_row() {
    let html = render(
        Some("assets/photo.jpg"),
        "Travel",
        "<p>Hello</p>",
        CoverType::Image,
        &empty_attrs(),
        None,
        false,
    );
    assert!(
        html.contains(r#"<div class="moss-collection-cover-row">"#),
        "Got: {}",
        html
    );
    assert!(
        html.contains(r#"<div class="moss-collection-cover"><img src="assets/photo.jpg""#),
        "Got: {}",
        html
    );
    assert!(html.contains(r#"alt="Travel cover""#), "Got: {}", html);
    assert!(
            html.contains(r#"<div class="moss-collection-cover-body"><h1 class="moss-folder-title">Travel</h1><p>Hello</p></div>"#),
            "Got: {}", html,
        );
}

#[test]
fn test_render_video_cover_uses_thumbnail() {
    let html = render(
        Some("clip.mp4"),
        "Demo",
        "<p>text</p>",
        CoverType::Video,
        &empty_attrs(),
        None,
        false,
    );
    assert!(
        html.contains("clip.thumb.jpg"),
        "Video should use thumbnail. Got: {}",
        html
    );
    assert!(
        html.contains(r#"<div class="moss-collection-cover">"#),
        "Got: {}",
        html
    );
}

#[test]
fn test_render_iframe_cover() {
    let html = render(
        Some("widget.html"),
        "Interactive",
        "<p>text</p>",
        CoverType::Iframe,
        &empty_attrs(),
        None,
        false,
    );
    assert!(
        html.contains("<iframe"),
        "Iframe cover should render iframe. Got: {}",
        html
    );
    assert!(html.contains("widget.html"), "Got: {}", html);
    assert!(
        html.contains("sandbox="),
        "Iframe should be sandboxed. Got: {}",
        html
    );
}

#[test]
fn editor_preview_cover_wrapper_names_the_cover_field() {
    // The media div (and only the media div) says it renders from `cover:`.
    // The row must stay bare — an fm attribute there would swallow every
    // body-text click inside the cover row (fm outranks data-source-line
    // in the bridge's resolveSourceTarget).
    for (path, ct) in [("assets/photo.jpg", CoverType::Image), ("clip.mp4", CoverType::Video)] {
        let html = render(
            Some(path),
            "Travel",
            "<p>Hello</p>",
            ct,
            &empty_attrs(),
            None,
            true,
        );
        assert!(
            html.contains(r#"<div class="moss-collection-cover" data-source-fm="cover">"#),
            "Got: {}",
            html
        );
        assert!(
            html.contains(r#"<div class="moss-collection-cover-row">"#),
            "row must not carry the fm attribute. Got: {}",
            html
        );
        assert!(
            html.contains(r#"data-source-fm="title""#),
            "heading annotation must survive alongside cover's. Got: {}",
            html
        );
    }

    // Publish shape: no annotations at all.
    let off = render(
        Some("assets/photo.jpg"),
        "Travel",
        "<p>Hello</p>",
        CoverType::Image,
        &empty_attrs(),
        None,
        false,
    );
    assert!(!off.contains("data-source-fm"), "Got: {}", off);
}

#[test]
fn test_render_without_cover_returns_content_unchanged() {
    let html = render(
        None,
        "Travel",
        "<p>Hello</p>",
        CoverType::Image,
        &empty_attrs(),
        None,
        false,
    );
    assert_eq!(html, "<p>Hello</p>");
}

#[test]
fn test_render_escapes_html_in_label() {
    // Label must be escaped for both the alt attribute and the visible h1.
    let html = render(
        Some("cover.jpg"),
        "Code & Tips",
        "",
        CoverType::Image,
        &empty_attrs(),
        None,
        false,
    );
    assert!(
        html.contains("Code &amp; Tips cover"),
        "alt escaping; got: {}",
        html
    );
    assert!(
        html.contains(r#"<h1 class="moss-folder-title">Code &amp; Tips</h1>"#),
        "h1 should contain escaped label; got: {}",
        html,
    );
}

#[test]
fn test_render_with_media_attrs() {
    let attrs = MediaAttrs {
        fit: None,
        position: Some(Position::Left),
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: std::collections::BTreeMap::new(),
    };
    let html = render(
        Some("photo.jpg"),
        "Travel",
        "<p>Hello</p>",
        CoverType::Image,
        &attrs,
        None,
        false,
    );
    assert!(
        html.contains(r#"style="object-position:left""#),
        "Got: {}",
        html
    );
    assert!(html.contains(r#"<img src="photo.jpg""#), "Got: {}", html);
}

#[test]
fn test_render_with_fit_and_position() {
    let attrs = MediaAttrs {
        fit: Some(Fit::Contain),
        position: Some(Position::Top),
        align: None,
        color: None,
        class_names: Vec::new(),
        extra_attrs: std::collections::BTreeMap::new(),
    };
    let html = render(
        Some("photo.jpg"),
        "Travel",
        "<p>text</p>",
        CoverType::Image,
        &attrs,
        None,
        false,
    );
    assert!(
        html.contains(r#"style="object-fit:contain;object-position:top""#),
        "Got: {}",
        html
    );
}
