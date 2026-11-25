//! A renamed folder keeps its old address answering: a publish, the folder
//! renamed, then a dry run and a second publish through a local deploy
//! plugin. Covers a folder with pages and an `index.md`, one with pages and
//! no `index.md`, and one whose only page is its `index.md`.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

const PLUGIN: &str = r#"
window.LocaldirPlugin = { deploy: async function (ctx) {
  return { success: true, message: "ok",
           deployment: { method: "local", url: "http://127.0.0.1:9", deployed_at: new Date().toISOString(), metadata: {} } };
} };
"#;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn deploy(root: &Path, home: &Path, extra: &[&str]) -> String {
    let out = Command::new(MOSS)
        .args(["deploy", root.to_str().unwrap(), "--allow-plugins"])
        .args(extra)
        .env("MOSS_HOME", home)
        .env("HOME", home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "deploy {extra:?} failed: {text}");
    text
}

fn renamed_folder_forwards(pages: &[(&str, &str)]) {
    let tmp = tempfile::tempdir().unwrap();
    let (home, root) = (tmp.path().join("home"), tmp.path().join("site"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome.\n");
    for (rel, title) in pages {
        write(&root, &format!("river/{rel}"), &format!("---\ntitle: {title}\n---\nText.\n"));
    }
    write(&root, ".moss/config.toml", "[hooks]\ndeploy = \"localdir\"\n");
    write(
        &root,
        ".moss/plugins/localdir/manifest.json",
        r#"{"name":"localdir","version":"0.0.1","display_name":"Local","description":"","author":"","entry":"main.bundle.js","global_name":"LocaldirPlugin","requires":[],"contributes":{"deploy_target":{}}}"#,
    );
    write(&root, ".moss/plugins/localdir/main.bundle.js", PLUGIN);
    deploy(&root, &home, &[]);
    std::fs::rename(root.join("river"), root.join("stream")).unwrap();

    let dry = deploy(&root, &home, &["--dry-run"]);
    assert!(!dry.contains("/river/"), "the old folder address is not reported going offline: {dry}");
    let built = dry.lines().find_map(|l| l.split_once("Site generated at ").map(|(_, p)| p.trim().to_string())).expect(&dry);
    let stub = std::fs::read_to_string(Path::new(&built).join("river/index.html")).unwrap();
    assert!(stub.contains("url=/stream/"), "river/ forwards to stream/: {stub}");

    deploy(&root, &home, &[]);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".moss/redirects.json")).unwrap()).unwrap();
    assert_eq!(saved["river/"], "stream/", "{saved}");
}

#[test]
fn a_renamed_folder_with_an_index_and_pages_forwards_its_address() {
    renamed_folder_forwards(&[("index.md", "River"), ("flood.md", "Flood")]);
}

#[test]
fn a_renamed_folder_without_an_index_forwards_its_address() {
    renamed_folder_forwards(&[("flood.md", "Flood"), ("rain.md", "Rain")]);
}

#[test]
fn a_renamed_folder_whose_only_page_is_its_index_forwards_its_address() {
    renamed_folder_forwards(&[("index.md", "River")]);
}
