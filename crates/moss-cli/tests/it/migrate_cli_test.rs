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
fn lift_apply_on_a_page_without_a_uid_writes_no_uid() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "Piers/p1.md", "---\ntitle: \"Pier 1\"\n---\n\n::: {.eyebrow}\nPier 1\n:::\n\nText.\n");
    let o = migrate(&root, &home, &["--lift", "eyebrow", "--as", "stage", "--place", "eyebrow", "--apply", "--json"]);
    assert!(o.status.success(), "{}", stdout(&o));
    let page = std::fs::read_to_string(root.join("Piers/p1.md")).unwrap();
    assert!(!page.contains("uid:") && page.contains("\nstage: Pier 1\n") && !page.contains(":::"), "{page}");
}

#[test]
fn a_folder_ordered_by_weight_keeps_its_previous_and_next_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "Journal/Journal.md", "---\ntitle: Journal\nsort: weight\n---\n\nEntries.\n");
    write(&root, "Journal/one.md", "---\ntitle: One\nweight: 1\n---\n\nFirst.\n");
    write(&root, "Journal/two.md", "---\ntitle: Two\nweight: 2\n---\n\nSecond.\n");
    let o = migrate(&root, &home, &["--apply"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let journal = std::fs::read_to_string(root.join("Journal/Journal.md")).unwrap();
    assert_eq!(journal, "---\ntitle: Journal\nsort: weight\nseries: true\n---\n\nEntries.\n");
    let again = migrate(&root, &home, &["--json"]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&again)).expect("stdout is one JSON value");
    assert_eq!(report["changes"].as_array().unwrap().len(), 0, "{report}");
}

#[test]
fn a_stray_closer_is_deleted_in_the_same_change_as_the_page_s_other_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "Talks/index.md", "---\ntitle: Talks\nsort: [b, a]\n---\n\nPoster.\n10/25 · Hall\n:::\n");
    write(&root, "Talks/a.md", "---\ntitle: A\n---\n\nA.\n");
    write(&root, "Talks/b.md", "---\ntitle: B\n---\n\nB.\n");
    let o = migrate(&root, &home, &["--apply", "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let report: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("stdout is one JSON value");
    assert_eq!(report["changes"].as_array().unwrap().len(), 1, "{report}");
    let talks = std::fs::read_to_string(root.join("Talks/index.md")).unwrap();
    assert_eq!(talks, "---\ntitle: Talks\nsort: [b, a]\nseries: true\n---\n\nPoster.\n10/25 · Hall\n");
}

#[test]
fn the_first_build_after_an_older_moss_names_the_folder_migrate_then_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("site"), tmp.path().join("home"));
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "[site]\nlang = \"en\"\n");
    write(&root, "Shelf/Shelf.md", "---\ntitle: Shelf\ncascade:\n  sort: weight\n---\n\nShelf.\n");
    write(&root, "Shelf/Books/Books.md", "---\ntitle: Books\n---\n\nBooks.\n");
    write(&root, "Shelf/Books/a.md", "---\ntitle: A\n---\n\nA.\n");
    write(&root, "Shelf/Books/b.md", "---\ntitle: B\n---\n\nB.\n");
    // Only the cascade orders the folder. Each build reads a changed page, so none ends at the generation it already has.
    let build = |n: usize| {
        write(&root, "Shelf/Books/a.md", &format!("---\ntitle: A\n---\n\nA, take {n}.\n"));
        let o = Command::new(MOSS)
            .args(["build", root.to_str().unwrap(), "--no-plugins"])
            .env("MOSS_HOME", &home)
            .env("HOME", &home)
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stderr).into_owned()
    };
    let note = "Pages in `Shelf/Books` no longer link to the previous and next page";
    let first = build(1);
    assert!(!first.contains("no longer link"), "a folder never built before is not told");

    // The profile sits beside the build's output, wherever the folder's store is.
    let current = first.lines().find_map(|l| l.split("Site generated at ").nth(1)).expect("the build names its output");
    let profile = Path::new(current.trim()).parent().unwrap().join("profile.jsonl");
    let mut records = std::fs::read_to_string(&profile).unwrap();
    records.push_str("{\"kind\":\"build\",\"moss_version\":\"0.15.29\"}\n");
    std::fs::write(&profile, records).unwrap();
    assert!(build(2).contains(note), "the first build after 0.15.29 names the folder");
    assert!(!build(3).contains("no longer link"), "the next build does not repeat it");

    let o = Command::new(MOSS).arg("migrate").arg(&root).arg("--apply").env("MOSS_HOME", &home).env("HOME", &home).env("XDG_CACHE_HOME", home.join(".cache")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_to_string(root.join("Shelf/Books/Books.md")).unwrap(), "---\ntitle: Books\nseries: true\n---\n\nBooks.\n");
}
