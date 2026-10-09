//! Date formatting and extraction utilities for the static site generator
//!
//! Provides date formats used throughout the generated site:
//! - Compact format: "2025 · 09" (for lists and sidebars)
//! - Month-only format: "11" (for minimal layout where year is section header)
//!
//! Also provides date extraction from documents and file paths.

use crate::build::types::ParsedDocument;
use crate::i18n::Language;

/// A date parsed to whatever precision it was written at — year, year+month,
/// or a full year+month+day. The one parser six formatters used to each
/// re-implement by splitting on `-`/`/`/`T` and re-validating the parts.
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

    let year_part = parts[0];
    if year_part.len() != 4 || !year_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: i32 = year_part.parse().ok()?;

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

impl PartialDate {
    /// The earliest calendar date this partial date could denote: a missing
    /// day defaults to the 1st, a missing month to January.
    pub fn earliest(&self) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::from_ymd_opt(self.year, self.month.unwrap_or(1), self.day.unwrap_or(1))
    }

    /// The latest calendar date this partial date could denote: a missing
    /// day defaults to the last day of the month, a missing month to
    /// December 31st. Used by a `since:` cutoff — a post dated only to the
    /// month is excluded only once every day that month could name is past
    /// the cutoff, not just the 1st.
    pub fn latest(&self) -> Option<chrono::NaiveDate> {
        match (self.month, self.day) {
            (Some(month), Some(day)) => chrono::NaiveDate::from_ymd_opt(self.year, month, day),
            (Some(month), None) => {
                let (next_year, next_month) =
                    if month == 12 { (self.year + 1, 1) } else { (self.year, month + 1) };
                chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .and_then(|d| d.pred_opt())
            }
            (None, _) => chrono::NaiveDate::from_ymd_opt(self.year, 12, 31),
        }
    }
}

/// Formats a bare "YYYY" into its Chinese-numeral year, or `None` if `year`
/// isn't all ASCII digits. Shared by both CJK formatters below for the
/// year-only partial-date case, where there's no month to key off.
fn format_cjk_year_only(year: &str) -> Option<String> {
    use crate::i18n::numerals::to_chinese_year;
    if year.is_empty() || !year.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}年", to_chinese_year(year)))
}

/// Format date for vertical CJK typesetting: "2025-09-24" -> "二〇二五年·九月二十四日"
///
/// # Examples
/// ```ignore
/// assert_eq!(format_vertical_cjk_date("2025-09-24"), Some("二〇二五年·九月二十四日".to_string()));
/// assert_eq!(format_vertical_cjk_date("1694-01-01"), Some("一六九四年·一月一日".to_string()));
/// assert_eq!(format_vertical_cjk_date("1699"), Some("一六九九年".to_string()));
/// ```
pub fn format_vertical_cjk_date(date_str: &str) -> Option<String> {
    let pd = parse_partial_date(date_str)?;
    let year = pd.year.to_string();
    let Some(month) = pd.month else {
        return format_cjk_year_only(&year);
    };

    use crate::i18n::numerals::{to_chinese_day, to_chinese_month, to_chinese_year};
    let mut result = format!("{}年·{}", to_chinese_year(&year), to_chinese_month(month));
    if let Some(day) = pd.day {
        result.push_str(&to_chinese_day(day));
    }
    Some(result)
}

/// Format compact date for vertical CJK: "2025-09" -> "二〇二五年·九月"
///
/// # Examples
/// ```ignore
/// assert_eq!(format_vertical_cjk_date_compact("2025-09"), Some("二〇二五年·九月".to_string()));
/// assert_eq!(format_vertical_cjk_date_compact("1699"), Some("一六九九年".to_string()));
/// ```
pub fn format_vertical_cjk_date_compact(date_str: &str) -> Option<String> {
    let pd = parse_partial_date(date_str)?;
    let year = pd.year.to_string();
    let Some(month) = pd.month else {
        return format_cjk_year_only(&year);
    };

    use crate::i18n::numerals::{to_chinese_month, to_chinese_year};
    Some(format!("{}年·{}", to_chinese_year(&year), to_chinese_month(month)))
}

/// The predicate the article date line uses to pick a CJK formatter. Defined
/// in [`crate::i18n`] — the counts and the dates must answer it the same way —
/// and re-exported here for the callers that reach it through this module.
pub use crate::i18n::use_cjk_numerals;

/// [`format_vertical_cjk_date`] under [`use_cjk_numerals`], else [`format_article_date`].
pub fn format_display_date(date_str: &str, lang: Language, typesetting: Option<&str>) -> String {
    if use_cjk_numerals(typesetting, lang) {
        format_vertical_cjk_date(date_str).unwrap_or_else(|| format_article_date(date_str, lang))
    } else {
        format_article_date(date_str, lang)
    }
}

/// [`format_vertical_cjk_date_compact`] under [`use_cjk_numerals`], else [`format_date_string`].
pub fn format_compact_date(date_str: &str, lang: Language, typesetting: Option<&str>) -> String {
    if use_cjk_numerals(typesetting, lang) {
        format_vertical_cjk_date_compact(date_str).unwrap_or_else(|| format_date_string(date_str))
    } else {
        format_date_string(date_str)
    }
}

/// The date prefix a year-grouped minimal row carries — the month, because the
/// year is already the section heading above it.
///
/// Three things the raw month never got right:
/// - a year-only date (`1700`) has no month, and printing the year again beside
///   a `1700` heading said nothing twice. It gets no prefix at all.
/// - under [`use_cjk_numerals`] the month is written `十二月`, not `12`: an
///   Arabic digit in a vertical run lies on its side, which is the whole reason
///   the predicate exists.
/// - the `"YYYY · MM"` display form is accepted as well as ISO, because the
///   caller falls back to `date_display` when there is no raw date.
///
/// # Examples
/// ```ignore
/// use moss::build::components::date::format_month_prefix;
/// assert_eq!(format_month_prefix("2025-11-17", Language::En, None), "11");
/// assert_eq!(format_month_prefix("1700", Language::En, None), "");
/// ```
pub fn format_month_prefix(date_str: &str, lang: Language, typesetting: Option<&str>) -> String {
    let Some(month) = month_of(date_str) else {
        return String::new();
    };
    if use_cjk_numerals(typesetting, lang) {
        crate::i18n::numerals::to_chinese_month(month)
    } else {
        format!("{:0>2}", month)
    }
}

/// The month number in a `YYYY-MM-DD`, `YYYY/MM/DD` or `YYYY · MM` string.
/// `None` when the string carries no month (a year-only date, or junk).
fn month_of(date_str: &str) -> Option<u32> {
    let trimmed = date_str.trim();
    if let Some(month_part) = trimmed.split(" · ").nth(1) {
        return month_part.trim().parse::<u32>().ok().filter(|m| (1..=12).contains(m));
    }
    parse_partial_date(trimmed).and_then(|pd| pd.month)
}

/// A year-group heading, in Chinese numerals under [`use_cjk_numerals`].
///
/// `一七〇三` stands upright in a vertical column; `1703` lies on its side, and
/// a heading is the one piece of chrome a reader scans a long scroll for.
pub fn format_year_heading(year: i32, lang: Language, typesetting: Option<&str>) -> String {
    if use_cjk_numerals(typesetting, lang) {
        crate::i18n::numerals::to_chinese_year(&year.to_string())
    } else {
        year.to_string()
    }
}

/// Formats a date string for an article's own displayed date line, in `lang`'s
/// conventional written form: "April 10, 1919" / "April 1919" (en),
/// "1919年4月10日" / "1919年4月" (zh-hans/zh-hant). A year-only date (`date:
/// 1925`) stays a bare year in every language — there is no month or day to
/// spell out, and "1925年" would claim a precision the source never gave.
/// Returns the original string unparsed if it does not parse as a date at
/// all.
///
/// This is the HORIZONTAL-typesetting path — [`format_display_date`] is what
/// chooses between this and [`format_vertical_cjk_date`], a vertical CJK
/// page's own already-conventional Chinese-numeral form ("一九一九年·四月十日"),
/// which this function does not touch.
///
/// Before this fix, every precision — including a full `YYYY-MM-DD` — printed
/// as raw numbers joined by " · " ("1919 · 4 · 10"): never a documented
/// display choice, just what a numeric formatter built for a listing's
/// compact card ([`format_date_string`], whose OWN "print what was given"
/// partial-precision format IS documented and unchanged by this fix) also
/// did for the one place — a full article date — where the site's language
/// has a real written form to use instead. A full, tested, unused month-name
/// table (`i18n::strings::t`'s `month_N`/`month_abbr_N` keys) already existed
/// for exactly this and had no caller until now.
///
/// # Examples
/// ```ignore
/// use moss::build::components::date_formatters::format_article_date;
/// use moss::i18n::Language;
/// assert_eq!(format_article_date("2024-09-22", Language::En), "September 22, 2024");
/// assert_eq!(format_article_date("2025-11", Language::En), "November 2025");
/// assert_eq!(format_article_date("1919-04-10", Language::ZhHant), "1919年4月10日");
/// ```
pub fn format_article_date(date_str: &str, lang: Language) -> String {
    let Some(pd) = parse_partial_date(date_str) else {
        return date_str.to_string();
    };
    match (pd.month, pd.day) {
        (Some(month), day) => format_long_date(pd.year, month, day, lang),
        (None, _) => pd.year.to_string(),
    }
}

/// The locale table behind [`format_article_date`]'s month+[day] rungs: a
/// month name plus year, and a day when there is one, assembled in the order
/// and with the punctuation each language actually writes it in. `month` is
/// looked up in `i18n::strings::t`'s "month_N" table (already zh-hans/
/// zh-hant-aware — the Chinese entries there are `"N月"`, so `{year}年{month}`
/// alone reads correctly with no separate day-suffix logic needed for the
/// month-only rung).
fn format_long_date(year: i32, month: u32, day: Option<u32>, lang: Language) -> String {
    let month_name = crate::i18n::t(lang, &format!("month_{month}"));
    match lang {
        Language::En => match day {
            Some(day) => format!("{month_name} {day}, {year}"),
            None => format!("{month_name} {year}"),
        },
        Language::ZhHans | Language::ZhHant => match day {
            Some(day) => format!("{year}年{month_name}{day}日"),
            None => format!("{year}年{month_name}"),
        },
    }
}

/// Extracts the year from a date string.
///
/// # Examples
/// ```ignore
/// use moss::build::components::date_formatters::extract_year;
/// assert_eq!(extract_year("2025-09-02"), Some(2025));
/// assert_eq!(extract_year("invalid"), None);
/// ```
///
/// # Arguments
/// * `date_str` - Date string that may contain a year
///
/// # Returns
/// The year as i32 if found, None otherwise
pub fn extract_year(date_str: &str) -> Option<i32> {
    parse_partial_date(date_str).map(|pd| pd.year)
}

/// Format various date string formats to "YYYY · MM"
///
/// Handles common date formats: YYYY-MM-DD, YYYY/MM/DD, YYYY-MM, etc.
///
/// # Examples
/// ```ignore
/// use moss::build::components::date::format_date_string;
/// assert_eq!(format_date_string("2025-09-02"), "2025 · 09");
/// assert_eq!(format_date_string("2025/01/15"), "2025 · 01");
/// ```
pub fn format_date_string(date_str: &str) -> String {
    match parse_partial_date(date_str) {
        Some(PartialDate { year, month: Some(month), .. }) => format!("{} · {:0>2}", year, month),
        Some(PartialDate { year, month: None, .. }) => year.to_string(),
        None => date_str.to_string(),
    }
}

/// Try to extract a raw date string from a URL filename.
///
/// e.g., "posts/2025-01-15.html" → Some("2025-01-15")
///
/// A leaf page's date-stamped filename carries its slug right after the date
/// (`2021-04-09-spring.html`), so the returned string is the whole
/// pre-`.html` stem, not just the year-month-day run — `parse_partial_date`
/// already ignores anything past the day when a caller re-parses it. This
/// function's own job is only to validate that a date is there and reject
/// the filename otherwise, which it now defers to `parse_partial_date`
/// (year+month required, same as before) instead of re-checking digit-ness
/// and length by hand — that hand-rolled check never validated the numeric
/// range, so `2021-99-09-spring.html` used to pass through as a "date".
fn extract_raw_date_from_filename(url_path: &str) -> Option<String> {
    let filename = url_path.rsplit('/').next().unwrap_or(url_path);
    let date_part = filename.strip_suffix(".html")?;
    parse_partial_date(date_part)?.month?;
    Some(date_part.to_string())
}

/// Extract date from document preferring frontmatter over filename.
///
/// Returns (display, raw) where:
/// - `display` is formatted as "YYYY · MM" (or "Unknown")
/// - `raw` is an ISO-ish date string suitable for sorting, or None if no date found
///
/// # Arguments
/// * `doc` - The parsed document to extract date from
/// * `_root_path` - Unused; kept so callers need not change
pub fn extract_date_from_doc(doc: &ParsedDocument, _root_path: &str) -> (String, Option<String>, bool) {
    // An event's own time is the page's "when"; `date` only says when it was posted.
    if let Some(ev) = &doc.event {
        return (format_date_string(&ev.when), Some(ev.when.clone()), true);
    }
    // First priority: Use frontmatter date if available
    if let Some(frontmatter_date) = &doc.date {
        return (format_date_string(frontmatter_date), Some(frontmatter_date.clone()), true);
    }

    // Second: Try filename-based date extraction from url_path
    if let Some(raw) = extract_raw_date_from_filename(&doc.url_path) {
        return (format_date_string(&raw), Some(raw), true);
    }

    // No `start`, no `date`, no date in the filename: undated. A file's creation
    // time is not a date the author gave the page, and ranking it as one put
    // undated pages ahead of every dated page under `sort: date`.
    ("Unknown".to_string(), None, false)
}

#[cfg(test)]
#[path = "date_tests.rs"]
mod tests;
