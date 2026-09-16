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
//!
//! See `docs/reference/typed-body-ast.md` (principle: AST is data).

use super::document::Document;
use super::node::{Block, Inline};
use super::shortcode::Shortcode;
use std::collections::HashMap;

/// Find the first `Inline::Image` reachable from the document's top-level
/// block sequence.
///
/// Search order (depth-first, document order):
/// 1. `Block::Figure { image, .. }` — the image-only paragraph promoted
///    in PR3. Direct match.
/// 2. `Block::Paragraph(inlines)` — look for the first `Inline::Image`
///    appearing in the paragraph (descending into nested
///    Emphasis/Strong/Link children — matches today's
///    `transform_events::body_cover_path` capture behavior, which
///    extracts the first markdown-origin image regardless of inline
///    wrapping).
/// 3. `Block::Shortcode(_)` — descend into shortcode bodies (Grid cells,
///    Hero overlay, Gallery items) in document order. Production's
///    cover chain explicitly considers shortcode-borne images.
/// 4. `Block::List`, `Block::BlockQuote`, `Block::Callout`, `Block::Table`
///    — descend into their nested block sequences.
/// 5. `Block::LinkCard` — descend into children.
///
/// Returns `None` if no image is found anywhere reachable.
///
/// # Phase 4 PR7a (2026-05-28)
///
/// Replaces the `body_cover_path` capture currently in
/// `pipeline.rs::transform_events`. Closes the acceptance criteria of
/// issue #643 (first-body-image AST walker) on top of the typed AST.
/// # Render order, not source order (2026-07-29)
///
/// The walk is depth-first over the source tree, but a footnote definition
/// does not render where it sits — `footnotes::render_section` hoists it to
/// the end of the page. Writing `[^a]: ![diagram](d.png)` directly under the
/// paragraph that cites it is the ordinary Obsidian habit, and a single-pass
/// source-order walk hands that diagram the cover over the photo the reader
/// actually meets in the body. The consumer is `body_cover_path`, i.e. the
/// page's `og:image`, so the wrong answer ships as the social card.
///
/// Hence two passes: body first with definitions skipped; then the endnote
/// section, walked in INDEX order — the order `render_section` emits the
/// `<li>`s — not source order, which is not the order the reader meets the
/// notes. The second pass only ever runs when the body has no image at all,
/// so it answers the case the definition arm below exists for — an image
/// that lives *only* in a note is still the only image — without letting an
/// endnote outrank the body.
pub fn find_first_block_image(doc: &Document) -> Option<&Inline> {
    // The renderer's identity decision, from the same lookup `is_hoisted`
    // uses — a first-sighting counter here diverged from it whenever a
    // label's first definition was NESTED inside another note (the pass-one
    // skip never descends, so the nested first was never counted and a
    // later top-level repeat passed for it).
    let hoisted = super::footnotes::hoisted_definition_bodies(&doc.blocks);
    find_first_image_in_blocks(&doc.blocks, Notes::Hoisted, &hoisted).or_else(|| {
        // No body image: the cover falls to the endnote section, whose
        // notes render in first-reference index order. Within one note's
        // body the same identity rule applies — a nested HOISTED
        // definition's content is skipped here and searched separately at
        // its OWN entry in this same iteration (which can land before or
        // after the host's, depending on first-reference order), while a
        // nested repeat renders in place.
        let index = super::footnotes::FootnoteIndex::build(&doc.blocks);
        index.entries().iter().find_map(|(_, label)| {
            super::footnotes::FootnoteIndex::definition(&doc.blocks, label).and_then(
                |children| find_first_image_in_blocks(children, Notes::Hoisted, &hoisted),
            )
        })
    })
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

fn find_first_image_in_blocks<'a>(
    blocks: &'a [Block],
    notes: Notes,
    hoisted: &HashMap<String, usize>,
) -> Option<&'a Inline> {
    for block in blocks {
        if let Some(img) = find_first_image_in_block(block, notes, hoisted) {
            return Some(img);
        }
    }
    None
}

fn find_first_image_in_block<'a>(
    block: &'a Block,
    notes: Notes,
    hoisted: &HashMap<String, usize>,
) -> Option<&'a Inline> {
    match block {
        // Direct hit: PR3's typed Figure variant.
        Block::Figure { image, .. } => Some(image),

        // Paragraphs may contain inline images (possibly nested inside
        // emphasis/strong/link children — the inline walker descends).
        Block::Paragraph(inlines) => find_first_image_in_inlines(inlines),

        // List items, blockquotes, callouts, table cells: descend into
        // their nested block sequences. Same depth-first document-order
        // walk as the top-level loop.
        Block::List { items, .. } => {
            for item in items {
                if let Some(img) = find_first_image_in_blocks(item, notes, hoisted) {
                    return Some(img);
                }
            }
            None
        }
        Block::BlockQuote(children) => find_first_image_in_blocks(children, notes, hoisted),
        Block::Callout { children, .. } => find_first_image_in_blocks(children, notes, hoisted),
        Block::Table { header, rows, .. } => {
            // Headers walked first (document order), then rows.
            for cell in header {
                if let Some(img) = find_first_image_in_inlines(cell) {
                    return Some(img);
                }
            }
            for row in rows {
                for cell in row {
                    if let Some(img) = find_first_image_in_inlines(cell) {
                        return Some(img);
                    }
                }
            }
            None
        }

        // Shortcodes: descend by variant. Gallery/Hero/Grid carry images.
        // A definition inside a shortcode body is NOT hoisted — the endnote
        // section does not reach in there — so its images are body matter.
        Block::Shortcode(sc) => find_first_image_in_shortcode(sc, Notes::InPlace, hoisted),

        // Compound-link grid cells (PR4.5 LinkCard) wrap block-level
        // children including images.
        Block::LinkCard { children, .. } => find_first_image_in_blocks(children, notes, hoisted),

        Block::Heading { children, .. } => find_first_image_in_inlines(children),

        // Only the doc-order-FIRST definition of a label is hoisted; a
        // REPEAT renders where it sits, so a repeat's images are body matter
        // and must be eligible on pass one. Decided by the same identity
        // `render.rs` asks (`hoisted_definition_bodies`) — everything inside
        // a HOISTED definition's subtree renders in the endnote section
        // (in place within the host's endnote, or hoisted to its own), so
        // the walk skips the whole subtree without descending. Pass two
        // never reaches a top-level definition through this arm (it walks
        // note bodies directly, in index order — see
        // `find_first_block_image`); a definition met here mid-body is
        // either a repeat (descend: it renders right here) or, inside a
        // note body on pass two, a nested hoisted definition whose content
        // belongs to its own, later endnote (skip).
        Block::FootnoteDefinition { label, children } => match notes {
            Notes::Hoisted
                if hoisted.get(label.as_str()) == Some(&(children.as_ptr() as usize)) =>
            {
                None
            }
            _ => find_first_image_in_blocks(children, notes, hoisted),
        },

        Block::CodeBlock { .. } | Block::ThematicBreak | Block::Other(_) => None,
    }
}

fn find_first_image_in_inlines(inlines: &[Inline]) -> Option<&Inline> {
    for inline in inlines {
        match inline {
            Inline::Image { .. } => return Some(inline),
            Inline::Link { children, .. }
            | Inline::Emphasis(children)
            | Inline::Strong(children)
            | Inline::Strikethrough(children) => {
                if let Some(img) = find_first_image_in_inlines(children) {
                    return Some(img);
                }
            }
            Inline::Text(_)
            | Inline::Code(_)
            | Inline::LineBreak
            | Inline::FootnoteRef(_)
            | Inline::TaskMarker(_)
            | Inline::Other(_) => {}
        }
    }
    None
}

fn find_first_image_in_shortcode<'a>(
    sc: &'a Shortcode,
    notes: Notes,
    hoisted: &HashMap<String, usize>,
) -> Option<&'a Inline> {
    match sc {
        Shortcode::Subscribe(_) | Shortcode::Buttons(_) | Shortcode::Recent(_) | Shortcode::Apply(_) => None,
        Shortcode::Gallery(args) => {
            // Gallery items carry image refs but not as Inline::Image —
            // they're typed as GalleryItem { src, alt, attrs }. The
            // cover chain currently consumes hero/figure/paragraph
            // images; gallery images are not part of body_cover_path
            // today. Skip.
            //
            // If a future Phase makes Gallery items consumable here,
            // the right move is to add a synthetic Inline::Image; for
            // now matching today's transform_events behavior is the
            // gate.
            let _ = args;
            None
        }
        Shortcode::Hero(args) => {
            // Hero overlay walked first; the hero image itself is not
            // surfaced as an Inline::Image (it's a `Url` on the
            // HeroShortcode). The cover chain reads
            // `hero_image_url` directly from `apply_typed_shortcodes`
            // / extract_hero — that field is the source of truth for
            // hero-rung; body_cover_path explicitly excludes it.
            find_first_image_in_blocks(&args.overlay, notes, hoisted)
        }
        Shortcode::Grid(args) => {
            // Walk each grid cell's blocks in document order.
            for cell in &args.cells {
                if let Some(img) = find_first_image_in_blocks(cell, notes, hoisted) {
                    return Some(img);
                }
            }
            None
        }
    }
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
    /// The consumer is `body_cover_path`, i.e. the page `og:image`, so getting
    /// this wrong ships an endnote diagram as the article's social card.
    #[test]
    fn a_body_image_outranks_one_hoisted_into_the_endnotes() {
        let doc = super::super::parse(
            "Intro paragraph[^a].\n\n[^a]: ![diagram](d.png)\n\nLater section.\n\n![photo](p.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => assert_eq!(
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
            "Para one[^1].\n\n[^1]: see ![chart](c1.png)\n\nPara two[^1].\n\n[^1]: see ![chart2](c2.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => assert_eq!(
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
            "Body[^a][^b].\n\n[^a]: OUT\n\n    > [^b]: IN ![in](in.png)\n\n[^b]: REPEAT ![repeat](r.png)\n",
        );
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => assert_eq!(
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
            Some(Inline::Image { src, .. }) => assert_eq!(
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
            Some(Inline::Image { src, .. }) => assert_eq!(src, &Url::unresolved("d.png")),
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
            },
        ]);
        assert!(find_first_block_image(&doc).is_none());
    }

    #[test]
    fn finds_image_inside_figure() {
        let doc = Document::from_blocks(vec![img_block("photo.jpg")]);
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "photo.jpg");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn finds_image_inside_paragraph() {
        let doc = Document::from_blocks(vec![Block::Paragraph(vec![
            Inline::Text("see ".into()),
            img("inline.png"),
        ])]);
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "inline.png");
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn finds_image_inside_emphasis_or_link() {
        // Inline images can appear nested inside emphasis/strong/link
        // children. The walker must descend.
        let doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Emphasis(vec![img(
            "nested.png",
        )])])]);
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => {
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
            Some(Inline::Image { src, .. }) => {
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
            columns: 1,
            ratio: None,
            classes: String::new(),
            cells: vec![cell],
            width: None,
        }))]);
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => {
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
            image: Some(Url::resolved("hero.jpg", UrlKind::Asset)),
            extra_images: Vec::new(),
            attrs: String::new(),
            classes: String::new(),
            overlay,
            overlay_text: String::new(),
            width: None,
            mobile: None,
            caption: String::new(),
        }))]);
        match find_first_block_image(&doc) {
            Some(Inline::Image { src, .. }) => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "overlay.jpg");
            }
            _ => panic!("expected overlay image"),
        }
    }
}
