//! A file named by its date stamp is a dated post, untitled when the stamp is
//! all the name says.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn build(files: &[(&str, &str)]) -> Site {
    let mut all = vec![("index.md", "---\ntitle: Harbor Notes\n---\n\nWelcome.\n")];
    all.extend_from_slice(files);
    Site::with_config("url = \"https://notes.example\"\n", &all)
}

fn page(site: &Site, slug: &str) -> scraper::Html {
    scraper::Html::parse_document(&site.html(&format!("notes/{slug}/index.html")))
}

fn text_of(html: &scraper::Html, selector: &str) -> Vec<String> {
    let sel = scraper::Selector::parse(selector).unwrap();
    html.select(&sel).map(|e| e.text().collect::<String>().trim().to_string()).collect()
}

fn meta(html: &scraper::Html, property: &str) -> Option<String> {
    let sel = scraper::Selector::parse(&format!("meta[property=\"{property}\"]")).unwrap();
    html.select(&sel).next().and_then(|e| e.value().attr("content")).map(str::to_string)
}

const ARTICLE_H1: &str = "article h1";

#[test]
fn a_bare_stamp_is_a_dated_untitled_post_labelled_by_its_cjk_opening() {
    let s = build(&[("notes/2026-11-06.md", "今天和朋友在院子里吃了火锅，天气很冷，大家都很开心，一直聊到很晚才散。\n")]);
    let html = page(&s, "2026-11-06");
    assert_eq!(text_of(&html, ARTICLE_H1), Vec::<String>::new());
    assert_eq!(meta(&html, "article:published_time").as_deref(), Some("2026-11-06"));
    assert_eq!(meta(&html, "og:title").as_deref(), Some("今天和朋友在院子里吃了火锅，天气很冷，大…"));
    assert_eq!(text_of(&html, "title"), vec!["今天和朋友在院子里吃了火锅，天气很冷，大… - Harbor Notes"]);
}

#[test]
fn a_bare_stamp_is_labelled_by_its_first_eight_words() {
    let s = build(&[("notes/2026-11-06.md", "Walked along the river this morning and saw three herons\n")]);
    let html = page(&s, "2026-11-06");
    assert_eq!(text_of(&html, ARTICLE_H1), Vec::<String>::new());
    assert_eq!(meta(&html, "og:title").as_deref(), Some("Walked along the river this morning and saw…"));
    assert_eq!(meta(&html, "article:published_time").as_deref(), Some("2026-11-06"));
}

#[test]
fn a_stamp_with_a_time_orders_posts_of_one_day_by_that_time() {
    let s = build(&[
        ("notes/2026-11-06 0900.md", "Apple morning\n"),
        ("notes/2026-11-06 1432.md", "Zebra evening\n"),
    ]);
    let listing = scraper::Html::parse_document(&s.html("notes/index.html"));
    assert_eq!(text_of(&listing, ".moss-card-title"), vec!["Zebra evening", "Apple morning"]);
    assert_eq!(meta(&page(&s, "2026-11-06-1432"), "article:published_time").as_deref(), Some("2026-11-06T14:32"));
}

#[test]
fn a_compact_stamp_is_a_dated_untitled_post() {
    let s = build(&[("notes/202611061432.md", "Three herons by the river\n")]);
    let html = page(&s, "202611061432");
    assert_eq!(text_of(&html, ARTICLE_H1), Vec::<String>::new());
    assert_eq!(meta(&html, "article:published_time").as_deref(), Some("2026-11-06T14:32"));
    assert_eq!(meta(&html, "og:title").as_deref(), Some("Three herons by the river"));
}

#[test]
fn words_after_the_stamp_are_the_title_and_the_stamp_is_the_date() {
    let s = build(&[("notes/2026-11-06 Some words.md", "Body text.\n")]);
    let html = page(&s, "2026-11-06-some-words");
    assert_eq!(text_of(&html, ARTICLE_H1), vec!["Some words"]);
    assert_eq!(meta(&html, "og:title").as_deref(), Some("Some words"));
    assert_eq!(meta(&html, "article:published_time").as_deref(), Some("2026-11-06"));
}

#[test]
fn frontmatter_date_wins_over_the_stamp() {
    let s = build(&[("notes/2026-11-06.md", "---\ndate: 2025-01-02\n---\n\nA post.\n")]);
    assert_eq!(meta(&page(&s, "2026-11-06"), "article:published_time").as_deref(), Some("2025-01-02"));
}

#[test]
fn an_events_start_means_the_stamp_gives_no_date() {
    let s = build(&[("notes/2026-11-06.md", "---\nstart: 2026-12-20 14:00\ntimezone: Asia/Taipei\n---\n\nA gathering.\n")]);
    assert_eq!(meta(&page(&s, "2026-11-06"), "article:published_time"), None);
}

#[test]
fn an_explicit_title_is_kept_on_a_bare_stamp() {
    let s = build(&[("notes/2026-11-06.md", "---\ntitle: Kept title\n---\n\nA post.\n")]);
    let html = page(&s, "2026-11-06");
    assert_eq!(text_of(&html, ARTICLE_H1), vec!["Kept title"]);
    assert_eq!(meta(&html, "og:title").as_deref(), Some("Kept title"));
}

/// A page named by a bare stamp that dates itself (`date:` or `start:`) is not
/// an untitled post: it keeps the heading its name has always given it, as an
/// event file named by its day does.
#[test]
fn a_bare_stamp_with_its_own_date_or_start_keeps_its_heading() {
    for front in ["date: 2026-11-06", "start: 2026-12-20 14:00\ntimezone: Asia/Taipei"] {
        let s = build(&[("notes/2026-11-06.md", &format!("---\n{front}\n---\n\nA gathering in the hall.\n"))]);
        let html = page(&s, "2026-11-06");
        assert_eq!(text_of(&html, ARTICLE_H1), vec!["2026 11 06"], "{front}");
        assert_eq!(meta(&html, "og:title").as_deref(), Some("2026 11 06"), "{front}");
    }
}
