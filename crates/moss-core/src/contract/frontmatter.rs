//! Serializer that maps [`schema_fields::BUILTIN_FIELDS`] into
//! [`FrontmatterFieldJson`] entries for inclusion in the `moss describe --json`
//! payload.
//!
//! This keeps the describe contract in sync with the SSOT: adding a new entry
//! to `BUILTIN_FIELDS` automatically surfaces it here without any manual edit.

use crate::schema::{FieldType, Widget};
use crate::schema_fields::BUILTIN_FIELDS;
use serde::Serialize;

/// JSON representation of a single builtin frontmatter field, as surfaced in
/// `DescribePayload::frontmatter`.
#[derive(Serialize)]
pub struct FrontmatterFieldJson {
    /// Field name as it appears in YAML frontmatter (e.g. `"children_style"`).
    pub name: &'static str,
    /// Data type string (e.g. `"string"`, `"boolean"`, `"integer"`, `"array"`,
    /// `"object"`, `"one_of"`).
    #[serde(rename = "type")]
    pub field_type: &'static str,
    /// UI widget hint (e.g. `"select"`, `"text-input"`, `"checkbox"`).
    pub widget: &'static str,
    /// Default value as a raw JSON literal (e.g. `"true"`, `"\"list\""`).
    /// `null` when no default is defined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<&'static str>,
    /// Allowed values for `select` / enum fields. `null` for non-enum fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<&'static [&'static str]>,
    /// Human-readable description of the field.
    pub description: &'static str,
    /// UI group name for the add-property dropdown (e.g. `"Common"`, `"Children"`).
    /// Empty string for `skip_schema` (internal) fields.
    pub group: &'static str,
    /// When `true`, the field is internal to the build pipeline and is **not**
    /// exposed in the editor form or validation schema. Theme authors and content
    /// authors can ignore these fields; they are included here for tooling
    /// completeness.
    pub skip_schema: bool,
}

/// Serialize `FieldType` to the lowercase wire name used in the JSON contract.
fn field_type_str(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "string",
        FieldType::Boolean => "boolean",
        FieldType::Integer => "integer",
        FieldType::Number => "number",
        FieldType::Array => "array",
        FieldType::Object => "object",
        FieldType::OneOf => "one_of",
    }
}

/// Serialize `Widget` to the kebab-case wire name used in the JSON contract.
fn widget_str(w: &Widget) -> &'static str {
    match w {
        Widget::TextInput => "text-input",
        Widget::TextArea => "text-area",
        Widget::DatePicker => "date-picker",
        Widget::NumberInput => "number-input",
        Widget::Checkbox => "checkbox",
        Widget::Select => "select",
        Widget::TagInput => "tag-input",
        Widget::FilePicker => "file-picker",
        Widget::CodeEditor => "code-editor",
        Widget::Union => "union",
        Widget::WikilinkPicker => "wikilink-picker",
        Widget::WikilinkListPicker => "wikilink-list-picker",
    }
}

/// Build the list of [`FrontmatterFieldJson`] entries from [`BUILTIN_FIELDS`].
///
/// All fields are included — both those exposed in the editor schema and those
/// marked `skip_schema: true` (internal/build-only). Callers can distinguish
/// them via `FrontmatterFieldJson::skip_schema`.
pub fn frontmatter_fields() -> Vec<FrontmatterFieldJson> {
    BUILTIN_FIELDS
        .iter()
        .map(|bf| FrontmatterFieldJson {
            name: bf.name,
            field_type: field_type_str(&bf.field_type),
            widget: widget_str(&bf.widget),
            default: bf.default_json,
            enum_values: bf.enum_values,
            description: bf.description,
            group: bf.group,
            skip_schema: bf.skip_schema,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_fields_non_empty_and_includes_children_style() {
        let fields = frontmatter_fields();
        assert!(
            fields.len() >= 30,
            "expected at least 30 fields, got {}",
            fields.len()
        );

        let cs = fields
            .iter()
            .find(|f| f.name == "children_style")
            .expect("children_style must be present");

        assert_eq!(cs.field_type, "string");
        assert_eq!(cs.widget, "select");
        assert_eq!(
            cs.enum_values,
            Some(&["list", "summary", "grid"][..])
        );
        assert!(!cs.skip_schema);
        assert_eq!(cs.group, "Child Styles");
    }

    #[test]
    fn frontmatter_fields_includes_skip_schema_entries() {
        let fields = frontmatter_fields();
        let uid = fields.iter().find(|f| f.name == "uid").expect("uid");
        assert!(uid.skip_schema, "uid must be skip_schema");
    }

    /// `skip_schema` fields are filtered out of `moss describe`'s HUMAN
    /// output but not out of `--json`, so this description is a published,
    /// machine-readable contract string — and it is the SSOT that
    /// `docs/reference/contract.md` and the hooks-site contract fixture are
    /// generated from. It described the uid as derivable for five separate
    /// surfaces' worth of corrections; `generate_uid` ignores its path
    /// argument and returns random bytes, so a plugin author who recomputed a
    /// uid to re-join `.moss/social/*.json` would miss on every key.
    #[test]
    fn uid_description_never_claims_the_uid_is_derivable() {
        let fields = frontmatter_fields();
        let uid = fields.iter().find(|f| f.name == "uid").expect("uid");
        let lower = uid.description.to_lowercase();
        assert!(
            !lower.contains("content-addressab"),
            "uid is RANDOM, not content-addressable: {:?}",
            uid.description
        );
        assert!(
            lower.contains("random"),
            "the uid description must say it is random: {:?}",
            uid.description
        );
    }

    #[test]
    fn frontmatter_fields_no_duplicate_names() {
        let fields = frontmatter_fields();
        let mut seen = std::collections::HashSet::new();
        for f in &fields {
            assert!(
                seen.insert(f.name),
                "duplicate name '{}' in frontmatter_fields()",
                f.name
            );
        }
    }

}
