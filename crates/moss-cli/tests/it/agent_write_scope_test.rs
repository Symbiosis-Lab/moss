//! Agent write scope through the shipped command boundary, including repeat init.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn site() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join(".moss")).unwrap();
    fs::create_dir(tmp.path().join("home")).unwrap();
    fs::write(tmp.path().join(".moss/config.toml"), "[site]\nlang = \"en\"\n").unwrap();
    fs::write(tmp.path().join("index.md"), "# Example\n\nHello.\n").unwrap();
    tmp
}

fn run(root: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .current_dir(root)
        .env("MOSS_HOME", root.join("home"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {}\n{}",
        String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    out
}

fn assert_pointer(root: &Path) {
    let pointer = fs::read_to_string(root.join("AGENTS.md")).expect("root pointer exists");
    for hint in ["moss guide", "moss describe --json", ".moss/AGENTS.md"] {
        assert!(pointer.contains(hint), "missing {hint}: {pointer}");
    }
}

#[test]
fn init_with_current_conventions_creates_root_pointer_without_backup_or_rewrite() {
    let tmp = site();
    let root = tmp.path();
    let conventions = root.join(".moss/AGENTS.md");
    fs::write(&conventions, include_bytes!("../../../moss-build/src/assets/agents/template.md")).unwrap();
    let before = fs::read(&conventions).unwrap();
    let modified = fs::metadata(&conventions).unwrap().modified().unwrap();

    let out = run(root, &["agents", "init"]);

    assert_pointer(root);
    assert!(String::from_utf8_lossy(&out.stdout).contains("AGENTS.md is up to date"));
    assert_eq!(fs::read(&conventions).unwrap(), before);
    assert_eq!(fs::metadata(&conventions).unwrap().modified().unwrap(), modified);
    assert!(!root.join(".moss/AGENTS.md.bak").exists());
}

#[test]
fn explicit_init_restores_a_deleted_root_pointer() {
    let tmp = site();
    run(tmp.path(), &["agents", "init"]);
    let before = fs::read(tmp.path().join("AGENTS.md")).unwrap();
    fs::remove_file(tmp.path().join("AGENTS.md")).unwrap();

    run(tmp.path(), &["agents", "init"]);

    assert_eq!(fs::read(tmp.path().join("AGENTS.md")).unwrap(), before);
}

#[test]
fn init_preserves_existing_root_bytes_and_prints_the_hint_even_when_current() {
    let tmp = site();
    let existing = b"# My conventions\r\nKeep these exact bytes.\r\n\xff";
    fs::write(tmp.path().join("AGENTS.md"), existing).unwrap();
    for _ in 0..2 {
        let out = run(tmp.path(), &["agents", "init"]);
        assert_eq!(fs::read(tmp.path().join("AGENTS.md")).unwrap(), existing);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("left AGENTS.md alone: it is yours"), "{stdout}");
        assert!(stdout.contains("Run `moss guide` before editing it."), "{stdout}");
    }
}

fn assert_never_creates_pointer(args: &[&str]) {
    let tmp = site();
    run(tmp.path(), args);
    assert!(!tmp.path().join("AGENTS.md").exists());
    run(tmp.path(), &["agents", "init"]);
    fs::remove_file(tmp.path().join("AGENTS.md")).unwrap();
    run(tmp.path(), args);
    assert!(!tmp.path().join("AGENTS.md").exists());
}

#[test]
fn build_never_creates_or_restores_root_pointer() {
    assert_never_creates_pointer(&["build", "--no-plugins"]);
}

#[test]
fn sync_never_creates_or_restores_root_pointer() {
    assert_never_creates_pointer(&["agents", "sync"]);
}

fn assert_author_owned(path: &str) {
    let tmp = site();
    let out = run(tmp.path(), &["describe", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let paths = &json["paths"];
    assert_eq!(paths["default_inside_moss"], "moss");
    assert_eq!(paths["default_outside_moss"], "author");
    let rules = paths["rules"].as_array().unwrap();
    let matching: Vec<_> = rules.iter().filter(|rule| rule["path"] == path).collect();
    assert_eq!(matching.len(), 1, "expected one rule for {path}: {paths}");
    assert_eq!(*matching[0], serde_json::json!({
        "path": path, "writer": "author", "changed_by": null,
    }));
}

#[test]
fn describe_assets_are_author_owned() {
    assert_author_owned(".moss/assets/");
}

#[test]
fn describe_templates_are_author_owned() {
    assert_author_owned(".moss/templates/");
}
