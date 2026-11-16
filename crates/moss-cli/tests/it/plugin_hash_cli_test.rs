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
    assert!(String::from_utf8_lossy(&outside.stderr).contains("not a moss site root"));
}

#[test]
fn install_rejects_an_explicit_nested_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let site = tmp.path().join("site");
    let nested = site.join("posts");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["plugin", "install", "lantern", nested.to_str().unwrap()])
        .env("HOME", tmp.path())
        .env("MOSS_HOME", tmp.path().join("tooling"))
        .output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("not a moss site root") && err.contains(nested.to_str().unwrap()), "{err}");
    assert!(!site.join(".moss/plugins").exists());
}

#[test]
fn install_does_not_accept_relocated_tooling() {
    let tmp = tempfile::tempdir().unwrap();
    let tooling = tmp.path().join("tooling/.moss");
    let nested = tmp.path().join("tooling/notes");
    std::fs::create_dir_all(&tooling).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    for explicit in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
        command.args(["plugin", "install", "lantern"])
            .env("HOME", tmp.path())
            .env("MOSS_HOME", &tooling)
            .current_dir(&nested);
        if explicit {
            command.arg(tooling.parent().unwrap());
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let err = String::from_utf8_lossy(&output.stderr);
        let expected = if explicit { "not a moss site root" } else { "not inside a moss site" };
        assert!(err.contains(expected) && err.contains("tooling"), "{err}");
        assert!(!tooling.join("plugins").exists());
    }
}
