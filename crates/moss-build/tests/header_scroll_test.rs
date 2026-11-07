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

#[test]
fn a_header_laid_over_the_opening_hero_is_not_kept() {
    let site = Site::with_config(
        "header_scroll = \"sticky\"\n",
        &[
            ("index.md", "---\ntitle: Harbour\n---\n\n:::hero {header=over}\nWords\n:::\n\nBody.\n"),
            ("plain.md", "---\ntitle: Plain\n---\n\n:::hero\nWords\n:::\n\nBody.\n"),
        ],
    );
    let over = site.html("index.html");
    assert!(over.contains(r#"data-header="over""#), "{over}");
    assert!(!over.contains("data-header-scroll"), "{over}");
    assert!(site.html("plain/index.html").contains(r#"data-header-scroll="sticky""#));
}

/// `<meta name="theme-color">` carries the theme's own page colour, so a
/// phone's browser chrome matches the page instead of moss's default paper.
mod theme_color {
    use super::site::Site;

    #[test]
    fn the_theme_color_meta_follows_the_themes_page_colour() {
        let site = Site::with_config(
            "color_scheme = \"light\"\n",
            &[
                ("index.md", "---\ntitle: Home\n---\n\nHello.\n"),
                (".moss/theme/style.css", ":root { --day: #fbfbfb; --paper: var(--day); }\n:root, body[data-section] { --moss-color-bg: var(--paper); }\n"),
            ],
        );
        let home = site.html("index.html");
        assert!(home.contains(r##"<meta name="theme-color" content="#fbfbfb">"##), "{home}");
        assert!(!home.contains("#faf8f5"), "moss's default paper is gone from the head");
    }
}
