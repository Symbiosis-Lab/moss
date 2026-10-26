//! Read-only structural queries over the typed AST.
//!
//! Pure helpers that walk a `Document` to extract structural facts
//! (first image, presence of a particular shortcode kind, etc.). No
//! mutation. Pure function of the AST.
//!
//! # Why this module exists
//!
//! Several pipeline-level decisions historically scanned rendered HTML
//! with regex (`first_body_image` in `cover.rs`, etc.). Phase 4
//! retires those by walking the typed AST instead. Each `find_*` helper
//! is the typed equivalent of one historical regex.

use super::document::Document;
use super::node::{Block, Inline};
use super::url::{Url, UrlKind};
use super::shortcode::Shortcode;
use super::visit::visit_blocks;
use std::collections::HashMap;

/// What a page marks a markdown image for, with an HTML comment on the line
/// after it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaMarker {
    /// `<!-- photography -->`
    Photography,
    /// `<!-- video-meta -->` or `<!-- video-meta: collection -->`
    Video { collection: Option<String> },
}

impl MediaMarker {
    /// The marker `html` consists of, if it is only that comment. Public
    /// because the same grammar applies after media written as HTML by hand,
    /// which the build reads from the HTML itself.
    pub fn parse(html: &str) -> Option<Self> {
        let body = html.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim();
        if body == "photography" {
            return Some(Self::Photography);
        }
        let rest = body.strip_prefix("video-meta")?;
        let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
        if !rest.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        Some(Self::Video { collection: (!rest.is_empty()).then(|| rest.to_string()) })
    }

    fn accepts(&self, kind: crate::resolve::ext_kind::ExtKind) -> bool {
        use crate::resolve::ext_kind::ExtKind;
        matches!((self, kind), (Self::Photography, ExtKind::Image) | (Self::Video { .. }, ExtKind::Video))
    }
}

/// A markdown image marked for a collection page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedMedia {
    pub marker: MediaMarker,
    /// The image's address, as the page renders it.
    pub href: String,
    /// The figure's caption, else the alt text. May be empty.
    pub title: String,
}

/// The markdown image `blocks[i]` marks, when `blocks[i]` is a marker comment
/// and the block before it shows one image on its own (a figure, or a
/// paragraph holding nothing else) of the kind the marker collects: an image
/// file for photography, a video file for video. The typed equivalent of the
/// markdown half of the photography and video regexes that used to scan the
/// rendered page, which depended on the exact markup around the `<img>`.
pub fn marked_media_at(blocks: &[Block], i: usize) -> Option<MarkedMedia> {
    let Block::Other(html) = blocks.get(i)? else { return None };
    let marker = MediaMarker::parse(html)?;
    let (image, caption) = match blocks.get(i.checked_sub(1)?)? {
        Block::Figure { image, caption, .. } => (image, caption.as_deref()),
        Block::Paragraph(inlines) => {
            let mut shown = inlines
                .iter()
                .filter(|i| !matches!(i, Inline::LineBreak) && !matches!(i, Inline::Text(t) if t.trim().is_empty()));
            match (shown.next(), shown.next()) {
                (Some(image), None) => (image, None),
                _ => return None,
            }
        }
        _ => return None,
    };
    let Inline::Image { src, alt, .. } = image else { return None };
    let href = src.href().to_string();
    let kind = crate::resolve::ext_kind::reference_kind_for_ext(&crate::path_ext::path_extension_lower(&href));
    if !marker.accepts(kind) {
        return None;
    }
    let caption = caption.map(super::plain_text::inlines_to_plain_text).filter(|c| !c.trim().is_empty());
    Some(MarkedMedia { marker, href, title: caption.unwrap_or_else(|| alt.clone()) })
}

/// Each pipe segment of an image, in either form, that is a mistyped display
/// word, as [`crate::media::display_word_warning`] words it (the text stays the
/// caption), and each width word (`![alt|wide|](x.png)`) dropped because the
/// image shares its paragraph with other content: only an image alone in its
/// paragraph becomes a figure that carries the word.
pub fn image_word_warnings(doc: &Document) -> Vec<String> {
    fn check(inlines: &[Inline], out: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                // A standard image's words count only after its first pipe.
                Inline::Image { alt, is_wikilink, .. } => out.extend(
                    alt.split('|').skip(usize::from(!is_wikilink)).filter_map(crate::media::display_word_warning),
                ),
                Inline::Link { children, .. } => check(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    super::visit::visit_blocks(doc, |block| {
        match block {
            Block::Paragraph(inlines) => {
                check(inlines, &mut out);
                let shown = |i: &&Inline| !matches!(i, Inline::LineBreak) && !matches!(i, Inline::Text(t) if t.trim().is_empty());
                if inlines.iter().filter(shown).count() > 1 {
                    for inline in inlines {
                        let Inline::Image { src, alt, is_wikilink, wikilink_pothole, .. } = inline else { continue };
                        // A wikilink's words are its whole pothole; a standard image's follow its alt.
                        let words = if *is_wikilink { wikilink_pothole.as_deref() } else { Some(alt.as_str()).filter(|a| a.contains('|')) };
                        let Some(word) = words.and_then(|w| crate::media::read_image_words(w).placement.width) else { continue };
                        let href = src.href();
                        out.push(format!("'{href}' has the width word '{word}', which is ignored because the image shares its paragraph with other content; put the image in its own paragraph"));
                    }
                }
            }
            Block::Figure { image, .. } => check(std::slice::from_ref(image), &mut out),
            _ => {}
        }
        true
    });
    out
}

/// Hrefs of the images in the document that a screen reader gets no text for:
/// an empty alt and, for a figure, no caption to stand in for it. Only image
/// files count; a video, audio or PDF written in the image form takes its
/// label from elsewhere. Descends into grid cells, callouts, lists and table
/// cells. A picture inside a link that has its own text is named by that text.
pub fn images_without_text(doc: &Document) -> Vec<String> {
    fn is_image_file(href: &str) -> bool {
        crate::resolve::ext_kind::reference_kind_for_ext(&crate::path_ext::path_extension_lower(href))
            == crate::resolve::ext_kind::ExtKind::Image
    }
    fn bare(image: &Inline, out: &mut Vec<String>) {
        if let Inline::Image { src, alt, .. } = image {
            let href = src.href();
            if alt.trim().is_empty() && is_image_file(href) {
                out.push(href.to_string());
            }
        }
    }
    fn walk(inlines: &[Inline], out: &mut Vec<String>) {
        for i in inlines {
            match i {
                Inline::Image { .. } => bare(i, out),
                Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) => walk(c, out),
                Inline::Link { children, .. } if super::inlines_to_plain_text(children).trim().is_empty() => {
                    walk(children, out)
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    super::visit::visit_blocks(doc, |block| {
        match block {
            Block::Paragraph(inlines) | Block::Heading { children: inlines, .. } => {
                walk(inlines, &mut out)
            }
            Block::Table { header, rows, .. } => {
                for cell in header.iter().chain(rows.iter().flatten()) {
                    walk(cell, &mut out);
                }
            }
            Block::Figure { image, caption, .. } if caption.as_ref().is_none_or(|c| c.is_empty()) => {
                bare(image, &mut out)
            }
            _ => {}
        }
        true
    });
    out
}

/// Whether the page body carries a raw `<style>` element. Its rules sit outside
/// the theme's cascade layers, so they beat every theme rule.
pub fn has_raw_style_block(doc: &Document) -> bool {
    let mut found = false;
    super::visit::visit_blocks(doc, |block| {
        if let Block::Other(html) = block {
            let lower = html.to_ascii_lowercase();
            found |= lower.match_indices("<style").any(|(at, tag)| {
                lower[at + tag.len()..].starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace())
            });
        }
        true
    });
    found
}

/// The source of the page's first content picture, or `None`.
///
/// A content picture is one the author put on the page to be looked at: a
/// figure, a picture standing alone in its paragraph, a picture in a grid
/// cell, the first item of a gallery. Searched depth-first in document order
/// through lists, quotes, callouts and shortcode bodies.
///
/// Not content pictures: an image set inside a sentence, heading or table
/// cell (an icon or an emoji), and an image wrapped in a link to another page
/// or to a page card (the page the link names owns that picture).
///
/// # Render order, not source order
///
/// The walk is depth-first over the source tree, but a footnote definition
/// does not render where it sits — `footnotes::render_section` hoists it to
/// the end of the page. Writing `[^a]: ![diagram](d.png)` directly under the
/// paragraph that cites it is the ordinary Obsidian habit, and a single-pass
/// source-order walk hands that diagram the cover over the photo the reader
/// actually meets in the body. The consumers are `body_cover_path` and
/// `body_cover_candidates`, i.e. the page's `og:image`, so the wrong answer ships as the social card.
///
/// Hence two passes: body first with definitions skipped; then the endnote
/// section, walked in INDEX order — the order `render_section` emits the
/// `<li>`s — not source order, which is not the order the reader meets the
/// notes. Endnote pictures come after every body picture, so an image that
/// lives *only* in a note is still found, without outranking the body.
pub fn find_first_block_image(doc: &Document) -> Option<&Url> {
    block_images(doc).into_iter().next()
}

/// Every content picture of the page (see [`find_first_block_image`] for what
/// counts), in the same order: body first, endnotes after. A gallery
/// contributes its first picture only.
pub fn block_images(doc: &Document) -> Vec<&Url> {
    // The renderer's identity decision, from the same lookup `is_hoisted`
    // uses — a first-sighting counter here diverged from it whenever a
    // label's first definition was NESTED inside another note (the pass-one
    // skip never descends, so the nested first was never counted and a
    // later top-level repeat passed for it).
    let hoisted = super::footnotes::hoisted_definition_bodies(&doc.blocks);
    let mut out = Vec::new();
    collect_images_in_blocks(&doc.blocks, Notes::Hoisted, &hoisted, &mut out);
    // The endnote section follows the body, its notes in first-reference
    // index order. Within one note's body the same identity rule applies — a
    // nested HOISTED definition's content is skipped here and collected at
    // its OWN entry in this iteration, while a nested repeat renders in place.
    let index = super::footnotes::FootnoteIndex::build(&doc.blocks);
    for (_, label) in index.entries() {
        if let Some(children) = super::footnotes::FootnoteIndex::definition(&doc.blocks, label) {
            collect_images_in_blocks(children, Notes::Hoisted, &hoisted, &mut out);
        }
    }
    out
}

/// Whether a footnote definition reached on this walk renders where it sits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Notes {
    /// Hoisted to the endnote section, so its images are endnote matter and
    /// lose to any body image however far down the page it sits.
    Hoisted,
    /// Rendered in place. True inside a shortcode body, which the endnote
    /// section does not reach — the same asymmetry `collect_heading_id_slots`
    /// encodes as `HoistScope` in `parser.rs`.
    InPlace,
}

fn collect_images_in_blocks<'a>(
    blocks: &'a [Block],
    notes: Notes,
    hoisted: &HashMap<String, usize>,
    out: &mut Vec<&'a Url>,
) {
    for block in blocks {
        collect_images_in_block(block, notes, hoisted, out);
    }
}

fn collect_images_in_block<'a>(
    block: &'a Block,
    notes: Notes,
    hoisted: &HashMap<String, usize>,
    out: &mut Vec<&'a Url>,
) {
    match block {
        Block::Figure { image: Inline::Image { src, .. }, .. } => out.push(src),
        Block::Figure { .. } => {}

        // An image-only paragraph is a figure that implicit-figure promotion
        // did not wrap; one with prose around the image is an icon or emoji.
        Block::Paragraph(inlines) if only_pictures(inlines) => collect_images_in_inlines(inlines, out),
        Block::Paragraph(_) => {}

        Block::List { items, .. } => {
            for item in items {
                collect_images_in_blocks(item, notes, hoisted, out);
            }
        }
        Block::BlockQuote(children)
        | Block::DefinitionList(children)
        | Block::DefinitionDefinition(children)
        | Block::Callout { children, .. } => collect_images_in_blocks(children, notes, hoisted, out),
        Block::DefinitionTitle(_) | Block::Table { .. } => {}

        // A definition inside a shortcode body is NOT hoisted — the endnote
        // section does not reach in there — so its images are body matter.
        Block::Shortcode(sc) => collect_images_in_shortcode(sc, Notes::InPlace, hoisted, out),

        // A card is a link to another page; the picture is that page's.
        Block::LinkCard { .. } | Block::Heading { .. } => {}

        // Only the doc-order-FIRST definition of a label is hoisted; a
        // REPEAT renders where it sits, so its images are body matter. Decided
        // by the same identity `render.rs` asks (`hoisted_definition_bodies`);
        // everything inside a HOISTED definition's subtree renders in the
        // endnote section, so the walk skips it here and `block_images`
        // visits the notes in index order afterwards.
        Block::FootnoteDefinition { label, children } => match notes {
            Notes::Hoisted
                if hoisted.get(label.as_str()) == Some(&(children.as_ptr() as usize)) => {}
            _ => collect_images_in_blocks(children, notes, hoisted, out),
        },

        Block::CodeBlock { .. } | Block::ThematicBreak | Block::Other(_) => {}
    }
}

fn collect_images_in_inlines<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Url>) {
    for inline in inlines {
        match inline {
            Inline::Image { src, .. } => out.push(src),
            Inline::Link { url: Url::Resolved(r), .. }
                if matches!(r.kind, UrlKind::Internal | UrlKind::Wikilink) => {}
            Inline::Link { children, .. }
            | Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => collect_images_in_inlines(children, out),
            _ => {}
        }
    }
}

/// Nothing in the paragraph but pictures: no words for them to sit among.
fn only_pictures(inlines: &[Inline]) -> bool {
    inlines.iter().all(|inline| match inline {
        Inline::Image { .. } | Inline::LineBreak => true,
        Inline::Text(t) => t.trim().is_empty(),
        Inline::Link { children, .. } | Inline::Emphasis(children) | Inline::Strong(children) => {
            only_pictures(children)
        }
        _ => false,
    })
}

fn collect_images_in_shortcode<'a>(
    sc: &'a Shortcode,
    notes: Notes,
    hoisted: &HashMap<String, usize>,
    out: &mut Vec<&'a Url>,
) {
    match sc {
        Shortcode::Subscribe(_) | Shortcode::Buttons(_) | Shortcode::Recent(_) => {}
        Shortcode::Gallery(args) => out.extend(args.items.first().map(|item| &item.src)),
        // The hero image itself is a `Url` on the shortcode, not an
        // `Inline::Image`: the cover chain reads `hero_image_url` for it.
        Shortcode::Hero(args) => collect_images_in_blocks(&args.overlay, notes, hoisted, out),
        Shortcode::Grid(args) => {
            for cell in &args.cells {
                collect_images_in_blocks(cell, notes, hoisted, out);
            }
        }
    }
}

/// True if the page may hold a picture the full-screen viewer opens: a
/// figure, a gallery, a paragraph's image, or raw HTML with an `<img`.
///
/// A deliberate superset. `fullscreen.js` owns the exact rules (alt text,
/// links, heroes, `no-viewer`), so the build only decides whether the page
/// carries the viewer's markup and the site its script; a false positive
/// costs a few hundred bytes, a false negative a picture that cannot open.
pub fn has_viewable_picture_recursive(doc: &Document) -> bool {
    let mut found = false;
    visit_blocks(doc, |block| {
        found = match block {
            Block::Figure { .. } | Block::Shortcode(Shortcode::Gallery(_)) => true,
            Block::Paragraph(inlines) => inlines.iter().any(|i| match i {
                Inline::Image { .. } => true,
                Inline::Other(html) => has_img_tag(html),
                _ => false,
            }),
            Block::Other(html) => has_img_tag(html),
            _ => false,
        };
        !found
    });
    found
}

fn has_img_tag(html: &str) -> bool {
    html.to_ascii_lowercase().contains("<img")
}

#[cfg(test)]
mod tests {
    use super::super::node::Inline;
    use super::super::shortcode::{GridShortcode, HeroShortcode};
    use super::super::url::{Url, UrlKind};
    use super::*;

    fn img(src: &str) -> Inline {
        Inline::Image {
            src: Url::resolved(src, UrlKind::Asset),
            alt: String::new(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        }
    }

    fn img_block(src: &str) -> Block {
        Block::Figure {
            image: img(src),
            caption: None,
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: None,
            italic_caption: false,
            link: None,
        }
    }

    fn p_with_text(text: &str) -> Block {
        Block::Paragraph(vec![Inline::Text(text.into())])
    }

    /// A footnote definition is hoisted to the endnote section, so the image
    /// inside one is the LAST thing on the rendered page — it must never win
    /// the cover over an image the reader actually meets in the body. The
    /// definition here sits above the body image in source, which is the
    /// ordinary Obsidian habit (`[^a]: …` written under the citing paragraph)
    /// and the only arrangement where source order and render order disagree.
    ///
    /// The consumers are `body_cover_path` and `body_cover_candidates`, i.e. the
    /// page `og:image`, so getting this wrong ships an endnote diagram as the article's social card.
    #[test]
    fn a_body_image_outranks_one_hoisted_into_the_endnotes() {
        let doc = super::super::parse(
            "Intro paragraph[^a].\n\n[^a]: ![diagram](d.png)\n\nLater section.\n\n![photo](p.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(src) => assert_eq!(
                src,
                &Url::unresolved("p.png"),
                "the endnote's image won the cover over the body's"
            ),
            other => panic!("expected an image, got {other:?}"),
        }
    }

    /// Only the FIRST definition of a label is hoisted — `render.rs` guards its
    /// arm with `footnotes::is_hoisted`, decided by identity. A repeated
    /// label's SECOND definition therefore renders where the author wrote it,
    /// in the body, so its image is body matter and must win the cover over the
    /// first definition's, which the reader only meets in the endnotes.
    ///
    /// The shape is a copy-pasted paragraph that brought its own note along.
    #[test]
    fn a_repeated_labels_second_definition_is_body_matter_not_endnote_matter() {
        let doc = super::super::parse(
            "Para one[^1].\n\n[^1]: ![chart](c1.png)\n\nPara two[^1].\n\n[^1]: ![chart2](c2.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(src) => assert_eq!(
                src,
                &Url::unresolved("c2.png"),
                "picked the hoisted first definition's image over the one that \
                 actually renders in the body"
            ),
            other => panic!("expected an image, got {other:?}"),
        }
    }

    /// The nested-first shape: `[^b]`'s FIRST definition sits inside `[^a]`'s
    /// body, so the later top-level `[^b]` is a repeat and renders in the
    /// body. A first-sighting counter never counted the nested first (the
    /// pass-one skip does not descend), so the repeat passed for it and its
    /// image — the page's only BODY image — lost the cover to an
    /// endnote-only one. Identity does not depend on what the walk skipped.
    #[test]
    fn a_repeat_whose_first_definition_is_nested_is_still_body_matter() {
        let doc = super::super::parse(
            "Body[^a][^b].\n\n[^a]: OUT\n\n    > [^b]: ![in](in.png)\n\n[^b]: ![repeat](r.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(src) => assert_eq!(
                src,
                &Url::unresolved("r.png"),
                "picked an endnote-only image over the repeat's body image"
            ),
            other => panic!("expected the repeat's body image, got {other:?}"),
        }
    }

    /// When every image lives in the endnotes, "first" means first in the
    /// ENDNOTE LIST — index order, the order `render_section` emits — not
    /// first in source order. `[^b]` is referenced first, so its note is
    /// fn-1 and its image is the first the reader meets, even though
    /// `[^a]`'s definition is written first.
    #[test]
    fn with_no_body_image_the_first_endnote_image_wins_not_the_first_written() {
        let doc =
            super::super::parse("Body[^b][^a].\n\n[^a]: ![a](a.png)\n\n[^b]: ![b](b.png)\n");
        match find_first_block_image(&doc) {
            Some(src) => assert_eq!(
                src,
                &Url::unresolved("b.png"),
                "picked the first-written definition's image over the first-rendered note's"
            ),
            other => panic!("expected an image, got {other:?}"),
        }
    }

    /// The other side of the same rule: the arm exists because an image that
    /// lives ONLY in a note is still the page's only image. Skipping notes
    /// outright would regress this, so the fix is two passes, not one filter.
    #[test]
    fn an_image_that_lives_only_in_a_note_is_still_found() {
        let doc = super::super::parse("Intro[^a].\n\n[^a]: ![diagram](d.png)\n");
        match find_first_block_image(&doc) {
            Some(src) => assert_eq!(src, &Url::unresolved("d.png")),
            other => panic!("expected the note's image, got {other:?}"),
        }
    }

    #[test]
    fn returns_none_on_empty_doc() {
        let doc = Document::new();
        assert!(find_first_block_image(&doc).is_none());
    }

    #[test]
    fn returns_none_when_no_image_anywhere() {
        let doc = Document::from_blocks(vec![
            p_with_text("plain prose"),
            Block::Heading {
                level: 2,
                children: vec![Inline::Text("Title".into())],
                id: None,
                contents_end: false,
                contents_label: None,
            },
        ]);
        assert!(find_first_block_image(&doc).is_none());
    }

    #[test]
    fn finds_image_inside_figure() {
        let doc = Document::from_blocks(vec![img_block("photo.jpg")]);
        match find_first_block_image(&doc) {
            Some(src) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "photo.jpg");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn a_picture_alone_in_its_paragraph_is_found_and_one_among_words_is_not() {
        let alone = Document::from_blocks(vec![Block::Paragraph(vec![img("alone.png")])]);
        assert_eq!(find_first_block_image(&alone), Some(&Url::resolved("alone.png", UrlKind::Asset)));

        let among_words = Document::from_blocks(vec![Block::Paragraph(vec![
            Inline::Text("press ".into()),
            img("icon.png"),
            Inline::Text(" to save".into()),
        ])]);
        assert_eq!(find_first_block_image(&among_words), None);
    }

    #[test]
    fn a_heading_or_table_picture_is_not_a_content_picture() {
        let heading = Document::from_blocks(vec![Block::Heading {
            level: 2,
            children: vec![img("badge.png"), Inline::Text("Title".into())],
            id: None,
            contents_end: false,
            contents_label: None,
        }]);
        assert_eq!(find_first_block_image(&heading), None);
        let table = super::super::parse("| a |\n|---|\n| ![](cell.png) |\n");
        assert_eq!(find_first_block_image(&table), None);
    }

    #[test]
    fn a_picture_linking_to_a_page_is_that_pages_and_one_linking_to_a_file_is_ours() {
        let link = |kind| {
            Block::Paragraph(vec![Inline::Link {
                url: Url::resolved("target", kind),
                title: None,
                children: vec![img("thumb.png")],
                is_wikilink: false,
                has_pothole: false,
            }])
        };
        for kind in [UrlKind::Internal, UrlKind::Wikilink] {
            assert_eq!(find_first_block_image(&Document::from_blocks(vec![link(kind)])), None, "{kind:?}");
        }
        for kind in [UrlKind::Asset, UrlKind::AssetNewtab, UrlKind::External] {
            assert!(find_first_block_image(&Document::from_blocks(vec![link(kind)])).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn the_first_gallery_item_is_a_content_picture() {
        let doc = super::super::parse(":::gallery\n![A](one.jpg)\n![B](two.jpg)\n:::\n");
        assert_eq!(find_first_block_image(&doc), Some(&Url::unresolved("one.jpg")));
    }

    #[test]
    fn finds_image_inside_emphasis_or_link() {
        // Inline images can appear nested inside emphasis/strong/link
        // children. The walker must descend.
        let doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Emphasis(vec![img(
            "nested.png",
        )])])]);
        match find_first_block_image(&doc) {
            Some(src) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "nested.png");
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn picks_first_image_in_document_order() {
        let doc = Document::from_blocks(vec![
            p_with_text("intro"),
            img_block("first.jpg"),
            img_block("second.jpg"),
        ]);
        match find_first_block_image(&doc) {
            Some(src) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "first.jpg");
            }
            _ => panic!("expected first.jpg"),
        }
    }

    #[test]
    fn finds_image_inside_grid_cell() {
        // PR4.5: Grid cells are typed Vec<Vec<Block>>. The walker must
        // descend into the inner block sequences.
        let cell = vec![img_block("grid.png")];
        let doc = Document::from_blocks(vec![Block::Shortcode(Shortcode::Grid(GridShortcode {
            source: Default::default(),
            columns: 1,
            ratio: None,
            classes: String::new(),
            cells: vec![cell],
            width: None,
            scroll: false,
            collage: false,
            equal_rows: false,
            mid_columns: None,
            label: None,
        }))]);
        match find_first_block_image(&doc) {
            Some(src) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "grid.png");
            }
            _ => panic!("expected grid.png"),
        }
    }

    #[test]
    fn finds_image_inside_blockquote() {
        let doc = Document::from_blocks(vec![Block::BlockQuote(vec![img_block("quoted.jpg")])]);
        assert!(find_first_block_image(&doc).is_some());
    }

    #[test]
    fn finds_image_inside_list_item() {
        let doc = Document::from_blocks(vec![Block::List {
            ordered: false,
            start: None,
            items: vec![vec![img_block("list.png")]],
            item_source_lines: vec![],
        }]);
        assert!(find_first_block_image(&doc).is_some());
    }

    #[test]
    fn hero_overlay_image_is_findable() {
        // Hero overlay may carry images. Until extract_hero runs, the
        // body walker still surfaces them (matches today's body_cover
        // behavior where the hero is part of the body until hoisted).
        let overlay = vec![img_block("overlay.jpg")];
        let doc = Document::from_blocks(vec![Block::Shortcode(Shortcode::Hero(HeroShortcode {
            source: Default::default(),
            image: Some(Url::resolved("hero.jpg", UrlKind::Asset)),
            image_alt: String::new(),
            extra_images: Vec::new(),
            attrs: String::new(),
            classes: String::new(),
            overlay,
            overlay_text: String::new(),
            width: None,
            mobile: None,
            panel: None,
            fade: None,
            max_height: None,
            align: None,
            caption: Vec::new(),
            slide_captions: Vec::new(),
            header: None,
            from: None,
            dark: None,
        }))]);
        match find_first_block_image(&doc) {
            Some(src) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "overlay.jpg");
            }
            _ => panic!("expected overlay image"),
        }
    }

    fn marked(src: &str, marker: &str) -> Option<MarkedMedia> {
        let blocks = vec![Block::Paragraph(vec![img(src)]), Block::Other(marker.to_string())];
        marked_media_at(&blocks, 1)
    }

    #[test]
    fn a_photography_marker_collects_an_image_file_only() {
        assert_eq!(marked("sunset.jpg", "<!-- photography -->").map(|m| m.marker), Some(MediaMarker::Photography));
        assert_eq!(marked("clip.mp4", "<!-- photography -->"), None, "a video is not a photo");
    }

    #[test]
    fn a_video_marker_collects_a_video_file_only() {
        let video = marked("clip.mp4", "<!-- video-meta: walks -->").map(|m| m.marker);
        assert_eq!(video, Some(MediaMarker::Video { collection: Some("walks".into()) }));
        assert_eq!(marked("sunset.jpg", "<!-- video-meta -->"), None, "a photo is not a video");
    }

    // ---- has_viewable_picture_recursive ----

    fn has_picture(markdown: &str) -> bool {
        has_viewable_picture_recursive(&crate::ast::parse(markdown))
    }

    #[test]
    fn any_figure_image_gallery_or_raw_img_marks_the_page() {
        assert!(has_picture("![Harbour](harbour.jpg)\n*Dusk*\n"));
        assert!(has_picture("![](divider.jpg)\n"));
        assert!(has_picture(":::grid 2\n![One](a.jpg)\n+++\n![Two](b.jpg)\n:::\n"));
        assert!(has_picture("> ![Harbour](harbour.jpg)\n"));
        assert!(has_picture(":::gallery\na.jpg\nb.jpg\n:::\n"));
        assert!(has_picture("<div><IMG src=\"a.jpg\"></div>\n"));
    }

    #[test]
    fn a_page_without_pictures_is_not_marked() {
        assert!(!has_picture("Words only, with a [link](page.md).\n"));
        assert!(!has_picture(":::hero {image=hero.jpg}\n# Title\n:::\n"));
        assert!(!has_picture("[![Harbour](harbour.jpg)](page.md)\n"));
    }
}
