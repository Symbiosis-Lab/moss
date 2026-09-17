//! Where a site's history lives: the record directory and the `ObjectStore`
//! blobs sit under.
//!
//! Nothing here knows what a [`super::record::PublishRecord`] means — it only
//! knows how to find one, by id or by listing, and where its blobs are. That
//! split is what lets [`super::record`], [`super::restore`] and
//! [`super::timeline`] all read the same store without importing each other.
//!
//! One vault, one store: every function below takes the resolved
//! `.moss/history/` directory directly (what [`super::HistoryStore`] wraps),
//! already fixed relative to the vault it belongs to. There is no site key and
//! no orphan recovery — those existed only because many vaults once shared one
//! app-data root and had to be told apart; a store that lives inside its own
//! vault needs neither.

use std::path::{Path, PathBuf};

use super::record::PublishRecord;
use crate::build::cache::ObjectStore;

/// Above this size a regular file is recorded by hash only — no blob is kept.
///
/// moss no longer has a fixed byte threshold for the chunked-upload protocol:
/// `seta::upload_policy::needs_chunking` sizes the routing decision from
/// measured link throughput, not a constant (see that module's header on why
/// a fixed number could not survive a 2.7x slower tester one day later). So
/// this is its own constant rather than a reuse, set to the 20 MB the design
/// settled on as "one notion of a large file", beside
/// `crate::build::store_gc::KEEP_GENERATIONS_DEFAULT`.
pub(crate) const HISTORY_MEDIA_CEILING: u64 = 20 * 1024 * 1024;

fn publishes_dir(root: &Path) -> PathBuf {
    root.join("publishes")
}

fn objects_dir(root: &Path) -> PathBuf {
    root.join("objects")
}

pub(crate) fn object_store(root: &Path) -> ObjectStore {
    ObjectStore::new(objects_dir(root))
}

/// Is history recording turned on for this vault? `[history].enabled`,
/// default on.
pub(crate) fn history_enabled(vault_root: &Path) -> bool {
    crate::build::site_config::get_history_enabled(&vault_root.to_string_lossy()).unwrap_or(true)
}

/// One record, by the id its filename gave it ([`super::record`]'s writer
/// naming). `None` for a missing or unparseable file — the same "unreadable
/// is not absent, but nothing to show either" posture as the rest of history.
pub(crate) fn load_record(root: &Path, id: &str) -> Option<PublishRecord> {
    let path = publishes_dir(root).join(format!("{id}.json"));
    // allow:raw_read .moss/history/ (ADR-083) — cloud-synced now, but content-addressed and write-once: a missing/evicted blob already reads as "not kept," never silently as fresh
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Every record for this site, oldest first — the filename's RFC3339-with-`:`
/// replaced-by-`-` prefix sorts chronologically as a plain string, so no
/// parsing is needed to order them.
pub(crate) fn list_records(root: &Path) -> Vec<(String, PublishRecord)> {
    let dir = publishes_dir(root);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<(String, PublishRecord)> = rd
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().into_owned();
            // allow:raw_read .moss/history/ (ADR-083) — cloud-synced now, but content-addressed and write-once: a missing/evicted blob already reads as "not kept," never silently as fresh
            let bytes = std::fs::read(&path).ok()?;
            let record: PublishRecord = serde_json::from_slice(&bytes).ok()?;
            Some((id, record))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The record whose `generation_id` matches the deploy baseline currently
/// live for its target — the recency-blind definition "Live" actually has.
/// A manual save or a restore can be newer than the last real publish and
/// must never wear the badge; the deploy baseline
/// (`build::manifest::published_record::load_for`) is the one thing that
/// knows what is actually live, so callers load it once and pass its
/// `generation_id` in here rather than this module reading
/// `.moss/deploy/records/` itself.
pub fn is_live(record: &PublishRecord, live_generation_id: Option<&str>) -> bool {
    live_generation_id.is_some_and(|gid| gid == record.generation_id)
}

/// Read one path's bytes as they stood at one version — the raw content
/// behind "Show in Finder" and a page's read-only source view. For a
/// symlink entry this is the target string, the same bytes `record_one`
/// stored for it.
pub(crate) fn read_version(root: &Path, id: &str, path: &str) -> Result<Vec<u8>, String> {
    let record = load_record(root, id).ok_or_else(|| format!("no such version: {id}"))?;
    let entry = record.entries.get(path).ok_or_else(|| format!("{path} is not in this version"))?;
    let store = object_store(root);
    let blob = store
        .get_path(&entry.hash)
        .ok_or_else(|| format!("the content of {path} at this version was not kept"))?;
    // allow:raw_read .moss/history/ (ADR-083) — cloud-synced now, but content-addressed and write-once: a missing/evicted blob already reads as "not kept," never silently as fresh
    std::fs::read(&blob).map_err(|e| format!("could not read {}: {e}", blob.display()))
}

/// Count files under a store root, recursively — the sharded object store has
/// no public "how many blobs" query, and every test in this module (and
/// `deploy::landed`'s integration test) needs one.
#[cfg(test)]
pub(crate) fn count_blob_files_for_tests(root: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count);
            } else {
                *count += 1;
            }
        }
    }
    let mut count = 0;
    walk(root, &mut count);
    count
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
