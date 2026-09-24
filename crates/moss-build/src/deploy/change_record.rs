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

use crate::build::manifest::change_set::{ChangeSet, PageVerb};
use crate::build::manifest::live_baseline::LiveEntry;
use crate::build::scan::article_map::ArticleMap;

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
    }
}

/// The combined page-row cap the receipt renders (task 4-5,
/// `publish-receipt.ts`'s `MAX_PAGE_ROWS`) — named here too because the
/// post-landing verification burst (task 4-6) must probe exactly the rows
/// the receipt shows, never more.
pub const MAX_PAGE_ROWS: usize = 3;

/// Group `records` by kind — Added, Moved, Removed, in that fixed order
/// (design §5 "Anatomy"; mirrors `publish-receipt.ts`'s `orderedPageRows`)
/// — preserving each kind's own path-sorted order, then keep only the first
/// [`MAX_PAGE_ROWS`]. Pure, so both the receipt's own renderer and the
/// verification burst read the identical selection without either
/// re-deriving it differently.
pub fn capped_page_rows(records: &[PageChangeRecord]) -> Vec<&PageChangeRecord> {
    const ORDER: [PageChangeKind; 3] =
        [PageChangeKind::Added, PageChangeKind::Moved, PageChangeKind::Removed];
    ORDER
        .iter()
        .flat_map(|kind| records.iter().filter(move |r| r.kind == *kind))
        .take(MAX_PAGE_ROWS)
        .collect()
}

#[cfg(test)]
#[path = "change_record_tests.rs"]
mod tests;
