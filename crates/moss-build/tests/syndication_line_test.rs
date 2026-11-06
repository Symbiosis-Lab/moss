//! A page's `syndicated:` copies are listed as a quiet "Also on" line at the
//! end of the article.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn page(front: &str) -> String {
    format!("---\ntitle: Harbour log\n{front}---\n\nThe boats came in.\n")
}

#[test]
fn three_copies_are_linked_in_order_with_syndication_markup() {
    let post = page("syndicated:\n  - https://bsky.app/profile/ana.example/post/3k\n  - https://social.example/@ana/112233\n  - https://matters.town/a/abc123\n");
    let site = Site::new(&[("index.md", "---\ntitle: Home\n---\n\nHi.\n"), ("log.md", &post)]);
    let html = site.html("log/index.html");
    let line = &html[html.find("class=\"moss-syndication\"").expect("line missing")..];
    let line = &line[..line.find("</p>").unwrap()];
    assert!(line.contains("Also on"), "{line}");
    let at = |needle: &str| line.find(needle).unwrap_or_else(|| panic!("{needle} missing in {line}"));
    assert!(at(">Bluesky</a>") < at(">Mastodon</a>") && at(">Mastodon</a>") < at(">Matters</a>"), "{line}");
    assert_eq!(line.matches("rel=\"syndication\" class=\"u-syndication\"").count(), 3, "{line}");
    let main = &html[html.find("<article").unwrap()..html.find("</article>").unwrap()];
    assert!(main.contains("moss-syndication"), "line sits inside the article");
}

#[test]
fn chinese_pages_use_the_chinese_label() {
    let site = Site::in_language(
        "zh-hans",
        "",
        &[("index.md", "---\ntitle: 首页\n---\n\n你好。\n"), ("log.md", &page("syndicated:\n  - https://matters.town/a/abc123\n"))],
    );
    assert!(site.html("log/index.html").contains("也发在"));
}

#[test]
fn no_line_without_the_field_and_non_http_entries_are_skipped() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("plain.md", &page("")),
        ("odd.md", &page("syndicated:\n  - javascript:alert(1)\n  - https://matters.town/a/abc123\n")),
    ]);
    assert!(!site.html("plain/index.html").contains("moss-syndication"));
    let odd = site.html("odd/index.html");
    assert!(odd.contains(">Matters</a>") && !odd.contains("javascript:alert"), "{odd}");
    assert!(!site.html("index.html").contains("moss-syndication"));
}

/// A folder's own page draws no line, even in the article layout.
#[test]
fn a_folder_page_draws_no_line_even_as_an_article() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("log/index.md", &page("layout: article\nsyndicated:\n  - https://matters.town/a/abc123\n")),
        ("log/day.md", &page("")),
    ]);
    assert!(!site.html("log/index.html").contains("moss-syndication"));
}
