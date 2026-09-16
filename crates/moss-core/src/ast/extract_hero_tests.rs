use super::super::document::BlockMeta;
use super::super::hooks::DefaultHooks;
use super::super::node::Inline;
use super::super::shortcode::HeroShortcode;
use super::super::url::{Url, UrlKind};
use super::*;

fn hero_block(image_url: Option<&str>, overlay: Vec<Block>) -> Block {
    Block::Shortcode(Shortcode::Hero(HeroShortcode {
        image: image_url.map(|u| Url::resolved(u, UrlKind::Asset)),
        extra_images: Vec::new(),
        attrs: String::new(),
        classes: String::new(),
        overlay,
        overlay_text: String::new(),
        width: None,
        mobile: None,
        caption: String::new(),
    }))
}

/// A caption belongs to the hero, not to whichever of the three syntaxes
/// named its image — so it is read the same way from all of them.
#[test]
fn caption_is_read_from_every_hero_syntax() {
    let cfg = ParseConfig::default();
    let cap = "Cover: the memorial wall (photo: A. Photographer)";

    let (attr_form, _, _) = parse_hero(&format!("{{image=cover.jpg caption=\"{cap}\"}}"), "", &cfg);
    assert_eq!(attr_form.caption, cap);
    assert_eq!(attr_form.overlay_text, "", "a caption is not overlay text");

    let (directive_form, _, _) = parse_hero(&format!("./cover.jpg {{caption=\"{cap}\"}}"), "", &cfg);
    assert_eq!(directive_form.caption, cap);

    let (body_form, _, _) = parse_hero(&format!("{{caption=\"{cap}\"}}"), "![[cover.jpg]]\n", &cfg);
    assert_eq!(body_form.caption, cap);

    let (plain, _, _) = parse_hero("{image=cover.jpg}", "", &cfg);
    assert_eq!(plain.caption, "", "no caption written, none invented");
}

#[test]
fn extract_hero_returns_none_when_no_hero_present() {
    let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Text("plain".into())])]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks);
    assert!(extraction.is_none());
    assert_eq!(doc.blocks.len(), 1, "doc unchanged when no hero");
}

#[test]
fn extract_hero_removes_top_level_hero_from_doc() {
    let mut doc = Document::from_blocks(vec![
        hero_block(Some("hero.jpg"), vec![]),
        Block::Paragraph(vec![Inline::Text("body".into())]),
    ]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert!(!extraction.html.is_empty(), "hero rendered");
    assert_eq!(extraction.image_url.as_deref(), Some("hero.jpg"));
    assert_eq!(doc.blocks.len(), 1, "hero removed from doc");
    assert!(matches!(&doc.blocks[0], Block::Paragraph(_)));
}

#[test]
fn extract_hero_captures_image_url_from_resolved_url() {
    let mut doc = Document::from_blocks(vec![hero_block(Some("path/to/hero.webp"), vec![])]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert_eq!(extraction.image_url.as_deref(), Some("path/to/hero.webp"));
}

#[test]
fn extract_hero_image_url_none_when_no_hero_image() {
    let mut doc = Document::from_blocks(vec![hero_block(None, vec![])]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert!(extraction.image_url.is_none());
}

#[test]
fn extract_hero_overlay_text_walks_typed_blocks() {
    // overlay carries typed paragraphs; first paragraph plain text
    // is the description-chain feed.
    let overlay = vec![Block::Paragraph(vec![
        Inline::Text("Hello, ".into()),
        Inline::Strong(vec![Inline::Text("world".into())]),
    ])];
    let mut doc = Document::from_blocks(vec![hero_block(None, overlay)]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert_eq!(extraction.overlay_text.as_deref(), Some("Hello, world"));
}

#[test]
fn extract_hero_overlay_text_skips_leading_heading() {
    // A heading is not paragraph prose — the extractor advances to
    // the first paragraph block.
    let overlay = vec![
        Block::Heading {
            level: 2,
            children: vec![Inline::Text("Title".into())],
            id: None,
        },
        Block::Paragraph(vec![Inline::Text("Description".into())]),
    ];
    let mut doc = Document::from_blocks(vec![hero_block(None, overlay)]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert_eq!(extraction.overlay_text.as_deref(), Some("Description"));
}

#[test]
fn extract_hero_finds_only_first_hero_when_multiple_present() {
    // Hero-hoisting semantics: the FIRST hero wins; subsequent
    // hero blocks stay in the body (rare authoring shape; the body
    // renderer will then have to decide what to do with them).
    let mut doc = Document::from_blocks(vec![
        hero_block(Some("a.jpg"), vec![]),
        Block::Paragraph(vec![Inline::Text("middle".into())]),
        hero_block(Some("b.jpg"), vec![]),
    ]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert_eq!(extraction.image_url.as_deref(), Some("a.jpg"));
    // After extraction: paragraph + remaining second hero.
    assert_eq!(doc.blocks.len(), 2);
}

#[test]
fn extract_hero_passes_source_line_to_render_shortcode() {
    // The hoisted hero is placed in the article template slot and IS
    // visible/clickable in the preview — it needs data-source-range.
    let hero = hero_block(None, vec![]);
    let meta_with_line = BlockMeta {
        source_line: Some(3),
        ..BlockMeta::default()
    };
    let mut doc = Document::from_blocks_with_meta(vec![hero], vec![meta_with_line]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert!(
        extraction.html.contains(r#"data-source-range="3-3""#),
        "hoisted hero should carry data-source-range from block_meta, got: {}",
        &extraction.html[..extraction.html.len().min(300)]
    );
}

#[test]
fn extract_hero_source_line_none_when_meta_absent() {
    // When block_meta has no source_line (e.g. emit_source_lines=false),
    // no data-source-range attribute should appear.
    let hero = hero_block(None, vec![]);
    let mut doc = Document::from_blocks(vec![hero]); // default BlockMeta, source_line=None
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("hero found");
    assert!(
        !extraction.html.contains("data-source-range"),
        "no source_line in meta → no data-source-range, got: {}",
        &extraction.html[..extraction.html.len().min(300)]
    );
}

#[test]
fn extract_hero_with_no_top_level_hero_in_nested_block() {
    // Top-level only: a hero inside a blockquote is NOT extracted.
    // (Realistically authors never write this, but the contract
    // matches today's interception semantics.)
    let mut doc = Document::from_blocks(vec![Block::BlockQuote(vec![hero_block(
        Some("nested.jpg"),
        vec![],
    )])]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks);
    assert!(extraction.is_none(), "nested hero NOT extracted");
    assert_eq!(doc.blocks.len(), 1);
}
