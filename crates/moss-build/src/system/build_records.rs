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

    /// Read-modify-write under the one lock, for a value whose parts must
    /// change together.
    fn update<R>(&self, key: String, f: impl FnOnce(&mut V) -> R) -> R
    where
        V: Default,
    {
        let mut slots = self.0.lock().expect("FolderSlot lock poisoned — a thread panicked while holding it");
        f(slots.entry(key).or_default())
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
    /// What the last seal found going offline, and what the author accepted
    /// losing — see `refuse_publish`. One slot, because the two change
    /// together: a seal prunes the accepted set against its own list, and an
    /// acceptance reads the list it accepts from. Under separate locks an
    /// acceptance could straddle a seal and accept an address the seal had
    /// just stopped reporting.
    removals: FolderSlot<FolderRemovals>,
}

/// See [`BuildRecords::removals`].
#[derive(Clone, Default)]
struct FolderRemovals {
    /// `None` until a seal records a verdict, which is not "clean".
    removed: Option<Vec<RemovedAddress>>,
    /// Unexplained removals the author accepted. Kept across rebuilds while
    /// they stay removals: the same set does not ask again, a new address
    /// does, and one that stops being a removal leaves the set at the next
    /// seal. Always a subset of `removed`'s unexplained paths.
    accepted: BTreeSet<String>,
}

impl FolderRemovals {
    fn unexplained(&self) -> impl Iterator<Item = &str> {
        self.removed
            .iter()
            .flatten()
            .filter(|r| r.reason == RemovalReason::Unexplained)
            .map(|r| r.path.as_str())
    }
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
    ///
    /// Also drops from the accepted set every address this seal does not
    /// find as an unexplained removal (the author fixed it, or redirected
    /// it), so one that goes offline again later is asked about again.
    pub fn record_removed_addresses(&self, folder_path: &str, removed: Vec<RemovedAddress>) {
        self.removals.update(Self::key(folder_path), |r| {
            r.removed = Some(removed);
            let current: BTreeSet<String> = r.unexplained().map(str::to_string).collect();
            r.accepted.retain(|path| current.contains(path));
        });
    }

    /// What the last seal of `folder_path` found going offline. `None` means
    /// no seal has recorded a verdict, which is not "clean".
    pub fn removed_addresses(&self, folder_path: &str) -> Option<Vec<RemovedAddress>> {
        self.removals.get(&Self::key(folder_path)).and_then(|r| r.removed)
    }

    /// Accept losing the addresses in `paths`: those of them that are
    /// unexplained removals of the last seal, added to what was accepted
    /// before. Nothing else is accepted, so a caller that showed a person a
    /// list passes that list and an address a rebuild added since stays
    /// pending. Returns the `paths` that were not accepted (not a current
    /// unexplained removal), so a caller can tell the list went stale.
    pub fn accept_unexplained_removals(&self, folder_path: &str, paths: &[String]) -> Vec<String> {
        self.removals.update(Self::key(folder_path), |r| {
            let current: BTreeSet<String> = r.unexplained().map(str::to_string).collect();
            let (taken, ignored): (Vec<String>, Vec<String>) =
                paths.iter().cloned().partition(|p| current.contains(p));
            r.accepted.extend(taken);
            ignored
        })
    }

    /// The removals that would refuse a publish of `folder_path` now.
    pub fn pending_removals(&self, folder_path: &str) -> Vec<crate::build::manifest::change_set::PendingRemoval> {
        let r = self.removals.get(&Self::key(folder_path)).unwrap_or_default();
        crate::build::manifest::change_set::pending_removals(r.removed.as_deref().unwrap_or(&[]), Some(&r.accepted))
    }

    /// Accept every removal pending now: what `moss deploy --accept-removals`
    /// does, in a single run where nothing can change between the build and
    /// this call. Crate-private: a caller that shows a person a list must
    /// accept that list with [`accept_unexplained_removals`](Self::accept_unexplained_removals).
    pub(crate) fn accept_all_pending_removals(&self, folder_path: &str) {
        self.removals.update(Self::key(folder_path), |r| {
            let current: Vec<String> = r.unexplained().map(str::to_string).collect();
            r.accepted.extend(current);
        });
    }

    /// Accepted paths that the last seal does not report as unexplained
    /// removals, read under the one lock: the invariant says there are none.
    #[cfg(test)]
    pub(crate) fn accepted_but_not_removed(&self, folder_path: &str) -> Vec<String> {
        self.removals.update(Self::key(folder_path), |r| {
            let current: BTreeSet<&str> = r.unexplained().collect();
            r.accepted.iter().filter(|p| !current.contains(p.as_str())).cloned().collect()
        })
    }

    #[cfg(test)]
    pub(crate) fn accepted_removals(&self, folder_path: &str) -> BTreeSet<String> {
        self.removals.get(&Self::key(folder_path)).unwrap_or_default().accepted
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
