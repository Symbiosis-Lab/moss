use super::*;
use crate::build::types::EventFields;

fn doc(start: &str, end: Option<&str>) -> ParsedDocument {
    ParsedDocument {
        event: Some(EventFields {
            start: EventTime::parse(start).unwrap(),
            when: EventTime::parse(start).unwrap().sort_key(),
            end: end.map(|e| EventTime::parse(e).unwrap()),
            timezone: None,
            status: None,
            tickets: None,
            online: None,
        }),
        ..Default::default()
    }
}

fn time_text(d: &ParsedDocument, lang: Language) -> String {
    let html = render(d, lang, false, None).unwrap();
    let from = html.find("\">").unwrap() + 2;
    let from = html[from..].find("\">").unwrap() + from + 2;
    html[from..html.find("</time>").unwrap()].to_string()
}

#[test]
fn clock_collapses_the_meridiem_only_when_shared() {
    let d = doc("2026-11-01 11:00", Some("2026-11-01 13:00"));
    assert_eq!(time_text(&d, Language::En), "Sunday, November 1, 2026, 11:00 AM–1:00 PM");
}

#[test]
fn midnight_and_noon_read_as_twelve() {
    let d = doc("2026-11-01 00:30", None);
    assert_eq!(time_text(&d, Language::En), "Sunday, November 1, 2026, 12:30 AM");
}

#[test]
fn chinese_uses_a_24_hour_clock_and_weekday_after_the_date() {
    let d = doc("2026-11-01 14:00", Some("2026-11-01 16:00"));
    assert_eq!(time_text(&d, Language::ZhHans), "2026年11月1日 星期日 14:00–16:00");
}

#[test]
fn unsafe_link_schemes_are_not_linked() {
    let mut d = doc("2026-11-01", None);
    d.event.as_mut().unwrap().tickets = Some("javascript:alert(1)".into());
    assert!(!render(&d, Language::En, false, None).unwrap().contains("<a "));
}

#[test]
fn tickets_and_online_links_each_carry_their_own_class() {
    let mut d = doc("2026-11-01", None);
    d.event.as_mut().unwrap().tickets = Some("https://example.test/tickets".into());
    d.event.as_mut().unwrap().online = Some("https://example.test/live".into());
    let html = render(&d, Language::En, false, None).unwrap();
    assert!(
        html.contains(r#"<a class="moss-event-link moss-event-tickets" href="https://example.test/tickets">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="moss-event-link moss-event-online" href="https://example.test/live">"#),
        "{html}"
    );
}

#[test]
fn a_page_without_an_event_renders_nothing() {
    assert!(render(&ParsedDocument::default(), Language::En, false, None).is_none());
}

#[test]
fn end_before_start_is_treated_as_no_end() {
    let d = doc("2026-11-01 14:00", Some("2026-11-01 09:00"));
    assert_eq!(time_text(&d, Language::En), "Sunday, November 1, 2026, 2:00 PM");
}
