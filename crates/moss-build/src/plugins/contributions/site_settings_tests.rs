use crate::plugins::types::PluginManifest;
use serde_json::{json, Value};

fn parse(site_settings: Value) -> Result<PluginManifest, String> {
    PluginManifest::parse(&json!({"name":"shop","version":"1.0.0","contributes":{"site_settings":site_settings}}).to_string())
        .map_err(|e| e.to_string())
}

fn settings(fields: Value) -> Result<PluginManifest, String> {
    parse(json!({"settings": fields}))
}

#[test]
fn a_nested_offer_parses_and_needs_no_entry() {
    let m = settings(json!([
        {"key":"currency","type":"string","label":"Currency"},
        {"key":"items","type":"map","label":"Products","add_label":"Add product","key_pattern":"^[a-z-]+$",
         "items":{"type":"object","fields":[
            {"key":"name","type":"string"},
            {"key":"sold_as","type":"string","options":[{"value":"unit","label":"Each"},{"value":"weight","label":"By weight"}]},
            {"key":"price","type":"number","scale":100,"when":{"sold_as":"unit"}},
            {"key":"per_kilo","type":"number","scale":100,"unit":"per kg","when":{"sold_as":"weight"}},
            {"key":"discount","type":"object","optional":true,"fields":[{"key":"percent","type":"number","default":10}]}]}},
        {"key":"open","type":"object","fields":[
            {"key":"mon","type":"array","items":{"type":"string","pattern":"^\\d\\d:\\d\\d$","pattern_message":"HH:MM"}}]}
    ]))
    .unwrap();
    let site = m.contributes.unwrap().site_settings.unwrap();
    assert_eq!(site.settings.len(), 3);
}

#[test]
fn each_rule_names_what_is_wrong() {
    let cases = [
        (json!([{"key":"token","type":"secret"}]), "syncs with the folder"),
        (json!([{"key":"a","type":"string"},{"key":"a","type":"number"}]), "declared twice"),
        (json!([{"key":"Bad Key","type":"string"}]), "lowercase letters"),
        (json!([{"key":"o","type":"object"}]), "lists its `fields`"),
        (json!([{"key":"m","type":"map"}]), "`items`"),
        (json!([{"key":"l","type":"array","items":{"type":"object","fields":[{"key":"x","type":"string"}]}}]), "of an `array`, a scalar"),
        (json!([{"key":"n","type":"string","scale":100}]), "belong to a `number`"),
        (json!([{"key":"n","type":"number","scale":30}]), "power of ten"),
        (json!([{"key":"s","type":"string","optional":true}]), "belongs to an `object`"),
        (json!([{"key":"s","type":"string","key_pattern":"^a$"}]), "belong to a `map`"),
        (json!([{"key":"s","type":"string","pattern":"("}]), "without a `pattern_message`"),
        (json!([{"key":"e","type":"string","hidden":true}]), "leave an undrawn key out"),
        (json!([{"key":"s","type":"string","pattern":"(","pattern_message":"x"}]), "not a regular expression"),
        (json!([{"key":"p","type":"number","when":{"kind":"a"}},{"key":"kind","type":"string","options":[{"value":"a","label":"A"}]}]), "declared earlier"),
        (json!([{"key":"kind","type":"string","options":[{"value":"a","label":"A"}]},{"key":"p","type":"number","when":{"kind":"b"}}]), "not one of"),
    ];
    for (fields, says) in cases {
        let err = settings(fields.clone()).expect_err(&fields.to_string());
        assert!(err.contains(says), "{fields}: {err}");
    }
}

#[test]
fn the_rules_reach_into_map_entries() {
    let err = settings(json!([{"key":"m","type":"map","items":{"type":"object","fields":[{"key":"t","type":"secret"}]}}])).unwrap_err();
    assert!(err.contains("syncs with the folder"), "{err}");
}

#[test]
fn the_flat_settings_list_refuses_the_nested_vocabulary() {
    let m = PluginManifest::parse(
        &json!({"name":"flat","version":"1.0.0","entry":"main.js","contributes":{"channel":{"setup":{"settings":[
            {"key":"o","type":"object","fields":[{"key":"x","type":"string"}]}]}}}})
        .to_string(),
    );
    assert!(m.unwrap_err().to_string().contains("only in `site_settings`"));
}

#[test]
fn an_action_names_a_route_on_a_known_service_and_nothing_else() {
    let ok = json!({"id":"connect","label":"Connect","service":"commerce","method":"POST","path":"/sites/{site}/stripe/onboarding"});
    let m = parse(json!({"actions":[ok]})).unwrap();
    let action = m.contributes.unwrap().site_settings.unwrap().action("connect").cloned().unwrap();
    use super::service_path;
    assert_eq!(service_path(&action.path, Some("her-blog")).as_deref(), Some("/sites/her-blog/stripe/onboarding"));
    assert_eq!(service_path(&action.path, Some("../x")), None);
    assert_eq!(service_path(&action.path, Some("")), None);
    assert_eq!(service_path(&action.path, None), None);
    for bad_path in ["https://evil.example/x", "//evil.example/x", "/sites/{site}/../admin", "/a?b=c", "/sites/{other}"] {
        let bad = json!({"id":"connect","label":"C","service":"commerce","method":"POST","path":bad_path});
        assert!(parse(json!({"actions":[bad]})).is_err(), "{bad_path}");
    }
    let put = json!({"id":"connect","label":"C","service":"commerce","method":"PUT","path":"/a"});
    assert!(parse(json!({"actions":[put]})).is_err());
}

#[test]
fn only_a_path_the_manifest_draws_may_be_written() {
    let m = settings(json!([
        {"key":"currency","type":"string"},
        {"key":"items","type":"map","key_pattern":"^[a-z]+$","items":{"type":"object","fields":[
            {"key":"name","type":"string"},
            {"key":"hours","type":"object","fields":[{"key":"min","type":"number"}]}]}},
        {"key":"tags","type":"map","items":{"type":"string"}},
        {"key":"open","type":"object","fields":[{"key":"mon","type":"array","items":{"type":"string"}}]}
    ]))
    .unwrap();
    let site = m.contributes.unwrap().site_settings.unwrap();
    let p = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    for ok in [&["currency"][..], &["items", "walk"], &["items", "walk", "name"], &["items", "walk", "hours", "min"], &["tags", "early-bird"], &["open", "mon"], &["open"]] {
        assert!(site.check_change(&p(ok), None).is_ok(), "{ok:?}");
    }
    for bad in [&[][..], &["endpoint"], &["open", "funday"], &["items", "Walk"], &["items", "walk", "secret"], &["tags", "x", "deeper"], &["currency", "code"], &["open", "mon", "0"]] {
        assert!(site.check_change(&p(bad), None).is_err(), "{bad:?}");
    }
    assert_eq!(site.settings[2].key_pattern.as_deref(), Some(super::super::settings::DEFAULT_KEY_PATTERN), "a map gets the default id rule");
}

#[test]
fn a_written_value_carries_only_declared_keys_of_the_declared_kinds() {
    let m = settings(json!([
        {"key":"currency","type":"string"},
        {"key":"cancel","type":"object","fields":[{"key":"late_percent","type":"number"}]},
        {"key":"services","type":"map","items":{"type":"object","fields":[
            {"key":"name","type":"string"},
            {"key":"labels","type":"map","items":{"type":"string"}}]}},
        {"key":"open","type":"object","fields":[{"key":"mon","type":"array","items":{"type":"string"}}]}
    ]))
    .unwrap();
    let site = m.contributes.unwrap().site_settings.unwrap();
    let p = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let ok = |path: &[&str], value: serde_json::Value| site.check_change(&p(path), Some(&value));
    assert!(ok(&["cancel"], json!({"late_percent":50})).is_ok());
    assert!(ok(&["services", "walk"], json!({"name":"Walk","labels":{"phone":"Phone"}})).is_ok());
    assert!(ok(&["open"], json!({"mon":["09:00-12:00"]})).is_ok());
    for (path, value) in [
        (&["cancel"][..], json!({"late_percent":50,"calendar":"https://cal.example/private.ics"})),
        (&["services", "walk"], json!({"name":"Walk","calendar":"https://cal.example/private.ics"})),
        (&["services"], json!({"walk":{"name":"Walk","notes":{"calendar":"x"}}})),
        (&["services"], json!({"Walk!":{"name":"Walk"}})),
        (&["services", "walk", "labels"], json!({"phone":{"calendar":"x"}})),
        (&["currency"], json!({"calendar":"https://cal.example/private.ics"})),
        (&["currency"], json!(12)),
        (&["cancel", "late_percent"], json!("half")),
        (&["open", "mon"], json!([{"calendar":"x"}])),
        (&["open"], json!("09:00")),
    ] {
        assert!(ok(path, value.clone()).is_err(), "{path:?} = {value}");
    }
}
