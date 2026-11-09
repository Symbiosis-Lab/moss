//! The list of files `moss deploy` hands a deploy plugin is the published set,
//! drafts left out, written with `/` on every system. The separator check can
//! only fail on Windows.

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

const PLUGIN: &str = r#"
window.ListPlugin = { deploy: async function (ctx) {
  return { success: true, message: "LISTED " + JSON.stringify(ctx.site_files),
           deployment: { method: "local", url: "http://127.0.0.1:9", deployed_at: new Date().toISOString(), metadata: {} } };
} };
"#;

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn a_deploy_plugin_is_handed_forward_slash_paths_without_a_nested_draft() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("site");
    // A Windows profile has these; without them moss finds no app data folder.
    for dir in ["AppData/Roaming", "AppData/Local"] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
    }
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome.\n");
    write(&root, "notes/visible.md", "---\ntitle: Visible\n---\nShown.\n");
    write(&root, "notes/plans.md", "---\ntitle: Plans\ndraft: true\n---\nNot yet.\n");
    write(&root, ".moss/config.toml", "[hooks]\ndeploy = \"list\"\n");
    write(
        &root,
        ".moss/plugins/list/manifest.json",
        r#"{"name":"list","version":"0.0.1","display_name":"List","description":"","author":"","entry":"main.bundle.js","global_name":"ListPlugin","contributes":{"deploy_target":{}}}"#,
    );
    write(&root, ".moss/plugins/list/main.bundle.js", PLUGIN);

    let out = Command::new(MOSS)
        .args(["deploy", root.to_str().unwrap(), "--allow-plugins"])
        .env("MOSS_HOME", &home)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "deploy failed: {stderr}");
    let line = stderr.lines().find_map(|l| l.split_once("LISTED ").map(|(_, r)| r)).unwrap_or_else(|| panic!("{stderr}"));
    let listed: Vec<String> = serde_json::from_str(line).unwrap();

    assert!(listed.iter().all(|f| !f.contains('\\')), "{listed:?}");
    assert!(listed.iter().any(|f| f.starts_with("notes/visible")), "{listed:?}");
    assert!(!listed.iter().any(|f| f.contains("plans")), "the draft is withheld: {listed:?}");
}
