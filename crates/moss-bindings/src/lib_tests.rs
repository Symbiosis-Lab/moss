use super::*;
use moss_core::schema::ContentSchema;
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
        "plugins": [
            {"name": "z-last", "fields": {"custom_flag": {"type": "string"}}},
            {"name": "example", "fields": {"custom_flag": {"type": "boolean"}}}
        ],
        "listing_styles": ["board"]
    }).to_string()).unwrap();
    let text = "---\ntitle: Example\nperformers: Example\ncustom_flag: true\nchildren_style: board\n---\nBody";
    assert_eq!(json_value(&site.check("a.md", text)), json!([]));
    assert_eq!(json_value(&site.schema_json())["frontmatter"]["fields"]["custom_flag"]["source"], "example");
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
    let schema: ContentSchema = serde_json::from_str(&site.schema_json()).unwrap();
    assert_eq!(schema.generator, "example");
    assert!(!schema.frontmatter.fields.contains_key("title"));
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
    let themed = Site::from_json(r#"{"files":["a.md"],"lang":"en","theme_css":"body { color: red; }"}"#).unwrap();
    let result = json_value(&themed.preview("a.md", "---\ntitle: Example\n---\nVisible **words**", "https://example.com/site/"));
    let html = result["html"].as_str().unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("<base href=\"https://example.com/site/\">"));
    // moss's own stylesheet first, the site's theme after it so the theme wins.
    let theme_at = html.find("body { color: red; }</style>").unwrap();
    assert!(html.find(".moss-article-title").is_some_and(|at| at < theme_at));
    assert!(html.contains("Visible <strong>words</strong>"));
    assert!(html.ends_with("</html>"));
    assert_eq!(result["title"], "Example");
    // The engine's revision for these bytes, so a host can order local and engine renders.
    assert_eq!(result["revision"], moss_core::page::content_hash("---\ntitle: Example\n---\nVisible **words**".as_bytes()));
    assert_eq!(result["revision"].as_str().unwrap().len(), 64);
    assert_eq!(result["diagnostics"], json!([]));
    let warned = json_value(&site().preview("a.md", ":::unknown\nWords\n:::", "/"));
    assert!(warned["diagnostics"].as_array().unwrap().iter().any(|d| d["severity"] == 2));
}

#[test]
fn discovered_notes_render_after_atomic_replacement() {
    let mut site = site();
    let text = "![[notes/b]]";
    assert_eq!(json_value(&site.embedded_notes("a.md", text)), json!(["notes/b.md"]));
    site.set_notes(r#"{"notes/b.md":"Embedded **words**"}"#).unwrap();
    let preview = || json_value(&site.preview("a.md", text, "/"))["html"].as_str().unwrap().to_owned();
    assert!(preview().contains("Embedded <strong>words</strong>"));
    assert!(site.set_notes(r#"{"notes/b.md":42}"#).is_err());
    assert!(site.preview("a.md", text, "/").contains("Embedded"));
    site.set_notes("{}").unwrap();
    assert!(!site.preview("a.md", text, "/").contains("Embedded"));
}

#[test]
fn initial_notes_are_available_to_preview() {
    let site = Site::from_json(r#"{"files":["b.md"],"lang":"en","notes":{"b.md":"Initial note"}}"#).unwrap();
    assert!(site.preview("a.md", "![[b]]", "/").contains("Initial note"));
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

#[test]
fn field_spans_cross_the_json_boundary_as_byte_offsets() {
    let text = "title: café\nnav\n---\nBody";
    assert_eq!(json_value(&field_spans(text)), json!([
        {"key": "title", "line": 1, "from": 0, "to": 5},
        {"key": "nav", "line": 2, "from": 13, "to": 16}
    ]));
}

#[test]
fn sources_json_round_trip_into_a_site() {
    let result = crate::site_bundle(r#"{
        "files":["post.md","index.md"],
        "texts":{"index.md":"---\nlang: fr\n---\nHome"},
        "config_toml":"[terms.people]\nfields = ['editor']",
        "plugins":[{"folder":"untrusted","manifest_json":"{}","entry_present":false,
            "admission":{"status":"refused","sentence":"host refused"}}],
        "future_field":true
    }"#).unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["bundle"]["lang"], "fr");
    assert_eq!(result["bundle"]["declared_name_lists"], serde_json::json!(["author", "tags", "editor"]));
    assert!(result["warnings"][0].as_str().unwrap().contains("host refused"));
    let site = crate::Site::from_json(&result["bundle"].to_string()).unwrap();
    assert!(site.preview("post.md", "# Article", "").contains("lang=\\\"fr\\\""));
    assert_eq!(crate::home_file(r#"["post.md","index.md"]"#).unwrap(), r#""index.md""#);
    assert_eq!(crate::home_file("[]").unwrap(), "null");
    assert!(crate::site_bundle("{").is_err());
    assert!(crate::home_file("{}").is_err());
    assert_eq!(crate::site_text_paths(r#"["z.MD","child/index.md","index.md","photo.jpg"]"#).unwrap(), r#"["index.md","z.MD"]"#);
    assert!(crate::site_text_paths("{}").is_err());
}
