//! A starter's listed plugins are allowed at pick time only when the copied
//! directory is the code the registry listed.

use moss_build::plugins::bundled::{code_hash, read_installed_manifest};
use moss_build::plugins::install::registry_client::enforce::{approve_starter_plugins, install, vouched_for};
use moss_build::plugins::install::registry_client::index::StarterPlugin;
use std::path::{Path, PathBuf};

fn carried(site: &Path, id: &str, entry: &str) -> (PathBuf, String) {
    let dir = site.join(".moss/plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), format!(r#"{{"name":"{id}","version":"1.0.0","entry":"index.js"}}"#)).unwrap();
    std::fs::write(dir.join("index.js"), entry).unwrap();
    let hash = code_hash(&dir, &read_installed_manifest(&dir).unwrap()).unwrap();
    (dir, hash)
}

fn listed(id: &str, code_sha256: &str) -> StarterPlugin {
    StarterPlugin { id: id.into(), version: "1.0.0".into(), code_sha256: code_sha256.into() }
}

#[test]
fn a_matching_directory_is_allowed_and_a_tampered_one_is_not() {
    let app_data = tempfile::tempdir().unwrap();
    install(Some(app_data.path()));
    let site = tempfile::tempdir().unwrap();
    let (lantern, lantern_hash) = carried(site.path(), "lantern", "var A = 1;");
    let (kiln, kiln_hash) = carried(site.path(), "kiln", "var B = 1;");
    let (_, absent_hash) = carried(tempfile::tempdir().unwrap().path(), "loom", "var C = 1;");
    std::fs::write(kiln.join("index.js"), "var B = 2;").unwrap();

    let plugins = [listed("lantern", &lantern_hash), listed("kiln", &kiln_hash), listed("loom", &absent_hash), listed("../escape", &lantern_hash)];
    assert_eq!(approve_starter_plugins(site.path(), &plugins), vec!["lantern".to_string()]);

    let vouched = |dir: &Path| vouched_for(&read_installed_manifest(dir).unwrap(), dir);
    assert!(vouched(&lantern), "the matching copy is allowed");
    assert!(!vouched(&kiln), "the edited copy waits for the usual prompt");
}
