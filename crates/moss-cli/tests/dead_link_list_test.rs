//! A build that finds dead root-relative links names them on stderr, and doing
//! so neither fails `--strict` nor adds to the problem count: the audit is
//! advisory by design, since a proxy or redirect rule can legitimately serve a
//! path moss never wrote.

use std::process::{Command, Stdio};

#[test]
fn dead_links_are_listed_on_stderr_without_failing_strict() {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site = tempfile::tempdir().expect("tempdir for the site");
    let site = site.path();
    std::fs::write(
        site.join("index.md"),
        "---\ntitle: Home\n---\n\n[one](/gone-one/) and [two](/gone-two/)\n",
    )
    .unwrap();
    let site_dir = std::fs::canonicalize(site).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site_dir)
        .arg("--no-plugins")
        .arg("--strict")
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        out.status.success(),
        "dead links are advisory and must not fail --strict; stderr:\n{stderr}"
    );
    assert!(stderr.contains("  '/gone-one/' in 'index.html'"), "stderr:\n{stderr}");
    assert!(stderr.contains("  '/gone-two/' in 'index.html'"), "stderr:\n{stderr}");
    assert!(
        !stderr.contains("reported above"),
        "the list must not raise the problem count; stderr:\n{stderr}"
    );
}
