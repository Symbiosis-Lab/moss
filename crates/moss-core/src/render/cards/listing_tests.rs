use super::*;
use crate::page::card::{SectionAccent, TermCardFacts};
use crate::page::site_page::PageFacts;

struct Host;

impl CardHost<PageFacts> for Host {
    fn resolve_url(&self, path: &str) -> String {
        format!("/{}", path.trim_start_matches('/'))
    }
    fn typesetting(&self) -> Option<&str> {
        None
    }
    fn now(&self) -> &str {
        "2026-11-01T15:00"
    }
    fn term_card(&self, _: &PageFacts) -> Option<TermCardFacts> { None }
    fn place_names(&self, _: &PageFacts) -> Option<String> { None }
    fn credits(&self, _: &PageFacts) -> Option<String> { None }
    fn section(&self, _: &PageFacts) -> Option<SectionAccent> { None }
}

fn card(url_path: &str, date: Option<&str>) -> ChildItemProps {
    ChildItemProps {
        title: url_path.into(),
        url: Some(format!("/{url_path}")),
        url_path: url_path.into(),
        date_raw: date.map(str::to_string),
        date_display: date.map(str::to_string),
        ..ChildItemProps::default()
    }
}

fn folder(url_path: &str) -> ChildItemProps {
    ChildItemProps { subfolder: true, child_count: Some(1), ..card(url_path, None) }
}

fn ask<'a>(style: &'a str, group: &'a str, upcoming: &'a HashSet<&'a str>, placement: &'a crate::media::Placement) -> RowsAsk<'a> {
    RowsAsk { style, group, pre_ordered: false, sort: None, upcoming, is_embed: false, placement, lazy_cards: false }
}

fn order(arranged: &Arranged<'_>) -> Vec<String> {
    arranged.flat().iter().map(|c| c.url_path.clone()).collect()
}

#[test]
fn folders_lead_and_pages_list_newest_first() {
    let items = vec![card("a.html", Some("2025-01-01")), folder("f/index.html"), card("b.html", Some("2026-01-01"))];
    let (none, place) = (HashSet::new(), Default::default());
    assert_eq!(order(&arrange(&items, &ask("list", "none", &none, &place))), ["f/index.html", "b.html", "a.html"]);
}

#[test]
fn upcoming_events_lead_soonest_first_in_their_own_section() {
    let items = vec![card("past.html", Some("2026-01-01")), card("later.html", Some("2026-12-20")), card("soon.html", Some("2026-11-20"))];
    let upcoming: HashSet<&str> = ["later.html", "soon.html"].into();
    let place = Default::default();
    let ask = ask("list", "upcoming", &upcoming, &place);
    assert_eq!(order(&arrange(&items, &ask)), ["soon.html", "later.html", "past.html"]);
    let html = rows_html(&items, &ask, &(), Language::En, None);
    let (soon, earlier) = (html.find("data-group=\"upcoming\"").unwrap(), html.find("data-group=\"earlier\"").unwrap());
    assert!(soon < html.find("soon.html").unwrap() && html.find("later.html").unwrap() < earlier && earlier < html.find("past.html").unwrap(), "{html}");
}

#[test]
fn year_sections_keep_an_order_that_is_not_by_date_and_put_undated_last() {
    let items = vec![card("x.html", None), card("old.html", Some("2024-05-01")), card("new.html", Some("2026-05-01")), card("old2.html", Some("2024-01-01"))];
    let (none, place) = (HashSet::new(), Default::default());
    let html = rows_html(&items, &RowsAsk { pre_ordered: true, ..ask("list", "year", &none, &place) }, &(), Language::En, None);
    let at = |s: &str| html.find(s).unwrap_or_else(|| panic!("{s} missing from {html}"));
    assert!(at("2024") < at("old.html") && at("old.html") < at("old2.html") && at("old2.html") < at("2026") && at("2026") < at("new.html") && at("new.html") < at("x.html"), "{html}");
}

#[test]
fn a_summary_divides_folders_from_pages_and_a_lead_marks_its_container() {
    let items = vec![folder("f/index.html"), card("a.html", Some("2026-01-01"))];
    let (none, place) = (HashSet::new(), Default::default());
    let html = rows_html(&items, &ask("lead", "none", &none, &place), &(), Language::En, None);
    assert!(html.contains("moss-child-section-divider") && html.contains("data-lead") && html.contains("data-layout=\"list\""), "{html}");
    let list = rows_html(&items, &ask("list", "none", &none, &place), &(), Language::En, None);
    assert!(!list.contains("moss-child-section-divider") && list.contains("data-layout=\"minimal\""), "{list}");
}

#[test]
fn a_row_takes_the_body_s_first_paragraph_and_a_grid_card_does_not() {
    let page = PageFacts { url_path: "a.html".into(), label: "A".into(), ..PageFacts::default() };
    let all = [page.clone()];
    let body = "The first paragraph.\n\nThe second.";
    let row = row_props(&page, &all, &Host, "list", None, body, false);
    assert_eq!(row.description.as_deref(), Some("The first paragraph."));
    assert_eq!(row_props(&page, &all, &Host, "grid", None, body, false).description, None);
}
