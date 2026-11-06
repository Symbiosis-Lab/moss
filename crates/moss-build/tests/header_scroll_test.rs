//! `[site].header_scroll` marks the page body so the stylesheet and script
//! keep the header at the top; `static` (the default) writes nothing.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: [(&str, &str); 1] = [("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n")];

#[test]
fn a_site_scrolls_its_header_away_unless_it_asks_otherwise() {
    let site = Site::new(&PAGES);
    assert!(!site.html("index.html").contains("data-header-scroll"));
}

#[test]
fn sticky_and_hide_mark_the_body() {
    for value in ["sticky", "hide"] {
        let site = Site::with_config(&format!("header_scroll = \"{value}\"\n"), &PAGES);
        let html = site.html("index.html");
        assert!(html.contains(&format!("data-header-scroll=\"{value}\"")), "{value}: {html}");
    }
}

#[test]
fn an_unknown_value_warns_and_stays_static() {
    let site = Site::with_config("header_scroll = \"slide\"\n", &PAGES);
    assert!(!site.html("index.html").contains("data-header-scroll"));
    assert!(
        site.logged.iter().any(|l| l.contains("header_scroll") && l.contains("slide")),
        "{:?}",
        site.logged
    );
}

#[test]
fn a_site_without_a_header_has_nothing_to_keep() {
    let site = Site::with_config("header = \"none\"\nheader_scroll = \"hide\"\n", &PAGES);
    assert!(!site.html("index.html").contains("data-header-scroll"));
}
