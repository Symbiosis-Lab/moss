//! The event facts in an article's meta line: when, place, status, links.
//!
//! An event is a page with `start`. Its meta line shows the event's time in
//! place of the posted date (a page has one "when": `start` if it has one,
//! else `date`), then the status label, the place and the `tickets` / `online`
//! links. The values are wall-clock text and are never converted: an event at
//! 14:00 is at 14:00 where it happens. `<time datetime>` carries that local
//! value without a zone offset, because the build has no time-zone database
//! (`chrono` is here without `chrono-tz`); the `timezone:` field is not
//! rendered on the page.

use crate::build::features::html_escape;
use crate::build::types::ParsedDocument;
use crate::i18n::{t, Language};
use chrono::{Datelike, NaiveDate};
use moss_core::event::EventTime;

const STATUSES: [&str; 4] = ["cancelled", "postponed", "moved-online", "rescheduled"];

/// The event's meta span, or `None` when the page is not an event.
pub fn render(doc: &ParsedDocument, lang: Language, emit_source_lines: bool) -> Option<String> {
    let ev = doc.event.as_ref()?;
    let (start, end) = (ev.start, ev.end);
    let status = ev.status.as_deref().map(str::trim).filter(|s| STATUSES.contains(s));

    let mut html = format!(
        r#"<span class="date moss-event-meta"{}{}>"#,
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
        html_escape(&format_range(start, end, lang)),
    ));
    // A resolved place already has its own linked line under the meta line.
    if doc.place_line.is_none() && !doc.location.is_empty() {
        html.push_str(&format!(
            r#"<span class="moss-event-place">{}</span>"#,
            html_escape(&doc.location.join(", ")),
        ));
    }
    for (url, key) in [(&ev.tickets, "event_tickets"), (&ev.online, "event_online")] {
        if let Some(url) = url.as_deref().map(str::trim).filter(|u| is_web_url(u)) {
            html.push_str(&format!(
                r#"<a class="moss-event-link" href="{}">{}</a>"#,
                html_escape(url),
                t(lang, key),
            ));
        }
    }
    html.push_str("</span>");
    Some(html)
}

fn is_web_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

fn ymd(t: EventTime) -> (u16, u8, u8) {
    match t {
        EventTime::Date(y, m, d) | EventTime::DateTime(y, m, d, _, _) => (y, m, d),
    }
}

fn hm(t: EventTime) -> Option<(u8, u8)> {
    match t {
        EventTime::DateTime(_, _, _, h, m) => Some((h, m)),
        EventTime::Date(..) => None,
    }
}

/// `Sunday, November 1, 2026` / `2026年11月1日 星期日`.
fn long_date(t: EventTime, lang: Language) -> String {
    let (y, m, d) = ymd(t);
    let date = crate::build::components::format_article_date(&t.day_key(), lang);
    let Some(day) = NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32) else { return date };
    let wd = crate::i18n::t(lang, &format!("weekday_{}", day.weekday().num_days_from_monday()));
    match lang {
        Language::En => format!("{wd}, {date}"),
        Language::ZhHans | Language::ZhHant => format!("{date} {wd}"),
    }
}

fn clock(h: u8, m: u8, lang: Language, meridiem: bool) -> String {
    match lang {
        Language::En => {
            let h12 = if h % 12 == 0 { 12 } else { h % 12 };
            let suffix = if meridiem { if h < 12 { " AM" } else { " PM" } } else { "" };
            format!("{h12}:{m:02}{suffix}")
        }
        Language::ZhHans | Language::ZhHant => format!("{h:02}:{m:02}"),
    }
}

fn format_range(start: EventTime, end: Option<EventTime>, lang: Language) -> String {
    // An `end` that is not after `start` adds nothing (the build warns about it).
    let end = end.filter(|e| e.sort_key() > start.sort_key());
    let same_day = end.is_some_and(|e| e.day_key() == start.day_key());
    let day = long_date(start, lang);
    match (hm(start), end) {
        // All-day, one day.
        (None, None) => day,
        (None, Some(_)) if same_day => day,
        (None, Some(e)) => format!("{day} – {}", long_date(e, lang)),
        // Timed.
        (Some((h, m)), None) => format!("{day}{}{}", sep(lang), clock(h, m, lang, true)),
        (Some((h, m)), Some(e)) => match hm(e) {
            Some((eh, em)) if same_day => {
                // "2:00–4:00 PM" when both fall in the same half of the day.
                let shared = lang == Language::En && (h < 12) == (eh < 12);
                format!(
                    "{day}{}{}–{}",
                    sep(lang),
                    clock(h, m, lang, !shared),
                    clock(eh, em, lang, true),
                )
            }
            Some((eh, em)) => format!(
                "{day}{}{} – {}{}{}",
                sep(lang),
                clock(h, m, lang, true),
                long_date(e, lang),
                sep(lang),
                clock(eh, em, lang, true),
            ),
            // Timed start, all-day end: through the end of that day.
            None => format!("{day}{}{} – {}", sep(lang), clock(h, m, lang, true), long_date(e, lang)),
        },
    }
}

fn sep(lang: Language) -> &'static str {
    match lang {
        Language::En => ", ",
        Language::ZhHans | Language::ZhHant => " ",
    }
}

#[cfg(test)]
#[path = "event_meta_tests.rs"]
mod tests;
