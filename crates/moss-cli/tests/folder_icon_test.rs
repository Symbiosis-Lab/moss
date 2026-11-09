//! `moss build` gives the site's root folder moss's folder icon on macOS, so a
//! folder made and built only from the terminal is marked like one the app
//! opened. All names and text are invented.
#![cfg(target_os = "macos")]

use std::process::Command;

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

#[test]
fn a_build_marks_the_site_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let site = tmp.path().join("garden");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Garden Notes\n---\n\nWelcome.\n").unwrap();

    let o = Command::new(MOSS)
        .args(["build", "--no-plugins"])
        .arg(&site)
        .env("MOSS_HOME", tmp.path().join("home"))
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli");
    assert!(o.status.success(), "build failed: {}", String::from_utf8_lossy(&o.stderr));

    let fork = site.join("Icon\r/..namedfork/rsrc");
    let len = std::fs::metadata(&fork).map(|m| m.len()).unwrap_or(0);
    assert!(len > 0, "the built site's folder carries no icon");
}
