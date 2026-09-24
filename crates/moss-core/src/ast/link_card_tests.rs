use super::*;
use crate::ast::url::UrlKind;

fn resolved(href: &str) -> ResolvedUrl {
    ResolvedUrl::new(href, UrlKind::External)
}

fn image_para(src: &str, alt: &str) -> Block {
    Block::Paragraph(vec![Inline::Image {
        src: Url::resolved(src, UrlKind::Asset),
        alt: alt.to_string(),
        title: None,
        is_wikilink: false,
        wikilink_pothole: None,
    }])
}

fn heading(text: &str) -> Block {
    Block::Heading {
        level: 3,
        children: vec![Inline::Text(text.to_string())],
        id: None,
    }
}

#[test]
fn extract_domain_strips_scheme_and_www() {
    assert_eq!(extract_domain("https://www.example.com/a/b"), "example.com");
    assert_eq!(extract_domain("http://example.org"), "example.org");
}

#[test]
fn domain_and_path_drops_query_and_fragment() {
    assert_eq!(
        domain_and_path("https://example.com/a/b?x=1#frag"),
        "example.com/a/b"
    );
    assert_eq!(domain_and_path("https://example.com/"), "example.com");
}

#[test]
fn no_authored_content_falls_back_to_placeholder_and_domain_path_title() {
    let resolved = resolved("https://example.com/story");
    let html = render_external_link_card(&resolved, &[]);
    assert!(html.contains(r#"class="moss-card" data-external target="_blank" rel="noopener""#));
    assert!(html.contains(r#"<div class="moss-card-cover moss-card-no-cover"></div>"#));
    assert!(html.contains(r#"<span class="moss-card-title">example.com/story</span>"#));
    assert!(html.contains(r#"<span class="moss-card-kicker">example.com</span>"#));
    // Never the retired shell.
    assert!(!html.contains("moss-grid-card"));
    assert!(!html.contains("link-preview"));
}

#[test]
fn bare_authored_image_becomes_the_cover_not_the_title() {
    let resolved = resolved("https://example.com/story");
    let children = vec![image_para("photo.jpg", "A photo")];
    let html = render_external_link_card(&resolved, &children);
    assert!(html.contains(r#"<div class="moss-card-cover"><img src="photo.jpg" alt="A photo" /></div>"#));
    // No caption text left over, so title falls back to domain+path — the
    // alt text must NOT leak into the title slot.
    assert!(html.contains(r#"<span class="moss-card-title">example.com/story</span>"#));
}

#[test]
fn authored_image_with_heading_uses_heading_as_title() {
    let resolved = resolved("https://example.com/story");
    let children = vec![image_para("photo.jpg", "alt text"), heading("A real headline")];
    let html = render_external_link_card(&resolved, &children);
    assert!(html.contains(r#"<img src="photo.jpg" alt="alt text" />"#));
    assert!(html.contains(r#"<span class="moss-card-title">A real headline</span>"#));
}

#[test]
fn text_only_whole_cell_gets_no_cover_placeholder() {
    // A multi-line whole-cell link with no image at all (e.g. `[Some\ntext](url)`)
    // still reaches this renderer (see shortcode_extract's compound-link
    // detection) and must degrade to the plain placeholder, not panic on an
    // absent cover image.
    let resolved = resolved("https://example.com");
    let children = vec![Block::Paragraph(vec![Inline::Text("Some text".to_string())])];
    let html = render_external_link_card(&resolved, &children);
    assert!(html.contains(r#"<div class="moss-card-cover moss-card-no-cover"></div>"#));
    assert!(html.contains(r#"<span class="moss-card-title">Some text</span>"#));
}

#[test]
fn href_and_title_are_escaped() {
    let resolved = resolved("https://example.com/?q=\"x\"&y");
    let children = vec![Block::Paragraph(vec![Inline::Text("<script>".to_string())])];
    let html = render_external_link_card(&resolved, &children);
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}
