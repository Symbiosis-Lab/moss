//! Visitor helpers over the typed AST.
//!
//! Pattern matching is the visitor framework (no `Visit` trait, no
//! `Box<dyn Node>`). These free functions exist for the cases that
//! genuinely need recursive descent across every variant — URL resolution,
//! shortcode-presence queries — and would otherwise be repeated in every
//! consumer.
//!
//! ## When to add a visitor here
//!
//! Add a free function only when the alternative is repeated recursive
//! traversal across multiple call sites. Per the design doc principle P4:
//! one-off transformations belong inline as `match block { ... }`.

use super::document::Document;
use super::node::{Block, Inline};
use super::shortcode::{Shortcode, ShortcodeKind};
use super::url::Url;

/// One mutable walk over the typed tree: every URL, and every piece of raw
/// HTML a block or an inline carries (a folder embed's marker is one of them).
trait MutVisitor {
    fn url(&mut self, url: &mut Url);
    fn raw_html(&mut self, html: &mut String);
}

/// Visits URLs only.
struct UrlVisitor<F>(F);

impl<F: FnMut(&mut Url)> MutVisitor for UrlVisitor<F> {
    fn url(&mut self, url: &mut Url) {
        (self.0)(url);
    }
    fn raw_html(&mut self, _html: &mut String) {}
}

/// Visits raw HTML only.
struct RawHtmlVisitor<F>(F);

impl<F: FnMut(&mut String)> MutVisitor for RawHtmlVisitor<F> {
    fn url(&mut self, _url: &mut Url) {}
    fn raw_html(&mut self, html: &mut String) {
        (self.0)(html);
    }
}

/// Visit every raw-HTML node in `block` and everything nested under it (a
/// callout, a list item, a grid cell, a link's text), with a callback that may
/// rewrite the HTML in place.
pub fn visit_block_raw_html_mut<F>(block: &mut Block, callback: F)
where
    F: FnMut(&mut String),
{
    visit_urls_in_block(block, &mut RawHtmlVisitor(callback));
}

/// Visit every URL in the document with a callback that may mutate it
/// in place. Walks links, image srcs, and all nested block/inline content.
///
/// Used by the resolve-classification pass: the upstream resolve pipeline
/// has already rewritten markdown sources so URLs come out as one of the
/// shapes documented in [`crate::ast::url::Url`]. The callback inspects the
/// raw string and replaces it with a [`crate::ast::url::Url::Resolved`].
pub fn visit_urls_mut<F>(doc: &mut Document, callback: F)
where
    F: FnMut(&mut Url),
{
    let mut visitor = UrlVisitor(callback);
    for block in &mut doc.blocks {
        visit_urls_in_block(block, &mut visitor);
    }
}

fn visit_urls_in_block<V>(block: &mut Block, v: &mut V)
where
    V: MutVisitor,
{
    match block {
        Block::Heading { children, .. } => {
            for inline in children {
                visit_urls_in_inline(inline, v);
            }
        }
        Block::Paragraph(children) | Block::DefinitionTitle(children) => {
            for inline in children {
                visit_urls_in_inline(inline, v);
            }
        }
        Block::Callout { children, .. } | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                visit_urls_in_block(nested, v);
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    visit_urls_in_block(nested, v);
                }
            }
        }
        Block::Table { header, rows, .. } => {
            for cell in header {
                for inline in cell {
                    visit_urls_in_inline(inline, v);
                }
            }
            for row in rows {
                for cell in row {
                    for inline in cell {
                        visit_urls_in_inline(inline, v);
                    }
                }
            }
        }
        Block::BlockQuote(children)
        | Block::DefinitionList(children)
        | Block::DefinitionDefinition(children) => {
            for nested in children {
                visit_urls_in_block(nested, v);
            }
        }
        Block::Shortcode(sc) => {
            visit_urls_in_shortcode(sc, v);
        }
        Block::Figure { image, caption, .. } => {
            // Descend into the image's src (the load-bearing URL); the
            // caption is a Vec<Inline> that may itself carry links —
            // unlikely in practice (captions default to alt text) but the
            // visitor must not silently skip them.
            visit_urls_in_inline(image, v);
            if let Some(cap_inlines) = caption {
                for inline in cap_inlines {
                    visit_urls_in_inline(inline, v);
                }
            }
        }
        Block::LinkCard { url, children } => {
            // Phase 4 PR4.5: the wrapping URL (compound-link href) +
            // every URL inside the inner block content.
            v.url(url);
            for nested in children {
                visit_urls_in_block(nested, v);
            }
        }
        Block::Other(html) => v.raw_html(html),
        Block::CodeBlock { .. } | Block::ThematicBreak => {
            // No URLs in these.
        }
    }
}

fn visit_urls_in_shortcode<V>(sc: &mut super::shortcode::Shortcode, v: &mut V)
where
    V: MutVisitor,
{
    use super::shortcode::Shortcode;
    match sc {
        Shortcode::Subscribe(_) => {} // No URLs.
        Shortcode::Buttons(args) => {
            for item in &mut args.items {
                v.url(&mut item.url);
            }
        }
        Shortcode::Gallery(args) => {
            for item in &mut args.items {
                v.url(&mut item.src);
                for inline in &mut item.caption {
                    visit_urls_in_inline(inline, v);
                }
            }
        }
        Shortcode::Hero(args) => {
            if let Some(image) = args.image.as_mut() {
                v.url(image);
            }
            for image in &mut args.extra_images {
                v.url(image);
            }
            // Phase 4 PR4.5 (2026-05-28): descend into the typed overlay
            // blocks so URLs inside `:::hero` overlay markdown (e.g. a
            // `[Read more](/x)` link in the overlay copy) get classified
            // by the same visitor pass.
            for block in &mut args.overlay {
                visit_urls_in_block(block, v);
            }
            // Captions are inline markdown too; a link in one is a body link.
            for inline in &mut args.caption {
                visit_urls_in_inline(inline, v);
            }
            for caption in &mut args.slide_captions {
                for inline in caption {
                    visit_urls_in_inline(inline, v);
                }
            }
        }
        Shortcode::Grid(args) => {
            // Phase 4 PR4.5 (2026-05-28): cells are now typed Vec<Block>;
            // descend into each cell. Compound-link cells render through
            // `Block::LinkCard { url, children }`, whose own visit arm
            // walks both the wrapping href and the inner children.
            for cell_blocks in &mut args.cells {
                for block in cell_blocks {
                    visit_urls_in_block(block, v);
                }
            }
        }
        Shortcode::Recent(_) => {} // No URLs.
    }
}

fn visit_urls_in_inline<V>(inline: &mut Inline, v: &mut V)
where
    V: MutVisitor,
{
    match inline {
        Inline::Link { url, children, .. } => {
            v.url(url);
            for nested in children {
                visit_urls_in_inline(nested, v);
            }
        }
        Inline::Image { src, .. } => {
            v.url(src);
        }
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            for nested in children {
                visit_urls_in_inline(nested, v);
            }
        }
        Inline::Other(html) => v.raw_html(html),
        Inline::Text(_)
        | Inline::Code(_)
        | Inline::LineBreak
        | Inline::FootnoteRef(_)
        | Inline::TaskMarker(_) => {}
    }
}

/// Visit every block (top-level + nested) with a read-only callback. The
/// callback returns `false` to short-circuit the traversal (any returned
/// `false` makes the whole walk return `false`).
///
/// Used for queries like "does any block contain a `:::subscribe`
/// shortcode?" — the body of `has_shortcode_recursive` below.
pub fn visit_blocks<F>(doc: &Document, mut callback: F) -> bool
where
    F: FnMut(&Block) -> bool,
{
    for block in &doc.blocks {
        if !visit_block(block, &mut callback) {
            return false;
        }
    }
    true
}

fn visit_block<F>(block: &Block, callback: &mut F) -> bool
where
    F: FnMut(&Block) -> bool,
{
    callback(block) && for_each_child_sequence(block, |blocks| blocks.iter().all(|nested| visit_block(nested, callback)))
}

/// Hand each block sequence `block` directly owns to `f`, in document order,
/// stopping at the first `false`. The one list of block containers the
/// read-only walks share, public so a query that needs a block's neighbours
/// (a marker comment that applies to the image before it) can recurse at the
/// block and keep page order. Every variant is named, so a new container
/// fails to compile here instead of being skipped by every walk at once.
pub fn for_each_child_sequence(block: &Block, mut f: impl FnMut(&[Block]) -> bool) -> bool {
    use super::shortcode::Shortcode;
    match block {
        // A footnote definition is a block container like any other: a walk
        // that skipped a note's body would answer "does any block …?" with a
        // No that only means "not anywhere I looked".
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::DefinitionList(children)
        | Block::DefinitionDefinition(children)
        | Block::FootnoteDefinition { children, .. }
        // The compound-link cell's inner content (image, heading, paragraphs).
        | Block::LinkCard { children, .. } => f(children),
        Block::List { items, .. } => items.iter().all(|item| f(item)),
        Block::Shortcode(Shortcode::Grid(args)) => args.cells.iter().all(|cell| f(cell)),
        Block::Shortcode(Shortcode::Hero(args)) => f(&args.overlay),
        Block::Shortcode(
            Shortcode::Subscribe(_) | Shortcode::Buttons(_) | Shortcode::Gallery(_) | Shortcode::Recent(_),
        ) => true,
        // No block children. Inline children of headings, paragraphs, terms,
        // tables and figures are visited by inline visitors, not block ones.
        Block::Heading { .. }
        | Block::Paragraph(_)
        | Block::DefinitionTitle(_)
        | Block::CodeBlock { .. }
        | Block::Table { .. }
        | Block::ThematicBreak
        | Block::Figure { .. }
        | Block::Other(_) => true,
    }
}

/// Core of every single-predicate `has_*_recursive` query below: true if any
/// block in the document (recursive, via [`visit_blocks`]) matches `pred`.
/// `has_callout_recursive` stays separate — its predicate is a multi-arm
/// match over several block shapes, not a single test, so folding it in here
/// would hide that shape rather than share it.
fn any_block(doc: &Document, pred: impl Fn(&Block) -> bool) -> bool {
    let mut found = false;
    visit_blocks(doc, |block| {
        if pred(block) {
            found = true;
            return false; // short-circuit
        }
        true
    });
    found
}

/// True if any block in the document is a shortcode of the given kind
/// (recursive — descends into callouts, blockquotes, list items).
///
/// Replaces the `project_has_inline_subscribe` filesystem scan once
/// shortcodes migrate to typed AST in Phase B.
pub fn has_shortcode_recursive(doc: &Document, kind: ShortcodeKind) -> bool {
    any_block(doc, |block| matches!(block, Block::Shortcode(sc) if sc.kind() == kind))
}

/// True if any `:::grid` in the document is a scroll row at all — see
/// [`GridShortcode::is_scroll_row`] — recursive, like
/// [`has_shortcode_recursive`]. Gates the scroll-row runtime script, so a
/// site with no scroll row ships none of it. A `{scroll}` grid whose cells
/// all fit still counts: it only *looks* like a plain grid on a wide
/// screen, but it needs the script's resize-driven `tabindex`/dots toggling
/// to become a real scroller once the viewport narrows — only a one-cell
/// `{scroll}` grid, which is never a scroll row at any width, ships none of
/// it.
pub fn has_scroll_row_recursive(doc: &Document) -> bool {
    any_block(
        doc,
        |block| matches!(block, Block::Shortcode(Shortcode::Grid(grid)) if grid.is_scroll_row()),
    )
}

/// True if any block in the document is a callout (recursive — a callout
/// nested inside a list item or another callout counts).
///
/// Gates the `callouts` site stylesheet partial: a build whose every page
/// answers `false` here never ships `assets/css/site/callouts.css`. The
/// query is a lowering of the typed tree, never a scan of emitted HTML —
/// parse once, lower to many.
///
/// # Shapes beyond `Block::Callout`
///
/// The gate is only as complete as the typed tree, and three documented paths
/// reach a `class="callout"` element without a `Block::Callout`:
///
/// 1. **`:::recent` fallback text.** `Recent.fallback_markdown` is a raw
///    `String`, not `Vec<Block>` like `Grid.cells` and `Hero.overlay` — it is
///    parsed at HTML-emit time. `visit_blocks` cannot descend into it, so this
///    function re-parses it for callout syntax below. Promoting the field to
///    `Vec<Block>` would delete that special case; until then it is the one
///    place this query looks at text rather than structure.
/// 2. **The pure-CSS region `:::{.callout}`.** `shortcode_extract` lowers an
///    empty-name fenced div to a literal `<div class="callout">` in a
///    `Block::Other` — see [`html_opens_a_callout`]. This is documented
///    authoring syntax, not hand-rolled markup, so it must gate the partial.
/// 3. **`{.callout}` on a typed shortcode** (`:::grid 2 {.callout}`), whose
///    classes live in the typed struct — see [`shortcode_classes`].
///
/// # What it still cannot see
///
/// A `.moss/theme/` script that injects a callout at runtime is outside the
/// AST entirely, and so is HTML a **plugin** adds in the `enhance` hook —
/// which runs after `SiteAssets` is folded, so even an AST query could not
/// help. Both are outside the "declared, never inferred" contract the
/// stylesheet module states: a site that hand-rolls moss's internal markup at
/// runtime is asking for the class without asking for the feature.
pub fn has_callout_recursive(doc: &Document) -> bool {
    let mut found = false;
    visit_blocks(doc, |block| {
        match block {
            Block::Callout { .. } => {
                found = true;
                return false; // short-circuit
            }
            Block::Shortcode(Shortcode::Recent(r)) if markdown_has_callout(&r.fallback_markdown) => {
                found = true;
                return false;
            }
            Block::Other(html) if html_opens_a_callout(html) => {
                found = true;
                return false;
            }
            Block::Shortcode(sc) if shortcode_classes(sc).is_some_and(has_callout_class) => {
                found = true;
                return false;
            }
            _ => {}
        }
        true
    });
    found
}

/// True if `class_list` (a space-separated `{.a .b}` or `class="…"` value)
/// contains `callout` as a whole token.
///
/// Token-wise, not substring: `callout-note` is a different class and a site
/// using only it has not asked for the callout partial.
fn has_callout_class(class_list: &str) -> bool {
    class_list.split_whitespace().any(|c| c == "callout")
}

/// The classes a typed shortcode puts on its wrapper, for the variants that
/// accept `{.foo}` class args.
fn shortcode_classes(sc: &Shortcode) -> Option<&str> {
    match sc {
        Shortcode::Buttons(b) => Some(b.classes.as_str()),
        Shortcode::Gallery(g) => Some(g.classes.as_str()),
        Shortcode::Grid(g) => Some(g.classes.as_str()),
        Shortcode::Hero(h) => Some(h.classes.as_str()),
        _ => None,
    }
}

/// True if a raw-HTML block opens an element carrying the `callout` class.
///
/// This is the pure-CSS region form — `:::{.callout}` — which
/// `shortcode_extract` lowers to a literal `<div class="callout">` in a
/// `Block::Other`, never to `Block::Callout`. It is documented authoring
/// syntax (`docs/authoring/customization.md`, styling rung 3), so a site whose
/// only callouts are written that way must still get the partial.
///
/// A crude `contains` on the whole block would fire on prose that merely
/// mentions the word; matching inside a `class="…"` attribute value keeps the
/// over-approximation to markup the author actually wrote.
///
/// moss's own lowering always produces lowercase, double-quoted `class="…"`,
/// but a `Block::Other` can also be HTML the author typed into the markdown by
/// hand. So this tolerates `CLASS`, single quotes, unquoted values, and spaces
/// around the `=`. Erring toward matching is the cheap direction: a false
/// positive costs 4 kB of CSS, a false negative renders an unstyled grey box.
fn html_opens_a_callout(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    // `split` rather than `match_indices` + slice: `clippy::string_slice` is
    // denied in this crate, and the iterator hands back the tail directly.
    lowered.split("class").skip(1).any(|after| {
        // Tolerate a space before the `=`.
        let Some(value) = after.trim_start().strip_prefix('=') else {
            return false; // `classname="…"`, or the bare word in prose.
        };
        let value = value.trim_start();
        let mut chars = value.chars();
        match chars.next() {
            Some(q @ ('"' | '\'')) => chars
                .as_str()
                .split_once(q)
                .is_some_and(|(list, _)| has_callout_class(list)),
            // Unquoted value: one token, ending at whitespace or the tag's
            // close — including the `/` of a self-closing tag.
            _ => value.split([' ', '\t', '\n', '\r', '>', '/']).next() == Some("callout"),
        }
    })
}

/// True if raw markdown contains Obsidian callout syntax (`> [!type]`).
///
/// Deliberately loose in the over-shipping direction: a false positive costs
/// 4 KB of CSS, a false negative renders an unstyled grey box.
fn markdown_has_callout(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('>') && line.trim_start_matches(['>', ' ']).starts_with("[!")
    })
}

#[cfg(test)]
mod tests {
    use super::super::node::{CalloutKind, Inline};
    use super::super::url::{Url, UrlKind};
    use super::*;

    fn paragraph_with_link(url: &str) -> Block {
        Block::Paragraph(vec![Inline::Link {
            url: Url::unresolved(url),
            title: None,
            children: vec![Inline::Text("t".into())],
            is_wikilink: false,
            has_pothole: false,
        }])
    }

    /// `visit_blocks` promises "every block (top-level + nested)". A footnote
    /// definition owns a `Vec<Block>`, so a walk that stops at the definition
    /// node answers a "does any block …?" query with a No that only means "not
    /// anywhere I looked" — the failure mode is a silent wrong answer, not an
    /// error. The blockquote control is what proves the miss is specific to
    /// this container rather than to nesting in general.
    #[test]
    fn visit_blocks_descends_into_a_footnote_definition_body() {
        let inner = Block::Paragraph(vec![Inline::Text("inside the note".into())]);
        let doc = Document::from_blocks(vec![Block::FootnoteDefinition {
            label: "a".into(),
            children: vec![inner.clone()],
        }]);

        let mut seen = 0usize;
        visit_blocks(&doc, |b| {
            if matches!(b, Block::Paragraph(_)) {
                seen += 1;
            }
            true
        });
        assert_eq!(
            seen, 1,
            "the note's body block was never visited — the catch-all swallowed it"
        );

        let control = Document::from_blocks(vec![Block::BlockQuote(vec![inner])]);
        let mut seen_control = 0usize;
        visit_blocks(&control, |b| {
            if matches!(b, Block::Paragraph(_)) {
                seen_control += 1;
            }
            true
        });
        assert_eq!(seen, seen_control, "identical content, different container");
    }

    /// A definition list owns its terms' definitions, and a definition owns
    /// its blocks; both are walked like any other container.
    #[test]
    fn visit_blocks_descends_into_a_definition_list() {
        let doc = Document::from_blocks(vec![Block::DefinitionList(vec![Block::DefinitionDefinition(vec![
            Block::ThematicBreak,
        ])])]);
        let mut seen = 0usize;
        visit_blocks(&doc, |b| {
            if matches!(b, Block::ThematicBreak) {
                seen += 1;
            }
            true
        });
        assert_eq!(seen, 1, "the definition's block was never visited");
    }

    #[test]
    fn visits_url_in_paragraph_link() {
        let mut doc = Document::from_blocks(vec![paragraph_with_link("docs/")]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["docs/".to_string()]);
    }

    #[test]
    fn visits_url_in_image_src() {
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Image {
            src: Url::unresolved("img.png"),
            alt: "x".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        }])]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["img.png".to_string()]);
    }

    #[test]
    fn callback_can_mutate_url_to_resolved() {
        // Critical contract: a single visit transitions Unresolved → Resolved.
        let mut doc = Document::from_blocks(vec![paragraph_with_link("docs/")]);
        visit_urls_mut(&mut doc, |u| {
            *u = Url::resolved("../docs/", UrlKind::Wikilink);
        });
        match &doc.blocks[0] {
            Block::Paragraph(children) => match &children[0] {
                Inline::Link { url, .. } => {
                    assert!(url.is_resolved());
                    let Url::Resolved(r) = url else {
                        panic!("expected Resolved, got {url:?}")
                    };
                    assert_eq!(r.href, "../docs/");
                }
                _ => panic!("expected Link"),
            },
            _ => panic!("expected Paragraph"),
        }
    }

    #[test]
    fn visits_url_inside_heading() {
        let mut doc = Document::from_blocks(vec![Block::Heading {
            level: 2,
            children: vec![Inline::Link {
                url: Url::unresolved("x"),
                title: None,
                children: vec![Inline::Text("t".into())],
                is_wikilink: false,
                has_pothole: false,
            }],
            id: None,
        }]);
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn visits_url_inside_emphasis_and_strong() {
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Strong(vec![
            Inline::Emphasis(vec![Inline::Link {
                url: Url::unresolved("nested"),
                title: None,
                children: vec![],
                is_wikilink: false,
                has_pothole: false,
            }]),
        ])])]);
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn visits_url_inside_link_children() {
        // Nested links can't appear in CommonMark, but link children can
        // contain images (e.g. `[![alt](img)](href)`). Both URLs visited.
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Link {
            url: Url::unresolved("outer"),
            title: None,
            children: vec![Inline::Image {
                src: Url::unresolved("inner.png"),
                alt: "".into(),
                title: None,
                is_wikilink: false,
                wikilink_pothole: None,
            }],
            is_wikilink: false,
            has_pothole: false,
        }])]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["outer".to_string(), "inner.png".to_string()]);
    }

    #[test]
    fn visits_urls_inside_list_items() {
        let mut doc = Document::from_blocks(vec![Block::List {
            ordered: false,
            start: None,
            items: vec![
                vec![paragraph_with_link("a")],
                vec![paragraph_with_link("b")],
            ],
            item_source_lines: vec![],
        }]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn visits_urls_inside_blockquote() {
        let mut doc =
            Document::from_blocks(vec![Block::BlockQuote(vec![paragraph_with_link("q")])]);
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn visits_urls_inside_table_header_and_rows() {
        let mut doc = Document::from_blocks(vec![Block::Table {
            header: vec![vec![Inline::Link {
                url: Url::unresolved("h"),
                title: None,
                children: vec![],
                is_wikilink: false,
                has_pothole: false,
            }]],
            rows: vec![vec![vec![Inline::Link {
                url: Url::unresolved("r"),
                title: None,
                children: vec![],
                is_wikilink: false,
                has_pothole: false,
            }]]],
            alignments: Vec::new(),
            header_source_line: None,
            row_source_lines: vec![],
        }]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["h".to_string(), "r".to_string()]);
    }

    /// The gate for the `callouts` stylesheet partial. A false negative here
    /// ships a `class="callout"` element with no rules — an unstyled grey box.
    #[test]
    fn detects_a_callout_anywhere_it_can_appear() {
        let callout = || Block::Callout {
            unknown_kind: None,
            kind: super::super::node::CalloutKind::Note,
            fold: None,
            title: None,
            children: vec![Block::Paragraph(vec![Inline::Text("x".into())])],
        };
        // Top level.
        assert!(has_callout_recursive(&Document::from_blocks(vec![callout()])));
        // Nested inside a list item — `visit_blocks` has to descend.
        assert!(has_callout_recursive(&Document::from_blocks(vec![Block::List {
            ordered: false,
            start: None,
            items: vec![vec![callout()]],
            item_source_lines: Vec::new(),
        }])));
        // A document with no callout must answer false, or the gate is a
        // constant and the partial always ships.
        assert!(!has_callout_recursive(&Document::from_blocks(vec![Block::Paragraph(vec![
            Inline::Text("no callout here".into())
        ])])));
    }

    /// `Recent.fallback_markdown` is a raw `String`, parsed at HTML-emit time,
    /// so `visit_blocks` cannot descend into it. A site whose ONLY callout is
    /// in a `:::recent` fallback still emits `class="callout"`, so the gate
    /// has to re-parse the text.
    #[test]
    fn detects_a_callout_in_a_recent_shortcode_fallback() {
        use super::super::shortcode::RecentShortcode;
        let with = Document::from_blocks(vec![Block::Shortcode(Shortcode::Recent(
            RecentShortcode {
                fallback_markdown: "> [!warning] Heads up\n> Nothing published yet.".into(),
                ..Default::default()
            },
        ))]);
        assert!(has_callout_recursive(&with), "a fallback callout must gate the partial on");

        let without = Document::from_blocks(vec![Block::Shortcode(Shortcode::Recent(
            RecentShortcode {
                fallback_markdown: "> Just a quote, no callout.".into(),
                ..Default::default()
            },
        ))]);
        assert!(!has_callout_recursive(&without), "a plain blockquote is not a callout");
    }

    /// `:::{.callout}` — the pure-CSS region — never becomes `Block::Callout`.
    /// `shortcode_extract` lowers it to a literal `<div class="callout">` in a
    /// `Block::Other`, so a gate that only matched the typed variant shipped
    /// no rules for a site that styles exclusively this way. It is documented
    /// authoring syntax (customization.md, styling rung 3), not hand-rolled
    /// markup, which is what separates it from the runtime-injection cases the
    /// gate deliberately ignores.
    #[test]
    fn detects_a_callout_written_as_a_css_region() {
        let other = |html: &str| Document::from_blocks(vec![Block::Other(html.into())]);
        assert!(has_callout_recursive(&other("<div class=\"callout\">\n")));
        assert!(has_callout_recursive(&other("<div class=\"lead callout wide\">\n")));
        assert!(has_callout_recursive(&other("<div id=\"x\" class='callout'>\n")));
        assert!(has_callout_recursive(&other("<div class=callout>\n")));

        // A `Block::Other` can also be HTML the author typed by hand, which is
        // not held to moss's lowering conventions.
        assert!(has_callout_recursive(&other("<div CLASS=\"callout\">\n")));
        assert!(has_callout_recursive(&other("<div class = \"callout\">\n")));
        assert!(has_callout_recursive(&other("<span class=callout/>")));
        // Second element in the same block still counts.
        assert!(has_callout_recursive(&other("<p class=\"lead\">hi</p><div class=\"callout\">")));

        // Token-wise, not substring: a different class, and prose that merely
        // says the word, must both leave the partial off.
        assert!(!has_callout_recursive(&other("<div class=\"callout-ish\">\n")));
        assert!(!has_callout_recursive(&other("<p>I love a good callout.</p>")));
        assert!(!has_callout_recursive(&other("<div class=\"grid\">\n")));
        assert!(!has_callout_recursive(&other("<div classname=\"callout\">\n")));
    }

    /// A typed shortcode can carry `{.callout}` too (`:::grid 2 {.callout}`);
    /// those classes live in the struct, not in any `Block::Other`.
    #[test]
    fn detects_a_callout_class_on_a_typed_shortcode() {
        use super::super::shortcode::GridShortcode;
        let grid = |classes: &str| {
            Document::from_blocks(vec![Block::Shortcode(Shortcode::Grid(GridShortcode {
                classes: classes.into(),
                ..Default::default()
            }))])
        };
        assert!(has_callout_recursive(&grid("callout")));
        assert!(has_callout_recursive(&grid("wide callout")));
        assert!(!has_callout_recursive(&grid("wide")));
        assert!(!has_callout_recursive(&grid("")));
    }

    #[test]
    fn visits_urls_inside_callout() {
        let mut doc = Document::from_blocks(vec![Block::Callout {
            unknown_kind: None,
            kind: super::super::node::CalloutKind::Note,
            fold: None,
            title: None,
            children: vec![paragraph_with_link("inside")],
        }]);
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn does_not_visit_text_or_code() {
        // Text/Code/LineBreak are leaves with no URL field; the visitor
        // must not synthesize visits.
        let mut doc = Document::from_blocks(vec![
            Block::Paragraph(vec![Inline::Text("plain".into()), Inline::Code("c".into())]),
            Block::CodeBlock {
                lang: None,
                value: "x".into(),
            },
            Block::ThematicBreak,
            Block::Other("<raw>".into()),
        ]);
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn empty_document_visits_nothing() {
        let mut doc = Document::new();
        let mut count = 0;
        visit_urls_mut(&mut doc, |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn visit_blocks_walks_top_level() {
        let doc = Document::from_blocks(vec![Block::ThematicBreak, Block::Paragraph(vec![])]);
        let mut count = 0;
        visit_blocks(&doc, |_| {
            count += 1;
            true
        });
        assert_eq!(count, 2);
    }

    #[test]
    fn visit_blocks_descends_into_blockquote() {
        let doc = Document::from_blocks(vec![Block::BlockQuote(vec![Block::ThematicBreak])]);
        let mut count = 0;
        visit_blocks(&doc, |_| {
            count += 1;
            true
        });
        assert_eq!(count, 2); // BlockQuote + nested ThematicBreak
    }

    #[test]
    fn visit_blocks_descends_into_list_items() {
        let doc = Document::from_blocks(vec![Block::List {
            ordered: false,
            start: None,
            items: vec![vec![Block::ThematicBreak], vec![Block::ThematicBreak]],
            item_source_lines: vec![],
        }]);
        let mut count = 0;
        visit_blocks(&doc, |_| {
            count += 1;
            true
        });
        assert_eq!(count, 3); // List + 2 ThematicBreaks
    }

    #[test]
    fn visit_blocks_short_circuits_when_callback_returns_false() {
        let doc = Document::from_blocks(vec![
            Block::ThematicBreak,
            Block::ThematicBreak,
            Block::ThematicBreak,
        ]);
        let mut count = 0;
        let result = visit_blocks(&doc, |_| {
            count += 1;
            count < 2 // stop after 2 visits
        });
        assert!(!result);
        assert_eq!(count, 2);
    }

    // -----------------------------------------------------------------
    // Phase 4 PR3 (2026-05-27): Block::Figure URL descent
    // -----------------------------------------------------------------

    #[test]
    fn visits_url_inside_figure_image() {
        let mut doc = Document::from_blocks(vec![Block::Figure {
            image: Inline::Image {
                src: Url::unresolved("fig.png"),
                alt: "f".into(),
                title: None,
                is_wikilink: false,
                wikilink_pothole: None,
            },
            caption: Some(vec![Inline::Text("f".into())]),
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
            italic_caption: false,
        }]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["fig.png".to_string()]);
    }

    #[test]
    fn figure_url_becomes_resolved_after_visit() {
        // Critical contract: a single visit transitions the figure's
        // image URL from Unresolved to Resolved (matching the
        // visit_urls_mut bypass-prevention invariant).
        let mut doc = Document::from_blocks(vec![Block::Figure {
            image: Inline::Image {
                src: Url::unresolved("p.jpg"),
                alt: "".into(),
                title: None,
                is_wikilink: false,
                wikilink_pothole: None,
            },
            caption: None,
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
            italic_caption: false,
        }]);
        visit_urls_mut(&mut doc, |u| {
            *u = Url::resolved("p.jpg", UrlKind::Asset);
        });
        match &doc.blocks[0] {
            Block::Figure { image, .. } => match image {
                Inline::Image { src, .. } => assert!(src.is_resolved()),
                _ => panic!("expected Image inside Figure"),
            },
            _ => panic!("expected Figure"),
        }
    }

    #[test]
    fn visits_url_inside_figure_caption_inlines() {
        // Defensive: caption is Vec<Inline>; if it carries a Link (rare —
        // captions default to alt-text Inline::Text), the URL must still
        // be visited.
        let mut doc = Document::from_blocks(vec![Block::Figure {
            image: Inline::Image {
                src: Url::unresolved("fig.png"),
                alt: "".into(),
                title: None,
                is_wikilink: false,
                wikilink_pothole: None,
            },
            caption: Some(vec![Inline::Link {
                url: Url::unresolved("credit"),
                title: None,
                children: vec![Inline::Text("credit".into())],
                is_wikilink: false,
                has_pothole: false,
            }]),
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
            italic_caption: false,
        }]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["fig.png".to_string(), "credit".to_string()]);
    }

    #[test]
    fn raw_html_walker_reaches_nodes_nested_in_lists_callouts_captions_and_links() {
        let mut blocks = vec![
            Block::List {
                ordered: false,
                start: None,
                items: vec![vec![Block::Other("<li-raw>".into())]],
                item_source_lines: vec![],
            },
            Block::Callout {
                unknown_kind: None,
                kind: CalloutKind::Note,
                fold: None,
                title: None,
                children: vec![Block::Other("<callout-raw>".into())],
            },
            Block::Figure {
                image: Inline::Image {
                    src: Url::unresolved("fig.png"),
                    alt: "".into(),
                    title: None,
                    is_wikilink: false,
                    wikilink_pothole: None,
                },
                caption: Some(vec![Inline::Other("<caption-raw>".into())]),
                width: None,
                align: None,
                class_names: Vec::new(),
                img_style: None,
                italic_caption: false,
            },
            Block::Paragraph(vec![
                Inline::Link {
                    url: Url::unresolved("x"),
                    title: None,
                    children: vec![Inline::Other("<link-raw>".into())],
                    is_wikilink: false,
                    has_pothole: false,
                },
                Inline::Emphasis(vec![Inline::Other("<em-raw>".into())]),
            ]),
        ];
        let mut seen: Vec<String> = Vec::new();
        for block in &mut blocks {
            visit_block_raw_html_mut(block, |html| seen.push(html.clone()));
        }
        assert_eq!(
            seen,
            vec![
                "<li-raw>".to_string(),
                "<callout-raw>".to_string(),
                "<caption-raw>".to_string(),
                "<link-raw>".to_string(),
                "<em-raw>".to_string(),
            ]
        );
    }

    #[test]
    fn has_shortcode_recursive_returns_false_on_empty_doc() {
        // Phase A: Shortcode enum is empty. Recursive query returns false
        // for any kind. Per-kind positive-case tests land alongside
        // each Phase B migration (when a Shortcode variant exists).
        let doc = Document::new();
        assert!(!has_shortcode_recursive(&doc, ShortcodeKind::Subscribe));
        assert!(!has_shortcode_recursive(&doc, ShortcodeKind::Buttons));
    }
}
