//! A page's `subtitle:` is drawn under its own title, once, wherever the
//! title block sits.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
    0xda, 0x63, 0x64, 0x60, 0x00, 0x00, 0x00, 0x06, 0x00, 0x02, 0x30, 0x81, 0xd0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn assert_once_after_title(html: &str, what: &str) {
    assert_eq!(html.matches("示例").count(), 1, "{what}: {html}");
    assert!(html.contains(r#"<p class="moss-subtitle""#), "{what}: {html}");
    let title = html.find("</h1>").unwrap();
    assert!(title < html.find("示例").unwrap(), "{what}: the subtitle comes after the title");
}

fn page(front: &str) -> String {
    format!("---\ntitle: Opening Night\nsubtitle: 示例\n{front}---\n\n![A hall](hall.png)\n\nBody.\n")
}

#[test]
fn a_page_draws_its_subtitle_under_its_title() {
    let site = Site::new(&[("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"), ("opening.md", &page(""))]);
    assert_once_after_title(&site.html("opening/index.html"), "plain");
}

#[test]
fn a_page_without_a_subtitle_draws_none() {
    let site = Site::new(&[("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"), ("closing.md", "---\ntitle: Closing\n---\n\nBody.\n")]);
    assert!(!site.html("closing/index.html").contains("moss-subtitle"));
}

#[test]
fn the_subtitle_follows_the_title_block_beside_and_after_a_picture() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("after.md", &page("title_block: after-picture\n")),
        ("beside.md", &page("title_block: beside-picture\n")),
    ]);
    std::fs::write(site.root.join("hall.png"), PNG).unwrap();
    site.rebuild();
    for (path, wrap) in [("after/index.html", false), ("beside/index.html", true)] {
        let html = site.html(path);
        assert_once_after_title(&html, path);
        assert!(html.find("<figure").unwrap() < html.find("<h1").unwrap(), "{path}: picture first");
        assert_eq!(html.contains("moss-title-beside"), wrap, "{path}");
        assert!(html.find("</h1>").unwrap() < html.find("示例").unwrap() && html.find("示例").unwrap() < html.rfind("Body.").unwrap(), "{path}");
    }
}

#[test]
fn a_folder_page_draws_its_subtitle() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("shows/index.md", "---\ntitle: Shows\nsubtitle: 示例\n---\n\nAll shows.\n"),
        ("shows/a.md", "---\ntitle: A\n---\n\nBody.\n"),
    ]);
    let html = site.html("shows/index.html");
    assert_eq!(html.matches("示例").count(), 1, "{html}");
}

#[test]
fn a_declared_field_with_the_same_value_draws_the_subtitle_only_once() {
    let site = Site::with_config(
        "",
        &[
            ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
            ("opening.md", "---\ntitle: Opening Night\nsubtitle: 示例\nsecond: 示例\n---\n\nBody.\n"),
        ],
    );
    site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[fields.second]\ntype = \"text\"\nplace = \"subtitle\"\n");
    site.rebuild();
    let html = site.html("opening/index.html");
    assert_eq!(html.matches("示例").count(), 1, "{html}");
    assert!(html.contains(r#"data-field="second""#), "{html}");
}
