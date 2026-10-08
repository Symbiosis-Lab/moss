//! IANA zones as iCalendar `VTIMEZONE` components (RFC 5545 section 3.6.5).
//!
//! The observances are listed explicitly (one per transition in the window the
//! events need) rather than as recurrence rules, so a client never has to guess
//! at a rule the zone database holds in a different form.

use jiff::tz::{Offset, TimeZone};
use jiff::{civil, Timestamp};

/// A zone looked up by its IANA name, or `None` when the name is unknown.
pub fn load(name: &str) -> Option<TimeZone> {
    TimeZone::get(name).ok()
}

/// A UTC offset in seconds: `+HHMM`, or `+HH:MM` when `colon`. Seconds are
/// appended only when non-zero, as `+HHMMSS` or `+HH:MM:SS`.
fn format_offset(seconds: i32, colon: bool) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let s = seconds.abs();
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    let sep = if colon { ":" } else { "" };
    if sec == 0 { format!("{sign}{h:02}{sep}{m:02}") } else { format!("{sign}{h:02}{sep}{m:02}{sep}{sec:02}") }
}

fn local_text(ts: Timestamp, offset: Offset) -> String {
    let dt = offset.to_datetime(ts);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second()
    )
}

/// The UTC offset in force in `zone` at the venue-local time `local`, as
/// `+05:30` (the form schema.org dates carry). `None` for an unknown zone.
/// A date-only value is taken at midnight; a time the zone skips or repeats
/// takes the offset a calendar app would pick. This is the one place zone
/// arithmetic for event times lives.
pub fn utc_offset(zone: &str, local: &moss_core::event::EventTime) -> Option<String> {
    use moss_core::event::EventTime::{Date, DateTime};
    let (y, mo, d, h, mi) = match *local {
        Date(y, m, d) => (y, m, d, 0, 0),
        DateTime(y, m, d, h, mi) => (y, m, d, h, mi),
    };
    let dt = civil::date(y as i16, mo as i8, d as i8).at(h as i8, mi as i8, 0, 0);
    let s = load(zone)?.to_ambiguous_zoned(dt).compatible().ok()?.offset().seconds();
    Some(format_offset(s, true))
}

/// The lines of a `VTIMEZONE` for `tzid`, covering every transition from the
/// start of `first_year` to the end of `last_year`. Lines are unfolded and carry
/// no line ending.
pub fn vtimezone(tzid: &str, tz: &TimeZone, first_year: i16, last_year: i16) -> Vec<String> {
    let at = |y: i16| civil::date(y, 1, 1).at(0, 0, 0, 0).to_zoned(TimeZone::UTC).map(|z| z.timestamp());
    let (Ok(from), Ok(to)) = (at(first_year), at(last_year.saturating_add(1))) else {
        return Vec::new();
    };
    let mut lines = vec!["BEGIN:VTIMEZONE".to_string(), format!("TZID:{tzid}")];

    // The observance in force when the window opens: an onset at the window's own start.
    let info = tz.to_offset_info(from);
    let mut current = info.offset();
    let kind = if info.dst().is_dst() { "DAYLIGHT" } else { "STANDARD" };
    observance(&mut lines, kind, &format!("{first_year:04}0101T000000"), current, current, info.abbreviation());

    for t in tz.following(from).take_while(|t| t.timestamp() < to) {
        let next = t.offset();
        let kind = if t.dst().is_dst() { "DAYLIGHT" } else { "STANDARD" };
        // DTSTART is the onset in the wall-clock time that was showing just before it.
        observance(&mut lines, kind, &local_text(t.timestamp(), current), current, next, t.abbreviation());
        current = next;
    }
    lines.push("END:VTIMEZONE".to_string());
    lines
}

fn observance(lines: &mut Vec<String>, kind: &str, onset: &str, from: Offset, to: Offset, name: &str) {
    lines.push(format!("BEGIN:{kind}"));
    lines.push(format!("DTSTART:{onset}"));
    lines.push(format!("TZOFFSETFROM:{}", format_offset(from.seconds(), false)));
    lines.push(format!("TZOFFSETTO:{}", format_offset(to.seconds(), false)));
    lines.push(format!("TZNAME:{name}"));
    lines.push(format!("END:{kind}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_york_2026_has_both_transitions_with_wall_clock_onsets() {
        let tz = load("America/New_York").unwrap();
        let text = vtimezone("America/New_York", &tz, 2026, 2026).join("\n");
        assert!(text.contains("DTSTART:20260308T020000\nTZOFFSETFROM:-0500\nTZOFFSETTO:-0400\nTZNAME:EDT"), "{text}");
        assert!(text.contains("DTSTART:20261101T020000\nTZOFFSETFROM:-0400\nTZOFFSETTO:-0500\nTZNAME:EST"), "{text}");
    }

    #[test]
    fn a_zone_without_transitions_still_has_one_observance() {
        let tz = load("Asia/Taipei").unwrap();
        let lines = vtimezone("Asia/Taipei", &tz, 2026, 2026);
        assert_eq!(lines.iter().filter(|l| l.starts_with("BEGIN:STANDARD")).count(), 1);
        assert!(lines.contains(&"TZOFFSETTO:+0800".to_string()));
    }

    #[test]
    fn utc_offset_follows_the_zone_on_that_date() {
        use moss_core::event::EventTime;
        let t = |s| EventTime::parse(s).unwrap();
        assert_eq!(utc_offset("America/New_York", &t("2026-07-04 12:00")).as_deref(), Some("-04:00"));
        assert_eq!(utc_offset("America/New_York", &t("2026-12-25")).as_deref(), Some("-05:00"));
        assert_eq!(utc_offset("Asia/Kolkata", &t("2026-12-25 09:00")).as_deref(), Some("+05:30"));
        assert_eq!(utc_offset("Mars/Olympus_Mons", &t("2026-12-25")), None);
    }

    #[test]
    fn offsets_format_with_or_without_colon_and_seconds_only_when_present() {
        assert_eq!(format_offset(19800, true), "+05:30");
        assert_eq!(format_offset(-18000, false), "-0500");
        assert_eq!(format_offset(0, false), "+0000");
        assert_eq!(format_offset(-19805, false), "-053005");
        assert_eq!(format_offset(19805, true), "+05:30:05");
    }

    #[test]
    fn unknown_names_do_not_load() {
        assert!(load("Mars/Olympus_Mons").is_none());
    }
}
