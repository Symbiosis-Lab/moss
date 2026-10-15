//! Display text for a declared field's typed value, in a page's language.
//!
//! The frontmatter holds the fact; this module decides how it reads for a
//! language and precision, so an author never writes a format. Pure: no I/O.

use crate::build::components::date::format_display_date;
use crate::i18n::{t, Language};
use moss_core::date::{EdtfDate, PartialDate, Precision, Qualifier};
use moss_core::fields::{DecimalMark, TypedValue};

/// The decimal mark a language writes numbers with. Parsing a count and
/// displaying one both read it from here, so a page's numbers round-trip.
pub fn decimal_mark(lang: Language) -> DecimalMark {
    match lang {
        Language::De => DecimalMark::Comma,
        Language::En | Language::ZhHans | Language::ZhHant => DecimalMark::Point,
    }
}

/// The date as written at its own precision: a year-precision date never
/// gains a month or day, and a month-precision one never gains a day.
fn at_precision(d: &EdtfDate) -> String {
    let PartialDate { year, month, day } = d.date;
    let mut out = format!("{year:04}");
    if d.precision != Precision::Year {
        if let Some(m) = month {
            out.push_str(&format!("-{m:02}"));
        }
    }
    if d.precision == Precision::Day {
        if let Some(day) = day {
            out.push_str(&format!("-{day:02}"));
        }
    }
    out
}

/// Adds the qualifier's prefix (approximate) and suffix (uncertain).
fn qualified(body: String, q: Option<Qualifier>, lang: Language) -> String {
    let approximate = matches!(q, Some(Qualifier::Approximate | Qualifier::Both));
    let uncertain = matches!(q, Some(Qualifier::Uncertain | Qualifier::Both));
    let mut out = if approximate { format!("{}{body}", t(lang, "field_approximate")) } else { body };
    if uncertain {
        out.push('?');
    }
    out
}

pub fn date_text(d: &EdtfDate, lang: Language, typesetting: Option<&str>) -> String {
    qualified(format_display_date(&at_precision(d), lang, typesetting), d.qualifier, lang)
}

pub fn period_text(
    start: &EdtfDate,
    end: Option<&EdtfDate>,
    open: bool,
    lang: Language,
    typesetting: Option<&str>,
) -> String {
    if open {
        let from = date_text(start, lang, typesetting);
        return t(lang, "field_open_from").replace("{start}", &from);
    }
    // A one-point period is the date itself, not a range with no end.
    let Some(end) = end else {
        return date_text(start, lang, typesetting);
    };
    let plain = start.qualifier.is_none() && end.qualifier.is_none();
    let (s, e) = (start.date, end.date);
    let same_month = s.year == e.year && s.month == e.month;

    if start.precision == Precision::Year && end.precision == Precision::Year {
        if plain && s.year == e.year {
            return date_text(start, lang, typesetting);
        }
        return format!("{}–{}", date_text(start, lang, typesetting), date_text(end, lang, typesetting));
    }
    if start.precision == Precision::Day && end.precision == Precision::Day && same_month {
        let (Some(month), Some(sd), Some(ed)) = (s.month, s.day, e.day) else {
            return join_spaced(start, end, lang, typesetting);
        };
        if lang == Language::ZhHans || lang == Language::ZhHant {
            return format!("{}–{}", date_text(start, lang, typesetting), date_text(end, lang, typesetting));
        }
        if plain && sd == ed {
            return date_text(start, lang, typesetting);
        }
        if plain {
            let name = t(lang, &format!("month_{month}"));
            return match lang {
                Language::De => format!("{sd}.–{ed}. {name} {}", s.year),
                _ => format!("{name} {sd}–{ed}, {}", s.year),
            };
        }
    }
    join_spaced(start, end, lang, typesetting)
}

fn join_spaced(start: &EdtfDate, end: &EdtfDate, lang: Language, typesetting: Option<&str>) -> String {
    format!("{} – {}", date_text(start, lang, typesetting), date_text(end, lang, typesetting))
}

/// The number as displayed, and the noun unchanged. Digits are grouped by
/// the language's convention and decimals use its separator.
pub fn count_text(number: f64, noun: Option<&str>, lang: Language) -> (String, Option<String>) {
    let (group, decimal) = match decimal_mark(lang) {
        DecimalMark::Comma => ('.', ','),
        DecimalMark::Point => (',', '.'),
    };
    let plain = format!("{number}");
    let (int_part, frac) = match plain.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (plain.as_str(), None),
    };
    let (sign, digits) = match int_part.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", int_part),
    };
    let mut text = format!("{sign}{}", group_digits(digits, group));
    if let Some(frac) = frac {
        text.push(decimal);
        text.push_str(frac);
    }
    (text, noun.map(str::to_string))
}

fn group_digits(digits: &str, sep: char) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

pub(crate) fn number_text(n: &serde_json::Number, lang: Language) -> String {
    match n.as_f64() {
        Some(x) => count_text(x, None, lang).0,
        None => n.to_string(),
    }
}

/// The EDTF value a `<time datetime>` carries: a date at its own precision, a
/// period as `start/end`, an open one as `start/..`. `None` for every other kind.
pub fn datetime_attr(v: &TypedValue) -> Option<String> {
    match v {
        TypedValue::Date(d) => Some(at_precision(d)),
        TypedValue::Period { start, end, open, .. } => Some(match (end, open) {
            (_, true) => format!("{}/..", at_precision(start)),
            (Some(end), false) => format!("{}/{}", at_precision(start), at_precision(end)),
            (None, false) => at_precision(start),
        }),
        _ => None,
    }
}

pub fn value_text(v: &TypedValue, lang: Language, typesetting: Option<&str>) -> String {
    match v {
        TypedValue::Text(s) | TypedValue::Url(s) => s.clone(),
        TypedValue::Choice { label, .. } => label.clone(),
        TypedValue::Number(n) => number_text(n, lang),
        TypedValue::Date(d) => date_text(d, lang, typesetting),
        TypedValue::Period { start, end, open, .. } => period_text(start, end.as_ref(), *open, lang, typesetting),
        TypedValue::Count { number: Some(n), noun, .. } => {
            let number = number_text(n, lang);
            match noun {
                Some(noun) => format!("{number} {noun}"),
                None => number,
            }
        }
        TypedValue::Count { number: None, text, .. } => text.clone(),
        TypedValue::Page(p) => p.target.clone(),
        TypedValue::Pages(pages) => {
            pages.iter().map(|p| p.target.as_str()).collect::<Vec<_>>().join(", ")
        }
    }
}

#[cfg(test)]
#[path = "field_text_tests.rs"]
mod tests;
