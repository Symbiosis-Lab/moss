use super::super::node::{CalloutKind, Fold, Inline};
use super::*;

fn first_block(md: &str) -> Block {
    parse(md)
        .blocks
        .into_iter()
        .next()
        .expect("at least one block")
}

// -----------------------------------------------------------------
// Phase 4 PR4: Block::Callout migration + Obsidian alias canonicalization
// -----------------------------------------------------------------

#[test]
fn parses_basic_callout_with_inline_title() {
    match first_block("> [!note] Heads up\n> Body line 1.\n") {
        Block::Callout {
            kind,
            fold,
            title,
            children,
        } => {
            assert_eq!(kind, CalloutKind::Note);
            assert!(fold.is_none(), "non-foldable callout");
            assert_eq!(title.as_deref(), Some("Heads up"));
            assert!(!children.is_empty(), "body should remain");
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn parses_titleless_callout() {
    match first_block("> [!warning]\n> Watch out.\n") {
        Block::Callout {
            kind,
            fold,
            title,
            children,
        } => {
            assert_eq!(kind, CalloutKind::Warning);
            assert!(fold.is_none());
            assert!(title.is_none(), "no inline title");
            assert!(!children.is_empty());
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_alias_tldr_canonicalizes_to_abstract() {
    match first_block("> [!tldr] Short summary\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Abstract),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_alias_hint_canonicalizes_to_tip() {
    match first_block("> [!hint] Pro tip\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Tip),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_alias_important_canonicalizes_to_tip() {
    match first_block("> [!important] Read this\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Tip),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_alias_check_done_canonicalizes_to_success() {
    for alias in &["check", "done"] {
        let md = format!("> [!{alias}] Yes\n> body\n");
        match first_block(&md) {
            Block::Callout { kind, .. } => assert_eq!(
                kind,
                CalloutKind::Success,
                "alias `{alias}` should canonicalize to Success"
            ),
            other => panic!("alias `{alias}` — expected Callout, got {other:?}"),
        }
    }
}

#[test]
fn callout_alias_help_faq_canonicalizes_to_question() {
    for alias in &["help", "faq"] {
        let md = format!("> [!{alias}] question\n> body\n");
        match first_block(&md) {
            Block::Callout { kind, .. } => assert_eq!(
                kind,
                CalloutKind::Question,
                "alias `{alias}` should canonicalize to Question"
            ),
            other => panic!("alias `{alias}` — expected Callout, got {other:?}"),
        }
    }
}

#[test]
fn callout_alias_caution_attention_canonicalizes_to_warning() {
    for alias in &["caution", "attention"] {
        let md = format!("> [!{alias}] careful\n> body\n");
        match first_block(&md) {
            Block::Callout { kind, .. } => assert_eq!(
                kind,
                CalloutKind::Warning,
                "alias `{alias}` should canonicalize to Warning"
            ),
            other => panic!("alias `{alias}` — expected Callout, got {other:?}"),
        }
    }
}

#[test]
fn callout_alias_fail_missing_canonicalizes_to_failure() {
    for alias in &["fail", "missing"] {
        let md = format!("> [!{alias}] oops\n> body\n");
        match first_block(&md) {
            Block::Callout { kind, .. } => assert_eq!(
                kind,
                CalloutKind::Failure,
                "alias `{alias}` should canonicalize to Failure"
            ),
            other => panic!("alias `{alias}` — expected Callout, got {other:?}"),
        }
    }
}

#[test]
fn callout_alias_error_canonicalizes_to_danger() {
    match first_block("> [!error] bad\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Danger),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_alias_cite_canonicalizes_to_quote() {
    match first_block("> [!cite] source\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Quote),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_foldable_open_suffix() {
    match first_block("> [!note]+ Open by default\n> body\n") {
        Block::Callout {
            kind, fold, title, ..
        } => {
            assert_eq!(kind, CalloutKind::Note);
            assert_eq!(fold, Some(Fold::Open));
            assert_eq!(title.as_deref(), Some("Open by default"));
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_foldable_closed_suffix() {
    match first_block("> [!note]- Closed by default\n> body\n") {
        Block::Callout {
            kind, fold, title, ..
        } => {
            assert_eq!(kind, CalloutKind::Note);
            assert_eq!(fold, Some(Fold::Closed));
            assert_eq!(title.as_deref(), Some("Closed by default"));
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_foldable_without_title() {
    match first_block("> [!tip]+\n> body\n") {
        Block::Callout {
            kind, fold, title, ..
        } => {
            assert_eq!(kind, CalloutKind::Tip);
            assert_eq!(fold, Some(Fold::Open));
            assert!(title.is_none());
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_unknown_kind_falls_back_to_note() {
    // Per shape-spec § 1 — unknown kind canonicalizes to Note.
    // Diagnostic emission is a Phase 4 followup (see parser.rs
    // `promote_callout` comment).
    match first_block("> [!unknownkind] body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Note),
        other => panic!("expected Callout (fallback to Note), got {other:?}"),
    }
}

#[test]
fn callout_multi_paragraph_body_preserves_blocks() {
    let md = "> [!info] Multi\n> First paragraph.\n>\n> Second paragraph.\n";
    match first_block(md) {
        Block::Callout {
            kind,
            title,
            children,
            ..
        } => {
            assert_eq!(kind, CalloutKind::Info);
            assert_eq!(title.as_deref(), Some("Multi"));
            // pulldown-cmark emits two paragraphs in the blockquote
            // body when separated by an empty `>` line.
            let para_count = children
                .iter()
                .filter(|b| matches!(b, Block::Paragraph(_)))
                .count();
            assert!(
                para_count >= 2,
                "expected at least 2 paragraphs, got {children:?}"
            );
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_nested_inside_callout() {
    // The docs promise nested callouts. After PR4 the outer is
    // Block::Callout containing an inner Block::Callout in its
    // children (no Stage 1 rewrite needed).
    let md = "> [!warning] Outer\n> Outer content.\n>\n> > [!tip] Inner\n> > Inner content.\n";
    match first_block(md) {
        Block::Callout {
            kind: outer_kind,
            children,
            ..
        } => {
            assert_eq!(outer_kind, CalloutKind::Warning);
            let inner = children.iter().find_map(|b| match b {
                Block::Callout { kind, title, .. } => Some((*kind, title.clone())),
                _ => None,
            });
            let (inner_kind, inner_title) =
                inner.expect("inner Block::Callout missing from outer's children");
            assert_eq!(inner_kind, CalloutKind::Tip);
            assert_eq!(inner_title.as_deref(), Some("Inner"));
        }
        other => panic!("expected outer Callout, got {other:?}"),
    }
}

#[test]
fn plain_blockquote_without_marker_stays_blockquote() {
    // Regression: an ordinary blockquote (no `[!type]` marker) must
    // remain Block::BlockQuote — only callout-shaped blockquotes
    // promote.
    match first_block("> Just a quote.\n> More of the quote.\n") {
        Block::BlockQuote(_) => {} // expected
        other => panic!("expected BlockQuote, got {other:?}"),
    }
}

#[test]
fn blockquote_with_text_starting_like_callout_but_unknown_kind_still_promotes() {
    // The marker `[!xyz]` is structurally a callout — we promote
    // and fall back to Note (per shape-spec). The author can fix
    // by removing the bracket prefix if they wanted a plain quote.
    match first_block("> [!xyz] not a real kind\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Note),
        other => panic!("expected Callout fallback, got {other:?}"),
    }
}

#[test]
fn callout_case_insensitive_kind() {
    // Stage 1 was case-insensitive; preserve that contract.
    match first_block("> [!WARNING] Loud\n> body\n") {
        Block::Callout { kind, .. } => assert_eq!(kind, CalloutKind::Warning),
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn callout_pending_alias_canonicalizes_to_todo() {
    // SoCiviC Theatre's voices.md uses `> [!pending]` — carried
    // over from Stage 1 support.
    match first_block("> [!pending] Trailer video\n> Add when ready.\n") {
        Block::Callout { kind, title, .. } => {
            assert_eq!(kind, CalloutKind::Todo);
            assert_eq!(title.as_deref(), Some("Trailer video"));
        }
        other => panic!("expected Callout, got {other:?}"),
    }
}

#[test]
fn empty_input_yields_empty_document() {
    let d = parse("");
    assert!(d.blocks.is_empty());
}

#[test]
fn parses_h1_heading() {
    match first_block("# Hello\n") {
        Block::Heading {
            level,
            children,
            id,
        } => {
            assert_eq!(level, 1);
            // Phase 4 PR2: parser populates id with the Obsidian anchor slug.
            assert_eq!(id.as_deref(), Some("hello"));
            assert!(matches!(&children[0], Inline::Text(t) if t == "Hello"));
        }
        other => panic!("expected Heading, got {other:?}"),
    }
}

#[test]
fn parses_h6_heading() {
    match first_block("###### tiny\n") {
        Block::Heading { level, .. } => assert_eq!(level, 6),
        other => panic!("expected Heading, got {other:?}"),
    }
}

#[test]
fn parses_paragraph_with_text() {
    match first_block("hello world\n") {
        Block::Paragraph(children) => {
            // pulldown-cmark may split into multiple Text events; merge.
            let s: String = children
                .iter()
                .filter_map(|i| match i {
                    Inline::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(s, "hello world");
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parses_link_with_unresolved_url() {
    // Critical contract: every URL starts as Unresolved.
    match first_block("[Docs](docs/)\n") {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link {
                url,
                title,
                children,
                is_wikilink,
            } => {
                assert!(url.is_unresolved());
                match url {
                    Url::Unresolved(s) => assert_eq!(s, "docs/"),
                    _ => unreachable!(),
                }
                assert!(title.is_none());
                assert!(!is_wikilink, "standard markdown link is not a wikilink");
                assert!(matches!(&children[0], Inline::Text(t) if t == "Docs"));
            }
            other => panic!("expected Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parses_link_with_moss_resolved_prefix_unchanged() {
    // The upstream resolve pipeline emits this shape; the parser must
    // preserve it verbatim for the visitor to classify later.
    match first_block("[t](moss-resolved:foo.md)\n") {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link {
                url: Url::Unresolved(s),
                ..
            } => assert_eq!(s, "moss-resolved:foo.md"),
            other => panic!("expected unresolved Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parser_link_inherits_wikilink_from_pulldown_cmark() {
    // PR7a Decision 2: pulldown-cmark with ENABLE_WIKILINKS emits
    // `Tag::Link { link_type: LinkType::WikiLink, .. }` for `[[target]]`
    // syntax. The typed AST must preserve that discriminator via
    // `Inline::Link::is_wikilink`. After PR7a flips render_document
    // to production, this flag drives the `class="wikilink"` emission
    // on the <a> tag.
    match first_block("[[wikilink-target]]\n") {
        Block::Paragraph(children) => {
            let link = children
                .iter()
                .find(|i| matches!(i, Inline::Link { .. }))
                .expect("expected an Inline::Link from [[…]]");
            match link {
                Inline::Link { is_wikilink, .. } => {
                    assert!(
                        *is_wikilink,
                        "[[…]] must set is_wikilink: true on the typed AST"
                    );
                }
                _ => unreachable!(),
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }

    // Negative case: a standard markdown link is NOT a wikilink.
    match first_block("[text](href)\n") {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { is_wikilink, .. } => {
                assert!(!is_wikilink, "[](…) must set is_wikilink: false");
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn parses_link_with_title() {
    match first_block(r#"[t](u "the title")"#) {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { title, .. } => assert_eq!(title.as_deref(), Some("the title")),
            other => panic!("expected Link, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parses_image_with_alt() {
    // Phase 4 PR3 (2026-05-27): an image-only paragraph is now
    // promoted to Block::Figure. Inline::Image lives inside the
    // Figure variant; the URL/alt/title contract is unchanged.
    // For image+text (where Block::Paragraph still applies), see
    // `image_with_caption_text_does_not_promote` below.
    match first_block("![cat photo](cat.jpg)\n") {
        Block::Figure { image, caption, .. } => {
            match image {
                Inline::Image {
                    src, alt, title, ..
                } => {
                    assert!(src.is_unresolved());
                    assert_eq!(alt, "cat photo");
                    assert!(title.is_none());
                }
                other => panic!("expected Image inside Figure, got {other:?}"),
            }
            let cap = caption.expect("caption from alt text");
            assert_eq!(cap.len(), 1);
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn parses_image_inside_paragraph_with_text() {
    // Companion to `parses_image_with_alt`: an image with sibling
    // prose stays as Block::Paragraph (no figure promotion). Holds
    // the parser's image-extraction contract for the non-figure case.
    match first_block("see ![cat photo](cat.jpg) here\n") {
        Block::Paragraph(children) => {
            let img = children
                .iter()
                .find(|i| matches!(i, Inline::Image { .. }))
                .expect("expected Inline::Image among siblings");
            match img {
                Inline::Image { src, alt, .. } => {
                    assert!(src.is_unresolved());
                    assert_eq!(alt, "cat photo");
                }
                _ => unreachable!(),
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parses_emphasis_and_strong() {
    let para = parse("*em* and **strong**\n")
        .blocks
        .into_iter()
        .next()
        .unwrap();
    match para {
        Block::Paragraph(children) => {
            let has_em = children.iter().any(|i| matches!(i, Inline::Emphasis(_)));
            let has_strong = children.iter().any(|i| matches!(i, Inline::Strong(_)));
            assert!(has_em, "missing Emphasis: {children:?}");
            assert!(has_strong, "missing Strong: {children:?}");
        }
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn parses_inline_code() {
    match first_block("`some code`\n") {
        Block::Paragraph(children) => {
            assert!(matches!(&children[0], Inline::Code(c) if c == "some code"));
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn parses_unordered_list() {
    match first_block("- one\n- two\n") {
        Block::List { ordered, items, .. } => {
            assert!(!ordered);
            assert_eq!(items.len(), 2);
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parser_handles_tight_list_items_with_inline_content() {
    // Phase 4 PR0.6 regression — pulldown-cmark's tight-list mode emits
    // inline events (Text/Strong/etc.) directly inside Tag::Item without
    // wrapping in Tag::Paragraph. Previously `parse_block` dropped these
    // stray inlines, producing empty <li></li> instead of the expected
    // <li><strong>bold</strong> text</li>.
    match first_block("- **bold** text\n- another item\n") {
        Block::List { ordered, items, .. } => {
            assert!(!ordered);
            assert_eq!(items.len(), 2, "expected two items, got {items:?}");
            let first_item = &items[0];
            assert_eq!(
                first_item.len(),
                1,
                "tight item should synthesize a single Paragraph, got {first_item:?}"
            );
            match &first_item[0] {
                Block::Paragraph(inlines) => {
                    let has_strong = inlines.iter().any(|i| matches!(i, Inline::Strong(_)));
                    let has_text = inlines
                        .iter()
                        .any(|i| matches!(i, Inline::Text(t) if t.contains("text")));
                    assert!(
                        has_strong,
                        "expected Inline::Strong inside item, got {inlines:?}"
                    );
                    assert!(has_text, "expected ' text' Inline::Text, got {inlines:?}");
                }
                other => panic!("expected Paragraph inside tight item, got {other:?}"),
            }
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn tight_list_items_with_links_preserved() {
    // Mirrors folder-note-site/obsidian/index.md — wikilinks + images
    // inside list items. Today these parse as Inline::Link / Inline::Image;
    // the contract is just that the inline content is NOT dropped.
    match first_block("- [link](url)\n- ![alt](img.jpg)\n") {
        Block::List { items, .. } => {
            assert_eq!(items.len(), 2);
            let first = &items[0];
            assert_eq!(
                first.len(),
                1,
                "expected one Block::Paragraph, got {first:?}"
            );
            match &first[0] {
                Block::Paragraph(inlines) => {
                    assert!(
                        inlines.iter().any(|i| matches!(i, Inline::Link { .. })),
                        "expected Inline::Link, got {inlines:?}"
                    );
                }
                other => panic!("expected Paragraph, got {other:?}"),
            }
            let second = &items[1];
            match &second[0] {
                Block::Paragraph(inlines) => {
                    assert!(
                        inlines.iter().any(|i| matches!(i, Inline::Image { .. })),
                        "expected Inline::Image, got {inlines:?}"
                    );
                }
                other => panic!("expected Paragraph, got {other:?}"),
            }
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn loose_list_items_with_paragraphs_still_work() {
    // Loose-list mode (blank lines between items) emits items as
    // Tag::Paragraph-wrapped blocks. The fix must not break this path.
    let md = "- first item\n\n- second item\n";
    match first_block(md) {
        Block::List { items, .. } => {
            assert_eq!(items.len(), 2);
            for item in &items {
                assert_eq!(item.len(), 1, "expected one block per item");
                assert!(
                    matches!(&item[0], Block::Paragraph(_)),
                    "expected Paragraph, got {:?}",
                    item[0]
                );
            }
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn tight_list_items_with_nested_list_preserve_structure() {
    // - first
    //   - nested
    // The outer item carries inline "first" + a nested Block::List.
    let md = "- first\n  - nested\n";
    match first_block(md) {
        Block::List { items, .. } => {
            assert_eq!(items.len(), 1);
            let outer = &items[0];
            assert!(
                outer.iter().any(|b| matches!(b, Block::Paragraph(_))),
                "expected outer item to carry a Paragraph for 'first', got {outer:?}"
            );
            assert!(
                outer.iter().any(|b| matches!(b, Block::List { .. })),
                "expected outer item to carry a nested List, got {outer:?}"
            );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parses_ordered_list() {
    match first_block("1. first\n2. second\n") {
        Block::List { ordered, items, .. } => {
            assert!(ordered);
            assert_eq!(items.len(), 2);
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parses_fenced_code_block_with_lang() {
    match first_block("```rust\nfn main() {}\n```\n") {
        Block::CodeBlock { lang, value } => {
            assert_eq!(lang.as_deref(), Some("rust"));
            assert!(value.contains("fn main"));
        }
        other => panic!("expected CodeBlock, got {other:?}"),
    }
}

#[test]
fn parses_fenced_code_block_without_lang() {
    match first_block("```\nbare\n```\n") {
        Block::CodeBlock { lang, value } => {
            assert!(lang.is_none());
            assert!(value.contains("bare"));
        }
        other => panic!("expected CodeBlock, got {other:?}"),
    }
}

#[test]
fn code_block_is_not_parsed_as_shortcode() {
    // Adversarial: the literal `:::buttons` inside a fenced code block
    // must NOT be treated as a shortcode. (Phase A's parser doesn't
    // recognize :::buttons at all yet; this test locks the contract.)
    let md = "```\n:::buttons\n[t](u)\n:::\n```\n";
    match first_block(md) {
        Block::CodeBlock { value, .. } => assert!(value.contains(":::buttons")),
        other => panic!("expected CodeBlock, got {other:?}"),
    }
}

#[test]
fn parses_blockquote() {
    match first_block("> quoted\n") {
        Block::BlockQuote(children) => {
            assert!(!children.is_empty());
        }
        other => panic!("expected BlockQuote, got {other:?}"),
    }
}

#[test]
fn parses_thematic_break() {
    match first_block("---\n") {
        Block::ThematicBreak => {}
        // Pulldown-cmark may emit a thematic break or treat `---` at the
        // start of a doc as a heading underline. Accept either by
        // checking that the parse produces SOMETHING.
        _other => {
            // Test the unambiguous mid-doc case.
            let d = parse("para\n\n---\n\nmore\n");
            let has_break = d.blocks.iter().any(|b| matches!(b, Block::ThematicBreak));
            assert!(
                has_break,
                "expected at least one ThematicBreak: {:?}",
                d.blocks
            );
        }
    }
}

#[test]
fn parses_table() {
    let md = "| h1 | h2 |\n| --- | --- |\n| a | b |\n| c | d |\n";
    match first_block(md) {
        Block::Table { header, rows, .. } => {
            assert_eq!(header.len(), 2);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 2);
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn html_block_passes_through_as_other() {
    match first_block("<div class=\"raw\">hi</div>\n\n") {
        Block::Other(html) => assert!(html.contains("<div")),
        other => panic!("expected Other, got {other:?}"),
    }
}

#[test]
fn parses_multiple_blocks() {
    let d = parse("# T\n\npara\n\n- li\n");
    assert_eq!(d.blocks.len(), 3);
    assert!(matches!(d.blocks[0], Block::Heading { .. }));
    assert!(matches!(d.blocks[1], Block::Paragraph(_)));
    assert!(matches!(d.blocks[2], Block::List { .. }));
}

#[test]
fn frontmatter_only_input_is_handled() {
    // Frontmatter is stripped by upstream code before reaching the
    // parser. If somehow a `---\nfoo:bar\n---` reaches us, the parser
    // must not panic.
    let _ = parse("---\nfoo: bar\n---\n");
}

#[test]
fn link_inside_heading_is_preserved() {
    match first_block("# [t](u)\n") {
        Block::Heading { children, .. } => {
            assert!(matches!(&children[0], Inline::Link { .. }));
        }
        other => panic!("expected Heading, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Phase 4 PR2: heading ID injection
// -----------------------------------------------------------------

fn heading_id(md: &str) -> Option<String> {
    let blocks = parse(md).blocks;
    for block in &blocks {
        if let Block::Heading { id, .. } = block {
            return id.clone();
        }
    }
    None
}

#[test]
fn heading_id_simple_phrase() {
    // SoCiviC `## Mission` baseline case.
    assert_eq!(heading_id("## Mission\n"), Some("mission".to_string()));
}

#[test]
fn heading_id_spaces_become_hyphens() {
    assert_eq!(
        heading_id("# Getting Started\n"),
        Some("getting-started".to_string())
    );
}

#[test]
fn heading_id_with_emphasis_uses_text_content() {
    // `*em*` inside a heading: the inner text is `em`, no surrounding
    // chars come from emphasis itself (production captures only Text/Code).
    assert_eq!(
        heading_id("# Hello *world*\n"),
        Some("hello-world".to_string())
    );
}

#[test]
fn heading_id_with_strong_uses_text_content() {
    assert_eq!(
        heading_id("# Bold **stuff**\n"),
        Some("bold-stuff".to_string())
    );
}

#[test]
fn heading_id_with_inline_link_uses_link_text() {
    // `# [Docs](url)` — the link label "Docs" comes through as Event::Text.
    assert_eq!(heading_id("# [Docs](url)\n"), Some("docs".to_string()));
}

#[test]
fn heading_id_with_inline_code_includes_code_payload() {
    // Production captures Event::Code, so `` `fn(x)` `` enters the slug.
    assert_eq!(
        heading_id("# call `fn(x)`\n"),
        Some("call-fn(x)".to_string())
    );
}

#[test]
fn heading_id_with_inline_html_strips_html() {
    // SoCiviC `# FAREWELL,<br>AND ERASE` — the `<br>` is Event::InlineHtml
    // and must NOT appear in the slug. Production's slug for this is
    // derived from "FAREWELL,AND ERASE".
    let id = heading_id("# FAREWELL,<br>AND ERASE\n").expect("heading id");
    // No `<br>` or `br` injected; punctuation preserved (`,`), spaces → `-`.
    assert!(!id.contains("br"), "got: {id}");
    assert_eq!(id, "farewell,and-erase");
}

#[test]
fn heading_id_cjk_preserved() {
    // 刘果's CJK headings exercise Unicode anchor normalization —
    // characters pass through unchanged (lowercase already, no whitespace).
    assert_eq!(heading_id("## 视频\n"), Some("视频".to_string()));
    assert_eq!(heading_id("## 中文标题\n"), Some("中文标题".to_string()));
}

#[test]
fn heading_id_obsidian_strip_chars() {
    // Pipes / brackets / hashes / backslashes / carets are stripped.
    assert_eq!(heading_id("# Note ^ref\n"), Some("note-ref".to_string()));
    assert_eq!(heading_id("# A | B\n"), Some("a-b".to_string()));
}

#[test]
fn duplicate_headings_get_suffixed_ids() {
    // Production behavior: first occurrence keeps slug; second gets `-1`,
    // third gets `-2`. The HashMap in pipeline.rs:1798 is the contract.
    let md = "# Mission\n\n# Mission\n\n# Mission\n";
    let doc = parse(md);
    let ids: Vec<Option<String>> = doc
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            Some("mission".to_string()),
            Some("mission-1".to_string()),
            Some("mission-2".to_string()),
        ]
    );
}

#[test]
fn duplicate_suffix_descends_into_blockquote() {
    // Headings inside a blockquote share the same id-counter as top-level.
    let md = "# Notes\n\n> # Notes\n";
    let doc = parse(md);
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["notes".to_string(), "notes-1".to_string()]);
}

#[test]
fn duplicate_suffix_descends_into_footnote_definition() {
    // A heading inside a footnote definition renders into the endnote
    // section of the SAME document, so it must share the id counter —
    // otherwise the page carries two `id="notes"` and the endnote heading's
    // permalink resolves to the body heading.
    let md = "## Notes\n\nX[^h].\n\n[^h]: body\n\n    ## Notes\n";
    let doc = parse(md);
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["notes".to_string(), "notes-1".to_string()]);
}

#[test]
fn duplicate_suffix_descends_into_grid_cells() {
    // A `:::grid` cell is parsed by a RECURSIVE `parse_with_config` with a
    // fresh id counter, so every cell heading arrives holding an
    // un-disambiguated base slug. The cell renders into the same page as the
    // body, so without a descent here the page carries two `id="notes"` and
    // `#notes` resolves to the grid card — never the author's section.
    let doc = parse(":::grid\n### Notes\n:::\n\n## Notes\n");
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["notes".to_string(), "notes-1".to_string()]);
}

#[test]
fn duplicate_suffix_dedups_two_cells_of_one_grid() {
    // The intra-shortcode collision: no outer heading needed.
    let doc = parse(":::grid 2\n### Team\n+++\n### Team\n:::\n");
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["team".to_string(), "team-1".to_string()]);
}

#[test]
fn duplicate_suffix_descends_into_hero_overlay() {
    let doc = parse(":::hero {image=a.jpg}\n### Notes\n:::\n\n## Notes\n");
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["notes".to_string(), "notes-1".to_string()]);
}

#[test]
fn duplicate_suffix_descends_into_link_card() {
    // The SoCiviC compound-link cell: `Block::LinkCard { children }` holds
    // block-level content, headings included.
    let doc = parse(":::grid\n[### Notes\n\ntext](/a)\n:::\n\n## Notes\n");
    let mut found_ids: Vec<String> = Vec::new();
    collect_heading_ids_recursive(&doc.blocks, &mut found_ids);
    assert_eq!(found_ids, vec!["notes".to_string(), "notes-1".to_string()]);
}

fn collect_heading_ids_recursive(blocks: &[Block], out: &mut Vec<String>) {
    use super::super::shortcode::Shortcode;
    for b in blocks {
        match b {
            Block::Heading { id, .. } => {
                if let Some(s) = id {
                    out.push(s.clone());
                }
            }
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. }
            | Block::FootnoteDefinition { children, .. } => {
                collect_heading_ids_recursive(children, out);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_heading_ids_recursive(item, out);
                }
            }
            Block::Shortcode(Shortcode::Grid(args)) => {
                for cell in &args.cells {
                    collect_heading_ids_recursive(cell, out);
                }
            }
            Block::Shortcode(Shortcode::Hero(args)) => {
                collect_heading_ids_recursive(&args.overlay, out);
            }
            _ => {}
        }
    }
}

#[test]
fn heading_id_empty_text_yields_empty_slug() {
    // Edge case: `# ###` strips to empty slug; suffix counter still ticks.
    // (obsidian_heading_anchor("") == "")
    let md = "# ###\n";
    let id = heading_id(md);
    assert_eq!(id, Some(String::new()));
}

#[test]
fn link_inside_emphasis_unwraps_correctly() {
    // *[link](u)* — emphasis wrapping a link is a real authoring pattern.
    match first_block("*[t](u)*\n") {
        Block::Paragraph(children) => match &children[0] {
            Inline::Emphasis(inner) => {
                assert!(matches!(&inner[0], Inline::Link { .. }));
            }
            other => panic!("expected Emphasis, got {other:?}"),
        },
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Phase 4 PR3 (2026-05-27): Block::Figure detection in Tag::Paragraph
// -----------------------------------------------------------------

#[test]
fn image_only_paragraph_promotes_to_figure() {
    // Canonical case: a paragraph containing exactly one image, no
    // sibling inline content, becomes Block::Figure. Caption defaults
    // to the image's alt text.
    match first_block("![A logo](logo.png)\n") {
        Block::Figure { image, caption, .. } => {
            match image {
                Inline::Image { src, alt, .. } => {
                    assert!(src.is_unresolved());
                    assert_eq!(alt, "A logo");
                }
                other => panic!("expected Image inside Figure, got {other:?}"),
            }
            let cap = caption.expect("caption from alt text");
            assert_eq!(cap.len(), 1);
            assert!(matches!(&cap[0], Inline::Text(t) if t == "A logo"));
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn image_only_paragraph_with_empty_alt_stays_as_paragraph() {
    // Empty-alt guard: a decorative image (no alt) does NOT promote
    // to Figure. Production's implicit-figure pass gates on
    // non-empty alt — wrapping a no-alt image in `<figure>` adds
    // visual noise (no figcaption text) without a11y benefit. The
    // bytes match production's `<p><img></p>` shape.
    //
    // Parity-probe evidence: pre-guard, 7 CJK 刘果 fixtures with
    // trailing empty-alt images flipped to "other" because the AST
    // emitted `<figure>` and prod did not. Guard restores parity.
    match first_block("![](logo.png)\n") {
        Block::Paragraph(children) => {
            assert_eq!(children.len(), 1);
            match &children[0] {
                Inline::Image { alt, .. } => assert_eq!(alt, ""),
                other => panic!("expected Image inside Paragraph, got {other:?}"),
            }
        }
        other => panic!("empty-alt image-only paragraph must stay as Paragraph, got {other:?}"),
    }
}

#[test]
fn image_with_whitespace_text_still_promotes_to_figure() {
    // Whitespace-only text or line-break siblings don't disqualify
    // (matches transform_events' "image-only modulo whitespace"
    // behavior). Verifying via a wikilink + trailing whitespace would
    // require an actual whitespace event; pulldown-cmark typically
    // strips this. The detector is defensive for the cases that
    // DO surface whitespace inlines (line breaks after the image).
    let md = "![alt](a.jpg)  \n";
    // The trailing "  \n" inside a paragraph emits a HardBreak event
    // (Inline::LineBreak). Promotion must still succeed.
    match first_block(md) {
        Block::Figure { image, .. } => assert!(matches!(image, Inline::Image { .. })),
        // pulldown-cmark may also collapse this differently; accept
        // Paragraph(LineBreak) as a tolerated fallback so the test is
        // not over-specified on pulldown-cmark whitespace semantics.
        // The critical regression we want to lock is that genuine
        // image+text mixes DON'T promote (covered by the test below).
        Block::Paragraph(_) => {}
        other => panic!("expected Figure or Paragraph, got {other:?}"),
    }
}

#[test]
fn image_with_caption_text_does_not_promote() {
    // Critical regression guard (cf. PR1 v2 commit 71c657af3): a
    // paragraph carrying image + prose / emphasis must NOT be
    // promoted to a figure. If we promoted, the caption text would
    // be lost and we'd produce a malformed figure with sibling
    // content swallowed.
    match first_block("![alt](a.jpg) plain caption text\n") {
        Block::Paragraph(children) => {
            assert!(children.iter().any(|i| matches!(i, Inline::Image { .. })));
            assert!(
                children
                    .iter()
                    .any(|i| matches!(i, Inline::Text(t) if t.contains("plain"))),
                "expected sibling Text to remain, got {children:?}"
            );
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn image_with_emphasis_sibling_does_not_promote() {
    // Pandoc-style "image + emphasis caption" is recognized in the
    // legacy transform_events as a captioned figure, but PR3's
    // simplified detection (one Image, no other content modulo
    // whitespace) leaves these as Paragraph. PR0's parity probe
    // already classifies these under image_emission / image_figures
    // depending on production behavior; PR3 owns ONLY the simple
    // image-only case. The downstream image+emphasis case is closed
    // out at PR7a when production flips.
    match first_block("![alt](a.jpg) *caption*\n") {
        Block::Paragraph(children) => {
            assert!(children.iter().any(|i| matches!(i, Inline::Image { .. })));
            assert!(
                children.iter().any(|i| matches!(i, Inline::Emphasis(_))),
                "expected Emphasis to remain, got {children:?}"
            );
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn two_images_in_one_paragraph_do_not_promote() {
    // Detection rule requires EXACTLY one image. Two images stay as
    // a paragraph (no figure wrap chosen — production would also
    // not wrap this in a figure).
    match first_block("![a](a.jpg) ![b](b.jpg)\n") {
        Block::Paragraph(children) => {
            let img_count = children
                .iter()
                .filter(|i| matches!(i, Inline::Image { .. }))
                .count();
            assert_eq!(img_count, 2);
        }
        other => panic!("expected Paragraph (two images), got {other:?}"),
    }
}

// Editor Image UX (2026-06-04): standard-image `|NN%` width carries
// into Block::Figure.width instead of leaking into the caption.
// ------------------------------------------------------------------

#[test]
fn standard_image_percent_promotes_with_width() {
    // ![alt|55%](pic.jpg) → Figure { width: Some("55%"), caption "alt" }
    match first_block("![alt|55%](pic.jpg)\n") {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            // caption is the remaining alt (width segment removed)
            let cap = caption.expect("caption from remaining alt");
            assert!(matches!(cap.as_slice(), [Inline::Text(t)] if t == "alt"));
        }
        other => panic!("expected a Figure, got {other:?}"),
    }
}

#[test]
fn standard_image_percent_empty_alt_still_promotes() {
    // ![|55%](pic.jpg) → Figure (no caption) carrying the width.
    match first_block("![|55%](pic.jpg)\n") {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            assert!(
                caption.is_none() || matches!(caption.as_deref(), Some([])),
                "empty-alt-with-width figure must not carry a caption: {caption:?}"
            );
        }
        other => panic!("expected a Figure even with empty alt when width present, got {other:?}"),
    }
}

#[test]
fn standard_image_no_width_unchanged() {
    // ![alt](pic.jpg) → Figure { width: None } (existing behavior)
    match first_block("![alt](pic.jpg)\n") {
        Block::Figure { width, .. } => assert_eq!(width, None),
        other => panic!("expected a Figure, got {other:?}"),
    }
}

#[test]
fn plain_paragraph_still_parses_as_paragraph() {
    // No regression: a normal text paragraph stays as Block::Paragraph.
    match first_block("just some prose\n") {
        Block::Paragraph(_) => {}
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Phase B Task 7: :::subscribe end-to-end
// -----------------------------------------------------------------

use super::super::shortcode::Shortcode;

#[test]
fn parses_subscribe_block_into_typed_shortcode() {
    let md = r#":::subscribe {placeholder="you@domain.com" button="Sign me up"}
:::
"#;
    let doc = parse(md);
    // Should find one Block::Shortcode(Subscribe) at top level.
    let mut found: Option<&Shortcode> = None;
    for block in &doc.blocks {
        if let Block::Shortcode(sc) = block {
            found = Some(sc);
            break;
        }
    }
    let sc = found.expect("expected Block::Shortcode");
    match sc {
        Shortcode::Subscribe(args) => {
            assert_eq!(args.placeholder.as_deref(), Some("you@domain.com"));
            assert_eq!(args.button.as_deref(), Some("Sign me up"));
        }
        other => panic!("expected Subscribe, got {other:?}"),
    }
}

#[test]
fn subscribe_block_does_not_leave_sentinel_in_other_block() {
    let md = ":::subscribe\n:::\n";
    let doc = parse(md);
    // No Block::Other should contain the sentinel string.
    for block in &doc.blocks {
        if let Block::Other(html) = block {
            assert!(
                !html.contains("MOSS_SHORTCODE"),
                "unsubstituted sentinel remained in AST: {html:?}"
            );
        }
    }
}

#[test]
fn subscribe_inside_paragraph_text_is_not_extracted() {
    // Adversarial: `:::subscribe` appearing inside running prose
    // (not as a block opener on its own line) is not a shortcode.
    // The extractor only matches when `:::name` is on its own line.
    let md = "Read more about :::subscribe in the docs.\n";
    let doc = parse(md);
    for block in &doc.blocks {
        assert!(
            !matches!(block, Block::Shortcode(_)),
            "`:::subscribe` inline-text was wrongly extracted as a shortcode"
        );
    }
}

#[test]
fn subscribe_block_alongside_other_content_preserves_order() {
    let md = "# H\n\nfirst para\n\n:::subscribe\ndescription: d\n:::\n\nlast para\n";
    let doc = parse(md);
    let kinds: Vec<&'static str> = doc
        .blocks
        .iter()
        .map(|b| match b {
            Block::Heading { .. } => "h",
            Block::Paragraph(_) => "p",
            Block::Shortcode(_) => "sc",
            _ => "x",
        })
        .collect();
    assert_eq!(kinds, vec!["h", "p", "sc", "p"]);
}

// -----------------------------------------------------------------
// 2026-05-28 (Phase 4 source-line wiring): ParseConfig threading
// -----------------------------------------------------------------

#[test]
fn parse_default_config_keeps_block_meta_empty() {
    let doc = parse("# H1\n\npara one\n\npara two\n");
    assert_eq!(doc.blocks.len(), 3);
    assert_eq!(doc.block_meta.len(), doc.blocks.len());
    for meta in &doc.block_meta {
        assert!(
            meta.source_line.is_none(),
            "default parse should not populate source_line: {meta:?}"
        );
    }
}

#[test]
fn parse_with_source_lines_assigns_1_based_line_numbers() {
    let md = "# H1\n\npara on line 3\n\n## H2 on line 5\n\npara on line 7\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    // Expected blocks: H1, P, H2, P (4 blocks).
    assert_eq!(doc.blocks.len(), 4);
    assert_eq!(doc.block_meta.len(), 4);
    // Line numbers should track the markdown source.
    assert_eq!(doc.block_meta[0].source_line, Some(1), "H1 on line 1");
    assert_eq!(doc.block_meta[1].source_line, Some(3), "P on line 3");
    assert_eq!(doc.block_meta[2].source_line, Some(5), "H2 on line 5");
    assert_eq!(doc.block_meta[3].source_line, Some(7), "P on line 7");
}

#[test]
fn source_line_offset_is_applied_additively() {
    // The parser applies `source_line_offset` additively to every block's
    // body-relative line. What the offset MEANS (how it maps the body back
    // to the editor's CM6 buffer) is decided by the caller in pipeline.rs —
    // this test only pins the additive mechanism, not a coordinate model.
    let md = "# H1\n\npara on line 3\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 7,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    assert_eq!(
        doc.block_meta[0].source_line,
        Some(8),
        "H1 body-line 1 + offset 7"
    );
    assert_eq!(
        doc.block_meta[1].source_line,
        Some(10),
        "P body-line 3 + offset 7"
    );
}

#[test]
fn source_lines_not_collapsed_across_multiline_shortcode() {
    // A multi-line shortcode (grid) must NOT collapse the source lines of
    // blocks after it. The grid spans lines 3–11; the heading after is on
    // line 13. Before the line-count-preserving placeholder fix it
    // collapsed to ~line 5, so editor→preview scroll-sync sent any cursor
    // past the block to the page bottom.
    let md =
        "# Title\n\n:::grid 3\n[\n![](a.jpg)\n](/x)\n+++\n[\n![](b.jpg)\n](/y)\n:::\n\n## After\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    // Blocks: H1 (line 1), Shortcode grid (line 3), H2 "After" (line 13).
    let last = doc
        .block_meta
        .last()
        .expect("at least one block")
        .source_line;
    assert_eq!(
        last,
        Some(13),
        "heading after a multi-line grid must keep its real line 13, not a collapsed line"
    );
}

#[test]
fn parse_with_source_lines_lists_and_blockquotes() {
    let md = "- item one\n- item two\n\n> quote on line 4\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.block_meta[0].source_line, Some(1), "ul on line 1");
    assert_eq!(doc.block_meta[1].source_line, Some(4), "bq on line 4");
}

// -----------------------------------------------------------------
// 2026-05-28 (Phase 4 source-line followup): per-<li> + per-<tr>
// line tracking on Block::List and Block::Table.
// -----------------------------------------------------------------

#[test]
fn parse_with_source_lines_populates_item_lines_on_list() {
    // Multi-item list spanning consecutive source lines; the parser
    // must capture the 1-based line of each `Tag::Item` start.
    let md = "- one\n- two\n- three\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List {
            items,
            item_source_lines,
            ..
        } => {
            assert_eq!(items.len(), 3);
            assert_eq!(
                item_source_lines.len(),
                3,
                "item_source_lines must be parallel to items"
            );
            assert_eq!(item_source_lines[0], Some(1));
            assert_eq!(item_source_lines[1], Some(2));
            assert_eq!(item_source_lines[2], Some(3));
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parse_default_config_leaves_item_source_lines_empty() {
    // Production publish builds (default config — `emit_source_lines:
    // false`) must NOT populate `item_source_lines`. The renderer
    // treats empty as "no annotations" so the published HTML is
    // byte-identical to the pre-followup output.
    let doc = parse("- one\n- two\n");
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List {
            item_source_lines, ..
        } => {
            assert!(
                    item_source_lines.is_empty(),
                    "default config must NOT populate item_source_lines (publish builds): {item_source_lines:?}"
                );
        }
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn parse_with_source_lines_populates_row_lines_on_table() {
    // Multi-row table: header on line 1, separator on line 2, body
    // rows on lines 3, 4, 5. The parser must capture the 1-based
    // line of each `Tag::TableRow` start.
    let md = "| h1 | h2 |\n| --- | --- |\n| a | b |\n| c | d |\n| e | f |\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Table {
            rows,
            header_source_line,
            row_source_lines,
            ..
        } => {
            assert_eq!(rows.len(), 3);
            // The header tr anchors at the markdown header row (line 1).
            assert_eq!(*header_source_line, Some(1), "header tr line");
            assert_eq!(
                row_source_lines.len(),
                3,
                "row_source_lines must be parallel to rows"
            );
            assert_eq!(row_source_lines[0], Some(3));
            assert_eq!(row_source_lines[1], Some(4));
            assert_eq!(row_source_lines[2], Some(5));
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn parse_default_config_leaves_row_source_lines_empty() {
    // Production publish builds must not populate table row lines.
    let md = "| h1 | h2 |\n| --- | --- |\n| a | b |\n";
    let doc = parse(md);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Table {
            header_source_line,
            row_source_lines,
            ..
        } => {
            assert!(header_source_line.is_none());
            assert!(row_source_lines.is_empty());
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn parse_captures_gfm_column_alignment() {
    // `|:--|:-:|--:|` → Left, Center, Right. moss previously discarded GFM
    // alignment entirely; the renderer now honors it over numeric detection.
    let md = "| L | C | R |\n|:--|:-:|--:|\n| a | b | c |\n";
    let doc = parse(md);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Table { alignments, .. } => {
            assert_eq!(
                alignments,
                &vec![
                    ColumnAlignment::Left,
                    ColumnAlignment::Center,
                    ColumnAlignment::Right,
                ]
            );
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn parse_unaligned_table_has_empty_alignments() {
    // A bare `|---|` separator carries no author alignment. pulldown emits
    // all-`None`; we normalize that to empty so the AST stays byte-stable.
    let md = "| h1 | h2 |\n| --- | --- |\n| a | b |\n";
    let doc = parse(md);
    match &doc.blocks[0] {
        Block::Table { alignments, .. } => {
            assert!(alignments.is_empty(), "unaligned table → empty alignments");
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

#[test]
fn parse_partial_alignment_keeps_none_for_unmarked_columns() {
    // Only the middle column is aligned; the vec is non-empty and the
    // unmarked columns stay `None` (render auto-detects those).
    let md = "| a | b | c |\n| --- | :-: | --- |\n| 1 | 2 | 3 |\n";
    let doc = parse(md);
    match &doc.blocks[0] {
        Block::Table { alignments, .. } => {
            assert_eq!(
                alignments,
                &vec![
                    ColumnAlignment::None,
                    ColumnAlignment::Center,
                    ColumnAlignment::None,
                ]
            );
        }
        other => panic!("expected Table, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// 2026-05-28 (Phase 4 followup B): ordered-list explicit start
// number captured from pulldown-cmark's `Tag::List(Option<u64>)`
// payload and round-tripped to the renderer as `<ol start="N">`.
// -----------------------------------------------------------------

#[test]
fn parse_ordered_list_start_3_captures_start_number() {
    // `3. foo` should capture `start: Some(3)` so the renderer can
    // emit `<ol start="3">`. CommonMark only honors the first
    // item's number — subsequent items are re-derived.
    let doc = parse("3. foo\n4. bar\n");
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List {
            ordered,
            start,
            items,
            ..
        } => {
            assert!(ordered, "ordered list");
            assert_eq!(*start, Some(3), "explicit start number captured");
            assert_eq!(items.len(), 2);
        }
        other => panic!("expected ordered List, got {other:?}"),
    }
}

#[test]
fn parse_ordered_list_default_start_collapses_to_none() {
    // pulldown-cmark normalizes `1. foo` to `Tag::List(Some(1))`,
    // but the AST canonicalizes this to `start: None` (semantically
    // identical to `<ol>` without a `start=` attribute, but cleaner).
    let doc = parse("1. foo\n2. bar\n");
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List { ordered, start, .. } => {
            assert!(ordered);
            assert!(
                start.is_none(),
                "implicit start=1 must collapse to None, got {start:?}"
            );
        }
        other => panic!("expected ordered List, got {other:?}"),
    }
}

#[test]
fn parse_unordered_list_has_no_start() {
    // `- foo` is unordered (`Tag::List(None)`). `start` must always
    // be `None` regardless of any subsequent reasoning.
    let doc = parse("- foo\n- bar\n");
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::List { ordered, start, .. } => {
            assert!(!ordered, "unordered list");
            assert!(
                start.is_none(),
                "unordered list must have start=None, got {start:?}"
            );
        }
        other => panic!("expected unordered List, got {other:?}"),
    }
}

#[test]
fn parse_with_source_lines_handles_list_after_blank_line_offset() {
    // List items can start past the document start; verify the
    // 1-based numbering tracks the actual source line, not a
    // 0-based index from the list opener.
    let md = "intro paragraph\n\n- item on line 3\n- item on line 4\n";
    let config = ParseConfig {
        emit_source_lines: true,
        implicit_figure: true,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config(md, &config);
    assert_eq!(doc.blocks.len(), 2);
    match &doc.blocks[1] {
        Block::List {
            item_source_lines, ..
        } => {
            assert_eq!(item_source_lines.len(), 2);
            assert_eq!(item_source_lines[0], Some(3));
            assert_eq!(item_source_lines[1], Some(4));
        }
        other => panic!("expected List as second block, got {other:?}"),
    }
}

#[test]
fn parse_implicit_figure_default_promotes_image_only_paragraph() {
    // Image-only paragraph with non-empty alt → promoted to Block::Figure.
    let doc = parse("![alt](photo.jpg)\n");
    assert_eq!(doc.blocks.len(), 1);
    assert!(
        matches!(doc.blocks[0], Block::Figure { .. }),
        "default config (implicit_figure=true) should promote: got {:?}",
        doc.blocks[0]
    );
}

#[test]
fn parse_implicit_figure_off_leaves_image_paragraph_unpromoted() {
    let config = ParseConfig {
        emit_source_lines: false,
        implicit_figure: false,
        source_line_offset: 0,
        math: false,
        hard_line_breaks: false,
    };
    let doc = parse_with_config("![alt](photo.jpg)\n", &config);
    assert_eq!(doc.blocks.len(), 1);
    match &doc.blocks[0] {
        Block::Paragraph(inlines) => {
            assert!(matches!(inlines[0], Inline::Image { .. }));
        }
        other => panic!("expected Paragraph with image, got {other:?}"),
    }
}

/// `[site].implicit_figure = false` is a whole-document opt-out, so the
/// unwrap walk has to reach every container the parser can put an
/// image-only paragraph inside. A footnote definition is the newest one,
/// and the walker's `_ => {}` catch-all is exactly why turning footnotes on
/// did not make the compiler say so: an opted-out site went on publishing a
/// `<figcaption>` in its endnotes.
#[test]
fn parse_implicit_figure_off_leaves_a_footnote_definition_image_unpromoted() {
    fn holds_figure(blocks: &[Block]) -> bool {
        blocks.iter().any(|block| match block {
            Block::Figure { .. } => true,
            Block::FootnoteDefinition { children, .. }
            | Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. } => holds_figure(children),
            Block::List { items, .. } => items.iter().any(|item| holds_figure(item)),
            _ => false,
        })
    }

    let config = ParseConfig {
        implicit_figure: false,
        ..ParseConfig::default()
    };
    let doc = parse_with_config("See [^a]\n\n[^a]: ![Cover](c.jpg)\n", &config);
    assert!(
        !holds_figure(&doc.blocks),
        "implicit_figure=false must reach inside a footnote definition, got {:?}",
        doc.blocks
    );

    // The default still promotes there, so the assertion above is testing
    // the opt-out and not an unrelated parse failure.
    let doc = parse("See [^a]\n\n[^a]: ![Cover](c.jpg)\n");
    assert!(
        holds_figure(&doc.blocks),
        "default config must still promote inside a footnote definition, got {:?}",
        doc.blocks
    );
}

// -----------------------------------------------------------------
// Implicit-figure caption renders inline markdown (option B)
//
// The implicit-figure caption is the image's alt content parsed as
// inline markdown — `*em*`, links, `` `code` `` and typeset math — while
// the `alt=` attribute keeps the flat plain-text source (math verbatim).
// Matches Pandoc's implicit-figure model. See
// docs/reference/target and the caption fix design.
// -----------------------------------------------------------------

#[test]
fn implicit_figure_caption_preserves_inline_markup() {
    // `![before *em* after](img.png)` → caption holds a typed
    // `Inline::Emphasis`, NOT a single flattened `Inline::Text`. The
    // image's `alt` stays the flat plain-text source (markers stripped).
    let block = first_block("![before *em* after](img.png)\n");
    match block {
        Block::Figure { caption, image, .. } => {
            let cap = caption.expect("caption must be present");
            assert!(
                cap.iter().any(|i| matches!(i, Inline::Emphasis(_))),
                "caption must carry a typed Emphasis node, got {cap:?}"
            );
            // The em markers must not survive as a flattened Text run.
            assert!(
                !cap.iter()
                    .any(|i| matches!(i, Inline::Text(t) if t.contains('*'))),
                "caption must not contain raw `*` markers, got {cap:?}"
            );
            match image {
                Inline::Image { alt, .. } => {
                    assert_eq!(
                        alt, "before em after",
                        "alt attribute must stay flat plain-text source"
                    );
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn implicit_figure_caption_carries_link_and_math_nodes() {
    // `![a [link](/x) and $x^2$ end](img.png)` (math on) → caption holds
    // a typed `Inline::Link` AND a math `Inline::Other` node so the
    // renderer's link + math hooks fire; the flat `alt` keeps the link
    // label and the math as its `$…$` source.
    let config = ParseConfig {
        emit_source_lines: false,
        implicit_figure: true,
        source_line_offset: 0,
        math: true,
        hard_line_breaks: false,
    };
    let doc = parse_with_config("![a [link](/x) and $x^2$ end](img.png)\n", &config);
    match doc.blocks.into_iter().next().expect("one block") {
        Block::Figure { caption, image, .. } => {
            let cap = caption.expect("caption must be present");
            assert!(
                cap.iter().any(|i| matches!(i, Inline::Link { .. })),
                "caption must carry a typed Link node, got {cap:?}"
            );
            assert!(
                cap.iter().any(|i| matches!(
                    i,
                    Inline::Other(html) if super::super::math_text::math_node_parts(html).is_some()
                )),
                "caption must carry a typed math node, got {cap:?}"
            );
            match image {
                Inline::Image { alt, .. } => {
                    assert_eq!(
                        alt, "a link and $x^2$ end",
                        "alt must stay flat: link label inlined, math as source"
                    );
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn implicit_figure_caption_only_math_is_a_math_node() {
    // `![$E=mc^2$](img.png)` (math on) → caption is a single math
    // `Inline::Other` node (so it typesets), alt is the `$…$` source.
    let config = ParseConfig {
        emit_source_lines: false,
        implicit_figure: true,
        source_line_offset: 0,
        math: true,
        hard_line_breaks: false,
    };
    let doc = parse_with_config("![$E=mc^2$](img.png)\n", &config);
    match doc.blocks.into_iter().next().expect("one block") {
        Block::Figure { caption, image, .. } => {
            let cap = caption.expect("caption must be present");
            assert!(
                cap.iter().any(|i| matches!(
                    i,
                    Inline::Other(html) if super::super::math_text::math_node_parts(html).is_some()
                )),
                "math-only caption must carry a math node, got {cap:?}"
            );
            match image {
                Inline::Image { alt, .. } => {
                    assert_eq!(alt, "$E=mc^2$", "alt must be the math source verbatim");
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn implicit_figure_empty_alt_still_yields_no_caption() {
    // Regression guard: an empty-alt image is not promoted (no figure,
    // hence no figcaption) — the rich-caption change must not regress it.
    let doc = parse("![](img.png)\n");
    assert!(
        !matches!(doc.blocks.first(), Some(Block::Figure { .. })),
        "empty-alt image must not promote to a figure: {:?}",
        doc.blocks
    );
}

// -----------------------------------------------------------------
// LineLookup unit tests (binary-search prefix-sum line table)
// -----------------------------------------------------------------

#[test]
fn line_lookup_offset_zero_is_line_one() {
    let lookup = LineLookup::build("hello\nworld\n", 0);
    assert_eq!(lookup.line_at(0), 1);
}

#[test]
fn line_lookup_after_first_newline_is_line_two() {
    let lookup = LineLookup::build("hello\nworld\n", 0);
    // Byte 6 is the 'w' of "world", which is on line 2.
    assert_eq!(lookup.line_at(6), 2);
}

#[test]
fn line_lookup_handles_multiline_block_starts() {
    let lookup = LineLookup::build("line1\nline2\nline3\n", 0);
    // First non-newline byte of each line.
    assert_eq!(lookup.line_at(0), 1, "byte 0 → line 1");
    assert_eq!(lookup.line_at(6), 2, "byte 6 → line 2");
    assert_eq!(lookup.line_at(12), 3, "byte 12 → line 3");
}

#[test]
fn line_lookup_empty_source() {
    let lookup = LineLookup::build("", 0);
    assert_eq!(lookup.line_at(0), 1, "empty source still has line 1");
}

// -----------------------------------------------------------------
// Wikilink percent-width — no-ContentGraph path
//
// `try_promote_to_figure` recovers a content-relative percent (`|55%`)
// from `wikilink_pothole` so the no-graph parse path (fragment/test
// render) carries width in `Block::Figure.width`, not as a spurious
// caption. These are regression guards for the no-graph fix.
//
// Sync: the with-graph twin lives in
// resolve/wikilink_dispatch.rs (image branch, `split_alt_width` call)
// — both split width via `media::split_alt_width`.
// -----------------------------------------------------------------

#[test]
fn wikilink_image_percent_no_graph_promotes_with_width() {
    // ![[pic.jpg|55%]] must carry |55% into Figure.width, not leak it
    // into the caption (regression guard for the no-graph fix).
    let block = first_block("![[pic.jpg|55%]]\n");
    match block {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            assert!(caption.is_none(), "percent must not become a caption");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn wikilink_image_percent_with_caption_no_graph() {
    // ![[pic.jpg|My cap|55%]] — width Some("55%"), caption "My cap".
    let block = first_block("![[pic.jpg|My cap|55%]]\n");
    match block {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            let cap = caption.as_ref().expect("caption must be present");
            assert!(
                matches!(cap.as_slice(), [Inline::Text(t)] if t == "My cap"),
                "caption should be the non-width segment, got {cap:?}"
            );
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

// -----------------------------------------------------------------
// Non-image wikilink embeds must NOT promote to Figure
//
// Figure is an image concept. pulldown-cmark parses every `![[…]]`
// as an Image event, so without a kind gate a video pothole
// (`![[clip.mov|77%]]`) was hijacked into Block::Figure — and
// `dispatch_wikilink_embeds` only dispatches Paragraph-shaped lone
// embeds, so the video synthesizer never ran: the page shipped
// `<figure><img src="clip.mov">` (broken image). The gate keys off
// the same classifier the dispatcher uses (`resolve::ext_kind`), so
// parse-time promotion and dispatch-time synthesis can never
// disagree about who owns the block.
// -----------------------------------------------------------------

#[test]
fn wikilink_video_percent_stays_paragraph() {
    let block = first_block("![[clip.mov|77%]]\n");
    match block {
        Block::Paragraph(inlines) => assert!(
            matches!(
                inlines.as_slice(),
                [Inline::Image {
                    is_wikilink: true,
                    ..
                }]
            ),
            "paragraph must hold the lone wikilink image, got {inlines:?}"
        ),
        other => panic!("video embed must stay Paragraph for dispatch, got {other:?}"),
    }
}

#[test]
fn wikilink_video_box_sizing_stays_paragraph() {
    // `|640x360` is the documented video sizing alias — it must reach
    // the dispatcher, not become a figcaption.
    let block = first_block("![[clip.mov|640x360]]\n");
    assert!(
        matches!(block, Block::Paragraph(_)),
        "expected Paragraph, got {block:?}"
    );
}

#[test]
fn wikilink_pdf_alias_stays_paragraph() {
    let block = first_block("![[report.pdf|80%]]\n");
    assert!(
        matches!(block, Block::Paragraph(_)),
        "expected Paragraph, got {block:?}"
    );
}

#[test]
fn wikilink_extensionless_stays_paragraph() {
    // `![[draft|55%]]` carries no extension intent — only the
    // with-graph dispatcher can resolve its kind, so the parser must
    // not commit it to an image Figure.
    let block = first_block("![[draft|55%]]\n");
    assert!(
        matches!(block, Block::Paragraph(_)),
        "expected Paragraph, got {block:?}"
    );
}

#[test]
fn wikilink_uppercase_image_ext_still_promotes() {
    // Extension matching is case-insensitive (vault files like
    // `photo.JPG` are common iPhone/camera exports).
    let block = first_block("![[photo.JPG|55%]]\n");
    assert!(
        matches!(block, Block::Figure { .. }),
        "expected Figure, got {block:?}"
    );
}

// -----------------------------------------------------------------
// Image alt: a line break inside alt is a space
// -----------------------------------------------------------------
//
// `alt` is an attribute AND (via the implicit-figure path) the visible
// `<figcaption>`, and Obsidian authors soft-wrap prose. Dropping the break
// ran the two lines together (`a\nb` → `ab`). The email walker adopted the
// browser/Obsidian rule in `infra/newsletter.rs` (a break inside alt is a
// space); these pin the same rule on the web side.

#[test]
fn image_alt_soft_break_becomes_a_space() {
    for hard_line_breaks in [false, true] {
        let config = ParseConfig {
            hard_line_breaks,
            ..ParseConfig::default()
        };
        let doc = parse_with_config("![Cover art\nby Jane](c.jpg)\n", &config);
        match doc.blocks.first().expect("one block") {
            Block::Figure { image, caption, .. } => {
                match image {
                    Inline::Image { alt, .. } => assert_eq!(
                        alt, "Cover art by Jane",
                        "soft-wrapped alt lines ran together (hard_line_breaks={hard_line_breaks})"
                    ),
                    other => panic!("expected Image, got {other:?}"),
                }
                let cap = caption.as_ref().expect("caption from alt text");
                assert!(
                    matches!(cap.as_slice(), [Inline::Text(t)] if t == "Cover art by Jane"),
                    "figcaption disagrees with alt: {cap:?}"
                );
            }
            other => panic!("expected Figure, got {other:?}"),
        }
    }
}

#[test]
fn image_alt_hard_break_becomes_a_space_while_caption_keeps_the_break() {
    // The one legal flattening inside an attribute is a space; a `<br>` is
    // legal inside a `<figcaption>`, so the two surfaces differ ON PURPOSE
    // for an EXPLICIT break. They must never differ on the text itself.
    let doc = parse("![one\\\ntwo](a.jpg)\n");
    match doc.blocks.first().expect("one block") {
        Block::Figure { image, caption, .. } => {
            match image {
                Inline::Image { alt, .. } => assert_eq!(alt, "one two"),
                other => panic!("expected Image, got {other:?}"),
            }
            let cap = caption.as_ref().expect("caption");
            assert!(
                matches!(cap.as_slice(), [Inline::Text(a), Inline::LineBreak, Inline::Text(b)] if a == "one" && b == "two"),
                "explicit break should survive in the caption: {cap:?}"
            );
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn image_alt_break_does_not_double_the_space() {
    // pulldown hands the trailing spaces of a wrapped line to the preceding
    // Text run, so a naive `push(' ')` would emit two.
    let doc = parse("![Cover art \nby Jane](c.jpg)\n");
    match doc.blocks.first().expect("one block") {
        Block::Figure { image, .. } => match image {
            Inline::Image { alt, .. } => assert_eq!(alt, "Cover art by Jane"),
            other => panic!("expected Image, got {other:?}"),
        },
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn nested_image_alt_folds_trailing_text_instead_of_leaking_as_body_prose() {
    // `![a ![b](inner.png) c](outer.png)` is valid CommonMark: the whole
    // span is ONE Image, and per the image-alt-flattening rule a nested
    // image's own text folds into the outer alt. Without depth-tracking,
    // the alt-collection loop stops at the INNER image's End(Image),
    // truncating alt to "a b" and leaking " c" out as sibling paragraph
    // prose instead of into the outer alt -- a different image than the
    // one the reader sees would be described, and body text would appear
    // that the author never wrote outside the image.
    match first_block("before ![a ![b](inner.png) c](outer.png) after\n") {
        Block::Paragraph(children) => {
            let images: Vec<&Inline> = children
                .iter()
                .filter(|i| matches!(i, Inline::Image { .. }))
                .collect();
            assert_eq!(images.len(), 1, "expected exactly one Image inline: {children:?}");
            match images[0] {
                Inline::Image { alt, .. } => assert_eq!(
                    alt, "a b c",
                    "the outer alt lost the inner image's trailing text: {children:?}"
                ),
                _ => unreachable!(),
            }
            let stray_text: String = children
                .iter()
                .filter_map(|i| match i {
                    Inline::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                stray_text, "before  after",
                "the inner image's trailing text leaked into sibling paragraph prose: {children:?}"
            );
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn nested_image_alone_in_a_paragraph_promotes_to_a_figure_with_a_real_nested_image_in_its_caption() {
    // `nested_image_alt_folds_trailing_text_instead_of_leaking_as_body_prose`
    // above only exercises the Paragraph case (surrounding "before"/"after"
    // text blocks promotion). When the SAME nested image is the paragraph's
    // only content, the promotion invariant promotes it to a Figure, and
    // `build_caption_inlines` re-parses the (depth-tracked) alt span through
    // the same inline machinery as body text -- so the figcaption must carry
    // a real `Inline::Image` for the inner image, not just its flattened
    // alt text folded into a single `Inline::Text` run.
    match first_block("![a ![b](inner.png) c](outer.png)\n") {
        Block::Figure { caption, image, .. } => {
            let cap = caption.expect("caption must be present");
            match cap.iter().find(|i| matches!(i, Inline::Image { .. })) {
                Some(Inline::Image { alt: inner_alt, .. }) => {
                    assert_eq!(inner_alt, "b", "nested image's own alt should survive in the caption");
                }
                other => panic!("caption must carry a real nested Image node, got {cap:?} ({other:?})"),
            }
            match image {
                Inline::Image { alt, .. } => assert_eq!(
                    alt, "a b c",
                    "outer alt must stay flat text even though the caption has a real nested Image"
                ),
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

/// The extractor has always collected unknown-shortcode warnings; until
/// 2026-08-05 nothing carried them out of the parse, so a misspelled
/// `:::gallry` produced a styled-but-empty div and a build that said nothing.
/// The only trace was `moss-unknown-shortcode` in the emitted HTML, which an
/// author has no reason to go looking for.
#[test]
fn unknown_shortcode_warning_reaches_the_caller() {
    let doc = crate::ast::parse(":::gallry\nbody\n:::\n");
    assert!(
        doc.warnings.iter().any(|w| w.contains("gallry")),
        "an unrecognized shortcode name must surface a warning on the Document, got {:?}",
        doc.warnings
    );
}

/// A misspelling nested inside an *unknown* shortcode still warns: that branch
/// recurses through `extract_with_state`, threading one collection.
#[test]
fn unknown_shortcode_warning_survives_unknown_nesting() {
    let doc = crate::ast::parse("::::outr\n:::gallry\nbody\n:::\n::::\n");
    assert!(
        doc.warnings.iter().filter(|w| w.contains("gallry")).count() == 1,
        "a misspelling nested inside another unknown shortcode must warn once, got {:?}",
        doc.warnings
    );
}

/// Closed 2026-08-05: a *valid* shortcode's body is re-parsed as a fragment
/// (`parse_cell_to_blocks` → `parse_fragment_with_config`), and that call
/// site used to return only `Vec<Block>`, dropping the fragment's `Document`
/// and its warnings with it — so a misspelling inside `:::grid` warned about
/// nothing. This test used to pin that boundary
/// (`unknown_shortcode_inside_a_valid_shortcode_does_not_yet_warn`, asserting
/// `warnings.is_empty()`); now `parse_cell_to_blocks` returns
/// `(Vec<Block>, Vec<String>)`, `parse_grid` threads the warnings through,
/// and `parse_shortcode_block`'s `"grid"` arm merges them — so they reach
/// `Document::warnings` the same way a top-level misspelling does.
#[test]
fn unknown_shortcode_inside_a_valid_shortcode_warns() {
    let doc = crate::ast::parse("::::grid\n:::gallry\nbody\n:::\n::::\n");
    assert!(
        doc.warnings.iter().any(|w| w.contains("gallry")),
        "a misspelling nested inside a valid shortcode's body must warn, got {:?}",
        doc.warnings
    );
}

/// The hero-overlay counterpart: `parse_hero`'s body is re-parsed via
/// `parse_overlay_to_blocks`, a second, independent call site that used to
/// drop the fragment's warnings the same way `parse_cell_to_blocks` did.
/// Closed alongside the grid case — `parse_overlay_to_blocks` now returns
/// `(Vec<Block>, Vec<String>)`, `parse_hero` threads the warnings through,
/// and `parse_shortcode_block`'s `"hero"` arm merges them.
#[test]
fn unknown_shortcode_inside_a_hero_overlay_warns() {
    let doc = crate::ast::parse("::::hero\n:::gallry\nbody\n:::\n::::\n");
    assert!(
        doc.warnings.iter().any(|w| w.contains("gallry")),
        "a misspelling nested inside a hero overlay must warn, got {:?}",
        doc.warnings
    );
}

/// The complement: a clean document must not acquire warnings, or the build
/// log becomes noise and authors learn to ignore it.
#[test]
fn valid_shortcodes_produce_no_warnings() {
    let doc = crate::ast::parse("::::grid\n:::hero\nbody\n:::\n::::\n");
    assert!(
        doc.warnings.is_empty(),
        "registered shortcode names must not warn, got {:?}",
        doc.warnings
    );
}
