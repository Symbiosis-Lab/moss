//! iCalendar (RFC 5545) text: escaping, line folding and the `VEVENT` writer.

use moss_core::event::EventTime;

/// One event, ready to write. Times are venue-local wall-clock values.
#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: EventTime,
    pub end: Option<EventTime>,
    /// IANA zone the times are in; `None` writes floating local time.
    pub tzid: Option<String>,
    /// `CANCELLED` or `TENTATIVE`; `None` writes no `STATUS`.
    pub status: Option<&'static str>,
    pub location: Option<String>,
    pub description: Option<String>,
    /// The event page's own address.
    pub url: Option<String>,
    /// Where an online or hybrid event is held.
    pub online: Option<String>,
    /// `DTSTAMP`, as `YYYYMMDDTHHMMSSZ`. Derived from the page, never the clock, so a
    /// rebuild of unchanged pages writes identical bytes.
    pub stamp: String,
}

/// Escape a TEXT value: backslash, semicolon, comma and line breaks.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            // Other control characters have no place in a TEXT value.
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Fold one content line at 75 octets, never inside a UTF-8 sequence, and end
/// every physical line with CRLF. Continuation lines start with one space,
/// which counts toward their 75.
pub fn push_folded(out: &mut String, line: &str) {
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

fn date_text(t: &EventTime) -> String {
    t.day_key().replace('-', "")
}

/// `YYYYMMDDTHHMMSS`; a date-only value is taken as midnight.
fn local_text(t: &EventTime) -> String {
    let key = t.sort_key().replace(['-', ':'], "");
    match key.split_once('T') {
        Some((d, hm)) => format!("{d}T{hm}00"),
        None => format!("{key}T000000"),
    }
}

/// The day after `t`'s day, as `YYYYMMDD`; `None` when that is not a date jiff
/// can represent (a calendar-invalid day, or the last day of year 9999).
fn next_day(t: &EventTime) -> Option<String> {
    let d: jiff::civil::Date = t.day_key().parse().ok()?;
    let n = d.tomorrow().ok()?;
    Some(format!("{:04}{:02}{:02}", n.year(), n.month(), n.day()))
}

/// The `DTSTART`/`DTEND` lines. An all-day `end` is inclusive on the page and
/// exclusive in iCalendar, hence the extra day.
fn time_lines(e: &Event) -> Vec<String> {
    let zone = e.tzid.as_deref().map(|z| format!(";TZID={z}")).unwrap_or_default();
    let mut lines = Vec::new();
    if e.start.is_all_day() {
        lines.push(format!("DTSTART;VALUE=DATE:{}", date_text(&e.start)));
        if let Some(end) = &e.end {
            match next_day(end) {
                Some(next) => lines.push(format!("DTEND;VALUE=DATE:{next}")),
                None => warn_no_dtend(e),
            }
        }
        return lines;
    }
    lines.push(format!("DTSTART{zone}:{}", local_text(&e.start)));
    if let Some(end) = &e.end {
        // A date-only end on a timed event covers its whole day.
        let end = if end.is_all_day() { next_day(end).map(|d| format!("{d}T000000")) } else { Some(local_text(end)) };
        match end {
            Some(end) => lines.push(format!("DTEND{zone}:{end}")),
            None => warn_no_dtend(e),
        }
    }
    lines
}

/// An end past the last day `jiff` represents has no successor day to write, so
/// the event is written without a `DTEND` and the author is told which one.
fn warn_no_dtend(e: &Event) {
    log::warn!("Event `{}` has no DTEND in its calendar file: its end is on the last day the calendar date type can represent", e.summary);
}

/// The unfolded `BEGIN:VEVENT` .. `END:VEVENT` lines.
pub fn vevent_lines(e: &Event) -> Vec<String> {
    let mut l = vec![
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", e.uid),
        format!("DTSTAMP:{}", e.stamp),
    ];
    l.extend(time_lines(e));
    l.push(format!("SUMMARY:{}", escape_text(&e.summary)));
    if let Some(s) = e.status {
        l.push(format!("STATUS:{s}"));
    }
    if let Some(v) = &e.location {
        l.push(format!("LOCATION:{}", escape_text(v)));
    }
    if let Some(v) = &e.description {
        l.push(format!("DESCRIPTION:{}", escape_text(v)));
    }
    if let Some(v) = &e.url {
        l.push(format!("URL:{v}"));
    }
    if let Some(v) = &e.online {
        l.push(format!("CONFERENCE;VALUE=URI:{v}"));
    }
    l.push("END:VEVENT".to_string());
    l
}

/// A complete calendar object. `zones` are the `VTIMEZONE` blocks the events refer to.
pub fn calendar_text(name: &str, zones: &[Vec<String>], events: &[Event], subscribable: bool) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//moss//calendar//EN".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        format!("X-WR-CALNAME:{}", escape_text(name)),
    ];
    if subscribable {
        lines.push("REFRESH-INTERVAL;VALUE=DURATION:PT12H".to_string());
        lines.push("X-PUBLISHED-TTL:PT12H".to_string());
    }
    lines.extend(zones.iter().flatten().cloned());
    for e in events {
        lines.extend(vevent_lines(e));
    }
    lines.push("END:VCALENDAR".to_string());
    let mut out = String::new();
    for line in &lines {
        push_folded(&mut out, line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escaping_covers_the_four_specials() {
        assert_eq!(escape_text("a\\b;c,d\ne\r\nf"), "a\\\\b\\;c\\,d\\ne\\nf");
    }

    #[test]
    fn folding_stops_at_75_octets_and_keeps_utf8_whole() {
        let mut out = String::new();
        push_folded(&mut out, &format!("SUMMARY:{}", "音".repeat(40)));
        for physical in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(physical.len() <= 75, "{} octets: {physical}", physical.len());
        }
        assert!(out.ends_with("\r\n"));
        let unfolded = out.replace("\r\n ", "");
        assert_eq!(unfolded, format!("SUMMARY:{}\r\n", "音".repeat(40)));
    }
}
