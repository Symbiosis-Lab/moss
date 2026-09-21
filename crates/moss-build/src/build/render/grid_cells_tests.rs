use super::*;

use crate::build::markdown::body_plan::render_segmented;
use moss_core::ast::{parse, visit_urls_mut, DefaultHooks, Document};
use moss_core::PageKind;

// ── harness ────────────────────────────────────────────────────────────

fn make_doc(title: &str, url_path: &str, cover: Option<&str>) -> ParsedDocument {
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        url_path: url_path.to_string(),
        cover: cover.map(|s| s.to_string()),
        lang: Language::ZhHans,
        kind: PageKind::Article,
        ..Default::default()
    }
}

/// A folder note. `props_for_document` counts children only for a
/// `PageKind::Folder`, which is what page_map makes every folder note.
fn make_folder(title: &str, url_path: &str, cover: Option<&str>) -> ParsedDocument {
    ParsedDocument { kind: PageKind::Folder, ..make_doc(title, url_path, cover) }
}

/// Parse markdown the way the pipeline does: parse, then classify every URL.
/// A cell whose link is still `Url::Unresolved` is deliberately not a link cell
/// (see [`resolved`]), so a test that skipped this would measure nothing.
fn doc_of(md: &str) -> Document {
    let mut doc = parse(md);
    visit_urls_mut(&mut doc, |u| {
        if let Url::Unresolved(s) = u {
            let kind = if s.starts_with("http") {
                UrlKind::External
            } else if s.ends_with(".png") || s.ends_with(".jpg") || s.ends_with(".webp") {
                UrlKind::Asset
            } else {
                UrlKind::Internal
            };
            *u = Url::resolved(s.clone(), kind);
        }
    });
    doc
}

fn plan_of(md: &str) -> BodyPlan {
    render_segmented(&doc_of(md), &DefaultHooks::new())
}

struct Page<'a> {
    url_path: &'a str,
    docs: &'a [ParsedDocument],
    overrides: HashMap<String, String>,
    graph: Option<&'a moss_core::content_graph::ContentGraph>,
    /// Project root. Only the colour passes read it — everything else works
    /// without touching disk, which is why the default is `None`.
    root: Option<String>,
    lang: Language,
    typesetting: Option<&'a str>,
}

impl<'a> Page<'a> {
    fn new(url_path: &'a str, docs: &'a [ParsedDocument]) -> Self {
        Page {
            url_path,
            docs,
            overrides: HashMap::new(),
            graph: None,
            root: None,
            lang: Language::En,
            typesetting: None,
        }
    }

    /// A build that has its content graph, i.e. every real build.
    fn with_graph(mut self, graph: &'a moss_core::content_graph::ContentGraph) -> Self {
        self.graph = Some(graph);
        self
    }

    /// A build rooted at a real directory, so cover images can be read.
    fn with_root(mut self, root: &std::path::Path) -> Self {
        self.root = Some(root.to_string_lossy().into_owned());
        self
    }

    /// The page's typesetting and language, for the passes that read them.
    fn vertical_cjk(mut self) -> Self {
        self.lang = Language::ZhHant;
        self.typesetting = Some("vertical");
        self
    }

    fn index(&self) -> BuildIndex<'_> {
        BuildIndex {
            documents: self.docs,
            graph: self.graph,
            dir_overrides: &self.overrides,
            root_path: self.root.as_deref(),
            media_lookup: None,
            lang: self.lang,
            page_url_path: self.url_path,
            typesetting: self.typesetting,
        }
    }

    /// Render `md`'s body with collection cards applied.
    fn cards(&self, md: &str) -> String {
        let mut plan = plan_of(md);
        apply_collection_cards(&mut plan, &self.index());
        plan.to_html()
    }

    /// Render `md`'s body through the summary-grid pass, in pipeline order
    /// (it runs before collection cards).
    fn summary(&self, md: &str) -> String {
        let mut plan = plan_of(md);
        apply_summary_grids(&mut plan, &self.index());
        apply_collection_cards(&mut plan, &self.index());
        plan.to_html()
    }

    /// Render `md`'s body with cover colours applied, in pipeline order.
    fn cover_colors(&self, md: &str) -> String {
        let mut plan = plan_of(md);
        apply_collection_cards(&mut plan, &self.index());
        apply_cell_cover_colors(&mut plan, &self.index());
        plan.to_html()
    }
}

/// Write a solid-colour PNG at `rel` under a fresh temp root.
fn root_with_image(rel: &str, rgb: [u8; 3]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(4, 4, image::Rgb(rgb))
        .save(&path)
        .unwrap();
    tmp
}

/// Both grid passes in pipeline order, the way `resolve_grid_cells` runs them.
fn both_passes(md: &str, page: &Page<'_>, link_meta: Option<&HashMap<String, LinkMeta>>) -> String {
    let mut plan = plan_of(md);
    apply_collection_cards(&mut plan, &page.index());
    apply_link_previews(&mut plan, link_meta);
    plan.to_html()
}

/// How many collection cards `html` contains.
///
/// The closing quote is load-bearing: a card's own element is
/// `<a … class="moss-card">`, but it CONTAINS four `moss-card-*` descendants
/// (`-cover`, `-content`, `-meta`, `-title`), so a prefix match counts five per
/// card.
fn card_count(html: &str) -> usize {
    html.matches(r#"class="moss-card""#).count()
}

// ── classify_cell ──────────────────────────────────────────────────────

/// The `n`th cell of a body whose FIRST block is a grid. The document is leaked
/// so the returned borrow outlives the call — test-only convenience.
fn classify_nth(md: &str, n: usize) -> GridCell<'static> {
    let doc: &'static Document = Box::leak(Box::new(doc_of(md)));
    let Block::Shortcode(moss_core::ast::Shortcode::Grid(grid)) = &doc.blocks[0] else {
        panic!("expected a grid as the first block");
    };
    classify_cell(&grid.cells[n])
}

#[test]
fn a_bare_link_cell_is_a_whole_cell_link() {
    let GridCell::Link(link) = classify_nth(":::grid 1\n[Writings](writings/)\n:::\n", 0) else {
        panic!("expected a link cell");
    };
    assert_eq!(link.href, "writings/");
    assert_eq!(link.text, "Writings");
    assert!(link.whole_cell, "a lone link is the whole cell");
    assert!(!link.external());
}

#[test]
fn a_link_with_a_soft_wrapped_description_is_a_link_cell() {
    let GridCell::Link(link) =
        classify_nth(":::grid 1\n[MDN](https://mdn.dev)\nWeb docs.\n:::\n", 0)
    else {
        panic!("expected a link cell");
    };
    assert_eq!(link.href, "https://mdn.dev");
    assert_eq!(link.text, "MDN");
    assert!(!link.whole_cell, "the description is outside the link");
    assert!(link.external());
}

#[test]
fn a_link_with_a_separate_description_paragraph_is_a_link_cell() {
    let md = ":::grid 1\n[MDN](https://mdn.dev)\n\nWeb docs.\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Link(_)));
}

#[test]
fn a_cell_whose_link_is_not_first_is_opaque() {
    // Prose beside a link reads as prose, not as a card.
    let md = ":::grid 1\nSee also [Foo](/foo)\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Opaque));
}

#[test]
fn a_cell_with_a_styled_description_is_opaque() {
    // The old scanner rejected anything with a tag after `</a>`; a typed
    // description must be running text for the same reason — a card subtitle
    // slot has no styling.
    let md = ":::grid 1\n[Foo](/foo)\n\n**Bold** description.\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Opaque));
}

#[test]
fn a_heading_cell_is_opaque() {
    let md = ":::grid 1\n### Section\n\nText.\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Opaque));
}

#[test]
fn a_compound_link_cell_carries_its_inner_text() {
    // The SoCiviC pattern: a whole cell wrapped in one link spanning block
    // content. The old scanner recovered the name by stripping tags out of the
    // rendered anchor, which also swept up a heading's permalink `#`.
    let md = ":::grid 1\n[![](poster.png)\n\n### Show Title\n\nA description](/shows/one/)\n:::\n";
    let GridCell::Link(link) = classify_nth(md, 0) else {
        panic!("expected a link cell");
    };
    assert_eq!(link.href, "/shows/one/");
    assert_eq!(link.text, "Show Title A description");
    assert!(!link.text.contains('#'), "no rendered chrome in the name");
    assert!(link.whole_cell);
}

#[test]
fn a_one_item_list_cell_is_the_link_it_contains() {
    // Grids predate `+++` dividers and were authored as markdown lists. The old
    // scanner accepted the shape by matching the first `<a href=` anywhere in
    // the cell; the typed classifier accepts it explicitly.
    let md = ":::grid 3\n- [My Folder](articles/my-folder)\n:::\n";
    let GridCell::Link(link) = classify_nth(md, 0) else {
        panic!("expected a link cell");
    };
    assert_eq!(link.href, "articles/my-folder");
    assert_eq!(link.text, "My Folder");
}

#[test]
fn a_multi_item_list_cell_is_opaque() {
    // The old scanner replaced the WHOLE cell with a card for its first link,
    // silently dropping every later item. Rendering the list the author wrote is
    // the honest outcome.
    let md = ":::grid 3\n- [A](a/)\n- [B](b/)\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Opaque));
}

// ── collection cards ───────────────────────────────────────────────────

#[test]
fn folder_links_in_a_grid_become_collection_cards() {
    let docs = vec![
        make_folder("Design Portfolio", "design/index.html", Some("cover.jpg")),
        make_doc("Project A", "design/project-a/index.html", None),
        make_folder("Code Projects", "code/index.html", None),
        make_doc("App 1", "code/app-1/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 2\n[Design](design/)\n+++\n[Code](code/)\n:::\n");
    assert_eq!(card_count(&html), 2, "got: {html}");
    // The author's link text wins over the linked page's own label.
    assert!(html.contains(">Design<"), "got: {html}");
    assert!(html.contains(">Code<"), "got: {html}");
    // The grid wrapper and its layout attributes survive the substitution.
    assert!(
        html.contains(r#"class="moss-grid" data-columns="2""#),
        "got: {html}"
    );
}

/// The ONE-renderer falsifier for the plain `:::grid` card, the twin of
/// `summary_cells_render_through_the_summary_card_emitter` below: the expected
/// markup is the grid-card emitter's own output for the shared props builder's
/// reading of the same page, never a hand-written literal. A second field-by-
/// field assembly (the `resolve_card` this replaced) would agree with a literal
/// for as long as nobody touched either, and fail this the first time one moved
/// — which is how a hand-picked card came to read `doc.date` alone while the
/// listing card for the same page reached further.
#[test]
fn collection_cells_render_through_the_grid_card_emitter() {
    let mut dated = make_doc("Ink Study", "works/ink-study/index.html", Some("ink.jpg|color=#0a2a3f"));
    dated.date = Some("2024-03-02".to_string());
    dated.description = Some("A study.".to_string());
    let docs = vec![
        dated,
        make_folder("Works", "works/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 2\n[Ink Study](works/ink-study/)\n+++\n[Works](works/)\n:::\n");

    let overrides = HashMap::new();
    let mut eager = true;
    for doc in &docs {
        let props = crate::build::components::child_list::props_for_document(
            doc, &docs, "", &overrides, None,
        );
        let expected = render_item_with_typesetting(&props, None, Language::En, None, None, eager);
        // The first card with a cover carries the LCP hint; only one does here.
        eager = false;
        assert!(
            html.contains(&expected),
            "card for {} did not come from grid_card::render_item_with_typesetting\nexpected: {expected}\ngot: {html}",
            doc.title,
        );
    }
}

/// A folder note whose source folder is spelled differently from its output
/// URL — the shape every mixed-case vault has.
fn folder_note(title: &str, url_path: &str, source_path: &str) -> ParsedDocument {
    ParsedDocument {
        source_path: Some(source_path.to_string()),
        ..make_doc(title, url_path, None)
    }
}

#[test]
fn a_grid_link_to_a_mixed_case_folder_finds_its_card() {
    // The author links the folder as it is spelled on disk; page_map slugified
    // the output URL, so no `url_path` equals the href. The graph knows the
    // source path, and the source path identifies the document — resolving the
    // card must not depend on re-spelling a url_path from the folder name.
    let docs = vec![
        folder_note("Mirror", "mirror/index.html", "MIRROR/index.md"),
        folder_note("Essay", "mirror/essay/index.html", "MIRROR/essay.md"),
    ];
    let graph =
        moss_core::content_graph::ContentGraph::from_paths(&["MIRROR/index.md", "MIRROR/essay.md"]);
    let page = Page::new("index.html", &docs).with_graph(&graph);
    let html = page.cards(":::grid 1\n[The Mirror](MIRROR/)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
    assert!(html.contains(">The Mirror<"), "got: {html}");
    // And the card links to the URL the site actually serves.
    assert!(html.contains(r#"href="/mirror/""#), "got: {html}");
}

#[test]
fn a_grid_link_naming_no_document_stays_uncarded() {
    // The resolver's job on a miss is to answer "nothing", never to synthesize
    // a folder URL that no document occupies (moss#903 bug 3).
    let docs = vec![folder_note("Mirror", "mirror/index.html", "MIRROR/index.md")];
    let graph = moss_core::content_graph::ContentGraph::from_paths(&["MIRROR/index.md"]);
    let page = Page::new("index.html", &docs).with_graph(&graph);
    let html = page.cards(":::grid 1\n[Ghost](GHOST/)\n:::\n");
    assert_eq!(card_count(&html), 0, "got: {html}");
}

#[test]
fn folder_links_without_a_trailing_slash_still_resolve() {
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 1\n[Design](design)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
}

#[test]
fn a_relative_href_resolves_inside_the_page_language_tree() {
    // A grid card link on a non-root page uses a relative href the browser
    // resolves against the page's own directory. With a same-named folder at
    // the site root, the href must anchor to the PAGE's url_path. Regression:
    // the 刘果 `/en/` home's `:::grid [Music](videos.md)` linked to root
    // `/video/` instead of `/en/video/` — both carry `url: video`.
    let docs = vec![
        make_folder("视频", "video/index.html", None),
        make_doc("clip A", "video/a/index.html", None),
        make_doc("clip B", "video/b/index.html", None),
        make_folder("Videos", "en/video/index.html", None),
        make_doc("Ave", "en/video/ave/index.html", None),
    ];
    let page = Page::new("en/index.html", &docs);
    let html = page.cards(":::grid 1\n[Music](video/)\n:::\n");
    // A card's own href is root-relative (`PathResolver::resolve_url`), so the
    // leading slash distinguishes the two collections rather than hiding them.
    assert!(html.contains(r#"href="/en/video/""#), "got: {html}");
    assert!(
        !html.contains(r#"href="/video/""#),
        "matched the root collection: {html}"
    );
    // `en/video/` has one child, the root collection has two — the count is the
    // second witness that the right document was picked.
    assert!(html.contains("1 article"), "got: {html}");
}

#[test]
fn cells_that_resolve_to_nothing_are_left_exactly_as_rendered() {
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 2\n[Nowhere](nowhere/)\n+++\nPlain prose.\n:::\n";
    assert_eq!(page.cards(md), plan_of(md).to_html());
}

#[test]
fn a_partially_resolving_grid_keeps_its_unresolved_cells() {
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 2\n[Design](design/)\n+++\n[Missing](missing/)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
    // The cell that resolved to nothing keeps the serializer's own bytes — a
    // lone-link cell is a `Block::LinkCard`, which brings its own anchor chrome.
    assert!(
        html.contains(
            r#"<a href="missing/" class="moss-grid-card" data-kind="link"><p>Missing</p></a>"#
        ),
        "unresolved cell kept verbatim: {html}"
    );
    assert!(html.contains(r#"data-columns="2""#), "got: {html}");
}

#[test]
fn no_cards_opts_a_grid_out_of_collection_cards() {
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1 {.no-cards}\n[Design](design/)\n:::\n";
    assert_eq!(page.cards(md), plan_of(md).to_html());
}

#[test]
fn a_class_merely_containing_the_token_does_not_opt_out() {
    // The old check was `grid_opener.contains("no-cards")`, which could not
    // tell `.no-cards` from `.no-cards-please`.
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 1 {.no-cards-please}\n[Design](design/)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
}

#[test]
fn external_links_never_become_collection_cards() {
    let docs = vec![make_doc("Design", "design/index.html", None)];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[External](https://example.com)\n:::\n";
    assert_eq!(page.cards(md), plan_of(md).to_html());
}

#[test]
fn an_internal_image_link_cell_keeps_its_authored_image() {
    // A `Block::LinkCard` built around an image is the author's own card —
    // resolving its href to a real page in the build must not throw the
    // image away for an auto-generated page card. Mirrors what an external
    // image-link cell already does (below).
    let docs = vec![make_doc("About", "about/index.html", None)];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 1\n[![Alt text](a.jpg)](about/)\n:::\n");
    assert!(html.contains("<img"), "authored image must survive: {html}");
    assert!(
        !html.contains(r#"class="moss-card""#),
        "must not become an auto page card: {html}"
    );
    assert!(
        html.contains(r#"data-kind="link""#),
        "wraps as an internal link card, the shape the serializer already emits: {html}"
    );
}

#[test]
fn an_internal_image_with_a_caption_keeps_its_authored_image() {
    // `detect_compound_link`'s caption carve-out (moss-core) routes an
    // ordinary image followed by a blank-line caption to `leading_link`
    // instead of `Block::LinkCard` — a different classify_cell shape, same
    // authored-image rule. The cell's own paragraph already links the image,
    // so this shape is left exactly as the serializer rendered it (like
    // `.no-cards`) rather than nesting a second `<a>` around it.
    let docs = vec![make_doc("About", "about/index.html", None)];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[![Poster alt](a.jpg)](about/)\n\nA caption paragraph.\n:::\n";
    let html = both_passes(md, &page, None);
    assert_eq!(
        html,
        plan_of(md).to_html(),
        "left exactly as the serializer rendered it"
    );
    assert!(html.contains("<img"), "authored image must survive: {html}");
    assert!(
        !html.contains(r#"class="moss-card""#),
        "must not become an auto page card: {html}"
    );
    assert!(
        html.contains("A caption paragraph."),
        "the caption must survive too: {html}"
    );
    assert_eq!(
        html.matches("<a ").count(),
        1,
        "exactly one anchor, around the image -- no nested wrapper: {html}"
    );
}

#[test]
fn a_bare_text_internal_link_cell_still_becomes_a_page_card() {
    // The other half of the same distinction: a link with no authored
    // content of its own still asks moss to generate a page card.
    let docs = vec![make_doc("About", "about/index.html", None)];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 1\n[About](about/)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
}

#[test]
fn an_external_image_link_cell_keeps_its_authored_image() {
    // The behaviour the internal case above must mirror: an authored image
    // wrapped in an external link keeps the image, inside the link-preview
    // anchor the serializer already emitted — left exactly as rendered,
    // same as `a_whole_cell_link_keeps_the_chrome_the_serializer_gave_it`.
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[![Alt text](a.jpg)](https://example.org/)\n:::\n";
    let html = both_passes(md, &page, None);
    assert!(html.contains("<img"), "authored image must survive: {html}");
    assert!(html.contains("link-preview"), "got: {html}");
    assert_eq!(
        html,
        plan_of(md).to_html(),
        "left exactly as the serializer rendered it"
    );
}

#[test]
fn no_cards_already_leaves_an_internal_image_link_cell_alone() {
    // `{.no-cards}` skips `apply_collection_cards` entirely, so it never hit
    // this bug: the serializer's own `data-kind="link"` markup survives
    // whether or not the href resolves to a page in the build.
    let docs = vec![make_doc("About", "about/index.html", None)];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1 {.no-cards}\n[![Alt text](a.jpg)](about/)\n:::\n";
    assert_eq!(page.cards(md), plan_of(md).to_html());
    let html = plan_of(md).to_html();
    assert!(html.contains("<img"), "got: {html}");
    assert!(html.contains(r#"data-kind="link""#), "got: {html}");
}

#[test]
fn a_grid_free_body_is_untouched() {
    let docs = vec![make_doc("Design", "design/index.html", None)];
    let page = Page::new("index.html", &docs);
    let md = "Just prose with a [link](design/).\n\n## And a heading\n";
    assert_eq!(page.cards(md), plan_of(md).to_html());
}

#[test]
fn a_cjk_href_resolves_after_percent_decoding() {
    let docs = vec![
        make_doc("古文", "writings/古文/index.html", None),
        make_doc("A", "writings/古文/a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.cards(":::grid 1\n[古文](writings/古文/)\n:::\n");
    assert_eq!(card_count(&html), 1, "got: {html}");
}

// ── link previews ──────────────────────────────────────────────────────

#[test]
fn an_external_link_cell_becomes_a_link_preview() {
    // The old scanner matched `<a href="…"`, but moss-core emits
    // `<a target="_blank" rel="noopener" href="…">` for external links, so this
    // conversion silently stopped happening. Reading the typed cell restores it.
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let html = both_passes(
        ":::grid 1\n[MDN](https://mdn.dev)\nWeb docs.\n:::\n",
        &page,
        None,
    );
    assert!(html.contains("link-preview"), "got: {html}");
    assert!(html.contains("mdn.dev"), "domain row: {html}");
    assert!(html.contains("MDN"), "manual title from the link text: {html}");
}

#[test]
fn a_bare_url_cell_takes_its_title_from_cached_metadata() {
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let mut meta = HashMap::new();
    meta.insert(
        "https://slykiten.com/".to_string(),
        LinkMeta {
            url: "https://slykiten.com/".to_string(),
            title: Some("Sly Kitten".to_string()),
            description: None,
            favicon: None,
            fetched_at: "2026-07-30T00:00:00Z".to_string(),
        },
    );
    let html = both_passes(
        ":::grid 1\n<https://slykiten.com/>\n:::\n",
        &page,
        Some(&meta),
    );
    assert!(html.contains("Sly Kitten"), "got: {html}");
}

#[test]
fn a_bare_url_with_no_cached_metadata_never_fakes_a_title() {
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let html = both_passes(":::grid 1\n<https://slykiten.com/>\n:::\n", &page, None);
    assert!(html.contains("link-preview"), "got: {html}");
    assert!(html.contains("slykiten.com"), "domain row: {html}");
    assert!(!html.contains("link-preview-title"), "no fake title: {html}");
}

#[test]
fn bare_url_cells_are_the_ones_reported_for_prewarm() {
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let mut plan = plan_of(
        ":::grid 3\n<https://a.example/>\n+++\n[Manual](https://b.example/)\nDesc.\n+++\n[Local](local/)\n:::\n",
    );
    apply_collection_cards(&mut plan, &page.index());
    assert_eq!(
        external_urls_needing_fetch(&plan),
        vec!["https://a.example/".to_string()],
        "only the bare URL wants a fetched title"
    );
}

#[test]
fn a_cell_already_turned_into_a_card_is_not_reclassified() {
    // The passes run in sequence on one plan. A cell replaced by pass 1 has its
    // typed content cleared, which is what keeps pass 2 off generated markup.
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = both_passes(":::grid 1\n[Design](design/)\n:::\n", &page, None);
    assert_eq!(card_count(&html), 1, "got: {html}");
    assert!(!html.contains("link-preview"), "got: {html}");
    assert!(!html.contains(r#"data-kind="link""#), "got: {html}");
}

#[test]
fn an_unresolved_internal_link_with_a_description_keeps_its_own_anchor() {
    // Every cell that reaches `preview_markup`'s internal branch is a
    // `leading_link` cell, whose own paragraph already rendered an `<a>` —
    // wrapping it in a second one would nest anchors, so it is left exactly
    // as the serializer rendered it.
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[Foo](/foo)\nA description.\n:::\n";
    let html = both_passes(md, &page, None);
    assert_eq!(
        html,
        plan_of(md).to_html(),
        "left exactly as the serializer rendered it"
    );
    assert!(!html.contains("link-preview"), "got: {html}");
    assert!(html.contains("A description."), "got: {html}");
    assert_eq!(
        html.matches("<a ").count(),
        1,
        "exactly one anchor -- no nested wrapper: {html}"
    );
}

#[test]
fn an_unresolved_internal_text_link_with_a_caption_keeps_its_own_anchor() {
    // The blank-line-separated-caption shape, unresolved: `cell.inner()`
    // already has its own `<a>` from the link's own paragraph, so wrapping
    // it again would nest anchors.
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md =
        ":::grid 1\n[Missing](missing.md)\n\nA caption paragraph under a text link.\n:::\n";
    let html = both_passes(md, &page, None);
    assert_eq!(
        html,
        plan_of(md).to_html(),
        "left exactly as the serializer rendered it"
    );
    assert!(
        html.contains("A caption paragraph under a text link."),
        "got: {html}"
    );
    assert_eq!(
        html.matches("<a ").count(),
        1,
        "exactly one anchor -- no nested wrapper: {html}"
    );
}

#[test]
fn no_cards_resolving_internal_text_link_with_a_caption_keeps_its_own_anchor() {
    // `.no-cards` skips `apply_collection_cards`, but `apply_link_previews`
    // runs unconditionally on every grid — it must not nest an anchor here
    // either, whether or not the link resolves to a page in the build.
    let docs = vec![make_doc("About", "about/index.html", None)];
    let page = Page::new("index.html", &docs);
    let md =
        ":::grid 1 {.no-cards}\n[About](about/)\n\nA caption paragraph under a text link.\n:::\n";
    let html = both_passes(md, &page, None);
    assert_eq!(
        html,
        plan_of(md).to_html(),
        "left exactly as the serializer rendered it"
    );
    assert_eq!(
        html.matches("<a ").count(),
        1,
        "exactly one anchor -- no nested wrapper: {html}"
    );
}

#[test]
fn a_whole_cell_link_keeps_the_chrome_the_serializer_gave_it() {
    // A `LinkCard` cell already IS an `<a class="moss-grid-card">`. Re-wrapping
    // it would nest anchors.
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[Foo](/foo)\n:::\n";
    assert_eq!(both_passes(md, &page, None), plan_of(md).to_html());
}

#[test]
fn a_two_link_cell_is_left_alone_by_both_passes() {
    // Also the two-link classify case: this asserts the output is byte-identical
    // to the unmodified plan, which is strictly more than "classifies as Opaque".
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1\n[Foo](/foo) [Bar](/bar)\n:::\n";
    assert_eq!(both_passes(md, &page, None), plan_of(md).to_html());
}

// ── moss#903 bug 1: CJK prose before a grid ────────────────────────────

#[test]
fn cjk_prose_immediately_before_a_grid_builds_and_places_the_grid_correctly() {
    // The whole reason this module exists. `html_prefix_is_balanced` advanced a
    // byte cursor one byte at a time and then sliced, so the first multi-byte
    // character in a folder-cover page's lede aborted the entire build:
    // "start byte index 66 is not a char boundary; it is inside '在'".
    //
    // Segment boundaries are block boundaries, so there is no cursor to
    // misplace — and the grid still lands where it belongs.
    let docs = vec![
        make_doc("文字", "writings/index.html", None),
        make_doc("A", "writings/a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let md = "潮汐作為一個文學計畫，關注的是非虛構寫作的現場。\n\n\
              :::grid 1\n[文字](writings/)\n:::\n\n之後的段落。\n";

    let mut plan = plan_of(md);
    apply_collection_cards(&mut plan, &page.index());
    apply_link_previews(&mut plan, None);
    let (lead, trailer) = plan.split_at_lede();

    // The lede stays in the cover column, the grid is released past it.
    assert!(lead.contains("潮汐作為一個文學計畫"), "lead: {lead}");
    assert!(
        !lead.contains("moss-grid"),
        "the grid escaped the column: {lead}"
    );
    assert!(
        trailer.contains(r#"class="moss-grid""#),
        "trailer: {trailer}"
    );
    assert!(
        trailer.contains(r#"class="moss-card"#),
        "card rendered: {trailer}"
    );
    assert!(trailer.contains("之後的段落。"), "trailer: {trailer}");
    // Lossless, and neither side ends mid-element.
    assert_eq!(format!("{lead}{trailer}"), plan.to_html());
    assert_eq!(lead.matches('<').count(), lead.matches('>').count());
}

// ── moss#903 bug 4: long-form body trapped in the cover column ─────────

#[test]
fn a_long_article_on_a_cover_page_is_not_trapped_in_the_narrow_column() {
    // Before ADR-034 the ONLY release point was a literal `.moss-grid` match,
    // so a cover-bearing folder home whose body is a long article typeset its
    // entire body in the narrow cover-body column, with half the viewport empty
    // beside it.
    let md = "A short standfirst that belongs beside the cover.\n\n\
              ## The first section\n\nSeveral paragraphs of body text.\n\n\
              ## The second section\n\nMore body text, still no grid anywhere.\n\n\
              ### A subsection\n\nAnd more again.\n";
    let plan = plan_of(md);
    let (lead, trailer) = plan.split_at_lede();

    assert!(lead.contains("standfirst"), "the standfirst stays: {lead}");
    assert!(!lead.contains("<h2"), "the article proper is released: {lead}");
    assert!(!lead.contains("first section"), "lead: {lead}");
    assert!(trailer.contains("The first section"), "trailer: {trailer}");
    assert!(trailer.contains("The second section"), "trailer: {trailer}");
    assert!(trailer.contains("A subsection"), "trailer: {trailer}");
    assert_eq!(format!("{lead}{trailer}"), plan.to_html());
}

#[test]
fn a_body_that_is_only_a_standfirst_stays_beside_the_cover() {
    // The other direction: releasing eagerly would leave the cover column empty
    // and the cover floating beside nothing.
    let plan = plan_of("Just a short introduction.\n\nAnd a second line of it.\n");
    let (lead, trailer) = plan.split_at_lede();
    assert!(lead.contains("short introduction"), "lead: {lead}");
    assert!(trailer.is_empty(), "nothing to release: {trailer:?}");
}

#[test]
fn a_cell_of_image_label_and_heading_link_is_opaque() {
    // The shape an author reaches for when hand-building what they wanted a
    // card to be: the cover, a kicker, and the title as a linked heading. It is
    // three blocks, so no card is built and the author gets exactly what they
    // typed. Documented in docs/reference/shortcode-grammar.md as the trap,
    // because the fix is counter-intuitive — DELETE the hand-built parts and
    // let the linked page's own `cover:` and `description:` supply them.
    let md = ":::grid 1\n![](poster.png)\n\nPart One\n\n### [Title](/works/one/)\n:::\n";
    assert!(matches!(classify_nth(md, 0), GridCell::Opaque));
}

#[test]
fn a_hand_built_cell_publishes_its_cover_images_color() {
    // The cell above is Opaque, so no card is built for it — and before this
    // pass that also meant no colour, leaving a theme that wanted the same
    // coloured band behind a bespoke layout with nothing to paint. The image
    // in the cell's cover position now supplies one on the cell's own
    // `.moss-grid-card`, the same variable a collection card carries.
    let tmp = root_with_image("poster.png", [48, 96, 160]);
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs).with_root(tmp.path());
    let md = ":::grid 1\n![](/poster.png)\n\nPart One\n\n### [Title](/works/one/)\n:::\n";
    let html = page.cover_colors(md);
    assert!(html.contains("data-cover-color"), "got: {html}");
    assert!(
        html.contains(r#"<div class="moss-grid-card" data-cover-color style="--moss-cover-color: hsla(214, 54%, 41%, 1)">"#),
        "got: {html}"
    );
}

#[test]
fn an_image_below_the_cells_text_is_not_a_cover() {
    // Only the FIRST block counts. An image further down is an illustration
    // inside the author's prose; colouring the whole cell from it would tint
    // text the image sits below.
    let tmp = root_with_image("poster.png", [48, 96, 160]);
    let docs: Vec<ParsedDocument> = vec![];
    let page = Page::new("index.html", &docs).with_root(tmp.path());
    let md = ":::grid 1\nPart One\n\n![](/poster.png)\n:::\n";
    let html = page.cover_colors(md);
    assert!(!html.contains("data-cover-color"), "got: {html}");
}

#[test]
fn a_cell_that_became_a_card_keeps_the_cards_own_color() {
    // The card brought its own chrome and its own band colour; re-wrapping it
    // with a second one would paint the colour of whatever image the author's
    // now-discarded cell happened to open with.
    let tmp = root_with_image("poster.png", [48, 96, 160]);
    let docs = vec![make_doc("Harbour", "works/harbour/index.html", Some("harbour.jpg|color=#0a2a3f"))];
    let page = Page::new("index.html", &docs).with_root(tmp.path());
    let html = page.cover_colors(":::grid 1\n[Harbour](works/harbour/)\n:::\n");
    assert_eq!(html.matches("data-cover-color").count(), 1, "got: {html}");
    assert!(html.contains("--moss-cover-color: hsla(203, 73%, 14%, 1)"), "got: {html}");
}

/// A `:::grid` folder card counts in Chinese numerals on a vertical CJK page.
///
/// `card_markup` passed a hard-coded `None` for typesetting, on the grounds
/// that a title plus a count has no vertical variant. It does: the count runs
/// through `i18n::article_count_label`, which writes 四篇 rather than `4 篇`
/// under vertical CJK — and `4` in a vertical column lies on its side
/// (zhu-da home, 2026-09-11).
#[test]
fn a_grid_folder_card_counts_in_chinese_under_vertical_cjk() {
    let docs = vec![
        make_folder("書", "書/index.html", None),
        make_doc("蘭亭序", "書/蘭亭序/index.html", None),
        make_doc("千字文", "書/千字文/index.html", None),
        make_doc("祭姪文稿", "書/祭姪文稿/index.html", None),
        make_doc("寒食帖", "書/寒食帖/index.html", None),
    ];
    let md = ":::grid 1\n[書](書/)\n:::\n";

    let vertical = Page::new("index.html", &docs).vertical_cjk().cards(md);
    assert!(vertical.contains("四篇"), "got: {vertical}");
    assert!(!vertical.contains("4 篇"), "got: {vertical}");

    // Horizontally the same card keeps the Arabic digit and the space.
    let mut horizontal_page = Page::new("index.html", &docs);
    horizontal_page.lang = Language::ZhHant;
    let horizontal = horizontal_page.cards(md);
    assert!(horizontal.contains("4 篇"), "got: {horizontal}");
}


// ── :::grid {.summary} ─────────────────────────────────────────────────

/// A page with everything a summary card reads.
fn make_page_doc(title: &str, url_path: &str, date: &str, description: &str) -> ParsedDocument {
    ParsedDocument {
        date: Some(date.to_string()),
        description: Some(description.to_string()),
        ..make_doc(title, url_path, None)
    }
}

/// The ONE-renderer falsifier: the expected markup is the summary emitter's own
/// output for the same props, never a hand-written literal. A parallel copy of
/// the emitter would pass a literal-based test forever and fail this one the
/// first time either copy moved.
#[test]
fn summary_cells_render_through_the_summary_card_emitter() {
    let docs = vec![
        make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study."),
        make_page_doc("Second Hand", "works/second-hand/index.html", "2023-08-19", "Another."),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.summary(":::grid {.summary}\n[Ink Study](works/ink-study/)\n+++\n[Second Hand](works/second-hand/)\n:::\n");

    let overrides = HashMap::new();
    for doc in &docs {
        let props = crate::build::components::child_list::props_for_document(
            doc, &docs, "", &overrides, None,
        );
        let expected = crate::build::components::child_summary::render_with_sort(
            &props,
            Language::En,
            None,
            None,
            moss_core::sort::SortAxis::Date,
        );
        assert!(
            html.contains(&expected),
            "card for {} did not come from child_summary::render_with_sort\nexpected: {expected}\ngot: {html}",
            doc.title,
        );
    }
    // Author order, not date order: Ink Study (2024) was picked second in the
    // corpus but written first in the fence.
    assert!(
        html.find("Ink Study").unwrap() < html.find("Second Hand").unwrap(),
        "cell order is the author's: {html}"
    );
}

/// `.summary` REPLACES the grid's container. Summary styling rides entirely on
/// `.moss-cards[data-layout="list"]`; a `.moss-grid` that merely contained
/// `.moss-card` elements would need a second stylesheet.
#[test]
fn a_summary_grid_wears_the_listing_wrapper() {
    let docs = vec![make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study.")];
    let page = Page::new("index.html", &docs);
    let html = page.summary(":::grid {.summary}\n[Ink Study](works/ink-study/)\n:::\n");

    assert!(html.contains(r#"<div class="moss-cards-container" data-embed>"#), "got: {html}");
    assert!(html.contains(r#"<div class="moss-cards" data-layout="list">"#), "got: {html}");
    assert!(!html.contains("moss-grid"), "no grid container survives: {html}");
}

/// A summary card is a full-measure row, so a column count means nothing — and
/// it is dropped BY CONSTRUCTION: the opener carrying `data-columns` is never
/// emitted, so no theme can act on a leftover attribute.
#[test]
fn a_column_count_does_not_survive_the_summary_variant() {
    let docs = vec![make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study.")];
    let page = Page::new("index.html", &docs);
    let html = page.summary(":::grid 3 {.summary}\n[Ink Study](works/ink-study/)\n:::\n");

    assert!(!html.contains("data-columns"), "got: {html}");
    assert!(!html.contains("moss-grid"), "got: {html}");
}

/// `scroll`'s attributes live on the opener the summary variant discards
/// entirely (it becomes a `.moss-cards-container` `BodySegment::Html`, not a
/// `.moss-grid`), so `.summary` wins over `scroll` the same way it already
/// wins over a column count: nothing downstream ever sees `data-scroll`.
#[test]
fn scroll_does_not_survive_the_summary_variant() {
    let docs = vec![make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study.")];
    let page = Page::new("index.html", &docs);
    let html = page.summary(":::grid 3 {.summary scroll}\n[Ink Study](works/ink-study/)\n:::\n");

    assert!(!html.contains("data-scroll"), "got: {html}");
    assert!(!html.contains("moss-grid"), "got: {html}");
}

/// Token-exact on the typed class list, the way `no-cards` is — `.summary-cards`
/// is a different class and renders an ordinary grid.
#[test]
fn a_class_merely_containing_summary_does_not_convert() {
    let docs = vec![
        make_doc("Design Portfolio", "design/index.html", None),
        make_doc("Project A", "design/project-a/index.html", None),
    ];
    let page = Page::new("index.html", &docs);
    let html = page.summary(":::grid 1 {.summary-cards}\n[Design](design/)\n:::\n");

    assert!(html.contains("moss-grid"), "got: {html}");
    assert!(!html.contains("moss-cards-container"), "got: {html}");
    assert_eq!(card_count(&html), 1, "got: {html}");
}

/// "Do not convert my cells" is strictly stronger than "convert my cells into
/// this shape". An author who writes both has contradicted themselves in a
/// direction where doing nothing is the safe reading.
#[test]
fn no_cards_beats_summary() {
    let docs = vec![
        make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study."),
    ];
    let page = Page::new("index.html", &docs);
    let md = ":::grid 1 {.summary .no-cards}\n[Ink Study](works/ink-study/)\n:::\n";
    assert_eq!(page.summary(md), plan_of(md).to_html());
}

/// A wrong link path shows the author their own content, not an empty styled
/// block — the courtesy `apply_collection_cards` already extends.
#[test]
fn a_summary_grid_that_resolves_nothing_is_left_alone() {
    let docs = vec![make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study.")];
    let page = Page::new("index.html", &docs);
    let md = ":::grid {.summary}\n[Typo](workz/ink-study/)\n:::\n";
    assert_eq!(page.summary(md), plan_of(md).to_html());
}

/// A mixed fence: the cell that resolves becomes a card, the one that does not
/// contributes the author's markup unwrapped — `.moss-grid-card` chrome is
/// scoped to `.moss-grid` and would render unstyled inside a cards list.
#[test]
fn an_unresolvable_cell_keeps_the_author_markup() {
    let docs = vec![make_page_doc("Ink Study", "works/ink-study/index.html", "2024-03-02", "A study.")];
    let page = Page::new("index.html", &docs);
    let html = page.summary(
        ":::grid {.summary}\n[Ink Study](works/ink-study/)\n+++\nJust a note.\n:::\n",
    );

    assert_eq!(html.matches(r#"class="moss-card""#).count(), 1, "got: {html}");
    assert!(html.contains("<p>Just a note.</p>"), "got: {html}");
    assert!(!html.contains("moss-grid-card"), "no grid chrome: {html}");
    assert!(
        html.find("Ink Study").unwrap() < html.find("Just a note.").unwrap(),
        "order preserved: {html}"
    );
}

/// The zhu-da case, and the regression `render_item_with_typesetting` had to be
/// taught on 2026-09-11: the count runs through `i18n::article_count_label`,
/// which writes 四篇 rather than `4 篇` under vertical CJK. Fails if
/// `typesetting` is not threaded into `render_with_sort`.
#[test]
fn a_folder_card_counts_in_cjk_numerals_under_vertical_typesetting() {
    let docs = vec![
        make_folder("書", "書/index.html", None),
        make_doc("蘭亭序", "書/蘭亭序/index.html", None),
        make_doc("千字文", "書/千字文/index.html", None),
        make_doc("祭姪文稿", "書/祭姪文稿/index.html", None),
        make_doc("寒食帖", "書/寒食帖/index.html", None),
    ];
    let md = ":::grid {.summary}\n[書](書/)\n:::\n";

    // `<div class="moss-card-meta">` is the SUMMARY emitter's meta slot; the
    // grid card writes a `<span>`. Asserting the element pins that the count
    // came through this variant, not through a collection card that happens to
    // agree about numerals.
    let vertical = Page::new("index.html", &docs).vertical_cjk().summary(md);
    assert!(
        vertical.contains(r#"<div class="moss-card-meta">四篇</div>"#),
        "got: {vertical}"
    );
    assert!(!vertical.contains("4 篇"), "got: {vertical}");

    let mut horizontal_page = Page::new("index.html", &docs);
    horizontal_page.lang = Language::ZhHant;
    let horizontal = horizontal_page.summary(md);
    assert!(
        horizontal.contains(r#"<div class="moss-card-meta">4 篇</div>"#),
        "got: {horizontal}"
    );
}
