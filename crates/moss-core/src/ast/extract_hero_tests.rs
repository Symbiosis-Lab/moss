use super::super::document::BlockMeta;
use super::super::hooks::DefaultHooks;
use super::super::node::Inline;
use super::super::parser::parse_with_config;
use super::super::shortcode::HeroShortcode;
use super::super::url::{Url, UrlKind};
use super::*;

fn hero_block(image_url: Option<&str>, overlay: Vec<Block>) -> Block {
    Block::Shortcode(Shortcode::Hero(HeroShortcode {
        source: Default::default(),
        image: image_url.map(|u| Url::resolved(u, UrlKind::Asset)),
        image_alt: String::new(),
        extra_images: Vec::new(),
        attrs: String::new(),
        classes: String::new(),
        overlay,
        overlay_text: String::new(),
        width: None,
        mobile: None,
        panel: None,
        fade: None,
        max_height: None,
        align: None,
        caption: Vec::new(),
        slide_captions: Vec::new(),
        header: None,
        from: None,
        dark: None,
    }))
}

/// The text of a caption that parsed to one plain run, for comparing against
/// the author's words.
fn caption_text(caption: &[Inline]) -> &str {
    match caption {
        [Inline::Text(t)] => t,
        other => panic!("a plain caption is one text run, got {other:?}"),
    }
}

/// A caption belongs to the hero, not to whichever of the three syntaxes
/// named its image — so it is read the same way from all of them.
#[test]
fn caption_is_read_from_every_hero_syntax() {
    let cfg = ParseConfig::default();
    let cap = "Cover: the memorial wall (photo: A. Photographer)";

    let (attr_form, _, _) = parse_hero(&format!("{{image=cover.jpg caption=\"{cap}\"}}"), "", &cfg);
    assert_eq!(caption_text(&attr_form.caption), cap);
    assert_eq!(attr_form.overlay_text, "", "a caption is not overlay text");

    let (directive_form, _, _) = parse_hero(&format!("./cover.jpg {{caption=\"{cap}\"}}"), "", &cfg);
    assert_eq!(caption_text(&directive_form.caption), cap);

    let (body_form, _, _) = parse_hero(&format!("{{caption=\"{cap}\"}}"), "![[cover.jpg]]\n", &cfg);
    assert_eq!(caption_text(&body_form.caption), cap);

    let (plain, _, _) = parse_hero("{image=cover.jpg}", "", &cfg);
    assert!(plain.caption.is_empty(), "no caption written, none invented");
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
            legacy_id: None,
            numbered_from: None,
            named_id: false,
            contents_end: false,
            contents_label: None,
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
    // Only the opening hero leaves the body; later heroes stay in place.
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

/// The `alt` key is the way to describe an `image=` or directive-form hero
/// picture, and it overrides the alt text of a body embed. Without it the
/// image stays decorative.
#[test]
fn alt_key_sets_the_hero_image_alt() {
    let cfg = ParseConfig::default();

    let (with_alt, _, _) = parse_hero("{image=cover.jpg alt=\"Harbour at dawn\"}", "", &cfg);
    assert_eq!(with_alt.image_alt, "Harbour at dawn");

    let (directive_form, _, _) = parse_hero("./cover.jpg {alt=\"Harbour at dawn\"}", "", &cfg);
    assert_eq!(directive_form.image_alt, "Harbour at dawn");

    let (embed, _, _) = parse_hero("{alt=\"Harbour at dawn\"}", "![old](cover.jpg)\n# T", &cfg);
    assert_eq!(embed.image_alt, "Harbour at dawn");

    let (without_alt, _, _) = parse_hero("{image=cover.jpg}", "", &cfg);
    assert_eq!(without_alt.image_alt, "");
}

/// `dark=` names the picture dark mode shows; without it there is none.
#[test]
fn dark_picture_is_read_from_the_attribute_block() {
    let cfg = ParseConfig::default();
    let (hero, _, _) = parse_hero("{image=day.jpg dark=night.jpg}", "", &cfg);
    assert_eq!(hero.dark, Some(Url::unresolved("night.jpg")));
    let (plain, _, _) = parse_hero("{image=day.jpg}", "", &cfg);
    assert_eq!(plain.dark, None);
}

/// Only a hero that opens the page leaves the body: one written after other
/// content stays where the author put it.
#[test]
fn a_hero_after_body_content_stays_in_place() {
    let mut doc = Document::from_blocks(vec![
        Block::Paragraph(vec![Inline::Text("first".into())]),
        hero_block(Some("hero.jpg"), vec![]),
    ]);
    let hooks = DefaultHooks::new();
    assert!(extract_hero(&mut doc, &hooks).is_none());
    assert_eq!(doc.blocks.len(), 2, "the hero stays in the body");
}

#[test]
fn only_closed_comments_with_nothing_after_are_comments() {
    assert!(is_comment(&Block::Other("<!-- 编辑注释 -->\n<!-- 第二条 -->\n".into())));
    assert!(!is_comment(&Block::Other("<!-- 注释 -->正文\n".into())));
    assert!(!is_comment(&Block::Other("<!-- 没有结尾\n".into())));
}

/// A comment is not content: a hero under one still opens the page, and keeps
/// `header=over`, which only an opening hero takes.
#[test]
fn a_comment_above_the_hero_does_not_stop_it_opening_the_page() {
    let mut hero = hero_block(Some("hero.jpg"), vec![]);
    if let Block::Shortcode(Shortcode::Hero(args)) = &mut hero {
        args.header = Some("over".into());
        args.from = Some("work/".into());
    }
    let mut doc = Document::from_blocks(vec![Block::Other("<!-- a note for the editor -->\n".into()), hero]);
    let hooks = DefaultHooks::new();
    let extraction = extract_hero(&mut doc, &hooks).expect("the hero opens the page");
    assert_eq!(extraction.image_url.as_deref(), Some("hero.jpg"));
    assert_eq!(doc.blocks.len(), 1, "the comment stays, the hero leaves");
    let drawn = extraction.drawn_from.expect("a from= hero is kept for its second render");
    assert_eq!(drawn.shortcode.header.as_deref(), Some("over"));
}

fn page(markdown: &str) -> Document {
    parse_with_config(markdown, &ParseConfig { page_body: true, ..ParseConfig::default() })
}

fn hero(markdown: &str) -> Option<HeroShortcode> {
    match page(markdown).blocks.into_iter().next()? {
        Block::Shortcode(Shortcode::Hero(mut hero)) => {
            hero.source = Default::default();
            Some(hero)
        }
        _ => None,
    }
}

#[test]
fn a_picture_above_the_only_title_is_the_fenced_hero() {
    for picture in ["![A hall](assets/hall.jpg)", "![[hall.jpg]]", "![](hall.jpg)", "![Hall (1884) \"east\"](hall.jpg)", "![[my hall (1).jpg]]"] {
        let plain = format!("{picture}\n\n# Page Title\n\nText.\n\n## Part\n");
        let fenced = format!(":::hero\n{picture}\n:::\n\n# Page Title\n\nText.\n\n## Part\n");
        assert!(hero(&plain).is_some(), "{picture}");
        assert_eq!(hero(&plain), hero(&fenced), "{picture}");
        assert_eq!(page(&plain).blocks[1..], page(&fenced).blocks[1..], "{picture}");
    }
}

/// A footer or partial is not a page body: nothing lifts a hero out of it,
/// so its opening picture stays where it is written.
#[test]
fn outside_a_page_body_the_picture_stays_a_picture() {
    let doc = parse_with_config("![Hall](hall.jpg)\n\n# Contact\n", &ParseConfig::default());
    assert!(matches!(doc.blocks[0], Block::Figure { .. }), "{:?}", doc.blocks[0]);
}

/// A grid cell is parsed as a fragment, so a picture above a heading in a cell stays a picture.
#[test]
fn a_picture_above_a_heading_in_a_grid_cell_stays_a_picture() {
    let doc = page(":::grid 2\n![Hall](hall.jpg)\n\n# Contact\n+++\nText.\n:::\n");
    let Block::Shortcode(Shortcode::Grid(grid)) = &doc.blocks[0] else { panic!("{:?}", doc.blocks) };
    assert!(matches!(grid.cells[0][0], Block::Figure { .. }), "{:?}", grid.cells[0]);
}

/// `extract_hero` looks past leading comments, so the picture after them opens the page.
#[test]
fn a_comment_above_the_picture_does_not_stop_it() {
    for markdown in ["<!-- note -->\n![Hall](hall.jpg)\n\n# Title\n", "<!--\n# Draft\n-->\n![Hall](hall.jpg)\n\n# Title\n"] {
        let doc = page(markdown);
        assert!(matches!(doc.blocks[1], Block::Shortcode(Shortcode::Hero(_))), "{:?}", doc.blocks);
    }
}

/// A click on the hero in the preview reaches the picture's own line.
#[test]
fn the_inferred_hero_sits_on_the_pictures_line() {
    let config = ParseConfig { emit_source_lines: true, page_body: true, ..ParseConfig::default() };
    let doc = parse_with_config("\n\n![Hall](hall.jpg)\n\n# Page Title\n", &config);
    assert!(matches!(doc.blocks[0], Block::Shortcode(Shortcode::Hero(_))));
    assert_eq!(doc.block_meta[0].source_line, Some(3));
    let Block::Shortcode(Shortcode::Hero(hero)) = &doc.blocks[0] else { unreachable!() };
    assert_eq!(hero.source.extra_lines, 0);
}

#[test]
fn near_misses_keep_the_picture_in_the_body() {
    for markdown in [
        // Numbered sections written as level-1 headings: none is the title.
        "![Hall](hall.jpg)\n\n# 1.\n\nText.\n\n# 2.\n\nText.\n",
        "![Hall](hall.jpg)\n*The hall in 1884*\n\n# Page Title\n",
        "![[hall.jpg|The hall in 1884]]\n\n# Page Title\n",
        "![Hall|body](hall.jpg)\n\n# Page Title\n",
        "![Hall|screen](hall.jpg)\n\n# Page Title\n",
        "![Hall](hall.jpg)\n\nA line of text.\n\n# Page Title\n",
        "![Hall](hall.jpg) ![Lake](lake.jpg)\n\n# Page Title\n",
        "[![Hall](hall.jpg)](https://example.org)\n\n# Page Title\n",
        "![Hall](hall.jpg)\n\n## Page Title\n",
        "# Page Title\n\n![Hall](hall.jpg)\n",
        "![[film.mp4]]\n\n# Page Title\n",
        "![Hall](my hall.jpg)\n\n# Page Title\n",
        "![[hall.jpg]] ![[lake.jpg]]\n\n# Page Title\n",
        "![Hall](hall.jpg \"The hall\")\n\n# Page Title\n",
    ] {
        assert!(hero(markdown).is_none(), "{markdown}");
    }
}
