use super::*;
use std::path::PathBuf;

fn site_with(config: Option<&str>) -> (tempfile::TempDir, MossPaths) {
    let dir = tempfile::tempdir().unwrap();
    // Canonical: a folder that does not exist yet is keyed by its spelling.
    let site = MossPaths::new(&std::fs::canonicalize(dir.path()).unwrap());
    if let Some(text) = config {
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        std::fs::write(site.config(), text).unwrap();
    }
    (dir, site)
}

fn config_text(site: &MossPaths) -> String {
    std::fs::read_to_string(site.config()).unwrap()
}

#[test]
fn a_folder_nobody_decided_is_undecided() {
    let (_d, site) = site_with(None);
    assert_eq!(folder_decision(&site, &PathBuf::from("reports")), Decision::Undecided);
}

#[test]
fn decisions_round_trip_for_relative_and_absolute_folders() {
    let (_d, site) = site_with(None);
    set_folder_decision(&site, &PathBuf::from("reports"), Decision::Show(vec!["pdf-pages".into()])).unwrap();
    set_folder_decision(&site, &site.project_root().join("scans/old"), Decision::Keep).unwrap();
    assert_eq!(
        folder_decision(&site, &site.project_root().join("reports")),
        Decision::Show(vec!["pdf-pages".into()])
    );
    assert_eq!(folder_decision(&site, &PathBuf::from("scans/old")), Decision::Keep);
    let text = config_text(&site);
    assert!(text.contains("\"reports/\" = [\"pdf-pages\"]"), "{text}");
    assert!(text.contains("\"scans/old/\" = false"), "{text}");
}

#[test]
fn setting_a_decision_keeps_comments_and_other_settings() {
    let (_d, site) = site_with(Some("# mine\n[site]\nlang = \"en\"\n"));
    set_folder_decision(&site, &PathBuf::from("a"), Decision::Keep).unwrap();
    let text = config_text(&site);
    assert!(text.starts_with("# mine\n[site]\nlang = \"en\"\n"), "{text}");
}

#[test]
fn saving_the_same_decision_again_writes_nothing() {
    let (_d, site) = site_with(None);
    set_folder_decision(&site, &PathBuf::from("a"), Decision::Keep).unwrap();
    let before = std::fs::metadata(site.config()).unwrap().modified().unwrap();
    let bytes = config_text(&site);
    std::thread::sleep(std::time::Duration::from_millis(20));
    set_folder_decision(&site, &PathBuf::from("a"), Decision::Keep).unwrap();
    assert_eq!(config_text(&site), bytes);
    assert_eq!(std::fs::metadata(site.config()).unwrap().modified().unwrap(), before);
}

#[test]
fn undecided_removes_the_key() {
    let (_d, site) = site_with(None);
    set_folder_decision(&site, &PathBuf::from("a"), Decision::Keep).unwrap();
    set_folder_decision(&site, &PathBuf::from("a"), Decision::Undecided).unwrap();
    assert_eq!(folder_decision(&site, &PathBuf::from("a")), Decision::Undecided);
}

#[test]
fn a_folder_outside_the_site_is_refused() {
    let (_d, site) = site_with(None);
    assert!(set_folder_decision(&site, Path::new("/elsewhere/else"), Decision::Keep).is_err());
}

fn heal(config: &str, pairs: &[(&str, &str)]) -> (bool, BTreeMap<String, Decision>) {
    let mut root: Table = toml::from_str(config).unwrap();
    let pairs: Vec<_> = pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let changed = heal_renamed_keys(&mut root, &pairs);
    (changed, decisions_in(&root))
}

const CONFIG: &str = "[derive]\n\"reports/\" = [\"pdf-pages\"]\n\"reports/old/\" = false\n\"reportsx/\" = false\n\"other/\" = false\n";

#[test]
fn renaming_a_folder_rekeys_it_and_the_keys_below_it() {
    let (changed, d) = heal(CONFIG, &[("reports", "papers")]);
    assert!(changed);
    assert_eq!(d["papers/"], Decision::Show(vec!["pdf-pages".into()]));
    assert_eq!(d["papers/old/"], Decision::Keep);
    assert!(!d.contains_key("reports/") && !d.contains_key("reports/old/"));
}

#[test]
fn healing_leaves_lookalike_and_unrelated_keys_alone() {
    let (_, d) = heal(CONFIG, &[("reports", "papers")]);
    assert_eq!(d["reportsx/"], Decision::Keep);
    assert_eq!(d["other/"], Decision::Keep);
}

#[test]
fn a_rename_of_a_parent_rekeys_nested_folders() {
    let (changed, d) = heal("[derive]\n\"a/b/\" = false\n", &[("a", "z")]);
    assert!(changed);
    assert!(d.contains_key("z/b/"));
}

#[test]
fn a_rename_that_touches_no_key_reports_no_change() {
    assert!(!heal(CONFIG, &[("elsewhere", "x")]).0);
    assert!(!heal("[site]\nlang = \"en\"\n", &[("a", "b")]).0);
}

#[test]
fn healing_the_file_rewrites_only_the_keys_and_keeps_comments() {
    let (_d, site) = site_with(Some("# mine\n[derive]\n\"reports/\" = false\n"));
    heal_renamed_folders(&site, &[("reports".into(), "papers".into())]).unwrap();
    let text = config_text(&site);
    assert!(text.starts_with("# mine\n"), "{text}");
    assert_eq!(folder_decision(&site, Path::new("papers")), Decision::Keep);
    assert_eq!(folder_decision(&site, Path::new("reports")), Decision::Undecided);
}

#[test]
fn renaming_a_folder_through_the_rename_engine_carries_its_decision_and_undo_restores_it() {
    let (_d, site) = site_with(None);
    let root = site.project_root().to_path_buf();
    std::fs::create_dir_all(root.join("reports")).unwrap();
    std::fs::write(root.join("reports/a.md"), "# a\n").unwrap();
    set_folder_decision(&site, &PathBuf::from("reports"), Decision::Keep).unwrap();

    let applied = crate::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &root.join("reports").to_string_lossy(),
        &root.join("papers").to_string_lossy(),
    ).unwrap();
    assert_eq!(folder_decision(&site, &PathBuf::from("papers")), Decision::Keep);
    assert_eq!(folder_decision(&site, &PathBuf::from("reports")), Decision::Undecided);

    crate::editor::rename_plan::undo_applied(&root, &applied).unwrap();
    assert_eq!(folder_decision(&site, &PathBuf::from("reports")), Decision::Keep);
    assert_eq!(folder_decision(&site, &PathBuf::from("papers")), Decision::Undecided);
}
