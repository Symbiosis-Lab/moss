use super::*;
use moss_core::event::EventTime;
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

fn time_element(d: &ParsedDocument, lang: Language) -> String {
    let html = render(d, lang, false, None, UpcomingLabel::Hidden, chrono::NaiveDateTime::default()).unwrap();
    let from = html.find(r#"<time class="moss-event-time""#).unwrap();
    let to = from + html[from..].find("</time>").unwrap() + "</time>".len();
    html[from..to].to_string()
}

fn time_text(d: &ParsedDocument, lang: Language) -> String {
    let element = time_element(d, lang);
    let inner = &element[element.find('>').unwrap() + 1..element.len() - "</time>".len()];
    let mut text = String::new();
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}

#[test]
fn the_page_time_carries_the_listing_date_parts() {
    let d = doc("2026-11-01 14:00", Some("2026-11-01 16:00"));
    let element = time_element(&d, Language::En);
    assert!(element.contains(r#"<span class="moss-when-day">1</span>"#), "{element}");
    assert!(element.contains(r#"<span class="moss-when-time">2:00–4:00 PM</span>"#), "{element}");
    assert_eq!(time_text(&d, Language::En), "Sunday, November 1, 2026, 2:00–4:00 PM");
}

#[test]
fn unsafe_link_schemes_are_not_linked() {
    let mut d = doc("2026-11-01", None);
    d.event.as_mut().unwrap().tickets = Some("javascript:alert(1)".into());
    assert!(!render(&d, Language::En, false, None, UpcomingLabel::Hidden, chrono::NaiveDateTime::default()).unwrap().contains("<a "));
}

#[test]
fn tickets_and_online_links_each_carry_their_own_class() {
    let mut d = doc("2026-11-01", None);
    d.event.as_mut().unwrap().tickets = Some("https://example.test/tickets".into());
    d.event.as_mut().unwrap().online = Some("https://example.test/live".into());
    let html = render(&d, Language::En, false, None, UpcomingLabel::Hidden, chrono::NaiveDateTime::default()).unwrap();
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
fn a_past_event_drops_tickets_and_keeps_online() {
    let mut d = doc("2026-11-01 14:00", Some("2026-11-01 16:00"));
    d.event.as_mut().unwrap().tickets = Some("https://example.test/tickets".into());
    d.event.as_mut().unwrap().online = Some("https://example.test/recording".into());
    let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
    let meta = |now| render(&d, Language::En, false, Some("/e.ics"), UpcomingLabel::Shown, now).unwrap();

    let before = meta(at("2026-11-01 15:59"));
    assert!(before.contains("moss-event-tickets") && before.contains("moss-event-calendar"), "{before}");

    let after = meta(at("2026-11-01 16:00"));
    assert!(after.contains(r#"data-when="past""#), "{after}");
    assert!(!after.contains("moss-event-tickets"), "{after}");
    assert!(after.contains("moss-event-online"), "{after}");
}

#[test]
fn an_event_page_prints_its_place_only_when_no_place_line_names_it() {
    let meta = |d: &ParsedDocument| render(d, Language::En, false, None, UpcomingLabel::Hidden, chrono::NaiveDateTime::default()).unwrap();
    let mut d = doc("2026-11-01", None);
    d.location = vec!["Harbor".to_string()];
    assert!(meta(&d).contains(r#"<span class="moss-event-place">Harbor</span>"#), "{}", meta(&d));
    d.term_lines = vec![crate::build::terms::TermLine {
        term: "places".into(),
        field: "location".into(),
        label: "Location".into(),
        separator: ": ".into(),
        links: "[Harbor](/places/harbor/)".into(),
        is_place: true,
    }];
    assert!(!meta(&d).contains("moss-event-place"), "{}", meta(&d));
}

#[test]
fn a_page_without_an_event_renders_nothing() {
    assert!(render(&ParsedDocument::default(), Language::En, false, None, UpcomingLabel::Hidden, chrono::NaiveDateTime::default()).is_none());
}
