//! The note that a theme token is outside what the closing picture's ink
//! measurement reads concerns only a site that has a closing picture: a
//! `:::hero` ending footer.md.

#[path = "support/site.rs"]
mod site;

use site::Site;

const THEME: &str = ":root { --moss-site-max-width: 1200px; }\n";
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn notes(site: &Site) -> usize {
    site.logged.iter().filter(|l| l.contains("closing picture's words")).count()
}

#[test]
fn a_site_without_a_closing_picture_gets_no_note_about_one() {
    let site = Site::new(&[("index.md", "---\ntitle: Example Lab\n---\n\nBody.\n"), (".moss/theme/style.css", THEME)]);
    assert_eq!(notes(&site), 0, "{:?}", site.logged);
}

#[test]
fn a_site_with_a_closing_picture_still_gets_it() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nBody.\n"),
        ("footer.md", ":::hero {image=day.png}\n## Open.\n:::\n"),
        (".moss/theme/style.css", THEME),
    ]);
    std::fs::write(site.root.join("day.png"), PNG).unwrap();
    let logged = site.rebuild();
    assert_eq!(logged.iter().filter(|l| l.contains("closing picture's words")).count(), 1, "{logged:?}");
}
