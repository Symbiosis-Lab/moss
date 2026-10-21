//! A footer.md that ends in a `:::hero` closes the page with that picture: the
//! hero renders in the footer (not hoisted to the page's hero slot, which a
//! footer has none of), is marked as the closing picture, stays outside the
//! footer's heading columns, and has the colophon after it inside the footer.

#[path = "support/site.rs"]
mod site;

use site::Site;

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

const HERO: &str = ":::hero {image=day.png dark=night.png align=top}\n## The shelf is open.\n\n[Propose a collection](https://example.org/new)\n:::\n";

fn build(footer: &str) -> Site {
    let site = Site::new(&[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", footer)]);
    for name in ["day.png", "night.png"] {
        std::fs::write(site.root.join(name), PNG).unwrap();
    }
    site.rebuild();
    site
}

fn footer_of(html: &str) -> &str {
    let from = html.find("<footer").expect("a footer");
    &html[from..html[from..].find("</footer>").map(|i| from + i).expect("footer closes")]
}

#[test]
fn a_last_block_hero_is_the_footers_closing_picture() {
    let site = build(HERO);
    let html = site.html("index.html");
    let footer = footer_of(&html);

    let hero_at = footer.find("<section class=\"moss-hero moss-hero-closing\"").expect("the hero renders in the footer");
    assert!(footer.contains("data-mobile=\"overlay\""), "words stay on the picture on a phone: {footer}");
    assert!(footer.contains("data-align=\"top\""), "{footer}");
    assert!(footer.contains("The shelf is open."), "{footer}");
    assert!(
        footer.find("moss-colophon").unwrap() > hero_at,
        "the colophon follows the picture inside the footer"
    );
    assert!(!html[..html.find("<footer").unwrap()].contains("moss-hero"), "not hoisted above the page");
}

#[test]
fn the_hero_pairs_a_light_and_a_dark_picture() {
    let site = build(HERO);
    let html = site.html("index.html");
    let pictures: Vec<&str> = footer_of(&html).split("<picture").skip(1).collect();
    assert_eq!(pictures.len(), 2);
    assert!(pictures[0].contains("/day.") && pictures[0].contains("data-hero-variant=\"light\""));
    assert!(pictures[1].contains("/night.") && pictures[1].contains("data-hero-variant=\"dark\""));
}

#[test]
fn a_closing_hero_stays_outside_the_heading_columns() {
    let site = build(&format!("## Contact\n\nWrite to us.\n\n{HERO}"));
    let html = site.html("index.html");
    let footer = footer_of(&html);
    let columns_end = footer.find("</section>").expect("the Contact column closes");
    assert!(footer[..columns_end].contains("Write to us."));
    assert!(footer[columns_end..].contains("moss-hero-closing"), "the picture follows the columns: {footer}");
    assert!(!footer[..columns_end].contains("moss-hero"), "and is not inside one: {footer}");
}

#[test]
fn a_hero_with_words_after_it_is_an_ordinary_hero() {
    let site = build(&format!("{HERO}\nMade with care.\n"));
    let html = site.html("index.html");
    assert!(!footer_of(&html).contains("moss-hero-closing"));
}
