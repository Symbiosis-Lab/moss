//! A `[site]` key the build does not read gets a note, not a problem: it names
//! the key and the file, suggests the closest known key when one is near, and
//! leaves `--strict` green, since older moss versions left keys no code reads.
//! A key the legacy migration writes must not be noted at all.

use std::process::{Command, Stdio};

fn strict_build(config: &str) -> (std::process::Output, String) {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site = tempfile::tempdir().expect("tempdir for the site");
    std::fs::write(site.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    std::fs::create_dir(site.path().join(".moss")).unwrap();
    std::fs::write(site.path().join(".moss/config.toml"), config).unwrap();
    let site_dir = std::fs::canonicalize(site.path()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site_dir)
        .args(["--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

#[test]
fn title_under_site_is_noted_and_points_at_the_home_page() {
    let (out, stderr) = strict_build("[site]\ntitle = \"x\"\n");
    assert!(out.status.success(), "a note must not fail --strict; stderr:\n{stderr}");
    assert!(stderr.contains("[site] title is not a setting"), "stderr:\n{stderr}");
    assert!(stderr.contains(".moss/config.toml"), "stderr:\n{stderr}");
}

#[test]
fn unknown_site_key_names_the_closest_known_key() {
    let (out, stderr) = strict_build("[site]\ncolophn = false\n");
    assert!(out.status.success(), "a note must not fail --strict; stderr:\n{stderr}");
    assert!(
        stderr.contains("unknown key \"colophn\"") && stderr.contains("did you mean \"colophon\"?"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn unknown_site_key_with_no_near_match_says_the_build_does_not_read_it() {
    let (out, stderr) = strict_build("[site]\narchivl = true\n");
    assert!(out.status.success(), "a note must not fail --strict; stderr:\n{stderr}");
    assert!(stderr.contains("unknown key \"archivl\""), "stderr:\n{stderr}");
    assert!(!stderr.contains("did you mean"), "no near key exists, so none is suggested:\n{stderr}");
}

#[test]
fn a_site_with_only_known_site_keys_is_quiet() {
    let (out, stderr) = strict_build("[site]\nlang = \"en\"\ncolophon = false\n");
    assert!(out.status.success(), "a clean site must pass --strict; stderr:\n{stderr}");
    assert!(!stderr.contains("[site]"), "stderr:\n{stderr}");
}

#[test]
fn a_legacy_rss_setting_migrates_without_a_site_note() {
    let (out, stderr) = strict_build("schema_version = 1\n[features]\nrss = true\n");
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("unknown key"), "the migration wrote a key the build does not list:\n{stderr}");
}
