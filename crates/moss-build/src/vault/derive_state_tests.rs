use super::*;
use std::path::Path;
use crate::system::derive::derive_cached;
use crate::system::derive::test_support::{script, DERIVED};
use std::fs;

const SKIP: &str = r#"printf '{"kind":"skip","reason":"locked","message":"It is locked."}'"#;

fn registry(dir: &Path, body: &str) -> DerivationRegistry {
    // One file per body: rewriting a binary is a re-pin, which the cache notices.
    let bin = script(dir, &format!("deriver-{}.sh", body.len()), body);
    let json = format!(
        r##"{{"name":"fakes","version":"1.0.0","contributes":{{"derivations":{{"pages":{{
            "accepts":["fake"],"noun":{{"one":"doc","other":"docs"}},
            "fragments":{{"text":"markdown"}},"stub":["#text"],
            "run":{{"binary":"unpack","artifact":[{{"kind":"path","binary":"{}"}}]}}}}}}}}}}"##,
        bin.display()
    );
    DerivationRegistry::from_plugins(&[crate::plugins::types::Plugin {
        manifest: crate::plugins::types::PluginManifest::parse(&json).unwrap(),
        path: dir.to_path_buf(),
    }])
}

fn decide(site: &MossPaths, folder: &str, d: Decision) {
    super::super::derive_folders::set_folder_decision(site, Path::new(folder), d).unwrap();
}

#[test]
fn folders_with_accepted_files_are_listed_at_any_depth_with_their_own_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    let site = MossPaths::new(&root);
    let reg = registry(dir.path(), DERIVED);
    for p in ["reports/a.fake", "reports/b.fake", "reports/deep/c.fake", "other/n.md", ".moss/x.fake"] {
        let path = root.join(p);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }
    let rows = derivation_state(&site, &reg);
    let summary: Vec<(&str, usize)> = rows.iter().map(|r| (r.folder.as_str(), r.count)).collect();
    assert_eq!(summary, vec![("reports/", 2), ("reports/deep/", 1)]);
    assert_eq!(rows[0].noun_other, "docs");
    assert_eq!(rows[0].decision, Decision::Undecided);
}

#[test]
fn a_subfolder_reports_the_decision_it_inherits_in_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    let site = MossPaths::new(&root);
    let reg = registry(dir.path(), DERIVED);
    fs::create_dir_all(root.join("reports/deep")).unwrap();
    fs::write(root.join("reports/deep/c.fake"), "x").unwrap();
    decide(&site, "reports", Decision::Show(vec!["pages".into()]));
    let rows = derivation_state(&site, &reg);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].folder, "reports/deep/");
    assert_eq!(rows[0].decision, Decision::Show(vec!["pages".into()]));
    assert_eq!(rows[0].stubs_missing, 1);
}

#[test]
fn a_decision_and_missing_stubs_show_up_in_the_row() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    let site = MossPaths::new(&root);
    let reg = registry(dir.path(), DERIVED);
    fs::create_dir_all(root.join("r")).unwrap();
    fs::write(root.join("r/a.fake"), "x").unwrap();
    decide(&site, "r", Decision::Show(vec!["pages".into()]));
    let row = &derivation_state(&site, &reg)[0];
    assert_eq!((row.decision.clone(), row.stubs_missing), (Decision::Show(vec!["pages".into()]), 1));
    decide(&site, "r", Decision::Keep);
    let row = &derivation_state(&site, &reg)[0];
    assert_eq!((row.decision.clone(), row.stubs_missing), (Decision::Keep, 0));
}

#[test]
fn a_skip_in_the_cache_is_reported_for_its_file_and_a_result_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    let site = MossPaths::new(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("locked.fake"), "one").unwrap();
    fs::write(root.join("fine.fake"), "two").unwrap();

    let skipping = registry(dir.path(), SKIP);
    derive_cached(&site, skipping.for_extension("fake").unwrap(), &root.join("locked.fake")).unwrap();
    let deriving = registry(dir.path(), DERIVED);
    derive_cached(&site, deriving.for_extension("fake").unwrap(), &root.join("fine.fake")).unwrap();

    let row = &derivation_state(&site, &skipping)[0];
    assert_eq!(row.count, 2);
    assert_eq!(
        row.skipped,
        vec![SkippedFile { file: "locked.fake".into(), reason: "locked".into(), message: "It is locked.".into() }]
    );
}

#[test]
fn a_file_whose_last_read_failed_is_listed_with_the_message_until_a_read_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("site");
    let site = MossPaths::new(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("bad.fake"), "one").unwrap();

    let failing = registry(dir.path(), "echo 'bad header' >&2; exit 3");
    assert!(derive_cached(&site, failing.for_extension("fake").unwrap(), &root.join("bad.fake")).is_err());
    let row = &derivation_state(&site, &failing)[0];
    assert_eq!(row.failed.len(), 1);
    assert_eq!(row.failed[0].file, "bad.fake");
    assert!(row.failed[0].message.contains("bad header"), "{:?}", row.failed);
    let json = serde_json::to_value(row).unwrap();
    assert_eq!(json["failed"][0]["file"], "bad.fake");

    let deriving = registry(dir.path(), DERIVED);
    derive_cached(&site, deriving.for_extension("fake").unwrap(), &root.join("bad.fake")).unwrap();
    assert!(derivation_state(&site, &deriving)[0].failed.is_empty());
}
