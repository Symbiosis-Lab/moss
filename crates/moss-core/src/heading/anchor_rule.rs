//! Whether a page shows heading permalink marks, and whether it has a contents list worth a bar.
//!
//! A permalink mark helps a reader who is linking into a long page and is clutter on a short one, so
//! a page shows them when at least two of its sections hold real running text. The floating
//! contents bar is the same question asked of the sections a reader would pick from: a page of two
//! headings over a card grid and a few links has nothing to navigate. Only counts feed the
//! decisions; the heading ids themselves are never conditional on them.

use crate::ast::{Block, Inline};

/// Half-words of running text a section needs to count as long (80 words).
const LONG_SECTION_HALF_WORDS: u32 = 160;
/// Long sections a page needs before it shows marks.
const LONG_SECTIONS_NEEDED: usize = 2;

/// The decision over measured sections: at least two hold 80 words, counted in half-words.
pub fn shows_heading_anchors(section_half_words: &[u32]) -> bool {
    section_half_words.iter().filter(|&&n| n >= LONG_SECTION_HALF_WORDS).count() >= LONG_SECTIONS_NEEDED
}

/// A top-level heading and what the blocks after it, up to the next heading, hold.
struct Span {
    level: u8,
    /// Paragraphs and the paragraphs of quotes and callouts.
    prose: u32,
    /// The paragraphs of list items, at any depth.
    listed: u32,
}

/// The page's top-level headings in order, each with its measured blocks. Text before the first
/// heading belongs to none. Lists, tables, code, figures, shortcodes (grids, card listings,
/// buttons), raw HTML, link cards and footnote definitions are not running text, and an image's
/// alt text is not read.
fn spans(blocks: &[Block]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for block in blocks {
        match block {
            Block::Heading { level, .. } => spans.push(Span { level: *level, prose: 0, listed: 0 }),
            other => {
                if let Some(span) = spans.last_mut() {
                    span.prose += prose_half_words(other);
                    span.listed += listed_half_words(other);
                }
            }
        }
    }
    spans
}

/// Measure a page's top-level blocks and decide on permalink marks.
///
/// A section is an H2, H3 or H4 and the blocks after it up to the next heading of any level; text
/// under an H1, H5 or H6 belongs to no section. Only prose counts.
pub fn wants_heading_anchors(blocks: &[Block]) -> bool {
    let sections: Vec<u32> = spans(blocks).iter().filter(|s| (2..=4).contains(&s.level)).map(|s| s.prose).collect();
    shows_heading_anchors(&sections)
}

/// Measure a page's top-level blocks and decide whether its sections are worth a contents bar.
///
/// The sections are the headings at the shallowest level from H2 down that the page uses, the
/// list the bar would show; each runs to the next heading at that level or above, its subsections
/// included. Prose and list items count, since a how-to section is often a list.
pub fn wants_contents_list(blocks: &[Block]) -> bool {
    let spans = spans(blocks);
    let Some(top) = spans.iter().map(|s| s.level).filter(|&l| l >= 2).min() else { return false };
    let mut sections = Vec::new();
    let mut open: Option<u32> = None;
    for span in &spans {
        if span.level <= top {
            sections.extend(open.take());
        }
        if span.level == top {
            open = Some(0);
        }
        if let Some(n) = open.as_mut() {
            *n += span.prose + span.listed;
        }
    }
    sections.extend(open);
    shows_heading_anchors(&sections)
}

fn prose_half_words(block: &Block) -> u32 {
    match block {
        Block::Paragraph(inlines) => {
            let mut text = String::new();
            push_reading_text(inlines, &mut text);
            half_words(&text)
        }
        Block::BlockQuote(children) | Block::Callout { children, .. } => children.iter().map(prose_half_words).sum(),
        _ => 0,
    }
}

fn listed_half_words(block: &Block) -> u32 {
    match block {
        Block::List { items, .. } => items.iter().flatten().map(|b| prose_half_words(b) + listed_half_words(b)).sum(),
        _ => 0,
    }
}

/// The words a reader reads: link text counts, an image's alt text does not.
fn push_reading_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) | Inline::Link { children: c, .. } => {
                push_reading_text(c, out)
            }
            Inline::LineBreak => out.push(' '),
            _ => {}
        }
    }
}

/// A run of non-CJK letters and digits is one word (two half-words); each CJK character is one half-word.
fn half_words(text: &str) -> u32 {
    let mut total = 0;
    let mut in_word = false;
    for c in text.chars() {
        if super::opening::is_cjk(c) {
            total += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                total += 2;
            }
            in_word = true;
        } else {
            in_word = false;
        }
    }
    total
}

#[cfg(test)]
#[path = "anchor_rule_tests.rs"]
mod tests;
