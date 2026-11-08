use super::*;
use serde_json::{json, Value};

fn value(result: Result<String, String>) -> Value {
    serde_json::from_str(&result.unwrap()).unwrap()
}

#[test]
fn tree_matches_the_engine_shape_and_filters_before_sorting() {
    let entries = json!([
        {"path":".hidden", "is_dir":false, "modified":null},
        {"path":"photo.png", "is_dir":false, "modified":123},
        {"path":"post.md", "is_dir":false, "modified":100, "frontmatter":"---\ndate: 2025-01-01\n---\n"},
        {"path":"index.md", "is_dir":false, "modified":1}
    ]);
    let tree = value(build_tree(&entries.to_string(), r#"{"root_name":"Site"}"#));
    assert_eq!(tree["name"], "Site");
    assert_eq!(tree["children"][0]["name"], "index.md");
    assert_eq!(tree["children"][0]["is_home"], true);
    assert_eq!(tree["children"][1]["publish_date"], "2025-01-01");
    assert_eq!(tree["children"].as_array().unwrap().len(), 3);
    for entries in ["bad", "{}", r#"[{"path":123,"is_dir":false}]"#] {
        assert!(build_tree(entries, r#"{"root_name":"Site"}"#).is_err());
    }
    assert!(build_tree("[]", "bad").is_err());
}

#[test]
fn save_uses_utf8_text_in_and_byte_arrays_out() {
    let out = value(decide_save(None, None, "{}", "河灣\n"));
    assert_eq!(out, json!({"status":"write", "bytes":"河灣\n".as_bytes()}));
    assert_eq!(value(decide_save(Some("same"), Some("stale"), "{}", "same")), json!({"status":"already-saved"}));
    let conflict = value(decide_save(Some("other writer"), Some("stale"), "{}", "mine"));
    assert_eq!(conflict["status"], "conflict");
    assert_eq!(conflict["disk"], value(parse_frontmatter("other writer")));
    assert!(decide_save(None, None, "{", "body").is_err());
    assert!(decide_save(None, None, "[]", "body").is_err());
}

#[test]
fn parse_reports_yaml_errors_without_losing_the_source() {
    let text = "---\r\ntitle: Example\r\n---\r\nbody\r\n";
    let parsed = value(parse_frontmatter(text));
    assert_eq!(parsed["frontmatter"]["title"], "Example");
    assert_eq!(parsed["hash"], page::content_hash(text.as_bytes()));
    assert_eq!(parsed["still_arriving"], false);
    let bad = "---\ntitle: [broken\n---\nbody";
    let parsed = value(parse_frontmatter(bad));
    assert!(parsed["frontmatter_error"].is_string());
    assert!(parsed["body"].as_str().unwrap().contains("[broken"));
}

#[test]
fn roles_and_headings_use_the_engine_rules() {
    assert_eq!(value(describe_source_role("zh-hans/footer.md")), json!({"language":"zh-hans", "slot":"footer-left", "can_be_page":true}));
    assert_eq!(value(describe_source_role("image.png"))["can_be_page"], false);
    assert_eq!(value(describe_source_role(""))["can_be_page"], false);
    let heading = value(compute_heading_state("post.md", r#"{"title":"Custom"}"#, "body"));
    assert_eq!(heading["text"], "Custom");
    assert_eq!(heading["visible"], true);
    assert_eq!(value(compute_heading_state("post.md", r#"{"title":""}"#, "body"))["visible"], false);
    assert!(compute_heading_state("post.md", "{", "body").is_err());
    assert!(compute_heading_state("post.md", "[]", "body").is_err());
}

#[test]
fn bootstrap_resolves_only_markdown_sources() {
    assert_eq!(value(bootstrap_source_path(Some("site"), Some("notes/a.md"))), json!("site/notes/a.md"));
    assert_eq!(value(bootstrap_source_path(None, Some("a.md"))), Value::Null);
    assert_eq!(value(bootstrap_source_path(Some("site"), Some("photo.png"))), Value::Null);
}

#[test]
fn tree_computes_settings_roles_from_config_and_file_facts() {
    let entries = r#"[{"path":"index.html","is_dir":false},{"path":"index.md","is_dir":false}]"#;
    let tree = value(build_tree(entries, r#"{"root_name":"Site","passthrough":["index.html"]}"#));
    assert_eq!(tree["children"][0]["special"], "page-settings");
}

#[test]
fn missing_disk_with_an_expected_hash_serializes_as_gone() {
    assert_eq!(value(decide_save(None, Some(&page::content_hash(b"old")), "{}", "new")), json!({"status":"gone"}));
}
