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

/// Visit every URL in the document with a callback that may mutate it
/// in place. Walks links, image srcs, and all nested block/inline content.
///
/// Used by the resolve-classification pass: the upstream resolve pipeline
/// has already rewritten markdown sources so URLs come out as one of the
/// shapes documented in [`crate::ast::url::Url`]. The callback inspects the
/// raw string and replaces it with a [`crate::ast::url::Url::Resolved`].
pub fn visit_urls_mut<F>(doc: &mut Document, mut callback: F)
where
    F: FnMut(&mut Url),
{
    for block in &mut doc.blocks {
        visit_urls_in_block(block, &mut callback);
    }
}

fn visit_urls_in_block<F>(block: &mut Block, callback: &mut F)
where
    F: FnMut(&mut Url),
{
    match block {
        Block::Heading { children, .. } => {
            for inline in children {
                visit_urls_in_inline(inline, callback);
            }
        }
        Block::Paragraph(children) => {
            for inline in children {
                visit_urls_in_inline(inline, callback);
            }
        }
        Block::Callout { children, .. } | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                visit_urls_in_block(nested, callback);
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    visit_urls_in_block(nested, callback);
                }
            }
        }
        Block::Table { header, rows, .. } => {
            for cell in header {
                for inline in cell {
                    visit_urls_in_inline(inline, callback);
                }
            }
            for row in rows {
                for cell in row {
                    for inline in cell {
                        visit_urls_in_inline(inline, callback);
                    }
                }
            }
        }
        Block::BlockQuote(children) => {
            for nested in children {
                visit_urls_in_block(nested, callback);
            }
        }
        Block::Shortcode(sc) => {
            visit_urls_in_shortcode(sc, callback);
        }
        Block::Figure { image, caption, .. } => {
            // Descend into the image's src (the load-bearing URL); the
            // caption is a Vec<Inline> that may itself carry links —
            // unlikely in practice (captions default to alt text) but the
            // visitor must not silently skip them.
            visit_urls_in_inline(image, callback);
            if let Some(cap_inlines) = caption {
                for inline in cap_inlines {
                    visit_urls_in_inline(inline, callback);
                }
            }
        }
        Block::LinkCard { url, children } => {
            // Phase 4 PR4.5: the wrapping URL (compound-link href) +
            // every URL inside the inner block content.
            callback(url);
            for nested in children {
                visit_urls_in_block(nested, callback);
            }
        }
        Block::CodeBlock { .. } | Block::ThematicBreak | Block::Other(_) => {
            // No URLs in these.
        }
    }
}

fn visit_urls_in_shortcode<F>(sc: &mut super::shortcode::Shortcode, callback: &mut F)
where
    F: FnMut(&mut Url),
{
    use super::shortcode::Shortcode;
    match sc {
        Shortcode::Subscribe(_) => {} // No URLs.
        Shortcode::Buttons(args) => {
            for item in &mut args.items {
                callback(&mut item.url);
            }
        }
        Shortcode::Gallery(args) => {
            for item in &mut args.items {
                callback(&mut item.src);
            }
        }
        Shortcode::Hero(args) => {
            if let Some(image) = args.image.as_mut() {
                callback(image);
            }
            for image in &mut args.extra_images {
                callback(image);
            }
            // Phase 4 PR4.5 (2026-05-28): descend into the typed overlay
            // blocks so URLs inside `:::hero` overlay markdown (e.g. a
            // `[Read more](/x)` link in the overlay copy) get classified
            // by the same visitor pass.
            for block in &mut args.overlay {
                visit_urls_in_block(block, callback);
            }
        }
        Shortcode::Grid(args) => {
            // Phase 4 PR4.5 (2026-05-28): cells are now typed Vec<Block>;
            // descend into each cell. Compound-link cells render through
            // `Block::LinkCard { url, children }`, whose own visit arm
            // walks both the wrapping href and the inner children.
            for cell_blocks in &mut args.cells {
                for block in cell_blocks {
                    visit_urls_in_block(block, callback);
                }
            }
        }
        Shortcode::Recent(_) => {} // No URLs.
        Shortcode::Apply(_) => {}  // No URLs.
    }
}

fn visit_urls_in_inline<F>(inline: &mut Inline, callback: &mut F)
where
    F: FnMut(&mut Url),
{
    match inline {
        Inline::Link { url, children, .. } => {
            callback(url);
            for nested in children {
                visit_urls_in_inline(nested, callback);
            }
        }
        Inline::Image { src, .. } => {
            callback(src);
        }
        Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
            for nested in children {
                visit_urls_in_inline(nested, callback);
            }
        }
        Inline::Text(_)
        | Inline::Code(_)
        | Inline::LineBreak
        | Inline::FootnoteRef(_)
        | Inline::TaskMarker(_)
        | Inline::Other(_) => {}
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
    if !callback(block) {
        return false;
    }
    match block {
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        // A footnote definition is a block container like any other. It is
        // listed here rather than left to the catch-all because the catch-all
        // is for blocks with no block children at all, and a walk that skipped
        // a note's body would answer "does any block …?" with a No that only
        // means "not anywhere I looked".
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                if !visit_block(nested, callback) {
                    return false;
                }
            }
        }
        Block::List { items, .. } => {
            for item_blocks in items {
                for nested in item_blocks {
                    if !visit_block(nested, callback) {
                        return false;
                    }
                }
            }
        }
        Block::LinkCard { children, .. } => {
            // Phase 4 PR4.5: descend into the compound-link cell's inner
            // block content (image + heading + paragraphs).
            for nested in children {
                if !visit_block(nested, callback) {
                    return false;
                }
            }
        }
        Block::Shortcode(super::shortcode::Shortcode::Grid(args)) => {
            // Phase 4 PR4.5: cells are typed Vec<Block>; descend so
            // `has_shortcode_recursive(_, Subscribe)` etc. find shortcodes
            // nested inside grid cells.
            for cell_blocks in &args.cells {
                for nested in cell_blocks {
                    if !visit_block(nested, callback) {
                        return false;
                    }
                }
            }
        }
        Block::Shortcode(super::shortcode::Shortcode::Hero(args)) => {
            // Phase 4 PR4.5: overlay is typed Vec<Block>; descend so
            // `has_shortcode_recursive(_, Subscribe)` etc. find shortcodes
            // nested inside `:::hero` overlays.
            for nested in &args.overlay {
                if !visit_block(nested, callback) {
                    return false;
                }
            }
        }
        // Headings, paragraphs, code blocks, tables, other shortcode
        // variants, thematic breaks, figures, raw HTML — terminal at the
        // block level. Inline children of headings/paragraphs are visited
        // by inline visitors, not block visitors.
        //
        // Every variant that OWNS a `Vec<Block>` must be named above, not
        // left to fall through here: this arm reads as "no block children",
        // and a container that lands in it is skipped in silence.
        _ => {}
    }
    true
}

/// True if any block in the document is a shortcode of the given kind
/// (recursive — descends into callouts, blockquotes, list items).
///
/// Replaces the `project_has_inline_subscribe` filesystem scan once
/// shortcodes migrate to typed AST in Phase B.
pub fn has_shortcode_recursive(doc: &Document, kind: ShortcodeKind) -> bool {
    let mut found = false;
    visit_blocks(doc, |block| {
        if let Block::Shortcode(sc) = block {
            if sc.kind() == kind {
                found = true;
                return false; // short-circuit
            }
        }
        true
    });
    found
}

/// True if any block in the document is a callout (recursive — a callout
/// nested inside a list item or another callout counts).
///
/// Gates the `callouts` site stylesheet partial: a build whose every page
/// answers `false` here never ships `assets/css/site/callouts.css`. The
/// query is a lowering of the typed tree, never a scan of emitted HTML —
/// see NORTH-STAR "parse once, lower to many".
///
/// # The four shapes it matches
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
    use super::super::node::Inline;
    use super::super::url::{Url, UrlKind};
    use super::*;

    fn paragraph_with_link(url: &str) -> Block {
        Block::Paragraph(vec![Inline::Link {
            url: Url::unresolved(url),
            title: None,
            children: vec![Inline::Text("t".into())],
            is_wikilink: false,
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
            }]],
            rows: vec![vec![vec![Inline::Link {
                url: Url::unresolved("r"),
                title: None,
                children: vec![],
                is_wikilink: false,
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
            }]),
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
        }]);
        let mut seen: Vec<String> = Vec::new();
        visit_urls_mut(&mut doc, |u| match u {
            Url::Unresolved(s) => seen.push(s.clone()),
            _ => {}
        });
        assert_eq!(seen, vec!["fig.png".to_string(), "credit".to_string()]);
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
