//! An event's time as words, and where it stands at the build's minute.
//!
//! The values are wall-clock text and are never converted: an event at 14:00
//! is at 14:00 where it happens. `<time datetime>` carries that local value
//! without a zone offset, since moss-core has no time-zone database. `now` is
//! the minute an event's end is compared with, `YYYY-MM-DDTHH:MM`.

use super::{EventSpan, EventTime};
use crate::i18n::{t, Language};
use crate::media::html_escape;

/// Where an event stands at `now`, as its page meta, card and row publish it.
#[derive(Debug, Clone, PartialEq)]
pub enum EventWhen {
    /// Not ended; `ends` is [`EventSpan::ends_at`], read by `event-ends.js` to
    /// turn the element past in the browser at that minute.
    Upcoming { ends: String },
    Past,
}

impl EventWhen {
    pub fn of(span: EventSpan, now: &str) -> Self {
        span.ends_if_upcoming(now).map_or(Self::Past, |ends| Self::Upcoming { ends })
    }

    /// ` data-when="…"`, with ` data-ends="…"` while upcoming.
    pub fn attrs(&self) -> String {
        match self {
            Self::Upcoming { ends } => format!(r#" data-when="upcoming" data-ends="{ends}""#),
            Self::Past => r#" data-when="past""#.to_string(),
        }
    }
}

/// The event's `tickets:` address while it has not ended. A past event sells
/// nothing, so every page, card and row that draws a Tickets link asks here.
/// `online:` is not gated: after an event it is often the recording.
pub fn open_tickets<'a>(tickets: Option<&'a str>, span: EventSpan, now: &str) -> Option<&'a str> {
    tickets.filter(|_| span.is_upcoming(now))
}

fn is_web_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// The Tickets link for an event's `tickets:` address, or `None` when the
/// address is not http or https. The event page passes `None`. A listing card
/// passes the event's title, which is read after the label for assistive
/// technology.
pub fn tickets_link(url: &str, lang: Language, label_suffix: Option<&str>) -> Option<String> {
    action_link(url, lang, label_suffix, "moss-event-tickets", "event_tickets")
}

/// The Online link for an event's `online:` address, or `None` when the
/// address is not http or https. The label suffix works as for
/// [`tickets_link`].
pub fn online_link(url: &str, lang: Language, label_suffix: Option<&str>) -> Option<String> {
    action_link(url, lang, label_suffix, "moss-event-online", "event_online")
}

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
    Some(format!(r#"<a class="moss-event-link {class}" href="{}">{}{}</a>"#, html_escape(url), t(lang, label_key), suffix))
}

/// A run of the written time. A `Part` is a piece a theme may style on its own
/// (a large day numeral, a small weekday), and that includes the one separator
/// before a clock time; `Text` is the remaining punctuation.
enum Seg {
    Text(&'static str),
    Part(&'static str, String),
}

/// `Sunday, November 1, 2026` / `Sonntag, 1. November 2026` / `2026年11月1日 星期日`, as runs.
fn long_date(at: EventTime, lang: Language) -> Vec<Seg> {
    let (y, m, d) = at.ymd();
    let month = Seg::Part("month", t(lang, &format!("month_{m}")).to_string());
    let weekday = crate::date::weekday_from_monday(i64::from(y), u32::from(m), u32::from(d));
    let weekday = Seg::Part("weekday", t(lang, &format!("weekday_{weekday}")).to_string());
    let year = |s: String| Seg::Part("year", s);
    match lang {
        Language::En => {
            vec![weekday, Seg::Text(", "), month, Seg::Text(" "), Seg::Part("day", d.to_string()), Seg::Text(", "), year(y.to_string())]
        }
        Language::De => {
            vec![weekday, Seg::Text(", "), Seg::Part("day", format!("{d}.")), Seg::Text(" "), month, Seg::Text(" "), year(y.to_string())]
        }
        Language::ZhHans | Language::ZhHant => {
            vec![year(format!("{y}年")), month, Seg::Part("day", format!("{d}日")), Seg::Text(" "), weekday]
        }
    }
}

fn clock(h: u8, m: u8, lang: Language, meridiem: bool) -> String {
    match lang {
        Language::En => {
            let h12 = if h.is_multiple_of(12) { 12 } else { h % 12 };
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
    let s = match lang {
        Language::En | Language::De => ", ",
        Language::ZhHans | Language::ZhHant => " ",
    };
    Seg::Part("sep", s.to_string())
}

/// The event's written time as runs: a date, a time of day when the start has
/// one, and the end when it is after `start`.
fn segments(start: EventTime, end: Option<EventTime>, lang: Language) -> Vec<Seg> {
    // Equal endpoints (or an all-day end on the start day) add no second date
    // or clock time to the displayed range.
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
            out.push(Seg::Text(" – "));
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
                out.push(Seg::Text(" – "));
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
/// can hide it. Shared by the page's meta line and listings, so both show the
/// same words in the same pieces.
pub fn time_parts(start: EventTime, end: Option<EventTime>, lang: Language) -> String {
    let mut html = String::new();
    for seg in segments(start, end, lang) {
        match seg {
            Seg::Text(s) => html.push_str(&html_escape(s)),
            Seg::Part(class, s) => html.push_str(&format!(r#"<span class="moss-when-{class}">{}</span>"#, html_escape(&s))),
        }
    }
    html
}

/// The written time as a `<time>` whose pieces (`moss-when-weekday`, `-day`,
/// `-month`, `-year`, `-time`) sit in their own spans so a theme can lay a
/// listing's date out as it likes. `datetime` is the start as written.
pub fn render_when(start: EventTime, end: Option<EventTime>, lang: Language) -> String {
    format!(r#"<time class="moss-when" datetime="{}">{}</time>"#, start.sort_key(), time_parts(start, end, lang))
}

/// The short "Upcoming" word an event's card or row shows after its time
/// while it has not ended at `now`, else nothing.
pub fn upcoming_mark(span: EventSpan, lang: Language, now: &str) -> String {
    if span.is_upcoming(now) {
        format!(r#"<span class="moss-event-upcoming">{}</span>"#, t(lang, "event_upcoming"))
    } else {
        String::new()
    }
}

#[cfg(test)]
#[path = "when_tests.rs"]
mod tests;
