//! Passes over a parsed document's inline runs.

use super::document::Document;
use super::node::{Block, Inline};

pub mod highlight;
pub mod line_breaks;
pub mod tags;

/// Call `f` on every top-level inline run in `doc`: a paragraph, heading,
/// definition title, figure caption, or table cell (header cells included),
/// at any depth of callout, quote, list item, definition or card. `f` handles
/// the nesting inside a run itself. Code blocks, shortcodes and raw HTML carry
/// no inline runs.
///
/// It visits one parse's runs: a shortcode body (a grid cell, a hero overlay)
/// is its own parse and gets the same passes there, so a reader of the whole
/// document, such as tag extraction, must descend into those bodies itself.
fn for_each_inline_run_mut(doc: &mut Document, f: &mut impl FnMut(&mut Vec<Inline>)) {
    for block in &mut doc.blocks {
        inline_runs_in_block(block, f);
    }
}

fn inline_runs_in_block(block: &mut Block, f: &mut impl FnMut(&mut Vec<Inline>)) {
    match block {
        Block::Heading { children, .. }
        | Block::Paragraph(children)
        | Block::DefinitionTitle(children) => f(children),
        Block::Figure { caption, .. } => {
            if let Some(caption) = caption {
                f(caption);
            }
        }
        Block::Callout { children, .. }
        | Block::BlockQuote(children)
        | Block::DefinitionList(children)
        | Block::DefinitionDefinition(children)
        | Block::LinkCard { children, .. }
        | Block::FootnoteDefinition { children, .. } => {
            for nested in children {
                inline_runs_in_block(nested, f);
            }
        }
        Block::List { items, .. } => {
            for nested in items.iter_mut().flatten() {
                inline_runs_in_block(nested, f);
            }
        }
        Block::Table { header, rows, .. } => {
            for cell in header.iter_mut().chain(rows.iter_mut().flatten()) {
                f(cell);
            }
        }
        Block::CodeBlock { .. } | Block::Shortcode(_) | Block::ThematicBreak | Block::Other(_) => {}
    }
}
