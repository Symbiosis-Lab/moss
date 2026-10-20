//! A `.moss/config.toml` that exists but does not parse fails the build: the
//! exit code is non-zero, stderr names the file and quotes the parser's
//! line and message, and stdout never claims a generated site.

use std::process::{Command, Stdio};

#[test]
fn unparseable_config_fails_the_build_and_quotes_the_parse_error() {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site = tempfile::tempdir().expect("tempdir for the site");
    std::fs::create_dir_all(site.path().join(".moss")).unwrap();
    // A bare non-ASCII key is not valid TOML.
    std::fs::write(
        site.path().join(".moss/config.toml"),
        "schema_version = 6\n\n[terms.地點]\ntype = \"place\"\n",
    )
    .unwrap();
    std::fs::write(site.path().join("index.md"), "---\ntitle: Home\nhome: true\n---\nhi\n").unwrap();
    let site_dir = std::fs::canonicalize(site.path()).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site_dir)
        .arg("--no-plugins")
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli build");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stderr.contains("config.toml") && stderr.contains("line 3, column 7") && stderr.contains("invalid table header"),
        "stderr:\n{stderr}"
    );
    assert!(!stdout.contains("Site generated") && !stdout.contains("Build complete"), "stdout:\n{stdout}");
    assert!(!site_dir.join(".moss/build.nosync/current").exists());
}
