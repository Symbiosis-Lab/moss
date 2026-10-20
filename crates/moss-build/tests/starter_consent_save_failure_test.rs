//! A consent record that cannot be saved allows nothing, in this process too.

use moss_build::plugins::bundled::{code_hash, read_installed_manifest};
use moss_build::plugins::install::registry_client::enforce::{approve_starter_plugins, install, vouched_for};
use moss_build::plugins::install::registry_client::index::StarterPlugin;

#[test]
fn a_failed_save_leaves_the_plugin_unallowed() {
    let scratch = tempfile::tempdir().unwrap();
    let app_data = scratch.path().join("app-data");
    std::fs::write(&app_data, "a file where the directory should be").unwrap();
    install(Some(&app_data));

    let dir = scratch.path().join("site/.moss/plugins/lantern");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), r#"{"name":"lantern","version":"1.0.0","entry":"index.js"}"#).unwrap();
    std::fs::write(dir.join("index.js"), "var A = 1;").unwrap();
    let manifest = read_installed_manifest(&dir).unwrap();
    let listed = [StarterPlugin { id: "lantern".into(), version: "1.0.0".into(), code_sha256: code_hash(&dir, &manifest).unwrap() }];

    assert!(approve_starter_plugins(&scratch.path().join("site"), &listed).is_empty());
    assert!(!vouched_for(&manifest, &dir), "the grant must not outlive its failed save");
}
