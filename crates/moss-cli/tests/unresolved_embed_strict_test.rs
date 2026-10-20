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

#[test]
fn unknown_folder_embed_option_fails_strict_and_keeps_the_known_ones() {
    let moss_home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir(site.path().join("work")).unwrap();
    std::fs::write(site.path().join("work/a.md"), "---\ntitle: Example Show\n---\n\nBody\n").unwrap();
    std::fs::write(
        site.path().join("index.md"),
        "---\ntitle: Home\n---\n\n![[work/|style:grid,bogus:1]]\n",
    )
    .unwrap();
    let site_dir = std::fs::canonicalize(site.path()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build"])
        .arg(&site_dir)
        .args(["--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
    assert!(
        stderr.contains("index.md") && stderr.contains("bogus") && stderr.contains("known options") && stderr.contains("limit"),
        "stderr:\n{stderr}"
    );
    let html = find_home_html(&site_dir).expect("built index.html");
    assert!(html.contains("Example Show"), "listing must render");
    assert!(!html.contains("style:grid,bogus:1"), "option string must not leak as a caption");
}

fn find_home_html(site: &std::path::Path) -> Option<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out) } else if p.file_name().is_some_and(|n| n == "index.html") { out.push(p) }
        }
    }
    let mut all = Vec::new();
    walk(site, &mut all);
    all.sort_by_key(|p| p.components().count());
    all.first().and_then(|p| std::fs::read_to_string(p).ok())
}

#[test]
fn unknown_callout_type_fails_strict_naming_page_and_type_and_still_renders() {
    let moss_home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    std::fs::write(
        site.path().join("index.md"),
        "---\ntitle: Home\n---\n\n> [!bio]- Bio\n> Example Hall.\n",
    )
    .unwrap();
    let site_dir = std::fs::canonicalize(site.path()).unwrap();
    let run = |strict: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
        cmd.arg("build").arg(&site_dir).arg("--no-plugins");
        if strict {
            cmd.arg("--strict");
        }
        cmd.env("MOSS_HOME", moss_home.path()).stdin(Stdio::null()).output().unwrap()
    };
    let out = run(true);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
    assert!(
        stderr.contains("index.md") && stderr.contains("bio") && stderr.contains("callout"),
        "stderr:\n{stderr}"
    );
    let out = run(false);
    assert!(out.status.success(), "a plain build still succeeds");
    let html = find_home_html(&site_dir).expect("built index.html");
    assert!(html.contains("data-type=\"note\"") && html.contains("Example Hall"), "renders as a note");
}
