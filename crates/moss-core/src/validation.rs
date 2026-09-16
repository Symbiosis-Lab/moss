//! Schema-driven frontmatter validation.
//!
//! Validates parsed frontmatter against a [`ContentSchema`], producing
//! LSP-compatible [`Diagnostic`] messages. Checks include:
//!
//! - Required fields missing
//! - Type mismatches (e.g. string where boolean expected)
//! - Enum constraint violations
//! - Date format validation (YYYY-MM-DD)
//! - Unknown fields (reported as `Hint`)

use crate::schema::{ContentSchema, FieldType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Frontmatter names moss does not use, paired with the field that does the job.
///
/// An unknown field is normally harmless — plugins and templates read their own
/// keys, so moss ignores what it doesn't recognize rather than rejecting it.
/// That silence is wrong for exactly one class of name: the field a writer
/// arrives with from another generator. `slug:` is the case that prompted this
/// — it is the custom-URL field in Hugo, Jekyll, Zola and Astro, moss spells it
/// `url:`, and writing `slug:` did nothing at all and said nothing about it.
///
/// Curated, not computed. Edit distance would pair `data:` with `date:` and
/// `image:` with nothing, producing confident wrong advice on fields that are
/// legitimately someone's own. Every entry here is a name that means something
/// specific somewhere else, so the suggestion is a translation rather than a
/// guess. Keys are compared after [`normalize_field_name`], so `publishDate`,
/// `publish_date` and `publish-date` all match one entry.
const FOREIGN_FIELD_HINTS: &[(&str, &str)] = &[
    // Custom URL segment — Hugo, Jekyll, Zola, Astro, Eleventy.
    ("slug", "url"),
    ("permalink", "url"),
    // Short blurb — Hugo (`summary`), Jekyll (`excerpt`).
    ("summary", "description"),
    ("excerpt", "description"),
    ("subtitle", "description"),
    // Taxonomy — Jekyll/Hugo split tags from categories; moss has one axis.
    ("categories", "tags"),
    ("category", "tags"),
    ("keywords", "tags"),
    // Lead image.
    ("image", "cover"),
    ("thumbnail", "cover"),
    ("banner", "cover"),
    ("featuredimage", "cover"),
    // Publication date — Astro (`pubDate`), assorted (`publishDate`).
    ("pubdate", "date"),
    ("publishdate", "date"),
    ("datepublished", "date"),
    // Singular in moss.
    ("authors", "author"),
    // Language.
    ("language", "lang"),
    ("locale", "lang"),
    // Manual ordering.
    ("order", "weight"),
    ("menuorder", "weight"),
    ("sortorder", "weight"),
];

/// Fold a frontmatter key to the form [`FOREIGN_FIELD_HINTS`] is keyed by:
/// lowercase, with `_` and `-` removed. `pubDate`, `pub_date` and `pub-date`
/// all fold to `pubdate`.
fn normalize_field_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The moss field a foreign frontmatter name most likely meant, if any.
///
/// Returns `None` for a name moss simply doesn't know — that is an ordinary
/// custom field and must stay silent. Callers should phrase the result as a
/// question, not a correction: a template really may read its own `image:`.
pub fn foreign_field_suggestion(name: &str) -> Option<&'static str> {
    let folded = normalize_field_name(name);
    FOREIGN_FIELD_HINTS
        .iter()
        .find(|(foreign, _)| *foreign == folded)
        .map(|(_, moss_field)| *moss_field)
}

/// Diagnostic severity levels (LSP-compatible integer values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Fatal error — the content cannot be published.
    Error = 1,
    /// Something likely wrong but not fatal.
    Warning = 2,
    /// Informational message.
    Info = 3,
    /// Suggestion or style hint.
    Hint = 4,
}

/// A validation diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Severity level.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Frontmatter field path (e.g. "title", "also_in[0]").
    pub path: Option<String>,
    /// Source line (1-based), if available.
    pub line: Option<usize>,
    /// Source column (1-based), if available.
    pub column: Option<usize>,
}

/// Validate parsed frontmatter against a content schema.
///
/// Returns a list of diagnostics. An empty list means the frontmatter is valid.
pub fn validate_frontmatter(
    fm: &HashMap<String, serde_yaml::Value>,
    schema: &ContentSchema,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // Check each field defined in the schema.
    for (name, def) in &schema.frontmatter.fields {
        match fm.get(name) {
            None => {
                if def.required {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!("required field '{}' is missing", name),
                        path: Some(name.clone()),
                        line: None,
                        column: None,
                    });
                }
            }
            Some(value) => {
                // Type check.
                if !value_matches_def(value, def) {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        message: format!(
                            "field '{}' has wrong type: expected {}, got {}",
                            name,
                            type_name(&def.field_type),
                            yaml_type_name(value),
                        ),
                        path: Some(name.clone()),
                        line: None,
                        column: None,
                    });
                }

                // Enum constraint check.
                if let Some(ref allowed) = def.enum_values {
                    if let Some(s) = value.as_str() {
                        if !allowed.contains(&s.to_string()) {
                            diags.push(Diagnostic {
                                severity: Severity::Error,
                                message: format!(
                                    "field '{}' has invalid value '{}'; allowed: {:?}",
                                    name, s, allowed
                                ),
                                path: Some(name.clone()),
                                line: None,
                                column: None,
                            });
                        }
                    }
                }

                // Date format validation for fields with format: "date".
                if def.format.as_deref() == Some("date") {
                    if let Some(s) = value.as_str() {
                        if !is_valid_date(s) {
                            diags.push(Diagnostic {
                                severity: Severity::Warning,
                                message: format!(
                                    "field '{}' has invalid date format '{}'; expected YYYY-MM-DD",
                                    name, s
                                ),
                                path: Some(name.clone()),
                                line: None,
                                column: None,
                            });
                        }
                    }
                }

                // Array item type check.
                if def.field_type == FieldType::Array {
                    if let (Some(items_def), Some(seq)) = (&def.items, value.as_sequence()) {
                        for (i, item) in seq.iter().enumerate() {
                            if !value_matches_type(item, &items_def.field_type) {
                                diags.push(Diagnostic {
                                    severity: Severity::Error,
                                    message: format!(
                                        "field '{}[{}]' has wrong type: expected {}, got {}",
                                        name,
                                        i,
                                        type_name(&items_def.field_type),
                                        yaml_type_name(item),
                                    ),
                                    path: Some(format!("{}[{}]", name, i)),
                                    line: None,
                                    column: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Check for unknown fields (not in schema) — report as Hint.
    // Skip keys that are internal (skip_schema) fields — they are managed by the
    // build pipeline and must not be flagged as unknown to the user.
    for key in fm.keys() {
        if schema.frontmatter.fields.contains_key(key)
            || schema.frontmatter.internal_fields.contains(key)
        {
            continue;
        }
        // Name a moss equivalent when the key is one another generator uses,
        // so the hint is actionable instead of merely true.
        let message = match foreign_field_suggestion(key) {
            Some(moss_field) => format!(
                "unknown field '{}' is not defined in the schema — did you mean '{}'?",
                key, moss_field
            ),
            None => format!("unknown field '{}' is not defined in the schema", key),
        };
        diags.push(Diagnostic {
            severity: Severity::Hint,
            message,
            path: Some(key.clone()),
            line: None,
            column: None,
        });
    }

    diags
}

/// Check if a YAML value satisfies a field definition. For `OneOf` unions the
/// value must match at least one member; otherwise it's a plain type check.
fn value_matches_def(value: &serde_yaml::Value, def: &crate::schema::FieldDefinition) -> bool {
    if def.field_type == FieldType::OneOf {
        return match &def.one_of {
            Some(members) => members.iter().any(|m| value_matches_def(value, m)),
            // A OneOf with no declared members accepts nothing meaningful;
            // treat as permissive to avoid false positives on malformed schemas.
            None => true,
        };
    }
    value_matches_type(value, &def.field_type)
}

/// Check if a YAML value matches a scalar/array field type.
fn value_matches_type(value: &serde_yaml::Value, expected: &FieldType) -> bool {
    match expected {
        FieldType::String => value.is_string(),
        // Mirrors deserialize_bool_lenient in frontmatter_typed.rs: the typed
        // build path coerces "true"/"false" strings, so this diagnostic must
        // accept them too or it'll flag a value the build path already fixed.
        FieldType::Boolean => {
            value.is_bool() || matches!(value.as_str(), Some("true") | Some("false"))
        }
        FieldType::Integer => {
            // Accept both i64 and u64.
            value.is_i64() || value.is_u64()
        }
        FieldType::Number => {
            // Accept integers and floats.
            value.is_number()
        }
        FieldType::Array => value.is_sequence(),
        FieldType::Object => value.is_mapping(),
        // OneOf is dispatched by value_matches_def before reaching here; a bare
        // OneOf with no members is permissive.
        FieldType::OneOf => true,
    }
}

/// Human-readable name for a field type.
fn type_name(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "string",
        FieldType::Boolean => "boolean",
        FieldType::Integer => "integer",
        FieldType::Number => "number",
        FieldType::Array => "array",
        FieldType::Object => "object",
        FieldType::OneOf => "one-of",
    }
}

/// Human-readable name for a YAML value's actual type.
fn yaml_type_name(value: &serde_yaml::Value) -> &'static str {
    match value {
        serde_yaml::Value::Null => "null",
        serde_yaml::Value::Bool(_) => "boolean",
        serde_yaml::Value::Number(n) => {
            if n.is_f64() && !n.is_i64() && !n.is_u64() {
                "number"
            } else {
                "integer"
            }
        }
        serde_yaml::Value::String(_) => "string",
        serde_yaml::Value::Sequence(_) => "array",
        serde_yaml::Value::Mapping(_) => "object",
        serde_yaml::Value::Tagged(_) => "tagged",
    }
}

/// Validate a date string in YYYY-MM-DD format.
///
/// Requires exactly 10 characters: 4 digits, dash, 2 digits, dash, 2 digits.
fn is_valid_date(s: &str) -> bool {
    // Strict format: YYYY-MM-DD (exactly 10 chars)
    if s.len() != 10 {
        return false;
    }

    let bytes = s.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }

    // Verify all digit positions are ASCII digits.
    for &i in &[0, 1, 2, 3, 5, 6, 8, 9] {
        if !bytes[i].is_ascii_digit() {
            return false;
        }
    }

    // The byte-position checks above guarantee the three-segment shape and
    // ASCII-digit content, so the parses below cannot fail today. But "panics
    // only when the author was right" is the same shape that just bit us in
    // `date.rs` — refactor the byte checks above and these `.unwrap()`s become
    // a panic on user input. Use slice-pattern destructuring + `let-else`
    // instead so the compiler enforces the three-segment shape, and bail
    // cleanly via `Result::Err` rather than a panic if parsing ever fails.
    let parts: Vec<&str> = s.split('-').collect();
    let [year_str, month_str, day_str] = parts.as_slice() else {
        return false;
    };
    let Ok(year) = year_str.parse::<u32>() else {
        return false;
    };
    let Ok(month) = month_str.parse::<u32>() else {
        return false;
    };
    let Ok(day) = day_str.parse::<u32>() else {
        return false;
    };

    if year < 1 || month < 1 || month > 12 || day < 1 {
        return false;
    }

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => return false,
    };

    day <= days_in_month
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::builtin_schema;

    fn make_fm(pairs: &[(&str, serde_yaml::Value)]) -> HashMap<String, serde_yaml::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn str_val(s: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(s.to_string())
    }

    fn bool_val(b: bool) -> serde_yaml::Value {
        serde_yaml::Value::Bool(b)
    }

    fn int_val(n: i64) -> serde_yaml::Value {
        serde_yaml::Value::Number(serde_yaml::Number::from(n))
    }

    #[test]
    fn test_valid_frontmatter() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("My Page")),
            ("date", str_val("2024-01-15")),
            ("draft", bool_val(false)),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_missing_required_title() {
        let schema = builtin_schema();
        let fm = make_fm(&[("date", str_val("2024-01-15"))]);

        let diags = validate_frontmatter(&fm, &schema);
        let missing: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("title"))
            .collect();
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn test_type_mismatch_string_for_boolean() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("draft", str_val("yes")), // should be boolean
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let type_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("wrong type"))
            .collect();
        assert_eq!(type_errs.len(), 1);
        assert!(type_errs[0].message.contains("draft"));
    }

    #[test]
    fn test_quoted_true_false_strings_accepted_for_boolean() {
        // #925: the typed build path (deserialize_bool_lenient) coerces
        // "true"/"false" strings for bool fields; this diagnostic must agree,
        // or the editor would show a fresh "wrong type" error for a value the
        // build path already accepts.
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("draft", str_val("true")),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let type_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("wrong type"))
            .collect();
        assert!(type_errs.is_empty(), "quoted \"true\" must not be flagged: {:?}", type_errs);
    }

    #[test]
    fn test_type_mismatch_boolean_for_string() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", bool_val(true)), // should be string
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let type_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("title"))
            .collect();
        assert_eq!(type_errs.len(), 1);
    }

    #[test]
    fn test_type_mismatch_string_for_integer() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("weight", str_val("heavy")), // should be integer
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let type_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("weight"))
            .collect();
        assert_eq!(type_errs.len(), 1);
    }

    #[test]
    fn test_enum_violation() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("children_style", str_val("table")), // not in ["list", "summary", "grid"]
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let enum_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("children_style"))
            .collect();
        assert_eq!(enum_errs.len(), 1);
        assert!(enum_errs[0].message.contains("table"));
    }

    #[test]
    fn test_enum_valid() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("children_style", str_val("list")),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let enum_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("children_style"))
            .collect();
        assert!(enum_errs.is_empty());
    }

    #[test]
    fn test_enum_summary_valid() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("children_style", str_val("summary")),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let enum_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("children_style"))
            .collect();
        assert!(enum_errs.is_empty());
    }

    #[test]
    fn test_enum_card_now_invalid() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("children_style", str_val("card")), // was valid, now invalid
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let enum_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("children_style"))
            .collect();
        assert_eq!(enum_errs.len(), 1);
        assert!(enum_errs[0].message.contains("card"));
    }

    #[test]
    fn test_invalid_date_format() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("date", str_val("01/15/2024")), // wrong format
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let date_warns: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("date"))
            .collect();
        assert_eq!(date_warns.len(), 1);
    }

    #[test]
    fn test_valid_date_format() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("date", str_val("2024-02-29")), // leap year
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let date_warns: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("date"))
            .collect();
        assert!(date_warns.is_empty());
    }

    #[test]
    fn test_invalid_leap_year() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("date", str_val("2023-02-29")), // not a leap year
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let date_warns: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("date"))
            .collect();
        assert_eq!(date_warns.len(), 1);
    }

    #[test]
    fn test_unknown_fields_are_hints() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("custom_field", str_val("value")),
            ("another_unknown", int_val(42)),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let hints: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Hint)
            .collect();
        assert_eq!(hints.len(), 2);
    }

    #[test]
    fn test_array_item_type_validation() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            (
                "also_in",
                serde_yaml::Value::Sequence(vec![
                    str_val("section-a"),
                    serde_yaml::Value::Number(serde_yaml::Number::from(42)), // wrong type
                ]),
            ),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let arr_errs: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("also_in[1]"))
            .collect();
        assert_eq!(arr_errs.len(), 1);
    }

    /// `sort:` takes an axis name or a list of child stems. The build has
    /// always honoured both and the field's own description documents both,
    /// but the schema declared a bare string — so a real site emitted
    /// "field 'sort' has wrong type: expected string, got array" on every
    /// folder index that spelled its order out. Both forms validate clean;
    /// a bad axis name is still an error.
    #[test]
    fn sort_accepts_an_axis_name_or_an_explicit_list() {
        let schema = builtin_schema();
        let errors = |fm: &HashMap<String, serde_yaml::Value>| -> Vec<String> {
            validate_frontmatter(fm, &schema)
                .into_iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message)
                .collect()
        };

        let list = make_fm(&[
            ("title", str_val("Test")),
            (
                "sort",
                serde_yaml::Value::Sequence(vec![
                    str_val("上篇"),
                    str_val("中篇"),
                    str_val("下篇"),
                ]),
            ),
        ]);
        assert!(errors(&list).is_empty(), "list form: {:?}", errors(&list));

        let axis = make_fm(&[("title", str_val("Test")), ("sort", str_val("weight"))]);
        assert!(errors(&axis).is_empty(), "axis form: {:?}", errors(&axis));

        let bogus = make_fm(&[("title", str_val("Test")), ("sort", str_val("banana"))]);
        assert!(
            errors(&bogus).iter().any(|m| m.contains("invalid value 'banana'")),
            "an unknown axis name must still be rejected: {:?}",
            errors(&bogus)
        );
    }

    #[test]
    fn test_valid_integer_field() {
        let schema = builtin_schema();
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("weight", int_val(10)),
        ]);

        let diags = validate_frontmatter(&fm, &schema);
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);
    }

    #[test]
    fn test_empty_frontmatter_only_required_errors() {
        let schema = builtin_schema();
        let fm = HashMap::new();

        let diags = validate_frontmatter(&fm, &schema);
        // Only "title" is required in the builtin schema
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("title"));
    }

    // --- Date validation unit tests ---

    #[test]
    fn skip_schema_fields_are_not_flagged_unknown() {
        let schema = builtin_schema();
        // `home` is a skip_schema field — it lives in internal_fields, not fields.
        // `bogus` is genuinely unknown.
        let fm = make_fm(&[
            ("title", str_val("Test")),
            ("home", bool_val(true)),
            ("bogus", int_val(1)),
        ]);
        let diags = validate_frontmatter(&fm, &schema);
        assert!(
            !diags.iter().any(|d| d.message.contains("'home'")),
            "skip_schema field 'home' must not produce an unknown-field hint"
        );
        assert!(
            diags.iter().any(|d| d.message.contains("'bogus'")),
            "genuinely unknown field 'bogus' must still produce an unknown-field hint"
        );
    }

    #[test]
    fn test_is_valid_date() {
        assert!(is_valid_date("2024-01-15"));
        assert!(is_valid_date("2024-02-29")); // leap year
        assert!(is_valid_date("2024-12-31"));
        assert!(is_valid_date("2000-02-29")); // century leap year

        assert!(!is_valid_date("2023-02-29")); // not leap year
        assert!(!is_valid_date("2024-13-01")); // month > 12
        assert!(!is_valid_date("2024-00-01")); // month 0
        assert!(!is_valid_date("2024-01-32")); // day > 31
        assert!(!is_valid_date("2024-04-31")); // April has 30 days
        assert!(!is_valid_date("not-a-date"));
        assert!(!is_valid_date("2024/01/15")); // wrong separator
        assert!(!is_valid_date("2024-1-5")); // this passes since parse() accepts it
        assert!(!is_valid_date("1900-02-29")); // not a leap year (divisible by 100 but not 400)
    }

    // -----------------------------------------------------------------------
    // Foreign-field hints
    // -----------------------------------------------------------------------

    #[test]
    fn slug_suggests_url() {
        // The case that prompted the table: `slug:` is the custom-URL field in
        // Hugo, Jekyll, Zola and Astro. moss spells it `url:` and used to
        // ignore `slug:` without a word.
        assert_eq!(foreign_field_suggestion("slug"), Some("url"));
    }

    #[test]
    fn a_name_moss_simply_does_not_know_stays_silent() {
        // Custom fields are legitimate — plugins and templates read their own
        // keys. Suggesting anything here would be noise on every build.
        assert_eq!(foreign_field_suggestion("bogus"), None);
        assert_eq!(foreign_field_suggestion("my_custom_thing"), None);
        // Near-misses that edit distance would have "corrected". `data:` is one
        // character from `date:` and is a perfectly ordinary custom field.
        assert_eq!(foreign_field_suggestion("data"), None);
        assert_eq!(foreign_field_suggestion("tag"), None);
    }

    #[test]
    fn case_and_separators_fold_to_one_entry() {
        // Astro writes `pubDate`, Hugo-era templates write `publish_date`, and
        // some exporters write `publish-date`. All are the same mistake.
        for spelling in ["pubDate", "PubDate", "pub_date", "pub-date", "PUBDATE"] {
            assert_eq!(
                foreign_field_suggestion(spelling),
                Some("date"),
                "{} should fold to the pubdate entry",
                spelling
            );
        }
        assert_eq!(foreign_field_suggestion("featuredImage"), Some("cover"));
    }

    #[test]
    fn every_suggested_field_actually_exists_in_the_schema() {
        // The failure this guards is worse than the silence it replaces:
        // pointing an author at a field moss also ignores. If a builtin field
        // is ever renamed, this fails instead of shipping confident bad advice.
        let schema = builtin_schema();
        for (foreign, suggested) in FOREIGN_FIELD_HINTS {
            assert!(
                schema.frontmatter.fields.contains_key(*suggested),
                "hint '{}' -> '{}' names a field the schema does not define",
                foreign,
                suggested
            );
        }
    }

    #[test]
    fn no_hint_key_is_itself_a_real_moss_field() {
        // A key that moss actually supports must never be reported as
        // meaningless. If moss ever adopts one of these names for real, the
        // entry has to go — this fails the moment that happens.
        let schema = builtin_schema();
        for (foreign, _) in FOREIGN_FIELD_HINTS {
            assert!(
                !schema
                    .frontmatter
                    .fields
                    .keys()
                    .any(|name| normalize_field_name(name) == *foreign),
                "'{}' is a real moss field and must not be listed as foreign",
                foreign
            );
        }
    }

    #[test]
    fn the_unknown_field_hint_carries_the_suggestion() {
        let schema = builtin_schema();
        let fm = make_fm(&[("title", str_val("Hi")), ("slug", str_val("privacy"))]);
        let diags = validate_frontmatter(&fm, &schema);
        let hint = diags
            .iter()
            .find(|d| d.path.as_deref() == Some("slug"))
            .expect("unknown field 'slug' must produce a diagnostic");
        assert_eq!(hint.severity, Severity::Hint);
        assert!(
            hint.message.contains("did you mean 'url'"),
            "hint should name the moss equivalent, got: {}",
            hint.message
        );
    }
}
