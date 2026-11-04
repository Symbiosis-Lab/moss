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
