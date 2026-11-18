//! Before a publish, moss names the pages the author wrote that would go
//! offline, and nothing else: generated files and the folder's pictures,
//! stylesheets and downloads are not pages a person hands out.

use crate::site;

use moss_build::build::manifest::change_set::{PublishedSnapshot, RemovalReason, RemovedAddress};
use site::Site;
use std::collections::HashMap;

const PAGES: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: River Notes\n---\n\nThe water came up past the second step.\n"),
    ("post.md", "---\ntitle: Flood\nurl: /flood/\n---\n\nDay three.\n"),
];

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Record a last publish that served `files`, then rebuild against it.
fn after_publish_of(site: &Site, snapshot: PublishedSnapshot) -> Vec<RemovedAddress> {
    let mut snapshot = PublishedSnapshot { target: "moss:test".into(), published_at: "2026-11-01T00:00:00Z".into(), ..snapshot };
    snapshot.files.insert("index.html".into(), "h".into());
    moss_build::build::manifest::published_record::save(&moss_build::moss_paths::MossPaths::new(&site.root), &snapshot).unwrap();
    site.rebuild();
    moss_build::system::build_records::records().removed_addresses(&site.root.to_string_lossy()).unwrap()
}

fn paths(removed: &[RemovedAddress]) -> Vec<&str> {
    removed.iter().map(|r| r.path.as_str()).collect()
}

/// A moved page, a deleted picture and two generated files go offline
/// together: only the page is named, and only the page holds the publish.
#[test]
fn only_the_page_is_named_when_a_page_a_picture_and_generated_files_go() {
    let site = Site::with_config("domain = \"example.com\"\n", PAGES);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("old/index.html", "h"), ("art/pic.png", "h"), ("llms.txt", "h"), ("legacy-feed.xml", "h")]),
        source_to_output: map(&[("post.md", "old/index.html")]),
        asset_source_to_output: Some(map(&[("art/pic.png", "art/pic.png")])),
        ..Default::default()
    });
    assert_eq!(paths(&removed), ["old/index.html"], "{removed:?}");
    assert_eq!(removed[0].moved_to.as_deref(), Some("flood/index.html"));
    let refusal = moss_build::deploy::refuse_publish(&site.root.to_string_lossy()).expect_err("a moved page holds the publish");
    assert!(refusal.contains("/old/") && refusal.contains("1 page would go offline"), "{refusal}");
    for other in ["pic.png", "llms.txt", "legacy-feed.xml"] {
        assert!(!refusal.contains(other), "{other} named: {refusal}");
    }
}

#[test]
fn a_deleted_picture_alone_raises_no_notice() {
    let site = Site::new(PAGES);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("art/pic.png", "h"), ("art/pic.webp", "h")]),
        asset_source_to_output: Some(map(&[("art/pic.png", "art/pic.png")])),
        ..Default::default()
    });
    assert_eq!(removed, vec![]);
    assert!(moss_build::deploy::refuse_publish(&site.root.to_string_lossy()).is_ok());
}

/// An author's HTML page and a notebook's viewer page are pages they wrote,
/// though the record files their sources with the folder's other files.
#[test]
fn an_html_page_and_a_notebook_page_are_named_when_their_sources_go() {
    let site = Site::new(PAGES);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("scales.html", "h"), ("nb/analysis.ipynb", "h"), ("nb/analysis.html", "h")]),
        asset_source_to_output: Some(map(&[("scales.html", "scales.html"), ("nb/analysis.ipynb", "nb/analysis.ipynb")])),
        ..Default::default()
    });
    let named: Vec<_> = removed.iter().map(|r| (r.path.as_str(), r.reason, r.source.as_deref())).collect();
    assert_eq!(
        named,
        [
            ("nb/analysis.html", RemovalReason::AuthorRemoved, Some("nb/analysis.ipynb")),
            ("scales.html", RemovalReason::AuthorRemoved, Some("scales.html")),
        ]
    );
}

/// An old address of a page that the last publish kept answering, and this
/// build no longer serves while the page is still there, is that page going
/// offline at its old address.
#[test]
fn a_lost_old_address_of_a_page_still_served_holds_the_publish() {
    let site = Site::new(PAGES);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("flood/index.html", "h"), ("before/index.html", "stub")]),
        source_to_output: map(&[("post.md", "flood/index.html")]),
        page_stubs: map(&[("before/index.html", "flood/index.html")]),
        ..Default::default()
    });
    assert_eq!(paths(&removed), ["before/index.html"], "{removed:?}");
    let refusal = moss_build::deploy::refuse_publish(&site.root.to_string_lossy()).expect_err("the old link breaks");
    assert!(refusal.contains("/before/ now lives at /flood/"), "{refusal}");
}

/// The page an old address forwarded to moved again: the old address is
/// named moved to where the page is now.
#[test]
fn a_lost_old_address_follows_its_page_to_where_it_is_now() {
    let site = Site::new(PAGES);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("older/index.html", "h"), ("before/index.html", "stub")]),
        source_to_output: map(&[("post.md", "older/index.html")]),
        page_stubs: map(&[("before/index.html", "older/index.html")]),
        ..Default::default()
    });
    let moved: Vec<_> = removed.iter().map(|r| (r.path.as_str(), r.moved_to.as_deref())).collect();
    assert_eq!(moved, [("before/index.html", Some("flood/index.html")), ("older/index.html", Some("flood/index.html"))]);
}

/// A members-only file served to everyone is a privacy warning, not a page
/// going offline, so it still holds the publish whatever it is.
#[test]
fn a_members_file_opened_to_everyone_still_holds_the_publish() {
    let site = Site::new(&[PAGES[0], ("data/rows.csv", "a,b\n")]);
    let removed = after_publish_of(&site, PublishedSnapshot {
        files: map(&[("_gated/data/rows.csv", "h")]),
        ..Default::default()
    });
    assert_eq!(removed.iter().map(|r| (r.path.as_str(), r.reason)).collect::<Vec<_>>(), [("data/rows.csv", RemovalReason::Opened)]);
    assert!(moss_build::deploy::refuse_publish(&site.root.to_string_lossy()).is_err());
}
