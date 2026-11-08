//! Publish-date resolution for moss content.
//!
//! Single public entry point: [`resolve_publish_date`]. Returns the file's
//! canonical `YYYY-MM-DD` date plus the [`DateSource`] that produced it.
//!
//! Returns canonicalized `YYYY-MM-DD` strings so lexicographic comparison
//! is correct chronological comparison. No `chrono` dep — moss-core's
//! existing convention is `Option<String>` for dates (see
//! `frontmatter::FrontMatter::date`).

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;

/// Provenance of a resolved publish date. Distinguishes explicit dates
/// (`Frontmatter`, `FilenamePrefix`) from implicit fallbacks (`Ctime`),
/// so consumers that care (e.g. file-tree zoning, RSS feed pubDate)
/// can treat them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    Frontmatter,
    FilenamePrefix,
    Ctime,
    None,
}

/// Resolve the publish date for a markdown file.
///
/// Precedence (first match wins):
///   1. Frontmatter `date` field.
///   2. `YYYY-MM-DD-…` prefix on any of the supplied `filenames`, in order.
///      The slice lets callers pass multiple candidate names — e.g. a build
///      pipeline that has both a slugified `url_path` and a source filename
///      can pass them both without composing its own precedence here.
///   3. `fallback_ctime`, used as-is (caller is responsible for canonical form).
///
/// Returns `(None, DateSource::None)` only when nothing matches.
///
/// Pure function — no I/O.
pub fn resolve_publish_date(
    frontmatter: &HashMap<String, Value>,
    filenames: &[&str],
    fallback_ctime: Option<&str>,
) -> (Option<String>, DateSource) {
    if let Some(d) = date_from_frontmatter(frontmatter) {
        return (Some(d), DateSource::Frontmatter);
    }
    for fname in filenames {
        if let Some(d) = date_from_filename_prefix(fname) {
            return (Some(d), DateSource::FilenamePrefix);
        }
    }
    if let Some(d) = fallback_ctime {
        return (Some(d.to_string()), DateSource::Ctime);
    }
    (None, DateSource::None)
}

// ── Private helpers ───────────────────────────────────────────────────────

/// Read the frontmatter `date` field and normalize to `YYYY-MM-DD`.
///
/// Accepts:
/// - ISO date: `2025-11-15`
/// - ISO timestamp: `2025-11-15T10:30:00Z`, `2025-11-15T10:30:00`
/// - Slash form: `2025/11/15`
fn date_from_frontmatter(frontmatter: &HashMap<String, Value>) -> Option<String> {
    let raw = match frontmatter.get("date")? {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if raw.is_empty() {
        return None;
    }
    normalize_date(&raw)
}

/// Parse a `YYYY-MM-DD-…` prefix from a filename (extension optional).
///
/// The filename must START with the date stamp. `news-2025-11-15.md` does NOT match.
fn date_from_filename_prefix(filename: &str) -> Option<String> {
    let stem = match filename.rsplit_once('.') {
        Some((stem, ext)) if ext.starts_with(|c: char| c.is_ascii_alphabetic()) => stem,
        _ => filename,
    };
    parse_filename_stamp(stem).map(|s| s.date)
}

/// The date stamp a file stem starts with, and what follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilenameStamp {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `HH:MM`, a local time.
    pub time: Option<String>,
    /// The words after the stamp, separators trimmed; `None` for a bare stamp.
    pub words: Option<String>,
}

impl FilenameStamp {
    /// The page date: `YYYY-MM-DD`, or `YYYY-MM-DDTHH:MM` when a time was written.
    pub fn page_date(&self) -> String {
        match &self.time {
            Some(t) => format!("{}T{t}", self.date),
            None => self.date.clone(),
        }
    }
}

/// Parse the date stamp at the start of a file stem: `YYYY-MM-DD`, optionally
/// followed by ` HHMM`, ` HH.MM`, ` HH-MM` or `THHMM`, or the compact
/// `YYYYMMDDHHMM`. The stamp must end the stem or be followed by a space or
/// hyphen and words, so `2026-11-06 1432 Autumn hotpot` has all three parts.
pub fn parse_filename_stamp(stem: &str) -> Option<FilenameStamp> {
    let digits = |from: usize, n: usize| stem.get(from..from + n).filter(|d| d.bytes().all(|c| c.is_ascii_digit()));
    if digits(0, 12).is_some() && digits(0, 13).is_none() {
        let date = format!("{}-{}-{}", digits(0, 4)?, digits(4, 2)?, digits(6, 2)?);
        return stamp_ending_at(stem, date, Some((digits(8, 2)?, digits(10, 2)?)), 12);
    }
    let date = stem.get(..10)?.to_string();
    if matches!(stem.as_bytes().get(10), Some(b' ' | b'T')) {
        if let Some((clock, end)) = clock_at(stem, 11) {
            if let Some(stamp) = stamp_ending_at(stem, date.clone(), Some(clock), end) {
                return Some(stamp);
            }
        }
    }
    stamp_ending_at(stem, date, None, 10)
}

/// Validate a stamp read up to byte `end` and read the words after it.
fn stamp_ending_at(stem: &str, date: String, clock: Option<(&str, &str)>, end: usize) -> Option<FilenameStamp> {
    parse_ymd(&date)?;
    let time = match clock {
        Some((h, m)) => {
            let (h, m) = (fixed_digits(h, 2)?, fixed_digits(m, 2)?);
            if h > 23 || m > 59 {
                return None;
            }
            Some(format!("{h:02}:{m:02}"))
        }
        None => None,
    };
    let rest = stem.get(end..)?;
    if !rest.is_empty() && !rest.starts_with([' ', '-']) {
        return None;
    }
    let words = rest.trim_matches([' ', '-']);
    Some(FilenameStamp { date, time, words: (!words.is_empty()).then(|| words.to_string()) })
}

/// A clock time `HHMM`, `HH.MM` or `HH-MM` starting at byte `from`: its two
/// digit pairs and where it ends.
fn clock_at(stem: &str, from: usize) -> Option<((&str, &str), usize)> {
    let pair = |at: usize| stem.get(at..at + 2).filter(|p| p.bytes().all(|c| c.is_ascii_digit()));
    let second = if matches!(stem.as_bytes().get(from + 2), Some(b'.' | b'-')) { from + 3 } else { from + 2 };
    Some(((pair(from)?, pair(second)?), second + 2))
}

/// A number written as exactly `n` ASCII digits.
pub(crate) fn fixed_digits(p: &str, n: usize) -> Option<u32> {
    if p.len() == n && p.bytes().all(|b| b.is_ascii_digit()) {
        p.parse().ok()
    } else {
        None
    }
}

/// Strictly parse `YYYY-MM-DD` (four, two and two ASCII digits, a real
/// calendar day, year 1 or later). The one home of the calendar rules.
pub(crate) fn parse_ymd(s: &str) -> Option<(u16, u8, u8)> {
    let mut parts = s.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (y, m, d) = (fixed_digits(y, 4)?, fixed_digits(m, 2)?, fixed_digits(d, 2)?);
    if y < 1 || !(1..=days_in_month(y, m)).contains(&d) {
        return None;
    }
    Some((y as u16, m as u8, d as u8))
}

/// Days in a month of the proleptic Gregorian calendar; 0 for a month outside 1..=12.
pub(crate) fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Normalize a date-ish string to `YYYY-MM-DD`.
fn normalize_date(s: &str) -> Option<String> {
    let s = s.trim();
    let date_part = s.split('T').next().unwrap_or(s);
    let normalized: String = date_part.replace('/', "-");
    let mut parts = normalized.split('-');
    let y = parts.next()?.parse::<u32>().ok()?;
    let m = parts.next()?.parse::<u32>().ok()?;
    let d = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("{:04}-{:02}-{:02}", y, m, d))
}

/// A date parsed to whatever precision it was written at: a year, a year and
/// month, or a full year, month and day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialDate {
    pub year: i32,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

/// Parses `YYYY`, `YYYY-MM`, or `YYYY-MM-DD` (also accepting `/` as the
/// separator, and dropping anything from a `T` or a space onward, so a full
/// ISO datetime or a "YYYY · MM" display string reduces to its date part).
/// The year must be exactly 4 numeric digits; a present month or day must be
/// numeric and in range (`1..=12`, `1..=31`). Anything else is `None`.
///
/// A 4th+ dash-separated segment is ignored rather than rejected — a leaf
/// page's date-stamped filename (`2021-04-09-spring.html`) carries its slug
/// in the same string the date is read from, and only the leading
/// year-month(-day) run needs to parse.
pub fn parse_partial_date(s: &str) -> Option<PartialDate> {
    let trimmed = s.trim();
    let mut end = trimmed.len();
    if let Some(i) = trimmed.find('T') {
        end = end.min(i);
    }
    if let Some(i) = trimmed.find(' ') {
        end = end.min(i);
    }
    let date_part = trimmed.get(..end)?.replace('/', "-");
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.is_empty() {
        return None;
    }

    let year = fixed_digits(parts[0], 4)? as i32;

    let parse_part = |part: &str, range: std::ops::RangeInclusive<u32>| -> Option<u32> {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse::<u32>().ok().filter(|v| range.contains(v))
    };

    let month = match parts.get(1) {
        Some(m) => Some(parse_part(m, 1..=12)?),
        None => None,
    };
    let day = match parts.get(2) {
        Some(d) => Some(parse_part(d, 1..=31)?),
        None => None,
    };

    Some(PartialDate { year, month, day })
}

/// The finest unit an EDTF date states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Precision {
    Year,
    Month,
    Day,
}

/// How sure the author is of a date: `?` uncertain, `~` approximate, `%` both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Qualifier {
    Uncertain,
    Approximate,
    Both,
}

/// One EDTF date: the calendar date, its precision and its qualifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdtfDate {
    pub date: PartialDate,
    pub qualifier: Option<Qualifier>,
}

impl EdtfDate {
    /// The finest unit the date states.
    pub fn precision(&self) -> Precision {
        match (self.date.month, self.date.day) {
            (Some(_), Some(_)) => Precision::Day,
            (Some(_), None) => Precision::Month,
            _ => Precision::Year,
        }
    }

    /// The date as written, without its qualifier: `1930`, `1930-10`, `1930-10-12`.
    pub fn value(&self) -> String {
        let mut out = format!("{:04}", self.date.year);
        if let Some(month) = self.date.month {
            out.push_str(&format!("-{month:02}"));
        }
        if let Some(day) = self.date.day {
            out.push_str(&format!("-{day:02}"));
        }
        out
    }
}

impl Serialize for EdtfDate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("EdtfDate", 3)?;
        state.serialize_field("value", &self.value())?;
        state.serialize_field("precision", &self.precision())?;
        state.serialize_field("qualifier", &self.qualifier)?;
        state.end()
    }
}

/// A parsed EDTF value: one date, or an interval with an optional end
/// (`None` is an open end, written `..` or left empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edtf {
    Date(EdtfDate),
    Interval { start: EdtfDate, end: Option<EdtfDate> },
}

/// Strict EDTF (ISO 8601-2) parser. Accepts `YYYY`, `YYYY-MM` and `YYYY-MM-DD`
/// checked against the calendar, one trailing or leading qualifier (`?`, `~`,
/// `%`), and an interval split once on `/`, `–` or `—`, with `..` or an empty
/// end for an open interval. The lenient [`parse_partial_date`] is separate and
/// unchanged.
pub fn parse_edtf(s: &str) -> Option<Edtf> {
    let s = s.trim();
    let Some(i) = s.find(['/', '–', '—']) else {
        return parse_edtf_date(s).map(Edtf::Date);
    };
    let (left, rest) = s.split_at(i);
    let sep = rest.chars().next()?.len_utf8();
    let right = rest.get(sep..)?.trim();
    let start = parse_edtf_date(left)?;
    let end = match right {
        "" | ".." => None,
        other => Some(parse_edtf_date(other)?),
    };
    Some(Edtf::Interval { start, end })
}

fn parse_edtf_date(s: &str) -> Option<EdtfDate> {
    let (body, qualifier) = if let Some(rest) = s.strip_prefix('~') {
        (rest, Some(Qualifier::Approximate))
    } else if let Some(rest) = s.strip_suffix('?') {
        (rest, Some(Qualifier::Uncertain))
    } else if let Some(rest) = s.strip_suffix('~') {
        (rest, Some(Qualifier::Approximate))
    } else if let Some(rest) = s.strip_suffix('%') {
        (rest, Some(Qualifier::Both))
    } else {
        (s, None)
    };
    let parts: Vec<&str> = body.split('-').collect();
    let (year, month, day) = match parts.as_slice() {
        [y] => (fixed_digits(y, 4)?, None, None),
        [y, m] => (fixed_digits(y, 4)?, Some(fixed_digits(m, 2)?), None),
        [y, m, d] => (fixed_digits(y, 4)?, Some(fixed_digits(m, 2)?), Some(fixed_digits(d, 2)?)),
        _ => return None,
    };
    if let Some(m) = month {
        if !(1..=12).contains(&m) {
            return None;
        }
    }
    if let (Some(m), Some(d)) = (month, day) {
        if !(1..=days_in_month(year, m)).contains(&d) {
            return None;
        }
    }
    Some(EdtfDate { date: PartialDate { year: year as i32, month, day }, qualifier })
}

/// The proleptic Gregorian `(year, month, day)` of a count of days since
/// 1970-01-01, by the era and day-of-era form of the days-to-civil algorithm.
/// Exact for any `days` whose intermediates fit `i64` (beyond ±10^16 days).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_from_days_crosses_the_epoch_a_leap_day_and_2100() {
        use super::civil_from_days;
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
        assert_eq!(civil_from_days(20_764), (2026, 11, 7));
    }

    use super::*;
    use serde_yaml::Value;
    use std::collections::HashMap;

    fn fm(pairs: &[(&str, &str)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect()
    }

    // ── resolve_publish_date — the public API ────────────────────────────

    #[test]
    fn resolve_uses_frontmatter_first() {
        let r = resolve_publish_date(
            &fm(&[("date", "2025-03-01")]),
            &["2024-01-01-old.md"],
            Some("2023-01-01"),
        );
        assert_eq!(r, (Some("2025-03-01".to_string()), DateSource::Frontmatter));
    }

    #[test]
    fn resolve_falls_through_to_filename() {
        let r = resolve_publish_date(
            &fm(&[("title", "x")]),
            &["2024-01-01-old.md"],
            Some("2023-01-01"),
        );
        assert_eq!(
            r,
            (Some("2024-01-01".to_string()), DateSource::FilenamePrefix)
        );
    }

    #[test]
    fn resolve_tries_each_filename_in_order() {
        // First filename has no prefix; second one matches.
        let r = resolve_publish_date(
            &fm(&[]),
            &["no-date.md", "2024-05-15-from-second.md"],
            None,
        );
        assert_eq!(
            r,
            (Some("2024-05-15".to_string()), DateSource::FilenamePrefix)
        );
    }

    #[test]
    fn resolve_falls_through_to_ctime() {
        let r = resolve_publish_date(&fm(&[]), &["no-date.md"], Some("2023-01-01"));
        assert_eq!(r, (Some("2023-01-01".to_string()), DateSource::Ctime));
    }

    #[test]
    fn resolve_returns_none_when_nothing_available() {
        let r = resolve_publish_date(&fm(&[]), &["no-date.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn resolve_handles_empty_filenames_slice() {
        let r = resolve_publish_date(&fm(&[]), &[], Some("2023-01-01"));
        assert_eq!(r, (Some("2023-01-01".to_string()), DateSource::Ctime));
    }

    // ── frontmatter forms (covered through the public API) ──────────────

    #[test]
    fn frontmatter_iso_date_passes_through() {
        let r = resolve_publish_date(&fm(&[("date", "2025-11-15")]), &[], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn frontmatter_iso_timestamp_truncated_to_date() {
        let r = resolve_publish_date(&fm(&[("date", "2025-11-15T10:30:00Z")]), &[], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn frontmatter_slash_form_normalized() {
        let r = resolve_publish_date(&fm(&[("date", "2025/11/15")]), &[], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn frontmatter_malformed_returns_none() {
        let r = resolve_publish_date(&fm(&[("date", "not a date")]), &[], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn frontmatter_empty_string_returns_none() {
        let r = resolve_publish_date(&fm(&[("date", "")]), &[], None);
        assert_eq!(r, (None, DateSource::None));
    }

    // ── filename forms (covered through the public API) ─────────────────

    #[test]
    fn filename_with_dated_slug() {
        let r = resolve_publish_date(&fm(&[]), &["2025-11-15-research-proposals.md"], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn filename_bare_date_md() {
        let r = resolve_publish_date(&fm(&[]), &["2025-11-15.md"], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn filename_no_prefix() {
        let r = resolve_publish_date(&fm(&[]), &["research-proposals.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_date_in_middle_does_not_match() {
        let r = resolve_publish_date(&fm(&[]), &["news-2025-11-15.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_invalid_date_returns_none() {
        let r = resolve_publish_date(&fm(&[]), &["2025-13-99-foo.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_extensionless() {
        let r = resolve_publish_date(&fm(&[]), &["2025-11-15-foo"], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    // ── filename stamps ─────────────────────────────────────────────────

    fn stamp(stem: &str) -> Option<(String, Option<String>, Option<String>)> {
        parse_filename_stamp(stem).map(|s| (s.date, s.time, s.words))
    }

    #[test]
    fn stamp_forms() {
        let d = |t: Option<&str>, w: Option<&str>| Some(("2026-11-06".to_string(), t.map(String::from), w.map(String::from)));
        assert_eq!(stamp("2026-11-06"), d(None, None));
        assert_eq!(stamp("2026-11-06 1432"), d(Some("14:32"), None));
        assert_eq!(stamp("2026-11-06 14.32"), d(Some("14:32"), None));
        assert_eq!(stamp("2026-11-06 14-32"), d(Some("14:32"), None));
        assert_eq!(stamp("2026-11-06T1432"), d(Some("14:32"), None));
        assert_eq!(stamp("202611061432"), d(Some("14:32"), None));
        assert_eq!(stamp("2026-11-06 Autumn hotpot"), d(None, Some("Autumn hotpot")));
        assert_eq!(stamp("2026-11-06-autumn-hotpot"), d(None, Some("autumn-hotpot")));
        assert_eq!(stamp("2026-11-06 1432 Autumn hotpot"), d(Some("14:32"), Some("Autumn hotpot")));
        assert_eq!(stamp("2026-11-06 1000km"), d(None, Some("1000km")));
    }

    #[test]
    fn not_a_stamp() {
        for stem in ["notes", "2026-11-06x", "2026-13-06", "2026-02-30", "20261106143", "2026110614321", "1432", "202611062561", "x2026-11-06", "2026-11-06春节"] {
            assert_eq!(stamp(stem), None, "{stem}");
        }
    }

    #[test]
    fn page_date_carries_the_time_when_written() {
        assert_eq!(parse_filename_stamp("2026-11-06").unwrap().page_date(), "2026-11-06");
        assert_eq!(parse_filename_stamp("202611061432").unwrap().page_date(), "2026-11-06T14:32");
    }

    // ── non-ASCII filenames must not panic (regression: #date-utf8-panic) ──

    #[test]
    fn filename_cjk_only_is_rejected_without_panic() {
        // The original report: opening this file in the editor crashed moss
        // because `&filename[..10]` cut inside the third byte of `法`.
        let r = resolve_publish_date(&fm(&[]), &["纽约诸法门.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_dated_then_cjk_slug_matches() {
        let r = resolve_publish_date(&fm(&[]), &["2025-11-15-纽约诸法门.md"], None);
        assert_eq!(r.0, Some("2025-11-15".to_string()));
    }

    #[test]
    fn filename_emoji_only_is_rejected_without_panic() {
        let r = resolve_publish_date(&fm(&[]), &["📝-notes.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_date_followed_by_cjk_without_separator_is_rejected() {
        // Byte 10 is the first byte of `春` (0xE6) — not `-` or `.`, so the
        // function must reject the filename rather than slice through `春`.
        let r = resolve_publish_date(&fm(&[]), &["2025-11-15春节.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    #[test]
    fn filename_with_non_ascii_inside_date_window_is_rejected() {
        // `é` (2 bytes) at position 0 means bytes[4] lands on a digit, not
        // `-` — the date prefix is broken before we reach the boundary check.
        // Earlier code panicked before getting this far for some inputs in
        // this shape; the test pins the no-panic invariant for the whole
        // pre-position-10 window.
        let r = resolve_publish_date(&fm(&[]), &["é025-11-15.md"], None);
        assert_eq!(r, (None, DateSource::None));
    }

    // ── property test: pure functions in moss-core must not panic on user input ──

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(2048))]

        #[test]
        fn resolve_publish_date_never_panics_on_arbitrary_input(
            name in ".*",
            ctime in proptest::option::of(".*"),
        ) {
            // The contract: moss-core takes untrusted user strings (filenames,
            // frontmatter values) and must return a value, not abort the host
            // process. Anything that reaches a Tauri command must satisfy this.
            let _ = resolve_publish_date(&HashMap::new(), &[name.as_str()], ctime.as_deref());
        }
    }
}
