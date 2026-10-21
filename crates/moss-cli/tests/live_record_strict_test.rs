//! A site that has published reads what is live from its publish record. A
//! missing record is a note, not a strict failure: `--strict` is for the
//! content. A record the publish wrote (even for a site with no tracked pages)
//! is read, so the note goes away after one publish.

use std::process::{Command, Stdio};

const MISSING: &str = "no usable record of what is live";

const STATE: &str = r#"schema_version = 1

[deployment]
site_id = "example-site"

[deployment.targets."moss:example-site"]
last_deployed_generation_id = "c1a362455823cb3f"
last_deployment_at = "2026-10-21T13:12:34+00:00"
last_deployment_url = "https://example-site.example.test"
"#;

/// What a publish of a lone home page leaves in `.moss/deploy/records/`.
const RECORD: &str = r#"{
  "generation_id": "c1a362455823cb3f",
  "target": "moss:example-site",
  "published_at": "2026-10-21T13:12:34.674820+00:00",
  "sources": {},
  "source_to_output": {},
  "files": {},
  "uids": {},
  "triples": []
}"#;

fn published_site(with_record: bool) -> (std::process::Output, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    std::fs::write(site.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    std::fs::create_dir_all(site.path().join(".moss/deploy/records")).unwrap();
    std::fs::write(site.path().join(".moss/state.toml"), STATE).unwrap();
    if with_record {
        std::fs::write(site.path().join(".moss/deploy/records/moss-0000000000000000.json"), RECORD).unwrap();
    }
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
fn a_published_site_with_no_record_passes_strict_and_says_so_as_a_note() {
    let (out, stderr) = published_site(false);
    assert!(out.status.success(), "--strict must not fail on a missing record; stderr:\n{stderr}");
    assert!(stderr.contains(MISSING), "the note is still shown; stderr:\n{stderr}");
    assert!(stderr.contains("[NOTE]"), "as a note; stderr:\n{stderr}");
}

#[test]
fn a_record_the_publish_wrote_is_read_and_nothing_is_reported() {
    let (out, stderr) = published_site(true);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains(MISSING), "stderr:\n{stderr}");
}
