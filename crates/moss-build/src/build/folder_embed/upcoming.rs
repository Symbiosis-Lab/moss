//! `children_group: upcoming`: events that have not ended first, soonest
//! first; everything else (past events, ordinary articles) after, newest
//! first. Decided when the site is built.

use crate::build::components::child_list::ChildItemProps;
use crate::build::types::ParsedDocument;
use chrono::Local;
use std::collections::HashSet;

/// True for an event that has not ended at `now`. An all-day event counts
/// through the whole of its last day; a timed one until its `end` (else its
/// `start`). Wall-clock times are compared with the build machine's clock,
/// since the build carries no time-zone database. Ordinary articles are never
/// upcoming, whatever their `date`.
fn is_upcoming(doc: &ParsedDocument, now: chrono::NaiveDateTime) -> bool {
    let Some(ev) = &doc.event else { return false };
    let last = ev.end.filter(|e| e.sort_key() > ev.start.sort_key()).unwrap_or(ev.start);
    if last.is_all_day() {
        last.day_key() >= now.format("%Y-%m-%d").to_string()
    } else {
        last.sort_key() >= now.format("%Y-%m-%dT%H:%M").to_string()
    }
}

/// The `url_path`s of the upcoming events among `docs`; empty unless `group`
/// is `upcoming`.
pub(super) fn urls<'a>(group: &str, docs: &[&'a ParsedDocument]) -> HashSet<&'a str> {
    if group != "upcoming" {
        return HashSet::new();
    }
    let now = Local::now().naive_local();
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
}
