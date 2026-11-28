use super::*;

fn span(start: &str, end: Option<&str>) -> (EventTime, Option<EventTime>) {
    (EventTime::parse(start).unwrap(), end.map(|e| EventTime::parse(e).unwrap()))
}

fn when(start: &str, end: Option<&str>, lang: Language) -> String {
    let (s, e) = span(start, end);
    render_when(s, e, lang)
}

fn text(start: &str, end: Option<&str>, lang: Language) -> String {
    let element = when(start, end, lang);
    let mut out = String::new();
    let mut in_tag = false;
    for c in element.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn a_time_carries_its_date_parts_in_their_own_spans() {
    let element = when("2026-11-01 14:00", Some("2026-11-01 16:00"), Language::En);
    assert!(element.contains(r#"<span class="moss-when-day">1</span>"#), "{element}");
    assert!(element.contains(r#"<span class="moss-when-time">2:00–4:00 PM</span>"#), "{element}");
    assert_eq!(text("2026-11-01 14:00", Some("2026-11-01 16:00"), Language::En), "Sunday, November 1, 2026, 2:00–4:00 PM");
}

#[test]
fn clock_collapses_the_meridiem_only_when_shared() {
    assert_eq!(text("2026-11-01 11:00", Some("2026-11-01 13:00"), Language::En), "Sunday, November 1, 2026, 11:00 AM–1:00 PM");
}

#[test]
fn midnight_and_noon_read_as_twelve() {
    assert_eq!(text("2026-11-01 00:30", None, Language::En), "Sunday, November 1, 2026, 12:30 AM");
}

#[test]
fn chinese_uses_a_24_hour_clock_and_weekday_after_the_date() {
    assert_eq!(text("2026-11-01 14:00", Some("2026-11-01 16:00"), Language::ZhHans), "2026年11月1日 星期日 14:00–16:00");
}

#[test]
fn german_puts_the_unit_after_the_last_clock() {
    assert_eq!(text("2026-11-01 14:00", Some("2026-11-01 16:00"), Language::De), "Sonntag, 1. November 2026, 14:00–16:00 Uhr");
}

#[test]
fn end_before_start_is_treated_as_no_end() {
    assert_eq!(text("2026-11-01 14:00", Some("2026-11-01 09:00"), Language::En), "Sunday, November 1, 2026, 2:00 PM");
}

#[test]
fn the_separator_before_a_clock_time_is_its_own_span() {
    let element = when("2026-11-01 14:00", Some("2026-11-01 16:00"), Language::En);
    assert!(
        element.contains(r#"<span class="moss-when-year">2026</span><span class="moss-when-sep">, </span><span class="moss-when-time">"#),
        "{element}"
    );
}

#[test]
fn a_date_only_event_has_no_separator_span() {
    assert!(!when("2026-11-01", None, Language::En).contains("moss-when-sep"));
}

#[test]
fn both_date_to_time_separators_of_a_timed_multi_day_range_are_spans() {
    let element = when("2026-11-01 14:00", Some("2026-11-03 16:00"), Language::En);
    assert_eq!(element.matches(r#"<span class="moss-when-sep">"#).count(), 2, "{element}");
    assert!(
        element.contains(r#"<span class="moss-when-time">2:00 PM</span> – <span class="moss-when-weekday">Tuesday</span>"#),
        "{element}"
    );
    assert!(
        element.contains(r#"<span class="moss-when-year">2026</span><span class="moss-when-sep">, </span><span class="moss-when-time">4:00 PM</span>"#),
        "{element}"
    );
}

#[test]
fn an_event_is_upcoming_until_the_minute_it_ends() {
    let (start, end) = span("2026-11-01 14:00", Some("2026-11-01 16:00"));
    let span = EventSpan { start, end };
    assert!(matches!(EventWhen::of(span, "2026-11-01T15:59"), EventWhen::Upcoming { ref ends } if ends == "2026-11-01T16:00"));
    assert_eq!(EventWhen::of(span, "2026-11-01T16:00"), EventWhen::Past);
    assert_eq!(open_tickets(Some("https://t.test"), span, "2026-11-01T15:59"), Some("https://t.test"));
    assert_eq!(open_tickets(Some("https://t.test"), span, "2026-11-01T16:00"), None);
    assert!(upcoming_mark(span, Language::En, "2026-11-01T15:59").contains("moss-event-upcoming"));
    assert_eq!(upcoming_mark(span, Language::En, "2026-11-01T16:00"), "");
}

#[test]
fn only_web_addresses_become_links() {
    assert_eq!(tickets_link("javascript:alert(1)", Language::En, None), None);
    let link = online_link(" https://example.test/live ", Language::En, Some("Talk")).unwrap();
    assert!(link.starts_with(r#"<a class="moss-event-link moss-event-online" href="https://example.test/live">"#), "{link}");
    assert!(link.contains(r#"<span class="visually-hidden">"#), "{link}");
}
