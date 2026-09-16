use super::*;

#[test]
fn test_no_duplicate_field_names() {
    let mut seen = std::collections::HashSet::new();
    for field in BUILTIN_FIELDS {
        assert!(
            seen.insert(field.name),
            "duplicate field name '{}' in BUILTIN_FIELDS",
            field.name
        );
    }
}

#[test]
fn test_array_fields_have_items_type() {
    for field in BUILTIN_FIELDS {
        if field.field_type == FieldType::Array {
            assert!(
                field.items_type.is_some(),
                "array field '{}' must have items_type set",
                field.name
            );
        }
    }
}

#[test]
fn test_labels_propagate_to_schema() {
    let schema = crate::schema::builtin_schema();
    let depth = schema
        .frontmatter
        .fields
        .get("children_depth")
        .expect("children_depth");
    assert_eq!(depth.label.as_deref(), Some("Depth"));
}

#[test]
fn test_no_label_means_none() {
    let schema = crate::schema::builtin_schema();
    let title = schema.frontmatter.fields.get("title").expect("title");
    assert!(title.label.is_none());
}

#[test]
fn test_select_fields_have_enum_values() {
    for field in BUILTIN_FIELDS {
        if field.widget == Widget::Select && !field.skip_schema {
            assert!(
                field.enum_values.is_some(),
                "select widget field '{}' should have enum_values",
                field.name
            );
        }
    }
}

#[test]
fn test_all_non_skip_fields_have_a_group() {
    for field in BUILTIN_FIELDS {
        if !field.skip_schema {
            assert!(
                !field.group.is_empty(),
                "field '{}' has skip_schema=false but no group",
                field.name
            );
        }
    }
}

#[test]
fn test_groups_are_valid_scope_groups() {
    const VALID: &[&str] = &["This Page", "Child Pages", "Child Styles", "Whole Site"];
    for field in BUILTIN_FIELDS {
        if !field.skip_schema {
            assert!(
                VALID.contains(&field.group),
                "field '{}' has unexpected group '{}'; expected one of {:?}",
                field.name,
                field.group,
                VALID
            );
        }
    }
}

#[test]
fn test_score_in_valid_range() {
    for field in BUILTIN_FIELDS {
        if !field.skip_schema {
            // score=0 is reserved for skip_schema fields; non-skip fields need a score
            assert!(
                field.score > 0,
                "non-skip field '{}' has score=0; set a score >= 1",
                field.name
            );
        }
    }
}

/// `skip_schema: true` hides a field from the editor chip bar entirely (see
/// module docs). A field with `enum_values`/a form widget looks like a
/// user-facing control, so marking one `skip_schema` is very likely a
/// miscategorization (this happened to `layout`: it shipped skip_schema
/// and was invisible in the chip bar for its whole life, caught only when
/// a user asked why it didn't show up). Fields that are genuinely
/// build-only/auto-generated/site-level must be named here explicitly, so
/// adding a new one to that set is a deliberate, reviewable change instead
/// of just placing an entry under a comment header.
const INTENTIONALLY_SKIP_SCHEMA: &[&str] = &[
    "home",
    "analytics",
    "uid",
    "cover_type",
    "children_source",
    "_from_sidebar_alias",
];

#[test]
fn test_skip_schema_fields_are_on_the_allowlist() {
    for field in BUILTIN_FIELDS {
        if field.skip_schema {
            assert!(
                INTENTIONALLY_SKIP_SCHEMA.contains(&field.name),
                "field '{}' is skip_schema=true but not in INTENTIONALLY_SKIP_SCHEMA — \
                     if it's a real per-page/site control (has enum_values, a widget, a \
                     user-facing description) it should be skip_schema=false with a group \
                     instead; if it's genuinely internal, add its name to the allowlist",
                field.name
            );
        }
    }
}

#[test]
fn every_file_picker_field_declares_file_kinds() {
    // Both directions of the SSOT `asset_field_names` reads: a FilePicker
    // field must declare the extension filter its picker needs, and nothing
    // else may claim to be a file picker without one.
    for f in BUILTIN_FIELDS {
        if matches!(f.widget, Widget::FilePicker) {
            assert!(
                f.file_kinds.is_some(),
                "FilePicker field `{}` declares no file_kinds",
                f.name
            );
        }
    }
}

#[test]
fn asset_field_names_is_exactly_cover_and_logo() {
    let names: Vec<&str> = asset_field_names().collect();
    assert_eq!(names, vec!["cover", "logo"]);
}
