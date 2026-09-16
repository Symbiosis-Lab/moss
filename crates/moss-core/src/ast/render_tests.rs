use super::super::hooks::DefaultHooks;
use super::super::node::Inline;
use super::super::url::{Url, UrlKind};
use super::*;

fn render(blocks: Vec<Block>) -> String {
    let doc = Document::from_blocks(blocks);
    render_document(&doc, &DefaultHooks::new())
}

#[test]
fn renders_empty_document_to_empty_string() {
    assert_eq!(render(vec![]), "");
}

#[test]
fn renders_paragraph() {
    let html = render(vec![Block::Paragraph(vec![Inline::Text("hi".into())])]);
    assert_eq!(html, "<p>hi</p>\n");
}

#[test]
fn renders_heading_with_id() {
    let html = render(vec![Block::Heading {
        level: 2,
        children: vec![Inline::Text("Setup".into())],
        id: Some("setup".into()),
    }]);
    assert_eq!(html, "<h2 id=\"setup\">Setup<a class=\"moss-heading-anchor\" href=\"#setup\" aria-label=\"Permalink to this section\"></a></h2>\n");
}

#[test]
fn renders_resolved_link_internal() {
    let html = render(vec![Block::Paragraph(vec![Inline::Link {
        url: Url::resolved("docs/", UrlKind::Internal),
        title: None,
        children: vec![Inline::Text("Docs".into())],
        is_wikilink: false,
    }])]);
    assert_eq!(html, "<p><a href=\"docs/\">Docs</a></p>\n");
}

#[test]
fn renders_resolved_link_wikilink_carries_class() {
    // PR7a: wikilink class can come from either the resolved URL kind
    // (legacy production path) OR the new is_wikilink AST flag.
    let html = render(vec![Block::Paragraph(vec![Inline::Link {
        url: Url::resolved("../docs/", UrlKind::Wikilink),
        title: None,
        children: vec![Inline::Text("Docs".into())],
        is_wikilink: false,
    }])]);
    assert!(html.contains(r#"class="wikilink""#), "got: {html}");
}

#[test]
fn renders_link_with_is_wikilink_flag_emits_class() {
    // PR7a: is_wikilink: true on a non-wikilink-kind URL still
    // produces the wikilink class. Parser sets this for any
    // pulldown-cmark Tag::Link { link_type: LinkType::WikiLink, .. }.
    let html = render(vec![Block::Paragraph(vec![Inline::Link {
        url: Url::resolved("../docs/", UrlKind::Internal),
        title: None,
        children: vec![Inline::Text("Docs".into())],
        is_wikilink: true,
    }])]);
    assert!(
        html.contains(r#"class="wikilink""#),
        "is_wikilink: true should produce class=\"wikilink\"; got: {html}"
    );
}

#[test]
fn renders_resolved_image() {
    let html = render(vec![Block::Paragraph(vec![Inline::Image {
        src: Url::resolved("cat.jpg", UrlKind::Asset),
        alt: "Cat".into(),
        title: None,
        is_wikilink: false,
        wikilink_pothole: None,
    }])]);
    assert_eq!(html, "<p><img src=\"cat.jpg\" alt=\"Cat\" /></p>\n");
}

#[test]
fn renders_emphasis_and_strong() {
    let html = render(vec![Block::Paragraph(vec![
        Inline::Emphasis(vec![Inline::Text("em".into())]),
        Inline::Text(" ".into()),
        Inline::Strong(vec![Inline::Text("strong".into())]),
    ])]);
    assert_eq!(html, "<p><em>em</em> <strong>strong</strong></p>\n");
}

#[test]
fn renders_inline_code_with_escaping() {
    let html = render(vec![Block::Paragraph(vec![Inline::Code("a<b>c".into())])]);
    assert_eq!(html, "<p><code>a&lt;b&gt;c</code></p>\n");
}

#[test]
fn renders_unordered_list_tight() {
    let html = render(vec![Block::List {
        ordered: false,
        start: None,
        items: vec![
            vec![Block::Paragraph(vec![Inline::Text("one".into())])],
            vec![Block::Paragraph(vec![Inline::Text("two".into())])],
        ],
        item_source_lines: vec![],
    }]);
    assert_eq!(html, "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n");
}

#[test]
fn renders_ordered_list() {
    let html = render(vec![Block::List {
        ordered: true,
        start: None,
        items: vec![vec![Block::Paragraph(vec![Inline::Text("a".into())])]],
        item_source_lines: vec![],
    }]);
    assert!(html.starts_with("<ol>"));
}

#[test]
fn render_ordered_list_emits_start_attribute_when_non_default() {
    // `3. foo` should produce `<ol start="3">…</ol>`. The attribute
    // appears immediately after `<ol`, before any `data-source-line`
    // (Phase 4 followup B contract).
    let html = render(vec![Block::List {
        ordered: true,
        start: Some(3),
        items: vec![vec![Block::Paragraph(vec![Inline::Text("a".into())])]],
        item_source_lines: vec![],
    }]);
    assert!(
        html.starts_with(r#"<ol start="3">"#),
        "expected start attr immediately after <ol, got: {html}"
    );
}

#[test]
fn render_ordered_list_omits_start_when_default_1() {
    // `start: None` is the canonical shape for "default 1." lists.
    // The renderer must NOT emit `start="1"` (semantically
    // identical to omitting the attr, but noisier).
    let html = render(vec![Block::List {
        ordered: true,
        start: None,
        items: vec![vec![Block::Paragraph(vec![Inline::Text("a".into())])]],
        item_source_lines: vec![],
    }]);
    assert!(html.starts_with("<ol>"), "expected bare <ol>, got: {html}");
    assert!(
        !html.contains("start="),
        "ordered list with default start should not emit start attr, got: {html}"
    );
}

#[test]
fn render_unordered_list_emits_no_start() {
    // Even if `start: Some(N)` were somehow set on an unordered
    // list (shouldn't happen via the parser, but defense in depth),
    // `<ul>` must never carry `start=`.
    let html = render(vec![Block::List {
        ordered: false,
        start: Some(5),
        items: vec![vec![Block::Paragraph(vec![Inline::Text("a".into())])]],
        item_source_lines: vec![],
    }]);
    assert!(html.starts_with("<ul>"), "expected bare <ul>, got: {html}");
    assert!(
        !html.contains("start="),
        "unordered list must never carry start attr, got: {html}"
    );
}

#[test]
fn renders_code_block_with_lang() {
    let html = render(vec![Block::CodeBlock {
        lang: Some("rust".into()),
        value: "fn main() {}".into(),
    }]);
    assert_eq!(
        html,
        "<pre><code class=\"language-rust\">fn main() {}</code></pre>\n"
    );
}

#[test]
fn renders_code_block_without_lang() {
    let html = render(vec![Block::CodeBlock {
        lang: None,
        value: "bare".into(),
    }]);
    assert_eq!(html, "<pre><code>bare</code></pre>\n");
}

#[test]
fn renders_thematic_break() {
    let html = render(vec![Block::ThematicBreak]);
    assert_eq!(html, "<hr />\n");
}

// -----------------------------------------------------------------
// Phase 4 PR4: Block::Callout render shape
// -----------------------------------------------------------------

use super::super::node::{CalloutKind, Fold};

#[test]
fn renders_basic_callout_with_title() {
    let html = render(vec![Block::Callout {
        kind: CalloutKind::Note,
        fold: None,
        title: Some("Heads up".into()),
        children: vec![Block::Paragraph(vec![Inline::Text("Body.".into())])],
    }]);
    assert!(
        html.contains(r#"<div class="callout" data-type="note">"#),
        "expected callout div with data-type, got: {html}"
    );
    assert!(
        html.contains(r#"<div class="callout-title">Heads up</div>"#),
        "expected inline title slot, got: {html}"
    );
    assert!(
        html.contains(r#"<div class="callout-content">"#),
        "expected content slot, got: {html}"
    );
    assert!(html.contains("<p>Body.</p>"), "body must render: {html}");
}

#[test]
fn renders_callout_falls_back_to_default_title() {
    let html = render(vec![Block::Callout {
        kind: CalloutKind::Warning,
        fold: None,
        title: None,
        children: vec![],
    }]);
    assert!(
        html.contains(r#"<div class="callout-title">Warning</div>"#),
        "expected capitalized fallback title, got: {html}"
    );
}

#[test]
fn renders_foldable_callout_with_data_fold_attribute() {
    let html_open = render(vec![Block::Callout {
        kind: CalloutKind::Tip,
        fold: Some(Fold::Open),
        title: Some("Open".into()),
        children: vec![],
    }]);
    assert!(
        html_open.contains(r#"data-type="tip""#) && html_open.contains(r#"data-fold="open""#),
        "expected data-fold='open' attribute, got: {html_open}"
    );

    let html_closed = render(vec![Block::Callout {
        kind: CalloutKind::Tip,
        fold: Some(Fold::Closed),
        title: None,
        children: vec![],
    }]);
    assert!(
        html_closed.contains(r#"data-fold="closed""#),
        "expected data-fold='closed' attribute, got: {html_closed}"
    );
}

#[test]
fn callout_alias_renders_canonical_data_type_slug() {
    // tldr → abstract; ensures the canonicalized slug is what
    // appears in HTML.
    let html = render(vec![Block::Callout {
        kind: CalloutKind::Abstract,
        fold: None,
        title: Some("TL;DR".into()),
        children: vec![],
    }]);
    assert!(
        html.contains(r#"data-type="abstract""#),
        "expected canonical slug 'abstract', got: {html}"
    );
}

#[test]
fn callout_title_is_html_escaped() {
    // Title is rendered through escape_text (the same function the
    // existing renderer uses for text content). escape_text escapes
    // `<`, `>`, `&` but NOT `"` — `"` is only dangerous inside HTML
    // attribute values, and title sits between `<div>` tags as text.
    let html = render(vec![Block::Callout {
        kind: CalloutKind::Warning,
        fold: None,
        title: Some(r#"Use <script> & "quotes""#.into()),
        children: vec![],
    }]);
    assert!(
        html.contains("Use &lt;script&gt; &amp;"),
        "title must escape lt/gt/amp, got: {html}"
    );
    // No raw `<script>` may appear inside the title div.
    assert!(
        !html.contains("<div class=\"callout-title\">Use <script>"),
        "unescaped angle brackets leaked, got: {html}"
    );
}

#[test]
fn renders_blockquote_with_paragraph() {
    let html = render(vec![Block::BlockQuote(vec![Block::Paragraph(vec![
        Inline::Text("q".into()),
    ])])]);
    assert_eq!(html, "<blockquote>\n<p>q</p>\n</blockquote>\n");
}

#[test]
fn renders_table() {
    let html = render(vec![Block::Table {
        header: vec![vec![Inline::Text("A".into())]],
        rows: vec![vec![vec![Inline::Text("1".into())]]],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    // Accessible scroll wrapper around the (semantically intact) table.
    assert!(html.contains("<div class=\"moss-table-scroll\" tabindex=\"0\">"));
    assert!(html.contains("<thead>"));
    assert!(html.contains("<tbody>"));
    // Column "A"/"1" is all-numeric → auto right-aligned (header matches).
    assert!(html.contains("<th class=\"moss-col-right\">A</th>"));
    assert!(html.contains("<td class=\"moss-col-right\">1</td>"));
}

fn text_row(cells: &[&str]) -> Vec<Vec<Inline>> {
    cells
        .iter()
        .map(|c| vec![Inline::Text((*c).into())])
        .collect()
}

#[test]
fn table_auto_right_aligns_numeric_columns_only() {
    // Real-shape row: text name, thousands-separated count, number+unit.
    let html = render(vec![Block::Table {
        header: text_row(&["播客名称", "订阅数", "更新间隔"]),
        rows: vec![
            text_row(&["天真不天真", "1,457,776", "17.1天"]),
            text_row(&["凹凸电波", "1,317,696", "6.9天"]),
        ],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    // Text column: no alignment class on header or body.
    assert!(html.contains("<th>播客名称</th>"), "{html}");
    assert!(html.contains("<td>天真不天真</td>"), "{html}");
    // Numeric columns (plain and number+unit): header + cells right-aligned.
    assert!(
        html.contains("<th class=\"moss-col-right\">订阅数</th>"),
        "{html}"
    );
    assert!(
        html.contains("<td class=\"moss-col-right\">1,457,776</td>"),
        "{html}"
    );
    assert!(
        html.contains("<td class=\"moss-col-right\">17.1天</td>"),
        "{html}"
    );
}

#[test]
fn table_honors_gfm_alignment_over_numeric_detection() {
    // Author explicitly LEFT-aligned a numeric column and CENTERed a text
    // one; both overrides win over auto-detection.
    let html = render(vec![Block::Table {
        header: text_row(&["N", "M"]),
        rows: vec![text_row(&["1", "x"])],
        alignments: vec![ColumnAlignment::Left, ColumnAlignment::Center],
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    assert!(html.contains("<td>1</td>"), "author Left must win: {html}");
    assert!(
        html.contains("<th class=\"moss-col-center\">M</th>"),
        "{html}"
    );
    assert!(
        html.contains("<td class=\"moss-col-center\">x</td>"),
        "{html}"
    );
}

#[test]
fn table_mixed_column_below_threshold_stays_left() {
    // 2 of 5 cells numeric (40% < 80%) → column stays left, no right class.
    let html = render(vec![Block::Table {
        header: text_row(&["Col"]),
        rows: vec![
            text_row(&["第1名"]),
            text_row(&["12"]),
            text_row(&["abc"]),
            text_row(&["34"]),
            text_row(&["N/A"]),
        ],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    assert!(
        html.contains("<td>12</td>"),
        "no right-align expected: {html}"
    );
    assert!(
        !html.contains("moss-col-right"),
        "sub-threshold column must stay left: {html}"
    );
}

#[test]
fn table_always_wraps_in_scroll_container() {
    let html = render(vec![Block::Table {
        header: text_row(&["a", "b"]),
        rows: vec![text_row(&["x", "y"])],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    assert!(html.contains("<div class=\"moss-table-scroll\" tabindex=\"0\">"));
    assert!(html.trim_end().ends_with("</div>"));
}

#[test]
fn cell_reads_as_number_matrix() {
    for s in [
        "1",
        "28",
        "1,457,776",
        "17.1天",
        "7.1天",
        "99%",
        "¥1,200",
        "-5",
        "3.5k",
        "1,200人",
    ] {
        assert!(cell_reads_as_number(s), "should read as number: {s:?}");
    }
    for s in [
        "",
        "第1名",
        "2020-2021",
        "治愈陪伴",
        "GIADA迦达",
        "N/A",
        "abc",
    ] {
        assert!(!cell_reads_as_number(s), "should NOT read as number: {s:?}");
    }
}

#[test]
fn table_partial_gfm_alignment_auto_detects_unmarked_columns() {
    // alignments = [None, Center, None]: col0 (numeric) auto-right-aligns,
    // col1 honors the GFM center, col2 (text) stays left. Confirms
    // per-column resolution mixes author intent and detection correctly.
    let html = render(vec![Block::Table {
        header: text_row(&["N", "C", "T"]),
        rows: vec![text_row(&["1", "x", "aa"]), text_row(&["2", "y", "bb"])],
        alignments: vec![
            ColumnAlignment::None,
            ColumnAlignment::Center,
            ColumnAlignment::None,
        ],
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    assert!(
        html.contains("<td class=\"moss-col-right\">1</td>"),
        "col0 numeric → right: {html}"
    );
    assert!(
        html.contains("<td class=\"moss-col-center\">x</td>"),
        "col1 GFM center: {html}"
    );
    assert!(html.contains("<td>aa</td>"), "col2 text → left: {html}");
}

#[test]
fn table_header_only_no_body_is_valid() {
    // No body rows: wrapper + thead, no tbody, no numeric detection, no panic.
    let html = render(vec![Block::Table {
        header: text_row(&["A", "B"]),
        rows: vec![],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }]);
    assert!(html.contains("<div class=\"moss-table-scroll\" tabindex=\"0\">"));
    assert!(html.contains("<thead>"));
    assert!(!html.contains("<tbody>"), "no rows → no tbody: {html}");
    assert!(
        !html.contains("moss-col-right"),
        "no body → no detection: {html}"
    );
    assert!(html.trim_end().ends_with("</div>"));
}

#[test]
fn renders_other_block_passes_html_through() {
    let html = render(vec![Block::Other("<custom></custom>".into())]);
    assert_eq!(html, "<custom></custom>");
}

#[test]
fn text_escapes_lt_gt_amp() {
    let html = render(vec![Block::Paragraph(vec![Inline::Text("a<b>c&d".into())])]);
    assert_eq!(html, "<p>a&lt;b&gt;c&amp;d</p>\n");
}

#[test]
fn round_trips_parse_to_render_for_canonical_doc() {
    // End-to-end: post-resolve markdown → parse → simulate visit
    // (mark every URL Internal) → render → check shape.
    //
    // Phase 4 PR2: the parser now populates Block::Heading.id with the
    // Obsidian anchor slug, so the rendered <h1> carries id="title".
    let md = "# Title\n\npara with [link](docs/) and *em*.\n";
    let mut doc = super::super::parser::parse(md);
    super::super::visit::visit_urls_mut(&mut doc, |u| match u {
        Url::Unresolved(s) => *u = Url::resolved(s.clone(), UrlKind::Internal),
        _ => {}
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(html.contains(r##"<h1 id="title">Title<a class="moss-heading-anchor" href="#title" aria-label="Permalink to this section"></a></h1>"##), "got: {html}");
    assert!(html.contains(r#"<a href="docs/">link</a>"#));
    assert!(html.contains("<em>em</em>"));
}

// -----------------------------------------------------------------
// Phase 4 PR3 (2026-05-27): Block::Figure render
// -----------------------------------------------------------------

#[test]
fn figure_renders_with_caption() {
    // Canonical shape: <figure class="moss-image">{inner img}{figcaption}</figure>.
    // DefaultHooks::new() has no snapshot, so inner is the bare <img>
    // (test path). Production wires DefaultHooks::with_snapshot which
    // routes inner through synth — same shape, richer attrs.
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("logo.png", UrlKind::Asset),
            alt: "A logo".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: Some(vec![Inline::Text("A logo".into())]),
        width: None,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }]);
    assert!(
        html.starts_with(r#"<figure class="moss-image">"#),
        "expected figure wrap, got: {html}"
    );
    assert!(html.contains(r#"src="logo.png""#), "got: {html}");
    // The visible caption is the image's description, so the `alt` must
    // not say it a second time — a screen reader would announce the same
    // sentence twice (once as the accessible name, once as the caption).
    assert!(html.contains(r#"alt="""#), "got: {html}");
    assert!(
        html.contains("<figcaption>A logo</figcaption>"),
        "got: {html}"
    );
    assert_eq!(
        html.matches("A logo").count(),
        1,
        "caption text must appear exactly once (caption only): {html}"
    );
    assert!(html.ends_with("</figure>\n"), "got: {html}");
}

#[test]
fn figure_without_caption_keeps_alt_as_the_accessible_name() {
    // The other half of the rule: no `<figcaption>` means nothing else
    // describes the image, so `alt` is its ONLY accessible name and must
    // survive. (Reachable via a width-only embed, e.g. `![[p.jpg|55%]]`
    // carrying an alt, and via any future caption-less figure.)
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("p.jpg", UrlKind::Asset),
            alt: "A photo of the wall".into(),
            title: None,
            is_wikilink: true,
            wikilink_pothole: None,
        },
        caption: None,
        width: Some("55%".to_string()),
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }]);
    assert!(
        html.contains(r#"alt="A photo of the wall""#),
        "an uncaptioned figure must keep its alt: {html}"
    );
    assert!(!html.contains("<figcaption"), "got: {html}");
}

#[test]
fn wikilink_alias_lands_in_the_caption_only_not_also_in_alt() {
    // The reported defect: `![[cover.jpg|credit line]]` put the alias in
    // BOTH `alt=` and `<figcaption>`, so assistive tech read it twice.
    // This is the no-graph (parser) twin of the dispatcher path in
    // `resolve::wikilink_dispatch`; both build the same `Block::Figure`
    // and both are de-duplicated by the one renderer.
    let md = "![[cover.jpg|Photo: Kyiv memorial wall]]\n";
    let mut doc = super::super::parser::parse(md);
    super::super::visit::visit_urls_mut(&mut doc, |u| {
        if let Url::Unresolved(s) = u {
            *u = Url::resolved(s.clone(), UrlKind::Asset)
        }
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(
        html.contains("<figcaption>Photo: Kyiv memorial wall</figcaption>"),
        "alias must still be the visible caption: {html}"
    );
    assert!(
        html.contains(r#"alt="""#),
        "alias must NOT repeat in alt: {html}"
    );
}

#[test]
fn uncaptioned_wikilink_alias_stays_in_alt() {
    // The trap case: with `implicit_figure = false` there is no figure and
    // no caption, so the alias is the image's only accessible name. It
    // must stay in `alt=` — stripping it here would silently un-name every
    // image on an opted-out site.
    let md = "![[cover.jpg|Photo: Kyiv memorial wall]]\n";
    let cfg = super::super::parser::ParseConfig {
        implicit_figure: false,
        ..Default::default()
    };
    let mut doc = super::super::parser::parse_with_config(md, &cfg);
    super::super::visit::visit_urls_mut(&mut doc, |u| {
        if let Url::Unresolved(s) = u {
            *u = Url::resolved(s.clone(), UrlKind::Asset)
        }
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(!html.contains("<figure"), "got: {html}");
    assert!(
        html.contains(r#"alt="Photo: Kyiv memorial wall""#),
        "an uncaptioned image keeps the alias as its accessible name: {html}"
    );
}

#[test]
fn inline_wikilink_alias_inside_prose_stays_in_alt() {
    // Same trap, different shape: an image embedded mid-paragraph never
    // becomes a figure, so its alias is the only accessible name.
    let md = "Text before ![[cover.jpg|Photo: Kyiv memorial wall]] text after\n";
    let mut doc = super::super::parser::parse(md);
    super::super::visit::visit_urls_mut(&mut doc, |u| {
        if let Url::Unresolved(s) = u {
            *u = Url::resolved(s.clone(), UrlKind::Asset)
        }
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(!html.contains("<figcaption"), "got: {html}");
    assert!(
        html.contains(r#"alt="Photo: Kyiv memorial wall""#),
        "got: {html}"
    );
}

#[test]
fn figure_renders_without_caption_when_none() {
    // Empty-alt case: caption: None → no <figcaption> element.
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("x.png", UrlKind::Asset),
            alt: String::new(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: None,
        width: None,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }]);
    assert!(html.contains("<figure"), "got: {html}");
    assert!(
        !html.contains("<figcaption"),
        "expected no figcaption, got: {html}"
    );
    assert!(html.contains("</figure>"), "got: {html}");
}

#[test]
fn figure_renders_no_figcaption_for_empty_caption_vec() {
    // Defensive: caption: Some(vec![]) is treated identically to None.
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("x.png", UrlKind::Asset),
            alt: "x".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: Some(vec![]),
        width: None,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }]);
    assert!(!html.contains("<figcaption"), "got: {html}");
}

// Editor Image UX (2026-06-04): a `%`-suffixed Figure width renders as
// an inline style="width:NN%"; a named token stays data-width=.
// -----------------------------------------------------------------

#[test]
fn figure_percent_width_emits_inline_style() {
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("pic.jpg", UrlKind::Asset),
            alt: "alt".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: None,
        width: Some("55%".to_string()),
        align: None,
        class_names: vec![],
        img_style: None,
    }]);
    assert!(
        html.contains(r#"<figure class="moss-image" style="width:55%""#),
        "got: {html}"
    );
    assert!(
        !html.contains("data-width="),
        "percent must not emit data-width: {html}"
    );
}

#[test]
fn figure_named_width_still_emits_data_width() {
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("pic.jpg", UrlKind::Asset),
            alt: "alt".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: None,
        width: Some("wide".to_string()),
        align: None,
        class_names: vec![],
        img_style: None,
    }]);
    assert!(html.contains(r#"data-width="wide""#), "got: {html}");
    assert!(
        !html.contains("style=\"width"),
        "named token must not emit style: {html}"
    );
}

#[test]
fn figure_caption_escapes_html_unsafe_chars() {
    // Caption is a Vec<Inline>; Inline::Text passes through
    // escape_text. The figure renderer must NOT double-escape; the
    // existing inline path is the single source of escaping.
    let html = render(vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("p.jpg", UrlKind::Asset),
            alt: "a<b>c".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: Some(vec![Inline::Text("a<b>c".into())]),
        width: None,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }]);
    assert!(
        html.contains("<figcaption>a&lt;b&gt;c</figcaption>"),
        "got: {html}"
    );
}

#[test]
fn figure_end_to_end_from_parser_to_render() {
    // Parse → visit (resolve URL) → render: covers the full path.
    let md = "![A photo](photo.jpg)\n";
    let mut doc = super::super::parser::parse(md);
    super::super::visit::visit_urls_mut(&mut doc, |u| match u {
        Url::Unresolved(s) => *u = Url::resolved(s.clone(), UrlKind::Asset),
        _ => {}
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(
        html.contains(r#"<figure class="moss-image">"#),
        "expected figure, got: {html}"
    );
    assert!(html.contains(r#"src="photo.jpg""#), "got: {html}");
    assert!(
        html.contains("<figcaption>A photo</figcaption>"),
        "got: {html}"
    );
}

#[test]
fn paragraph_with_image_and_text_does_not_become_figure() {
    // End-to-end regression guard: ![img](u) caption text MUST stay
    // as a paragraph (not get the figure wrap) so the prose isn't
    // swallowed. Mirrors the parser-side guard `image_with_caption_text_does_not_promote`.
    let md = "![alt](a.jpg) plain text\n";
    let mut doc = super::super::parser::parse(md);
    super::super::visit::visit_urls_mut(&mut doc, |u| match u {
        Url::Unresolved(s) => *u = Url::resolved(s.clone(), UrlKind::Asset),
        _ => {}
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(
        !html.contains("<figure"),
        "image+text must not be wrapped in figure, got: {html}"
    );
    assert!(html.contains("plain text"), "got: {html}");
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "visit_urls_mut missing")]
fn unresolved_url_in_link_panics_in_debug() {
    // Critical contract: the bypass class is a debug-time crash.
    let _ = render(vec![Block::Paragraph(vec![Inline::Link {
        url: Url::unresolved("docs/"),
        title: None,
        children: vec![],
        is_wikilink: false,
    }])]);
}

// -----------------------------------------------------------------
// 2026-05-28 (Phase 4 source-line wiring): BlockMeta → data-source-line
// emission.
// -----------------------------------------------------------------

/// Render with explicit per-block meta. Helper for the source-line tests.
fn render_with_meta(blocks: Vec<Block>, meta: Vec<BlockMeta>) -> String {
    let doc = Document::from_blocks_with_meta(blocks, meta);
    render_document(&doc, &DefaultHooks::new())
}

#[test]
fn paragraph_emits_data_source_line_when_meta_set() {
    let html = render_with_meta(
        vec![Block::Paragraph(vec![Inline::Text("hi".into())])],
        vec![BlockMeta {
            source_line: Some(7),
        }],
    );
    assert_eq!(html, "<p data-source-line=\"7\">hi</p>\n");
}

#[test]
fn heading_emits_data_source_line_through_hook() {
    let html = render_with_meta(
        vec![Block::Heading {
            level: 2,
            children: vec![Inline::Text("Setup".into())],
            id: Some("setup".into()),
        }],
        vec![BlockMeta {
            source_line: Some(3),
        }],
    );
    assert!(
            html.contains(r##"<h2 id="setup" data-source-line="3">Setup<a class="moss-heading-anchor" href="#setup" aria-label="Permalink to this section"></a></h2>"##),
            "got: {html}"
        );
}

#[test]
fn list_blockquote_codeblock_table_hr_emit_data_source_line() {
    // Each block type that the legacy transform_events annotated
    // must emit `data-source-line` when meta carries it. Single
    // smoke test covering every top-level block kind.
    let blocks = vec![
        Block::BlockQuote(vec![Block::Paragraph(vec![Inline::Text("q".into())])]),
        Block::List {
            ordered: false,
            start: None,
            items: vec![vec![Block::Paragraph(vec![Inline::Text("a".into())])]],
            item_source_lines: vec![],
        },
        Block::List {
            ordered: true,
            start: None,
            items: vec![vec![Block::Paragraph(vec![Inline::Text("b".into())])]],
            item_source_lines: vec![],
        },
        Block::CodeBlock {
            lang: Some("rust".into()),
            value: "x".into(),
        },
        Block::Table {
            header: vec![vec![Inline::Text("H".into())]],
            rows: vec![vec![vec![Inline::Text("c".into())]]],
            alignments: Vec::new(),
            header_source_line: None,
            row_source_lines: vec![],
        },
        Block::ThematicBreak,
    ];
    let meta = vec![
        BlockMeta {
            source_line: Some(1),
        },
        BlockMeta {
            source_line: Some(2),
        },
        BlockMeta {
            source_line: Some(3),
        },
        BlockMeta {
            source_line: Some(4),
        },
        BlockMeta {
            source_line: Some(5),
        },
        BlockMeta {
            source_line: Some(6),
        },
    ];
    let html = render_with_meta(blocks, meta);
    assert!(
        html.contains(r#"<blockquote data-source-line="1">"#),
        "blockquote missing: {html}"
    );
    assert!(
        html.contains(r#"<ul data-source-line="2">"#),
        "ul missing: {html}"
    );
    assert!(
        html.contains(r#"<ol data-source-line="3">"#),
        "ol missing: {html}"
    );
    assert!(
        html.contains(r#"<pre data-source-line="4">"#),
        "pre missing: {html}"
    );
    assert!(
        html.contains(r#"<table data-source-line="5">"#),
        "table missing: {html}"
    );
    assert!(
        html.contains(r#"<hr data-source-line="6" />"#),
        "hr missing: {html}"
    );
}

#[test]
fn list_emits_per_li_data_source_line_when_parser_tracks() {
    // 2026-05-28 (Phase 4 source-lines followup): `<li>` carries
    // `data-source-line="N"` when the parser populated
    // `item_source_lines`. Mirrors the legacy transform_events shape
    // (commit f91aca8fa, 2026-04-01) that emitted on `<li>` for
    // proportional scroll-sync interpolation. The outer `<ul>` carries
    // BlockMeta.source_line separately.
    let blocks = vec![Block::List {
        ordered: false,
        start: None,
        items: vec![
            vec![Block::Paragraph(vec![Inline::Text("one".into())])],
            vec![Block::Paragraph(vec![Inline::Text("two".into())])],
            vec![Block::Paragraph(vec![Inline::Text("three".into())])],
        ],
        item_source_lines: vec![Some(10), Some(11), Some(12)],
    }];
    let meta = vec![BlockMeta {
        source_line: Some(10),
    }];
    let html = render_with_meta(blocks, meta);
    assert!(
        html.contains(r#"<ul data-source-line="10">"#),
        "ul opener missing: {html}"
    );
    assert!(
        html.contains(r#"<li data-source-line="10">one</li>"#),
        "li 10 missing: {html}"
    );
    assert!(
        html.contains(r#"<li data-source-line="11">two</li>"#),
        "li 11 missing: {html}"
    );
    assert!(
        html.contains(r#"<li data-source-line="12">three</li>"#),
        "li 12 missing: {html}"
    );
}

#[test]
fn list_omits_li_data_source_line_when_parser_did_not_track() {
    // When `item_source_lines` is empty (default — parser ran with
    // `emit_source_lines: false`), no per-`<li>` attribute is emitted.
    // Locks the publish-build invariant: byte-identical output to the
    // pre-followup renderer.
    let blocks = vec![Block::List {
        ordered: false,
        start: None,
        items: vec![
            vec![Block::Paragraph(vec![Inline::Text("a".into())])],
            vec![Block::Paragraph(vec![Inline::Text("b".into())])],
        ],
        item_source_lines: vec![],
    }];
    let html = render_with_meta(blocks, vec![BlockMeta::default()]);
    assert_eq!(html, "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n");
}

#[test]
fn table_emits_per_tr_data_source_line_when_parser_tracks() {
    // Header `<tr>` carries `header_source_line`; each body `<tr>`
    // carries the matching `row_source_lines[i]`.
    let blocks = vec![Block::Table {
        header: vec![vec![Inline::Text("H".into())]],
        rows: vec![
            vec![vec![Inline::Text("1".into())]],
            vec![vec![Inline::Text("2".into())]],
            vec![vec![Inline::Text("3".into())]],
        ],
        alignments: Vec::new(),
        header_source_line: Some(5),
        row_source_lines: vec![Some(7), Some(8), Some(9)],
    }];
    let meta = vec![BlockMeta {
        source_line: Some(5),
    }];
    let html = render_with_meta(blocks, meta);
    assert!(
        html.contains(r#"<table data-source-line="5">"#),
        "table opener missing: {html}"
    );
    assert!(html.contains(r#"<thead>"#), "thead missing: {html}");
    // Header row line — note the header tr is on the marker line
    // because pulldown-cmark anchors the head row to the line of the
    // `| h |` header markdown row.
    // Column H/1/2/3 is all-numeric → cells carry moss-col-right; the
    // per-`<tr>` source-line annotation is unaffected.
    assert!(
        html.contains(r#"<tr data-source-line="5"><th class="moss-col-right">H</th>"#),
        "head tr missing: {html}"
    );
    assert!(
        html.contains(r#"<tr data-source-line="7"><td class="moss-col-right">1</td>"#),
        "body tr 7 missing: {html}"
    );
    assert!(
        html.contains(r#"<tr data-source-line="8"><td class="moss-col-right">2</td>"#),
        "body tr 8 missing: {html}"
    );
    assert!(
        html.contains(r#"<tr data-source-line="9"><td class="moss-col-right">3</td>"#),
        "body tr 9 missing: {html}"
    );
}

#[test]
fn table_omits_tr_data_source_line_when_parser_did_not_track() {
    // Publish-build invariant: byte-identical output to pre-followup.
    let blocks = vec![Block::Table {
        header: vec![vec![Inline::Text("A".into())]],
        rows: vec![vec![vec![Inline::Text("1".into())]]],
        alignments: Vec::new(),
        header_source_line: None,
        row_source_lines: vec![],
    }];
    let html = render_with_meta(blocks, vec![BlockMeta::default()]);
    // No `data-source-line` anywhere — the table opener also has
    // BlockMeta::default() (None), so the entire `<table>...</table>`
    // block is annotation-free.
    assert!(
        !html.contains("data-source-line"),
        "no annotation expected: {html}"
    );
    assert!(html.contains("<thead>"));
    // Numeric column → right-aligned cells (annotation-free otherwise).
    assert!(html.contains("<tr><th class=\"moss-col-right\">A</th></tr>"));
    assert!(html.contains("<tr><td class=\"moss-col-right\">1</td></tr>"));
}

#[test]
fn figure_emits_data_source_line_on_outer_tag() {
    let blocks = vec![Block::Figure {
        image: Inline::Image {
            src: Url::resolved("p.jpg", UrlKind::Asset),
            alt: "A".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        },
        caption: Some(vec![Inline::Text("A".into())]),
        width: None,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    }];
    let meta = vec![BlockMeta {
        source_line: Some(9),
    }];
    let html = render_with_meta(blocks, meta);
    assert!(
        html.contains(r#"<figure class="moss-image" data-source-line="9">"#),
        "got: {html}"
    );
}

#[test]
fn no_data_source_line_when_meta_none() {
    // Default `Document::from_blocks` creates meta vec of all
    // `BlockMeta::default()`; nothing should leak.
    let html = render(vec![
        Block::Paragraph(vec![Inline::Text("hi".into())]),
        Block::ThematicBreak,
    ]);
    assert!(
        !html.contains("data-source-line"),
        "default render must NOT emit data-source-line, got: {html}"
    );
}

#[test]
fn end_to_end_parse_with_config_emits_data_source_line() {
    // The full path: parse_with_config → visit_urls_mut → render_document.
    let md = "# Title\n\nfirst paragraph\n\n## Sub\n\nsecond paragraph\n";
    let config = super::super::parser::ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let mut doc = super::super::parser::parse_with_config(md, &config);
    super::super::visit::visit_urls_mut(&mut doc, |u| match u {
        Url::Unresolved(s) => *u = Url::resolved(s.clone(), UrlKind::Internal),
        _ => {}
    });
    let html = render_document(&doc, &DefaultHooks::new());
    assert!(
            html.contains(r##"<h1 id="title" data-source-line="1">Title<a class="moss-heading-anchor" href="#title" aria-label="Permalink to this section"></a></h1>"##),
            "H1 should carry data-source-line=1: {html}"
        );
    assert!(
        html.contains(r#"<p data-source-line="3">first paragraph</p>"#),
        "first paragraph should carry data-source-line=3: {html}"
    );
    assert!(
            html.contains(r##"<h2 id="sub" data-source-line="5">Sub<a class="moss-heading-anchor" href="#sub" aria-label="Permalink to this section"></a></h2>"##),
            "H2 should carry data-source-line=5: {html}"
        );
    assert!(
        html.contains(r#"<p data-source-line="7">second paragraph</p>"#),
        "second paragraph should carry data-source-line=7: {html}"
    );
}
