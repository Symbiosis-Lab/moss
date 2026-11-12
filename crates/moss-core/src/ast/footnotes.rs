//! Footnotes: numbering, and the endnote section.
//!
//! One module owns the whole feature because its two halves must agree by
//! construction. The parser gives us `Inline::FootnoteRef(label)` and
//! `Block::FootnoteDefinition { label, .. }`; everything a reader sees —
//! the printed number, the marker id, the endnote `<li>`, the back-link —
//! is derived HERE, at render time, from the document as a whole. Split
//! across two modules, the marker id and the back-link href drift, and a
//! dangling `#fnref-3` is invisible in a diff.
//!
//! Numbering is FIRST-REFERENCE order, not source order. Scope is the whole
//! block tree except shortcode bodies, which render through
//! [`super::render::render_blocks`] as their own little documents.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::hooks::{escape_text, RenderHooks};
use super::node::{Block, Inline};
use super::render::{render_blocks_with, render_inlines};

/// The document's footnotes, numbered.
///
/// Numbering is **first-reference order** — the order a reader meets the
/// markers, not the order the author wrote the definitions. It is derived
/// state, deliberately not stored on the AST (same rule as
/// [`super::node::ColumnAlignment`]'s numeric auto-alignment): the same
/// `Block::FootnoteDefinition` renders differently depending on which
/// document it is part of, so the number cannot belong to the node.
///
/// Scope: the whole block tree EXCEPT shortcode bodies. A `:::grid` cell or
/// `:::hero` overlay is parsed and rendered as its own little document
/// through [`super::render::render_blocks`], which carries no index.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FootnoteIndex {
    numbers: std::collections::HashMap<String, usize>,
    order: Vec<(usize, String)>,
}

impl FootnoteIndex {
    /// Build the index for a block tree. Empty when the tree defines no
    /// footnotes, which is the overwhelmingly common case.
    pub fn build(blocks: &[Block]) -> Self {
        Self::from_definitions(blocks, &Definitions::collect(blocks))
    }

    /// [`Self::build`] from a walk the caller already made.
    pub(super) fn from_definitions(blocks: &[Block], definitions: &Definitions<'_>) -> Self {
        if definitions.labels.is_empty() {
            return Self::default();
        }
        let mut order: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        collect_refs(blocks, &mut order, &mut seen, definitions);
        // From here the reader is reading the endnote section top to
        // bottom, and the numbering follows that reading. A HOISTED note's
        // body can itself carry a marker (`[^1]: see [^2]`); those markers
        // are met inside the endnote list, so they extend the order rather
        // than seeding it. When the walked list runs out and
        // never-referenced definitions remain (GFM drops these; moss
        // numbers them after the referenced notes, in source order, so the
        // author's text is never silently deleted), the reader's
        // next stop is the first such note's body — so it joins the walk
        // right there, not after the loop ends. Appending the tail after
        // the loop mis-numbered a marker met only inside an unreferenced
        // note's body BELOW notes the reader had not reached yet. (A
        // REPEAT's body renders in place, so the main walk above already
        // collected its markers at the position the reader meets them.)
        let mut i = 0;
        loop {
            if i == order.len() {
                let Some(next) = definitions.labels.iter().find(|l| !seen.contains(**l)) else {
                    break;
                };
                seen.insert(next.to_string());
                order.push(next.to_string());
            }
            if let Some(children) = definitions.body(&order[i]) {
                collect_refs(children, &mut order, &mut seen, definitions);
            }
            i += 1;
        }

        let mut index = Self::default();
        for label in order {
            if definitions.body(&label).is_none() {
                continue;
            }
            let n = index.order.len() + 1;
            index.numbers.insert(label.clone(), n);
            index.order.push((n, label));
        }
        index
    }

    /// True when the document has no footnotes to render.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The printed number for `label`, or `None` when no definition in this
    /// index owns it.
    pub fn number(&self, label: &str) -> Option<usize> {
        self.numbers.get(label).copied()
    }

    /// `(number, label)` pairs in endnote order.
    pub fn entries(&self) -> &[(usize, String)] {
        &self.order
    }
}

/// The doc-order-first definition of each label, found in one walk.
///
/// Every "which definition owns this label" question (the endnote body, the
/// hoist decision) is answered from here, so they agree by construction and
/// the tree is walked once however many notes it has. Descends into every
/// structural container and stops at shortcode bodies, so a label defined
/// only inside one is absent and never hoisted, matching the renderer.
pub(super) struct Definitions<'a> {
    /// In document order of each label's first definition.
    labels: Vec<&'a str>,
    bodies: HashMap<&'a str, &'a [Block]>,
}

impl<'a> Definitions<'a> {
    pub(super) fn collect(blocks: &'a [Block]) -> Self {
        let mut definitions = Self {
            labels: Vec::new(),
            bodies: HashMap::new(),
        };
        definitions.walk(blocks);
        definitions
    }

    fn walk(&mut self, blocks: &'a [Block]) {
        for block in blocks {
            count_visit();
            match block {
                Block::FootnoteDefinition { label, children } => {
                    if let Entry::Vacant(slot) = self.bodies.entry(label.as_str()) {
                        slot.insert(children);
                        self.labels.push(label.as_str());
                    }
                    self.walk(children);
                }
                Block::BlockQuote(children)
                | Block::Callout { children, .. }
                | Block::LinkCard { children, .. } => self.walk(children),
                Block::DefinitionList(children) | Block::DefinitionDefinition(children) => self.walk(children),
                Block::List { items, .. } => {
                    for item in items {
                        self.walk(item);
                    }
                }
                _ => {}
            }
        }
    }

    /// The body of `label`'s first definition, the one the endnote renders.
    pub(super) fn body(&self, label: &str) -> Option<&'a [Block]> {
        self.bodies.get(label).copied()
    }

    /// Body address per label: the identity [`is_hoisted`] decides by, owned
    /// so a context that outlives the borrow can keep it.
    pub(super) fn addresses(&self) -> HashMap<String, usize> {
        self.bodies
            .iter()
            .map(|(label, body)| (label.to_string(), body.as_ptr() as usize))
            .collect()
    }
}

/// Collect footnote marker labels in reading order, deduplicated. Skips
/// HOISTED definition bodies (the caller walks those in endnote order, where
/// they read) and shortcode bodies (they render through a separate entry
/// point) — but descends into a REPEAT's body, which renders in place: its
/// markers are body markers the reader meets mid-page, and skipping them
/// numbered such a note after everything else while its printed marker sat
/// mid-body, out of first-reference order.
fn collect_refs(
    blocks: &[Block],
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
    definitions: &Definitions<'_>,
) {
    for block in blocks {
        match block {
            Block::Heading { children, .. } | Block::Paragraph(children) => {
                collect_refs_in_inlines(children, out, seen)
            }
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. } => collect_refs(children, out, seen, definitions),
            Block::FootnoteDefinition { label, children } => {
                if definitions.body(label).map(<[Block]>::as_ptr) != Some(children.as_ptr()) {
                    collect_refs(children, out, seen, definitions);
                }
            }
            Block::DefinitionTitle(children) => collect_refs_in_inlines(children, out, seen),
            Block::DefinitionList(children) | Block::DefinitionDefinition(children) => collect_refs(children, out, seen, definitions),
            Block::List { items, .. } => {
                for item in items {
                    collect_refs(item, out, seen, definitions);
                }
            }
            Block::Table { header, rows, .. } => {
                for cell in header.iter().chain(rows.iter().flatten()) {
                    collect_refs_in_inlines(cell, out, seen);
                }
            }
            Block::Figure { caption, .. } => {
                if let Some(caption) = caption {
                    collect_refs_in_inlines(caption, out, seen);
                }
            }
            _ => {}
        }
    }
}

fn collect_refs_in_inlines(inlines: &[Inline], out: &mut Vec<String>, seen: &mut HashSet<String>) {
    for inline in inlines {
        match inline {
            Inline::FootnoteRef(label) => {
                if seen.insert(label.clone()) {
                    out.push(label.clone());
                }
            }
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children)
            | Inline::Mark(children)
            | Inline::Link { children, .. } => collect_refs_in_inlines(children, out, seen),
            _ => {}
        }
    }
}

/// The per-document render context: footnote numbering and whether headings carry permalinks.
///
/// Default (empty index, permalinks left to the hooks) is what every
/// [`super::render::render_blocks`] entry point carries.
#[derive(Default)]
pub struct FootnoteCtx {
    index: FootnoteIndex,
    /// The page fails [`crate::heading::anchor_rule::wants_heading_anchors`].
    permalinks_off: bool,
    /// Markers emitted per label so far. Drives the `fnref-N-K` id suffix
    /// and, once the whole document is rendered, the number of back-links.
    emitted: HashMap<String, usize>,
    /// Body address of the doc-order-FIRST definition per label — the one
    /// the endnote section hoists, recorded by IDENTITY, not by a walk
    /// counter. A "sightings so far" counter depends on walk order, and this
    /// document is walked twice: the body pass skips a hoisted definition
    /// WITHOUT descending, so a definition nested inside it was never
    /// counted, and a later repeat of that label passed for the first — its
    /// text vanished from every surface. The endnote pass then re-walked the
    /// hoisted bodies and counted the same definitions again.
    hoisted: HashMap<String, usize>,
}

impl FootnoteCtx {
    /// The context `render_document` uses: numbering for the whole tree.
    pub fn for_document(blocks: &[Block]) -> Self {
        // One walk answers both "the definition the section hoists" and
        // "the definition the body pass skips".
        let definitions = Definitions::collect(blocks);
        Self {
            index: FootnoteIndex::from_definitions(blocks, &definitions),
            emitted: HashMap::new(),
            hoisted: definitions.addresses(),
            permalinks_off: !crate::heading::anchor_rule::wants_heading_anchors(blocks),
        }
    }

    /// The document is too short for heading permalinks; its headings keep their ids.
    pub(super) fn permalinks_off(&self) -> bool {
        self.permalinks_off
    }
}

/// The id of the K-th marker for footnote N. The one place this shape is
/// spelled, so the endnote's `href="#fnref-…"` can never drift from the
/// body's `id="fnref-…"`.
fn marker_id(n: usize, k: usize) -> String {
    if k == 1 {
        format!("fnref-{n}")
    } else {
        format!("fnref-{n}-{k}")
    }
}

/// Emit an in-body `[^label]` marker.
///
/// A label the index doesn't own gets its literal source back instead of a
/// superscript pointing at an id nobody will emit — the honest fallback on
/// the shortcode-body render path.
///
/// `hooks.emit_footnote_anchors()` gates the `id=`/`href=` pair (default
/// `true`, byte-identical to before this hook existed): a consumer that
/// opts out still gets the visible superscript number, just no anchor to
/// hang a fragment link off of. See `RenderHooks::emit_footnote_anchors`.
pub(super) fn render_marker<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    label: &str,
    ctx: &mut FootnoteCtx,
) {
    let Some(n) = ctx.index.number(label) else {
        out.push_str("[^");
        out.push_str(&escape_text(label));
        out.push(']');
        return;
    };
    let k = ctx.emitted.entry(label.to_string()).or_insert(0);
    *k += 1;
    if hooks.emit_footnote_anchors() {
        let id = marker_id(n, *k);
        let _ = write!(
            out,
            r##"<sup class="moss-footnote-ref" id="{id}" tabindex="-1"><a href="#fn-{n}" role="doc-noteref">{n}</a></sup>"##
        );
    } else {
        let _ = write!(out, r##"<sup class="moss-footnote-ref">{n}</sup>"##);
    }
}

/// Whether this `Block::FootnoteDefinition` is the one the endnote section
/// hoists (so the body render emits nothing for it).
///
/// Only the FIRST definition of a label in document order is hoisted. A
/// repeated label is an authoring error; the repeat renders in place —
/// wrong, but visible, which beats deleting the author's text.
///
/// Decided by identity — is `children` the body [`FootnoteCtx::hoisted`]
/// recorded? — so the answer does not depend on which pass is asking or in
/// what order the tree is walked. (Two same-label definitions with EMPTY
/// bodies alias the dangling `Vec` pointer and both answer "hoisted"; both
/// emit nothing either way, so no text is lost.) A `render_blocks` entry
/// point carries an empty map and every definition renders in place,
/// keeping the literal-source fallback for un-indexed labels.
pub(super) fn is_hoisted(label: &str, children: &[Block], ctx: &FootnoteCtx) -> bool {
    is_hoisted_in(label, children, &ctx.hoisted)
}

/// [`is_hoisted`]'s identity check, taking [`Definitions::addresses`]
/// directly rather than a full [`FootnoteCtx`] — for walks (e.g.
/// `ast::plain_text::render_plain_text`) that never build a `FootnoteCtx`.
pub(super) fn is_hoisted_in(
    label: &str,
    children: &[Block],
    hoisted: &HashMap<String, usize>,
) -> bool {
    hoisted.get(label) == Some(&(children.as_ptr() as usize))
}

/// Emit the endnote section: one `<li>` per footnote in index order, each
/// ending in a back-link per marker that pointed at it.
///
/// Two passes, and the order is load-bearing. A note body can itself carry a
/// marker (`[^1]: see [^2]`), so every body must render before ANY back-link
/// list is written — otherwise a marker emitted late would have no matching
/// back-link and the `fnref` ids would dangle.
pub fn render_section<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    blocks: &[Block],
    ctx: &mut FootnoteCtx,
) {
    if ctx.index.is_empty() {
        return;
    }
    // (number, label, blocks before the trailing paragraph, that paragraph's
    // inline HTML). The back-link rides inside the trailing paragraph so it
    // sits at the end of the note's text, not on a line of its own.
    let mut notes: Vec<(usize, String, String, Option<String>)> = Vec::new();
    let definitions = Definitions::collect(blocks);
    for (n, label) in ctx.index.entries().to_vec() {
        let Some(children) = definitions.body(&label) else {
            continue;
        };
        let (mut head, mut tail) = (String::new(), None);
        match children.split_last() {
            Some((Block::Paragraph(inlines), rest)) => {
                render_blocks_with(hooks, &mut head, rest, ctx);
                let mut last = String::new();
                render_inlines(hooks, &mut last, inlines, ctx);
                tail = Some(last);
            }
            _ => render_blocks_with(hooks, &mut head, children, ctx),
        }
        notes.push((n, label, head, tail));
    }
    let emit_anchors = hooks.emit_footnote_anchors();
    out.push_str("<section class=\"moss-footnotes\" role=\"doc-endnotes\">\n<ol>\n");
    for (n, label, head, tail) in notes {
        let backrefs = ctx.emitted.get(&label).copied().unwrap_or(0);
        if emit_anchors {
            let _ = write!(out, "<li id=\"fn-{n}\" tabindex=\"-1\">");
        } else {
            out.push_str("<li>");
        }
        out.push_str(&head);
        if tail.is_some() || backrefs > 0 {
            out.push_str("<p>");
            out.push_str(tail.as_deref().unwrap_or_default());
            if emit_anchors {
                push_backrefs(out, n, backrefs);
            }
            out.push_str("</p>\n");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ol>\n</section>\n");
}

/// One return arrow per marker, so a note referenced twice ends with two.
///
/// The arrow is `&#8617;&#xFE0E;`: U+21A9 followed by VARIATION SELECTOR-15,
/// which forces text presentation. Without it, Chrome on Android/iOS picks the
/// emoji form and the back-link renders as a coloured glyph instead of matching
/// the surrounding body text. Same treatment as the comment reply button
/// (`build/features/comment/render.rs`).
fn push_backrefs(out: &mut String, n: usize, count: usize) {
    for k in 1..=count {
        let id = marker_id(n, k);
        let nth = if k == 1 {
            String::new()
        } else {
            format!(" ({k})")
        };
        let _ = write!(
            out,
            r##" <a class="moss-footnote-backref" href="#{id}" role="doc-backlink" aria-label="Back to reference {n}{nth}">&#8617;&#xFE0E;</a>"##
        );
    }
}

#[cfg(test)]
thread_local! {
    static DEFINITION_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Ticks once per block a definition lookup walks, so tests can pin the cost.
fn count_visit() {
    #[cfg(test)]
    DEFINITION_VISITS.with(|n| n.set(n.get() + 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::hooks::DefaultHooks;
    use crate::ast::parser::parse;

    const REPEATED: &str = "A[^a] B[^b]\n\n[^a]: first a\n\n[^b]: bee\n\n[^a]: second a\n";
    const IN_LIST_AND_QUOTE: &str =
        "- item[^l]\n\n  [^l]: in list\n\n> quote[^q]\n>\n> [^q]: in quote\n";
    const DEFINED_BEFORE_USE: &str = "[^early]: defined first [^inner]\n\n[^inner]: inner note\n\n[^lonely]: never cited\n\nText[^early].\n";
    const NESTED_FIRST: &str =
        "See[^n] and[^o].\n\n> [^n]: quoted first\n\n[^o]: outer\n\n    [^m]: inside o\n\n[^n]: repeat\n\n[^m]: repeat of m\n";
    const NO_FOOTNOTES: &str = "Just text.\n\n## Heading\n";

    /// HTML and plain text of each case above, recorded before definition lookups shared one walk.
    const RENDERED: [(&str, &str); 5] = [
        (
            "<p>A<sup class=\"moss-footnote-ref\" id=\"fnref-1\" tabindex=\"-1\"><a href=\"#fn-1\" role=\"doc-noteref\">1</a></sup> B<sup class=\"moss-footnote-ref\" id=\"fnref-2\" tabindex=\"-1\"><a href=\"#fn-2\" role=\"doc-noteref\">2</a></sup></p>\n<p>second a</p>\n<section class=\"moss-footnotes\" role=\"doc-endnotes\">\n<ol>\n<li id=\"fn-1\" tabindex=\"-1\"><p>first a <a class=\"moss-footnote-backref\" href=\"#fnref-1\" role=\"doc-backlink\" aria-label=\"Back to reference 1\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-2\" tabindex=\"-1\"><p>bee <a class=\"moss-footnote-backref\" href=\"#fnref-2\" role=\"doc-backlink\" aria-label=\"Back to reference 2\">&#8617;&#xFE0E;</a></p>\n</li>\n</ol>\n</section>\n",
            "A[1] B[2]\n\nsecond a\n\n--\n[1] first a\n[2] bee\n\n",
        ),
        (
            "<ul>\n<li>\n<p>item<sup class=\"moss-footnote-ref\" id=\"fnref-1\" tabindex=\"-1\"><a href=\"#fn-1\" role=\"doc-noteref\">1</a></sup></p>\n</li>\n</ul>\n<blockquote>\n<p>quote<sup class=\"moss-footnote-ref\" id=\"fnref-2\" tabindex=\"-1\"><a href=\"#fn-2\" role=\"doc-noteref\">2</a></sup></p>\n</blockquote>\n<section class=\"moss-footnotes\" role=\"doc-endnotes\">\n<ol>\n<li id=\"fn-1\" tabindex=\"-1\"><p>in list <a class=\"moss-footnote-backref\" href=\"#fnref-1\" role=\"doc-backlink\" aria-label=\"Back to reference 1\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-2\" tabindex=\"-1\"><p>in quote <a class=\"moss-footnote-backref\" href=\"#fnref-2\" role=\"doc-backlink\" aria-label=\"Back to reference 2\">&#8617;&#xFE0E;</a></p>\n</li>\n</ol>\n</section>\n",
            "- item[1]\n\n> quote[2]\n\n--\n[1] in list\n[2] in quote\n\n",
        ),
        (
            "<p>Text<sup class=\"moss-footnote-ref\" id=\"fnref-1\" tabindex=\"-1\"><a href=\"#fn-1\" role=\"doc-noteref\">1</a></sup>.</p>\n<section class=\"moss-footnotes\" role=\"doc-endnotes\">\n<ol>\n<li id=\"fn-1\" tabindex=\"-1\"><p>defined first <sup class=\"moss-footnote-ref\" id=\"fnref-2\" tabindex=\"-1\"><a href=\"#fn-2\" role=\"doc-noteref\">2</a></sup> <a class=\"moss-footnote-backref\" href=\"#fnref-1\" role=\"doc-backlink\" aria-label=\"Back to reference 1\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-2\" tabindex=\"-1\"><p>inner note <a class=\"moss-footnote-backref\" href=\"#fnref-2\" role=\"doc-backlink\" aria-label=\"Back to reference 2\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-3\" tabindex=\"-1\"><p>never cited</p>\n</li>\n</ol>\n</section>\n",
            "Text[1].\n\n--\n[1] defined first [2]\n[2] inner note\n[3] never cited\n\n",
        ),
        (
            "<p>See<sup class=\"moss-footnote-ref\" id=\"fnref-1\" tabindex=\"-1\"><a href=\"#fn-1\" role=\"doc-noteref\">1</a></sup> and<sup class=\"moss-footnote-ref\" id=\"fnref-2\" tabindex=\"-1\"><a href=\"#fn-2\" role=\"doc-noteref\">2</a></sup>.</p>\n<p>repeat</p>\n<p>repeat of m</p>\n<section class=\"moss-footnotes\" role=\"doc-endnotes\">\n<ol>\n<li id=\"fn-1\" tabindex=\"-1\"><p>quoted first <a class=\"moss-footnote-backref\" href=\"#fnref-1\" role=\"doc-backlink\" aria-label=\"Back to reference 1\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-2\" tabindex=\"-1\"><p>outer <a class=\"moss-footnote-backref\" href=\"#fnref-2\" role=\"doc-backlink\" aria-label=\"Back to reference 2\">&#8617;&#xFE0E;</a></p>\n</li>\n<li id=\"fn-3\" tabindex=\"-1\"><p>inside o</p>\n</li>\n</ol>\n</section>\n",
            "See[1] and[2].\n\nrepeat\n\nrepeat of m\n\n--\n[1] quoted first\n[2] outer\n[3] inside o\n\n",
        ),
        (
            "<p>Just text.</p>\n<h2 id=\"heading\">Heading</h2>\n",
            "Just text.\n\nHeading\n\n",
        ),
    ];

    /// The per-label search the index used before [`Definitions`]: the first
    /// definition of `label` in document order, at any depth.
    fn first_definition<'a>(blocks: &'a [Block], label: &str) -> Option<&'a [Block]> {
        blocks.iter().find_map(|block| match block {
            Block::FootnoteDefinition { label: l, children } if l == label => Some(children.as_slice()),
            Block::FootnoteDefinition { children, .. }
            | Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. }
            | Block::DefinitionList(children)
            | Block::DefinitionDefinition(children) => first_definition(children, label),
            Block::List { items, .. } => items.iter().find_map(|item| first_definition(item, label)),
            _ => None,
        })
    }

    fn visits_during(f: impl FnOnce()) -> usize {
        DEFINITION_VISITS.with(|n| n.set(0));
        f();
        DEFINITION_VISITS.with(|n| n.get())
    }

    /// Blocks one definition walk reaches: the containers it descends into, not shortcode bodies.
    fn walkable_blocks(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| {
                1 + match block {
                    Block::FootnoteDefinition { children, .. }
                    | Block::BlockQuote(children)
                    | Block::Callout { children, .. }
                    | Block::LinkCard { children, .. }
                    | Block::DefinitionList(children)
                    | Block::DefinitionDefinition(children) => walkable_blocks(children),
                    Block::List { items, .. } => items.iter().map(|item| walkable_blocks(item)).sum(),
                    _ => 0,
                }
            })
            .sum()
    }

    #[test]
    fn numbering_and_hoisting_match_the_first_definition_in_document_order() {
        let cases: [(&str, &[&str], (&str, &str)); 5] = [
            (REPEATED, &["a", "b"], RENDERED[0]),
            (IN_LIST_AND_QUOTE, &["l", "q"], RENDERED[1]),
            (DEFINED_BEFORE_USE, &["early", "inner", "lonely"], RENDERED[2]),
            (NESTED_FIRST, &["n", "o", "m"], RENDERED[3]),
            (NO_FOOTNOTES, &[], RENDERED[4]),
        ];
        for (markdown, labels, expected) in cases {
            let doc = parse(markdown);
            let index = FootnoteIndex::build(&doc.blocks);
            let numbered: Vec<(usize, String)> =
                labels.iter().enumerate().map(|(i, l)| (i + 1, l.to_string())).collect();
            assert_eq!(index.entries(), numbered.as_slice(), "{markdown:?}");
            let ctx = FootnoteCtx::for_document(&doc.blocks);
            assert_eq!(ctx.index, index, "{markdown:?}");
            let first_bodies: HashMap<String, usize> = labels
                .iter()
                .map(|l| {
                    let body = first_definition(&doc.blocks, l).expect("defined");
                    (l.to_string(), body.as_ptr() as usize)
                })
                .collect();
            assert_eq!(ctx.hoisted, first_bodies, "{markdown:?}");
            let rendered = (
                crate::ast::render::render_document(&doc, &DefaultHooks::new()),
                crate::ast::plain_text::render_plain_text(&doc),
            );
            assert_eq!((rendered.0.as_str(), rendered.1.as_str()), expected, "{markdown:?}");
        }
    }

    #[test]
    fn every_entry_point_walks_the_tree_for_definitions_once() {
        let mut markdown = String::new();
        for i in 1..=30 {
            markdown.push_str(&format!("Paragraph {i} cites[^n{i}].\n\n"));
        }
        for i in 1..=10 {
            markdown.push_str(&format!("[^n{i}]: plain note {i}\n\n"));
        }
        for i in 11..=20 {
            markdown.push_str(&format!("- [^n{i}]: listed note {i}\n\n"));
        }
        for i in 21..=30 {
            markdown.push_str(&format!("> [^n{i}]: quoted note {i}\n\n"));
        }
        let doc = parse(&markdown);
        let one_walk = walkable_blocks(&doc.blocks);
        assert_eq!(FootnoteIndex::build(&doc.blocks).entries().len(), 30);

        assert_eq!(visits_during(|| { FootnoteIndex::build(&doc.blocks); }), one_walk);
        assert_eq!(visits_during(|| { FootnoteCtx::for_document(&doc.blocks); }), one_walk);
        assert_eq!(visits_during(|| { crate::ast::plain_text::render_plain_text(&doc); }), one_walk);
        assert_eq!(visits_during(|| { crate::ast::query::block_images(&doc); }), one_walk);
        let hooks = DefaultHooks::new();
        assert_eq!(visits_during(|| { crate::ast::render::render_document(&doc, &hooks); }), 2 * one_walk);
    }
}
