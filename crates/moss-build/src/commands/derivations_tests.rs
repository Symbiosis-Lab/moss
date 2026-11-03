//! The derivation commands against a temporary site holding an invented
//! plugin (`fake-reader`, files ending `.fake`) whose deriver is a shell script.

use std::fs;

use super::*;

const DERIVER: &str = r###"#!/bin/sh
REQ=$(cat)
case "$REQ" in
  *locked*) printf '%s' '{"kind":"skip","reason":"locked","message":"This file is locked."}' ;;
  *) printf '%s' '{"kind":"derived","fragments":{"text":"## Hello\n\nworld"},"facts":{"title":"From Facts"}}' ;;
esac
"###;

/// A site with the plugin installed and `files` written (paths relative to the site).
fn site(files: &[(&str, &str)]) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join(".moss/plugins/fake-reader");
    fs::create_dir_all(&plugin).unwrap();
    let deriver = plugin.join("deriver.sh");
    fs::write(&deriver, DERIVER).unwrap();
    fs::set_permissions(&deriver, fs::Permissions::from_mode(0o755)).unwrap();
    let manifest = serde_json::json!({
        "name": "fake-reader",
        "version": "1.0.0",
        "contributes": {"derivations": {"reader": {
            "accepts": ["fake"],
            "noun": {"one": "fake", "other": "fakes"},
            "fragments": {"text": "markdown"},
            "stub": ["#text"],
            "run": {"binary": "deriver", "artifact": [{"kind": "path", "binary": deriver}]}
        }}}
    });
    fs::write(plugin.join("manifest.json"), manifest.to_string()).unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

fn row<'a>(rows: &'a [FolderDerivation], folder: &str) -> &'a FolderDerivation {
    rows.iter().find(|r| r.folder == folder).unwrap_or_else(|| panic!("no row for {folder}: {rows:?}"))
}

fn show() -> FolderDerivationChoice {
    FolderDerivationChoice::Show("reader".to_string())
}

#[test]
fn state_lists_a_folder_of_accepted_files_as_undecided() {
    let dir = site(&[("reports/a.fake", "x"), ("reports/b.fake", "y"), ("notes/n.md", "hi")]);
    let rows = derivation_state_in(dir.path()).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let r = row(&rows, "reports/");
    assert_eq!((r.derivation_id.as_str(), r.noun_one.as_str(), r.noun_other.as_str()), ("reader", "fake", "fakes"));
    assert_eq!(r.count, 2);
    assert_eq!(r.decision, DerivationDecision::Undecided);
    assert_eq!(r.stubs_missing, 0);
}

#[test]
fn state_follows_the_decision_and_counts_missing_pages() {
    let dir = site(&[("reports/a.fake", "x")]);
    set_folder_derivation_in(dir.path(), "reports/", show()).unwrap();
    fs::remove_file(dir.path().join("reports/a.md")).unwrap();
    let rows = derivation_state_in(dir.path()).unwrap();
    let r = row(&rows, "reports/");
    assert_eq!(r.decision, DerivationDecision::Show);
    assert_eq!(r.stubs_missing, 1, "a shown folder reports the page it lacks");
}

#[test]
fn state_reports_the_files_the_plugin_declined() {
    let dir = site(&[("reports/ok.fake", "x"), ("reports/locked.fake", "locked")]);
    let page = "---\n---\n\n![[locked.fake#text]]\n";
    fs::write(dir.path().join("reports/p.md"), page).unwrap();
    // The decline is found in the cache, which a read of the file fills.
    crate::system::derive::site::DerivedSite::load(true, dir.path()).expect("the plugin derives").read("reports/locked.fake", "text");
    let rows = derivation_state_in(dir.path()).unwrap();
    let r = row(&rows, "reports/");
    assert_eq!(r.skipped.len(), 1, "{r:?}");
    assert_eq!((r.skipped[0].file.as_str(), r.skipped[0].reason.as_str()), ("locked.fake", "locked"));
}

#[test]
fn showing_a_folder_records_the_choice_and_writes_its_pages() {
    let dir = site(&[("reports/a.fake", "x"), ("reports/b.fake", "y")]);
    let mut written = set_folder_derivation_in(dir.path(), "reports/", show()).unwrap();
    written.sort();
    assert_eq!(written, ["reports/a.md", "reports/b.md"]);
    assert!(fs::read_to_string(dir.path().join("reports/a.md")).unwrap().contains("![[a.fake#text]]"));
    let config = fs::read_to_string(dir.path().join(".moss/config.toml")).unwrap();
    assert!(config.contains("\"reports/\" = [\"reader\"]"), "{config}");
    // Doing it again writes nothing new.
    assert!(set_folder_derivation_in(dir.path(), "reports/", show()).unwrap().is_empty());
}

#[test]
fn keeping_after_showing_leaves_the_pages_and_flips_the_decision() {
    let dir = site(&[("reports/a.fake", "x")]);
    set_folder_derivation_in(dir.path(), "reports/", show()).unwrap();
    let written = set_folder_derivation_in(dir.path(), "reports/", FolderDerivationChoice::Keep).unwrap();
    assert!(written.is_empty());
    assert!(dir.path().join("reports/a.md").exists(), "undo leaves the pages already written");
    let rows = derivation_state_in(dir.path()).unwrap();
    assert_eq!(row(&rows, "reports/").decision, DerivationDecision::Keep);
}

#[test]
fn showing_refuses_an_unknown_derivation_and_a_folder_outside_the_site() {
    let dir = site(&[("reports/a.fake", "x")]);
    let unknown = FolderDerivationChoice::Show("nope".to_string());
    assert!(set_folder_derivation_in(dir.path(), "reports/", unknown).unwrap_err().contains("nope"));
    assert!(set_folder_derivation_in(dir.path(), "../elsewhere/", show()).is_err());
    assert!(set_folder_derivation_in(dir.path(), "missing/", FolderDerivationChoice::Keep).is_err());
    assert!(!dir.path().join(".moss/config.toml").exists(), "a refused choice records nothing");
}

