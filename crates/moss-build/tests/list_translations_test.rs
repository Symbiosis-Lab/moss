//! `moss list --json` rows name a page's other-language versions, so an agent
//! can tell which rows are one piece without re-reading every file.

#[path = "support/site.rs"]
mod site;

use moss_build::build::emit::inventory::{inventory_path, InventoryEntry};
use site::Site;

#[test]
fn a_translated_page_names_its_other_versions_and_a_single_language_page_names_none() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("Essay.md", "---\ntitle: Essay\ndate: 2026-01-01\n---\n\nText.\n"),
        ("Song.md", "---\ntitle: Song\ndate: 2026-01-02\ntranslationKey: song\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/Lied.md", "---\ntitle: Lied\ndate: 2026-01-04\ntranslationKey: song\n---\n\nText.\n"),
    ]);
    let rows: Vec<InventoryEntry> =
        serde_json::from_slice(&std::fs::read(inventory_path(&site.root.join(".moss"))).unwrap()).unwrap();
    let by_title = |t: &str| rows.iter().find(|r| r.title == t).unwrap_or_else(|| panic!("no row {t}"));

    let song = by_title("Song");
    assert_eq!(song.translations, vec![by_title("Lied").url_path.clone()]);
    assert_eq!(by_title("Lied").translations, vec![song.url_path.clone()]);
    assert!(by_title("Essay").translations.is_empty());
}
