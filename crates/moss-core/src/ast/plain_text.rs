//! Plain-text lowering of the typed AST.
//!
//! Two things live here, deliberately kept apart:
//!
//! - [`inlines_to_plain_text`] — the crate's one inline-flattening POLICY
//!   (what does an equation, a line break, a footnote marker "look like" as
//!   bare text). Promoted from `heading::text::inlines_to_text` per that
//!   module's own doc comment: "a THIRD non-heading consumer appearing is
//!   the signal to promote this walker to its own `ast/plain_text.rs`."
//!   `ast::extract_hero`'s hero-overlay rung was the second consumer;
//!   `newsletter.rs`'s email plain text (ADR-036) is the third.
//!
//!   **`build::page::meta::extract_description` deliberately does NOT use
//!   this policy**, despite being named alongside email in ADR-036's
//!   initial scope. Investigation during that migration found the two
//!   functions disagree on what an image contributes: this walker folds an
//!   image's `alt` into the flattened text (right for a heading whose only
//!   content is `![diagram](x.png)`); `extract_description` drops a
//!   whole-image paragraph with NO text at all and falls through to the
//!   next real paragraph (`test_extract_description_strips_image_syntax`,
//!   `a_broken_image_paragraph_is_deliberately_not_the_description` in
//!   `meta_tests.rs` pin this). That is a deliberate excerpt-quality choice
//!   — an image's alt attribute, or a footnote reference that broke an
//!   image's alt bracket, is not SEO-description material even though a
//!   heading slug or hero overlay may reasonably want it. See ADR-036's
//!   "meta.rs" note for the full reasoning.
//! - [`render_plain_text`] — a full `Document` → plain-text LOWERING,
//!   parallel to [`super::render::render_document`]'s HTML lowering. This is
//!   new: nothing before ADR-036 walked the whole typed tree into a
//!   plain-text document. `newsletter.rs`'s email plain-text body used to be
//!   a fourth independent `pulldown_cmark::Event` walk, hand-tracking its
//!   own list/quote/image nesting state; this function derives the same
//!   shape (dash/numbered bullets, `>` quote prefixing, fenced code with a
//!   language tag, `[N]` footnote markers + a hoisted endnote section,
//!   `[image: url — alt]` markers, `text (href)` links) from the tree's
//!   actual structure, so it can never structurally disagree with the HTML
//!   lowering the way two independent walkers eventually did.
//!
//! `render_plain_text` and `inlines_to_plain_text` answer different
//! questions and must not be confused: the former is "what does this whole
//! document read as in a plain-text mail client" (links keep their href,
//! footnotes are numbered and hoisted, images become a bracketed marker);
//! the latter is "what is this inline run's bare text" (a link keeps only
//! its visible words, a footnote marker is a pointer and contributes
//! nothing) — the same policy a heading's `<hN id>` slug or an SEO
//! `<meta name="description">` needs. Reusing one for the other's job would
//! either leak hrefs into a heading anchor or drop every link's destination
//! from an email body.

use std::borrow::Cow;
use std::collections::HashMap;

use super::document::Document;
use super::footnotes::{self, FootnoteIndex};
use super::math_text::math_source_from_other;
use super::node::{Block, CalloutKind, Fold, Inline};
use super::url::Url;

/// The vocabulary [`push_atom`] speaks — every distinguishable kind of
/// content a plain-text-flattening walker can encounter. Moved verbatim
/// from `heading::text` (see that module for [`super::super::heading::text::events_to_text`],
/// the mid-parse sibling that reads `pulldown_cmark::Event`s instead of
/// `Inline`s and therefore cannot share this walker, only its policy).
pub(crate) enum TextAtom<'a> {
    /// Author text or code-span content. Reproduced verbatim: a code span's
    /// backticks are markup, its contents are prose.
    Verbatim(&'a str),
    /// An equation, already in **delimited markdown-source** form
    /// (`$…$` / `$$…$$`). See [`super::math_text`] for why plain-text
    /// contexts get the source rather than the bare TeX.
    Math(Cow<'a, str>),
    /// An explicit line break inside the flattened run.
    Break,
}

/// **THE** policy for [`inlines_to_plain_text`] / `events_to_text`: the
/// single place that decides what each kind of content contributes to
/// flattened plain text. Both walkers funnel through it.
pub(crate) fn push_atom(out: &mut String, atom: TextAtom<'_>) {
    match atom {
        TextAtom::Verbatim(t) => out.push_str(t),
        TextAtom::Math(src) => out.push_str(&src),
        TextAtom::Break => out.push(' '),
    }
}

fn push_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(t) => push_atom(out, TextAtom::Verbatim(t)),
            Inline::Code(c) => push_atom(out, TextAtom::Verbatim(c)),
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => push_inlines(children, out),
            // A footnote marker is a pointer, not reading text: it must not
            // shift a heading's anchor slug or leak into a description.
            Inline::FootnoteRef(_) => {}
            // A checkbox contributes no flattened text.
            Inline::TaskMarker(_) => {}
            Inline::Link { children, .. } => push_inlines(children, out),
            Inline::Image { alt, .. } => push_atom(out, TextAtom::Verbatim(alt)),
            Inline::LineBreak => push_atom(out, TextAtom::Break),
            // A math fallback node is raw HTML, but it is the only
            // `Inline::Other` that carries author text.
            Inline::Other(html) => {
                if let Some(src) = math_source_from_other(html) {
                    push_atom(out, TextAtom::Math(Cow::Owned(src)));
                }
            }
        }
    }
}

/// Flatten an inline slice to plain text: emphasis/link/image markup
/// disappears, leaving the words a reader would say aloud. A link keeps
/// only its visible children (not its href); an image keeps only its alt
/// text; a footnote marker and a task checkbox contribute nothing.
///
/// This is the policy heading slugs, autocomplete labels, hero-overlay
/// description text, and SEO description extraction all share — see the
/// module doc for why this is deliberately NOT what a whole-document email
/// body wants (that's [`render_plain_text`]).
pub fn inlines_to_plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    push_inlines(inlines, &mut out);
    out
}

/// Render a [`Document`] to a plain-text string.
///
/// Structural parallel to [`super::render::render_document`]: one pass
/// builds the document's [`FootnoteIndex`] and the hoisted-body map, then
/// walks every top-level block, then appends the hoisted endnote section.
/// Conventions (matching `newsletter.rs`'s pre-ADR-036 hand-rolled walker,
/// which this supersedes):
///
/// - Paragraphs and headings: flattened text followed by a blank line.
///   Headings carry no special markup — a plain-text mail client has no
///   bold/size, so a heading reads as a short paragraph.
/// - Lists: `- ` (unordered) or `N. ` (ordered, renumbered to skip any item
///   a footnote hoist emptied) with two-space indent per nesting level.
/// - Blockquotes and callouts: every line of the body prefixed with `> `
///   (callouts additionally get a synthesized `[!kind] Title` marker line,
///   reconstructing the Obsidian source shape the typed parser consumed).
/// - Code blocks: fenced with the language tag, each line under the quote
///   prefix (if any) its container carries.
/// - Tables: cells joined with `" | "`, one line per row.
/// - Links: `text (href)`. Images: `[image: href]` or `[image: href — alt]`.
/// - Footnotes: `[N]` inline (or `[^label]` when the label isn't indexed),
///   hoisted to a trailing `--` / `[N] body` section in index order.
///
/// Does NOT append any email-specific footer (unsubscribe line, etc.) —
/// callers own their own presentation around this lowering.
pub fn render_plain_text(doc: &Document) -> String {
    let index = FootnoteIndex::build(&doc.blocks);
    let hoisted = footnotes::hoisted_definition_bodies(&doc.blocks);
    let mut out = String::new();
    render_blocks(&doc.blocks, &index, &hoisted, 0, &mut out);
    render_footnote_section(&doc.blocks, &index, &hoisted, &mut out);
    out
}

fn render_blocks(
    blocks: &[Block],
    index: &FootnoteIndex,
    hoisted: &HashMap<String, usize>,
    list_depth: usize,
    out: &mut String,
) {
    for block in blocks {
        render_block(block, index, hoisted, list_depth, out);
    }
}

fn render_blocks_to_string(
    blocks: &[Block],
    index: &FootnoteIndex,
    hoisted: &HashMap<String, usize>,
    list_depth: usize,
) -> String {
    let mut buf = String::new();
    render_blocks(blocks, index, hoisted, list_depth, &mut buf);
    buf
}

/// Same emptiness test as `render::hoist_emptied`: a container HAD children
/// and they all rendered to nothing (the only thing inside was a hoisted
/// footnote definition). Exact, never `.trim()` — see that function's doc
/// for why.
fn hoist_emptied(children: &[Block], rendered: &str) -> bool {
    !children.is_empty() && rendered.is_empty()
}

/// Prefix every non-empty line of `body` with `> `. A blank line (a
/// paragraph separator) stays blank rather than becoming a bare `>` —
/// matching the pre-ADR-036 email walker's convention, which never wrote
/// the prefix on a separator line. Nested quotes compose for free: an
/// inner quote's own `> `-prefixed lines are non-empty, so the outer quote
/// adds its own `> ` on top, producing `> > ` for doubly-nested content.
fn quote_prefix_lines(body: &str) -> String {
    body.split('\n')
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_href(url: &Url) -> &str {
    match url {
        Url::Resolved(r) => &r.href,
        Url::Unresolved(s) => {
            // Defensive: visit_urls_mut should have resolved every URL
            // before any renderer sees the document. Emit the raw string
            // rather than panic — matches render.rs's release-mode fallback.
            debug_assert!(false, "Url::Unresolved({s:?}) reached render_plain_text");
            s
        }
    }
}

fn render_block(
    block: &Block,
    index: &FootnoteIndex,
    hoisted: &HashMap<String, usize>,
    list_depth: usize,
    out: &mut String,
) {
    match block {
        Block::Heading { children, .. } => {
            render_inlines_plain(children, index, out);
            out.push_str("\n\n");
        }
        Block::Paragraph(children) => {
            render_inlines_plain(children, index, out);
            out.push_str("\n\n");
        }
        Block::Callout {
            kind,
            fold,
            title,
            children,
        } => {
            let body = render_blocks_to_string(children, index, hoisted, list_depth);
            if hoist_emptied(children, &body) {
                return;
            }
            let mut marker_line = callout_marker(*kind, *fold);
            if let Some(t) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                marker_line.push(' ');
                marker_line.push_str(t);
            }
            let mut full = marker_line;
            if !body.is_empty() {
                full.push('\n');
                full.push_str(body.trim_end_matches('\n'));
            }
            out.push_str(&quote_prefix_lines(&full));
            out.push_str("\n\n");
        }
        Block::List {
            ordered,
            start,
            items,
            ..
        } => {
            let mut body = String::new();
            let mut num = start.unwrap_or(1);
            let indent = "  ".repeat(list_depth);
            for item_blocks in items {
                let item = if let [Block::Paragraph(inlines)] = item_blocks.as_slice() {
                    let mut s = String::new();
                    render_inlines_plain(inlines, index, &mut s);
                    s
                } else {
                    // Not the single-paragraph shortcut above, so build the
                    // item block-by-block rather than via
                    // `render_blocks_to_string`: a nested `List` immediately
                    // following a `Paragraph` continues that paragraph's own
                    // line — matching the source markdown, which has no
                    // blank line between "- outer" and its indented
                    // "  - inner" — so `Paragraph`'s usual blank-line
                    // separator is collapsed to a single newline at exactly
                    // that one adjacency. Every other pair of blocks keeps
                    // its normal blank-line separation.
                    let mut s = String::new();
                    for (i, child) in item_blocks.iter().enumerate() {
                        if i > 0
                            && matches!(child, Block::List { .. })
                            && matches!(item_blocks[i - 1], Block::Paragraph(_))
                        {
                            while s.ends_with('\n') {
                                s.pop();
                            }
                            s.push('\n');
                        }
                        render_block(child, index, hoisted, list_depth + 1, &mut s);
                    }
                    s
                };
                if hoist_emptied(item_blocks, &item) {
                    continue;
                }
                body.push_str(&indent);
                if *ordered {
                    body.push_str(&num.to_string());
                    body.push_str(". ");
                    num += 1;
                } else {
                    body.push_str("- ");
                }
                body.push_str(item.trim_end_matches('\n'));
                body.push('\n');
            }
            out.push_str(&body);
            if !body.is_empty() && list_depth == 0 {
                out.push('\n');
            }
        }
        Block::CodeBlock { lang, value } => {
            out.push_str("```");
            out.push_str(lang.as_deref().unwrap_or(""));
            out.push('\n');
            out.push_str(value);
            if !value.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        Block::Table { header, rows, .. } => {
            render_table_row(header, index, out);
            for row in rows {
                render_table_row(row, index, out);
            }
            out.push('\n');
        }
        Block::BlockQuote(children) => {
            let body = render_blocks_to_string(children, index, hoisted, list_depth);
            if hoist_emptied(children, &body) {
                return;
            }
            out.push_str(&quote_prefix_lines(body.trim_end_matches('\n')));
            out.push_str("\n\n");
        }
        Block::Shortcode(_) => {
            // Shortcodes are stripped or expanded out of the raw markdown
            // upstream (e.g. newsletter.rs's `expand_recent_then_strip`)
            // before it ever reaches a parser — a shortcode block surviving
            // to this renderer is defensive-only and contributes no text.
        }
        Block::ThematicBreak => out.push_str("---\n\n"),
        Block::Figure { image, .. } => {
            render_inline_plain(image, index, out);
            out.push_str("\n\n");
        }
        Block::LinkCard { url, children } => {
            let body = render_blocks_to_string(children, index, hoisted, list_depth);
            if hoist_emptied(children, &body) {
                return;
            }
            out.push_str(body.trim());
            let href = url_href(url);
            if !href.is_empty() {
                out.push_str(" (");
                out.push_str(href);
                out.push(')');
            }
            out.push_str("\n\n");
        }
        Block::FootnoteDefinition { label, children } => {
            if footnotes::is_hoisted_in(label, children, hoisted) {
                return;
            }
            render_blocks(children, index, hoisted, list_depth, out);
        }
        Block::Other(_) => {
            // Raw HTML (block-level) is not text-bearing in a plain-text
            // lowering — matches the pre-ADR-036 walker, which never
            // arm'd `Event::Html`.
        }
    }
}

/// Reconstruct the Obsidian callout marker line (`[!note]+`) from the
/// typed `kind`/`fold` fields — the inverse of the parser's callout
/// detection. The pre-ADR-036 walker never saw this: callouts didn't exist
/// as a distinct construct at the raw-event level it operated on, so a
/// `[!note]` blockquote just rendered as an ordinary quote carrying its
/// marker line as literal paragraph text. The typed parser now consumes
/// that marker into `kind`/`title`, so a plain-text lowering that skipped
/// this would silently drop the callout signal instead of preserving it.
fn callout_marker(kind: CalloutKind, fold: Option<Fold>) -> String {
    let suffix = match fold {
        Some(Fold::Open) => "+",
        Some(Fold::Closed) => "-",
        None => "",
    };
    format!("[!{}]{}", kind.as_slug(), suffix)
}

fn render_table_row(cells: &[Vec<Inline>], index: &FootnoteIndex, out: &mut String) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push_str(" | ");
        }
        render_inlines_plain(cell, index, out);
    }
    out.push('\n');
}

fn render_inlines_plain(inlines: &[Inline], index: &FootnoteIndex, out: &mut String) {
    for inline in inlines {
        render_inline_plain(inline, index, out);
    }
}

fn render_inline_plain(inline: &Inline, index: &FootnoteIndex, out: &mut String) {
    match inline {
        Inline::Text(t) => out.push_str(t),
        Inline::Code(c) => {
            out.push('`');
            out.push_str(c);
            out.push('`');
        }
        Inline::Emphasis(children) | Inline::Strong(children) => {
            render_inlines_plain(children, index, out);
        }
        Inline::Strikethrough(children) => {
            out.push_str("~~");
            render_inlines_plain(children, index, out);
            out.push_str("~~");
        }
        Inline::FootnoteRef(label) => match index.number(label) {
            Some(n) => {
                out.push('[');
                out.push_str(&n.to_string());
                out.push(']');
            }
            None => {
                out.push_str("[^");
                out.push_str(label);
                out.push(']');
            }
        },
        Inline::TaskMarker(checked) => {
            out.push_str(if *checked { "[x] " } else { "[ ] " });
        }
        Inline::Link { url, children, .. } => {
            let mut text = String::new();
            render_inlines_plain(children, index, &mut text);
            out.push_str(&text);
            let href = url_href(url);
            if !href.is_empty() {
                out.push_str(" (");
                out.push_str(href);
                out.push(')');
            }
        }
        Inline::Image { src, alt, .. } => {
            let href = url_href(src);
            if alt.trim().is_empty() {
                out.push_str("[image: ");
                out.push_str(href);
                out.push(']');
            } else {
                out.push_str("[image: ");
                out.push_str(href);
                out.push_str(" — ");
                out.push_str(alt);
                out.push(']');
            }
        }
        Inline::LineBreak => out.push('\n'),
        Inline::Other(html) => {
            if let Some(src) = math_source_from_other(html) {
                out.push('`');
                out.push_str(&src);
                out.push('`');
            }
            // A non-math `Inline::Other` is raw inline HTML — invisible in
            // plain text, matching the pre-ADR-036 walker (no Html arm).
        }
    }
}

fn render_footnote_section(
    blocks: &[Block],
    index: &FootnoteIndex,
    hoisted: &HashMap<String, usize>,
    out: &mut String,
) {
    if index.is_empty() {
        return;
    }
    out.push_str("--\n");
    for (n, label) in index.entries() {
        let Some(children) = FootnoteIndex::definition(blocks, label) else {
            continue;
        };
        let body = render_blocks_to_string(children, index, hoisted, 0);
        out.push('[');
        out.push_str(&n.to_string());
        out.push_str("] ");
        out.push_str(body.trim_end());
        out.push('\n');
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_with_config;
    use crate::ast::parser::ParseConfig;

    fn render(md: &str) -> String {
        let mut doc = parse_with_config(md, &ParseConfig::default());
        crate::ast::classify_remaining_urls(&mut doc);
        render_plain_text(&doc)
    }

    #[test]
    fn paragraph_renders_flat_text_with_blank_line() {
        assert_eq!(render("Hello world.\n"), "Hello world.\n\n");
    }

    #[test]
    fn heading_has_no_special_markup() {
        assert_eq!(render("# Title\n"), "Title\n\n");
    }

    #[test]
    fn unordered_list_uses_dash_bullets() {
        assert_eq!(render("- one\n- two\n"), "- one\n- two\n\n");
    }

    #[test]
    fn ordered_list_numbers_items() {
        assert_eq!(render("1. one\n2. two\n"), "1. one\n2. two\n\n");
    }

    #[test]
    fn nested_list_indents_two_spaces_per_level() {
        let out = render("- outer\n  - inner\n");
        assert!(out.contains("- outer\n"));
        assert!(out.contains("  - inner\n"), "got: {out:?}");
    }

    #[test]
    fn blockquote_prefixes_every_line_with_gt() {
        // A soft break inside a paragraph folds to a literal "\n" in the AST
        // (matching `heading::text`'s documented SoftBreak handling), so this
        // stays two lines, each carrying its own `> ` prefix.
        assert_eq!(
            render("> line one\n> line two\n"),
            "> line one\n> line two\n\n"
        );
    }

    #[test]
    fn blockquote_with_two_paragraphs_keeps_blank_separator_unprefixed() {
        let out = render("> para one\n>\n> para two\n");
        assert_eq!(out, "> para one\n\n> para two\n\n");
    }

    #[test]
    fn nested_blockquote_doubles_the_prefix() {
        let out = render("> outer\n> > inner\n");
        assert!(out.contains("> > inner"), "got: {out:?}");
    }

    #[test]
    fn code_block_keeps_language_tag() {
        let out = render("```rust\nfn main() {}\n```\n");
        assert_eq!(out, "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn code_block_with_no_language_has_empty_info_string() {
        let out = render("```\nplain\n```\n");
        assert_eq!(out, "```\nplain\n```\n\n");
    }

    #[test]
    fn table_cells_join_with_pipe() {
        let out = render("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert_eq!(out, "a | b\n1 | 2\n\n");
    }

    #[test]
    fn task_markers_render_as_checkbox_text() {
        let out = render("- [ ] todo\n- [x] done\n");
        assert_eq!(out, "- [ ] todo\n- [x] done\n\n");
    }

    #[test]
    fn link_renders_text_and_href() {
        let out = render("[docs](https://example.com/docs)\n");
        assert_eq!(out, "docs (https://example.com/docs)\n\n");
    }

    #[test]
    fn image_with_alt_renders_bracketed_marker() {
        let out = render("![a cat](cat.jpg)\n");
        assert_eq!(out, "[image: cat.jpg — a cat]\n\n");
    }

    #[test]
    fn image_without_alt_renders_bracketed_marker_no_dash() {
        let out = render("![](cat.jpg)\n");
        assert_eq!(out, "[image: cat.jpg]\n\n");
    }

    #[test]
    fn strikethrough_keeps_literal_markers() {
        let out = render("~~gone~~\n");
        assert_eq!(out, "~~gone~~\n\n");
    }

    #[test]
    fn footnote_marker_and_hoisted_endnote_section() {
        let out = render("See[^a].\n\n[^a]: The note.\n");
        assert_eq!(out, "See[1].\n\n--\n[1] The note.\n\n");
    }

    #[test]
    fn footnote_numbering_follows_first_reference_order() {
        let out = render("First[^b], then[^a].\n\n[^a]: A.\n[^b]: B.\n");
        assert!(out.starts_with("First[1], then[2].\n\n"), "got: {out:?}");
        assert!(out.contains("[1] B."), "got: {out:?}");
        assert!(out.contains("[2] A."), "got: {out:?}");
    }

    #[test]
    fn unreferenced_footnote_is_not_silently_dropped() {
        let out = render("No marker here.\n\n[^orphan]: Orphan note.\n");
        assert!(out.contains("[1] Orphan note."), "got: {out:?}");
    }

    #[test]
    fn list_item_that_is_only_a_hoisted_footnote_is_erased_without_a_bullet() {
        let out = render("- one\n- [^a]: hoisted note\n- two\n\nref[^a]\n");
        // The middle item's entire content is the hoisted definition; it
        // must vanish along with its bullet, and the surviving items
        // renumber 1/2 rather than skipping a number.
        assert!(
            out.contains("1. one\n2. two\n") || out.contains("- one\n- two\n"),
            "got: {out:?}"
        );
    }

    #[test]
    fn callout_reconstructs_marker_line() {
        let out = render("> [!note] Heads up\n> Body text.\n");
        assert_eq!(out, "> [!note] Heads up\n> Body text.\n\n");
    }

    #[test]
    fn callout_without_title_has_bare_marker() {
        let out = render("> [!warning]\n> Body.\n");
        assert_eq!(out, "> [!warning]\n> Body.\n\n");
    }

    #[test]
    fn inlines_to_plain_text_drops_link_href_and_footnote_marker() {
        let doc = parse_with_config(
            "A [link](/x) and a note[^a].\n\n[^a]: n\n",
            &ParseConfig::default(),
        );
        let Block::Paragraph(inlines) = &doc.blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(inlines_to_plain_text(inlines), "A link and a note.");
    }
}
