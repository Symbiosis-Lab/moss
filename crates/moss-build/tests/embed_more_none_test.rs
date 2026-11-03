//! A capped listing embed draws a "More" link; `more:none` turns it off for a
//! page that writes its own link under the listing.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn home_with(embed: &str) -> String {
    home_with_pages(embed, &[])
}

fn home_with_pages(embed: &str, extra: &[(&str, &str)]) -> String {
    let home = format!("---\ntitle: Example Hall\n---\n\n{embed}\n");
    let mut files = vec![
        ("index.md", home.as_str()),
        ("shows/index.md", "---\ntitle: Shows\n---\n\nAll shows.\n"),
        ("shows/one.md", "---\ntitle: One\ndate: 2026-01-01\n---\n\nBody.\n"),
        ("shows/two.md", "---\ntitle: Two\ndate: 2026-02-01\n---\n\nBody.\n"),
        ("shows/three.md", "---\ntitle: Three\ndate: 2026-03-01\n---\n\nBody.\n"),
    ];
    files.extend_from_slice(extra);
    let site = Site::new(&files);
    site.html("index.html")
}

#[test]
fn a_capped_embed_draws_a_more_link_by_default() {
    let html = home_with("![[shows/|limit:2]]");
    assert_eq!(html.matches("moss-embed-more").count(), 1, "{html}");
}

#[test]
fn more_none_is_a_word_not_a_page_named_none() {
    let html = home_with_pages(
        "![[shows/|limit:2,more:none]]",
        &[("none.md", "---\ntitle: None\n---\n\nA page.\n")],
    );
    assert!(!html.contains("moss-embed-more"), "{html}");
}
