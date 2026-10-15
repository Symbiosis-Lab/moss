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
fn an_eyebrow_holds_its_fields_in_declaration_order_without_a_separator() {
    let placed = [
        (decl("kind", FieldKind::Choice, Place::Eyebrow), TypedValue::Choice { value: "Building".into(), label: "Built".into() }),
        (decl("years", FieldKind::Period, Place::Eyebrow), period("1927/1933")),
    ];
    let out = draw(&placed, Language::En, None, &no_pages);
    assert_eq!(
        out.eyebrow,
        "<p class=\"moss-eyebrow\"><span data-field=\"kind\">Built</span><span data-field=\"years\"><time datetime=\"1927/1933\">1927–1933</time></span></p>\n"
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
