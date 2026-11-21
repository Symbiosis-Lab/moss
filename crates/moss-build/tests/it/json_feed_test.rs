//! A JSON Feed item's `id` must not change when its article is edited, or
//! readers show the edit as a new article.

use crate::site;

use site::Site;

fn ids(site: &Site) -> Vec<(String, String)> {
    let feed: serde_json::Value = serde_json::from_str(&site.html("feed.json")).unwrap();
    feed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (item["title"].as_str().unwrap().to_string(), item["id"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn an_edited_article_keeps_its_feed_id_without_a_publish_record() {
    let site = Site::with_config("url = \"https://example.org\"\n", &[
        ("index.md", "# Home\n"),
        ("log.md", "---\ntitle: Harbour log\ndate: 2026-01-01\n---\n\nThe boats came in.\n"),
        ("pinned.md", "---\ntitle: Pinned\ndate: 2026-01-02\nuid: ab12cd34\n---\n\nKept.\n"),
    ]);
    let before = ids(&site);
    site.write("log.md", "---\ntitle: Harbour log\ndate: 2026-01-01\n---\n\nThe boats came in late.\n");
    site.write("pinned.md", "---\ntitle: Pinned\ndate: 2026-01-02\nuid: ab12cd34\n---\n\nKept, edited.\n");
    site.rebuild();
    assert_eq!(ids(&site), before);
    assert!(before.contains(&("Pinned".into(), "ab12cd34".into())), "{before:?}");
}

/// 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D,
    0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// Two articles whose covers are written beside them, one with display words.
fn covered_site() -> Site {
    let site = Site::with_config("url = \"https://example.org\"\n", &[
        ("index.md", "# Home\n"),
        ("Field Notes/long-article.md", "---\ntitle: Long article\ndate: 2026-01-01\ncover: one.png\n---\n\nText.\n"),
        ("Field Notes/framed.md", "---\ntitle: Framed\ndate: 2026-01-02\ncover: one.png|contain\n---\n\nText.\n"),
    ]);
    std::fs::write(site.root.join("Field Notes/one.png"), PNG).unwrap();
    site.rebuild();
    site
}

/// The address `url` names, checked against the files the build wrote.
fn assert_served(site: &Site, url: &str) {
    let rel = url.strip_prefix("https://example.org/").unwrap_or_else(|| panic!("{url} is not on the site"));
    assert!(site.output(rel).is_file(), "{url} names no built file");
}

const COVERED: [&str; 2] = ["Long article", "Framed"];

#[test]
fn a_feed_item_shows_the_picture_its_page_shares() {
    let site = covered_site();
    let feed: serde_json::Value = serde_json::from_str(&site.html("feed.json")).unwrap();
    for title in COVERED {
        let item = feed["items"].as_array().unwrap().iter().find(|i| i["title"] == title).unwrap();
        assert_eq!(item["image"], "https://example.org/field-notes/one.png", "{title}");
    }
    assert_served(&site, "https://example.org/field-notes/one.png");
}

#[test]
fn a_syndicated_copy_shows_the_picture_its_page_shares() {
    use moss_build::syndication::{channel::{ChannelSpec, Form}, shape::{shape_with, ShapeOptions}};
    let site = covered_site();
    let map_path = moss_build::moss_paths::MossPaths::new(&site.root).article_map();
    let map: moss_build::build::scan::article_map::ArticleMap =
        serde_json::from_str(&std::fs::read_to_string(map_path).unwrap()).unwrap();
    let spec = ChannelSpec { forms: vec![Form::Long], ..Default::default() };
    for title in COVERED {
        let post = map.articles.values().find(|a| a.title == title).unwrap();
        let shaped = shape_with(post, &spec, "https://example.org", &ShapeOptions::default()).unwrap();
        assert_eq!(shaped.article.unwrap().cover.as_deref(), Some("https://example.org/field-notes/one.png"), "{title}");
    }
}
