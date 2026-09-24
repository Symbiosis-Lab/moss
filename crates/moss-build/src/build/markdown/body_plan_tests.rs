//! Tests for the segmented body render.
//!
//! The load-bearing one is [`plan_reassembles_to_render_document_output`]: if a
//! segmented render ever stops reproducing `render_document`'s bytes, every
//! structural decision built on the plan is standing on sand. It runs over a
//! corpus rather than one sample so a new `Block` variant that serializes
//! differently block-by-block than in bulk fails here rather than in a fixture.

use super::*;
use moss_core::ast::{
    parse as parse_raw, parse_with_config as parse_with_config_raw, visit_urls_mut, DefaultHooks,
    ParseConfig, Url, UrlKind,
};

/// Parse the way the pipeline does: parse, then classify every URL. The
/// renderer accepts only `Url::Resolved`, so a test that skips this trips the
/// `visit_urls_mut missing` debug assert instead of exercising the plan.
fn parse(md: &str) -> moss_core::ast::Document {
    let mut doc = parse_raw(md);
    resolve(&mut doc);
    doc
}

fn parse_with_config(md: &str, cfg: &ParseConfig) -> moss_core::ast::Document {
    let mut doc = parse_with_config_raw(md, cfg);
    resolve(&mut doc);
    doc
}

fn resolve(doc: &mut moss_core::ast::Document) {
    visit_urls_mut(doc, |u| {
        if let Url::Unresolved(s) = u {
            let kind = if s.starts_with("http") {
                UrlKind::External
            } else if s.ends_with(".png") || s.ends_with(".jpg") {
                UrlKind::Asset
            } else {
                UrlKind::Internal
            };
            *u = Url::resolved(s.clone(), kind);
        }
    });
}

/// Markdown that exercises every block kind the plan has to segment across:
/// prose, headings, a grid (its own segment), a gallery, a table, code, lists,
/// callouts, blockquotes, math, and CJK prose immediately before a grid — the
/// exact shape that used to abort the build (moss#903 bug 1).
fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("empty", ""),
        ("prose only", "Hello.\n\nSecond paragraph.\n"),
        (
            "cjk prose then grid",
            "潮汐作為一個文學計畫，關注的是非虛構寫作的現場。\n\n\
             :::grid 2\n[A](a/)\n+++\n[B](b/)\n:::\n\nAfter.\n",
        ),
        (
            "grid first block",
            ":::grid 2\n[A](a/)\n+++\n[B](b/)\n:::\n\nTrailing prose.\n",
        ),
        (
            "two grids",
            "Intro.\n\n:::grid 1\n[A](a/)\n:::\n\nBetween.\n\n:::grid 3 1:2:1 {.no-cards}\n[B](b/)\n:::\n",
        ),
        (
            "subscribe between paragraphs",
            "Intro.\n\n:::subscribe {button=\"Join\"}\n:::\n\nAfter.\n",
        ),
        (
            "headings and lede",
            "# Title\n\nIntro paragraph.\n\n## Section\n\nBody text.\n\n### Deeper\n\nMore.\n",
        ),
        (
            "table gallery code list",
            "Intro.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n:::gallery 3\n![](one.png)\n![](two.png)\n:::\n\n\
             ```rust\nlet x = 1;\n```\n\n- one\n- two\n\n1. first\n2. second\n",
        ),
        (
            "callout blockquote hr",
            "> [!note] Heads up\n> Body of the callout.\n\n> plain quote\n\n---\n\nAfter the rule.\n",
        ),
        (
            "compound link cell",
            ":::grid 2\n[![](poster.png)\n\n### Show Title\n\nA description](/shows/one/)\n+++\nPlain cell.\n:::\n",
        ),
        (
            "math and emphasis",
            "Inline $a+b$ and display:\n\n$$\\frac{1}{2}$$\n\n**bold** and *em* and `code`.\n",
        ),
        (
            "footnotes",
            "Para with a marker[^1].\n\nSecond para[^2].\n\n[^1]: First note.\n\n[^2]: Second note.\n",
        ),
    ]
}

#[test]
fn plan_reassembles_to_render_document_output() {
    let hooks = DefaultHooks::new();
    for (name, md) in corpus() {
        let doc = parse(md);
        let flat = moss_core::ast::render_document(&doc, &hooks);
        let plan = render_segmented(&doc, &hooks);
        assert_eq!(
            plan.to_html(),
            flat,
            "segmented render diverged from render_document for '{name}'"
        );
    }
}

#[test]
fn plan_reassembles_with_source_lines_on() {
    // `data-source-line` / `data-source-range` are per-top-level-block, so a
    // segmented walk that lost the meta vec would silently drop them. This is
    // the regression guard for using `render_block_with_meta` over
    // `render_blocks`.
    let cfg = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: true,
        hard_line_breaks: false,
    };
    let hooks = DefaultHooks::new();
    for (name, md) in corpus() {
        let doc = parse_with_config(md, &cfg);
        let flat = moss_core::ast::render_document(&doc, &hooks);
        let plan = render_segmented(&doc, &hooks);
        assert_eq!(
            plan.to_html(),
            flat,
            "segmented render dropped source-line annotations for '{name}'"
        );
    }
    // And prove the annotations are actually present, so the equality above
    // isn't vacuously comparing two un-annotated strings.
    let doc = parse_with_config("Intro.\n\n:::grid 1\n[A](a/)\n:::\n", &cfg);
    let html = render_segmented(&doc, &hooks).to_html();
    assert!(html.contains("data-source-line="), "got: {html}");
    assert!(html.contains("data-source-range="), "got: {html}");
}

#[test]
fn grid_blocks_become_their_own_segment_with_typed_cells() {
    let hooks = DefaultHooks::new();
    let doc = parse("Intro.\n\n:::grid 2\n[A](a/)\n+++\nPlain cell.\n:::\n\nAfter.\n");
    let plan = render_segmented(&doc, &hooks);
    let grids: Vec<&GridEmission> = plan
        .segments
        .iter()
        .filter_map(|s| match s {
            BodySegment::Grid(g) => Some(g),
            _ => None,
        })
        .collect();
    assert_eq!(grids.len(), 1, "expected exactly one grid segment");
    let grid = grids[0];
    assert_eq!(grid.cells.len(), 2);
    assert!(grid.open_tag.starts_with(r#"<div class="moss-grid" data-columns="2""#));
    // The typed cells are carried, not just their markup — a link-only cell
    // arrives as a single `LinkCard`, which is what cell classification keys
    // on instead of hunting for the first `<a href>` in the emitted markup.
    assert!(
        matches!(grid.cells[0].blocks.as_slice(), [Block::LinkCard { .. }]),
        "got: {:?}",
        grid.cells[0].blocks
    );
    assert!(matches!(grid.cells[1].blocks.as_slice(), [Block::Paragraph(_)]));
    // The plain cell carries its content without the card chrome, and the
    // chrome decision alongside it.
    assert_eq!(grid.cells[1].inner(), "<p>Plain cell.</p>");
    assert!(grid.cells[1].parts.carded);
    assert!(!grid.cells[0].parts.carded, "a LinkCard cell brings its own <a> chrome");
}

#[test]
fn no_cards_is_read_from_typed_classes_not_the_serialized_tag() {
    let hooks = DefaultHooks::new();
    let doc = parse(":::grid 2 {.no-cards}\n[A](a/)\n:::\n");
    let plan = render_segmented(&doc, &hooks);
    let BodySegment::Grid(grid) = &plan.segments[0] else {
        panic!("expected a grid segment, got {:?}", plan.segments[0]);
    };
    assert!(grid.no_cards());

    // A class that merely CONTAINS the token must not opt out — the old
    // `grid_opener.contains("no-cards")` check could not tell these apart.
    let doc = parse(":::grid 2 {.no-cards-please}\n[A](a/)\n:::\n");
    let plan = render_segmented(&doc, &hooks);
    let BodySegment::Grid(grid) = &plan.segments[0] else {
        panic!("expected a grid segment");
    };
    assert!(!grid.no_cards());
}

#[test]
fn summary_is_read_from_typed_classes_not_the_serialized_tag() {
    let hooks = DefaultHooks::new();
    let doc = parse(":::grid {.summary}\n[A](a/)\n:::\n");
    let plan = render_segmented(&doc, &hooks);
    let BodySegment::Grid(grid) = &plan.segments[0] else {
        panic!("expected a grid segment, got {:?}", plan.segments[0]);
    };
    assert!(grid.summary());

    // Token-exact, like `no_cards` — `.summary-cards` is a different class.
    let doc = parse(":::grid {.summary-cards}\n[A](a/)\n:::\n");
    let plan = render_segmented(&doc, &hooks);
    let BodySegment::Grid(grid) = &plan.segments[0] else {
        panic!("expected a grid segment");
    };
    assert!(!grid.summary());
}

/// The ADR-034 invariant still holds for a `.summary` fence BEFORE the render
/// pass runs: the class changes what a later phase does with the segment, not
/// what the serializer emitted.
#[test]
fn a_summary_fence_reassembles_byte_identically_before_the_pass() {
    let hooks = DefaultHooks::new();
    let md = "Intro.\n\n:::grid 3 {.summary}\n[A](a/)\n+++\n[B](b/)\n:::\n\nTrailing.\n";
    let doc = parse(md);
    assert_eq!(
        render_segmented(&doc, &hooks).to_html(),
        moss_core::ast::render_document(&doc, &hooks),
    );
}

#[test]
fn map_html_rewrites_every_piece_including_grid_cells() {
    let hooks = DefaultHooks::new();
    let doc = parse("Intro.\n\n:::grid 1\n[A](a/)\n:::\n");
    let mut plan = render_segmented(&doc, &hooks);
    plan.map_html(&|s| s.replace("a/", "REWRITTEN"));
    let html = plan.to_html();
    assert!(html.contains("REWRITTEN"), "got: {html}");
    assert!(!html.contains(r#"href="a/""#), "got: {html}");
}

// ── lede_end ───────────────────────────────────────────────────────────

fn lede_end_of(md: &str) -> (usize, usize) {
    let doc = parse(md);
    (lede_end(&doc.blocks), doc.blocks.len())
}

#[test]
fn lede_ends_at_the_first_heading_after_a_paragraph() {
    // moss#903 bug 4: a long-form article on a cover-bearing folder home was
    // typeset in the narrow cover column for its ENTIRE body because the only
    // recognized release point was a literal `.moss-grid`.
    let (end, total) = lede_end_of(
        "An introduction that belongs beside the cover.\n\n\
         ## The first section\n\nBody.\n\n## Another\n\nMore body.\n",
    );
    assert_eq!(end, 1, "release at the first section heading");
    assert!(end < total);
}

#[test]
fn a_leading_heading_does_not_end_the_lede() {
    // `# Title` then an intro paragraph is one lede, not a release point:
    // releasing at block 0 would leave the cover column empty.
    let (end, _) = lede_end_of("# Title\n\nIntro paragraph.\n");
    assert_eq!(end, 2);
}

#[test]
fn lede_ends_at_a_grid() {
    let (end, _) = lede_end_of("Intro.\n\n:::grid 2\n[A](a/)\n:::\n");
    assert_eq!(end, 1);
}

#[test]
fn lede_ends_at_a_grid_even_as_the_very_first_block() {
    let (end, _) = lede_end_of(":::grid 2\n[A](a/)\n:::\n\nAfter.\n");
    assert_eq!(end, 0);
}

#[test]
fn lede_ends_at_a_gallery_or_table() {
    assert_eq!(lede_end_of("Intro.\n\n:::gallery 2\n![](a.png)\n:::\n").0, 1);
    assert_eq!(
        lede_end_of("Intro.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n").0,
        1
    );
}

#[test]
fn lede_ends_at_a_widened_figure_but_not_a_body_width_one() {
    let mut blocks = vec![
        Block::Paragraph(vec![moss_core::ast::Inline::Text("Intro".into())]),
        Block::Figure {
            image: moss_core::ast::Inline::Text(String::new()),
            caption: None,
            width: Some("body".to_string()),
            align: None,
            class_names: vec![],
            img_style: None,
        },
    ];
    assert_eq!(lede_end(&blocks), blocks.len(), "body width is the default measure");
    if let Block::Figure { width, .. } = &mut blocks[1] {
        *width = Some("page".to_string());
    }
    assert_eq!(lede_end(&blocks), 1, "a widened figure forces a release");
}

#[test]
fn a_short_intro_with_no_release_point_stays_whole() {
    let (end, total) = lede_end_of("Just an intro.\n\nAnd a second line of it.\n");
    assert_eq!(end, total);
}

// ── scroll-row accessible name ────────────────────────────────────────
//
// A `:::grid {scroll}` row with no explicit `label` is otherwise a keyboard
// stop with no accessible name — a screen reader announces an unnamed
// focusable element. `render_segmented` is the one place that can see the
// blocks preceding a top-level grid, so it names an unlabeled row after the
// nearest preceding heading's text (see `nearest_heading_label`), the same
// `aria-label` an author's own `label=` would have produced.

fn grid_open_tag(md: &str) -> String {
    let hooks = DefaultHooks::new();
    let doc = parse(md);
    let plan = render_segmented(&doc, &hooks);
    let BodySegment::Grid(grid) = plan
        .segments
        .into_iter()
        .find(|s| matches!(s, BodySegment::Grid(_)))
        .expect("expected a grid segment")
    else {
        unreachable!()
    };
    grid.open_tag
}

#[test]
fn scroll_row_under_a_heading_is_named_by_that_heading() {
    let open_tag = grid_open_tag(
        "## Related\n\n:::grid 3 {scroll}\nA\n+++\nB\n+++\nC\n+++\nD\n:::\n",
    );
    assert!(
        open_tag.contains(r#"role="region" aria-label="Related""#),
        "got: {open_tag}"
    );
}

#[test]
fn scroll_row_explicit_label_wins_over_the_heading() {
    let open_tag = grid_open_tag(
        "## Related\n\n:::grid 3 {scroll label=\"Custom name\"}\nA\n+++\nB\n+++\nC\n+++\nD\n:::\n",
    );
    assert!(
        open_tag.contains(r#"role="region" aria-label="Custom name""#),
        "got: {open_tag}"
    );
    assert!(!open_tag.contains("Related"), "got: {open_tag}");
}

#[test]
fn scroll_row_with_no_preceding_heading_gets_no_role() {
    let open_tag = grid_open_tag(":::grid 3 {scroll}\nA\n+++\nB\n+++\nC\n+++\nD\n:::\n\nAfter.\n");
    assert!(open_tag.contains(r#"data-scroll tabindex="0""#), "got: {open_tag}");
    assert!(!open_tag.contains("role="), "got: {open_tag}");
    assert!(!open_tag.contains("aria-label"), "got: {open_tag}");
}

#[test]
fn scroll_row_picks_the_nearer_of_two_preceding_headings() {
    let open_tag = grid_open_tag(
        "## First\n\nBody one.\n\n## Second\n\n:::grid 3 {scroll}\nA\n+++\nB\n+++\nC\n+++\nD\n:::\n",
    );
    assert!(
        open_tag.contains(r#"aria-label="Second""#),
        "got: {open_tag}"
    );
    assert!(!open_tag.contains("First"), "got: {open_tag}");
}

#[test]
fn scroll_row_heading_fallback_flattens_inline_markup() {
    // `nearest_heading_label` reuses `inlines_to_plain_text`, the shared
    // flattening policy — a heading with emphasis in it still produces a
    // plain `aria-label`, not the markdown asterisks.
    let open_tag = grid_open_tag(
        "## Read *this* first\n\n:::grid 3 {scroll}\nA\n+++\nB\n+++\nC\n+++\nD\n:::\n",
    );
    assert!(
        open_tag.contains(r#"aria-label="Read this first""#),
        "got: {open_tag}"
    );
}

#[test]
fn a_scroll_row_that_fits_still_gets_the_accessible_name_machinery() {
    // Owner-side rule: a fitting row (`fits_without_scrolling()`) still IS a
    // scroll row (`is_scroll_row()`) — it only looks like a plain grid on a
    // wide screen, and becomes a real scroll region once the viewport
    // narrows — so it needs a name for assistive tech just as much as a row
    // that always scrolls, and gets `data-fits` alongside `data-scroll`.
    let open_tag = grid_open_tag("## Related\n\n:::grid 3 {scroll}\nA\n+++\nB\n+++\nC\n:::\n");
    assert!(open_tag.contains("data-scroll"), "got: {open_tag}");
    assert!(open_tag.contains("data-fits"), "got: {open_tag}");
    assert!(open_tag.contains(r#"tabindex="0""#), "got: {open_tag}");
    assert!(
        open_tag.contains(r#"role="region" aria-label="Related""#),
        "got: {open_tag}"
    );
}

#[test]
fn a_single_cell_scroll_row_gets_no_accessible_name_machinery() {
    // The one case that opts all the way out: nothing to scroll at any
    // width, so `is_scroll_row()` itself is false.
    let open_tag = grid_open_tag("## Related\n\n:::grid 3 {scroll}\nA\n:::\n");
    assert!(!open_tag.contains("data-scroll"), "got: {open_tag}");
    assert!(!open_tag.contains("tabindex"), "got: {open_tag}");
    assert!(!open_tag.contains("role="), "got: {open_tag}");
    assert!(!open_tag.contains("aria-label"), "got: {open_tag}");
}

#[test]
fn plan_lede_segments_split_the_body_without_cutting_an_element() {
    let hooks = DefaultHooks::new();
    let doc = parse(
        "Intro beside the cover.\n\n## Section\n\nFull-width body.\n\n\
         :::grid 2\n[A](a/)\n:::\n",
    );
    let plan = render_segmented(&doc, &hooks);
    let lead: String = plan.segments[..plan.lede_segments]
        .iter()
        .map(|s| s.html().into_owned())
        .collect();
    let trailer: String = plan.segments[plan.lede_segments..]
        .iter()
        .map(|s| s.html().into_owned())
        .collect();
    assert_eq!(format!("{lead}{trailer}"), plan.to_html());
    assert!(lead.contains("Intro beside the cover."), "lead: {lead}");
    assert!(!lead.contains("<h2"), "the heading belongs to the trailer: {lead}");
    assert!(trailer.contains("<h2"), "trailer: {trailer}");
    assert!(trailer.contains("moss-grid"), "trailer: {trailer}");
    // Each side closes every tag it opens — the property the deleted
    // `html_prefix_is_balanced` byte-walker tried to check by scanning.
    assert_eq!(lead.matches('<').count(), lead.matches('>').count());
}
