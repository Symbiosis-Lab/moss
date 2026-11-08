//! A country map's search field: the index a build writes beside the shapes,
//! the markup that points at it, `search: false`, and the field's language.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn page(extra: &str) -> String {
    format!("---\ntitle: Risk levels\ndate: 2026-11-03\ncountry_map:\n  by: Level\n  levels: [Low, High]\n{extra}---\n| Country | Level |\n|---|---|\n| Mexico | High |\n| Japan | Low |\n")
}

fn index_of(site: &Site) -> Option<std::path::PathBuf> {
    std::fs::read_dir(site.output("_moss")).ok()?.map(|e| e.unwrap().path().join("country-search.json")).find(|p| p.is_file())
}

#[test]
fn a_country_map_writes_one_index_and_its_field_names_it() {
    let site = Site::new(&[("index.md", "---\ntitle: Home\n---\n\nHi.\n"), ("risk.md", &page(""))]);
    let file = index_of(&site).expect("an index beside the shapes");
    let index: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(index["c"].as_array().unwrap().len() > 230);
    let html = site.html("risk/index.html");
    let dir = file.parent().unwrap().file_name().unwrap().to_string_lossy().to_string();
    assert!(html.contains(&format!("data-search=\"/_moss/{dir}/country-search.json\"")), "the figure names the index it fetches");
    assert!(html.contains("<a class=\"moss-country-search\" href=\"#moss-country-rows\""), "without script the field is a link to the list");
    assert!(html.contains("Search a country or city"));
    assert!(html.contains("id=\"moss-country-rows\""), "the link has somewhere to go");
}

#[test]
fn an_embedded_map_links_its_field_to_its_own_list() {
    let site = Site::new(&[("risk.md", &page("")), ("guide.md", "---\ntitle: Guide\n---\n\n![[risk|style:map]]\n")]);
    let html = site.html("guide/index.html");
    assert!(html.contains("<div class=\"moss-country-list\" id=\"moss-country-rows\">"), "{}", &html[..html.len().min(300)]);
    assert_eq!(html.matches("id=\"moss-country-rows\"").count(), 1);
}

#[test]
fn search_false_ships_neither_the_field_nor_the_index() {
    let site = Site::new(&[("risk.md", &page("  search: false\n"))]);
    assert!(index_of(&site).is_none());
    let html = site.html("risk/index.html");
    assert!(!html.contains("moss-country-search") && !html.contains("data-search"), "no field");
}

#[test]
fn the_field_and_its_messages_are_in_the_site_language() {
    let site = Site::in_language("de", "", &[("risk.md", &page(""))]);
    let html = site.html("risk/index.html");
    for text in ["Land oder Stadt suchen", "{n} Treffer", "Kein Land und keine Stadt passt zu", "Nicht bewertet"] {
        assert!(html.contains(text), "{text} missing");
    }
}
