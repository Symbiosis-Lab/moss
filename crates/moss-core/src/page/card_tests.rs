use super::*;
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

fn page(url_path: &str, folder: bool) -> PageFacts {
    PageFacts { url_path: url_path.into(), is_folder_index: folder, label: url_path.into(), ..PageFacts::default() }
}

#[test]
fn a_folder_counts_its_pages_and_sorts_by_its_latest_child() {
    let mut a = page("notes/a.html", false);
    a.date = Some("2026-03-01".into());
    let mut b = page("notes/deeper/b.html", false);
    b.date = Some("2026-05-01".into());
    let all = vec![page("notes/index.html", true), a, page("notes/deeper/index.html", true), b];
    let props = props_for(&all[0], &all, &Host, None);
    assert_eq!(props.child_count, Some(2));
    assert!(props.subfolder);
    assert_eq!(props.date_raw.as_deref(), Some("2026-05-01"));
    assert_eq!(props.date_display, None, "a folder shows only its own date");
}

#[test]
fn an_event_card_carries_its_time_place_and_open_tickets() {
    let mut e = page("events/talk.html", false);
    e.event = crate::event::EventSpan::from_fields(Some("2026-11-01 14:00"), Some("2026-11-01 16:00"));
    e.date = e.event.map(|s| s.start.sort_key());
    e.card = CardFacts { location: vec!["Harbor".into()], tickets: Some("https://t.test".into()), ..CardFacts::default() };
    let props = props_for(&e, std::slice::from_ref(&e), &Host, None);
    assert_eq!(props.place.as_deref(), Some("Harbor"));
    assert_eq!(props.tickets.as_deref(), Some("https://t.test"));
    assert!(props.when_html.unwrap().contains("moss-event-upcoming"));
    assert!(matches!(props.when, Some(EventWhen::Upcoming { .. })));
    assert_eq!(props.url.as_deref(), Some("/events/talk"));
}

#[test]
fn an_outlet_takes_the_link_and_the_page_keeps_its_permalink() {
    let mut p = page("links/a.html", false);
    p.card = CardFacts { outlet: outlet_href(Some("https://example.test/post"), false), cover: Some("img/a.png|crop:top".into()), ..CardFacts::default() };
    let props = props_for(&p, std::slice::from_ref(&p), &Host, None);
    assert_eq!(props.url.as_deref(), Some("https://example.test/post"));
    assert_eq!(props.permalink.as_deref(), Some("/links/a"));
    assert_eq!(props.cover.as_deref(), Some("/img/a.png|crop:top"));
    assert_eq!(props.cover_type, Some(CoverType::Image));
}

#[test]
fn only_a_pageless_entry_may_link_to_a_path_on_the_site() {
    assert_eq!(outlet_href(Some("/files/a.pdf"), true).as_deref(), Some("/files/a.pdf"));
    assert_eq!(outlet_href(Some("/files/a.pdf"), false), None);
    assert_eq!(outlet_href(Some("//evil.test"), true), None);
}

#[test]
fn a_cover_type_comes_from_the_field_then_the_extension() {
    assert_eq!(detect_cover_type("a.mp4?x=1", None), CoverType::Video);
    assert_eq!(detect_cover_type("a.html", None), CoverType::Iframe);
    assert_eq!(detect_cover_type("a.mp4", Some("image")), CoverType::Image);
}
