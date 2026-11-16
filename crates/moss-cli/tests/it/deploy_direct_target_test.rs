//! `moss deploy` on a folder whose `[hooks] deploy` names a host built into
//! moss that this version cannot publish to yet: refused at once, with the
//! key named, before anything is built.

use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn deploy(extra: &[&str]) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("site");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome.\n");
    write(&root, ".moss/config.toml", "[hooks]\ndeploy = \"server\"\n");

    let out = Command::new(MOSS)
        .arg("deploy")
        .arg(&root)
        .args(extra)
        .env("MOSS_HOME", &home)
        .env("HOME", &home)
        .env_remove("MOSS_ENV")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("deploy = \"server\""), "{stderr}");
    assert!(!stderr.contains("plugin"), "a built-in host is not a missing plugin: {stderr}");
    assert!(!root.join(".moss/build.nosync/current").exists(), "refused before the build");
}

#[test]
fn a_deploy_to_your_own_server_is_refused_before_the_build() {
    deploy(&[]);
}

#[test]
fn a_dry_run_to_your_own_server_is_refused_before_the_build() {
    deploy(&["--dry-run"]);
}
