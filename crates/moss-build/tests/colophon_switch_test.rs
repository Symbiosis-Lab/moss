//! `[site].colophon = false` leaves moss's credit line out of every page; a
//! theme no longer has to hide it with CSS.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"),
    ("footer.md", "## Contact\n\nWrite to us.\n"),
];

#[test]
fn the_colophon_ships_unless_the_site_turns_it_off() {
    let site = Site::new(&PAGES);
    let html = site.html("index.html");
    assert!(html.contains("class=\"moss-colophon\""), "default: {html}");
}

#[test]
fn colophon_false_omits_the_element_from_the_output() {
    let site = Site::with_config("colophon = false\n", &PAGES);
    let html = site.html("index.html");
    assert!(!html.contains("moss-colophon"), "{html}");
    assert!(!html.contains("Published with moss"), "{html}");
    assert!(html.contains("Write to us."), "the footer itself stays");
}
