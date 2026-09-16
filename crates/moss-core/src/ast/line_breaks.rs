//! Obsidian-style hard line breaks.
//!
//! Obsidian ships `strictLineBreaks = false` by default, which sets remark's
//! `breaks: true`: a single newline inside a paragraph becomes a `<br>`.
//! CommonMark — and therefore pulldown-cmark — treats that same newline as a
//! space. So the *most ordinary markdown there is*, a paragraph the author
//! wrapped across two lines, renders differently in Obsidian's preview and on
//! the published moss site, with no exotic syntax involved.
//!
//! pulldown-cmark 0.13 exposes no hard-break option — verified against both
//! upstream and the workspace `[patch.crates-io]` fork, whose `Options::all()`
//! is exactly 17 flags with no `breaks` among them. A post-parse transform over
//! the typed AST is therefore the only available mechanism, and it is the
//! sanctioned layer: structural decisions belong to the typed data, never to a
//! regex pass over emitted HTML.
//!
//! The transform is exact rather than heuristic because a soft break is the
//! only thing that produces a bare newline inline. `parse_inline` maps
//! `Event::SoftBreak` to `Inline::Text("\n")` (parser.rs), and pulldown splits
//! every line break into its own `SoftBreak` event, so a `Text` inline never
//! contains a newline as part of a longer run. Matching `Text` whose content is
//! exactly `"\n"` therefore hits soft breaks and nothing else.
//!
//! Fenced code is untouched: `Block::CodeBlock` stores its body as a `String`,
//! not as inlines, so it is never walked here.

use super::document::Document;
use super::node::{Block, Inline};

/// Rewrite every soft break in `doc` as a hard `<br>`.
///
/// Call only when the site opted in (`ParseConfig::hard_line_breaks`).
pub fn apply(doc: &mut Document) {
    for block in &mut doc.blocks {
        in_block(block);
    }
}

fn in_block(block: &mut Block) {
    match block {
        Block::Heading { children, .. } | Block::Paragraph(children) => in_inlines(children),
        Block::Figure { caption, .. } => {
            if let Some(caption) = caption {
                in_inlines(caption);
            }
        }
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::LinkCard { children, .. }
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                in_block(nested);
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for nested in item {
                    in_block(nested);
                }
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in row {
                    in_inlines(cell);
                }
            }
        }
        // A soft break inside a code block is literal text the author typed,
        // and `Shortcode` carries its own already-rendered payload.
        Block::CodeBlock { .. }
        | Block::Shortcode(_)
        | Block::ThematicBreak
        | Block::Other(_) => {}
    }
}

fn in_inlines(inlines: &mut Vec<Inline>) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Text(s) if s == "\n" => *inline = Inline::LineBreak,
            Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => in_inlines(children),
            Inline::Link { children, .. } => in_inlines(children),
            Inline::Text(_)
            | Inline::Image { .. }
            | Inline::Code(_)
            | Inline::LineBreak
            | Inline::FootnoteRef(_)
            | Inline::TaskMarker(_)
            | Inline::Other(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    fn breaks(md: &str) -> Document {
        let mut doc = parse(md);
        apply(&mut doc);
        doc
    }

    fn count_line_breaks(inlines: &[Inline]) -> usize {
        inlines
            .iter()
            .map(|i| match i {
                Inline::LineBreak => 1,
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) => {
                    count_line_breaks(c)
                }
                Inline::Link { children, .. } => count_line_breaks(children),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn soft_break_becomes_a_hard_break() {
        let doc = breaks("one\ntwo\n");
        match &doc.blocks[0] {
            Block::Paragraph(children) => {
                assert_eq!(count_line_breaks(children), 1, "{children:?}");
                assert!(!children.iter().any(|i| matches!(i, Inline::Text(s) if s == "\n")));
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn paragraph_boundaries_are_not_line_breaks() {
        // A blank line already ends the paragraph — it must not also become a
        // <br>, or every paragraph gains a trailing break.
        let doc = breaks("one\n\ntwo\n");
        assert_eq!(doc.blocks.len(), 2);
        for block in &doc.blocks {
            match block {
                Block::Paragraph(children) => assert_eq!(count_line_breaks(children), 0),
                other => panic!("expected paragraph, got {other:?}"),
            }
        }
    }

    #[test]
    fn code_block_content_is_untouched() {
        let doc = breaks("```\nline one\nline two\n```\n");
        match &doc.blocks[0] {
            Block::CodeBlock { value, .. } => {
                assert!(value.contains("line one\nline two"), "{value:?}");
            }
            other => panic!("expected code block, got {other:?}"),
        }
    }

    #[test]
    fn breaks_inside_emphasis_and_list_items_are_found() {
        let doc = breaks("- *soft\nwrap*\n");
        // One break, nested two levels down (list item → paragraph → emphasis).
        match &doc.blocks[0] {
            Block::List { items, .. } => match &items[0][0] {
                Block::Paragraph(children) => assert_eq!(count_line_breaks(children), 1),
                other => panic!("expected paragraph, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn without_apply_the_document_is_unchanged() {
        let doc = parse("one\ntwo\n");
        match &doc.blocks[0] {
            Block::Paragraph(children) => {
                assert_eq!(count_line_breaks(children), 0);
                assert!(children.iter().any(|i| matches!(i, Inline::Text(s) if s == "\n")));
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }
}
