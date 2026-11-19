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
