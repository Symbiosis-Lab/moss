//! The display forms of a date: compact (`2025 · 09`) for lists, the month
//! alone under a year heading, the long article date, CJK numerals under
//! vertical typesetting, and the `<time>` element that carries them.

use crate::date::{parse_partial_date, PartialDate};
use super::Language;

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

/// [`format_vertical_cjk_date_compact`] under [`use_cjk_numerals`], else
/// [`format_date_string`]. A date with a month (`2026-02`, `2026-02-14`) is
/// written as the month in `lang` ("February 2026", "2026年2月"): "2026 · 02"
/// reads as a version number.
pub fn format_compact_date(date_str: &str, lang: Language, typesetting: Option<&str>) -> String {
    if use_cjk_numerals(typesetting, lang) {
        format_vertical_cjk_date_compact(date_str).unwrap_or_else(|| format_date_string(date_str))
    } else if let Some(PartialDate { year, month: Some(month), .. }) = parse_partial_date(date_str) {
        format_article_date(&format!("{year}-{month:02}"), lang)
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
/// use moss_core::i18n::dates::format_month_prefix;
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
        Language::De => match day {
            Some(day) => format!("{day}. {month_name} {year}"),
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
/// use moss_core::i18n::dates::format_date_string;
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

/// The `datetime` attribute value for a raw date: the date at the precision it
/// was written (`2026`, `2026-03`, `2026-03-04`), or `None` when it does not parse.
pub fn datetime_attr(raw: &str) -> Option<String> {
    let pd = parse_partial_date(raw)?;
    Some(match (pd.month, pd.day) {
        (Some(m), Some(d)) => format!("{:04}-{:02}-{:02}", pd.year, m, d),
        (Some(m), None) => format!("{:04}-{:02}", pd.year, m),
        _ => format!("{:04}", pd.year),
    })
}

/// The local clock time a raw date writes after its day (`2026-11-06T14:32`,
/// `2026-11-06 14:32`, seconds allowed), as `HH:MM`. A timestamp with a zone
/// (`…T00:50:43Z`) gives none: its clock is not the author's.
pub fn clock_time(raw: &str) -> Option<&str> {
    let after_day = raw.trim().get(10..)?.strip_prefix(['T', ' '])?;
    let (clock, rest) = (after_day.get(..5)?, after_day.get(5..)?);
    let digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    let (h, m) = clock.split_once(':')?;
    let seconds_only = rest.is_empty() || rest.strip_prefix(':').is_some_and(digits);
    (digits(h) && digits(m) && seconds_only).then_some(clock)
}

/// A day's display text with a clock time after it, as `lang` writes the two together.
pub fn with_clock(day: &str, clock: &str, lang: Language) -> String {
    match lang {
        Language::ZhHans | Language::ZhHant => format!("{day} {clock}"),
        Language::En | Language::De => format!("{day}, {clock}"),
    }
}

/// A date's display text as `<time datetime>` carrying the full date, when the
/// raw date parses; otherwise the escaped text alone.
pub fn time_html(display: &str, raw: &str) -> String {
    match datetime_attr(raw).filter(|_| !display.is_empty()) {
        Some(iso) => format!(r#"<time datetime="{iso}">{}</time>"#, crate::media::html_escape(display)),
        None => crate::media::html_escape(display),
    }
}

#[cfg(test)]
#[path = "dates_tests.rs"]
mod tests;
