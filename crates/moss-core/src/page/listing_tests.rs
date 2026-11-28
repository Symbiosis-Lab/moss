use std::collections::HashMap;

use super::{keep, select, ListingScope, ListingSite};
use crate::page::site_page::{site_pages, PageFacts};
use crate::page::SiteBundle;

const NOW: &str = "2026-11-26T12:00";

fn pages(texts: &[(&str, &str)]) -> Vec<PageFacts> {
    let texts: HashMap<String, String> = texts.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut files: Vec<String> = texts.keys().cloned().collect();
    files.sort();
    site_pages(&texts, &SiteBundle { files, lang: "en".into(), ..Default::default() }, "site").pages
}

fn sources<'a>(picked: &[&'a PageFacts]) -> Vec<&'a str> {
    picked.iter().map(|p| p.source_path.as_deref().unwrap_or(&p.url_path)).collect()
}

#[test]
fn a_when_filter_keeps_events_by_whether_they_have_ended() {
    let site = pages(&[
        ("events/index.md", "# Events\n"),
        ("events/soon.md", "---\nstart: 2030-01-01\n---\n# Soon\n"),
        ("events/over.md", "---\nstart: 2020-01-01\n---\n# Over\n"),
        ("events/undated.md", "---\nstart:\n---\n# Undated\n"),
        ("events/notes.md", "# Notes\n"),
    ]);
    let picked = select(&ListingScope { folder: "events", ..Default::default() }, &site, ListingSite::default());
    let kept = |when| {
        let mut got = sources(&keep(picked.clone(), None, false, when, NOW, |_| false));
        got.sort();
        got
    };
    assert_eq!(kept(Some("upcoming")), ["events/soon.md"]);
    // An empty `start:` is an event waiting for its date: not yet past.
    assert_eq!(kept(Some("past")), ["events/notes.md", "events/over.md"]);
    assert_eq!(kept(None), ["events/notes.md", "events/over.md", "events/soon.md", "events/undated.md"]);
}

#[test]
fn a_listing_across_editions_shows_a_translated_page_once_in_the_hosts_edition() {
    let mut site = pages(&[("notes/index.md", "# Notes\n"), ("notes/a.md", "# A\n"), ("notes/a-zh.md", "# A\n"), ("notes/b.md", "# B\n")]);
    let url = |site: &[PageFacts], src: &str| site.iter().find(|p| p.source_path.as_deref() == Some(src)).unwrap().url_path.clone();
    let (a, a_zh) = (url(&site, "notes/a.md"), url(&site, "notes/a-zh.md"));
    for page in &mut site {
        if page.url_path == a {
            page.translations = vec![a_zh.clone()];
        } else if page.url_path == a_zh {
            page.edition = "zh".into();
            page.translations = vec![a.clone()];
        }
    }
    let listed = |edition| {
        let mut got = sources(&select(&ListingScope { folder: "notes", edition, ..Default::default() }, &site, ListingSite::default()));
        got.sort();
        got
    };
    assert_eq!(listed("zh"), ["notes/a-zh.md", "notes/b.md"]);
    assert_eq!(listed(""), ["notes/a.md", "notes/b.md"]);
}
