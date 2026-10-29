//! `![[figure.png|Description|]]` means what `![Description|](figure.png)` means:
//! the description is the image's alt text, there is no caption, and the
//! trailing `|` is syntax that never reaches the page.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xfc, 0xcf, 0xc0, 0x50, 0x0f, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xa9, 0x8c, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn page(body: &str) -> String {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n"),
        ("p.md", &format!("---\ntitle: P\n---\n\n{body}\n")),
    ]);
    std::fs::write(site.root.join("figure.png"), PNG).unwrap();
    site.rebuild();
    site.html("p/index.html")
}

#[test]
fn a_trailing_pipe_makes_the_wiki_description_alt_only() {
    let html = page("![[figure.png|A description of the figure|]]");
    assert!(html.contains("alt=\"A description of the figure\""), "{html}");
    assert!(!html.contains("<figcaption") && !html.contains("figure|"), "{html}");
}

#[test]
fn without_the_pipe_the_wiki_description_stays_the_caption() {
    let html = page("![[figure.png|A description]]");
    assert!(html.contains("<figcaption>A description</figcaption>"), "{html}");
}

#[test]
fn an_escaped_pipe_stays_in_the_alt() {
    let html = page("![[figure.png|Desc F\\||]]");
    assert!(html.contains("alt=\"Desc F|\""), "{html}");
    assert!(!html.contains("<figcaption"), "{html}");
}
