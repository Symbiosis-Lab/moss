use super::*;
use crate::system::derive::test_support::entry;
use std::fs;

fn fixture(stub: &[&str]) -> (tempfile::TempDir, MossPaths, DerivationEntry) {
    let dir = tempfile::tempdir().unwrap();
    let site = MossPaths::new(dir.path());
    let mut e = entry(Path::new("/bin/true"), "1.0.0");
    e.contribution.accepts = vec!["fake".into()];
    e.contribution.stub = stub.iter().map(|s| s.to_string()).collect();
    (dir, site, e)
}

fn touch(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

#[test]
fn a_stub_embeds_its_file_in_the_layout_the_manifest_gives() {
    let (d, _site, e) = fixture(&["#text", ""]);
    touch(d.path(), "017 Report.fake", "x");
    let written = write_stubs(d.path(), &e);
    assert_eq!(written, vec![d.path().join("017 Report.md")]);
    assert_eq!(
        fs::read_to_string(&written[0]).unwrap(),
        "---\ninferred: [title, description, date]\n---\n![[017 Report.fake#text]]\n\n![[017 Report.fake]]\n"
    );
}

#[test]
fn another_layout_gives_another_stub() {
    let (d, _site, e) = fixture(&["", "#text"]);
    touch(d.path(), "a.fake", "x");
    let written = write_stubs(d.path(), &e);
    assert!(fs::read_to_string(&written[0]).unwrap().ends_with("![[a.fake]]\n\n![[a.fake#text]]\n"));
}

#[test]
fn files_the_derivation_does_not_accept_get_no_stub() {
    let (d, _site, e) = fixture(&["#text"]);
    touch(d.path(), "a.png", "x");
    touch(d.path(), "b.md", "page");
    assert!(write_stubs(d.path(), &e).is_empty());
}

#[test]
fn an_existing_page_of_the_same_name_is_never_rewritten() {
    let (d, _site, e) = fixture(&["#text"]);
    touch(d.path(), "a.fake", "x");
    touch(d.path(), "a.md", "mine");
    assert!(write_stubs(d.path(), &e).is_empty());
    assert_eq!(fs::read_to_string(d.path().join("a.md")).unwrap(), "mine");
}

#[test]
fn a_file_another_page_already_embeds_gets_no_stub() {
    let (d, _site, e) = fixture(&["#text"]);
    touch(d.path(), "a b.fake", "x");
    touch(d.path(), "c.fake", "x");
    touch(d.path(), "notes.md", "Intro\n\n![[a b.fake#text|caption]]\n");
    let written = write_stubs(d.path(), &e);
    assert_eq!(written, vec![d.path().join("c.md")]);
}

#[test]
fn running_it_twice_writes_nothing_the_second_time() {
    let (d, site, e) = fixture(&["#text"]);
    touch(d.path(), "a.fake", "x");
    assert_eq!(write_stubs(d.path(), &e).len(), 1);
    assert!(write_stubs(d.path(), &e).is_empty());
    assert_eq!(stubs_missing(d.path(), &e), 0);
}

#[test]
fn a_deleted_stub_stays_deleted_until_someone_asks_again() {
    let (d, site, e) = fixture(&["#text"]);
    touch(d.path(), "a.fake", "x");
    let written = write_stubs(d.path(), &e);
    fs::remove_file(&written[0]).unwrap();
    assert_eq!(stubs_missing(d.path(), &e), 1);
}

fn registry_for(e: &DerivationEntry) -> DerivationRegistry {
    let json = format!(
        r##"{{"name":"{}","version":"1.0.0","contributes":{{"derivations":{{"{}":{}}}}}}}"##,
        e.plugin_id,
        e.derivation_id,
        serde_json::to_string(&e.contribution).unwrap()
    );
    DerivationRegistry::from_plugins(&[crate::plugins::types::Plugin {
        manifest: crate::plugins::types::PluginManifest::parse(&json).unwrap(),
        path: PathBuf::new(),
    }])
}

#[test]
fn an_arrival_in_a_shown_folder_gets_only_its_own_stub() {
    let (d, site, e) = fixture(&["#text"]);
    let reg = registry_for(&e);
    let folder = d.path().join("reports");
    touch(&folder, "old.fake", "x");
    touch(&folder, "new.fake", "x");
    super::super::derive_folders::set_folder_decision(
        &site,
        &folder,
        Decision::Show(vec![e.derivation_id.clone()]),
    )
    .unwrap();
    let written = stub_arrivals(&site, &reg, &[folder.join("new.fake")]);
    assert_eq!(written, vec![folder.join("new.md")]);
    assert!(!folder.join("old.md").exists());
}

#[test]
fn an_arrival_in_an_undecided_or_kept_folder_writes_nothing() {
    let (d, site, e) = fixture(&["#text"]);
    let reg = registry_for(&e);
    touch(d.path(), "a.fake", "x");
    assert!(stub_arrivals(&site, &reg, &[d.path().join("a.fake")]).is_empty());
    super::super::derive_folders::set_folder_decision(&site, d.path(), Decision::Keep).unwrap();
    assert!(stub_arrivals(&site, &reg, &[d.path().join("a.fake")]).is_empty());
}

#[test]
fn an_embed_with_display_params_still_counts_as_embedding_the_file() {
    let (d, _site, e) = fixture(&["#text"]);
    touch(d.path(), "a.fake", "x");
    touch(d.path(), "page.md", "![[a.fake#text|w:300]]\n");
    assert!(write_stubs(d.path(), &e).is_empty());
}

#[test]
fn a_stub_is_never_written_over_an_existing_file() {
    let (d, _site, _e) = fixture(&["#text"]);
    touch(d.path(), "a.md", "the author's own page");
    assert!(!create_stub(&d.path().join("a.md"), "stub").unwrap());
    assert_eq!(fs::read_to_string(d.path().join("a.md")).unwrap(), "the author's own page");
}
