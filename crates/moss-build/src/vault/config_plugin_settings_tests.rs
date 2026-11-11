use super::{plugin_settings, save_plugin_settings, write_plugin_settings};

fn save_plugin_setting(dir: &str, plugin: &str, path: &[String], value: Option<serde_json::Value>) -> Result<(), String> {
    write_plugin_settings(dir, plugin, &[(path.to_vec(), value)])
}
use serde_json::json;
use std::fs;

const CONFIG: &str = r#"schema_version = 6

# What the page offers.
[services.catalog]
currency = "usd"   # lowercase
open = { mon = ["09:00-17:00"], tue = ["09:00-17:00"] }

[services.catalog.items.walk]
name = "Guided walk"
per_hour = 4000
hours = { min = 2, max = 6 }

[services.other]
token_hint = "untouched"
"#;

fn folder() -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".moss")).unwrap();
    fs::write(tmp.path().join(".moss/config.toml"), CONFIG).unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    (tmp, path)
}

fn path(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn text(folder: &str) -> String {
    fs::read_to_string(std::path::Path::new(folder).join(".moss/config.toml")).unwrap()
}

/// The lines of `after` that are not in `before`, and the reverse.
fn changed(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    let added = a.iter().filter(|l| !b.contains(l)).map(|l| l.to_string()).collect();
    let removed = b.iter().filter(|l| !a.contains(l)).map(|l| l.to_string()).collect();
    (added, removed)
}

#[test]
fn one_changed_price_rewrites_one_line_and_keeps_every_comment() {
    let (_tmp, dir) = folder();
    save_plugin_setting(&dir, "catalog", &path(&["items", "walk", "per_hour"]), Some(json!(4500))).unwrap();
    let (added, removed) = changed(CONFIG, &text(&dir));
    assert_eq!(added, vec!["per_hour = 4500"]);
    assert_eq!(removed, vec!["per_hour = 4000"]);
}

#[test]
fn a_value_inside_an_inline_table_changes_only_that_table() {
    let (_tmp, dir) = folder();
    save_plugin_setting(&dir, "catalog", &path(&["open", "mon"]), Some(json!(["10:00-12:00", "13:00-18:00"]))).unwrap();
    save_plugin_setting(&dir, "catalog", &path(&["items", "walk", "hours", "max"]), Some(json!(8))).unwrap();
    let after = text(&dir);
    let (added, removed) = changed(CONFIG, &after);
    assert_eq!(removed.len(), 2, "{after}");
    assert!(added.iter().any(|l| l.starts_with("open = {") && l.contains("13:00-18:00") && l.contains("tue")), "{added:?}");
    assert!(added.iter().any(|l| l.starts_with("hours = {") && l.contains("max = 8")), "{added:?}");
    assert!(after.contains("# What the page offers.") && after.contains("# lowercase"));
}

#[test]
fn a_new_entry_and_a_removed_key_leave_the_rest_alone() {
    let (_tmp, dir) = folder();
    save_plugin_setting(&dir, "catalog", &path(&["items", "intro"]), Some(json!({"name": "Intro call", "minutes": 30, "price": 0}))).unwrap();
    save_plugin_setting(&dir, "catalog", &path(&["currency"]), None).unwrap();
    let values = plugin_settings(&dir, "catalog").unwrap();
    assert_eq!(values["items"]["intro"]["minutes"], 30);
    assert_eq!(values["items"]["walk"]["per_hour"], 4000);
    assert!(values.get("currency").is_none());
    assert!(text(&dir).contains("token_hint = \"untouched\""));
    // Removing under a table that is not there is a no-op, not a new table.
    save_plugin_setting(&dir, "catalog", &path(&["deposit", "percent"]), None).unwrap();
    assert!(plugin_settings(&dir, "catalog").unwrap().get("deposit").is_none());
    save_plugin_setting(&dir, "absent", &path(&["x"]), None).unwrap();
    assert!(!text(&dir).contains("[services.absent]"));
}

#[test]
fn no_path_reaches_outside_the_plugins_own_table() {
    let (_tmp, dir) = folder();
    for plugin in ["../site", "catalog.other", ""] {
        assert!(save_plugin_setting(&dir, plugin, &path(&["x"]), Some(json!(1))).is_err(), "{plugin}");
    }
    // A key named like a dotted path is one key, never a path into another table.
    save_plugin_setting(&dir, "catalog", &path(&["other.token_hint"]), Some(json!("x"))).unwrap();
    assert_eq!(plugin_settings(&dir, "other").unwrap()["token_hint"], "untouched");
    // A step that is a value, not a table, is refused rather than replaced.
    let err = save_plugin_setting(&dir, "catalog", &path(&["currency", "code"]), Some(json!("eur"))).unwrap_err();
    assert!(err.contains("not a table"), "{err}");
    assert!(save_plugin_setting(&dir, "catalog", &[], Some(json!(1))).is_err());
}

#[test]
fn a_change_of_several_keys_lands_in_one_write() {
    let (_tmp, dir) = folder();
    let item = |k: &str| path(&["items", "walk", k]);
    write_plugin_settings(&dir, "catalog", &[
        (item("charge"), Some(json!("fixed"))),
        (item("per_hour"), None),
        (item("hours"), None),
        (item("minutes"), Some(json!(60))),
        (item("price"), Some(json!(0))),
    ])
    .unwrap();
    let walk = &plugin_settings(&dir, "catalog").unwrap()["items"]["walk"];
    assert_eq!(walk, &json!({"name": "Guided walk", "charge": "fixed", "minutes": 60, "price": 0}));
    // A batch with one bad change writes nothing.
    let before = text(&dir);
    assert!(write_plugin_settings(&dir, "catalog", &[(item("name"), Some(json!("Walk"))), (path(&["currency", "code"]), Some(json!("x")))]).is_err());
    assert_eq!(text(&dir), before);
}

#[test]
fn a_save_writes_nothing_the_manifest_does_not_draw() {
    let (_tmp, dir) = folder();
    let manifest = crate::plugins::types::PluginManifest::parse(&json!({"name":"catalog","version":"1.0.0","contributes":{"site_settings":{"settings":[
        {"key":"currency","type":"string"},
        {"key":"open","type":"object","fields":[{"key":"mon","type":"array","items":{"type":"string"}}]}]}}}).to_string()).unwrap();
    let site = manifest.contributes.unwrap().site_settings.unwrap();
    save_plugin_settings(&dir, "catalog", &site, &[(path(&["currency"]), Some(json!("eur")))]).unwrap();
    let before = text(&dir);
    let private = json!({"mon": ["09:00-12:00"], "calendar": "https://cal.example/private.ics"});
    assert!(save_plugin_settings(&dir, "catalog", &site, &[(path(&["open"]), Some(private))]).is_err());
    assert_eq!(text(&dir), before, "a refused change writes nothing");
}
