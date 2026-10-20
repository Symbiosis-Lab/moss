use super::*;
use moss_core::date::{parse_edtf, Edtf};
use moss_core::fields::{FieldKind, PageLink};

fn decl(name: &str, kind: FieldKind, place: Place) -> FieldDecl {
    FieldDecl { name: name.into(), kind, choices: vec![], place: Some(place), label: vec![], scope: vec![] }
}

fn period(text: &str) -> TypedValue {
    match parse_edtf(text) {
        Some(Edtf::Interval { start, end }) => TypedValue::Period { start, open: end.is_none(), end, text: text.into() },
        other => panic!("not a period: {other:?}"),
    }
}

fn no_pages(_: &str) -> Option<String> {
    None
}

fn text(s: &str) -> TypedValue {
    TypedValue::Text(s.into())
}

#[test]
fn an_eyebrow_holds_its_fields_in_declaration_order_separated_by_a_space() {
    let placed = [
        (decl("kind", FieldKind::Choice, Place::Eyebrow), TypedValue::Choice { value: "Building".into(), label: "Built".into() }),
        (decl("years", FieldKind::Period, Place::Eyebrow), period("1927/1933")),
    ];
    let out = draw(&placed, Language::En, None, &no_pages);
    assert_eq!(
        out.eyebrow,
        "<p class=\"moss-eyebrow\"><span data-field=\"kind\">Built</span> <span data-field=\"years\"><time datetime=\"1927/1933\">1927–1933</time></span></p>\n"
    );
    assert_eq!((out.subtitle.as_str(), out.after_masthead().as_str()), ("", ""));
}

#[test]
fn an_open_period_reads_in_the_pages_language_and_keeps_an_open_datetime() {
    let placed = [(decl("years", FieldKind::Period, Place::Eyebrow), period("1923/.."))];
    let out = draw(&placed, Language::De, None, &no_pages);
    assert!(out.eyebrow.contains("<time datetime=\"1923/..\">ab 1923</time>"), "{}", out.eyebrow);
}

#[test]
fn a_count_is_a_data_element_then_its_noun_and_a_bare_count_has_no_span() {
    let count = |n: i64, noun: Option<&str>| TypedValue::Count { number: Some(n.into()), noun: noun.map(String::from), text: String::new() };
    let placed = [
        (decl("figure", FieldKind::Count, Place::KeyFigure), count(1382, Some("flats"))),
        (decl("units", FieldKind::Count, Place::KeyFigure), count(40, None)),
    ];
    let out = draw(&placed, Language::En, None, &no_pages);
    assert_eq!(
        out.key_figures,
        "<p class=\"moss-key-figure\" data-field=\"figure\"><data value=\"1382\">1,382</data> <span>flats</span></p>\n\
         <p class=\"moss-key-figure\" data-field=\"units\"><data value=\"40\">40</data></p>\n"
    );
}

#[test]
fn only_the_first_subtitle_is_drawn() {
    let placed = [
        (decl("district", FieldKind::Text, Place::Subtitle), text("Eastbank")),
        (decl("tagline", FieldKind::Text, Place::Subtitle), text("Other")),
    ];
    let out = draw(&placed, Language::En, None, &no_pages);
    assert_eq!(out.eyebrow, "");
    assert_eq!(out.subtitle, "<p class=\"moss-subtitle\" data-field=\"district\">Eastbank</p>\n");
}

#[test]
fn facts_use_the_label_or_the_capitalised_name_and_link_pages_by_the_text_as_written() {
    let mut architect = decl("architect", FieldKind::Page, Place::Facts);
    architect.label = vec![("en".into(), "Architect".into()), ("de".into(), "Architektin".into())];
    let page = |t: &str, alias: Option<&str>| PageLink { target: t.into(), alias: alias.map(String::from) };
    let placed = [
        (architect, TypedValue::Page(page("People/Anna Weber", None))),
        (
            decl("partners", FieldKind::Pages, Place::Facts),
            TypedValue::Pages(vec![page("anna", Some("Dr. Weber")), page("Missing <One>", None)]),
        ),
    ];
    let href = |t: &str| (t == "anna" || t.ends_with("Anna Weber")).then(|| "/people/anna-weber/".to_string());
    let en = draw(&placed, Language::En, None, &href);
    assert_eq!(
        en.facts,
        "<dl class=\"moss-facts\" data-beside>\n\
         <div data-field=\"architect\"><dt>Architect</dt><dd><a href=\"/people/anna-weber/\">Anna Weber</a></dd></div>\n\
         <div data-field=\"partners\"><dt>Partners</dt><dd><a href=\"/people/anna-weber/\">Dr. Weber</a>, Missing &lt;One&gt;</dd></div>\n\
         </dl>\n"
    );
    let de = draw(&placed, Language::De, None, &href);
    assert!(de.facts.contains("<dt>Architektin</dt>"), "{}", de.facts);
}

#[test]
fn an_unplaced_or_empty_field_draws_nothing() {
    let mut unplaced = decl("note", FieldKind::Text, Place::Facts);
    unplaced.place = None;
    let placed = [(unplaced, text("x")), (decl("blank", FieldKind::Pages, Place::Facts), TypedValue::Pages(vec![]))];
    assert_eq!(draw(&placed, Language::En, None, &no_pages), Placements::default());
}

mod title_block_after_picture {
    use super::super::{after_leading_picture, mark_body_start, BODY_START};

    const TITLE: &str = "<p class=\"moss-eyebrow\"><span data-field=\"stage\">Pier 4</span></p>\n<h1 class=\"moss-article-title\">Harbour</h1>\n";

    fn page(body: &str) -> String {
        let mut html = format!("{TITLE}{body}");
        mark_body_start(&mut html);
        html
    }

    #[test]
    fn the_marker_goes_right_after_the_title_and_the_eyebrow_stays_above_it() {
        let html = page("<p>x</p>");
        assert_eq!(html, format!("{}{BODY_START}\n<p>x</p>", TITLE.trim_end_matches('\n')));
    }

    #[test]
    fn a_body_without_moss_title_gets_no_marker() {
        let mut html = "<h1>Authored</h1><p>x</p>".to_string();
        mark_body_start(&mut html);
        assert!(!html.contains(BODY_START));
    }

    #[test]
    fn the_title_block_moves_past_leading_blocks_to_after_the_first_figure() {
        let html = page("\n<!-- a note -->\n<div class=\"trail\"><divider></divider><div><p>Road</p></div></div>\n<figure class=\"moss-image\"><img src=\"a.jpg\"><figcaption>Dusk</figcaption></figure>\n<blockquote><p>Note</p></blockquote>\n");
        let out = after_leading_picture(&html);
        let at = |s: &str| out.find(s).unwrap_or_else(|| panic!("{s} missing from {out}"));
        assert!(at("trail") < at("<figure") && at("</figure>") < at("moss-eyebrow") && at("moss-eyebrow") < at("<h1") && at("<h1") < at("<blockquote>"), "{out}");
        assert!(!out.contains(BODY_START));
    }

    #[test]
    fn prose_or_a_heading_before_any_picture_keeps_the_title_block_first() {
        let picture = "<figure class=\"moss-image\"><img></figure>";
        let bodies = [
            format!("\n<p>Text first</p>\n{picture}"),
            format!("\n<h2>Section</h2>\n{picture}"),
            format!("\n<section><p>Prose</p></section>\n{picture}"),
            "\n<figure class=\"moss-place-map\"><svg></svg></figure>\n<figure class=\"moss-image\"><img></figure>".to_string(),
            "\n<p>No picture</p>".to_string(),
        ];
        for body in &bodies {
            let out = after_leading_picture(&page(body));
            assert!(out.starts_with(TITLE.trim_end_matches('\n')), "{out}");
            assert!(!out.contains(BODY_START));
        }
    }

    #[test]
    fn beside_wraps_the_picture_and_the_title_block_in_one_div_after_leading_divs() {
        use super::super::beside_leading_picture;
        let html = page("\n<div class=\"trail\"><p>Road</p></div>\n<figure class=\"moss-image\"><img></figure>\n<p>Text</p>");
        let out = beside_leading_picture(&html);
        let at = |s: &str| out.find(s).unwrap_or_else(|| panic!("{s} missing from {out}"));
        assert!(at("trail") < at("<div class=\"moss-title-beside\"><figure") && at("</figure>") < at("moss-eyebrow") && at("</h1>") < at("</div></div>") && at("</div></div>") < at("<p>Text</p>"), "{out}");
        assert!(!out.contains(BODY_START));
    }

    #[test]
    fn beside_without_a_leading_picture_keeps_the_title_block_first() {
        use super::super::beside_leading_picture;
        let out = beside_leading_picture(&page("\n<p>Text first</p>\n<figure class=\"moss-image\"><img></figure>"));
        assert!(out.starts_with(TITLE.trim_end_matches('\n')) && !out.contains("moss-title-beside") && !out.contains(BODY_START), "{out}");
    }

    #[test]
    fn an_unclosed_block_keeps_the_title_block_first() {
        let out = after_leading_picture(&page("\n<div class=\"x\"><figure class=\"moss-image\"><img></figure>"));
        assert!(out.starts_with("<p class=\"moss-eyebrow\""), "{out}");
    }
}

#[test]
fn a_folder_index_and_a_slot_keep_their_title_first() {
    use crate::build::components::field_places::title_block_after_picture;
    let page = ParsedDocument { title_block: Some("after-picture".into()), kind: moss_core::page_kind::PageKind::Article, ..Default::default() };
    assert!(title_block_after_picture(&page));
    assert!(!title_block_after_picture(&ParsedDocument { slot_only: true, ..page.clone() }));
    assert!(!title_block_after_picture(&ParsedDocument { kind: moss_core::page_kind::PageKind::Folder, ..page.clone() }));
    assert!(!title_block_after_picture(&ParsedDocument { title_block: None, ..page }));
}

#[test]
fn beside_picture_follows_the_same_page_rules_as_after_picture() {
    use crate::build::components::field_places::{title_block_beside_picture, title_block_follows_picture};
    let page = ParsedDocument { title_block: Some("beside-picture".into()), kind: moss_core::page_kind::PageKind::Article, ..Default::default() };
    assert!(title_block_beside_picture(&page) && title_block_follows_picture(&page));
    assert!(!title_block_beside_picture(&ParsedDocument { slot_only: true, ..page.clone() }));
    assert!(!title_block_beside_picture(&ParsedDocument { kind: moss_core::page_kind::PageKind::Folder, ..page.clone() }));
    assert!(!title_block_beside_picture(&ParsedDocument { title_block: Some("after-picture".into()), ..page }));
}
