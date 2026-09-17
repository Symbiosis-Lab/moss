//! Memoizes the xxh3 manifest hash of a CAS blob, keyed by the blob's oid.
//!
//! `pipeline.rs::copy_deferred_assets` hashes the just-linked output file on
//! every cache hit, even though nothing about that file could have changed:
//! `object_store.link_to(oid, target)` COW-copies the blob named `oid`
//! byte-for-byte into `target`, and a content-addressed blob's bytes cannot
//! change for a given oid. So `xxh3(target)` is a pure function of `oid` —
//! unlike a `(size, mtime)` staleness heuristic, there is no invalidation
//! rule to get wrong. See the TODO(perf) comments at both call sites in
//! `pipeline.rs` for why this key was chosen over the one they describe.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

/// Crude cap on the memo's size. Bounding growth matters (a memo that grows
/// across a vault's whole history is a slow leak); the exact number and
/// eviction policy do not, because a miss just falls through to a real read
/// — the failure mode this cache can have is "slower," never "wrong." So
/// when the map would grow past this, it is cleared outright rather than
/// given an LRU or any other real eviction policy.
const MAX_ENTRIES: usize = 50_000;

/// `oid -> xxh3` memo, with interior mutability so it can be threaded through
/// the asset walk as a shared `&ManifestHashMemo` (mirroring `ObjectStore`'s
/// own `&self` methods) rather than a `&mut` borrowed across two separate
/// loops. Loaded once at the start of a build and saved once at the end —
/// not on every asset — so the memo never causes more I/O than it saves.
pub(crate) struct ManifestHashMemo {
    entries: RefCell<HashMap<String, String>>,
    /// Hit/miss tally for the one summary line this build logs. Without it a
    /// memo that never hits and a memo whose saved reads were never worth
    /// anything look identical from outside — the same invisibility the
    /// orphan-prune INFO line exists to cure.
    hits: Cell<usize>,
    misses: Cell<usize>,
}

impl ManifestHashMemo {
    /// Load a previously-persisted memo from disk. Returns an empty memo if
    /// the file doesn't exist or can't be parsed — a cold memo is just a
    /// memo full of misses, not a correctness problem.
    pub(crate) fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();
        Self { entries: RefCell::new(entries), hits: Cell::new(0), misses: Cell::new(0) }
    }

    /// Look up the memoized xxh3 hash for a blob oid. A hit is a map lookup
    /// — no file I/O.
    pub(crate) fn get(&self, oid: &str) -> Option<String> {
        let hit = self.entries.borrow().get(oid).cloned();
        let counter = if hit.is_some() { &self.hits } else { &self.misses };
        counter.set(counter.get() + 1);
        hit
    }

    /// `(hits, misses)` since this memo was loaded, for the build's summary
    /// line. Counts lookups, not assets: two assets sharing one CAS blob are
    /// one miss and one hit, which is the honest number for "reads avoided."
    pub(crate) fn stats(&self) -> (usize, usize) {
        (self.hits.get(), self.misses.get())
    }

    /// Log this build's one memo summary line, alongside the source cache's
    /// own hits/misses. Lives here rather than at the call site so the memo
    /// owns both the counting and the reporting of it.
    pub(crate) fn log_summary(&self) {
        let (hits, misses) = self.stats();
        if hits + misses == 0 {
            return;
        }
        log::info!(
            "[background-assets] manifest-hash memo: {} hits, {} misses ({} full reads avoided)",
            hits, misses, hits
        );
    }

    /// Record a successfully-computed xxh3 hash for a blob oid. Callers must
    /// only call this with the result of an actual `compute_binary_hash_file`
    /// success — never with a fallback value (e.g. the CAS oid substituted
    /// when hashing fails) — or a single transient read failure would poison
    /// every future build with a wrong hash for that oid forever.
    pub(crate) fn record(&self, oid: &str, hash: String) {
        let mut entries = self.entries.borrow_mut();
        if !entries.contains_key(oid) && entries.len() >= MAX_ENTRIES {
            // Crude on purpose (see MAX_ENTRIES doc): dropping every live
            // entry costs the next build a real read per asset, same as a
            // cold memo. It never serves a wrong hash.
            entries.clear();
        }
        entries.insert(oid.to_string(), hash);
    }

    /// Persist the memo to disk (atomic write). Called once per build.
    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }
        let json = serde_json::to_string(&*self.entries.borrow())
            .map_err(|e| format!("Failed to serialize ManifestHashMemo: {}", e))?;
        // `.pending.<uuid>` mirrors HashIndex::save — iCloud excludes `.tmp`
        // files from sync, and the uuid keeps two concurrent builds' temp
        // files from colliding.
        let tmp = path.with_extension(format!("json.pending.{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, json.as_bytes())  // allow:raw_write temp for the memo's own atomic save, under .moss/cache
            .map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;
        // allow:unlink rename into place for the hash memo, not staging
        std::fs::rename(&tmp, path).map_err(|e| {
            format!("Failed to rename {} -> {}: {}", tmp.display(), path.display(), e)
        })
    }
}
