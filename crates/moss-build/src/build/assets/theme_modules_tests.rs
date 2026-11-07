use super::*;
use crate::build::cloud_ledger::InputEvidence;

fn theme(files: &[(&str, &str)]) -> (tempfile::TempDir, ThemeModules) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(".moss/theme");
    for (name, body) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let evidence = InputEvidence::new(root.path());
    let modules = ThemeModules::read(&dir, &evidence).unwrap();
    (root, modules)
}

#[test]
fn script_mjs_maps_every_sibling_js_and_mjs_to_a_hashed_url() {
    let (_root, m) = theme(&[
        ("script.mjs", "import './kernel.js';"),
        ("kernel.js", "export const k = 1;"),
        ("lib/figures.mjs", "export const f = 2;"),
        ("script.js", "var classic;"),
        ("grain.png", "not js"),
    ]);
    let map = m.importmap_tag();
    assert!(map.contains(r#"type="importmap""#), "{map}");
    assert!(map.contains("\"/_moss/theme/kernel.js\":\"/_moss/theme/kernel."), "{map}");
    assert!(map.contains("\"/_moss/theme/lib/figures.mjs\":\"/_moss/theme/lib/figures."), "{map}");
    assert!(!map.contains("grain.png"), "{map}");
    assert!(!map.contains("\"/_moss/theme/script.js\""), "script.js stays classic: {map}");
    let tag = m.entry_tag();
    assert!(tag.contains(r#"<script type="module" src="/_moss/theme/script."#), "{tag}");
}

#[test]
fn a_lt_in_a_file_name_never_reaches_the_script_block_raw() {
    let (_root, m) = theme(&[("a<b.js", "export {};")]);
    let map = m.importmap_tag();
    assert!(map.contains("a\\u003cb.js"), "{map}");
    assert_eq!(map.matches('<').count(), 2, "only the script tag's own brackets: {map}");
}

/// Modules are read in composed name order, so their hashes enter the render
/// fingerprint in one order on every machine.
#[test]
fn modules_are_read_in_composed_name_order() {
    let (_root, m) = theme(&[("e\u{301}clair.js", "export const a = 1;"), ("f.js", "export const b = 2;")]);
    let rels: Vec<&str> = m.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(rels, ["f.js", "e\u{301}clair.js"]);
}
