//! A place name missing from `.moss/places.toml` drops its page off every map,
//! so it is a problem naming the page, field and name, with the nearest listed
//! name. A listed place without coordinates is not.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PLACES: &str = "\n[terms.places]\ntype = \"place\"\nfields = [\"location\"]\ntitle = \"Places\"\n";

fn gazetteer() -> (&'static str, &'static str) {
    (".moss/places.toml", "[\"Harbor\"]\nlat = 35.0\nlng = 135.0\nprecision = \"city\"\n[\"Nowhere\"]\nprecision = \"city\"\n")
}

fn problems(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("is not in .moss/places.toml")).collect()
}

#[test]
fn an_unlisted_place_name_is_a_problem_with_a_suggestion() {
    let _ = moss_build::build::cli_output::take_cli_problems();
    let site = Site::with_config(PLACES, &[gazetteer(), ("index.md", "---\ntitle: Home\n---\n\nHi.\n"), ("posts/t.md", "---\ntitle: T\nlocation: Harbr\n---\n\nBody.\n")]);
    let found = problems(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    for part in ["posts/t.md", "location", "'Harbr'", "did you mean 'Harbor'"] {
        assert!(found[0].contains(part), "{part} missing from {}", found[0]);
    }
    assert!(moss_build::build::cli_output::take_cli_problems() >= 1, "counts toward --strict");
}

#[test]
fn a_listed_place_without_coordinates_is_not_unknown() {
    let site = Site::with_config(PLACES, &[gazetteer(), ("index.md", "---\ntitle: Home\n---\n\nHi.\n"), ("posts/t.md", "---\ntitle: T\nlocation: Nowhere\n---\n\nBody.\n")]);
    assert!(problems(&site).is_empty(), "{:?}", site.logged);
}
