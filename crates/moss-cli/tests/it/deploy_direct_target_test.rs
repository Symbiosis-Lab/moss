//! `moss deploy` to your own server before moss may publish there: refused
//! with the next step named, before anything is built.

use moss_build::moss_paths::MossPaths;
use std::path::Path;
use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn deploy(config: &str, extra: &[&str], says: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let root = tmp.path().join("site");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome.\n");
    write(&root, ".moss/config.toml", config);

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
    assert!(stderr.contains(says), "{stderr}");
    assert!(!stderr.contains("plugin"), "a built-in host is not a missing plugin: {stderr}");
    assert!(!MossPaths::new(&root).current_ptr().exists(), "refused before the build");
}

const NO_SERVER: &str = "[hooks]\ndeploy = \"server\"\n";
const UNTRUSTED: &str = "[hooks]\ndeploy = \"server\"\n\n[deploy.server]\nto = \"sftp://deploy@example.com/var/www/html\"\n";

#[test]
fn a_deploy_before_the_server_is_set_is_refused_before_the_build() {
    deploy(NO_SERVER, &[], "Set the server first");
}

#[test]
fn a_deploy_before_the_key_is_trusted_is_refused_before_the_build() {
    deploy(UNTRUSTED, &[], "to trust its key");
}

#[test]
fn a_dry_run_to_your_own_server_is_refused_before_the_build() {
    deploy(NO_SERVER, &["--dry-run"], "--dry-run doesn't cover your own server");
}
