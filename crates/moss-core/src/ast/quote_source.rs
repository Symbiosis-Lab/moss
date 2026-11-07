//! A quotation's source line: the link that says where the quoted words sit.
//!
//! One rule serves the rendered `<blockquote cite>` and the page's JSON-LD
//! `citation`, so the markup and the metadata cannot disagree.

use super::grid_parts::quote_cell;
use super::node::{Block, Inline};
use super::plain_text::inlines_to_plain_text;
use super::visit::for_each_child_sequence;
use super::shortcode::Shortcode;

/// The first absolute link of a quotation's source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSource {
    pub href: String,
    pub text: String,
}

/// The source of a prose quotation: its last paragraph begins with an em dash
/// or horizontal bar and holds an absolute link.
pub fn prose_quote_source(children: &[Block]) -> Option<QuoteSource> {
    let Some(Block::Paragraph(line)) = children.last() else {
        return None;
    };
    let first = line.iter().find(|i| !matches!(i, Inline::Text(s) if s.trim().is_empty()))?;
    let Inline::Text(lead) = first else { return None };
    if !lead.trim_start().starts_with(['\u{2014}', '\u{2015}']) {
        return None;
    }
    source_link(line)
}

/// The first link in `line` (looking inside emphasis) whose address is
/// absolute `http` or `https`.
pub fn source_link(line: &[Inline]) -> Option<QuoteSource> {
    line.iter().find_map(|inline| match inline {
        Inline::Link { url, children, is_wikilink: false, .. } if is_absolute_web(url.href()) => {
            Some(QuoteSource { href: url.href().to_string(), text: inlines_to_plain_text(children) })
        }
        Inline::Emphasis(inner) | Inline::Strong(inner) | Inline::Strikethrough(inner) => source_link(inner),
        _ => None,
    })
}

fn is_absolute_web(href: &str) -> bool {
    let lower = href.get(..8).unwrap_or(href).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The source a quote cell's own dash line does not already supply: the cell's
/// italic source line, for a quote that has no dash line of its own.
pub(crate) fn grid_source_cite(quote: &Block, source: &[Inline]) -> Option<QuoteSource> {
    let Block::BlockQuote(children) = quote else { return None };
    prose_quote_source(children).is_none().then(|| source_link(source)).flatten()
}

/// A quote cell's cite: its own dash line when it has one, else its source line.
fn grid_quote_source(quote: &Block, source: &[Inline]) -> Option<QuoteSource> {
    let Block::BlockQuote(children) = quote else { return None };
    prose_quote_source(children).or_else(|| grid_source_cite(quote, source))
}

/// Every cited quotation in `blocks`, one entry per distinct address, in
/// document order.
pub fn citations(blocks: &[Block]) -> Vec<QuoteSource> {
    let mut found = Vec::new();
    collect(blocks, &mut found);
    found
}

fn collect(blocks: &[Block], found: &mut Vec<QuoteSource>) {
    for block in blocks {
        match block {
            Block::BlockQuote(children) => {
                if let Some(source) = prose_quote_source(children) {
                    push_distinct(found, source);
                }
            }
            Block::Shortcode(Shortcode::Grid(args)) => {
                for cell in &args.cells {
                    if let Some((quote, source)) = quote_cell(cell) {
                        if let Some(source) = grid_quote_source(quote, source) {
                            push_distinct(found, source);
                        }
                    } else {
                        collect(cell, found);
                    }
                }
                continue;
            }
            _ => {}
        }
        for_each_child_sequence(block, |children| {
            collect(children, found);
            true
        });
    }
}

fn push_distinct(found: &mut Vec<QuoteSource>, source: QuoteSource) {
    if !found.iter().any(|s| s.href == source.href) {
        found.push(source);
    }
}

#[cfg(test)]
#[path = "quote_source_tests.rs"]
mod tests;
