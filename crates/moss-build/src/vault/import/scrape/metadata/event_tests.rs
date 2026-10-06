use super::super::derive;

fn page(json: &str) -> String {
    format!(
        r#"<html><head><script type="application/ld+json">{json}</script></head><body></body></html>"#
    )
}

#[test]
fn event_start_and_end_keep_wall_clock_and_never_write_date() {
    let m = derive(&page(
        r#"{"@type":"Event","startDate":"2026-11-01T19:30:00-04:00","endDate":"2026-11-01T21:00:00Z"}"#,
    ));
    assert_eq!(m.event.start.as_deref(), Some("2026-11-01 19:30"));
    assert_eq!(m.event.end.as_deref(), Some("2026-11-01 21:00"));
    assert_eq!(m.date, None);

    let m = derive(&page(r#"{"@type":"Event","startDate":"2026-11-01"}"#));
    assert_eq!(m.event.start.as_deref(), Some("2026-11-01"));
}

#[test]
fn invalid_start_is_not_written() {
    let m = derive(&page(r#"{"@type":"Event","startDate":"2026-13-40"}"#));
    assert_eq!(m.event.start, None);
}

#[test]
fn event_status_names_and_urls_map_to_the_four_words_and_scheduled_is_absent() {
    let status = |s: &str| {
        derive(&page(&format!(r#"{{"@type":"Event","eventStatus":"{s}"}}"#))).event.status
    };
    assert_eq!(status("https://schema.org/EventCancelled").as_deref(), Some("cancelled"));
    assert_eq!(status("EventPostponed").as_deref(), Some("postponed"));
    assert_eq!(status("http://schema.org/EventMovedOnline").as_deref(), Some("moved-online"));
    assert_eq!(status("EventRescheduled").as_deref(), Some("rescheduled"));
    assert_eq!(status("https://schema.org/EventScheduled"), None);
    assert_eq!(status("SomethingElse"), None);
}

#[test]
fn offers_object_and_array_become_tickets() {
    let m = derive(&page(
        r#"{"@type":"Event","offers":{"@type":"Offer","url":"https://tickets.example/a"}}"#,
    ));
    assert_eq!(m.event.tickets.as_deref(), Some("https://tickets.example/a"));
    let m = derive(&page(
        r#"{"@type":"Event","offers":[{"@type":"Offer","price":"5"},{"url":"https://tickets.example/b"},{"url":"https://tickets.example/c"}]}"#,
    ));
    assert_eq!(m.event.tickets.as_deref(), Some("https://tickets.example/b"));
}

#[test]
fn virtual_location_becomes_online() {
    let m = derive(&page(
        r#"{"@type":"Event","location":{"@type":"VirtualLocation","url":"https://stream.example/live"}}"#,
    ));
    assert_eq!(m.event.online.as_deref(), Some("https://stream.example/live"));
    assert_eq!(m.event.location, None);
}

#[test]
fn hybrid_location_array_splits_place_and_online() {
    let m = derive(&page(
        r#"{"@type":"MusicEvent","location":[
            {"@type":"Place","name":"Example Hall","address":"1 Example St\nExample City"},
            {"@type":"VirtualLocation","url":"https://stream.example/live"}]}"#,
    ));
    assert_eq!(m.event.location.as_deref(), Some("Example Hall"));
    assert_eq!(m.event.online.as_deref(), Some("https://stream.example/live"));
}

#[test]
fn place_without_name_uses_address_first_line_and_string_location_is_kept() {
    let m = derive(&page(
        r#"{"@type":"Event","location":{"@type":"Place","address":"200 Example Ave\nExample City"}}"#,
    ));
    assert_eq!(m.event.location.as_deref(), Some("200 Example Ave"));
    let m = derive(&page(r#"{"@type":"TheaterEvent","location":"Example Theater"}"#));
    assert_eq!(m.event.location.as_deref(), Some("Example Theater"));
}

#[test]
fn article_plus_event_keeps_article_date_and_event_start() {
    let m = derive(&page(
        r#"{"@graph":[
            {"@type":"Article","headline":"Recap","datePublished":"2026-11-02T09:00:00Z"},
            {"@type":"Event","startDate":"2026-11-01T14:00:00-05:00"}]}"#,
    ));
    assert_eq!(m.date.as_deref(), Some("2026-11-02"));
    assert_eq!(m.event.start.as_deref(), Some("2026-11-01 14:00"));
}
