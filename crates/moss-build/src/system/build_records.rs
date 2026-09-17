//! What one build of a folder leaves behind for the next one to read.
//!
//! # Why this is process-global rather than app-managed
//!
//! These two records — the last build's in-memory content hashes and its
//! missing-media verdict — are what moss shares *between builds of the same
//! folder*. Until 2026-08-29 they were fields on `AppState`, reached through
//! `app.manage`, so the only process that could write or read them was one
//! with a `tauri::AppHandle`. The headless arms of the host seam were
//! therefore no-ops, with two measured consequences (moss#1116):
//!
//! - `moss build --serve --watch` had no race-free refresh baseline, so every
//!   rebuild diffed against the asynchronously-sealed `hashes.json` and
//!   suppressed refreshes for edits that had in fact changed the output.
//! - No CLI-driven test could observe any state moss shares between builds,
//!   which is why the CLI and the app drifted apart unfalsifiably.
//!
//! Neither record has a Tauri type in it, and neither describes a *window* —
//! they describe a *folder*, on the folder's own timeline. Same reasoning that
//! moved `FolderSessionRegistry` out on 2026-08-24; this module is its
//! sibling. Being process-global is also what deletes the divergence rather
//! than relocating it: there is no second arm for a headless host to answer
//! differently, because the host is not asked.
//!
//! Keyed by folder path string after normalization — this module normalizes all
//! callers' input so keys are consistent regardless of path-separator style. Before
//! normalization, callers had to remember to normalize themselves, which was
//! unreliable on Windows where a path can mix separators (one source yields
//! backslashes, another forward slashes). A raw-string reader and a canonicalized
//! writer could produce mismatched keys, leaving build verdicts unreadable to
//! later callers. The module now owns the normalization, every caller lands on the
//! same key automatically.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::build::manifest::link_audit::DeadLink;
use crate::build::types::MissingMedia;
use crate::types::content::SiteHashes;

/// One per-folder verdict, replaced wholesale on every write, never merged.
/// `None` on read means no build has answered yet in this process — distinct
/// from "clean" (an empty `Vec`, or whatever value a build genuinely
/// produced). `content_hashes`, `missing_media` and `promised_dead_links`
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

/// The per-folder records the build tail writes and the watcher reads.
#[derive(Default)]
pub struct BuildRecords {
    content_hashes: FolderSlot<SiteHashes>,
    missing_media: FolderSlot<Vec<MissingMedia>>,
    /// This seal's own still-pending promises the link audit caught dead —
    /// see `link_audit::dead_links_among_promises` and `refuse_publish`.
    promised_dead_links: FolderSlot<Vec<DeadLink>>,
    /// Structural sources (a page, `config.toml`, the user stylesheet) the
    /// last build of this folder had to carry forward rather than read — see
    /// `cloud_ledger::structural_stale_paths` and `refuse_publish`.
    stale_sources: FolderSlot<Vec<String>>,
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

    /// Always called, including with an empty `Vec` — that is how a fixed
    /// reference stops blocking a publish. Writing only on failure would
    /// leave the last broken build's verdict standing forever, and there is
    /// deliberately no override.
    pub fn record_missing_media(&self, folder_path: &str, missing: Vec<MissingMedia>) {
        self.missing_media.record(Self::key(folder_path), missing);
    }

    /// What the last build of `folder_path` found missing. `None` means no
    /// build has finished in this process — which is NOT "clean".
    pub fn missing_media(&self, folder_path: &str) -> Option<Vec<MissingMedia>> {
        self.missing_media.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec` — a video that finishes
    /// encoding between one seal and the next must have its refusal cleared
    /// by the CLEAN verdict, not left standing because nothing wrote over it.
    pub fn record_promised_dead_links(&self, folder_path: &str, dead: Vec<DeadLink>) {
        self.promised_dead_links.record(Self::key(folder_path), dead);
    }

    /// What the last seal of `folder_path` found among its own unfulfilled
    /// promises. `None` means no seal has recorded a verdict — not "clean",
    /// the same distinction `missing_media` draws.
    pub fn promised_dead_links(&self, folder_path: &str) -> Option<Vec<DeadLink>> {
        self.promised_dead_links.get(&Self::key(folder_path))
    }

    /// Always called, including with an empty `Vec` — the same reasoning as
    /// `record_missing_media`: a source that arrives must clear the refusal,
    /// and only an unconditional write does that.
    pub fn record_stale_sources(&self, folder_path: &str, stale: Vec<String>) {
        self.stale_sources.record(Self::key(folder_path), stale);
    }

    /// What the last build of `folder_path` carried forward rather than read.
    /// `None` means no build has finished in this process — which is NOT
    /// "clean", the same distinction `missing_media` draws.
    pub fn stale_sources(&self, folder_path: &str) -> Option<Vec<String>> {
        self.stale_sources.get(&Self::key(folder_path))
    }

    /// Drop the content-hash baseline for a folder moss is no longer
    /// watching: returning to the folder falls back to disk for the first
    /// rebuild, which is correct — a baseline never compared against a live
    /// screen is not one.
    pub fn forget_content_hashes(&self, folder_path: &str) {
        self.content_hashes.forget(&Self::key(folder_path));
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

}
