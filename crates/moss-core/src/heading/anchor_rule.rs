//! Whether a page shows heading permalink marks.
//!
//! A permalink mark helps a reader who is linking into a long page and is clutter on a short one, so
//! a page shows them when at least two of its sections hold real running text. Only counts feed
//! the decision; the heading ids themselves are never conditional on it.

use crate::ast::{Block, Inline};

/// Half-words of running text a section needs to count as long (80 words).
const LONG_SECTION_HALF_WORDS: u32 = 160;
/// Long sections a page needs before it shows marks.
const LONG_SECTIONS_NEEDED: usize = 2;

/// The decision over measured sections: at least two hold 80 words, counted in half-words.
pub fn shows_heading_anchors(section_half_words: &[u32]) -> bool {
    section_half_words.iter().filter(|&&n| n >= LONG_SECTION_HALF_WORDS).count() >= LONG_SECTIONS_NEEDED
}

/// Measure a page's top-level blocks and decide.
///
/// A section is an H2, H3 or H4 and the blocks after it up to the next heading of any level; text
/// before the first one, and under an H1, H5 or H6, belongs to no section. Only paragraphs and the
/// paragraphs of quotes and callouts count: lists, tables, code, figures, shortcodes, raw HTML,
/// link cards and footnote definitions are not running text, and an image's alt text is not read.
pub fn wants_heading_anchors(blocks: &[Block]) -> bool {
    let mut sections = Vec::new();
    let mut open: Option<u32> = None;
    for block in blocks {
        match block {
            Block::Heading { level, .. } => {
                sections.extend(open.take());
                if (2..=4).contains(level) {
                    open = Some(0);
                }
            }
            other => {
                if let Some(n) = open.as_mut() {
                    *n += prose_half_words(other);
                }
            }
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
