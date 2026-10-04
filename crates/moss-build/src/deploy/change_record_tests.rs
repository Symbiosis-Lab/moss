use super::*;
use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
use crate::build::manifest::change_set::ChangedPage;
use crate::build::scan::article_map::ArticleInfo;
use std::collections::HashMap;

fn article_info(source_path: &str, url: &str, title: &str) -> ArticleInfo {
    ArticleInfo {
        source_path: source_path.to_string(),
        title: title.to_string(),
        content: String::new(),
        html_content: None,
        frontmatter: HashMap::new(),
        url_path: url.to_string(),
        date: None,
        tags: Vec::new(),
        uid: None,
    }
}

fn article_map(entries: &[(&str, &str, &str)]) -> ArticleMap {
    let mut articles = HashMap::new();
    for (url, source_path, title) in entries {
        articles.insert(url.to_string(), article_info(source_path, url, title));
    }
    ArticleMap { articles, ..Default::default() }
}

fn live_entry(url: &str, source_path: &str, title: &str) -> LiveEntry {
    LiveEntry {
        uid: format!("uid-{source_path}"),
        url: url.to_string(),
        source_path: source_path.to_string(),
        title: title.to_string(),
    }
}

fn added(source_path: &str) -> ChangedPage {
    ChangedPage { source_path: source_path.to_string(), verb: PageVerb::Added }
}

fn deleted(source_path: &str) -> ChangedPage {
    ChangedPage { source_path: source_path.to_string(), verb: PageVerb::Deleted }
}

fn renames(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(old, new)| (old.to_string(), new.to_string())).collect()
}

/// A page renamed AND, per the change set's own hash-only view, both
/// "added" (new source path) and "deleted" (old source path gone) — must
/// still collapse to exactly one Moved row, never three.
#[test]
fn a_renamed_page_seen_as_added_and_deleted_by_the_change_set_is_one_moved_row() {
    let change_set = ChangeSet {
        classified: true,
        added: 1,
        deleted: 1,
        pages: vec![added("new.md"), deleted("old.md")],
        ..Default::default()
    };
    let renames = renames(&[("old-page", "new-page")]);
    let current = article_map(&[("new-page", "new.md", "New Title")]);
    let prev = vec![live_entry("old-page", "old.md", "Old Title")];

    let summary = build_page_change_records(&change_set, &renames, &current, &prev, &[]);

    assert_eq!(summary.records.len(), 1, "{:?}", summary.records);
    let row = &summary.records[0];
    assert_eq!(row.kind, PageChangeKind::Moved);
    assert_eq!(row.path, "new-page");
    assert_eq!(row.old_path.as_deref(), Some("old-page"));
    assert_eq!(row.title, "New Title");
    assert_eq!((summary.pages_added, summary.pages_moved, summary.pages_removed), (0, 1, 0));
}

#[test]
fn a_plain_added_page_with_no_rename_involved() {
    let change_set = ChangeSet {
        classified: true,
        added: 1,
        pages: vec![added("fresh.md")],
        ..Default::default()
    };
    let current = article_map(&[("fresh", "fresh.md", "Fresh Page")]);

    let summary = build_page_change_records(&change_set, &HashMap::new(), &current, &[], &[]);

    assert_eq!(summary.records.len(), 1);
    let row = &summary.records[0];
    assert_eq!(row.kind, PageChangeKind::Added);
    assert_eq!(row.path, "fresh");
    assert_eq!(row.title, "Fresh Page");
    assert_eq!(row.old_path, None);
    assert_eq!((summary.pages_added, summary.pages_moved, summary.pages_removed), (1, 0, 0));
}

/// A Removed row's title comes from `prev_triples` (task 4-1's field), not
/// from anything in the current article map — the page is gone from it.
#[test]
fn a_plain_removed_page_takes_its_title_from_prev_triples() {
    let change_set = ChangeSet {
        classified: true,
        deleted: 1,
        pages: vec![deleted("gone.md")],
        ..Default::default()
    };
    let current = article_map(&[]);
    let prev = vec![live_entry("gone-page", "gone.md", "Gone Page")];

    let summary = build_page_change_records(&change_set, &HashMap::new(), &current, &prev, &[]);

    assert_eq!(summary.records.len(), 1);
    let row = &summary.records[0];
    assert_eq!(row.kind, PageChangeKind::Removed);
    assert_eq!(row.path, "gone-page");
    assert_eq!(row.title, "Gone Page");
    assert_eq!(row.old_path, None);
    assert_eq!((summary.pages_added, summary.pages_moved, summary.pages_removed), (0, 0, 1));
}

#[test]
fn pages_updated_counts_edited_plus_restyled_and_never_becomes_a_row() {
    let change_set = ChangeSet { classified: true, edited: 2, restyled: 3, ..Default::default() };

    let summary = build_page_change_records(&change_set, &HashMap::new(), &ArticleMap::default(), &[], &[]);

    assert_eq!(summary.pages_updated, 5);
    assert!(summary.records.is_empty());
}

#[test]
fn records_are_sorted_by_path() {
    let change_set = ChangeSet {
        classified: true,
        added: 2,
        pages: vec![added("b.md"), added("a.md")],
        ..Default::default()
    };
    let current = article_map(&[("b-page", "b.md", "B"), ("a-page", "a.md", "A")]);

    let summary = build_page_change_records(&change_set, &HashMap::new(), &current, &[], &[]);

    let paths: Vec<&str> = summary.records.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, vec!["a-page", "b-page"]);
}

/// task 4-6's verification burst must probe exactly the rows the receipt
/// shows: grouped by kind (Added, Moved, Removed), capped at
/// `MAX_PAGE_ROWS`, never re-sorted within a kind — mirrors
/// `publish-receipt.ts`'s `orderedPageRows` + `MAX_PAGE_ROWS` slice.
#[test]
fn capped_page_rows_groups_by_kind_and_caps_at_three() {
    let records = vec![
        PageChangeRecord { kind: PageChangeKind::Removed, path: "r1".into(), title: "R1".into(), old_path: None },
        PageChangeRecord { kind: PageChangeKind::Added, path: "a1".into(), title: "A1".into(), old_path: None },
        PageChangeRecord { kind: PageChangeKind::Added, path: "a2".into(), title: "A2".into(), old_path: None },
        PageChangeRecord {
            kind: PageChangeKind::Moved,
            path: "m1".into(),
            title: "M1".into(),
            old_path: Some("m1-old".into()),
        },
    ];

    let capped = capped_page_rows(&records);

    let paths: Vec<&str> = capped.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, vec!["a1", "a2", "m1"], "Added before Moved before Removed, capped at 3");
}

fn gone(path: &str, reason: RemovalReason, source: Option<&str>) -> RemovedAddress {
    RemovedAddress { path: path.into(), reason, moved_to: None, source: source.map(Into::into) }
}

fn summarize(change_set: &ChangeSet, removed: &[RemovedAddress]) -> PageChangeSummary {
    build_page_change_records(change_set, &HashMap::new(), &ArticleMap::default(), &[], removed)
}

fn classified() -> ChangeSet {
    ChangeSet { classified: true, ..Default::default() }
}

#[test]
fn a_deleted_folder_file_is_one_removed_address_and_no_page_row() {
    let summary = summarize(&classified(), &[gone("docs/guide.pdf", RemovalReason::AuthorRemoved, Some("docs/guide.pdf"))]);

    assert!(summary.records.is_empty(), "{:?}", summary.records);
    assert_eq!(
        summary.removed_addresses,
        [RemovedAddressRecord { path: "/docs/guide.pdf".into(), reason: RemovalReason::AuthorRemoved, moved_to: None }]
    );
}

/// A removed page keeps its page record; the address list holds its output
/// too, and it must not appear a second time.
#[test]
fn a_removed_page_has_its_page_record_and_no_address_record() {
    let change_set = ChangeSet { classified: true, deleted: 1, pages: vec![deleted("gone.md")], ..Default::default() };
    let prev = vec![live_entry("gone/", "gone.md", "Gone Page")];

    let summary = build_page_change_records(
        &change_set,
        &HashMap::new(),
        &ArticleMap::default(),
        &prev,
        &[gone("gone/index.html", RemovalReason::AuthorRemoved, Some("gone.md"))],
    );

    assert_eq!(summary.records.len(), 1, "{:?}", summary.records);
    assert!(summary.removed_addresses.is_empty());
}

/// A renamed page's old address is its Moved row's `old_path`, so the list
/// leaves it out.
#[test]
fn a_renamed_pages_old_address_is_named_by_its_moved_row_only() {
    let renames: HashMap<String, String> = [("old-page/".to_string(), "new-page/".to_string())].into();
    let current = article_map(&[("new-page/", "new.md", "New")]);

    let summary = build_page_change_records(
        &classified(),
        &renames,
        &current,
        &[],
        &[gone("old-page/index.html", RemovalReason::Unexplained, Some("new.md"))],
    );

    assert_eq!(summary.records.len(), 1);
    assert!(summary.removed_addresses.is_empty());
}

/// A page that moved with no redirect (and no uid rename to give it a Moved
/// row) and whose loss was accepted: the old address is named once, in the
/// list, with where it went. No page row exists to name it.
#[test]
fn a_moved_page_with_no_stub_is_named_once_in_the_list() {
    let removed = [RemovedAddress {
        path: "old/index.html".into(),
        reason: RemovalReason::Unexplained,
        moved_to: Some("new/index.html".into()),
        source: Some("moved.md".into()),
    }];
    let change_set = ChangeSet { classified: true, edited: 1, pages: vec![ChangedPage { source_path: "moved.md".into(), verb: PageVerb::Edited }], ..Default::default() };

    let summary = summarize(&change_set, &removed);

    assert!(summary.records.is_empty());
    assert_eq!(
        summary.removed_addresses,
        [RemovedAddressRecord { path: "/old/".into(), reason: RemovalReason::Unexplained, moved_to: Some("/new/".into()) }]
    );
}

#[test]
fn the_root_index_prints_as_a_slash() {
    let summary = summarize(&classified(), &[gone("index.html", RemovalReason::Unexplained, None)]);
    assert_eq!(summary.removed_addresses[0].path, "/");
}

#[test]
fn nothing_removed_names_no_address() {
    assert!(summarize(&classified(), &[]).removed_addresses.is_empty());
}

#[test]
fn the_summary_serializes_the_list_beside_the_page_records() {
    let summary = summarize(&classified(), &[gone("x.pdf", RemovalReason::AuthorRemoved, None)]);
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        serde_json::json!({
            "pages_added": 0, "pages_moved": 0, "pages_removed": 0, "pages_updated": 0,
            "records": [],
            "removed_addresses": [{"path": "/x.pdf", "reason": "author_removed"}]
        })
    );
}
