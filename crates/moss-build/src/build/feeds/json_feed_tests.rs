use super::*;
use moss_core::PageKind;

fn generate_json_feed(documents: &[ParsedDocument], site_title: &str, site_url: &str, site_lang: &str, dir_overrides: &HashMap<String, String>) -> String {
    json_feed(&feed_items(documents, site_url), site_title, site_url, site_lang, dir_overrides)
}

const SITE: &str = "https://example.org";

fn article(title: &str, date: &str, url_path: &str, html: &str) -> ParsedDocument {
    ParsedDocument {
        title: title.to_string(),
        label: title.to_string(),
        html_content: html.to_string(),
        url_path: url_path.to_string(),
        date: Some(date.to_string()),
        kind: PageKind::Article,
        ..Default::default()
    }
}

fn feed(documents: &[ParsedDocument]) -> Value {
    serde_json::from_str(&generate_json_feed(documents, "A site", SITE, "en", &Default::default())).expect("valid JSON")
}

#[test]
fn a_feed_names_its_version_and_addresses() {
    let f = feed(&[]);
    assert_eq!(f["version"], "https://jsonfeed.org/version/1.1");
    assert_eq!(f["home_page_url"], "https://example.org/");
    assert_eq!(f["feed_url"], "https://example.org/feed.json");
    assert_eq!(f["items"].as_array().unwrap().len(), 0);
}

#[test]
fn an_item_is_keyed_by_the_note_id_not_the_address() {
    let mut a = article("Spring", "2026-03-01", "notes/spring/index.html", "<p>Hi</p>");
    a.uid = Some("note-7".into());
    let f = feed(&[a]);
    let item = &f["items"][0];
    assert_eq!(item["id"], "note-7");
    assert_eq!(item["url"], "https://example.org/notes/spring/index.html");
    assert_eq!(item["date_published"], "2026-03-01T00:00:00Z");
}

#[test]
fn a_derived_note_id_gives_way_to_the_address() {
    let mut a = article("Spring", "2026-03-01", "notes/spring/index.html", "<p>Hi</p>");
    a.uid = Some("3f2a9c1b".into());
    a.uid_derived = true;
    assert_eq!(feed(&[a])["items"][0]["id"], "https://example.org/notes/spring/index.html");
}

#[test]
fn an_item_carries_what_rss_has_no_element_for() {
    let mut a = article("Spring", "2026-03-01", "notes/spring/index.html", "<p>Hi</p>");
    a.description = Some("The first warm day.".into());
    a.cover = Some("notes/spring/bank.webp".into());
    a.tags = Some(vec!["flood".into()]);
    let item = feed(&[a])["items"][0].clone();
    assert_eq!(item["summary"], "The first warm day.");
    assert_eq!(item["image"], "https://example.org/notes/spring/bank.webp");
    assert_eq!(item["tags"][0], "flood");
    assert_eq!(item["language"], "en");
}

#[test]
fn links_and_pictures_are_absolute_and_carry_no_tracking() {
    let html = r##"<p><a href="../other/">Other</a> <a href="https://elsewhere.net/x">Out</a> <a href="#fn-1">1</a></p><picture><source srcset="bank.avif"><img src="bank.webp" srcset="bank-2x.webp 2x" alt="Bank"></picture>"##;
    let item = feed(&[article("Spring", "2026-03-01", "notes/spring/index.html", html)])["items"][0].clone();
    let content = item["content_html"].as_str().unwrap();
    assert!(content.contains(r#"href="https://example.org/notes/other/""#), "{content}");
    assert!(content.contains(r#"href="https://elsewhere.net/x""#), "{content}");
    assert!(content.contains(r##"href="#"##), "a fragment link stays within the item: {content}");
    assert!(content.contains(r#"src="https://example.org/notes/spring/bank.webp""#), "{content}");
    assert!(!content.contains("srcset") && !content.contains("<source"), "{content}");
    assert!(!content.contains("utm_"), "{content}");
}

#[test]
fn the_feed_lists_the_same_articles_as_rss_in_the_same_order() {
    let docs = vec![
        article("Old", "2025-01-01", "old/index.html", "<p>a</p>"),
        article("New", "2026-01-01", "new/index.html", "<p>b</p>"),
        ParsedDocument { date: None, ..article("Undated", "", "about/index.html", "<p>c</p>") },
    ];
    let titles: Vec<_> = feed(&docs)["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap().to_string()).collect();
    assert_eq!(titles, ["New", "Old"]);
}
