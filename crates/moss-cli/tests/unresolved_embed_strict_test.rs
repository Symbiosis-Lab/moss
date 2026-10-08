//! A folder embed that resolves to nothing leaves a visible placeholder and is
//! also a build problem: it names the page and the target on stderr, and
//! `--strict` exits 1.

use std::process::{Command, Stdio};

fn strict_build(index_md: &str) -> (std::process::Output, String) {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site = tempfile::tempdir().expect("tempdir for the site");
    std::fs::write(site.path().join("index.md"), index_md).unwrap();
    let site_dir = std::fs::canonicalize(site.path()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site_dir)
        .arg("--no-plugins")
        .arg("--strict")
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

#[test]
fn unresolved_folder_embed_fails_strict_and_names_page_and_target() {
    let (out, stderr) = strict_build("---\ntitle: Home\n---\n\n![[nowhere/]]\n");
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
    assert!(stderr.contains("index.md") && stderr.contains("nowhere/"), "stderr:\n{stderr}");
}

#[test]
fn invalid_folder_embed_marker_fails_strict() {
    // A hand-written marker with no `path=` field cannot be parsed.
    let (out, stderr) = strict_build("---\ntitle: Home\n---\n\n<!--MOSS_MARKER_FOLDER_LIST:garbage-->\n");
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
    assert!(stderr.contains("Invalid folder-embed marker"), "stderr:\n{stderr}");
}
