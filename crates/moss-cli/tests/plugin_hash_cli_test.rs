use std::path::Path;
use std::process::{Command, Output};

fn moss(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moss-cli")).arg("plugin").args(args).output().unwrap()
}

fn invented_plugin(root: &Path) -> std::path::PathBuf {
    let dir = root.join("lantern");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), r#"{"name":"lantern","version":"1.2.0","entry":"main.js"}"#).unwrap();
    std::fs::write(dir.join("main.js"), "export function on_build() {}").unwrap();
    dir
}

#[test]
fn prints_the_hash_the_loader_keys_consent_on() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = invented_plugin(tmp.path());
    let manifest = moss_build::plugins::bundled::read_installed_manifest(&dir).unwrap();
    let expected = moss_build::plugins::bundled::code_hash(&dir, &manifest).unwrap();

    let plain = moss(&["hash", dir.to_str().unwrap()]);
    assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
    assert_eq!(String::from_utf8(plain.stdout).unwrap(), format!("{expected}\n"));

    let json = moss(&["hash", dir.to_str().unwrap(), "--json"]);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value, serde_json::json!({"id": "lantern", "version": "1.2.0", "code_sha256": expected}));
}

#[test]
fn fails_on_a_directory_that_is_not_a_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = moss(&["hash", tmp.path().to_str().unwrap()]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(empty.stdout.is_empty());

    let dir = invented_plugin(tmp.path());
    std::fs::remove_file(dir.join("main.js")).unwrap();
    let no_entry = moss(&["hash", dir.to_str().unwrap(), "--json"]);
    assert_eq!(no_entry.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_entry.stderr).contains("entry"));
}

#[test]
fn install_without_an_id_or_outside_a_site_fails_before_any_network() {
    let none = moss(&["install"]);
    assert_eq!(none.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&none.stderr).contains("moss plugin install <id> [<folder>]"));

    let tmp = tempfile::tempdir().unwrap();
    let outside = moss(&["install", "lantern", tmp.path().to_str().unwrap()]);
    assert_eq!(outside.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&outside.stderr).contains("not inside a moss site"));
}
