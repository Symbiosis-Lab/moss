//! An agent or CI job reads the built site from where `moss build` says it is,
//! never from a path it spells itself: `--json` ends with one object on stdout
//! naming the generation the build wrote.

use std::process::{Command, Stdio};

fn build(site: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let moss_home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins"])
        .args(extra)
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli")
}

#[test]
fn a_json_build_prints_one_object_naming_its_output() {
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    let out = build(&site, &["--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("one JSON object, not {stdout:?}: {e}"));
    let output = std::path::Path::new(report["output"].as_str().expect("an output path"));
    assert!(output.is_absolute(), "{output:?}");
    assert!(std::fs::read_to_string(output.join("index.html")).unwrap().contains("Welcome."));
}

#[test]
fn a_plain_build_names_its_output_on_stderr_and_leaves_stdout_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    let out = build(&site, &[]);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", String::from_utf8_lossy(&out.stdout));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let line = stderr.lines().find_map(|l| l.strip_prefix("Output: ")).unwrap_or_else(|| panic!("{stderr}"));
    assert!(std::path::Path::new(line).join("index.html").is_file(), "{line}");
}
