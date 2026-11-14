//! A build whose site could not be written is a failed build: `moss build`
//! says so and exits non-zero. A script that reads the exit code would
//! otherwise go on to use the site the build before it left. Run through the
//! real CLI.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn build(site: &Path, moss_home: &Path) -> (Output, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins"])
        .env("MOSS_HOME", moss_home)
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

fn set_mode(dir: &Path, mode: u32) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_build_that_cannot_write_the_site_fails() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nFirst words.\n").unwrap();

    let (first, stderr) = build(&site, moss_home.path());
    assert!(first.status.success(), "the first build must succeed:\n{stderr}");
    let served = site.join(".moss/build.nosync/current/index.html");
    assert!(std::fs::read_to_string(&served).unwrap().contains("First words."));

    // The next build renders, then has nowhere to put its site.
    let generations = site.join(".moss/build.nosync/generations");
    set_mode(&generations, 0o555);
    if std::fs::write(generations.join("probe"), "").is_ok() {
        // Root writes through a read-only directory, so nothing would fail.
        set_mode(&generations, 0o755);
        return;
    }
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nSecond words.\n").unwrap();
    let (second, stderr) = build(&site, moss_home.path());
    set_mode(&generations, 0o755);

    let page = std::fs::read_to_string(&served).unwrap();
    assert!(
        page.contains("First words.") && !page.contains("Second words."),
        "premise: the second build must leave the first build's site in place:\n{stderr}"
    );
    assert_eq!(second.status.code(), Some(1), "a build that wrote no site must fail:\n{stderr}");
    assert!(stderr.contains("Build failed: the site was not written."), "{stderr}");
    assert!(!stderr.contains("0 problems"), "a failed build must not report a clean one:\n{stderr}");
}
