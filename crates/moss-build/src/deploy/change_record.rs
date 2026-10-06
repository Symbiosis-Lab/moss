//! The Added/Moved/Removed page rows a publish receipt shows, merged from a
//! completion's change set, its detected renames, and the current and
//! previous article maps (design doc 2026-09-10, step 4).
//!
//! Pure: no filesystem, no network. `change_set::classify` and
//! `redirects::detect_renames` already compute the inputs this reads; this
//! module only combines them, once, at publish-completion time.
//!
//! # Why renames are folded first
//!
//! `redirects::detect_renames` and `change_set::classify` see the same page
//! rename as different events. The rename function reports one `old_url ->
//! new_url` pair by uid; the change set — which only compares source-hash
//! presence, not URL — reports the same event as an Added `ChangedPage` for
//! the new source path and a Deleted `ChangedPage` for the one it replaced.
//! Left unreconciled, one rename would render as three rows instead of one
//! Moved row, so renames are folded into Moved rows first and the Added/
//! Deleted loops below skip whichever side a rename already covers.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::build::manifest::change_set::{ChangeSet, PageVerb, RemovalReason, RemovedAddress};
use crate::build::manifest::live_baseline::LiveEntry;
use crate::build::scan::article_map::ArticleMap;
use crate::build::served_path::served_address;

/// What kind of row a page's change renders as in the receipt.
#[derive(Clone, Copy, Debug, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PageChangeKind {
    Added,
    Moved,
    Removed,
}

/// One page row in the receipt's Added/Moved/Removed ring.
#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
pub struct PageChangeRecord {
    pub kind: PageChangeKind,
    /// The page's current URL for Added/Moved; its last-known URL for Removed.
    pub path: String,
    pub title: String,
    /// Moved only: the URL this page served before the rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

/// A public address this publish stopped serving that is not a page row: a
/// file the author deleted, a generated file whose loss was accepted, a page
/// whose address changed with no redirect.
///
/// Page records ([`PageChangeRecord`]) carry paths WITHOUT a leading slash;
/// this list carries served addresses, WITH one, by the one rule the publish
/// gate prints them by.
#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
pub struct RemovedAddressRecord {
    pub path: String,
    pub reason: RemovalReason,
    /// Where the page that lived here is now served, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
}

/// The completion-scoped change record a publish receipt renders.
#[derive(Clone, Debug, Serialize, specta::Type, PartialEq, Default)]
pub struct PageChangeSummary {
    pub pages_added: u32,
    pub pages_moved: u32,
    pub pages_removed: u32,
    /// Edited + Restyled pages: counted, never itemized — the receipt's row
    /// set is Added/Moved/Removed only.
    pub pages_updated: u32,
    pub records: Vec<PageChangeRecord>,
    /// Removed public addresses that no removed or moved page row already
    /// names, each once. Nothing here is a page row.
    pub removed_addresses: Vec<RemovedAddressRecord>,
}

/// Merge a change set, detected renames, and the current/previous article
/// maps into the Added/Moved/Removed rows a publish receipt shows.
///
/// `prev_triples` is the previous publish record's `{uid, url, source_path,
/// title}` entries ([`LiveEntry`]) — the same baseline `renames`
/// was itself computed against, so a Removed row's title is read from here
/// rather than re-derived from anything current.
pub fn build_page_change_records(
    change_set: &ChangeSet,
    renames: &HashMap<String, String>,
    current_map: &ArticleMap,
    prev_triples: &[LiveEntry],
    removed: &[RemovedAddress],
) -> PageChangeSummary {
    // Step 1: source_path -> current url, and source_path -> the old live entry.
    let source_to_url: HashMap<&str, &str> = current_map
        .articles
        .iter()
        .map(|(url, info)| (info.source_path.as_str(), url.as_str()))
        .collect();
    let prev_by_source: HashMap<&str, &LiveEntry> =
        prev_triples.iter().map(|e| (e.source_path.as_str(), e)).collect();

    let mut records: Vec<PageChangeRecord> = Vec::new();
    let mut consumed_old: HashSet<&str> = HashSet::new();
    let mut consumed_new: HashSet<&str> = HashSet::new();

    // Step 2: renames become Moved rows first.
    for (old_url, new_url) in renames {
        // A renamed page's new_url with no current_map.articles entry falls
        // back to an empty title rather than unwrapping.
        let title = current_map
            .articles
            .get(new_url)
            .map(|info| info.title.clone())
            .unwrap_or_default();
        records.push(PageChangeRecord {
            kind: PageChangeKind::Moved,
            path: new_url.clone(),
            title,
            old_path: Some(old_url.clone()),
        });
        consumed_old.insert(old_url.as_str());
        consumed_new.insert(new_url.as_str());
    }

    // Step 3: Added pages whose url isn't already a rename's destination.
    for page in &change_set.pages {
        if page.verb != PageVerb::Added {
            continue;
        }
        let Some(&url) = source_to_url.get(page.source_path.as_str()) else {
            continue;
        };
        if consumed_new.contains(url) {
            continue;
        }
        records.push(PageChangeRecord {
            kind: PageChangeKind::Added,
            path: url.to_string(),
            title: current_map.articles[url].title.clone(),
            old_path: None,
        });
    }

    // Step 4: Deleted pages whose old url isn't already a rename's origin.
    for page in &change_set.pages {
        if page.verb != PageVerb::Deleted {
            continue;
        }
        let Some(entry) = prev_by_source.get(page.source_path.as_str()) else {
            continue;
        };
        if consumed_old.contains(entry.url.as_str()) {
            continue;
        }
        records.push(PageChangeRecord {
            kind: PageChangeKind::Removed,
            path: entry.url.clone(),
            title: entry.title.clone(),
            old_path: None,
        });
    }

    // Removed public addresses that are not pages. A removed page has its
    // Removed row and a renamed one its Moved row, so an address one of those
    // names is skipped; no address appears twice.
    let represented: HashSet<String> = records
        .iter()
        .filter(|r| r.kind == PageChangeKind::Removed)
        .map(|r| r.path.as_str())
        .chain(records.iter().filter_map(|r| r.old_path.as_deref()))
        .map(|p| p.trim_start_matches('/').to_string())
        .collect();
    let deleted_pages: HashSet<&str> = change_set
        .pages
        .iter()
        .filter(|p| p.verb == PageVerb::Deleted)
        .map(|p| p.source_path.as_str())
        .collect();
    let removed_addresses: Vec<RemovedAddressRecord> = removed
        .iter()
        .filter(|a| {
            let pretty = crate::build::scan::article_map::to_pretty_url(&a.path);
            !represented.contains(&pretty)
                && !a.source.as_deref().is_some_and(|src| deleted_pages.contains(src))
        })
        .map(|a| RemovedAddressRecord {
            path: served_address(&a.path),
            reason: a.reason,
            moved_to: a.moved_to.as_deref().map(served_address),
        })
        .collect();

    // Step 6: one ring, sorted by path.
    records.sort_by(|a, b| a.path.cmp(&b.path));

    // Step 7: counts read off the sorted list; pages_updated is step 5 —
    // Edited/Restyled pages never become rows, only this count.
    let pages_added = records.iter().filter(|r| r.kind == PageChangeKind::Added).count() as u32;
    let pages_moved = records.iter().filter(|r| r.kind == PageChangeKind::Moved).count() as u32;
    let pages_removed =
        records.iter().filter(|r| r.kind == PageChangeKind::Removed).count() as u32;

    PageChangeSummary {
        pages_added,
        pages_moved,
        pages_removed,
        pages_updated: change_set.edited + change_set.restyled,
        records,
        removed_addresses,
    }
}

/// How many rows the receipt shows in all — page rows and removed-address
/// rows together. The post-publish check probes exactly the rows shown, never
/// more, and the receipt's page-row renderer applies the same number.
pub const MAX_RECEIPT_ROWS: usize = 3;

/// Group `records` by kind — Added, Moved, Removed, in that fixed order
/// — preserving each kind's own path-sorted order, then keep only the first
/// [`MAX_RECEIPT_ROWS`]. Pure, so the receipt and the post-publish check read
/// the identical selection without either re-deriving it differently.
pub fn capped_page_rows(records: &[PageChangeRecord]) -> Vec<&PageChangeRecord> {
    ordered_page_rows(records).take(MAX_RECEIPT_ROWS).collect()
}

fn ordered_page_rows(records: &[PageChangeRecord]) -> impl Iterator<Item = &PageChangeRecord> {
    const ORDER: [PageChangeKind; 3] =
        [PageChangeKind::Added, PageChangeKind::Moved, PageChangeKind::Removed];
    ORDER.iter().flat_map(move |kind| records.iter().filter(move |r| r.kind == *kind))
}

/// The receipt's rows: the page rows first, the removed addresses filling the
/// slots they leave, and how many rows of either kind did not fit.
pub struct CappedRows<'a> {
    pub pages: Vec<&'a PageChangeRecord>,
    pub addresses: Vec<&'a RemovedAddressRecord>,
    pub hidden: usize,
}

/// The one selection of which rows the receipt shows within
/// [`MAX_RECEIPT_ROWS`]; the check that probes them and the payload that
/// names them both come from here.
pub fn capped_rows<'a>(records: &'a [PageChangeRecord], addresses: &'a [RemovedAddressRecord]) -> CappedRows<'a> {
    let pages = capped_page_rows(records);
    let room = MAX_RECEIPT_ROWS - pages.len();
    CappedRows {
        hidden: records.len() + addresses.len() - pages.len() - addresses.len().min(room),
        addresses: addresses.iter().take(room).collect(),
        pages,
    }
}

#[cfg(test)]
#[path = "change_record_tests.rs"]
mod tests;
