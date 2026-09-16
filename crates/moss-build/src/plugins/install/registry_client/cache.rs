//! The registry cache in app data: the accepted bytes of both documents, the
//! monotonic serial floors, and when each was last fetched.
//!
//! Every function takes `app_data_dir` rather than an app handle, so
//! the whole file is unit-testable against a tempdir — the same shape
//! `startup::recent_folders` uses, and for the same reason.
//!
//! **The cache stores the bytes that passed validation, and re-validates them
//! on the way out.** Storing re-serialized structs and trusting them back
//! would put the rules on one side of the disk only: a locally tampered
//! `index.json` carrying an arbitrary `download_url` + `sha256` would be
//! believed wholesale, and a `schema_version` the network path refuses would
//! be served happily from disk. Round-tripping the raw bytes also means an
//! additive field a newer registry publishes survives the cache instead of
//! being silently dropped by an older client's struct.
//!
//! Staleness here is benign: integrity lives in the entries (pinned hash,
//! pinned version, monotonic serial), never in the transport. A stale cache
//! can miss a newly published plugin; it can never install unreviewed bytes.
//! The one thing that must NOT degrade with age is revocation, which is why
//! the cached kill list keeps being enforced when a refresh fails — and why
//! a kill list that is present but unreadable reports [`RevocationVerdict::Unknown`]
//! rather than "not revoked".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::index::{accept_index, accept_revoked, RegistryIndex, Revocation, RevokedList};
use crate::infra::atomic_write::{write_atomic, write_json_atomic};

const CACHE_DIR: &str = "registry";
const INDEX_FILE: &str = "index.json";
const REVOKED_FILE: &str = "revoked.json";
const ANCHOR_FILE: &str = "anchor.json";

/// The replay-protection anchor: the highest serial ever *accepted* for each
/// document, and how many revocations the last accepted kill list held.
///
/// This is a separate file from the documents it guards, because a floor kept
/// inside `revoked.json` is a floor that `rm revoked.json` resets. It is a
/// durability record, not the sole witness: [`load`] takes the max of the
/// anchor and the cached document's own serial, so deleting *either* file
/// alone cannot lower the floor. Only losing both loses the floor, and that
/// is also the state in which nothing is cached to protect.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Anchor {
    #[serde(default)]
    pub highest_index_serial: u64,
    #[serde(default)]
    pub highest_revoked_serial: u64,
    /// Feeds the shrink check that refuses a silently emptied kill list.
    #[serde(default)]
    pub revocation_count: usize,
    /// ISO-8601 UTC, for the catalog's quiet "last refreshed" line. Its
    /// presence is also the evidence that a kill list was once seen, which is
    /// how [`RevocationVerdict::Unknown`] is told apart from "never fetched".
    #[serde(default)]
    pub index_fetched_at: Option<String>,
    #[serde(default)]
    pub revoked_fetched_at: Option<String>,
}

/// What the cache can say about one id+version.
///
/// Three states, because `Option` cannot hold the one that matters: a kill
/// list that is present but unreadable is NOT evidence that nothing is
/// revoked. The design's "fail closed, always" rule is a rule about the whole
/// path, and a corrupt file in app data reaches the same decision a corrupt
/// file on the wire does.
#[derive(Debug, Clone, PartialEq)]
pub enum RevocationVerdict<'a> {
    /// A kill list is loaded and does not cover this id+version.
    NotRevoked,
    /// A kill list is loaded and covers it.
    Revoked(&'a Revocation),
    /// A kill list was seen at some point but cannot be read now. Not an
    /// all-clear: a caller that collapses this into `NotRevoked` has made the
    /// kill list defeatable by corrupting one file. What it *does* mean
    /// depends on where the plugin's trust comes from, which this layer does
    /// not know — see `enforce::check`, the one place that decides.
    Unknown,
}

/// Everything the client remembers between runs.
#[derive(Debug, Clone, Default)]
pub struct CachedRegistry {
    /// The last accepted catalog, re-validated on load. `None` before the
    /// first successful fetch, which the catalog shows as
    /// bundled-plugins-only rather than as an error.
    pub index: Option<RegistryIndex>,
    /// The last accepted kill list, re-validated on load.
    pub revoked: Option<RevokedList>,
    pub anchor: Anchor,
}

impl CachedRegistry {
    /// Whether this exact id+version is revoked. The one place anything asks,
    /// so the loader and the catalog cannot disagree.
    pub fn revocation_for(&self, id: &str, version: &str) -> RevocationVerdict<'_> {
        match &self.revoked {
            Some(list) => match list.find(id, version) {
                Some(revocation) => RevocationVerdict::Revoked(revocation),
                None => RevocationVerdict::NotRevoked,
            },
            // No list loaded. If one was ever fetched, its absence now is a
            // corrupt or deleted file, not an all-clear.
            None if self.anchor.revoked_fetched_at.is_some() => RevocationVerdict::Unknown,
            None => RevocationVerdict::NotRevoked,
        }
    }

    /// A kill list was fetched once and the cached copy no longer parses.
    /// Worth one log line per refresh, not one per plugin load.
    ///
    /// Asks [`Self::revocation_for`] rather than restating its `Unknown`
    /// condition: a second copy of that test drifts silently the first time a
    /// term is added to one of them, and the warning would then stop matching
    /// what enforcement actually does. The id and version are unused on this
    /// path — `Unknown` is a fact about the list, not about a plugin.
    pub fn revoked_is_unreadable(&self) -> bool {
        matches!(self.revocation_for("", ""), RevocationVerdict::Unknown)
    }
}

fn cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CACHE_DIR)
}

/// Load whatever is cached, re-running every acceptance rule against it.
///
/// Never fails: an empty cache is the first-run state. A document that no
/// longer passes its own rules is dropped rather than served, and for the
/// kill list that drop surfaces as [`RevocationVerdict::Unknown`], never as
/// "nothing revoked".
pub fn load(app_data_dir: &Path) -> CachedRegistry {
    let dir = cache_dir(app_data_dir);
    let mut anchor: Anchor = std::fs::read_to_string(dir.join(ANCHOR_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    // Validate against a zero floor first: the cached document's own serial is
    // then folded INTO the floor below. Validating against the anchor's floor
    // would reject a cached document whose serial equals it in the shrink
    // check, and would make a lost anchor silently lower the floor.
    let index = std::fs::read_to_string(dir.join(INDEX_FILE))
        .ok()
        .and_then(|raw| match accept_index(&raw, 0) {
            Ok(index) => Some(index),
            Err(e) => {
                log::warn!("cached registry index no longer passes its own rules ({e}); dropping it");
                None
            }
        });
    let revoked = std::fs::read_to_string(dir.join(REVOKED_FILE))
        .ok()
        .and_then(|raw| match accept_revoked(&raw, 0, 0) {
            Ok(list) => Some(list),
            Err(e) => {
                log::warn!("cached kill list no longer passes its own rules ({e}); revocation state is UNKNOWN until the next successful refresh");
                None
            }
        });

    // The floor is the max of both witnesses, so deleting either file alone
    // cannot lower it.
    if let Some(index) = &index {
        anchor.highest_index_serial = anchor.highest_index_serial.max(index.serial);
    }
    if let Some(list) = &revoked {
        anchor.highest_revoked_serial = anchor.highest_revoked_serial.max(list.serial);
        anchor.revocation_count = anchor.revocation_count.max(list.revocations.len());
        // A kill list on disk is itself proof one was fetched, even if the
        // anchor recording that fact is gone.
        anchor.revoked_fetched_at = anchor.revoked_fetched_at.or(Some(String::from("unknown")));
    }

    CachedRegistry {
        index,
        revoked,
        anchor,
    }
}

/// Persist the accepted bytes of an index, then raise the anchor.
///
/// `raw` is what the origin served and what [`accept_index`] approved — not a
/// re-serialization of the parsed struct. See the module header.
///
/// The document is written before the floor is raised. Neither order is
/// unsafe now that [`load`] derives the floor from both files, so the choice
/// is made on the cheaper failure: a written document with an unraised floor
/// costs nothing (the floor is re-derived on the next load), while a raised
/// floor with no document briefly refuses nothing that matters.
pub fn store_index(
    app_data_dir: &Path,
    raw: &str,
    index: &RegistryIndex,
    fetched_at: &str,
) -> Result<(), String> {
    let dir = cache_dir(app_data_dir);
    write_atomic(&dir.join(INDEX_FILE), raw)?;
    update_anchor(&dir, |a| {
        a.highest_index_serial = a.highest_index_serial.max(index.serial);
        a.index_fetched_at = Some(fetched_at.to_string());
    })
}

/// Persist the accepted bytes of a kill list, then raise the anchor.
pub fn store_revoked(
    app_data_dir: &Path,
    raw: &str,
    revoked: &RevokedList,
    fetched_at: &str,
) -> Result<(), String> {
    let dir = cache_dir(app_data_dir);
    write_atomic(&dir.join(REVOKED_FILE), raw)?;
    update_anchor(&dir, |a| {
        a.highest_revoked_serial = a.highest_revoked_serial.max(revoked.serial);
        a.revocation_count = a.revocation_count.max(revoked.revocations.len());
        a.revoked_fetched_at = Some(fetched_at.to_string());
    })
}

/// Read-modify-write the anchor.
///
/// This is NOT atomic against a concurrent writer in another process — `max`
/// over a stale snapshot does not make an RMW safe. Within this process the
/// refresh lock in [`super::fetch`] serializes it; across processes, [`load`]
/// deriving the floor from the documents themselves is what bounds the
/// damage, because a lost anchor update is re-derived rather than believed.
fn update_anchor(dir: &Path, mutate: impl FnOnce(&mut Anchor)) -> Result<(), String> {
    let mut anchor: Anchor = std::fs::read_to_string(dir.join(ANCHOR_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    mutate(&mut anchor);
    write_json_atomic(&dir.join(ANCHOR_FILE), &anchor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_raw(serial: u64) -> String {
        format!(r#"{{"schema_version":1,"serial":{serial},"entries":[]}}"#)
    }

    fn revoked_raw(serial: u64, count: usize) -> String {
        let revs: Vec<String> = (0..count)
            .map(|i| format!(r#"{{"id":"bad{i}","versions":["*"],"reason":"r"}}"#))
            .collect();
        format!(
            r#"{{"schema_version":1,"serial":{serial},"revocations":[{}]}}"#,
            revs.join(",")
        )
    }

    fn put_index(dir: &Path, serial: u64) {
        let raw = index_raw(serial);
        let parsed = accept_index(&raw, 0).unwrap();
        store_index(dir, &raw, &parsed, "2026-08-30T00:00:00.000Z").unwrap();
    }

    fn put_revoked(dir: &Path, serial: u64, count: usize) {
        let raw = revoked_raw(serial, count);
        let parsed = accept_revoked(&raw, 0, 0).unwrap();
        store_revoked(dir, &raw, &parsed, "2026-08-30T00:00:01.000Z").unwrap();
    }

    #[test]
    fn an_empty_app_data_dir_loads_as_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let cache = load(dir.path());
        assert!(cache.index.is_none());
        assert!(cache.revoked.is_none());
        assert_eq!(cache.anchor, Anchor::default());
    }

    #[test]
    fn stored_documents_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        put_index(dir.path(), 28);
        put_revoked(dir.path(), 7, 2);

        let cache = load(dir.path());
        assert_eq!(cache.index.unwrap().serial, 28);
        assert_eq!(cache.revoked.unwrap().revocations.len(), 2);
        assert_eq!(cache.anchor.highest_index_serial, 28);
        assert_eq!(cache.anchor.highest_revoked_serial, 7);
        assert_eq!(cache.anchor.revocation_count, 2);
        assert_eq!(
            cache.anchor.index_fetched_at.as_deref(),
            Some("2026-08-30T00:00:00.000Z")
        );
    }

    #[test]
    fn the_cache_stores_the_bytes_the_origin_served_not_a_reserialization() {
        // An additive field a newer registry publishes must survive the cache,
        // rather than being dropped by an older client's struct on the way
        // through it.
        let dir = tempfile::tempdir().unwrap();
        let raw = r#"{"schema_version":1,"serial":9,"entries":[],"published_by":"ci"}"#;
        let parsed = accept_index(raw, 0).unwrap();
        store_index(dir.path(), raw, &parsed, "t").unwrap();

        let on_disk = std::fs::read_to_string(dir.path().join(CACHE_DIR).join(INDEX_FILE)).unwrap();
        assert!(
            on_disk.contains("published_by"),
            "the served bytes must be what is cached, got: {on_disk}"
        );
    }

    #[test]
    fn the_serial_floor_survives_deleting_the_document_it_guards() {
        // The anti-rollback property must not be defeated by `rm`.
        let dir = tempfile::tempdir().unwrap();
        put_revoked(dir.path(), 7, 2);
        std::fs::remove_file(dir.path().join(CACHE_DIR).join(REVOKED_FILE)).unwrap();

        let cache = load(dir.path());
        assert!(cache.revoked.is_none(), "the document is gone");
        assert_eq!(
            cache.anchor.highest_revoked_serial, 7,
            "but the floor it was validated against is not"
        );
        assert_eq!(cache.anchor.revocation_count, 2);
    }

    #[test]
    fn the_serial_floor_survives_deleting_the_anchor_that_records_it() {
        // The other half of the test above. The anchor is a durability record,
        // not the sole witness, so `rm anchor.json` must not reset the floor
        // while the documents it was derived from are still on disk.
        let dir = tempfile::tempdir().unwrap();
        put_index(dir.path(), 28);
        put_revoked(dir.path(), 7, 2);
        std::fs::remove_file(dir.path().join(CACHE_DIR).join(ANCHOR_FILE)).unwrap();

        let cache = load(dir.path());
        assert_eq!(cache.anchor.highest_index_serial, 28);
        assert_eq!(cache.anchor.highest_revoked_serial, 7);
        assert_eq!(cache.anchor.revocation_count, 2);
    }

    #[test]
    fn a_corrupt_cached_kill_list_reports_unknown_not_nothing_revoked() {
        // The cache side of "can't parse never means nothing revoked". One
        // flipped byte in app data, no network involved.
        let dir = tempfile::tempdir().unwrap();
        put_revoked(dir.path(), 7, 1);
        std::fs::write(dir.path().join(CACHE_DIR).join(REVOKED_FILE), "}{ garbage").unwrap();

        let cache = load(dir.path());
        assert!(cache.revoked.is_none());
        assert_eq!(
            cache.revocation_for("bad0", "1.0.0"),
            RevocationVerdict::Unknown,
            "a kill list that was seen and is now unreadable is not an all-clear"
        );
    }

    #[test]
    fn a_cached_document_that_no_longer_passes_its_own_rules_is_dropped() {
        // Locally tampered app data must not be trusted more than the wire.
        let dir = tempfile::tempdir().unwrap();
        put_index(dir.path(), 28);
        std::fs::write(
            dir.path().join(CACHE_DIR).join(INDEX_FILE),
            r#"{"schema_version":99,"serial":500,"entries":[]}"#,
        )
        .unwrap();

        let cache = load(dir.path());
        assert!(
            cache.index.is_none(),
            "a schema_version the network path refuses must not be served from disk"
        );
        assert_eq!(
            cache.anchor.highest_index_serial, 28,
            "and the tampered serial must not raise the floor"
        );
    }

    #[test]
    fn a_kill_list_cannot_be_replaced_by_an_index_served_at_its_url() {
        // A Pages redirect or a wrong path in the publish workflow must not
        // silently un-revoke everything. Without each document requiring its
        // own discriminating key, index.json parses as an empty kill list at
        // the index's much higher serial.
        let index = include_str!("../../../../tests/fixtures/registry/index.json");
        assert!(
            accept_revoked(index, 0, 0).is_err(),
            "index.json must not parse as a kill list"
        );
        let revoked = include_str!("../../../../tests/fixtures/registry/revoked.json");
        assert!(
            accept_index(revoked, 0).is_err(),
            "revoked.json must not parse as an index"
        );
    }

    #[test]
    fn floors_only_ever_rise() {
        let dir = tempfile::tempdir().unwrap();
        put_index(dir.path(), 28);
        put_index(dir.path(), 5);
        assert_eq!(load(dir.path()).anchor.highest_index_serial, 28);
    }

    #[test]
    fn revocation_lookup_reads_the_cached_list() {
        let dir = tempfile::tempdir().unwrap();
        put_revoked(dir.path(), 7, 1);
        let cache = load(dir.path());
        assert!(matches!(
            cache.revocation_for("bad0", "9.9.9"),
            RevocationVerdict::Revoked(_)
        ));
        assert_eq!(
            cache.revocation_for("fine", "1.0.0"),
            RevocationVerdict::NotRevoked
        );
    }

    #[test]
    fn nothing_is_revoked_before_the_first_fetch() {
        // The one state in which "not revoked" is the honest answer: no kill
        // list has ever been seen, so there is nothing to have lost.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path()).revocation_for("anything", "1.0.0"),
            RevocationVerdict::NotRevoked
        );
    }

    #[test]
    fn an_empty_kill_list_that_was_fetched_means_nothing_is_revoked() {
        let dir = tempfile::tempdir().unwrap();
        put_revoked(dir.path(), 1, 0);
        assert_eq!(
            load(dir.path()).revocation_for("anything", "1.0.0"),
            RevocationVerdict::NotRevoked
        );
    }

    #[test]
    fn a_write_leaves_no_temp_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        put_index(dir.path(), 1);
        put_revoked(dir.path(), 1, 0);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join(CACHE_DIR))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }
}
