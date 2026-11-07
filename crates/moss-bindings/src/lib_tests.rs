use super::*;
use moss_core::validation::Severity;
use serde_json::{json, Value};

fn site() -> Site {
    Site::from_json(r#"{"files":["a.md","notes/b.md"],"lang":"en"}"#).unwrap()
}

fn json_value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn composes_optional_site_and_plugin_fields_once() {
    let mut site = Site::from_json(&json!({
        "files": [], "lang": "en",
        "declared_name_lists": ["performers"],
        "plugins": [{"name": "example", "fields": {"custom_flag": {"type": "boolean"}}}],
        "listing_styles": ["board"]
    }).to_string()).unwrap();
    let text = "---\ntitle: Example\nperformers: Example\ncustom_flag: true\nchildren_style: board\n---\nBody";
    assert_eq!(json_value(&site.check("a.md", text)), json!([]));
    assert_eq!(site.inputs.schema.frontmatter.fields["custom_flag"].source.as_deref(), Some("example"));
    site.set_notes("{}").unwrap();
    assert_eq!(json_value(&site.check("a.md", text)), json!([]));
    let invalid = json_value(&site.check("a.md", "---\ntitle: Example\ncustom_flag: [bad]\n---"));
    assert_eq!(invalid[0]["severity"], 1);
    assert_eq!(invalid[0]["path"], "custom_flag");
}

#[test]
fn explicit_schema_takes_precedence_over_composition() {
    let site = Site::from_json(&json!({
        "files": [], "lang": "en",
        "schema": {
            "generator": "example", "version": "1",
            "frontmatter": {"fields": {"custom_flag": {"type": "boolean"}}}
        },
        "plugins": [{"name": "example", "fields": {"custom_flag": {"type": "string"}}}]
    }).to_string()).unwrap();
    assert_eq!(json_value(&site.check("a.md", "---\ncustom_flag: true\n---")), json!([]));
    assert_eq!(site.inputs.schema.generator, "example");
    assert!(!site.inputs.schema.frontmatter.fields.contains_key("title"));
}

#[test]
fn omitted_schema_uses_builtins() {
    assert_eq!(json_value(&site().check("a.md", "---\ntitle: Example\n---\nBody")), json!([]));
    let schema: ContentSchema = serde_json::from_str(&builtin_schema_json()).unwrap();
    assert!(schema.frontmatter.fields.contains_key("title"));
}

#[test]
fn check_preserves_whole_file_lines_and_editor_shape() {
    let result = json_value(&site().check("a.md", "---\ntitle: Example\ndraft: [bad]\n---\n\n[[missing]]\n\n:::grid 2\nLeft\n---\nRight\n:::"));
    let items = result.as_array().unwrap();
    for (severity, line) in [(1, 3), (2, 6), (4, 10)] {
        assert!(items.iter().any(|d| d["severity"] == severity && d["line"] == line), "{result}");
    }
    let error = items.iter().find(|d| d["severity"] == 1).unwrap();
    assert_eq!(error["path"], "draft");
    assert!(error["message"].is_string());
    assert!(error["column"].is_null());
    assert_eq!(error.as_object().unwrap().len(), 5);
}

#[test]
fn all_severities_are_lsp_numbers_including_info() {
    let items = diagnostics([Severity::Error, Severity::Warning, Severity::Info, Severity::Hint]
        .into_iter().map(|severity| Diagnostic {
            severity, message: "example".into(), path: None, line: None, column: None,
        }).collect());
    let value = serde_json::to_value(items).unwrap();
    assert_eq!(value.as_array().unwrap().iter().map(|d| d["severity"].clone()).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
}

#[test]
fn preview_wraps_page_text_title_language_css_and_base() {
    let result = json_value(&site().preview("a.md", "---\ntitle: Example\n---\nVisible **words**", "body { color: red; }", "https://example.com/site/"));
    let html = result["html"].as_str().unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("<base href=\"https://example.com/site/\">"));
    assert!(html.contains("<style>body { color: red; }</style>"));
    assert!(html.contains("Visible <strong>words</strong>"));
    assert!(html.ends_with("</html>"));
    assert_eq!(result["title"], "Example");
    assert_eq!(result["diagnostics"], json!([]));
    let warned = json_value(&site().preview("a.md", ":::unknown\nWords\n:::", "", "/"));
    assert!(warned["diagnostics"].as_array().unwrap().iter().any(|d| d["severity"] == 2));
}

#[test]
fn discovered_notes_render_after_atomic_replacement() {
    let mut site = site();
    let text = "![[notes/b]]";
    assert_eq!(json_value(&site.embedded_notes("a.md", text)), json!(["notes/b.md"]));
    site.set_notes(r#"{"notes/b.md":"Embedded **words**"}"#).unwrap();
    let preview = || json_value(&site.preview("a.md", text, "", "/"))["html"].as_str().unwrap().to_owned();
    assert!(preview().contains("Embedded <strong>words</strong>"));
    assert!(site.set_notes(r#"{"notes/b.md":42}"#).is_err());
    assert!(site.preview("a.md", text, "", "/").contains("Embedded"));
    site.set_notes("{}").unwrap();
    assert!(!site.preview("a.md", text, "", "/").contains("Embedded"));
}

#[test]
fn initial_notes_are_available_to_preview() {
    let site = Site::from_json(r#"{"files":["b.md"],"lang":"en","notes":{"b.md":"Initial note"}}"#).unwrap();
    assert!(site.preview("a.md", "![[b]]", "", "/").contains("Initial note"));
}

#[test]
fn malformed_json_and_wrong_types_are_errors_without_panics() {
    for text in ["", "{", "null", "[]", "{}", r#"{"files":42,"lang":"en"}"#, r#"{"files":[],"lang":"en","schema":{}}"#, r#"{"files":[],"lang":"en","plugins":[{"name":"x","fields":{"x":{"type":"wrong"}}}]}"#] {
        assert!(Site::from_json(text).is_err(), "{text}");
    }
    let mut site = site();
    for text in ["", "{", "null", "[]", r#"{"b.md":false}"#] {
        assert!(site.set_notes(text).is_err(), "{text}");
    }
}
