//! What one build of a folder leaves behind for the next one to read.
//!
//! # Why this is process-global rather than app-managed
//!
//! These two records — the last build's in-memory content hashes and its
//! publish-preflight verdict — are what moss shares *between builds of the same
//! folder*. They live in process-global state rather than in app state, so a
//! headless host (the CLI) reads and writes them the same way the app does:
//! `moss build --serve --watch` gets a race-free refresh baseline instead of
//! diffing against the asynchronously-sealed `hashes.json`, and a CLI-driven
//! test can observe state moss shares between builds.
//!
//! Neither record has a Tauri type in it, and neither describes a *window* —
//! they describe a *folder*, on the folder's own timeline. There is no second
//! arm for a headless host to answer differently, because the host is not
//! asked.
//!
//! Keyed by folder path string after normalization — this module owns the
//! normalization, so keys are consistent regardless of path-separator style. A
//! path can mix separators on Windows (one source yields backslashes, another
//! forward slashes), and a raw-string reader and a canonicalized writer would
//! otherwise produce mismatched keys, leaving build verdicts unreadable.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
use crate::build::manifest::link_audit::DeadLink;
use crate::build::types::PublishPreflightProjection;
use crate::types::content::SiteHashes;

/// One per-folder verdict, replaced wholesale on every write, never merged.
/// `None` on read means no build has answered yet in this process — distinct
/// from "clean" (an empty `Vec`, or whatever value a build genuinely
/// produced). `content_hashes`, `publish_preflight` and `promised_dead_links`
/// used to hand-write this insert/clone pair three times over; collapsed
/// 2026-09-16 (thermo review of the publish promise gate) since the three
/// differ only in `V`. `stale_sources` (2026-09-17) reuses the same shape.
struct FolderSlot<V>(Mutex<HashMap<String, V>>);

impl<V> Default for FolderSlot<V> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

impl<V: Clone> FolderSlot<V> {
    fn record(&self, key: String, value: V) {
        self.0.lock().expect("FolderSlot lock poisoned — a thread panicked while holding it").insert(key, value);
    }

    fn get(&self, key: &str) -> Option<V> {
        self.0.lock().expect("FolderSlot lock poisoned — a thread panicked while holding it").get(key).cloned()
    }

    fn forget(&self, key: &str) {
        self.0.lock().expect("FolderSlot lock poisoned — a thread panicked while holding it").remove(key);
    }
}

impl FolderSlot<PublishPreflightProjection> {
    /// Replace this folder's immutable projection only when its admission
    /// generation is at least as new as the installed answer. The comparison
    /// and replacement share one lock, so callers cannot split the ordering
    /// policy from the atomic write.
    fn install_if_newer(&self, key: String, projection: PublishPreflightProjection) {
        let mut projections = self
            .0
            .lock()
            .expect("FolderSlot lock poisoned — a thread panicked while holding it");
        if projections
            .get(&key)
            .is_some_and(|current| current.build_generation > projection.build_generation)
        {
            return;
        }
        projections.insert(key, projection);
    }
}

/// The per-folder records the build tail writes and the watcher reads.
#[derive(Default)]
pub struct BuildRecords {
    content_hashes: FolderSlot<SiteHashes>,
    publish_preflight: FolderSlot<PublishPreflightProjection>,
    /// This seal's own still-pending promises the link audit caught dead —
    /// see `link_audit::dead_links_among_promises` and `refuse_publish`.
    promised_dead_links: FolderSlot<Vec<DeadLink>>,
    /// Structural sources (a page, `config.toml`, the user stylesheet) the
    /// last build of this folder had to carry forward rather than read — see
    /// `cloud_ledger::structural_stale_paths` and `refuse_publish`.
    stale_sources: FolderSlot<Vec<String>>,
    /// The merged redirect map (`feeds::redirects::merge_redirects`'s result)
    /// the last build of this folder emitted stubs from. Redirect-stub bytes
    /// are a pure function of this map, so an unchanged map means the stub
    /// set this build would write is byte-identical to what is already
    /// staged — see `feeds::redirects::emit_redirect_table`.
    redirect_signature: FolderSlot<BTreeMap<String, String>>,
    /// The public addresses the last seal of this folder found the last
    /// publish serving and this build does not — see `refuse_publish`.
    removed_addresses: FolderSlot<Vec<RemovedAddress>>,
    /// The exact set of unexplained removals the author accepted losing. Kept
    /// across rebuilds: the same set does not ask again, a new address does.
    accepted_removals: FolderSlot<BTreeSet<String>>,
}

impl BuildRecords {
    /// Normalize a folder path to a canonical string key, so all callers land on
    /// the same key regardless of path-separator style.
    fn key(folder_path: &str) -> String {
        crate::vault_root::resolve_input(folder_path).to_string_lossy().into_owned()
    }

    /// Record so the watcher can diff the live screen against race-free data
    /// instead of re-reading the `hashes.json` a detached seal task writes at
    /// an unpredictable time. Callers must gate this on `should_stash_hashes`
    /// — the record must describe what the screen shows, so a withheld or
    /// cancelled build must not write one.
    pub fn record_content_hashes(&self, folder_path: &str, hashes: SiteHashes) {
        self.content_hashes.record(Self::key(folder_path), hashes);
    }

    /// Retaining (not consuming) is load-bearing: the same value serves as
    /// BOTH the `new` side of the just-finished rebuild's diff and the
    /// `previous` (baseline) side of the NEXT rebuild's — see
    /// `crate::build::watch::baseline_for_rebuild`. `None` means no build of
    /// this folder has finished in this process yet; callers fall back to
    /// `load_previous_hashes`.
    pub fn content_hashes(&self, folder_path: &str) -> Option<SiteHashes> {
        self.content_hashes.get(&Self::key(folder_path))
    }

    /// Install one completed build's publish evidence. The admission epoch is
    /// the build generation: an older build may finish later, but cannot
    /// replace a later build's answer. A clean projection still replaces a
    /// broken one because its empty vector is a completed answer.
    pub fn install_publish_preflight(&self, folder_path: &str, projection: PublishPreflightProjection) {
        self.publish_preflight.install_if_newer(Self::key(folder_path), projection);
    }

    /// The last completed build's atomic publish evidence. `None` means no
    /// build has finished in this process — which is not a clean verdict.
    pub fn publish_preflight(&self, folder_path: &str) -> Option<PublishPreflightProjection> {
        self.publish_preflight.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec` — a video that finishes
    /// encoding between one seal and the next must have its refusal cleared
    /// by the CLEAN verdict, not left standing because nothing wrote over it.
    pub fn record_promised_dead_links(&self, folder_path: &str, dead: Vec<DeadLink>) {
        self.promised_dead_links.record(Self::key(folder_path), dead);
    }

    /// What the last seal of `folder_path` found among its own unfulfilled
    /// promises. `None` means no seal has recorded a verdict — not "clean",
    /// the same distinction the preflight projection draws.
    pub fn promised_dead_links(&self, folder_path: &str) -> Option<Vec<DeadLink>> {
        self.promised_dead_links.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec` — the same reasoning as
    /// `install_publish_preflight`: a source that arrives must clear the refusal,
    /// and only an unconditional write does that.
    pub fn record_stale_sources(&self, folder_path: &str, stale: Vec<String>) {
        self.stale_sources.record(Self::key(folder_path), stale);
    }

    /// What the last build of `folder_path` carried forward rather than read.
    /// `None` means no build has finished in this process — which is NOT
    /// "clean", the same distinction the preflight projection draws.
    pub fn stale_sources(&self, folder_path: &str) -> Option<Vec<String>> {
        self.stale_sources.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec`: a rebuild that restores
    /// the address must clear the refusal, and only an unconditional write
    /// does that.
    pub fn record_removed_addresses(&self, folder_path: &str, removed: Vec<RemovedAddress>) {
        self.removed_addresses.record(Self::key(folder_path), removed);
    }

    /// What the last seal of `folder_path` found going offline. `None` means
    /// no seal has recorded a verdict, which is not "clean".
    pub fn removed_addresses(&self, folder_path: &str) -> Option<Vec<RemovedAddress>> {
        self.removed_addresses.get(&Self::key(folder_path))
    }

    /// Accept losing exactly the unexplained addresses the last seal found,
    /// replacing any earlier acceptance. Not a standing permission: an address
    /// that only shows up in a later build is not in this set.
    pub fn accept_unexplained_removals(&self, folder_path: &str) {
        let accepted = self
            .removed_addresses(folder_path)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.reason == RemovalReason::Unexplained)
            .map(|r| r.path)
            .collect();
        self.accepted_removals.record(Self::key(folder_path), accepted);
    }

    /// The set recorded by [`accept_unexplained_removals`](Self::accept_unexplained_removals);
    /// `None` when the author has accepted nothing for this folder.
    pub fn accepted_removals(&self, folder_path: &str) -> Option<BTreeSet<String>> {
        self.accepted_removals.get(&Self::key(folder_path))
    }

    /// Drop the content-hash baseline for a folder moss is no longer
    /// watching: returning to the folder falls back to disk for the first
    /// rebuild, which is correct — a baseline never compared against a live
    /// screen is not one.
    pub fn forget_content_hashes(&self, folder_path: &str) {
        self.content_hashes.forget(&Self::key(folder_path));
    }

    /// Record the merged redirect map the just-finished build emitted stubs
    /// from, so the next build in this process can tell whether it needs to
    /// re-emit anything.
    pub fn record_redirect_signature(&self, folder_path: &str, merged: BTreeMap<String, String>) {
        self.redirect_signature.record(Self::key(folder_path), merged);
    }

    /// The merged redirect map the last build of `folder_path` emitted stubs
    /// from, in this process. `None` means no build of this folder has
    /// finished here yet — not "no redirects", which is `Some(empty)`.
    pub fn redirect_signature(&self, folder_path: &str) -> Option<BTreeMap<String, String>> {
        self.redirect_signature.get(&Self::key(folder_path))
    }
}

static RECORDS: std::sync::OnceLock<BuildRecords> = std::sync::OnceLock::new();

/// The process's build records. One per process, like the folder-session
/// registry — a folder has one answer regardless of who is asking.
pub fn records() -> &'static BuildRecords {
    RECORDS.get_or_init(BuildRecords::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(generation: u64, reference: &str) -> PublishPreflightProjection {
        PublishPreflightProjection {
            build_generation: generation,
            missing_references: vec![crate::build::types::MissingReferenceOccurrence {
                source_path: "page.md".to_string(),
                source_revision: crate::build::types::SourceRevision::from_source("page"),
                reference: reference.to_string(),
                source_span: crate::build::types::SourceSpan {
                    start_byte: 0,
                    end_byte: 1,
                    line: 1,
                },
            }],
        }
    }

    /// The round trip, and the retain: a peek must not consume, because the
    /// same value is both this rebuild's `new` side and the next one's
    /// baseline.
    #[test]
    fn recorded_hashes_read_back_and_survive_the_read() {
        let records = BuildRecords::default();
        let folder = "/tmp/a-vault";
        assert!(records.content_hashes(folder).is_none());

        let mut hashes = SiteHashes::default();
        hashes.files.insert("index.html".into(), "abc".into());
        records.record_content_hashes(folder, hashes.clone());

        assert!(records.content_hashes("/tmp/another-vault").is_none());
        assert_eq!(
            records.content_hashes(folder).expect("recorded").files,
            hashes.files
        );
        assert!(
            records.content_hashes(folder).is_some(),
            "peek must retain — the baseline is read once per rebuild, forever"
        );

        records.forget_content_hashes(folder);
        assert!(records.content_hashes(folder).is_none());
    }

    #[test]
    fn older_preflight_completion_cannot_replace_a_newer_projection() {
        let records = BuildRecords::default();
        records.install_publish_preflight("/tmp/ordered-vault", projection(2, "newer.png"));
        records.install_publish_preflight("/tmp/ordered-vault", projection(1, "older.png"));

        let installed = records.publish_preflight("/tmp/ordered-vault").expect("newer projection remains");
        assert_eq!(installed.build_generation, 2);
        assert_eq!(installed.missing_references[0].reference, "newer.png");
    }

}
