use crate::ast::{parse, Block};

fn blocks(md: &str) -> Vec<Block> {
    parse(md).blocks
}

/// The plain form and the fenced form must build the same block.
fn same_as_fenced(plain: &str, fenced_body: &str) {
    let fenced = blocks(&format!(":::gallery rows\n{fenced_body}\n:::"));
    assert_eq!(blocks(plain), fenced, "plain: {plain:?}");
}

#[test]
fn two_pictures_on_lines_match_the_fenced_gallery() {
    same_as_fenced(
        "![The entrance door](door.jpg)\n![The room](room.jpg)",
        "![The entrance door](door.jpg)\n![The room](room.jpg)",
    );
}

#[test]
fn wiki_embed_pipe_text_is_the_alt_in_both_forms() {
    same_as_fenced("![[a.jpg|Dusk]]\n![[b.jpg|Dawn]]", "![[a.jpg|Dusk]]\n![[b.jpg|Dawn]]");
    same_as_fenced("![[a.jpg|Dusk|cover top]]\n![[b.jpg]]", "![[a.jpg|Dusk|cover top]]\n![[b.jpg]]");
}

#[test]
fn pictures_on_one_line_match() {
    same_as_fenced("![A](a.jpg) ![B](b.jpg)", "![A](a.jpg)\n![B](b.jpg)");
}

#[test]
fn wiki_embeds_match() {
    same_as_fenced("![[a.jpg]]\n![[b.png]]", "![[a.jpg]]\n![[b.png]]");
}

#[test]
fn italic_line_under_a_picture_is_its_caption() {
    same_as_fenced(
        "![A](a.jpg)\n*Dusk, [quay](q.md)*\n![B](b.jpg)\n*Dawn*",
        "![A](a.jpg)\n*Dusk, [quay](q.md)*\n![B](b.jpg)\n*Dawn*",
    );
}

#[test]
fn italic_line_under_several_pictures_on_one_line_captions_the_row() {
    match &blocks("![A](a.jpg) ![B](b.jpg) ![C](c.jpg)\n*Three covers*")[..] {
        [Block::Shortcode(crate::ast::Shortcode::Gallery(g))] => {
            assert_eq!(g.items.len(), 3);
            assert!(g.items.iter().all(|i| i.caption.is_empty()), "no picture owns it: {g:?}");
            assert!(!g.caption.is_empty(), "the row owns it: {g:?}");
        }
        other => panic!("expected one gallery, got {other:?}"),
    }
}

#[test]
fn bare_pipe_keeps_alt_only() {
    same_as_fenced("![A door|](a.jpg)\n![B](b.jpg)", "![A door|](a.jpg)\n![B](b.jpg)");
}

#[test]
fn a_gallery_is_made_of_rows() {
    match &blocks("![A](a.jpg)\n![B](b.jpg)")[..] {
        [Block::Shortcode(crate::ast::Shortcode::Gallery(g))] => {
            assert!(g.rows);
            assert_eq!(g.items.len(), 2);
        }
        other => panic!("expected one gallery, got {other:?}"),
    }
}

#[test]
fn what_stays_a_paragraph() {
    for md in [
        "![A](a.jpg)",                                  // one picture is a figure already
        "![A](a.jpg) and ![B](b.jpg)",                  // prose between
        "Look: ![A](a.jpg)\n![B](b.jpg)",               // prose before
        "[![A](a.jpg)](p/)\n![B](b.jpg)",               // linked picture
        "![A](a.jpg)\n![B](clip.mp4)",                  // video
        "![A|55%](a.jpg)\n![B](b.jpg)",                 // asks for a width
        "![A](a.jpg) *beta*\n![B](b.jpg)",              // italic on the picture's own line
        "![A](a.jpg)\n*one*\n*two*\n![B](b.jpg)",       // two captions
    ] {
        assert!(
            blocks(md).iter().all(|b| !matches!(b, Block::Shortcode(_))),
            "should stay a paragraph: {md:?}"
        );
    }
}

#[test]
fn a_grid_cell_takes_the_rule_and_a_lone_picture_does_not() {
    let cells = |md: &str| match blocks(md).into_iter().next() {
        Some(Block::Shortcode(crate::ast::Shortcode::Grid(g))) => g.cells,
        other => panic!("expected a grid, got {other:?}"),
    };
    let plain = cells(":::grid 2\n![A](a.jpg)\n![B](b.jpg)\n+++\ntext\n:::");
    let fenced = cells(":::grid 2\n:::gallery rows\n![A](a.jpg)\n![B](b.jpg)\n:::\n+++\ntext\n:::");
    assert_eq!(plain[0], fenced[0]);
    let single = cells(":::grid 2\n![A](a.jpg)\n+++\ntext\n:::");
    assert!(matches!(single[0][0], Block::Figure { .. }), "{:?}", single[0]);
}

/// A picture alone on a line of a mixed paragraph.
mod lines {
    use crate::ast::node::{Block, Inline};
    use crate::ast::parser::{parse_fragment_with_config, parse_with_config, ParseConfig};

    fn breaks() -> ParseConfig {
        ParseConfig { hard_line_breaks: true, emit_source_lines: true, ..Default::default() }
    }

    fn blocks(md: &str) -> Vec<Block> {
        parse_with_config(md, &breaks()).blocks
    }

    /// One letter per block: `t` a text paragraph, `P` a picture paragraph,
    /// `F` a figure, `G` a gallery, `q` a quote, `?` anything else.
    fn shape(blocks: &[Block]) -> String {
        blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(i) if i.iter().any(|x| matches!(x, Inline::Text(t) if !t.trim().is_empty())) => 't',
                Block::Paragraph(_) => 'P',
                Block::Figure { .. } => 'F',
                Block::Shortcode(_) => 'G',
                Block::BlockQuote(_) => 'q',
                _ => '?',
            })
            .collect()
    }

    #[test]
    fn a_picture_on_the_line_after_text_is_its_own_paragraph() {
        assert_eq!(shape(&blocks("Some words.\n![](a.jpg)")), "tP");
        assert_eq!(shape(&blocks("![](a.jpg)\nSome words.")), "Pt");
        assert_eq!(shape(&blocks("One.\n![](a.jpg)\nTwo.")), "tPt");
    }

    #[test]
    fn the_split_matches_a_blank_line() {
        let plain = ParseConfig { hard_line_breaks: true, ..Default::default() };
        for (split, blank) in [
            ("Some words.\n![](a.jpg)", "Some words.\n\n![](a.jpg)"),
            ("![](a.jpg)\nSome words.", "![](a.jpg)\n\nSome words."),
            ("One.\nTwo.\n![](a.jpg)\nThree.", "One.\nTwo.\n\n![](a.jpg)\n\nThree."),
            ("Some words.\n![[a.jpg]]", "Some words.\n\n![[a.jpg]]"),
            ("Some words.\n![[clip.mp4]]", "Some words.\n\n![[clip.mp4]]"),
            ("Some words.\n![](a.jpg)\n![](b.jpg)", "Some words.\n\n![](a.jpg)\n![](b.jpg)"),
            ("Intro.\n![A](a.jpg)\n*Dusk*", "Intro.\n\n![A](a.jpg)\n*Dusk*"),
            ("![A](a.jpg)\n*Dusk*\nAfter.", "![A](a.jpg)\n*Dusk*\n\nAfter."),
            ("Intro.\n[![A](a.jpg)](https://example.com)\n*Dusk*", "Intro.\n\n[![A](a.jpg)](https://example.com)\n*Dusk*"),
            ("Intro.\n![[a.jpg]]\n*Dusk*", "Intro.\n\n![[a.jpg]]\n*Dusk*"),
            ("Intro.\n![[clip.mp4]]\n*Dusk*", "Intro.\n\n![[clip.mp4]]\n*Dusk*"),
            ("Intro.\n![A](a.jpg)\n*Dusk*\n![B](b.jpg)\n*Dawn*", "Intro.\n\n![A](a.jpg)\n*Dusk*\n![B](b.jpg)\n*Dawn*"),
        ] {
            assert_eq!(parse_with_config(split, &plain).blocks, parse_with_config(blank, &plain).blocks, "{split:?}");
        }
    }

    #[test]
    fn a_split_captioned_picture_honours_implicit_figure_off() {
        let off = ParseConfig { hard_line_breaks: true, implicit_figure: false, ..Default::default() };
        let (split, blank) = ("Intro.\n![A](a.jpg)\n*Dusk*", "Intro.\n\n![A](a.jpg)\n*Dusk*");
        let doc = parse_with_config(split, &off);
        assert_eq!(shape(&doc.blocks), "tP");
        assert_eq!(doc.blocks, parse_with_config(blank, &off).blocks);
    }

    #[test]
    fn only_an_italic_line_directly_under_a_picture_is_its_caption() {
        assert_eq!(shape(&blocks("![A](a.jpg)\n*One*\n*Two*\nAfter.")), "Ft");
        // The italic line above stays a paragraph of its own, not a caption.
        assert_eq!(shape(&blocks("*Above*\n![A](a.jpg)\nAfter.")), "PPt");
    }

    #[test]
    fn text_lines_around_a_picture_keep_their_breaks() {
        let b = blocks("One.\nTwo.\n![](a.jpg)");
        assert_eq!(shape(&b), "tP");
        let Block::Paragraph(text) = &b[0] else { panic!("{b:?}") };
        assert_eq!(text.iter().filter(|i| matches!(i, Inline::LineBreak)).count(), 1);
    }

    #[test]
    fn a_split_picture_keeps_its_alt_and_gets_no_caption() {
        let b = blocks("Some words.\n![A heron](a.jpg)");
        assert_eq!(shape(&b), "tP");
        let Block::Paragraph(p) = &b[1] else { panic!("{b:?}") };
        assert!(matches!(&p[..], [Inline::Image { alt, .. }] if alt == "A heron"), "{p:?}");
    }

    #[test]
    fn linked_pictures_and_wiki_embeds_split_too() {
        assert_eq!(shape(&blocks("Some words.\n[![](a.jpg)](https://example.com)")), "tP");
        assert_eq!(shape(&blocks("Some words.\n![[a.jpg]]")), "tP");
        assert_eq!(shape(&blocks("Some words.\n![[clip.mp4]]")), "tP");
    }

    #[test]
    fn a_run_of_picture_lines_is_a_row() {
        assert_eq!(shape(&blocks("Some words.\n![](a.jpg)\n![](b.jpg)")), "tG");
    }

    #[test]
    fn a_picture_beside_text_on_its_line_stays() {
        assert_eq!(shape(&blocks("Some words ![](a.jpg) more.")), "t");
        assert_eq!(shape(&blocks("![](a.jpg) a caption on the same line\nNext line.")), "t");
    }

    #[test]
    fn a_soft_break_is_a_space_and_does_not_split() {
        let plain = ParseConfig::default();
        assert_eq!(shape(&parse_with_config("Some words.\n![](a.jpg)", &plain).blocks), "t");
    }

    #[test]
    fn a_picture_with_its_italic_caption_line_is_still_a_figure() {
        assert_eq!(shape(&blocks("![A heron](a.jpg)\n*At dusk*")), "F");
    }

    #[test]
    fn each_piece_carries_its_own_source_line() {
        let doc = parse_with_config("Intro.\n\nOne.\n*across\nlines*\n![](a.jpg)\nThree.", &breaks());
        assert_eq!(shape(&doc.blocks), "ttPt");
        let lines: Vec<_> = doc.block_meta.iter().map(|m| m.source_line).collect();
        assert_eq!(lines, [Some(1), Some(3), Some(6), Some(7)]);

        // CommonMark breaks: the soft break is a space, the backslash break splits.
        let plain = ParseConfig { emit_source_lines: true, ..Default::default() };
        let doc = parse_with_config("One.\nTwo.\\\n![](a.jpg)", &plain);
        assert_eq!(shape(&doc.blocks), "tP");
        let lines: Vec<_> = doc.block_meta.iter().map(|m| m.source_line).collect();
        assert_eq!(lines, [Some(1), Some(3)]);
    }

    #[test]
    fn only_top_level_paragraphs_split() {
        assert_eq!(shape(&blocks("> Some words.\n> ![](a.jpg)")), "q");
        assert_eq!(shape(&parse_fragment_with_config("Some words.\n![](a.jpg)", &breaks()).blocks), "t");
    }
}
