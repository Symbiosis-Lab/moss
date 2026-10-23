//! An archival page whose footer ends on a closing picture: the edition block
//! sits on the picture's foot, beside the credit, above the colophon's row.

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

const HERO: &str = ":::hero {image=day.png align=top}\n## The shelf is open.\n:::\n";

fn build(archival: bool, footer: &str) -> String {
    let config = if archival { "archival = true\n" } else { "" };
    let site = Site::with_config(config, &[("index.md", "---\ntitle: Harbour\n---\n\nBody text.\n"), ("footer.md", footer)]);
    std::fs::write(site.root.join("day.png"), PNG).unwrap();
    site.rebuild();
    site.html("index.html")
}

/// The closing picture's whole element, from its opening tag to its close.
fn closing_picture(html: &str) -> &str {
    let from = html.find("<section class=\"moss-hero moss-hero-closing\"").expect("a closing picture");
    let to = from + html[from..].find("</section>").expect("the picture closes") + "</section>".len();
    &html[from..to]
}

#[test]
fn the_edition_sits_on_a_closing_pictures_foot_above_the_colophon() {
    let html = build(true, HERO);
    let picture = closing_picture(&html);
    assert_eq!(html.matches("class=\"moss-edition\"").count(), 1, "one edition block: {html}");
    assert!(picture.contains("class=\"moss-edition\""), "the edition is inside the picture: {picture}");
    assert!(picture.contains("Cite this page"), "{picture}");
    let colophon = html.find("class=\"moss-colophon\"").expect("a colophon");
    assert!(colophon > html.find(picture).unwrap() + picture.len(), "the colophon follows the picture: {html}");
}

#[test]
fn an_archival_footer_without_a_closing_picture_keeps_the_edition_in_the_footer() {
    let html = build(true, "Made with care.\n");
    assert!(!html.contains("moss-hero-closing"), "{html}");
    let from = html.find("<footer").unwrap();
    let footer = &html[from..from + html[from..].find("</footer>").unwrap()];
    let words = footer.find("Made with care.").expect("the footer text");
    let edition = footer.find("class=\"moss-edition\"").expect("the edition in the footer");
    assert!(words < edition, "the edition follows the footer's text: {footer}");
}

#[test]
fn a_closing_picture_without_archival_carries_no_edition() {
    let html = build(false, HERO);
    assert!(closing_picture(&html).contains("moss-hero-closing"));
    assert!(!html.contains("moss-edition"), "{html}");
}
