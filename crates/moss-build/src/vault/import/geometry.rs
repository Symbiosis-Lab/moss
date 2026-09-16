//! Canvas geometry → reading order.
//!
//! Canvas builders (Readymag, Cargo, old Squarespace) position widgets
//! absolutely, so DOM order ≠ visual order — a page's images typically
//! cluster after its text in the DOM while sitting beside specific passages
//! visually. Reading order is recovered by sorting on (top, left), and
//! side media is interleaved INTO a text column proportionally.

use super::blocks::Block;

/// A widget (or block) with its canvas position. Header-zone widgets with
/// no px geometry (percent-positioned fixed chrome) are encoded with
/// `top = f64::NEG_INFINITY` so they sort first, keeping DOM order among
/// themselves via the stable sort.
pub(crate) struct Positioned<T> {
    pub top: f64,
    pub left: f64,
    pub height: f64,
    pub item: T,
}

/// Stable (top, left) sort — the canvas reading order.
pub(crate) fn order<T>(mut widgets: Vec<Positioned<T>>) -> Vec<Positioned<T>> {
    widgets.sort_by(|a, b| {
        a.top
            .partial_cmp(&b.top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.left
                    .partial_cmp(&b.left)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    widgets
}

/// Interleave side media into a text column's block sequence.
///
/// The text column is ONE widget spanning `(text_top, text_height)`; its
/// blocks flow inside with unknown per-block pixel heights. A media item at
/// vertical fraction `f` of the column is inserted after the text block
/// where cumulative character weight first reaches `f × total` — a
/// uniform-density ESTIMATE, good enough to land media beside the right
/// passage (the acceptance corpus verifies placement page by page). Media
/// above the span leads, below trails; ties keep media order (already
/// geometry-sorted by the caller).
pub(crate) fn interleave_into_text(
    text_blocks: Vec<(Block, usize)>,
    text_top: f64,
    text_height: f64,
    media: Vec<Positioned<Block>>,
) -> Vec<Block> {
    let total: usize = text_blocks.iter().map(|(_, w)| *w).sum();
    if total == 0 || text_height <= 0.0 {
        let mut out: Vec<Block> = media.into_iter().map(|m| m.item).collect();
        out.extend(text_blocks.into_iter().map(|(b, _)| b));
        return out;
    }

    // Target cumulative-weight threshold for each media item.
    let mut media_slots: Vec<(f64, Block)> = media
        .into_iter()
        .map(|m| {
            let f = ((m.top - text_top) / text_height).clamp(0.0, 1.0);
            (f * total as f64, m.item)
        })
        .collect();
    // Caller passes geometry-sorted media; keep that order per slot.
    media_slots.reverse(); // pop() from the front

    let mut out = Vec::with_capacity(text_blocks.len() + media_slots.len());
    // Media at fraction 0 goes before any text.
    while media_slots.last().is_some_and(|(t, _)| *t <= 0.0) {
        out.push(media_slots.pop().expect("checked last").1);
    }
    let mut cum = 0usize;
    for (block, weight) in text_blocks {
        cum += weight;
        out.push(block);
        // Strict: media whose position lands exactly at a block's START
        // (threshold == cum of the previous block) reads as "beside that
        // block" and belongs after it, not before.
        while media_slots.last().is_some_and(|(t, _)| *t < cum as f64) {
            out.push(media_slots.pop().expect("checked last").1);
        }
    }
    out.extend(media_slots.into_iter().rev().map(|(_, b)| b));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Block {
        Block::RichText { html: s.into() }
    }
    fn img(s: &str) -> Block {
        Block::Image {
            src: s.into(),
            alt: String::new(),
        }
    }
    fn pos(top: f64, left: f64, item: Block) -> Positioned<Block> {
        Positioned {
            top,
            left,
            height: 100.0,
            item,
        }
    }

    #[test]
    fn order_sorts_top_then_left_keeping_header_zone_first() {
        let out = order(vec![
            pos(500.0, 0.0, img("late")),
            pos(f64::NEG_INFINITY, 0.0, text("header-a")),
            pos(0.0, 200.0, img("right")),
            pos(0.0, 0.0, img("left")),
            pos(f64::NEG_INFINITY, 0.0, text("header-b")),
        ]);
        let names: Vec<_> = out
            .iter()
            .map(|p| match &p.item {
                Block::RichText { html } => html.clone(),
                Block::Image { src, .. } => src.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, ["header-a", "header-b", "left", "right", "late"]);
    }

    #[test]
    fn interleave_places_media_by_proportional_char_weight() {
        // Text spans 0..1000px, four equal blocks of 100 chars each.
        // Media at 0px → before all; at 490px (~49%) → after block 2;
        // at 2000px (past end) → trailing.
        let blocks = vec![
            (text("b1"), 100),
            (text("b2"), 100),
            (text("b3"), 100),
            (text("b4"), 100),
        ];
        let media = vec![
            pos(0.0, 0.0, img("lead")),
            pos(490.0, 0.0, img("mid")),
            pos(2000.0, 0.0, img("tail")),
        ];
        let out = interleave_into_text(blocks, 0.0, 1000.0, media);
        let names: Vec<_> = out
            .iter()
            .map(|b| match b {
                Block::RichText { html } => html.clone(),
                Block::Image { src, .. } => src.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, ["lead", "b1", "b2", "mid", "b3", "b4", "tail"]);
    }

    #[test]
    fn empty_text_yields_media_then_nothing() {
        let out = interleave_into_text(vec![], 0.0, 0.0, vec![pos(10.0, 0.0, img("only"))]);
        assert_eq!(out.len(), 1);
    }
}
