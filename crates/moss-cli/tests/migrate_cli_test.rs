//! `moss migrate` through the real binary: the automatic rewrite builds a
//! scratch copy twice in one process, and `--apply` saves a version before it
//! writes. All names and text are invented.

use std::path::Path;
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn migrate(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(MOSS).arg("migrate").arg(root).args(args).env("MOSS_HOME", home).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_translation_key_the_path_implies_is_offered_once_the_site_builds_the_same_without_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "index.md", "---\ntitle: Home\n---\nHello\n");
    write(&root, "en/about.md", "---\ntitle: About\ntranslationKey: about\n---\nUs\n");
    write(&root, "de/about.md", "---\ntitle: Über uns\ntranslationKey: about\n---\nWir\n");
    let o = migrate(&root, &home, &["--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let report: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("stdout is one JSON value");
    assert_eq!(report["same_output"], true, "{report}");
    assert_eq!(report["changes"].as_array().unwrap().len(), 2, "{report}");
    assert_eq!(report["applied"], false);
    assert!(std::fs::read_to_string(root.join("en/about.md")).unwrap().contains("translationKey"), "a dry run writes nothing");
}

#[test]
fn lift_apply_saves_a_version_holding_the_old_pages_then_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "# the site\n[site]\nlang = \"en\"\n");
    write(&root, "index.md", "---\ntitle: Home\nuid: \"a1b2c3d6\"\n---\nHello\n");
    // With their uids written, the version's build stamps nothing into them.
    let before = "---\ntitle: \"Pier 1\"\nuid: \"a1b2c3d4\"\n---\n\n::: {.eyebrow}\nPier 1 of 2\n:::\n\nText.\n";
    write(&root, "Piers/p1.md", before);
    write(&root, "Piers/p2.md", "---\ntitle: \"Pier 2\"\nuid: \"a1b2c3d5\"\n---\n\n::: {.eyebrow}\nPier 2 of 2\n:::\n\nText.\n");
    let o = migrate(&root, &home, &["--lift", "eyebrow", "--as", "stage", "--place", "eyebrow", "--apply", "--json"]);
    assert!(o.status.success(), "{}", stdout(&o));
    let report: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("stdout is one JSON value");
    assert_eq!(report["applied"], true, "{report}");
    let id = report["saved_version"].as_str().expect("a saved version").to_string();
    assert_eq!(std::fs::read_to_string(root.join("Piers/p1.md")).unwrap(), "---\ntitle: \"Pier 1\"\nuid: \"a1b2c3d4\"\nstage: Pier 1 of 2\n---\n\nText.\n");
    assert!(std::fs::read_to_string(root.join(".moss/config.toml")).unwrap().starts_with("# the site\n"));

    let restored = Command::new(MOSS)
        .current_dir(&root)
        .args(["history", "Piers/p1.md", "--restore", "--at", &id])
        .env("MOSS_HOME", &home)
        .output()
        .unwrap();
    assert!(restored.status.success(), "{}", String::from_utf8_lossy(&restored.stderr));
    assert_eq!(std::fs::read_to_string(root.join("Piers/p1.md")).unwrap(), before);
}

#[test]
fn a_usage_error_under_json_is_json() {
    let tmp = tempfile::tempdir().unwrap();
    let o = migrate(tmp.path(), tmp.path(), &["--lift", "x", "--json"]);
    assert_eq!(o.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("stdout is one JSON value");
    assert!(report["error"].as_str().unwrap().contains("--as"), "{report}");
}

#[test]
fn lift_apply_on_a_page_without_a_uid_plans_from_the_file_the_build_stamped() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "Piers/p1.md", "---\ntitle: \"Pier 1\"\n---\n\n::: {.eyebrow}\nPier 1\n:::\n\nText.\n");
    let o = migrate(&root, &home, &["--lift", "eyebrow", "--as", "stage", "--apply", "--json"]);
    assert!(o.status.success(), "{}", stdout(&o));
    let page = std::fs::read_to_string(root.join("Piers/p1.md")).unwrap();
    assert!(page.contains("\nuid: ") && page.contains("\nstage: Pier 1\n") && !page.contains(":::"), "{page}");
}
