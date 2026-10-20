//! `children_group: upcoming`: events that have not ended first, soonest
//! first; everything else (past events, ordinary articles) after, newest
//! first. Decided when the site is built.

use crate::build::components::child_list::ChildItemProps;
use crate::build::types::ParsedDocument;
use chrono::Local;
use moss_core::event::EventTime;
use std::collections::HashSet;

/// True for an event that has not ended at `now`: see [`ends_at`]. Wall-clock
/// times are compared with the build machine's clock, since the build carries
/// no time-zone database. Ordinary articles are never upcoming, whatever
/// their `date`.
pub(crate) fn is_upcoming(doc: &ParsedDocument, now: chrono::NaiveDateTime) -> bool {
    let Some(ev) = &doc.event else { return false };
    span_is_upcoming(ev.start, ev.end, now)
}

/// The test behind [`is_upcoming`], for callers that hold an event's times
/// rather than its page.
pub(crate) fn span_is_upcoming(start: EventTime, end: Option<EventTime>, now: chrono::NaiveDateTime) -> bool {
    ends_if_upcoming(start, end, now).is_some()
}

/// [`ends_at`] while the event has not ended at `now`, else `None`.
pub(crate) fn ends_if_upcoming(start: EventTime, end: Option<EventTime>, now: chrono::NaiveDateTime) -> Option<String> {
    let ends = ends_at(start, end);
    (now.format("%Y-%m-%dT%H:%M").to_string() < ends).then_some(ends)
}

/// When an event is over, as wall-clock `YYYY-MM-DDTHH:MM`: an all-day event
/// at the start of the day after its last day, a timed one at its `end` (else
/// its `start`). The one definition of "ended": the event is over from this
/// minute on, and `hero-ends.ts` compares the same way.
pub(crate) fn ends_at(start: EventTime, end: Option<EventTime>) -> String {
    let last = end.unwrap_or(start);
    if !last.is_all_day() {
        return last.sort_key();
    }
    let (y, m, d) = last.ymd();
    chrono::NaiveDate::from_ymd_opt(i32::from(y), u32::from(m), u32::from(d))
        .and_then(|day| day.succ_opt())
        .map_or_else(|| last.day_key() + "T23:59", |next| format!("{}T00:00", next.format("%Y-%m-%d")))
}

/// `"upcoming"` or `"past"`: the `data-when` value of an event's page, card
/// and row.
pub(crate) fn when_of(ev: &crate::build::types::EventFields, now: chrono::NaiveDateTime) -> &'static str {
    if span_is_upcoming(ev.start, ev.end, now) { "upcoming" } else { "past" }
}

/// Whether `doc` passes a `when:` filter: `past` keeps whatever is not
/// upcoming (articles included), `upcoming` keeps only events that have not
/// ended. No filter, or an unknown one, keeps everything.
pub(super) fn keeps(when: Option<&str>, doc: &ParsedDocument, now: chrono::NaiveDateTime) -> bool {
    match when {
        Some("upcoming") => is_upcoming(doc, now),
        Some("past") => !is_upcoming(doc, now),
        _ => true,
    }
}

/// The build's "now" for every past/upcoming decision.
pub(crate) fn now() -> chrono::NaiveDateTime {
    Local::now().naive_local()
}

/// The `url_path`s of the upcoming events among `docs`; empty unless `group`
/// is `upcoming`.
pub(super) fn urls<'a>(group: &str, docs: &[&'a ParsedDocument], now: chrono::NaiveDateTime) -> HashSet<&'a str> {
    if group != "upcoming" {
        return HashSet::new();
    }
    docs.iter().filter(|d| is_upcoming(d, now)).map(|d| d.url_path.as_str()).collect()
}

/// Upcoming items first (soonest first), then the rest (newest first). With
/// `keep_order` (the caller already ordered by a non-date axis) each part
/// keeps its given order.
pub(super) fn order<'a>(
    items: Vec<&'a ChildItemProps>,
    upcoming: &HashSet<&str>,
    keep_order: bool,
) -> Vec<&'a ChildItemProps> {
    let (mut soon, mut rest): (Vec<_>, Vec<_>) =
        items.into_iter().partition(|i| upcoming.contains(i.url_path.as_str()));
    if !keep_order {
        soon.sort_by(|a, b| moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), true));
        rest.sort_by(|a, b| moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), false));
    }
    soon.into_iter().chain(rest).collect()
}

/// `order` for whole documents, run before `limit` cuts the list, so the
/// limit counts from the upcoming run rather than from the folder's sort.
pub(super) fn order_docs<'a>(
    docs: Vec<&'a ParsedDocument>,
    upcoming: &HashSet<&str>,
    keep_order: bool,
) -> Vec<&'a ParsedDocument> {
    use moss_core::sort::{sort_by_resolved, ResolvedSort, SortAxis};
    let (soon, rest): (Vec<_>, Vec<_>) =
        docs.into_iter().partition(|d| upcoming.contains(d.url_path.as_str()));
    if keep_order {
        return soon.into_iter().chain(rest).collect();
    }
    let by = |axis| ResolvedSort { axis, explicit_order: None, series_default: false };
    sort_by_resolved(&soon, &by(SortAxis::DateAsc))
        .into_iter()
        .chain(sort_by_resolved(&rest, &by(SortAxis::Date)))
        .collect()
}

/// The two runs as `<section class="moss-cards-group" data-group="…">`; an
/// empty run is not emitted. `render` draws one item.
pub(super) fn sections(
    articles: &[&ChildItemProps],
    upcoming: &HashSet<&str>,
    render: impl Fn(&ChildItemProps) -> String,
) -> String {
    let mut html = String::new();
    for (name, in_group) in [("upcoming", true), ("earlier", false)] {
        let items: Vec<&&ChildItemProps> =
            articles.iter().filter(|a| upcoming.contains(a.url_path.as_str()) == in_group).collect();
        if items.is_empty() {
            continue;
        }
        html.push_str(&format!("<section class=\"moss-cards-group\" data-group=\"{name}\">\n"));
        for article in items {
            html.push_str(&render(article));
            html.push('\n');
        }
        html.push_str("</section>\n");
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::types::EventFields;
    use moss_core::event::EventTime;

    fn all_day_event(day: EventTime) -> ParsedDocument {
        ParsedDocument {
            event: Some(EventFields {
                start: day,
                end: None,
                when: day.sort_key(),
                timezone: None,
                status: None,
                tickets: None,
                online: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn all_day_event_stays_upcoming_through_its_last_day() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert!(is_upcoming(&all_day_event(EventTime::Date(2026, 10, 8)), now), "last day is today");
        assert!(!is_upcoming(&all_day_event(EventTime::Date(2026, 10, 7)), now), "last day was yesterday");
    }

    #[test]
    fn timed_event_with_an_inclusive_end_day_stays_upcoming_that_evening() {
        let doc = ParsedDocument {
            event: EventFields::from_frontmatter(&moss_core::frontmatter_typed::FrontMatter {
                start: Some("2026-10-08 14:00".into()),
                end: Some("2026-10-08".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        assert!(is_upcoming(&doc, day.and_hms_opt(20, 0, 0).unwrap()));
        assert!(!is_upcoming(&doc, day.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap()));
    }

    #[test]
    fn an_event_is_over_from_the_minute_it_ends() {
        let start = EventTime::DateTime(2026, 10, 8, 19, 0);
        let end = Some(EventTime::DateTime(2026, 10, 8, 21, 30));
        let at = |h, m| chrono::NaiveDate::from_ymd_opt(2026, 10, 8).unwrap().and_hms_opt(h, m, 0).unwrap();
        assert!(span_is_upcoming(start, end, at(21, 29)));
        assert!(!span_is_upcoming(start, end, at(21, 30)));
        assert_eq!(ends_at(start, end), "2026-10-08T21:30");
    }

    #[test]
    fn an_all_day_event_is_over_at_midnight_after_its_last_day() {
        let day = EventTime::Date(2026, 12, 31);
        assert_eq!(ends_at(day, None), "2027-01-01T00:00");
        let at = |y, m, d, h, min| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap();
        assert!(span_is_upcoming(day, None, at(2026, 12, 31, 23, 59)));
        assert!(!span_is_upcoming(day, None, at(2027, 1, 1, 0, 0)));
    }
}
