//! Tests for `[![[embed]]](/url)` — a wikilink embed inside a markdown link.
//!
//! The load-bearing assertion is `matches_markdown_image_spelling`: the whole
//! point of the pass is that the wikilink spelling and the CommonMark spelling
//! land on the SAME typed shape, so the editor and the build can agree.

use super::super::node::{Block, Inline};
use super::super::parser::{parse, parse_with_config, ParseConfig};
use super::super::url::Url;

/// The single inline of a one-paragraph document.
fn only_inline(md: &str) -> Inline {
    let doc = parse(md);
    assert_eq!(doc.blocks.len(), 1, "expected one block: {:?}", doc.blocks);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert_eq!(inlines.len(), 1, "expected one inline: {inlines:?}");
            inlines[0].clone()
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn linked_embed_is_a_link_wrapping_an_image() {
    match only_inline("[![[card.png]]](/awards/x/)") {
        Inline::Link {
            url,
            children,
            is_wikilink,
            ..
        } => {
            assert_eq!(url, Url::unresolved("/awards/x/"));
            assert!(!is_wikilink, "the outer link is an ordinary markdown link");
            match children.as_slice() {
                [Inline::Image {
                    src,
                    alt,
                    is_wikilink,
                    ..
                }] => {
                    assert_eq!(src, &Url::unresolved("card.png"), "src is the EMBED, not the link");
                    assert_eq!(alt, "");
                    assert!(is_wikilink, "the inner image keeps wikilink resolution");
                }
                other => panic!("expected one image child, got {other:?}"),
            }
        }
        other => panic!("expected a link, got {other:?}"),
    }
}

#[test]
fn matches_markdown_image_spelling() {
    // Same shape, modulo alt text and the wikilink resolution flag on the
    // image — everything downstream (URL resolution, asset registry, render)
    // keys off this structure.
    let wiki = only_inline("[![[card.png]]](/awards/x/)");
    let commonmark = only_inline("[![](card.png)](/awards/x/)");
    match (&wiki, &commonmark) {
        (
            Inline::Link {
                url: wu,
                children: wc,
                ..
            },
            Inline::Link {
                url: cu,
                children: cc,
                ..
            },
        ) => {
            assert_eq!(wu, cu);
            match (wc.as_slice(), cc.as_slice()) {
                ([Inline::Image { src: ws, alt: wa, .. }], [Inline::Image { src: cs, alt: ca, .. }]) => {
                    assert_eq!(ws, cs);
                    assert_eq!(wa, ca);
                }
                other => panic!("expected image children, got {other:?}"),
            }
        }
        other => panic!("expected two links, got {other:?}"),
    }
}

#[test]
fn embed_matches_the_same_embed_standing_alone() {
    // The image node must be byte-identical to what the standalone embed
    // parses to — that is what "one code path" means here.
    // `width=400` is a typed param, so this also pins that the pothole
    // survives and is NOT mistaken for a caption.
    let standalone = match parse_with_config(
        "![[card.png|width=400]]",
        &ParseConfig {
            implicit_figure: false,
            ..ParseConfig::default()
        },
    )
    .blocks
    .remove(0)
    {
        Block::Paragraph(mut inlines) => inlines.remove(0),
        other => panic!("expected paragraph, got {other:?}"),
    };
    match only_inline("[![[card.png|width=400]]](/awards/x/)") {
        Inline::Link { children, .. } => assert_eq!(children, vec![standalone]),
        other => panic!("expected a link, got {other:?}"),
    }
}

#[test]
fn a_pothole_alias_is_alt_text() {
    // The width/params half of pothole classification is covered by
    // `embed_matches_the_same_embed_standing_alone`; this pins the other
    // branch, where the pothole IS a caption.
    match only_inline("[![[card.png|Award card]]](/x/)") {
        Inline::Link { children, .. } => match children.as_slice() {
            [Inline::Image { alt, .. }] => assert_eq!(alt, "Award card"),
            other => panic!("expected one image child, got {other:?}"),
        },
        other => panic!("expected a link, got {other:?}"),
    }
}

#[test]
fn works_in_prose_headings_lists_and_tables() {
    // "EVERYWHERE" is the requirement: not just the `:::grid` cell that the
    // `detect_compound_link` special case used to be the only cure for.
    fn has_linked_image(inlines: &[Inline]) -> bool {
        inlines.iter().any(|i| match i {
            Inline::Link { children, .. } => {
                matches!(children.as_slice(), [Inline::Image { .. }])
            }
            Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) => {
                has_linked_image(c)
            }
            _ => false,
        })
    }

    let doc = parse("before [![[a.png]]](/x/) after\n");
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(has_linked_image(inlines), "{inlines:?}");
            assert!(
                inlines.iter().any(|i| matches!(i, Inline::Text(t) if t == "before ")),
                "surrounding prose is preserved: {inlines:?}"
            );
            assert!(
                inlines.iter().any(|i| matches!(i, Inline::Text(t) if t == " after")),
                "surrounding prose is preserved: {inlines:?}"
            );
        }
        other => panic!("expected paragraph, got {other:?}"),
    }

    match &parse("## [![[a.png]]](/x/)\n").blocks[0] {
        Block::Heading { children, .. } => assert!(has_linked_image(children), "{children:?}"),
        other => panic!("expected heading, got {other:?}"),
    }

    match &parse("- [![[a.png]]](/x/)\n").blocks[0] {
        Block::List { items, .. } => match &items[0][0] {
            Block::Paragraph(inlines) => assert!(has_linked_image(inlines), "{inlines:?}"),
            other => panic!("expected paragraph, got {other:?}"),
        },
        other => panic!("expected list, got {other:?}"),
    }

    // BOTH table halves. `header` is a `Vec<Vec<Inline>>` field of its own, so
    // a walk that matches `Block::Table { rows, .. }` silently skips it — this
    // test used to assert `rows` only and its name promised more than it
    // checked (the header cell lost the author's `![[a.png]]` entirely and
    // published a U+E000 as link text).
    match &parse("| [![[hdr.png]]](/h/) | b |\n|---|---|\n| [![[a.png]]](/x/) | d |\n").blocks[0] {
        Block::Table { header, rows, .. } => {
            assert!(has_linked_image(&header[0]), "header cell: {header:?}");
            assert!(has_linked_image(&rows[0][0]), "body cell: {rows:?}");
        }
        other => panic!("expected table, got {other:?}"),
    }

    match &parse("> [![[a.png]]](/x/)\n").blocks[0] {
        Block::BlockQuote(children) => match &children[0] {
            Block::Paragraph(inlines) => assert!(has_linked_image(inlines), "{inlines:?}"),
            other => panic!("expected paragraph, got {other:?}"),
        },
        other => panic!("expected blockquote, got {other:?}"),
    }
}

#[test]
fn two_embeds_in_one_paragraph_each_get_their_own() {
    let doc = parse("[![[a.png]]](/a/) and [![[b.png]]](/b/)\n");
    let srcs: Vec<String> = match &doc.blocks[0] {
        Block::Paragraph(inlines) => inlines
            .iter()
            .filter_map(|i| match i {
                Inline::Link { children, .. } => match children.as_slice() {
                    [Inline::Image { src, .. }] => Some(format!("{src:?}")),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        other => panic!("expected paragraph, got {other:?}"),
    };
    assert_eq!(
        srcs,
        vec![
            format!("{:?}", Url::unresolved("a.png")),
            format!("{:?}", Url::unresolved("b.png")),
        ]
    );
}

// ---- shapes that must NOT be claimed ------------------------------------

/// Render-free structural probe: does the document contain a link wrapping an
/// image anywhere?
fn contains_linked_image(blocks: &[Block]) -> bool {
    fn in_inlines(inlines: &[Inline]) -> bool {
        inlines.iter().any(|i| match i {
            Inline::Link { children, .. } => {
                matches!(children.as_slice(), [Inline::Image { .. }]) || in_inlines(children)
            }
            Inline::Emphasis(c) | Inline::Strong(c) | Inline::Strikethrough(c) => in_inlines(c),
            _ => false,
        })
    }
    blocks.iter().any(|b| match b {
        Block::Paragraph(inlines) | Block::Heading { children: inlines, .. } => in_inlines(inlines),
        Block::BlockQuote(children) | Block::Callout { children, .. } => {
            contains_linked_image(children)
        }
        _ => false,
    })
}

/// True when a substitution sentinel survived anywhere in `blocks`.
///
/// `{:?}` renders a private-use codepoint as the nine ASCII characters
/// `\u{e000}`, so `dump.contains('\u{e000}')` on a Debug dump is vacuously
/// false — it never sees the real char and never fails. (It didn't: the first
/// version of `a_sentinel_never_reaches_a_url_or_a_title` passed with the fix
/// reverted.) Check both spellings.
fn sentinel_leaks(blocks: &[Block]) -> bool {
    let dump = format!("{blocks:?}");
    dump.contains('\u{e000}') || dump.contains("\\u{e000}")
}

#[test]
fn escaped_bang_stays_literal() {
    // `\![[x]]` is pulldown's escape: a literal `!` plus a wikilink LINK.
    let doc = parse("[\\![[card.png]]](/x/)\n");
    assert!(!contains_linked_image(&doc.blocks), "{:?}", doc.blocks);
}

#[test]
fn no_url_is_not_a_link() {
    // `[![[x.png]]]` has no destination — pulldown already parses the embed
    // correctly there, wrapped in literal brackets. Leave it alone.
    let doc = parse("[![[card.png]]]\n");
    assert!(!contains_linked_image(&doc.blocks), "{:?}", doc.blocks);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => assert!(
            inlines
                .iter()
                .any(|i| matches!(i, Inline::Image { is_wikilink: true, .. })),
            "{inlines:?}"
        ),
        other => panic!("expected paragraph, got {other:?}"),
    }
}

#[test]
fn empty_target_does_not_panic_and_is_not_a_link() {
    let doc = parse("[![[]]](/x/)\n");
    assert!(!contains_linked_image(&doc.blocks), "{:?}", doc.blocks);
}

#[test]
fn inert_regions_are_untouched() {
    // Inline code span.
    let doc = parse("`[![[card.png]]](/x/)`\n");
    assert!(!contains_linked_image(&doc.blocks), "{:?}", doc.blocks);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => match inlines.as_slice() {
            [Inline::Code(code)] => assert_eq!(code, "[![[card.png]]](/x/)"),
            other => panic!("expected one code span, got {other:?}"),
        },
        other => panic!("expected paragraph, got {other:?}"),
    }

    // Fenced code block: the author's bytes survive verbatim.
    let doc = parse("```\n[![[card.png]]](/x/)\n```\n");
    match &doc.blocks[0] {
        Block::CodeBlock { value, .. } => assert_eq!(value.trim(), "[![[card.png]]](/x/)"),
        other => panic!("expected code block, got {other:?}"),
    }
}

#[test]
fn an_outer_bang_keeps_the_shape_an_image_alt() {
    // `![![[x]]](/u)` is an image whose alt text happens to look like an
    // embed. Claiming it would put an `Inline::Image` inside an alt string.
    let doc = parse("![![[card.png]]](/x/)\n");
    assert!(!contains_linked_image(&doc.blocks), "{:?}", doc.blocks);
}

#[test]
fn multiline_link_text_is_left_to_pulldown() {
    // A destination that spans a newline is not the shape we claim; the
    // author's bytes must not be replaced by a sentinel that then leaks.
    let doc = parse("[![[card.png]]](/x/\n)\n");
    assert!(!sentinel_leaks(&doc.blocks), "sentinel leaked: {:?}", doc.blocks);
}

#[test]
fn a_heading_holding_an_embed_keeps_a_clean_anchor() {
    // The `id` is slugged mid-parse, when the heading text still reads
    // `U+E000…`. Left uncorrected, `<h2 id>` carries a private-use codepoint
    // and `[[Page#Awards]]` — what heading completion inserts — no longer
    // resolves. Recomputed after restore, the anchor is what the equivalent
    // CommonMark spelling produces.
    let md = "## Awards [![[badge.png]]](/awards/)\n";
    match &parse(md).blocks[0] {
        Block::Heading { id, .. } => {
            assert_eq!(id.as_deref(), Some("awards"), "anchor holds a sentinel");
        }
        other => panic!("expected heading, got {other:?}"),
    }
    // And the AST's two views of a heading agree, which is the invariant
    // `heading::text` exists to hold (see its module doc: one policy, two
    // adapters). `extract_headings` reads the same `id`.
    for md in [
        "## Awards [![[badge.png]]](/awards/)\n",
        "## [![[badge.png]]](/awards/) trailing\n",
        "## Plain heading\n",
    ] {
        let doc = parse(md);
        let Block::Heading { children, id, .. } = &doc.blocks[0] else {
            panic!("expected heading for {md:?}");
        };
        let label = crate::ast::plain_text::inlines_to_plain_text(children);
        assert_eq!(
            id.as_deref().expect("heading id"),
            crate::heading::anchor::obsidian_heading_anchor(&label),
            "slug and label disagree for {md:?}"
        );
    }
}

#[test]
fn duplicate_heading_numbering_sees_the_restored_slug() {
    // `assign_heading_id_suffixes` runs AFTER restore, so it must number the
    // corrected base slugs. Two headings that only collide once the sentinel
    // is gone have to come out `awards` / `awards-1`.
    let doc = parse("## Awards [![[a.png]]](/x/)\n\n## Awards\n");
    let ids: Vec<Option<&str>> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading { id, .. } => Some(id.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![Some("awards"), Some("awards-1")]);
}

#[test]
fn a_sentinel_never_reaches_a_url_or_a_title() {
    // `[![[a.png]]]([![[b.png]]](/u))` puts the inner compound inside the
    // OUTER link's destination. An href holding U+E000 is not a URL.
    for md in [
        "[![[a.png]]]([![[b.png]]](/u))\n",
        "[![[a.png]]](/u \"[![[b.png]]](/t)\")\n",
    ] {
        let blocks = parse(md).blocks;
        assert!(!sentinel_leaks(&blocks), "sentinel leaked for {md:?}: {blocks:?}");
    }
}

#[test]
fn author_written_private_use_text_survives() {
    // The sentinel body carries the per-parse nonce precisely so a literal
    // `U+E000 0 U+E001` in prose is NOT mistaken for embed 0 and replaced by
    // an image the author never wrote.
    let md = "keep \u{e000}0\u{e001} me\n\n[![[a.png]]](/x/)\n";
    let doc = parse(md);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            let text = crate::ast::plain_text::inlines_to_plain_text(inlines);
            assert_eq!(text, "keep \u{e000}0\u{e001} me", "author's bytes were eaten");
        }
        other => panic!("expected paragraph, got {other:?}"),
    }
    // …and the real compound in the same document still resolves.
    assert!(contains_linked_image(&doc.blocks[1..]), "{:?}", doc.blocks);
}

#[test]
fn source_lines_are_unmoved_by_substitution() {
    // The sentinel is line-count preserving, so a block AFTER a linked embed
    // keeps its real file line — the invariant editor↔preview scroll sync
    // depends on.
    let doc = parse_with_config(
        "para one\n\n[![[card.png]]](/x/)\n\npara three\n",
        &ParseConfig {
            emit_source_lines: true,
            ..ParseConfig::default()
        },
    );
    assert_eq!(doc.block_meta[0].source_line, Some(1));
    assert_eq!(doc.block_meta[1].source_line, Some(3));
    assert_eq!(doc.block_meta[2].source_line, Some(5));
}
