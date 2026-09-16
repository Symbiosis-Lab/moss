//! Normalization for union-typed frontmatter fields (`children`, `series`,
//! `byline`, `colophon`).
//!
//! These fields accept more than one authored shape — `children` is a bool OR a
//! wikilink/path string; `series` is a bool OR an ordered list of wikilinks;
//! `byline` and `colophon` are one credit string OR a list of them. The
//! schema models them as [`crate::schema::FieldType::OneOf`]; this module is the
//! SINGLE place that maps an authored value to its canonical resolved form.
//!
//! Both the build pipeline and the editor's save path call these functions, so
//! the compiler and the editor can never interpret a value differently. The
//! functions are pure (zero I/O) and operate on [`serde_yaml::Value`] so a caller
//! holding a typed struct value can feed it via `serde_yaml::to_value` and a
//! caller holding a raw map can feed the entry directly.
//!
//! The decision tables below are the contract — they are mirrored by the
//! `union_members_round_trip` sync test and by the per-row unit tests at the
//! bottom of this file. Changing a row means changing both.

use serde_yaml::Value;

/// Canonical resolved form of the `children` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildrenNorm {
    /// Whether to render a children feed at all.
    pub children: bool,
    /// The folder reference (wikilink `[[News]]` or resolved path
    /// `news/index.md`) when targeting a different folder; `None` for the
    /// page's own direct children.
    pub source: Option<String>,
}

/// Canonical resolved form of the `series` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesNorm {
    /// Whether the page participates in a sequential series.
    pub series: bool,
    /// Explicit child order (list of wikilinks) when authored as a list;
    /// `None` for the bool flag form (order falls back to `sort`/weight).
    pub order: Option<Vec<String>>,
}

/// Normalize an authored `children` value.
///
/// | Input value                         | Output                              |
/// |-------------------------------------|-------------------------------------|
/// | `Bool(true)`                        | `{children:true,  source:None}`     |
/// | `Bool(false)`                       | `{children:false, source:None}`     |
/// | `String("true")` / `String("false")`| parsed as the bool                  |
/// | `String("")`                        | `{children:false, source:None}`     |
/// | `String(s)` non-empty, non-bool     | `{children:true,  source:Some(s)}`  |
/// | absent / null / number / map / seq  | `{children:false, source:None}`     |
pub fn normalize_children(v: &Value) -> ChildrenNorm {
    match v {
        Value::Bool(b) => ChildrenNorm { children: *b, source: None },
        Value::String(s) => {
            let t = s.trim();
            match t {
                "true" => ChildrenNorm { children: true, source: None },
                "false" | "" => ChildrenNorm { children: false, source: None },
                _ => ChildrenNorm { children: true, source: Some(s.clone()) },
            }
        }
        _ => ChildrenNorm { children: false, source: None },
    }
}

/// Normalize an authored `series` value.
///
/// | Input value                    | Output                                  |
/// |--------------------------------|-----------------------------------------|
/// | `Bool(b)`                      | `{series:b, order:None}`                |
/// | `String("true")`/`"false"`     | parsed as the bool                      |
/// | `Sequence` of strings          | `{series:true, order:Some(strings)}`    |
/// | `Sequence` with any non-string | `{series:false, order:None}` (malformed)|
/// | empty `Sequence`               | `{series:true, order:None}`             |
/// | absent / other                 | `{series:false, order:None}`            |
pub fn normalize_series(v: &Value) -> SeriesNorm {
    match v {
        Value::Bool(b) => SeriesNorm { series: *b, order: None },
        Value::String(s) => match s.trim() {
            "true" => SeriesNorm { series: true, order: None },
            _ => SeriesNorm { series: false, order: None },
        },
        Value::Sequence(items) => {
            if items.is_empty() {
                return SeriesNorm { series: true, order: None };
            }
            let mut order = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Value::String(s) => order.push(s.clone()),
                    // Any non-string element ⇒ malformed; ignore the whole list.
                    _ => return SeriesNorm { series: false, order: None },
                }
            }
            SeriesNorm { series: true, order: Some(order) }
        }
        _ => SeriesNorm { series: false, order: None },
    }
}

/// Normalize an authored credit value — `byline:` or `colophon:` — into its
/// display rows.
///
/// Both are plain display strings, one row per authored entry, and they differ
/// only in where they render (head vs foot), so they share this one rule. A
/// single string is one row; a YAML block scalar is one row per line (real
/// bylines run to three: writer, editor, first publisher); a list is one row
/// per item, and an item that itself spans lines splits further. Blank lines
/// are dropped and every row is trimmed, so the trailing newline a block
/// scalar always carries never becomes an empty row.
///
/// | Input value                                | Output                       |
/// |--------------------------------------------|------------------------------|
/// | `String("作者 糜緒洋")`                      | `["作者 糜緒洋"]`             |
/// | block scalar `"作者 X\n編輯 Y\n"`            | `["作者 X", "編輯 Y"]`        |
/// | `Sequence(["作者 X", "編輯 Y"])`             | `["作者 X", "編輯 Y"]`        |
/// | `String("")` / whitespace / empty sequence  | `[]`                         |
/// | `Null`                                      | `[]`                         |
/// | `Bool` / `Number` / `Mapping`               | `Err(_)`                     |
/// | sequence holding a non-string item          | `Err(_)`                     |
///
/// Errors carry an author-facing message naming the shape that was found.
/// This is why the field does NOT use `#[serde(untagged)]`: an untagged enum
/// collapses every failure into "data did not match any variant of untagged
/// enum", which names neither the field nor the problem.
pub fn normalize_credit_rows(v: &Value) -> Result<Vec<String>, String> {
    fn push_rows(out: &mut Vec<String>, raw: &str) {
        for line in raw.lines() {
            let t = line.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }

    let mut rows = Vec::new();
    match v {
        Value::Null => {}
        Value::String(s) => push_rows(&mut rows, s),
        Value::Sequence(items) => {
            for it in items {
                match it {
                    Value::String(s) => push_rows(&mut rows, s),
                    Value::Null => {}
                    other => {
                        return Err(format!(
                            "expected a string or a list of strings, found a list holding {}",
                            shape_name(other)
                        ))
                    }
                }
            }
        }
        other => {
            return Err(format!(
                "expected a string or a list of strings, found {}",
                shape_name(other)
            ))
        }
    }
    Ok(rows)
}

/// Normalize an authored term-claim value (`author_page:` / `tag_page:`).
///
/// | Input value                          | Output                          |
/// |--------------------------------------|---------------------------------|
/// | `Bool(true)` / `String("true")`      | `Some(TermClaim::UseTitle)`     |
/// | `Bool(false)` / `String("false"/"")` | `None` (no claim)               |
/// | `String(s)` non-empty, non-bool      | `Some(TermClaim::Name(s))`      |
/// | `Null`                               | `None`                          |
/// | other shapes                         | `Err(_)`                        |
pub fn normalize_term_claim(v: &Value) -> Result<Option<crate::terms::TermClaim>, String> {
    use crate::terms::TermClaim;
    match v {
        Value::Null => Ok(None),
        Value::Bool(true) => Ok(Some(TermClaim::UseTitle)),
        Value::Bool(false) => Ok(None),
        Value::String(s) => {
            let t = s.trim();
            match t {
                "true" => Ok(Some(TermClaim::UseTitle)),
                "false" | "" => Ok(None),
                _ => Ok(Some(TermClaim::Name(t.to_string()))),
            }
        }
        other => Err(format!(
            "expected true, or the claimed name as a string, found {}",
            shape_name(other)
        )),
    }
}

/// Normalize an authored `author` value into a list of names.
///
/// A single string is ONE name, kept verbatim — `author: "方六、常籮"` and
/// `author: "A and B"` are one pre-formatted entry, because splitting on
/// prose separators would guess. A YAML list is one name per item; the list
/// form is how co-authors are stated structurally. Unlike
/// [`normalize_credit_rows`], a multi-line string does NOT split: a name is
/// not a credit block.
///
/// | Input value                       | Output               |
/// |-----------------------------------|----------------------|
/// | `String("馬欣宜")`                 | `["馬欣宜"]`          |
/// | `Sequence(["方六", "常籮"])`        | `["方六", "常籮"]`     |
/// | `String("")` / blank / empty seq  | `[]`                 |
/// | `Null`                            | `[]`                 |
/// | `Bool` / `Number` / `Mapping`     | `Err(_)`             |
/// | sequence holding a non-string     | `Err(_)`             |
pub fn normalize_name_list(v: &Value) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut push = |raw: &str| {
        let t = raw.trim();
        if !t.is_empty() {
            names.push(t.to_string());
        }
    };
    match v {
        Value::Null => {}
        Value::String(s) => push(s),
        Value::Sequence(items) => {
            for it in items {
                match it {
                    Value::String(s) => push(s),
                    Value::Null => {}
                    other => {
                        return Err(format!(
                            "expected a name or a list of names, found a list holding {}",
                            shape_name(other)
                        ))
                    }
                }
            }
        }
        other => {
            return Err(format!(
                "expected a name or a list of names, found {}",
                shape_name(other)
            ))
        }
    }
    Ok(names)
}

/// Author-facing name for a YAML value's shape, used in `normalize_credit_rows`
/// errors. Deliberately plain words, not serde type names.
fn shape_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "nothing",
        Value::Bool(_) => "a true/false value",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Sequence(_) => "a list",
        Value::Mapping(_) => "a set of key/value pairs",
        Value::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::String(v.to_string())
    }

    // --- normalize_children: one assertion per decision-table row ---

    #[test]
    fn children_bool_true() {
        assert_eq!(
            normalize_children(&Value::Bool(true)),
            ChildrenNorm { children: true, source: None }
        );
    }

    #[test]
    fn children_bool_false() {
        assert_eq!(
            normalize_children(&Value::Bool(false)),
            ChildrenNorm { children: false, source: None }
        );
    }

    #[test]
    fn children_string_true_false() {
        assert_eq!(
            normalize_children(&s("true")),
            ChildrenNorm { children: true, source: None }
        );
        assert_eq!(
            normalize_children(&s("false")),
            ChildrenNorm { children: false, source: None }
        );
    }

    #[test]
    fn children_empty_string_is_off() {
        assert_eq!(
            normalize_children(&s("")),
            ChildrenNorm { children: false, source: None }
        );
    }

    #[test]
    fn children_wikilink() {
        assert_eq!(
            normalize_children(&s("[[News]]")),
            ChildrenNorm { children: true, source: Some("[[News]]".to_string()) }
        );
    }

    #[test]
    fn children_bare_path() {
        // B3: the compiler accepts resolved paths, not just wikilinks.
        assert_eq!(
            normalize_children(&s("news/index.md")),
            ChildrenNorm { children: true, source: Some("news/index.md".to_string()) }
        );
    }

    #[test]
    fn children_other_types_off() {
        assert_eq!(
            normalize_children(&Value::Null),
            ChildrenNorm { children: false, source: None }
        );
        assert_eq!(
            normalize_children(&Value::Number(3.into())),
            ChildrenNorm { children: false, source: None }
        );
    }

    // --- normalize_series: one assertion per decision-table row ---

    #[test]
    fn series_bool() {
        assert_eq!(
            normalize_series(&Value::Bool(true)),
            SeriesNorm { series: true, order: None }
        );
        assert_eq!(
            normalize_series(&Value::Bool(false)),
            SeriesNorm { series: false, order: None }
        );
    }

    #[test]
    fn series_list_of_strings() {
        let seq = Value::Sequence(vec![s("[[Ch 1]]"), s("[[Ch 2]]")]);
        assert_eq!(
            normalize_series(&seq),
            SeriesNorm {
                series: true,
                order: Some(vec!["[[Ch 1]]".to_string(), "[[Ch 2]]".to_string()])
            }
        );
    }

    #[test]
    fn series_malformed_list_off() {
        let seq = Value::Sequence(vec![s("[[Ch 1]]"), Value::Bool(true)]);
        assert_eq!(
            normalize_series(&seq),
            SeriesNorm { series: false, order: None }
        );
    }

    #[test]
    fn series_empty_list_is_flag() {
        assert_eq!(
            normalize_series(&Value::Sequence(vec![])),
            SeriesNorm { series: true, order: None }
        );
    }

    #[test]
    fn series_other_off() {
        assert_eq!(
            normalize_series(&Value::Null),
            SeriesNorm { series: false, order: None }
        );
    }

    // --- round-trip: a value re-expressed via serde_yaml::to_value (the build
    //     side's call shape) normalizes identically to the raw value (editor). ---

    #[test]
    fn children_roundtrip_via_to_value() {
        let raw = s("[[News]]");
        let reexpressed = serde_yaml::to_value("[[News]]").unwrap();
        assert_eq!(normalize_children(&raw), normalize_children(&reexpressed));
    }

    // --- normalize_credit_rows: one assertion per decision-table row ---

    #[test]
    fn credit_single_string_is_one_row() {
        assert_eq!(normalize_credit_rows(&s("作者 糜緒洋")).unwrap(), vec!["作者 糜緒洋"]);
    }

    #[test]
    fn credit_block_scalar_is_one_row_per_line() {
        // What `byline: |` yields: lines plus the trailing newline it always carries.
        let block = s("作者　糜緒洋\n編輯　謝丁\n首發媒體　[端傳媒](https://x)\n");
        assert_eq!(
            normalize_credit_rows(&block).unwrap(),
            vec!["作者　糜緒洋", "編輯　謝丁", "首發媒體　[端傳媒](https://x)"]
        );
    }

    #[test]
    fn credit_list_is_one_row_per_item() {
        let seq = Value::Sequence(vec![s("作者 X"), s("編輯 Y")]);
        assert_eq!(normalize_credit_rows(&seq).unwrap(), vec!["作者 X", "編輯 Y"]);
    }

    #[test]
    fn credit_rows_are_trimmed_and_blank_lines_dropped() {
        assert_eq!(
            normalize_credit_rows(&s("  作者 X  \n\n   \n編輯 Y\n")).unwrap(),
            vec!["作者 X", "編輯 Y"]
        );
    }

    #[test]
    fn credit_empty_shapes_yield_no_rows() {
        assert!(normalize_credit_rows(&s("")).unwrap().is_empty());
        assert!(normalize_credit_rows(&s("   \n  ")).unwrap().is_empty());
        assert!(normalize_credit_rows(&Value::Sequence(vec![])).unwrap().is_empty());
        assert!(normalize_credit_rows(&Value::Null).unwrap().is_empty());
    }

    #[test]
    fn credit_wrong_shapes_report_what_was_found() {
        let err = normalize_credit_rows(&Value::Bool(true)).unwrap_err();
        assert!(err.contains("true/false"), "message names the shape: {err}");
        let err = normalize_credit_rows(&Value::Mapping(Default::default())).unwrap_err();
        assert!(err.contains("key/value"), "message names the shape: {err}");
        let err = normalize_credit_rows(&Value::Sequence(vec![s("作者 X"), Value::Bool(true)])).unwrap_err();
        assert!(err.contains("list holding"), "message names the offending item: {err}");
    }

    // --- normalize_name_list: one assertion per decision-table row ---

    #[test]
    fn author_single_string_is_one_name_verbatim() {
        assert_eq!(normalize_name_list(&s("馬欣宜")).unwrap(), vec!["馬欣宜"]);
        // A pre-formatted co-author string is ONE entry: moss never splits prose.
        assert_eq!(normalize_name_list(&s("方六、常籮")).unwrap(), vec!["方六、常籮"]);
        // Unlike credit rows, a multi-line string does not split into entries.
        assert_eq!(normalize_name_list(&s("A\nB")).unwrap(), vec!["A\nB"]);
    }

    #[test]
    fn author_list_is_one_name_per_item() {
        let seq = Value::Sequence(vec![s(" 方六 "), s("常籮")]);
        assert_eq!(normalize_name_list(&seq).unwrap(), vec!["方六", "常籮"]);
    }

    #[test]
    fn author_empty_shapes_yield_no_names() {
        assert!(normalize_name_list(&s("")).unwrap().is_empty());
        assert!(normalize_name_list(&s("   ")).unwrap().is_empty());
        assert!(normalize_name_list(&Value::Sequence(vec![])).unwrap().is_empty());
        assert!(normalize_name_list(&Value::Null).unwrap().is_empty());
    }

    #[test]
    fn author_wrong_shapes_report_what_was_found() {
        let err = normalize_name_list(&Value::Number(1.into())).unwrap_err();
        assert!(err.contains("a number"), "message names the shape: {err}");
        let err = normalize_name_list(&Value::Sequence(vec![Value::Bool(true)])).unwrap_err();
        assert!(err.contains("list holding"), "message names the offending item: {err}");
    }
}
