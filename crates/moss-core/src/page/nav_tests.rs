use std::collections::HashMap;

use super::*;
use crate::page::site_page::{site_pages, PageFacts};
use crate::page::SiteBundle;

fn pages(texts: &[(&str, &str)]) -> Vec<PageFacts> {
    let texts: HashMap<String, String> = texts.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut files: Vec<String> = texts.keys().cloned().collect();
    files.sort();
    site_pages(&texts, &SiteBundle { files, lang: "en".into(), ..Default::default() }, "site").pages
}

fn page<'a>(pages: &'a [PageFacts], url: &str) -> &'a PageFacts {
    pages.iter().find(|p| p.url_path == url).unwrap_or_else(|| panic!("no page at {url}"))
}

fn trail(segments: &[BreadcrumbSegment]) -> Vec<(&str, &str)> {
    segments.iter().map(|s| (s.title.as_str(), s.url.as_str())).collect()
}

#[test]
fn a_site_without_nav_items_shows_the_trail_through_each_folder() {
    let site = pages(&[
        ("index.md", "# Home\n"),
        ("guide/index.md", "---\ntitle: The Guide\n---\n"),
        ("guide/part-two/deep.md", "---\ntitle: Deep\n---\n"),
    ]);
    let deep = page(&site, "guide/part-two/deep/index.html");
    let segments = breadcrumb_segments(deep, &site, "Site", true, false).unwrap();
    assert_eq!(trail(&segments), [("Site", "/"), ("The Guide", "/guide/"), ("part two", "/guide/part-two/"), ("Deep", "")]);
    assert!(breadcrumb_segments(page(&site, "index.html"), &site, "Site", true, false).is_none(), "a home page has none");
}

#[test]
fn a_top_level_page_is_a_nav_item_and_turns_the_trail_off() {
    let site = pages(&[
        ("index.md", "# Home\n"),
        ("about.md", "---\nnav_label: Who\n---\n# About us\n"),
        ("guide/index.md", "# Guide\n"),
        ("guide/one.md", "# One\n"),
    ]);
    assert!(has_nav_items(&site, "", true));
    assert!(breadcrumb_segments(page(&site, "guide/one/index.html"), &site, "Site", true, false).is_none());
    let bar = nav_bar_pages(&site, Language::En, "", true);
    let html: Vec<String> = nav_items(&bar, Some("about/index.html"), false, Language::En).into_iter().map(|i| i.html).collect();
    assert_eq!(html, [r#"<a href="/about/" class="active" aria-current="page" data-source-file="about.md" data-source-fm="nav">Who</a>"#]);
}

#[test]
fn a_translated_home_folder_is_where_the_site_name_links() {
    let site = pages(&[
        ("index.md", "---\nbreadcrumb: true\n---\n# Home\n"),
        ("zh-hans/index.md", "# 首页\n"),
        ("zh-hans/guide/index.md", "---\ntitle: 指南\n---\n"),
        ("zh-hans/guide/one.md", "---\ntitle: 一\n---\n"),
    ]);
    assert!(page(&site, "zh-hans/index.html").translates_home);
    let one = page(&site, "zh-hans/guide/one/index.html");
    let segments = breadcrumb_segments(one, &site, "站", true, false).unwrap();
    assert_eq!(trail(&segments), [("站", "/zh-hans/"), ("指南", "/zh-hans/guide/"), ("一", "")]);
}

#[test]
fn a_trail_of_three_crumbs_folds_its_middle() {
    let segment = |title: &str, url: &str| BreadcrumbSegment { title: title.into(), url: url.into(), is_current: url.is_empty() };
    let markup = TrailMarkup { home_path: "/", logo_html: "", current_visible: false, source_fm: false, lang: Language::En };
    let short = trail_html(&[segment("Site", "/"), segment("Guide", "/guide/"), segment("One", "")], &markup);
    assert!(!short.contains("moss-breadcrumb-more"), "{short}");
    let long = trail_html(&[segment("Site", "/"), segment("Guide", "/guide/"), segment("Part", "/guide/part/"), segment("One", "")], &markup);
    assert!(long.contains("moss-breadcrumb-more") && long.ends_with(r#"<div class="moss-breadcrumb-menu" hidden></div>"#), "{long}");
}
