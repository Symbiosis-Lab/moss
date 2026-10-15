//! Declared frontmatter fields: the type a site gives a field name, and the
//! typed value a page's raw frontmatter value parses to.
//!
//! Pure: no I/O. The declarations are read from `.moss/config.toml` one layer
//! up; this module only says what a value must look like and what it becomes.

use crate::date::{parse_edtf, Edtf, EdtfDate};
use crate::media::{split_pipe, wikilink_inner};
use serde::Serialize;
use serde_json::Value;

/// The type a declared field holds. Names match the `type` key in config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Date,
    Url,
    Choice,
    Period,
    Count,
    Page,
    Pages,
}

const KIND_NAMES: &[(&str, FieldKind)] = &[
    ("text", FieldKind::Text),
    ("number", FieldKind::Number),
    ("date", FieldKind::Date),
    ("url", FieldKind::Url),
    ("choice", FieldKind::Choice),
    ("period", FieldKind::Period),
    ("count", FieldKind::Count),
    ("page", FieldKind::Page),
    ("pages", FieldKind::Pages),
];

impl FieldKind {
    /// Every accepted `type` name, in the order a diagnostic lists them.
    pub fn names() -> Vec<&'static str> {
        KIND_NAMES.iter().map(|(name, _)| *name).collect()
    }

    pub fn from_name(name: &str) -> Option<FieldKind> {
        KIND_NAMES.iter().find(|(n, _)| *n == name).map(|(_, kind)| *kind)
    }
}

/// One `[fields.<name>]` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDecl {
    pub name: String,
    pub kind: FieldKind,
    /// For Choice: allowed values in declaration order, each with its label.
    pub choices: Vec<Choice>,
    /// Where the field is placed on the page, if anywhere.
    pub place: Option<Place>,
    /// Folder prefixes (site-relative, no leading slash) this field applies to; empty = whole site.
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Eyebrow,
    Subtitle,
    Facts,
    KeyFigure,
}

const PLACE_NAMES: &[(&str, Place)] = &[
    ("eyebrow", Place::Eyebrow),
    ("subtitle", Place::Subtitle),
    ("facts", Place::Facts),
    ("key-figure", Place::KeyFigure),
];

/// The `place` names a declaration may use. `none` is also accepted in config
/// and means no place; it is not in this table because it has no `Place`.
pub fn place_names() -> Vec<&'static str> {
    PLACE_NAMES.iter().map(|(name, _)| *name).collect()
}

pub fn parse_place(s: &str) -> Option<Place> {
    PLACE_NAMES.iter().find(|(n, _)| *n == s).map(|(_, place)| *place)
}

/// A parsed field value, shaped the way plugins receive it. Serialized
/// untagged, so the JSON is each variant's own fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TypedValue {
    Text(String),
    Number(serde_json::Number),
    Date(EdtfDate),
    Url(String),
    Choice { value: String, label: String },
    Period { start: EdtfDate, end: Option<EdtfDate>, text: String },
    Count {
        #[serde(skip_serializing_if = "Option::is_none")]
        number: Option<serde_json::Number>,
        #[serde(skip_serializing_if = "Option::is_none")]
        noun: Option<String>,
        text: String,
    },
    Page(PageLink),
    Pages(Vec<PageLink>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageLink {
    pub target: String,
}

/// Checks one raw frontmatter value against its declaration and returns the
/// typed value plugins receive. `Err` carries a reason a person can act on;
/// the caller adds the file and field name.
pub fn parse_value(decl: &FieldDecl, value: &Value) -> Result<TypedValue, String> {
    if value.is_null() {
        return Err("has no value; give it one or remove the line".to_string());
    }
    match decl.kind {
        FieldKind::Text => match value {
            Value::String(s) => Ok(TypedValue::Text(s.clone())),
            Value::Number(n) => Ok(TypedValue::Text(n.to_string())),
            Value::Bool(b) => Ok(TypedValue::Text(b.to_string())),
            _ => Err("expected text".to_string()),
        },
        FieldKind::Number => match value {
            Value::Number(n) => Ok(TypedValue::Number(n.clone())),
            Value::String(s) => s
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(TypedValue::Number)
                .ok_or_else(|| format!("\"{s}\" is not a number")),
            _ => Err("expected a number".to_string()),
        },
        FieldKind::Date => {
            let s = date_text(value).ok_or_else(|| "expected a date like 1930-10-12, 1930-10 or 1930".to_string())?;
            match parse_edtf(&s) {
                Some(Edtf::Date(date)) => Ok(TypedValue::Date(date)),
                Some(Edtf::Interval { .. }) => {
                    Err(format!("\"{s}\" is an interval; a date field takes one date, such as 1930-10"))
                }
                None => Err(format!("\"{s}\" is not a date; write YYYY, YYYY-MM or YYYY-MM-DD")),
            }
        }
        FieldKind::Url => match value {
            Value::String(s) if s.starts_with("http://") || s.starts_with("https://") || s.starts_with('/') => {
                Ok(TypedValue::Url(s.clone()))
            }
            _ => Err("expected an address starting with http://, https:// or /".to_string()),
        },
        FieldKind::Choice => {
            let raw = match value {
                Value::String(s) => s.as_str(),
                other => return Err(format!("{other} is not one of: {}", allowed(decl))),
            };
            match decl.choices.iter().find(|c| c.value == raw) {
                Some(choice) => Ok(TypedValue::Choice { value: choice.value.clone(), label: choice.label.clone() }),
                None => Err(format!("\"{raw}\" is not one of: {}", allowed(decl))),
            }
        }
        FieldKind::Period => {
            let s = date_text(value).ok_or_else(|| "expected a period like 1927/1933".to_string())?;
            match parse_edtf(&s) {
                Some(Edtf::Date(start)) => Ok(TypedValue::Period { start, end: None, text: s }),
                Some(Edtf::Interval { start, end }) => Ok(TypedValue::Period { start, end, text: s }),
                None => Err(format!(
                    "\"{s}\" is not a period; write 1927/1933, 1927–1933, 1927/.. for an open end, or one date"
                )),
            }
        }
        FieldKind::Count => match value {
            Value::Number(n) => Ok(TypedValue::Count { number: Some(n.clone()), noun: None, text: n.to_string() }),
            Value::String(s) => Ok(parse_count(s).unwrap_or_else(|| TypedValue::Count {
                number: None,
                noun: None,
                text: s.clone(),
            })),
            _ => Err("expected text like \"1,382 flats\" or a number".to_string()),
        },
        FieldKind::Page => match value {
            Value::String(s) => page_target(s)
                .map(|target| TypedValue::Page(PageLink { target }))
                .ok_or_else(|| "write it as \"[[name]]\" so moss rename keeps it pointing at the page".to_string()),
            Value::Array(_) => Err("write the link in quotes, as \"[[name]]\"".to_string()),
            _ => Err("expected a page link written as \"[[name]]\"".to_string()),
        },
        FieldKind::Pages => {
            let items: Vec<&Value> = match value {
                Value::Array(items) => items.iter().collect(),
                single => vec![single],
            };
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let Some(target) = item.as_str().and_then(page_target) else {
                    return Err(format!(
                        "{} is not a page link; write each one in quotes, as \"[[name]]\"",
                        item_text(item)
                    ));
                };
                out.push(PageLink { target });
            }
            Ok(TypedValue::Pages(out))
        }
    }
}

/// Whether a declaration applies to a page, by its site-relative source path.
pub fn applies_to(decl: &FieldDecl, source_path: &str) -> bool {
    if decl.scope.is_empty() {
        return true;
    }
    let path = strip_leading(source_path);
    decl.scope.iter().any(|entry| {
        let entry = strip_leading(entry).trim_end_matches('/');
        entry.is_empty() || path == entry || path.starts_with(&format!("{entry}/"))
    })
}

fn strip_leading(s: &str) -> &str {
    let mut s = s;
    loop {
        if let Some(rest) = s.strip_prefix("./") {
            s = rest;
        } else if let Some(rest) = s.strip_prefix('/') {
            s = rest;
        } else {
            return s;
        }
    }
}

fn allowed(decl: &FieldDecl) -> String {
    if decl.choices.is_empty() {
        return "(no values are declared)".to_string();
    }
    decl.choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")
}

fn item_text(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// A date as the author wrote it: a string, or a bare YAML year (`1930`).
fn date_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) if n.is_i64() || n.is_u64() => Some(n.to_string()),
        _ => None,
    }
}

/// `1,382 flats` → `{number: 1382, noun: "flats", text}`. A leading number with
/// no noun, or prose such as `about 65 million`, is `None`: the caller keeps the text.
fn parse_count(s: &str) -> Option<TypedValue> {
    let is_grouping = |c: char| c.is_ascii_digit() || matches!(c, ',' | '.' | '\u{202F}' | '\u{00A0}' | ' ');
    if !s.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let run_end = s.char_indices().find(|(_, c)| !is_grouping(*c)).map_or(s.len(), |(i, _)| i);
    let (run, _) = s.split_at(run_end);
    // The number ends at its last digit; anything after that is separator or noun.
    let number_end = run.char_indices().rfind(|(_, c)| c.is_ascii_digit()).map(|(i, c)| i + c.len_utf8())?;
    let (number_text, tail) = s.split_at(number_end);
    let noun = tail.trim_start();
    if tail.len() == noun.len() || !noun.starts_with(char::is_alphabetic) {
        return None;
    }
    let clean: String = number_text.chars().filter(|c| !matches!(c, ',' | '\u{202F}' | '\u{00A0}' | ' ')).collect();
    let number = match clean.parse::<i64>() {
        Ok(i) => serde_json::Number::from(i),
        Err(_) => serde_json::Number::from_f64(clean.parse::<f64>().ok()?)?,
    };
    Some(TypedValue::Count { number: Some(number), noun: Some(noun.to_string()), text: s.to_string() })
}

/// A `[[target]]` or `[[target|alias]]` link, returning the target.
fn page_target(s: &str) -> Option<String> {
    let (target, _alias) = split_pipe(wikilink_inner(s)?);
    let target = target.trim();
    (!target.is_empty()).then(|| target.to_string())
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
