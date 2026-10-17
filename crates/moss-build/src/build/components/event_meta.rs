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
/// `calendar_href` is the event file's address, from the calendar plan; the
/// "Add to calendar" link goes after the tickets and online links.
pub fn render(doc: &ParsedDocument, lang: Language, emit_source_lines: bool, calendar_href: Option<&str>) -> Option<String> {
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
        time_parts(start, end, lang),
    ));
    // A resolved place already has its own linked line under the meta line.
    if doc.place_line.is_none() && !doc.location.is_empty() {
        html.push_str(&format!(
            r#"<span class="moss-event-place">{}</span>"#,
            html_escape(&doc.location.join(", ")),
        ));
    }
    if let Some(link) = ev.tickets.as_deref().and_then(|u| tickets_link(u, lang, None)) {
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

fn is_web_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// The Tickets link for an event's `tickets:` address, or `None` when the
/// address is not http or https. The event page passes `None`. A listing card
/// passes the event's title, which is read after the label for assistive
/// technology.
pub(crate) fn tickets_link(url: &str, lang: Language, label_suffix: Option<&str>) -> Option<String> {
    action_link(url, lang, label_suffix, "moss-event-tickets", "event_tickets")
}

/// The Online link for an event's `online:` address, or `None` when the
/// address is not http or https. The label suffix works as for
/// [`tickets_link`].
pub(crate) fn online_link(url: &str, lang: Language, label_suffix: Option<&str>) -> Option<String> {
    action_link(url, lang, label_suffix, "moss-event-online", "event_online")
}

/// One of the event's action links: an http or https address, labelled by
/// `label_key`, with the `event_tickets_for` suffix when a title is given.
fn action_link(url: &str, lang: Language, label_suffix: Option<&str>, class: &str, label_key: &str) -> Option<String> {
    let url = url.trim();
    if !is_web_url(url) {
        return None;
    }
    let suffix = label_suffix
        .map(|title| {
            format!(
                r#"<span class="visually-hidden">{}</span>"#,
                html_escape(&t(lang, "event_tickets_for").replace("{}", title)),
            )
        })
        .unwrap_or_default();
    Some(format!(
        r#"<a class="moss-event-link {class}" href="{}">{}{}</a>"#,
        html_escape(url),
        t(lang, label_key),
        suffix,
    ))
}

/// A run of the written time. A `Part` is a piece a theme may style on its own
/// (a large day numeral, a small weekday), and that includes the one separator
/// before a clock time; `Text` is the remaining punctuation.
enum Seg {
    Text(String),
    Part(&'static str, String),
}

fn text(s: &str) -> Seg {
    Seg::Text(s.to_string())
}

/// `Sunday, November 1, 2026` / `Sonntag, 1. November 2026` / `2026年11月1日 星期日`, as runs.
fn long_date(t: EventTime, lang: Language) -> Vec<Seg> {
    let (y, m, d) = t.ymd();
    let month = crate::i18n::t(lang, &format!("month_{m}")).to_string();
    let weekday = NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
        .map(|day| crate::i18n::t(lang, &format!("weekday_{}", day.weekday().num_days_from_monday())));
    match lang {
        Language::En => {
            let mut v = Vec::new();
            if let Some(wd) = weekday {
                v.push(Seg::Part("weekday", wd.to_string()));
                v.push(text(", "));
            }
            v.extend([
                Seg::Part("month", month),
                text(" "),
                Seg::Part("day", d.to_string()),
                text(", "),
                Seg::Part("year", y.to_string()),
            ]);
            v
        }
        Language::De => {
            let mut v = Vec::new();
            if let Some(wd) = weekday {
                v.push(Seg::Part("weekday", wd.to_string()));
                v.push(text(", "));
            }
            v.extend([
                Seg::Part("day", format!("{d}.")),
                text(" "),
                Seg::Part("month", month),
                text(" "),
                Seg::Part("year", y.to_string()),
            ]);
            v
        }
        Language::ZhHans | Language::ZhHant => {
            let mut v = vec![
                Seg::Part("year", format!("{y}年")),
                Seg::Part("month", month),
                Seg::Part("day", format!("{d}日")),
            ];
            if let Some(wd) = weekday {
                v.push(text(" "));
                v.push(Seg::Part("weekday", wd.to_string()));
            }
            v
        }
    }
}

fn clock(h: u8, m: u8, lang: Language, meridiem: bool) -> String {
    match lang {
        Language::En => {
            let h12 = if h % 12 == 0 { 12 } else { h % 12 };
            let suffix = if meridiem { if h < 12 { " AM" } else { " PM" } } else { "" };
            format!("{h12}:{m:02}{suffix}")
        }
        // `meridiem` marks the last clock of a time, which carries the unit:
        // "14:00–16:00 Uhr".
        Language::De => format!("{h:02}:{m:02}{}", if meridiem { " Uhr" } else { "" }),
        Language::ZhHans | Language::ZhHant => format!("{h:02}:{m:02}"),
    }
}

/// The separator between a date and its clock time: a comma in English, a
/// space in Chinese. A `Part`, so a theme can hide it.
fn sep(lang: Language) -> Seg {
    Seg::Part(
        "sep",
        match lang {
            Language::En | Language::De => ", ",
            Language::ZhHans | Language::ZhHant => " ",
        }
        .to_string(),
    )
}

/// The event's written time as runs: a date, a time of day when the start has
/// one, and the end when it is after `start`.
fn segments(start: EventTime, end: Option<EventTime>, lang: Language) -> Vec<Seg> {
    // The event is already validated. Equal endpoints (or an all-day end on
    // the start day) add no second date or clock time to the displayed range.
    let end = end.filter(|e| e.sort_key() > start.sort_key());
    let same_day = end.is_some_and(|e| e.day_key() == start.day_key());
    let mut out = long_date(start, lang);
    let time = |out: &mut Vec<Seg>, s: String| {
        out.push(sep(lang));
        out.push(Seg::Part("time", s));
    };
    match (start.hm(), end) {
        (None, None) => {}
        (None, Some(_)) if same_day => {}
        (None, Some(e)) => {
            out.push(text(" – "));
            out.extend(long_date(e, lang));
        }
        (Some((h, m)), None) => time(&mut out, clock(h, m, lang, true)),
        (Some((h, m)), Some(e)) => match e.hm() {
            Some((eh, em)) if same_day => {
                // "2:00–4:00 PM" when both fall in the same half of the day.
                let shared = match lang {
                    Language::En => (h < 12) == (eh < 12),
                    Language::De => true,
                    _ => false,
                };
                time(&mut out, format!("{}–{}", clock(h, m, lang, !shared), clock(eh, em, lang, true)));
            }
            end_time => {
                time(&mut out, clock(h, m, lang, true));
                out.push(text(" – "));
                out.extend(long_date(e, lang));
                // A timed start with an all-day end runs through that day.
                if let Some((eh, em)) = end_time {
                    time(&mut out, clock(eh, em, lang, true));
                }
            }
        },
    }
    out
}

/// The written time as inner HTML: each part in its `moss-when-*` span, the
/// punctuation as bare text. The one exception is the separator before the
/// clock time, `moss-when-sep`, so a theme that sets the time on its own line
/// can hide it. Shared by the page's meta line and listings, so
/// both show the same words in the same pieces.
fn time_parts(start: EventTime, end: Option<EventTime>, lang: Language) -> String {
    let mut html = String::new();
    for seg in segments(start, end, lang) {
        match seg {
            Seg::Text(t) => html.push_str(&html_escape(&t)),
            Seg::Part(class, t) => {
                html.push_str(&format!(r#"<span class="moss-when-{class}">{}</span>"#, html_escape(&t)))
            }
        }
    }
    html
}

/// The same written time as [`render`] puts in the page's meta line, as a
/// `<time>` whose pieces (`moss-when-weekday`, `-day`, `-month`, `-year`,
/// `-time`) sit in their own spans so a theme can lay a listing's date out as
/// it likes. `datetime` is the start as written; the visible text is the same
/// words the page shows.
pub fn render_when(start: EventTime, end: Option<EventTime>, lang: Language) -> String {
    format!(r#"<time class="moss-when" datetime="{}">{}</time>"#, start.sort_key(), time_parts(start, end, lang))
}

#[cfg(test)]
#[path = "event_meta_tests.rs"]
mod tests;
