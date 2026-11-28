//! The event facts in an article's meta line: when, place, status, links.
//!
//! An event is a page with `start`. Its meta line shows the event's time in
//! place of the posted date (a page has one "when": `start` if it has one,
//! else `date`), then the status label, the place and the `tickets` / `online`
//! links. The written time and the links are `moss_core::event::when`'s; the
//! `timezone:` field is not rendered on the page.

use crate::build::features::html_escape;
use crate::build::types::ParsedDocument;
use crate::i18n::{t, Language};
use moss_core::event::when::{online_link, open_tickets, tickets_link, time_parts, upcoming_mark, EventWhen};

const STATUSES: [&str; 4] = ["cancelled", "postponed", "moved-online", "rescheduled"];

/// Whether an event's meta span carries the "Upcoming" label.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UpcomingLabel {
    Shown,
    Hidden,
}

/// The event's meta span, or `None` when the page is not an event.
/// `calendar_href` is the event file's address, from the calendar plan; the
/// "Add to calendar" link goes after the tickets and online links. The caller
/// passes none for a past event, which shows no Tickets either. `label`
/// says whether the "Upcoming" label follows the time while the event has not
/// ended.
pub fn render(
    doc: &ParsedDocument,
    lang: Language,
    emit_source_lines: bool,
    calendar_href: Option<&str>,
    label: UpcomingLabel,
    now: chrono::NaiveDateTime,
) -> Option<String> {
    let ev = doc.event.as_ref()?;
    let (start, end) = (ev.start, ev.end);
    let now = crate::build::folder_embed::upcoming::minute(now);
    let status = ev.status.as_deref().map(str::trim).filter(|s| STATUSES.contains(s));

    let mut html = format!(
        r#"<span class="date moss-event-meta"{}{}{}>"#,
        EventWhen::of(ev.span(), &now).attrs(),
        status.map(|s| format!(r#" data-status="{s}""#)).unwrap_or_default(),
        if emit_source_lines { r#" data-source-fm="start""# } else { "" },
    );
    if let Some(s) = status {
        html.push_str(&format!(
            r#"<span class="moss-event-status">{}</span>"#,
            t(lang, &format!("event_status_{s}")),
        ));
    }
    html.push_str(&format!(
        r#"<time class="moss-event-time" datetime="{}">{}</time>"#,
        start.sort_key(),
        time_parts(start, end, lang),
    ));
    if label == UpcomingLabel::Shown {
        html.push_str(&upcoming_mark(ev.span(), lang, &now));
    }
    // A resolved place already has its own linked line under the meta line.
    if !doc.draws_place_line() && !doc.location.is_empty() {
        html.push_str(&format!(
            r#"<span class="moss-event-place">{}</span>"#,
            html_escape(&doc.location.join(", ")),
        ));
    }
    if let Some(link) = open_tickets(ev.tickets.as_deref(), ev.span(), &now).and_then(|u| tickets_link(u, lang, None)) {
        html.push_str(&link);
    }
    if let Some(link) = ev.online.as_deref().and_then(|u| online_link(u, lang, None)) {
        html.push_str(&link);
    }
    if let Some(href) = calendar_href {
        html.push_str(&format!(
            r#"<a class="moss-event-link moss-event-calendar" href="{}">{}</a>"#,
            html_escape(href),
            t(lang, "add_to_calendar"),
        ));
    }
    html.push_str("</span>");
    Some(html)
}

#[cfg(test)]
#[path = "event_meta_tests.rs"]
mod tests;
