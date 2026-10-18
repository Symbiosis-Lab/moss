//! `approve` and `change_since_approval` against the process-wide consent
//! store. Its own test binary: the store is global, and turning consent on
//! would change what every other test in a shared binary is allowed to load.

use std::path::Path;

use moss_build::plugins::contributions::abilities::Ability;
use moss_build::plugins::install::registry_client::{approval, enforce};
use moss_build::plugins::types::PluginManifest;

fn write_plugin(dir: &Path, manifest: &str, entry: &str) -> PluginManifest {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    std::fs::write(dir.join("index.js"), entry).unwrap();
    PluginManifest::parse(manifest).unwrap()
}

#[test]
fn approving_records_what_was_allowed_and_a_later_change_names_what_is_new() {
    let tmp = tempfile::tempdir().unwrap();
    let app_data = tmp.path().join("app-data");
    enforce::install(Some(&app_data));

    let allowed = tmp.path().join("project/.moss/plugins/demo");
    write_plugin(
        &allowed,
        r#"{"name":"demo","version":"1.0.0","entry":"index.js","contributes":{"listing_styles":{"board":{"label":"Board"}}}}"#,
        "var A = 1;",
    );
    let other = tmp.path().join("project/.moss/plugins/never-asked");
    write_plugin(&other, r#"{"name":"never-asked","version":"1.0.0","entry":"index.js"}"#, "var B;");

    enforce::approve(&allowed).unwrap();

    let stored = approval::load(&app_data);
    let record = stored.record_for(&allowed.canonicalize().unwrap()).expect("the approval was saved");
    assert_eq!(record.version.as_deref(), Some("1.0.0"));
    assert_eq!(
        record.abilities,
        Some(vec![Ability::DrawsPages { style: "board".into(), label: "Board".into(), description: None }])
    );

    assert_eq!(enforce::change_since_approval(&allowed), None, "unchanged code");
    assert_eq!(enforce::change_since_approval(&other), None, "never approved");

    write_plugin(
        &allowed,
        r#"{"name":"demo","version":"1.1.0","entry":"index.js","capabilities":["deploy"],"contributes":{"listing_styles":{"board":{"label":"Board"}}}}"#,
        "var A = 2;",
    );
    let change = enforce::change_since_approval(&allowed).expect("the code moved");
    assert_eq!(change.approved_version.as_deref(), Some("1.0.0"));
    assert_eq!(change.version, "1.1.0");
    assert_eq!(change.new_abilities, Some(vec![Ability::Publishes { hook: "deploy".into() }]));
}
