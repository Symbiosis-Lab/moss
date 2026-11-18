//! `header=over` on a full-width band (`::: {screen header=over}`): the band
//! that opens the page takes the header over its pictures as an opening hero
//! does, with the ink read from every picture in it. A band anywhere else
//! loses the attribute, as a later hero does.

use moss_core::ast::{is_comment, Block, Document, Inline, Url};

use super::HeaderOver;
use crate::build::media::dimensions::MediaDimensionLookup;

const OVER: &str = r#" data-header="over""#;

/// Whether `html` begins with a `screen` band that asks for the header over it.
fn starts_with_over_band(html: &str) -> bool {
    let Some(rest) = html.trim_start().strip_prefix("<div") else { return false };
    let tag = rest.split('>').next().unwrap_or_default();
    tag.contains(r#" data-width="screen""#) && tag.contains(OVER)
}

fn opens_over_band(block: &Block) -> bool {
    matches!(block, Block::Other(html) if starts_with_over_band(html))
}

fn pictures(inlines: &[Inline], out: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Image { src: Url::Resolved(r), .. } => out.push(r.href.clone()),
            Inline::Link { children, .. } | Inline::Emphasis(children) | Inline::Strong(children) => pictures(children, out),
            _ => {}
        }
    }
}

/// The pictures between the band's opening block and its closing `</div>`.
fn band_pictures(blocks: &[Block]) -> Vec<String> {
    let mut depth = 1_i32;
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Other(html) => {
                depth += html.matches("<div").count() as i32 - html.matches("</div>").count() as i32;
                if depth <= 0 {
                    break;
                }
            }
            Block::Paragraph(inlines) => pictures(inlines, &mut out),
            Block::Figure { image, .. } => pictures(std::slice::from_ref(image), &mut out),
            _ => {}
        }
    }
    out
}

/// Keep `header=over` on the band that opens the page (when no hero does) and
/// give it its ink, which is returned for the page's header; take it off every
/// other band.
pub fn mark_opening_band(doc: &mut Document, page_opens_with_band: bool, lookup: Option<&MediaDimensionLookup>) -> Option<HeaderOver> {
    let opening = page_opens_with_band
        .then(|| doc.blocks.iter().position(|b| !is_comment(b)))
        .flatten()
        .filter(|&i| opens_over_band(&doc.blocks[i]));
    let mut over = None;
    for i in 0..doc.blocks.len() {
        if !opens_over_band(&doc.blocks[i]) {
            continue;
        }
        let is_opening = Some(i) == opening;
        let ink = is_opening.then(|| super::ink_over_band(lookup, &band_pictures(&doc.blocks[i + 1..])));
        if let Block::Other(html) = &mut doc.blocks[i] {
            *html = match &ink {
                Some(ink) => html.replacen(OVER, &format!("{OVER}{}", ink.ink_attrs()), 1),
                None => html.replacen(OVER, "", 1),
            };
        }
        over = over.or(ink);
    }
    over
}
