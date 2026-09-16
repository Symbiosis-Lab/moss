//! What the UX design calls a timeline: the list of moments worth showing
//! for one page or for the whole site, and what changed at each. Everything
//! here is a fold over [`super::store::list_records`]'s output — no new I/O,
//! no new fold over paths that [`crate::build::manifest::change_set`]
//! doesn't already know how to do (see [`crate::build::manifest::change_set::diff_hashes`],
//! which both `classify` and this module call).

use std::collections::HashSet;
use std::path::Path;

use crate::build::manifest::change_set::{self, ChangedPage, PageVerb};
use crate::build::manifest::SealedManifest;

use super::record::{self, PublishRecord, Trigger};
use super::store;

/// One row in a timeline — a publish, a manual save, or a pre-restore save.
///
/// `changed`/`added`/`removed` compare this record to the one immediately
/// before it (globally, not per-target — the UX's "moment before" is
/// chronological, not target-scoped); [`page_timeline`] leaves them at zero
/// since a single page's row has no per-record file counts to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub published_at: String,
    pub target: String,
    pub trigger: Trigger,
    pub label: Option<String>,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
}

/// What one version's pages look like next to the site as it is now.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteVersionPages {
    /// Edited, added or deleted since this version, in the vocabulary
    /// `change_set::classify` already uses for the same three verbs.
    /// [`PageVerb::Restyled`] never appears — a flat history record has no
    /// output hash to detect it from, only ever Added/Edited/Deleted.
    pub changed_since: Vec<ChangedPage>,
    pub unchanged: Vec<String>,
}

/// The timeline for one page: a row for every record where the page's hash
/// differs from the previous record's (appearing or disappearing counts as
/// differing), plus every manual and every restore record regardless —
/// those are always worth a stop on any page's timeline, not just the one
/// the author was looking at when they made the save.
pub(crate) fn page_timeline(root: &Path, path: &str) -> Vec<Row> {
    let records = store::list_records(root);
    let mut rows = Vec::new();
    let mut prev_hash: Option<String> = None;
    for (id, record) in &records {
        let cur_hash = record.entries.get(path).map(|e| e.hash.clone());
        let qualifies = match record.trigger {
            Trigger::Manual | Trigger::Restore => true,
            Trigger::Publish => cur_hash != prev_hash,
        };
        if qualifies {
            rows.push(row_for(id, record, 0, 0, 0));
        }
        prev_hash = cur_hash;
    }
    rows
}

/// Every record, newest-first-agnostic (oldest first, matching
/// [`super::store::list_records`]), each with its own changed/added/removed
/// count against the record before it.
pub(crate) fn site_timeline(root: &Path) -> Vec<Row> {
    let records = store::list_records(root);
    let mut rows = Vec::with_capacity(records.len());
    let mut prev: Option<&PublishRecord> = None;
    for (id, record) in &records {
        let (changed, added, removed) = diff_counts(prev, record);
        rows.push(row_for(id, record, changed, added, removed));
        prev = Some(record);
    }
    rows
}

fn row_for(id: &str, record: &PublishRecord, changed: usize, added: usize, removed: usize) -> Row {
    Row {
        id: id.to_string(),
        published_at: record.published_at.clone(),
        target: record.target.clone(),
        trigger: record.trigger,
        label: record.label.clone(),
        changed,
        added,
        removed,
    }
}

/// `changed`/`added`/`removed` between two records, tallied from
/// [`change_set::diff_hashes`]'s verbs. No previous record (the very first
/// one) reads as every one of its entries being added, the same reading
/// `classify` gives a page with no previous publish at all.
fn diff_counts(prev: Option<&PublishRecord>, cur: &PublishRecord) -> (usize, usize, usize) {
    let cur_hashes = record::flat_hashes(cur);
    let Some(prev) = prev else {
        return (0, cur_hashes.len(), 0);
    };
    let prev_hashes = record::flat_hashes(prev);
    let mut changed = 0;
    let mut added = 0;
    let mut removed = 0;
    for page in change_set::diff_hashes(&prev_hashes, &cur_hashes) {
        match page.verb {
            PageVerb::Added => added += 1,
            PageVerb::Edited => changed += 1,
            PageVerb::Deleted => removed += 1,
            PageVerb::Restyled => {}
        }
    }
    (changed, added, removed)
}

/// One version's pages against the live tree now: edited/added/deleted since,
/// reusing [`change_set::diff_hashes`] directly — a flat record has no
/// carry-gap and no output hash, so the shared core is the whole answer with
/// nothing layered on top (unlike `classify`, which needs its own restyle
/// pass and its own deletion loop).
pub(crate) fn site_version_pages(root: &Path, id: &str, current: &SealedManifest) -> Result<SiteVersionPages, String> {
    let record = store::load_record(root, id).ok_or_else(|| format!("no such version: {id}"))?;
    let record_hashes = record::flat_hashes(&record);
    let current_hashes = record::current_entry_hashes(current);

    let changed_since = change_set::diff_hashes(&record_hashes, &current_hashes);
    let changed_paths: HashSet<&str> = changed_since.iter().map(|c| c.source_path.as_str()).collect();
    let mut unchanged: Vec<String> =
        record_hashes.keys().filter(|path| !changed_paths.contains(path.as_str())).cloned().collect();
    unchanged.sort();

    Ok(SiteVersionPages { changed_since, unchanged })
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
