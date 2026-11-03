//! The classes a folder listing emits only when it has entries are the ones
//! `is_listing_class` names, so `moss doctor --surface` can tell a rule for an
//! empty listing from a dead one.

#[path = "support/site.rs"]
mod site;

use std::collections::BTreeSet;

use moss_build::build::folder_embed::listing_markup::is_listing_name;
use site::Site;

fn classes_of_lab(style: &str, event_start: &str) -> BTreeSet<String> {
    let event = format!("---\ntitle: Spring fair\nstart: {event_start}\n---\n\nBody.\n");
    let lab = format!("---\ntitle: Lab\n---\n\n![[/events/|when:upcoming,limit:4,style:{style}]]\n");
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("events/index.md", "---\ntitle: Events\nchildren: false\n---\n"),
        ("events/fair.md", &event),
        ("lab.md", &lab),
    ]);
    let page = scraper::Html::parse_document(&site.html("lab/index.html"));
    let mut found = BTreeSet::new();
    for el in page.select(&scraper::Selector::parse("*").unwrap()) {
        let el = el.value();
        found.extend(el.classes().map(|c| format!(".{c}")));
        found.extend(el.id().map(|i| format!("#{i}")));
        found.extend(el.attrs().map(|(k, _)| format!("[{k}")));
    }
    found
}

#[test]
fn every_class_an_upcoming_listing_adds_is_a_listing_class() {
    for style in ["summary", "list", "grid", "minimal"] {
        let with_entries = classes_of_lab(style, "2099-03-01 19:30");
        let empty = classes_of_lab(style, "2001-03-01 19:30");
        let added: Vec<&String> = with_entries.difference(&empty).collect();
        assert!(added.iter().any(|c| c.as_str() == ".moss-cards-container"), "{style}: {added:?}");
        let missed: Vec<&&String> = added.iter().filter(|n| !is_listing_name(n.chars().next().unwrap(), &n[1..])).collect();
        assert!(missed.is_empty(), "{style}: names only a non-empty listing emits, not named by is_listing_name: {missed:?}");
    }
}
