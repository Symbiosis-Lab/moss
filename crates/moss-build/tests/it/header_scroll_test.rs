//! `[site].header_scroll` marks the page body so the stylesheet and script
//! keep the header at the top; `static` (the default) writes nothing.

use crate::site;

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
    assert!(over.contains(r#"<header data-header="over""#), "{over}");
    assert!(!over.contains("data-header-scroll"), "{over}");
    assert!(!site.html("plain/index.html").contains(r#"<header data-header="over""#));
    assert!(site.html("plain/index.html").contains(r#"data-header-scroll="sticky""#));
}

#[test]
fn a_header_laid_over_the_opening_band_is_not_kept() {
    let site = Site::with_config(
        "header_scroll = \"sticky\"\n",
        &[
            ("index.md", "---\ntitle: Harbour\n---\n\n::: {screen header=over}\n# A river town\n:::\n\nBody.\n"),
            ("later.md", "---\ntitle: Later\n---\n\nIntro.\n\n::: {screen header=over}\n# A river town\n:::\n"),
        ],
    );
    let over = site.html("index.html");
    assert!(over.contains(r#"<header data-header="over""#) && !over.contains("data-header-scroll"), "{over}");
    let later = site.html("later/index.html");
    assert!(!later.contains("data-header=") && later.contains(r#"data-header-scroll="sticky""#), "{later}");
}

/// The header's ink over a band is measured from the band's picture as the
/// page is parsed; a picture replaced under the same name is measured again
/// on the rebuild that file's change starts, not replayed from the last parse.
#[test]
fn the_ink_over_a_band_follows_its_picture_when_the_picture_changes() {
    let page = "---\ntitle: Harbour\n---\n\n::: {screen header=over}\n![The harbour](harbour.png)\n:::\n\nBody.\n";
    let site = Site::new(&[("index.md", page)]);
    let picture = site.root.join("harbour.png");
    let paint = |rgb: [u8; 3]| image::RgbImage::from_pixel(1600, 900, image::Rgb(rgb)).save(&picture).unwrap();
    let header = |site: &Site| {
        let html = site.html("index.html");
        let at = html.find("<header").expect("a header");
        html[at..at + html[at..].find('>').unwrap()].to_string()
    };

    paint([12, 14, 24]);
    site.rebuild();
    assert!(header(&site).contains(r#"data-header-ink="light""#), "{}", header(&site));

    paint([244, 242, 236]);
    site::build_sync(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![picture.clone()]))
        .unwrap_or_else(|e| panic!("build failed: {e}"));
    assert!(header(&site).contains(r#"data-header-ink="dark""#), "{}", header(&site));
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
