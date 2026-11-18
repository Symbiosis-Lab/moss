//! Where the automatic place map lands on an article: heading the facts
//! column, after a body with no text, or beside the first text block. All
//! names are invented.

use std::path::{Path, PathBuf};
use std::process::Command;

const CONFIG: &str = r#"schema_version = 6

[site]
locator = "align-right"

[terms.places]
type = "place"
fields = ["location"]

[fields.architect]
type = "text"
place = "facts"
label = "Architect"

[fields.client]
type = "text"
place = "facts"
label = "Client"
"#;

const PLACES: &str = "[\"Harbor Row\"]\nlat = 40.71\nlng = -74.01\nprecision = \"exact\"\n";

/// A 1x1 PNG.
const PNG: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0x00, 0x00, 0x00, 0x06, 0x00, 0x02, 0x30, 0x81, 0xd0, 0x2f, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];

const FACTS: &str = "architect: Anna Weber\nclient: Harbor Trust\n";

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Builds a site of `pages` (path, text) and returns the built HTML of `page`.
fn built_page(pages: &[(&str, &str)], page: &str) -> String {
    let tmp = tempfile::tempdir().unwrap();
    let root: PathBuf = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", CONFIG.as_bytes());
    write(&root, ".moss/places.toml", PLACES.as_bytes());
    write(&root, "index.md", b"---\ntitle: Home\n---\nWelcome\n");
    write(&root, "Notes/photo.png", PNG);
    for (rel, text) in pages {
        write(&root, rel, text.as_bytes());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&root)
        .arg("--no-plugins")
        .env("MOSS_HOME", &home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let html = std::fs::read_to_string(root.join(".moss/build.nosync/current").join(page)).unwrap();
    // The head repeats the page's text and images; only the article is placed.
    html[html.find("<article").expect("an article")..].to_string()
}

fn at(html: &str, needle: &str) -> usize {
    html.find(needle).unwrap_or_else(|| panic!("missing {needle} in {html}"))
}

#[test]
fn a_page_with_a_facts_column_has_the_locator_head_it() {
    let page = format!("---\ntitle: Quay House\nlocation: Harbor Row\n{FACTS}---\n![[photo.png]]\n");
    let html = built_page(&[("Notes/quay.md", &page)], "notes/quay/index.html");
    assert_eq!(html.matches("moss-place-locator").count(), 1, "one locator: {html}");
    let column = at(&html, "<div class=\"moss-facts-column\"");
    let locator = at(&html, "class=\"moss-place-locator ");
    let first_fact = at(&html, "<div data-field=\"architect\">");
    assert!(column < locator && locator < first_fact, "locator inside the column, above the first fact: {html}");
    assert!(html[column..first_fact].contains("<dl class=\"moss-facts\">"), "{html}");
}

#[test]
fn a_page_with_text_and_a_facts_column_still_has_the_locator_head_it() {
    let page = format!("---\ntitle: Quay House\nlocation: Harbor Row\n{FACTS}---\nThe first paragraph.\n\nThe second.\n");
    let html = built_page(&[("Notes/quay.md", &page)], "notes/quay/index.html");
    let locator = at(&html, "class=\"moss-place-locator ");
    assert!(at(&html, "<div class=\"moss-facts-column\"") < locator && locator < at(&html, "<div data-field=\"architect\">"), "{html}");
    assert_eq!(html.matches("moss-place-locator").count(), 1, "one locator, none beside the text: {html}");
}

#[test]
fn a_text_less_body_without_facts_takes_the_locator_after_the_picture_unfloated() {
    let page = "---\ntitle: Quay House\nlocation: Harbor Row\n---\n![[photo.png]]\n";
    let html = built_page(&[("Notes/quay.md", page)], "notes/quay/index.html");
    let picture = at(&html, "photo");
    let locator = at(&html, "<div class=\"moss-place-locator moss-align-end\"");
    assert!(picture < locator, "locator after the picture: {html}");
    assert!(!html.contains("moss-place-locator moss-align-right"), "not floated: {html}");
}

#[test]
fn a_page_with_text_and_no_facts_floats_the_locator_before_the_first_text() {
    let page = "---\ntitle: Quay House\nlocation: Harbor Row\n---\n![[photo.png]]\n\nThe first paragraph.\n";
    let html = built_page(&[("Notes/quay.md", page)], "notes/quay/index.html");
    let picture = at(&html, "photo");
    let locator = at(&html, "<div class=\"moss-place-locator moss-align-right\"");
    assert!(picture < locator && locator < at(&html, "The first paragraph."), "{html}");
}
