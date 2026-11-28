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
/// produced).
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

/// What a seal found among its own still-pending promises.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromisedDeadLinks {
    /// Links the link audit caught dead — see `link_audit::dead_links_among_promises`.
    pub links: Vec<DeadLink>,
    /// Base names of those promises' sources that are still in the cloud,
    /// sorted and unique. A promise in this list is waiting on a download, not
    /// an encode, and may never arrive.
    pub in_cloud: Vec<String>,
}

/// The per-folder records the build tail writes and the watcher reads.
#[derive(Default)]
pub struct BuildRecords {
    content_hashes: FolderSlot<SiteHashes>,
    publish_preflight: FolderSlot<PublishPreflightProjection>,
    /// This seal's own still-pending promises the link audit caught dead —
    /// see `refuse_publish`.
    promised_dead_links: FolderSlot<PromisedDeadLinks>,
    /// How many embedded files the last render of this folder showed as a
    /// placeholder because their text was still being read. A publish waits
    /// for zero: a placeholder is not something to put on the web.
    pending_derivations: FolderSlot<usize>,
    unverifiable_moderation: FolderSlot<usize>,
    /// How the newest build ended, with its build generation.
    build_end: FolderSlot<Option<(u64, BuildEnd)>>,
    /// What the last seal found going offline, and what the author accepted
    /// losing — see `refuse_publish`. One slot, because the two change
    /// together: a seal prunes the accepted set against its own list, and an
    /// acceptance reads the list it accepts from. Under separate locks an
    /// acceptance could straddle a seal and accept an address the seal had
    /// just stopped reporting.
    removals: FolderSlot<FolderRemovals>,
    /// Rollbacks `--accept-stale-content` accepted, keyed by page with the
    /// published and proposed hashes, so changed bytes need accepting again.
    accepted_stale: FolderSlot<BTreeMap<String, String>>,
    /// `(old, served)` output paths the last seal kept answering — see
    /// `feeds::kept_addresses`.
    kept: FolderSlot<Vec<(String, String)>>,
    /// Scripts the last build published to everyone from a members folder.
    public_scripts: FolderSlot<Vec<String>>,
    /// The last build's file copies and folder redirects, for its seal to write.
    late_redirects: FolderSlot<crate::build::feeds::kept_addresses::LateRedirects>,
    /// The top-level entries the last shown build listed before its scan.
    scanned_listing: FolderSlot<Vec<std::path::PathBuf>>,
}

/// How a build ended, for the publish that follows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuildEnd {
    /// Its generation is the one a publish reads.
    Publishable,
    /// It produced no generation: the render or its background work failed,
    /// or the output could not be written or read back.
    Failed,
    /// Held back because these sources could not be read.
    SourcesUnread(Vec<String>),
    /// Stopped on a file the cloud provider has not handed back; its arrival
    /// starts the next build.
    Downloading,
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
            .filter(|r| r.reason.refuses())
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
    /// an unpredictable time. Only a build that shows records one (`build::stash_shown_build`)
    /// — the record must describe what the screen shows, so a withheld or
    /// cancelled build must not write one.
    pub fn record_content_hashes(&self, folder_path: &str, hashes: SiteHashes) {
        self.content_hashes.record(Self::key(folder_path), hashes);
    }

    /// Retaining (not consuming) is load-bearing: the same value serves as
    /// BOTH the `new` side of the just-finished rebuild's diff and the
    /// `previous` (baseline) side of the NEXT rebuild's — see
    /// `crate::build::watch::RefreshPlan`. `None` means no build of
    /// this folder has finished in this process yet; callers fall back to
    /// `load_previous_hashes`.
    pub fn content_hashes(&self, folder_path: &str) -> Option<SiteHashes> {
        self.content_hashes.get(&Self::key(folder_path))
    }

    /// Recorded with the stash, under the same guard: a build that did not
    /// show leaves the previous listing, so the sweep still rebuilds for what
    /// it missed.
    pub fn record_scanned_listing(&self, folder_path: &str, listing: Vec<std::path::PathBuf>) {
        self.scanned_listing.record(Self::key(folder_path), listing);
    }

    /// `None` until a build of this folder has shown in this process.
    pub fn scanned_listing(&self, folder_path: &str) -> Option<Vec<std::path::PathBuf>> {
        self.scanned_listing.get(&Self::key(folder_path))
    }

    /// Install one completed build's publish evidence. The admission epoch is
    /// the build generation: an older build may finish later, but cannot
    /// replace a later build's answer. A clean projection still replaces a
    /// broken one because its empty vector is a completed answer.
    pub fn install_publish_preflight(&self, folder_path: &str, projection: PublishPreflightProjection) {
        self.publish_preflight.install_if_newer(Self::key(folder_path), projection);
    }

    /// Every record the publish gate reads, as the kept seal left them, for a
    /// build that ended at its entry. Each slot starts empty in a new process,
    /// and an empty slot would permit the publish.
    pub(crate) fn install_kept_seal(&self, folder_path: &str, evidence: &crate::build::inputs::SealEvidence) {
        let build_generation = crate::build::ship::next_promotion_epoch();
        self.install_publish_preflight(folder_path, PublishPreflightProjection {
            build_generation,
            missing_references: evidence.missing_references.clone(),
            unresolved_inputs: Vec::new(),
            drafts_linked: evidence.drafts_linked.clone(),
            remote_pictures: evidence.remote_pictures.clone(),
        });
        self.record_promised_dead_links(folder_path, PromisedDeadLinks::default());
        self.record_pending_derivations(folder_path, 0);
        self.record_unverifiable_moderation(folder_path, 0);
        self.record_build_end(folder_path, build_generation, BuildEnd::Publishable);
        self.record_kept(folder_path, evidence.kept.clone());
        if let Some(scripts) = &evidence.public_scripts {
            self.record_public_scripts(folder_path, scripts.clone());
        }
    }

    /// The last completed build's atomic publish evidence. `None` means no
    /// build has finished in this process — which is not a clean verdict.
    pub fn publish_preflight(&self, folder_path: &str) -> Option<PublishPreflightProjection> {
        self.publish_preflight.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec` — a video that finishes
    /// encoding between one seal and the next must have its refusal cleared
    /// by the CLEAN verdict, not left standing because nothing wrote over it.
    pub fn record_promised_dead_links(&self, folder_path: &str, promised: PromisedDeadLinks) {
        self.promised_dead_links.record(Self::key(folder_path), promised);
    }

    /// What the last seal of `folder_path` found among its own unfulfilled
    /// promises. `None` means no seal has recorded a verdict — not "clean",
    /// the same distinction the preflight projection draws.
    pub fn promised_dead_links(&self, folder_path: &str) -> Option<PromisedDeadLinks> {
        self.promised_dead_links.get(&Self::key(folder_path))
    }

    /// Always called by a render, zero included, so a rebuild that finds the
    /// text clears the refusal.
    pub fn record_pending_derivations(&self, folder_path: &str, count: usize) {
        self.pending_derivations.record(Self::key(folder_path), count);
    }

    /// `None` means no render has recorded an answer yet.
    pub fn pending_derivations(&self, folder_path: &str) -> Option<usize> {
        self.pending_derivations.get(&Self::key(folder_path))
    }

    /// How many moderation events the last build found with no trusted key
    /// verifying any of them; zero when the log verifies or comments are off.
    pub fn record_unverifiable_moderation(&self, folder_path: &str, events: usize) {
        self.unverifiable_moderation.record(Self::key(folder_path), events);
    }

    pub fn unverifiable_moderation(&self, folder_path: &str) -> Option<usize> {
        self.unverifiable_moderation.get(&Self::key(folder_path))
    }

    /// `build_generation` orders the answers: a failed build can end after the
    /// build that followed it sealed, and must not overwrite that answer.
    pub(crate) fn record_build_end(&self, folder_path: &str, build_generation: u64, end: BuildEnd) {
        self.build_end.update(Self::key(folder_path), |newest| {
            if newest.as_ref().is_none_or(|(generation, _)| *generation <= build_generation) {
                *newest = Some((build_generation, end));
            }
        });
    }

    /// `None` means no build of the folder has ended in this process.
    pub(crate) fn build_end(&self, folder_path: &str) -> Option<BuildEnd> {
        self.build_end.get(&Self::key(folder_path)).flatten().map(|(_, end)| end)
    }

    /// The newest build failed, so the generation on offer to a publish is
    /// from a build before it. A host can build again before it publishes.
    pub fn last_build_failed(&self, folder_path: &str) -> bool {
        self.build_end(folder_path) == Some(BuildEnd::Failed)
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

    /// Always called by a seal, empty included, so a rebuild that embeds the
    /// picture again clears the report.
    pub fn record_kept(&self, folder_path: &str, kept: Vec<(String, String)>) {
        self.kept.record(Self::key(folder_path), kept);
    }

    pub fn kept(&self, folder_path: &str) -> Vec<(String, String)> {
        self.kept.get(&Self::key(folder_path)).unwrap_or_default()
    }

    /// Always called by a build, empty included, so moving the data out clears it.
    pub(crate) fn record_public_scripts(&self, folder_path: &str, scripts: Vec<String>) {
        self.public_scripts.record(Self::key(folder_path), scripts);
    }

    /// `None` means no build of the folder has recorded a list, which is not "none".
    pub(crate) fn public_scripts(&self, folder_path: &str) -> Option<Vec<String>> {
        self.public_scripts.get(&Self::key(folder_path))
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

    pub(crate) fn accept_stale_content(&self, folder: &str, stale: BTreeMap<String, String>) {
        self.accepted_stale.update(Self::key(folder), |accepted| *accepted = stale);
    }

    pub(crate) fn unaccepted_stale_content(&self, folder: &str, stale: BTreeMap<String, String>) -> Vec<String> {
        let accepted = self.accepted_stale.get(&Self::key(folder)).unwrap_or_default();
        stale.into_iter().filter(|(path, pair)| accepted.get(path) != Some(pair)).map(|(path, _)| path).collect()
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

    /// Announcers omit accepted paths while retaining the seal's full removal record.
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

    /// Every slot the publish gate reads, as a new process finds them.
    #[cfg(test)]
    pub(crate) fn forget_folder(&self, folder_path: &str) {
        let key = Self::key(folder_path);
        self.publish_preflight.forget(&key);
        self.promised_dead_links.forget(&key);
        self.pending_derivations.forget(&key);
        self.unverifiable_moderation.forget(&key);
        self.build_end.forget(&key);
        self.removals.forget(&key);
        self.kept.forget(&key);
        self.public_scripts.forget(&key);
    }

    pub fn record_late_redirects(&self, folder_path: &str, late: crate::build::feeds::kept_addresses::LateRedirects) {
        self.late_redirects.record(Self::key(folder_path), late);
    }

    pub fn late_redirects(&self, folder_path: &str) -> crate::build::feeds::kept_addresses::LateRedirects {
        self.late_redirects.get(&Self::key(folder_path)).unwrap_or_default()
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
            missing_references: vec![crate::build::types::ReferenceOccurrence {
                source_path: "page.md".to_string(),
                source_revision: crate::build::types::SourceRevision::from_source("page"),
                reference: reference.to_string(),
                source_span: crate::build::types::SourceSpan {
                    start_byte: 0,
                    end_byte: 1,
                    line: 1,
                },
            }],
            unresolved_inputs: vec![],
            drafts_linked: vec![],
            remote_pictures: vec![],
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

    #[test]
    fn ordered_preflight_keeps_pending_inputs_until_a_cleaner_newer_attempt() {
        let records = BuildRecords::default();
        let folder = "/tmp/ordered-cloud-vault";
        let mut pending = projection(2, "unused");
        pending.missing_references.clear();
        pending.unresolved_inputs = vec!["about.md".into()];
        records.install_publish_preflight(folder, pending);
        let mut old_clean = projection(1, "unused");
        old_clean.missing_references.clear();
        records.install_publish_preflight(folder, old_clean);
        assert_eq!(records.publish_preflight(folder).unwrap().unresolved_inputs, ["about.md"]);

        let mut new_clean = projection(3, "unused");
        new_clean.missing_references.clear();
        records.install_publish_preflight(folder, new_clean);
        assert!(records.publish_preflight(folder).unwrap().unresolved_inputs.is_empty());
    }

    #[test]
    fn a_failed_build_that_ends_late_cannot_replace_the_newer_builds_answer() {
        let records = BuildRecords::default();
        let folder = "/tmp/a-vault";
        assert!(!records.last_build_failed(folder), "no build yet is not a failed build");

        records.record_build_end(folder, 2, BuildEnd::Publishable);
        records.record_build_end(folder, 1, BuildEnd::Failed);
        assert!(!records.last_build_failed(folder));

        records.record_build_end(folder, 3, BuildEnd::Failed);
        assert!(records.last_build_failed(folder));
    }
}
