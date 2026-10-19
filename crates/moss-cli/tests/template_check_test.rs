//! A template that cannot place its pages, or a language version with no
//! language tree, is a build problem: the build names the template file and
//! the missing piece on stderr, and `--strict` exits 1.

use std::path::Path;
use std::process::{Command, Output, Stdio};

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn production_template(folder: &str) -> String {
    format!("---\n_template_name: Production\n_template_kind: page\n_template_folder: {folder}\n---\n\nPoster.\n")
}

/// Builds a site under `--strict` and returns the exit status with stderr.
fn strict_build(files: &[(&str, &str)]) -> (Output, String) {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site = tempfile::tempdir().expect("tempdir for the site");
    for (rel, body) in files {
        write(site.path(), rel, body);
    }
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

const HOME: &str = "---\ntitle: Home\n---\n\nHello.\n";
const WORK_PAGE: &str = "---\ntitle: First work\n---\n\nA work.\n";

#[test]
fn build_warns_when_a_template_folder_is_gone() {
    let (out, stderr) = strict_build(&[
        ("index.md", HOME),
        (".moss/templates/production.md", &production_template("work/")),
    ]);
    assert!(stderr.contains("production.md"), "stderr:\n{stderr}");
    assert!(stderr.contains("work/"), "stderr:\n{stderr}");
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
}

#[test]
fn build_warns_on_a_language_file_without_a_tree() {
    let (out, stderr) = strict_build(&[
        ("index.md", HOME),
        ("work/first.md", WORK_PAGE),
        (".moss/templates/production.md", &production_template("work/")),
        (".moss/templates/production.zh-hans.md", &production_template("work/")),
    ]);
    assert!(stderr.contains("production.zh-hans.md"), "stderr:\n{stderr}");
    assert!(stderr.contains("zh-hans"), "stderr:\n{stderr}");
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
}

#[test]
fn a_template_whose_folder_exists_builds_clean_under_strict() {
    let (out, stderr) = strict_build(&[
        ("index.md", HOME),
        ("work/first.md", WORK_PAGE),
        (".moss/templates/production.md", &production_template("work/")),
    ]);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert!(!stderr.contains("production.md"), "stderr:\n{stderr}");
}

#[test]
fn build_warns_on_a_template_file_it_cannot_read_as_a_template() {
    let (out, stderr) = strict_build(&[
        ("index.md", HOME),
        ("work/first.md", WORK_PAGE),
        (".moss/templates/production.md", "---\n_template_name: Production\n---\n\nNo kind.\n"),
    ]);
    assert!(stderr.contains("production.md"), "stderr:\n{stderr}");
    assert!(!out.status.success(), "--strict must fail; stderr:\n{stderr}");
}
