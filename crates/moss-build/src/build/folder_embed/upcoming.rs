//! `children_group: upcoming`: events that have not ended first, soonest
//! first; everything else (past events, ordinary articles) after, newest
//! first. Decided when the site is built, as of the build's day.

use crate::build::components::child_list::ChildItemProps;
use crate::build::types::ParsedDocument;
use moss_core::event::{EventSpan, EventTime};
use std::collections::HashSet;

/// True for an event that has not ended at `now`: see [`EventSpan::ends_at`]. Ordinary
/// articles are never upcoming, whatever their `date`.
pub(crate) fn is_upcoming(doc: &ParsedDocument, now: chrono::NaiveDateTime) -> bool {
    moss_core::page::listing::is_upcoming(doc, &minute(now))
}

/// The test behind [`is_upcoming`], for callers that hold an event's times
/// rather than its page.
pub(crate) fn span_is_upcoming(start: EventTime, end: Option<EventTime>, now: chrono::NaiveDateTime) -> bool {
    ends_if_upcoming(start, end, now).is_some()
}

/// [`EventSpan::ends_at`] while the event has not ended at `now`, else `None`.
pub(crate) fn ends_if_upcoming(start: EventTime, end: Option<EventTime>, now: chrono::NaiveDateTime) -> Option<String> {
    EventSpan { start, end }.ends_if_upcoming(&minute(now))
}

/// `now` as the minute an event's end is compared with.
pub(crate) fn minute(now: chrono::NaiveDateTime) -> String {
    now.format("%Y-%m-%dT%H:%M").to_string()
}

/// Where an event stands at `now`, as its page meta, card and row publish it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EventWhen {
    /// Not ended; `ends` is [`EventSpan::ends_at`], read by `event-ends.js` to turn the
    /// element past in the browser at that minute.
    Upcoming { ends: String },
    Past,
}

impl EventWhen {
    pub(crate) fn of(ev: &crate::build::types::EventFields, now: chrono::NaiveDateTime) -> Self {
        ends_if_upcoming(ev.start, ev.end, now).map_or(Self::Past, |ends| Self::Upcoming { ends })
    }

    /// ` data-when="…"`, with ` data-ends="…"` while upcoming.
    pub(crate) fn attrs(&self) -> String {
        match self {
            Self::Upcoming { ends } => format!(r#" data-when="upcoming" data-ends="{ends}""#),
            Self::Past => r#" data-when="past""#.to_string(),
        }
    }
}

/// The event's `tickets:` address while it has not ended. A past event sells
/// nothing, so every page, card and row that draws a Tickets link asks here.
/// `online:` is not gated: after an event it is often the recording.
pub(crate) fn open_tickets(ev: &crate::build::types::EventFields, now: chrono::NaiveDateTime) -> Option<&str> {
    ev.tickets.as_deref().filter(|_| span_is_upcoming(ev.start, ev.end, now))
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
        assert_eq!(EventSpan { start, end }.ends_at(), "2026-10-08T21:30");
    }

    #[test]
    fn an_all_day_event_is_over_at_midnight_after_its_last_day() {
        let day = EventTime::Date(2026, 12, 31);
        assert_eq!(EventSpan { start: day, end: None }.ends_at(), "2027-01-01T00:00");
        let at = |y, m, d, h, min| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap();
        assert!(span_is_upcoming(day, None, at(2026, 12, 31, 23, 59)));
        assert!(!span_is_upcoming(day, None, at(2027, 1, 1, 0, 0)));
    }
}
