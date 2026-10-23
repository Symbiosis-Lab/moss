use super::*;
use crate::ast::parse;

fn cell(md: &str) -> Vec<Block> {
    parse(md).blocks
}

fn picture() -> Vec<Block> {
    cell("![A picture](p.jpg)")
}

fn quote() -> Vec<Block> {
    cell("> Communication is health.\n\n*Montaigne*")
}

fn spans_of(cells: &[Vec<Block>]) -> Vec<(u8, u8)> {
    spans(cells).expect("a collage")
}

#[test]
fn the_wide_spans_follow_the_count() {
    let table: [(usize, u32, &[u8]); 7] = [
        (3, 1, &[3, 3]),
        (4, 1, &[2, 2, 2]),
        (5, 2, &[3, 3, 3, 3]),
        (6, 2, &[2, 2, 2, 3, 3]),
        (7, 2, &[2, 2, 2, 2, 2, 2]),
        (8, 3, &[2, 2, 2, 2, 2, 2, 6]),
        (9, 3, &[2, 2, 2, 2, 2, 2, 3, 3]),
    ];
    for (count, rows, wide) in table {
        let s = spans_of(&vec![picture(); count]);
        assert_eq!(s.iter().map(|(w, _)| *w).collect::<Vec<_>>(), wide, "{count} cells");
        // Every wide row fills the six columns right of the lead; the stylesheet
        // reads the row count off `data-count`.
        assert_eq!(s.iter().map(|(w, _)| u32::from(*w)).sum::<u32>(), 6 * rows, "{count} cells");
    }
    assert!(spans(&vec![picture(); 2]).is_none());
    assert!(spans(&vec![picture(); 10]).is_none());
}

#[test]
fn on_a_phone_pictures_fill_rows_of_three_and_a_short_last_row_widens() {
    let phone = |count| spans_of(&vec![picture(); count]).into_iter().map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!(phone(4), [2, 2, 2]);
    assert_eq!(phone(3), [3, 3]);
    assert_eq!(phone(5), [2, 2, 2, 6]);
    assert_eq!(phone(9), [2, 2, 2, 2, 2, 2, 3, 3]);
}

#[test]
fn a_quote_takes_a_full_phone_row_wherever_it_sits() {
    let phone = |cells: Vec<Vec<Block>>| spans_of(&cells).into_iter().map(|(_, p)| p).collect::<Vec<_>>();
    // Four pictures after the lead and a quote: the pictures make one row of
    // three and a widened one, whatever the quote's place.
    assert_eq!(phone(vec![picture(), quote(), picture(), picture(), picture(), picture()]), [6, 2, 2, 2, 6]);
    assert_eq!(phone(vec![picture(), picture(), picture(), quote(), picture(), picture()]), [2, 2, 6, 2, 6]);
    assert_eq!(phone(vec![picture(), picture(), picture(), picture(), picture(), quote()]), [2, 2, 2, 6, 6]);
    // The wide spans do not move with the quote.
    assert_eq!(spans_of(&[picture(), picture(), quote(), picture()]), [(2, 3), (2, 6), (2, 3)]);
}

#[test]
fn a_quote_cell_is_a_blockquote_then_one_italic_line() {
    assert!(quote_cell(&quote()).is_some());
    assert!(quote_cell(&cell("> Words.\n\n*[Source](https://example.org/)*")).is_some());
    assert!(quote_cell(&cell("> Words.")).is_none());
    assert!(quote_cell(&cell("> Words.\n\nPlain source")).is_none());
    assert!(quote_cell(&cell("> Words.\n\n*Source* and more")).is_none());
    assert!(quote_cell(&cell("Words.\n\n*Source*")).is_none());
}

#[test]
fn a_quote_over_about_140_characters_is_long() {
    let short = quote();
    assert!(!is_long_quote(quote_cell(&short).unwrap().0));
    let long = cell(&format!("> {}\n\n*Source*", "word ".repeat(30)));
    assert!(is_long_quote(quote_cell(&long).unwrap().0));
}

fn render_md(md: &str) -> String {
    let mut doc = parse(md);
    crate::ast::visit_urls_mut(&mut doc, |url| {
        if let crate::ast::Url::Unresolved(s) = url {
            *url = crate::ast::Url::resolved(s.clone(), crate::ast::UrlKind::Internal);
        }
    });
    crate::ast::render_document(&doc, &crate::ast::DefaultHooks::new())
}

#[test]
fn a_collage_writes_its_rows_and_spans_into_the_one_style_attribute() {
    let html = render_md(":::grid 2 1:2 {collage}\n![A](a.jpg)\n+++\n![B](b.jpg)\n+++\n![C](c.jpg)\n:::");
    assert!(
        html.contains(r#"style="--moss-grid-ratio:minmax(0, 1fr) minmax(0, 2fr)" data-collage data-count="3""#),
        "got: {html}"
    );
    assert_eq!(html.matches(r#"data-tile="picture" style="--moss-collage-span:3;--moss-collage-span-sm:3""#).count(), 2, "got: {html}");
    // The lead's tracks are the stylesheet's; it carries no span.
    assert!(html.contains(r#"<div class="moss-grid-card" data-tile="picture"><figure"#), "got: {html}");
}

#[test]
fn a_tile_that_brings_its_own_chrome_keeps_its_span() {
    let mut part = GridCellParts::final_markup(r#"<a class="moss-grid-card" href="/x/">x</a>"#.into());
    part.layout = Some("--moss-collage-span:2".into());
    part.upright = true;
    assert_eq!(part.to_html(), r#"<a class="moss-grid-card" href="/x/" data-upright style="--moss-collage-span:2">x</a>"#);
    part.replace(r#"<article class="moss-card" style="--item-cover-color: red">y</article>"#.into());
    assert_eq!(
        part.to_html(),
        r#"<article class="moss-card" style="--moss-collage-span:2;--item-cover-color: red" data-upright>y</article>"#
    );
}

#[test]
fn a_quote_cell_renders_as_a_quote_figure_whose_source_alone_links() {
    let long = "word ".repeat(30);
    let html = render_md(&format!(
        ":::grid 2\n> Communication is health.\n\n*[Montaigne, 1924](https://example.org/essays/)*\n+++\n> {long}\n\n*Source*\n:::"
    ))
    .replace('\n', "");
    assert!(
        html.contains(r#"<figure class="moss-quote"><blockquote><p>Communication is health.</p></blockquote><figcaption><a href="https://example.org/essays/""#),
        "got: {html}"
    );
    assert!(html.contains(r#"<figure class="moss-quote" data-length="long"><blockquote>"#), "got: {html}");
    assert_eq!(html.matches("<a ").count(), 1, "only the source links: {html}");
}

#[test]
fn a_blockquote_with_an_italic_aside_in_running_prose_stays_a_blockquote() {
    let html = render_md("> Words.\n\n*An aside*");
    assert!(!html.contains("moss-quote"), "got: {html}");
}

#[test]
fn a_tile_is_a_picture_only_when_the_picture_is_all_it_holds() {
    assert_eq!(TileKind::of(&picture()), TileKind::Picture);
    assert_eq!(TileKind::of(&cell("![Credit](p.jpg)\n*Credit*")), TileKind::Picture);
    assert_eq!(TileKind::of(&cell("[![A](p.jpg)](/x/)")), TileKind::Picture);
    assert_eq!(TileKind::of(&quote()), TileKind::Quote);
    // A passage with a picture further down is text: it takes a full phone row
    // and never the picture's ratio.
    let text = cell("Some words first.\n\n![A picture](p.jpg)");
    assert_eq!(TileKind::of(&text), TileKind::Text);
    assert_eq!(spans_of(&[picture(), picture(), text, picture()]), [(2, 3), (2, 6), (2, 3)]);
    let html = render_md(":::grid {collage}\n![A](a.jpg)\n+++\nWords.\n\n![B](b.jpg)\n+++\n![C](c.jpg)\n:::");
    assert!(html.contains(r#"<div class="moss-grid-card" data-tile="text" style="--moss-collage-span:3;--moss-collage-span-sm:6">"#), "got: {html}");
}
