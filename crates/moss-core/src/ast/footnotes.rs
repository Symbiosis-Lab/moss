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
//! [`super::render::render_blocks`] as their own little documents. See
//! ADR-035 for the three call paths and why they differ.

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
/// through [`super::render::render_blocks`], which carries no index — see
/// ADR-035 for why the three call paths differ.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FootnoteIndex {
    numbers: std::collections::HashMap<String, usize>,
    order: Vec<(usize, String)>,
}

impl FootnoteIndex {
    /// Build the index for a block tree. Empty when the tree defines no
    /// footnotes, which is the overwhelmingly common case.
    pub fn build(blocks: &[Block]) -> Self {
        let mut defined: Vec<String> = Vec::new();
        collect_definitions(blocks, &mut defined, &mut HashSet::new());
        if defined.is_empty() {
            return Self::default();
        }
        let defined_set: HashSet<&str> = defined.iter().map(String::as_str).collect();

        let mut order: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let hoisted = first_definition_bodies(blocks, &defined);
        collect_refs(blocks, &mut order, &mut seen, &hoisted);
        // From here the reader is reading the endnote section top to
        // bottom, and the numbering follows that reading. A HOISTED note's
        // body can itself carry a marker (`[^1]: see [^2]`); those markers
        // are met inside the endnote list, so they extend the order rather
        // than seeding it. When the walked list runs out and
        // never-referenced definitions remain (GFM drops these; moss
        // numbers them after the referenced notes, in source order, so the
        // author's text is never silently deleted — ADR-035), the reader's
        // next stop is the first such note's body — so it joins the walk
        // right there, not after the loop ends. Appending the tail after
        // the loop mis-numbered a marker met only inside an unreferenced
        // note's body BELOW notes the reader had not reached yet. (A
        // REPEAT's body renders in place, so the main walk above already
        // collected its markers at the position the reader meets them.)
        let mut i = 0;
        loop {
            if i == order.len() {
                let Some(next) = defined.iter().find(|l| !seen.contains(l.as_str())) else {
                    break;
                };
                seen.insert(next.clone());
                order.push(next.clone());
            }
            if let Some(children) = Self::definition(blocks, &order[i]) {
                collect_refs(children, &mut order, &mut seen, &hoisted);
            }
            i += 1;
        }

        let mut index = Self::default();
        for label in order {
            if !defined_set.contains(label.as_str()) {
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

    /// The body blocks of the FIRST definition of `label` in document order,
    /// at any depth. A repeated label is an authoring error; first wins.
    pub fn definition<'a>(blocks: &'a [Block], label: &str) -> Option<&'a [Block]> {
        for block in blocks {
            let found = match block {
                Block::FootnoteDefinition { label: l, children } if l == label => {
                    return Some(children)
                }
                Block::FootnoteDefinition { children, .. }
                | Block::BlockQuote(children)
                | Block::Callout { children, .. }
                | Block::LinkCard { children, .. } => Self::definition(children, label),
                Block::List { items, .. } => items
                    .iter()
                    .find_map(|item| Self::definition(item, label)),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }
}

/// Collect footnote definition labels in document order, deduplicated.
/// Descends into every structural container; stops at shortcode bodies.
fn collect_definitions(blocks: &[Block], out: &mut Vec<String>, seen: &mut HashSet<String>) {
    for block in blocks {
        match block {
            Block::FootnoteDefinition { label, children } => {
                if seen.insert(label.clone()) {
                    out.push(label.clone());
                }
                collect_definitions(children, out, seen);
            }
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. } => collect_definitions(children, out, seen),
            Block::List { items, .. } => {
                for item in items {
                    collect_definitions(item, out, seen);
                }
            }
            _ => {}
        }
    }
}

/// [`first_definition_bodies`] over every label the tree defines, for walks
/// (e.g. `query.rs`'s cover search) that start from raw blocks rather than a
/// built index. A label defined only inside a shortcode body is absent —
/// `collect_definitions` stops there — so such definitions never test as
/// hoisted, matching the renderer, which never hoists them.
pub(super) fn hoisted_definition_bodies(blocks: &[Block]) -> HashMap<String, usize> {
    let mut defined = Vec::new();
    collect_definitions(blocks, &mut defined, &mut HashSet::new());
    first_definition_bodies(blocks, &defined)
}

/// Body address of the doc-order-first definition per label — the identity
/// [`is_hoisted`] decides by, shared so every walk that must agree with the
/// renderer answers "is this occurrence hoisted?" from the same lookup.
fn first_definition_bodies(blocks: &[Block], labels: &[String]) -> HashMap<String, usize> {
    labels
        .iter()
        .filter_map(|label| {
            FootnoteIndex::definition(blocks, label)
                .map(|children| (label.clone(), children.as_ptr() as usize))
        })
        .collect()
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
    hoisted: &HashMap<String, usize>,
) {
    for block in blocks {
        match block {
            Block::Heading { children, .. } | Block::Paragraph(children) => {
                collect_refs_in_inlines(children, out, seen)
            }
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. } => collect_refs(children, out, seen, hoisted),
            Block::FootnoteDefinition { label, children } => {
                if hoisted.get(label.as_str()) != Some(&(children.as_ptr() as usize)) {
                    collect_refs(children, out, seen, hoisted);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_refs(item, out, seen, hoisted);
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
            | Inline::Link { children, .. } => collect_refs_in_inlines(children, out, seen),
            _ => {}
        }
    }
}

/// Per-render footnote state: the document's [`FootnoteIndex`] plus the
/// counters that keep marker ids and back-links in step.
///
/// Default (empty index) is what every [`super::render::render_blocks`]
/// entry point carries, so a marker there falls back to its literal source.
#[derive(Default)]
pub struct FootnoteCtx {
    index: FootnoteIndex,
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
        let index = FootnoteIndex::build(blocks);
        // Built through the same `FootnoteIndex::definition` search the
        // endnote section renders from, so "the definition the section
        // hoists" and "the definition the body pass skips" are one lookup.
        let labels: Vec<String> = index.entries().iter().map(|(_, l)| l.clone()).collect();
        let hoisted = first_definition_bodies(blocks, &labels);
        Self {
            index,
            emitted: HashMap::new(),
            hoisted,
        }
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

/// [`is_hoisted`]'s identity check, taking the hoisted-bodies map directly
/// rather than a full [`FootnoteCtx`] — for walks (e.g.
/// `ast::plain_text::render_plain_text`) that only need
/// [`hoisted_definition_bodies`]'s numbering-free structural answer and
/// never build a `FootnoteCtx`.
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
    for (n, label) in ctx.index.entries().to_vec() {
        let Some(children) = FootnoteIndex::definition(blocks, &label) else {
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
