//! A home page that embeds the map of a place root also in the nav gets one note
//! naming the kind and both places; the output is unchanged.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PLACES: &str = "\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n";

fn files<'a>(extra: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut all = vec![
        (".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n"),
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n\n![[/places/|style:map]]\n"),
        ("posts/report.md", "---\ntitle: Report\nlocation: Harbor\n---\n\nBody.\n"),
    ];
    all.extend_from_slice(extra);
    all
}

fn notes(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("is also a nav item")).collect()
}

#[test]
fn a_nav_kind_whose_map_the_home_page_embeds_gets_one_note() {
    let site = Site::with_config(&format!("{PLACES}nav = true\n"), &files(&[]));
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    assert!(found[0].contains("`places`") && found[0].contains("nav = true") && found[0].contains("index.md"), "{}", found[0]);
    assert!(site.html("index.html").contains("moss-place-map"), "the output is unchanged: the map still draws");
}

#[test]
fn a_place_root_page_in_the_nav_gets_the_note_too() {
    let site = Site::with_config(PLACES, &files(&[("places/index.md", "---\ntitle: Places\nnav: true\n---\n\nAll of them.\n")]));
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    assert!(found[0].contains("places/index.md"), "{}", found[0]);
}

#[test]
fn a_kind_outside_the_nav_gets_no_note() {
    let site = Site::with_config(PLACES, &files(&[]));
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}
