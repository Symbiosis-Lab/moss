//! Content model types driven by the schema.
//!
//! The schema defines frontmatter fields, types, and UI widget hints.
//! The built-in schema is generated from [`crate::schema_fields::BUILTIN_FIELDS`],
//! the single source of truth for all frontmatter fields moss recognizes.
//! Plugin-contributed schemas are still parsed from JSON via [`parse_schema()`].
//!
//! All types derive `Serialize` + `Deserialize` so they can cross the
//! Tauri command boundary.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::schema_fields::{BuiltinField, BUILTIN_FIELDS};

/// The top-level content schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ContentSchema {
    /// Generator that produced this schema (e.g. "moss").
    pub generator: String,
    /// Schema format version.
    pub version: String,
    /// Frontmatter field definitions.
    pub frontmatter: FrontmatterSchema,
    /// Optional shortcode definitions.
    pub shortcodes: Option<ShortcodeSchema>,
}

/// Frontmatter schema: a map of field name to definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FrontmatterSchema {
    /// Field definitions keyed by field name.
    pub fields: HashMap<String, FieldDefinition>,
    /// Field names that exist in the build pipeline's `FrontMatter` struct
    /// but are not surfaced in the editor form (the `skip_schema: true`
    /// entries from `BUILTIN_FIELDS`). The editor uses this list to filter
    /// unknown values it shouldn't render as chips — auto-generated fields,
    /// build-only fields, and site-level config.
    ///
    /// Empty for plugin-contributed schemas loaded from JSON (they have no
    /// notion of internal fields). `#[serde(default)]` keeps backward
    /// compatibility with externally-loaded schema JSON that predates this
    /// field.
    #[serde(default)]
    pub internal_fields: Vec<String>,
}

/// A single field definition in the frontmatter schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FieldDefinition {
    /// The data type of the field.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Which UI widget to render for this field.
    /// `None` for sub-definitions (e.g. array item types) that have
    /// no direct UI representation.
    #[serde(default)]
    pub widget: Option<Widget>,
    /// Whether this field is required.
    #[serde(default)]
    pub required: bool,
    /// Default value for the field.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Format hint (e.g. "date" for YYYY-MM-DD).
    #[serde(default)]
    pub format: Option<String>,
    /// Allowed values for select/enum fields.
    #[serde(default)]
    pub enum_values: Option<Vec<String>>,
    /// Item definition for array fields.
    #[serde(default)]
    #[cfg_attr(feature = "specta", specta(type = Option<serde_json::Value>))]
    pub items: Option<Box<FieldDefinition>>,
    /// Member variants for a `OneOf` union field. Each member is a full
    /// `FieldDefinition` carrying its own `field_type` + `widget`. Only set
    /// when `field_type == OneOf`. The specta override mirrors `items` to dodge
    /// the self-referential type (the chip bar reads members structurally from
    /// JSON; it needs no nominal TS type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "specta", specta(type = Option<Vec<serde_json::Value>>))]
    pub one_of: Option<Vec<FieldDefinition>>,
    /// Human-readable description of the field.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional human-readable label for the chip bar. When `None`, the frontend
    /// falls back to using the field key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// i18n key for the chip bar label, resolved by the TypeScript registry.
    /// `None` when no key is registered (frontend falls back to field name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    /// Display score for chip bar ordering and add-property search list ordering.
    /// Lower values appear first / sort higher in the list.
    /// Formula: score = 100 - (Frequency*6 + Importance*4); see schema_fields.rs.
    /// 0 means unset (skip-schema or plugin-contributed fields, sort to end).
    /// Serialized as "priority" for backwards compatibility with the frontend.
    #[serde(default, rename = "priority")]
    pub score: u8,
    /// Source of this field definition.
    /// `None` for builtin fields, `Some("review")` for plugin-contributed fields.
    /// Used by the frontend to group fields by source in the editor form.
    /// See docs/reference/plugin-schema-contributions.md.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// UI group name for the add-property dropdown (e.g. "Common", "Children").
    /// `None` for plugin-contributed fields (shown in "Other" group).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// For `Widget::FilePicker` fields, the extension kinds the picker should
    /// restrict search results to (e.g. `cover` -> image/video, `logo` ->
    /// image only). `None` means unrestricted. Mirrors
    /// `schema_fields::BuiltinField::file_kinds` — see that field's doc comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_kinds: Option<Vec<crate::resolve::ext_kind::ExtKind>>,
}

/// Supported field types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Boolean,
    Integer,
    Number,
    Array,
    Object,
    /// A union of member variants (see `FieldDefinition::one_of`). The authored
    /// value matches exactly one member. Used for fields like `children`
    /// (bool | wikilink) and `series` (bool | wikilink-list) whose real value
    /// space the older scalar types could not express.
    OneOf,
}

/// UI widget types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum Widget {
    TextInput,
    TextArea,
    DatePicker,
    NumberInput,
    Checkbox,
    Select,
    TagInput,
    FilePicker,
    CodeEditor,
    /// Parent dispatcher for a `OneOf` field: reads `one_of` and renders the
    /// active branch's UI. Never falls through to value-type inference.
    Union,
    /// Single wikilink/path picker with folder autocomplete (e.g. `children`).
    WikilinkPicker,
    /// Ordered list of wikilinks (e.g. `series` explicit order).
    WikilinkListPicker,
}

/// Shortcode schema: delimiters and named definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShortcodeSchema {
    /// (open_delimiter, close_delimiter) pair.
    pub delimiters: (String, String),
    /// Named shortcode definitions.
    pub definitions: HashMap<String, ShortcodeDefinition>,
}

/// A single shortcode definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShortcodeDefinition {
    /// Display name.
    pub name: String,
    /// Whether the shortcode wraps content.
    #[serde(default)]
    pub has_content: bool,
    /// Parameter definitions.
    #[serde(default)]
    pub params: Vec<ShortcodeParam>,
}

/// A shortcode parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ShortcodeParam {
    /// Parameter name.
    pub name: String,
    /// Whether the parameter is required.
    #[serde(default)]
    pub required: bool,
    /// Default value.
    #[serde(default)]
    pub default: Option<String>,
}

/// Materialize one owned [`FieldDefinition`] from a const [`BuiltinField`].
///
/// Recurses for union fields: a `BuiltinField` carrying `one_of_members`
/// becomes a `OneOf` definition whose `one_of` is each member materialized the
/// same way. This is what lets the const table (scalars only) describe a union
/// whose owned representation is a `Vec<FieldDefinition>` built here, not in
/// const context — exactly the pattern `items` already uses.
fn materialize_field(bf: &BuiltinField) -> FieldDefinition {
    let items = bf.items_type.as_ref().map(|it| {
        Box::new(FieldDefinition {
            field_type: it.clone(),
            widget: None,
            required: false,
            default: None,
            format: None,
            enum_values: None,
            items: None,
            one_of: None,
            description: None,
            label: None,
            label_key: None,
            score: 0,
            source: None,
            group: None,
            file_kinds: None,
        })
    });

    let one_of = bf
        .one_of_members
        .map(|members| members.iter().map(materialize_field).collect());

    let default = bf.default_json.map(|s| {
        serde_json::from_str(s)
            .unwrap_or_else(|e| panic!("invalid default_json for '{}': {}", bf.name, e))
    });

    let enum_values = bf
        .enum_values
        .map(|vals| vals.iter().map(|s| s.to_string()).collect());

    FieldDefinition {
        field_type: bf.field_type.clone(),
        widget: Some(bf.widget.clone()),
        required: bf.required,
        default,
        format: bf.format.map(|s| s.to_string()),
        enum_values,
        items,
        one_of,
        description: if bf.description.is_empty() {
            None
        } else {
            Some(bf.description.to_string())
        },
        label: bf.label.map(|s| s.to_string()),
        label_key: if bf.label_key.is_empty() { None } else { Some(bf.label_key.to_string()) },
        score: bf.score,
        source: None,
        group: if bf.group.is_empty() { None } else { Some(bf.group.to_string()) },
        file_kinds: bf.file_kinds.map(|kinds| kinds.to_vec()),
    }
}

/// Return the built-in content schema.
///
/// Builds the schema programmatically from [`BUILTIN_FIELDS`] — the const
/// table in `schema_fields.rs`. Fields with `skip_schema: true` are excluded.
/// This replaces the previous `include_str!("builtin-schema.json")` approach,
/// ensuring the schema and the `FrontMatter` struct can never drift apart.
pub fn builtin_schema() -> ContentSchema {
    let mut fields = HashMap::new();
    let mut internal_fields = Vec::new();

    for bf in BUILTIN_FIELDS {
        if bf.skip_schema {
            internal_fields.push(bf.name.to_string());
            continue;
        }

        fields.insert(bf.name.to_string(), materialize_field(bf));
    }

    ContentSchema {
        generator: "moss".to_string(),
        version: "1.0".to_string(),
        frontmatter: FrontmatterSchema { fields, internal_fields },
        shortcodes: Some(ShortcodeSchema {
            delimiters: (":::".to_string(), ":::".to_string()),
            definitions: HashMap::new(),
        }),
    }
}

/// Parse a content schema from a JSON string.
pub fn parse_schema(json: &str) -> Result<ContentSchema, String> {
    serde_json::from_str(json).map_err(|e| format!("schema parse error: {}", e))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_schema_parses() {
        let schema = builtin_schema();
        assert_eq!(schema.generator, "moss");
        assert_eq!(schema.version, "1.0");
    }

    #[test]
    fn test_builtin_schema_has_required_title() {
        let schema = builtin_schema();
        let title = schema.frontmatter.fields.get("title").expect("title field");
        assert!(title.required);
        assert_eq!(title.field_type, FieldType::String);
        assert_eq!(title.widget, Some(Widget::TextInput));
    }

    #[test]
    fn test_builtin_schema_field_count() {
        let schema = builtin_schema();
        // 34 non-skip fields: 32 prior baseline + `author`, `publisher`,
        // `external_url` (added by moss-import / linkblog work, 2026-05).
        // (`email_subject`/`email_preview` were removed with the send-modal
        // redesign — the composer's editable fields superseded them.)
        // (`unlisted` removed 2026-06 — redundant with `draft`.)
        // (`listed` added 2026-06 — off-feed-but-indexable axis, orthogonal to `draft`.)
        // (`layout` un-hidden 2026-07-31 — it was mismarked skip_schema, which
        // hid it from the editor chip bar despite being a real per-page control.)
        // (`byline`/`colophon` added 2026-08 — authored credit rows rendered at
        // the head and the foot of an article.)
        // (`author_page`/`tag_page` added 2026-09 — term-page claims; see
        // moss_core::terms.)
        assert_eq!(schema.frontmatter.fields.len(), 40);
    }

    #[test]
    fn test_all_non_skip_fields_have_group() {
        let schema = builtin_schema();
        for (name, field) in &schema.frontmatter.fields {
            assert!(
                field.group.is_some(),
                "field '{}' is exposed in schema but has no group",
                name
            );
        }
    }

    #[test]
    fn test_builtin_schema_date_field() {
        let schema = builtin_schema();
        let date = schema.frontmatter.fields.get("date").expect("date field");
        assert_eq!(date.field_type, FieldType::String);
        assert_eq!(date.widget, Some(Widget::DatePicker));
        assert_eq!(date.format.as_deref(), Some("date"));
    }

    #[test]
    fn test_builtin_schema_children_union() {
        // `children` is a OneOf union: bool toggle OR a wikilink/path picker.
        let schema = builtin_schema();
        let children = schema.frontmatter.fields.get("children").expect("children field");
        assert_eq!(children.field_type, FieldType::OneOf, "children should be a union");
        assert_eq!(children.widget, Some(Widget::Union), "children should use the union widget");
        let members = children.one_of.as_ref().expect("children should have one_of members");
        assert_eq!(members.len(), 2, "children union has two members");
        assert_eq!(members[0].field_type, FieldType::Boolean);
        assert_eq!(members[0].widget, Some(Widget::Checkbox));
        assert_eq!(members[1].field_type, FieldType::String);
        assert_eq!(members[1].widget, Some(Widget::WikilinkPicker));
    }

    #[test]
    fn test_builtin_schema_series_union() {
        // `series` is a OneOf union: bool flag OR an ordered wikilink list.
        let schema = builtin_schema();
        let series = schema.frontmatter.fields.get("series").expect("series field");
        assert_eq!(series.field_type, FieldType::OneOf, "series should be a union");
        assert_eq!(series.widget, Some(Widget::Union));
        let members = series.one_of.as_ref().expect("series one_of members");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].field_type, FieldType::Boolean);
        assert_eq!(members[1].field_type, FieldType::Array);
        assert_eq!(members[1].widget, Some(Widget::WikilinkListPicker));
        // The array member carries an items definition (string).
        let items = members[1].items.as_ref().expect("series list items");
        assert_eq!(items.field_type, FieldType::String);
    }

    /// Type-aware sync guard (the forcing function against schema↔compiler drift):
    /// every declared member form of each `OneOf` field must round-trip through
    /// the shared normalizer to its canonical form. If a member is declared that
    /// the normalizer doesn't honor (or vice versa), this fails.
    #[test]
    fn union_members_round_trip() {
        use crate::frontmatter_union::{normalize_children, normalize_series};
        use serde_yaml::Value;

        // children: Boolean member ⇒ bool passes through; String member ⇒ source.
        assert!(normalize_children(&Value::Bool(true)).children);
        let n = normalize_children(&Value::String("[[News]]".into()));
        assert!(n.children && n.source.as_deref() == Some("[[News]]"));

        // series: Boolean member ⇒ flag; Array member ⇒ order list.
        assert!(normalize_series(&Value::Bool(true)).series);
        let s = normalize_series(&Value::Sequence(vec![Value::String("[[A]]".into())]));
        assert!(s.series && s.order.as_deref() == Some(&["[[A]]".to_string()][..]));

        // Both union fields must actually be OneOf with exactly their declared members.
        let schema = builtin_schema();
        for name in ["children", "series"] {
            let f = schema.frontmatter.fields.get(name).unwrap();
            assert_eq!(f.field_type, FieldType::OneOf, "{name} must be OneOf");
            let m = f.one_of.as_ref().unwrap_or_else(|| panic!("{name} needs one_of"));
            assert_eq!(m.len(), 2, "{name} has 2 members");
            assert_eq!(m[0].field_type, FieldType::Boolean, "{name} member 0 is the bool branch");
        }
    }

    #[test]
    fn test_builtin_schema_sidebar_field() {
        // sidebar: wikilink string for folder whose children appear in sidebar
        let schema = builtin_schema();
        let sidebar = schema.frontmatter.fields.get("sidebar").expect("sidebar field");
        assert_eq!(sidebar.field_type, FieldType::String, "sidebar should be string");
        assert_eq!(sidebar.widget, Some(Widget::TextInput), "sidebar should use text-input widget");
    }

    #[test]
    fn test_builtin_schema_also_in_array() {
        let schema = builtin_schema();
        let ai = schema.frontmatter.fields.get("also_in").expect("also_in field");
        assert_eq!(ai.field_type, FieldType::Array);
        assert_eq!(ai.widget, Some(Widget::TagInput));
        let items = ai.items.as_ref().expect("items");
        assert_eq!(items.field_type, FieldType::String);
    }

    #[test]
    fn test_builtin_schema_boolean_fields() {
        let schema = builtin_schema();
        for name in &["draft", "breadcrumb", "listed"] {
            let field = schema.frontmatter.fields.get(*name)
                .unwrap_or_else(|| panic!("{} field missing", name));
            assert_eq!(field.field_type, FieldType::Boolean, "{} should be boolean", name);
            assert_eq!(field.widget, Some(Widget::Checkbox), "{} should be checkbox", name);
        }
    }

    #[test]
    fn test_builtin_schema_integer_fields() {
        let schema = builtin_schema();
        for name in &["weight"] {
            let field = schema.frontmatter.fields.get(*name)
                .unwrap_or_else(|| panic!("{} field missing", name));
            assert_eq!(field.field_type, FieldType::Integer, "{} should be integer", name);
            assert_eq!(field.widget, Some(Widget::NumberInput), "{} should be number-input", name);
        }
    }

    #[test]
    fn test_builtin_schema_shortcodes() {
        let schema = builtin_schema();
        let sc = schema.shortcodes.as_ref().expect("shortcodes");
        assert_eq!(sc.delimiters, (":::".to_string(), ":::".to_string()));
    }

    #[test]
    fn test_parse_schema_invalid_json() {
        let result = parse_schema("not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("schema parse error"));
    }

    #[test]
    fn test_parse_schema_missing_fields() {
        let json = r#"{"generator":"test","version":"1.0"}"#;
        let result = parse_schema(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip_serialization() {
        let schema = builtin_schema();
        let json = serde_json::to_string(&schema).expect("serialize");
        let parsed = parse_schema(&json).expect("re-parse");
        assert_eq!(schema.generator, parsed.generator);
        assert_eq!(schema.frontmatter.fields.len(), parsed.frontmatter.fields.len());
    }

    #[test]
    fn test_field_type_serde() {
        // Verify the lowercase rename works
        let json = r#""string""#;
        let ft: FieldType = serde_json::from_str(json).expect("parse field type");
        assert_eq!(ft, FieldType::String);

        let json = r#""boolean""#;
        let ft: FieldType = serde_json::from_str(json).expect("parse boolean");
        assert_eq!(ft, FieldType::Boolean);
    }

    #[test]
    fn test_widget_serde() {
        // Verify the kebab-case rename works
        let json = r#""text-input""#;
        let w: Widget = serde_json::from_str(json).expect("parse widget");
        assert_eq!(w, Widget::TextInput);

        let json = r#""date-picker""#;
        let w: Widget = serde_json::from_str(json).expect("parse date-picker");
        assert_eq!(w, Widget::DatePicker);
    }

    #[test]
    fn test_builtin_fields_have_no_source() {
        let schema = builtin_schema();
        for (name, field) in &schema.frontmatter.fields {
            assert!(field.source.is_none(), "builtin field '{}' should have no source", name);
        }
    }

    #[test]
    fn test_field_definition_with_source_roundtrips() {
        let json = r#"{"type":"string","widget":"text-input","source":"review"}"#;
        let fd: FieldDefinition = serde_json::from_str(json).expect("parse");
        assert_eq!(fd.source, Some("review".to_string()));
        let serialized = serde_json::to_string(&fd).expect("serialize");
        assert!(serialized.contains(r#""source":"review""#));
    }

    #[test]
    fn test_field_definition_without_source_omits_it() {
        let json = r#"{"type":"string","widget":"text-input"}"#;
        let fd: FieldDefinition = serde_json::from_str(json).expect("parse");
        assert!(fd.source.is_none());
        let serialized = serde_json::to_string(&fd).expect("serialize");
        assert!(!serialized.contains("source"));
    }
}
