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

/// Parse a `YYYY-MM-DD-…` prefix from a filename.
///
/// The filename must START with the date in the exact form, optionally
/// followed by `-rest` and an extension. `news-2025-11-15.md` does NOT match.
fn date_from_filename_prefix(filename: &str) -> Option<String> {
    // Operate on bytes throughout: `&filename[..10]` panics when byte 10 lands
    // inside a multi-byte char (e.g. `纽约诸法门.md`). All bytes we *match*
    // against (digits, `-`, `.`) are ASCII, so byte comparisons are
    // semantically equivalent to char comparisons here.
    let bytes = filename.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let y = std::str::from_utf8(&bytes[0..4]).ok()?.parse::<u32>().ok()?;
    let m = std::str::from_utf8(&bytes[5..7]).ok()?.parse::<u32>().ok()?;
    let d = std::str::from_utf8(&bytes[8..10]).ok()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y == 0 {
        return None;
    }

    match bytes.get(10) {
        None | Some(b'-') | Some(b'.') => {}
        _ => return None,
    }

    Some(format!("{:04}-{:02}-{:02}", y, m, d))
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

#[cfg(test)]
mod tests {
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
