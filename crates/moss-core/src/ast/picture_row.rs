//! A paragraph made only of pictures is a `:::gallery rows`.
//!
//! Two or more pictures separated by nothing but spaces and line breaks, each
//! optionally followed by an italic line for its caption, build the same
//! [`GalleryShortcode`] the fenced block does, so one renderer owns the output.
//! Any prose, a link around a picture, a picture with a width or a non-image
//! embed keeps the paragraph as it is.
//!
//! A picture alone on a line of a paragraph that also holds text is split out
//! as its own paragraph, a run of such lines as a row, and a picture with an
//! italic line under it as a figure ([`split_lines`]).

use super::document::{BlockMeta, Document};
use super::node::{is_blank, linked_image, lone_italic, Block, Inline};
use super::parser::{alt_only_text, standard_image_words};
use super::shortcode::{GalleryItem, GalleryShortcode, Shortcode};
use super::url::Url;
use crate::resolve::ext_kind::{reference_kind_for_ext, ExtKind};

/// Replace each qualifying top-level paragraph with its gallery.
pub(super) fn promote(blocks: &mut [Block]) {
    for block in blocks.iter_mut() {
        if let Block::Paragraph(inlines) = block {
            if let Some(gallery) = picture_row(inlines) {
                *block = Block::Shortcode(Shortcode::Gallery(gallery));
            }
        }
    }
}

fn picture_row(inlines: &[Inline]) -> Option<GalleryShortcode> {
    let mut items: Vec<GalleryItem> = Vec::new();
    // Pictures on the previous line, which the italic line directly under it captions.
    let (mut line_pictures, mut above, mut caption) = (0, 0, Vec::new());
    // What the current line holds: nothing, pictures, or an italic caption.
    let (mut on_line, mut caption_line) = (false, false);
    for inline in inlines {
        match inline {
            Inline::Image { .. } if caption_line => return None,
            Inline::Image { .. } => {
                items.push(item(inline)?);
                line_pictures += 1;
                on_line = true;
            }
            Inline::Emphasis(inner) if !on_line && !inner.is_empty() && !items.is_empty() => {
                // Under several pictures on one line the italic line belongs to
                // the row; under one picture it is that picture's caption.
                if above > 1 {
                    if !caption.is_empty() {
                        return None;
                    }
                    caption = inner.clone();
                } else {
                    let last = items.last_mut()?;
                    if !last.caption.is_empty() {
                        return None;
                    }
                    last.caption = inner.clone();
                }
                (on_line, caption_line) = (true, true);
            }
            Inline::LineBreak => (on_line, caption_line, above, line_pictures) = (false, false, line_pictures, 0),
            Inline::Text(s) if s.trim().is_empty() => {
                if s.contains('\n') {
                    (on_line, caption_line, above, line_pictures) = (false, false, line_pictures, 0);
                }
            }
            _ => return None,
        }
    }
    (items.len() >= 2).then(|| GalleryShortcode { items, rows: true, caption, ..Default::default() })
}

/// The gallery item for a picture that is plainly an image with no placement.
fn item(image: &Inline) -> Option<GalleryItem> {
    let Inline::Image { src, alt, title, is_wikilink, wikilink_pothole } = image else {
        return None;
    };
    let Url::Unresolved(dest) = src else { return None };
    let ext = crate::path_ext::path_extension_lower(dest);
    if reference_kind_for_ext(&ext) != ExtKind::Image {
        return None;
    }
    let (alt, alt_only, words) = if *is_wikilink {
        let words = crate::media::read_image_words(wikilink_pothole.as_deref().unwrap_or(""));
        (words.caption.clone(), false, words)
    } else if let Some(alt) = alt_only_text(alt) {
        (alt, true, crate::media::ImageWords::default())
    } else {
        let words = standard_image_words(alt);
        (words.caption.clone(), false, words)
    };
    if !words.placement.is_empty() {
        return None;
    }
    Some(GalleryItem {
        src: Url::unresolved(crate::media::strip_wikilink(dest)),
        alt,
        alt_only,
        attrs: words.display,
        caption: Vec::new(),
        title: title.clone(),
    })
}

/// Split each top-level paragraph at its picture-only lines, as if a blank
/// line stood around them. Runs after the hard-break pass, so only a real line
/// break splits; a CommonMark soft break is a space and its picture is inline.
/// An italic line directly under a picture stays with it, as its caption. A
/// standard-markdown picture split out without one is not made a figure: the
/// author never saw a caption under it, so its alt stays alt, where a
/// blank-line `![alt](x)` would be captioned. A wiki or video embed split out
/// is dispatched as a lone embed, so a wiki embed's pipe text is its caption,
/// as on its own line.
///
/// `captioned` builds the figure for one picture and its italic line.
pub(super) fn split_lines(doc: &mut Document, captioned: &dyn Fn(&[Inline]) -> Option<Block>) {
    let mut blocks = Vec::with_capacity(doc.blocks.len());
    let mut block_meta = Vec::with_capacity(doc.block_meta.len());
    for (block, meta) in std::mem::take(&mut doc.blocks).into_iter().zip(std::mem::take(&mut doc.block_meta)) {
        let Some(pieces) = (match &block { Block::Paragraph(inlines) => pieces(inlines, captioned), _ => None }) else {
            blocks.push(block);
            block_meta.push(meta);
            continue;
        };
        for (offset, piece) in pieces {
            blocks.push(piece);
            block_meta.push(BlockMeta { source_line: meta.source_line.map(|line| line + offset) });
        }
    }
    doc.blocks = blocks;
    doc.block_meta = block_meta;
}

/// The blocks `inlines` splits into, each with its line offset in the source.
/// `None` when it holds no picture-only line beside text.
fn pieces(inlines: &[Inline], captioned: &dyn Fn(&[Inline]) -> Option<Block>) -> Option<Vec<(usize, Block)>> {
    if !inlines.iter().any(|i| matches!(i, Inline::LineBreak)) {
        return None;
    }
    let lines: Vec<&[Inline]> = inlines.split(|i| matches!(i, Inline::LineBreak)).collect();
    let picture_line = |line: &[Inline]| line.iter().any(is_picture) && line.iter().all(|i| is_picture(i) || is_blank(i));
    let in_run: Vec<bool> = (0..lines.len())
        .map(|n| picture_line(lines[n]) || (n > 0 && picture_line(lines[n - 1]) && lone_italic(lines[n]).is_some()))
        .collect();
    if !in_run.contains(&true) || lines.iter().zip(&in_run).all(|(l, run)| *run || l.iter().all(is_blank)) {
        return None;
    }
    let mut out: Vec<(usize, Vec<Inline>, bool)> = Vec::new();
    let mut source_line = 0;
    for (line, &pictures) in lines.iter().zip(&in_run) {
        match out.last_mut() {
            Some((_, run, kind)) if *kind == pictures => {
                run.push(Inline::LineBreak);
                run.extend_from_slice(line);
            }
            _ => out.push((source_line, line.to_vec(), pictures)),
        }
        source_line += 1 + line.iter().map(line_breaks_in).sum::<usize>();
    }
    out.retain(|(_, run, _)| !run.iter().all(|i| is_blank(i) || matches!(i, Inline::LineBreak)));
    Some(out.into_iter().map(|(line, run, pictures)| (line, finished(run, pictures, captioned))).collect())
}

/// The block one piece becomes; a run of picture lines reads as it would alone.
fn finished(run: Vec<Inline>, pictures: bool, captioned: &dyn Fn(&[Inline]) -> Option<Block>) -> Block {
    if pictures {
        if let Some(gallery) = picture_row(&run) {
            return Block::Shortcode(Shortcode::Gallery(gallery));
        }
        if let Some(figure) = captioned(&run) {
            return figure;
        }
    }
    Block::Paragraph(run)
}

fn is_picture(inline: &Inline) -> bool {
    match inline {
        Inline::Image { .. } => true,
        Inline::Link { children, .. } => linked_image(children).is_some(),
        _ => false,
    }
}

/// Source line breaks inside one inline: a soft break, or an emphasis or link
/// that runs across lines.
fn line_breaks_in(inline: &Inline) -> usize {
    match inline {
        Inline::LineBreak => 1,
        Inline::Text(s) => s.matches('\n').count(),
        Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) | Inline::Mark(c) => c.iter().map(line_breaks_in).sum(),
        Inline::Link { children, .. } => children.iter().map(line_breaks_in).sum(),
        Inline::Image { .. } | Inline::Code(_) | Inline::FootnoteRef(_) | Inline::TaskMarker(_) | Inline::Other(_) => 0,
    }
}

#[cfg(test)]
#[path = "picture_row_tests.rs"]
mod tests;
