//! A paragraph made only of pictures is a `:::gallery rows`.
//!
//! Two or more pictures separated by nothing but spaces and line breaks, each
//! optionally followed by an italic line for its caption, build the same
//! [`GalleryShortcode`] the fenced block does, so one renderer owns the output.
//! Any prose, a link around a picture, a picture with a width or a non-image
//! embed keeps the paragraph as it is.

use super::node::{Block, Inline};
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
    // What the current line holds: nothing, pictures, or an italic caption.
    let (mut on_line, mut caption_line) = (false, false);
    for inline in inlines {
        match inline {
            Inline::Image { .. } if caption_line => return None,
            Inline::Image { .. } => {
                items.push(item(inline)?);
                on_line = true;
            }
            Inline::Emphasis(inner) if !on_line && !inner.is_empty() && !items.is_empty() => {
                let last = items.last_mut()?;
                if !last.caption.is_empty() {
                    return None;
                }
                last.caption = inner.clone();
                (on_line, caption_line) = (true, true);
            }
            Inline::LineBreak => (on_line, caption_line) = (false, false),
            Inline::Text(s) if s.trim().is_empty() => {
                if s.contains('\n') {
                    (on_line, caption_line) = (false, false);
                }
            }
            _ => return None,
        }
    }
    (items.len() >= 2).then(|| GalleryShortcode { items, rows: true, ..Default::default() })
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

#[cfg(test)]
#[path = "picture_row_tests.rs"]
mod tests;
