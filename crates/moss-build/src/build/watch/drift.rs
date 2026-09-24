//! Does the disk still match what we built? The sweep's verdict, judged
//! against the manifest baseline.
//!
//! "Piece 1 — the sweep": the walk itself — walkdir, cloud-stub mapping,
//! the pass deadline — is the app-side driver's job
//! (`build_shell::watch::sweep`); this module owns only the *comparison*:
//! given what the walk found and what the manifest promised, is anything
//! different? It names no tauri and travels with the rest of `build/watch`.
//!
//! Three rules carry the safety of the whole design:
//!
//! - **The verdict function is the existing one.** Every byte-level question
//!   goes through [`source_metadata_matches_at`] — the shipped three-tier
//!   compare, with the manifest's racily-clean clock armed. A raw size+mtime
//!   compare here would reintroduce the exact iCloud pathology the
//!   content-hash gate was built for (providers touch mtimes without
//!   changing bytes → rebuild loop forever).
//! - **Evicted is "offline, not changed"** — for files AND directories. A
//!   dataless file must never read as a mismatch, and an unenumerable
//!   dataless directory must read as "offline subtree", never as "everything
//!   under it was deleted" (a deletion verdict here would remove pages —
//!   already documented history in moss's icloud code).
//! - **A deletion is proven, never inferred from walk coverage.** A manifest
//!   entry the walk did not yield is re-stat'd directly and counts as drift
//!   only when it is *positively* gone (`is_definitely_absent`): anything
//!   else — a walk-scope gap, a dataless stub, a permission error — must not
//!   become a rebuild that repeats every interval.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

use super::{source_metadata_verdict, SourceVerdict};

/// Which manifest stands as this pass's baseline, decided by generation.
///
/// [`detect_drift`] reads absence from the baseline's KEY SET as a CREATE, so
/// it is sound only against a manifest at least as new as what the screen
/// shows. Two stores describe that: the in-memory `stash` (the shown build's
/// main phase — current, but naming no asset the deferred phase has not
/// folded in yet) and the `sealed` manifest on disk (complete, but written
/// only when the seal tail lands, which can be a whole build later since the
/// tail queues behind the next build's stage-write span). Both carry the one
/// `captured_at` minted for their build (`PendingManifest::new`), so equal
/// clocks mean the same build, not the same second by luck.
///
/// Neither store alone is a baseline, and neither is a key set from the
/// older one with values from the newer: a page created since that seal is
/// judged a CREATE → `Structural` → a full rebuild that changes nothing. So
/// while the disk is behind the screen there is no verdict. That pause spans
/// the deferred media phase plus the seal tail — seconds on a text edit, the
/// encode time on a media-heavy build — and ends by itself when the manifest
/// lands; a seal that fails leaves drift suspended until the next build
/// seals, which is also the only thing that could repair it. A manifest
/// without a clock (pre-field) counts as behind any stash that has one, and
/// a manifest NEWER than the stash — one this process never stashed: an app
/// restart, or another moss process building the same vault — is the newest
/// truth and is judged as is.
///
/// `sealed` is read lazily: when the stash still matches `cache` (the
/// previous pass's copy, carrying the sweep's stat-identity refreshes)
/// nothing has landed since, the cache stands, and a site-sized manifest is
/// not re-read every pass to rediscover that. The cache also wins over an
/// equal-generation reread so those refreshes survive.
pub fn select_baseline(
    stash: Option<&SiteHashes>,
    cache: Option<SiteHashes>,
    sealed: impl FnOnce() -> Option<SiteHashes>,
) -> Option<SiteHashes> {
    if let (Some(s), Some(c)) = (stash, cache.as_ref()) {
        if s.captured_at == c.captured_at {
            return cache;
        }
    }
    let sealed = sealed()?;
    if stash.is_some_and(|s| s.captured_at > sealed.captured_at) {
        log::debug!(
            target: "moss::build::watch",
            "sweep: the sealed manifest is behind the screen (seal landing) — no drift verdict this pass"
        );
        return None;
    }
    match cache {
        Some(c) if c.captured_at == sealed.captured_at => Some(c),
        _ => Some(sealed),
    }
}

/// One file the sweep's walk yielded, keyed the manifest's way
/// (`path_to_relative_key` — the one normalizer; see the invariant tests).
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Manifest key: forward-slash, folder-relative.
    pub rel: String,
    /// Absolute path on disk.
    pub abs: PathBuf,
    /// The walk found the entry but its bytes are in the cloud (dataless /
    /// pre-Sonoma stub). Offline, not changed: exempt from both the
    /// modification compare and the deletion check.
    pub offline: bool,
    /// The stat the walk already paid for. The compare's fast tiers and the
    /// sticky-drift fingerprint read this instead of stat'ing the file a
    /// second and third time per pass — on a cloud root the walk runs every
    /// 2s, so a redundant stat here is a redundant stat on every file every
    /// pass. `None` (stub-target or stat failure) falls back to a fresh stat.
    pub meta: Option<std::fs::Metadata>,
}

/// One drifted path's stat, condensed: size, mtime at FULL nanosecond
/// resolution, and the ctime/inode identity.
///
/// This is the sticky-suppression key — "has this file changed AGAIN since
/// the dispatch that was supposed to clear it". Seconds-granular mtime alone
/// would wrongly suppress a same-size, same-second re-edit (the exact
/// blindness `mtime_nanos` closed in the content-hash gate), leaving the
/// preview stale until the file changed once more. All zeroes for a path
/// with no walk stat — a deletion, or a file that vanished mid-look — which
/// is right: a reappearance moves the stamp.
pub type DriftStamp = (u64, i64, u32, Option<i64>, Option<u64>);

/// What drifting means for one path. The distinction the sweep's dispatch
/// turns on: handing a create or a delete to `ContentOnly` would let the
/// incremental gates open on a batch that can move the site's structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftKind {
    /// Absent from the baseline, or probed `Gone` — the site's shape moved.
    Structural,
    /// `SourceVerdict::Changed` — the bytes moved, the shape did not.
    Edited,
}

/// One drifted path's whole verdict: what kind of drift, and the stat to
/// notice it drifting again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drift {
    pub kind: DriftKind,
    pub stamp: DriftStamp,
}

/// What one comparison pass concluded. `drifted` is empty ⇔ the screen still
/// matches the disk (as far as a stat walk can tell).
#[derive(Debug, Default)]
pub struct DriftReport {
    /// Every manifest key (or would-be key, for a new file) that no longer
    /// matches the baseline, each carrying its own kind and stat.
    ///
    /// **One map, not a list plus parallel judgments about it.** This was a
    /// `Vec<String>` beside a whole-report `structural: bool`, with the stats
    /// rebuilt afterwards by a second pass over the walk. Both of those
    /// out-of-band judgments produced a defect on the same vault, and both
    /// were the same defect: the sweep dispatches a SUBSET of `drifted`
    /// (sticky suppression is per path), and a judgment that does not travel
    /// with its path cannot be subsetted. The flag made a filtered batch
    /// inherit `Structural` from a path it no longer contained; the set-wide
    /// fingerprint made one path joining or leaving re-dispatch every path in
    /// the set, so a set that oscillates suppressed nothing — 877 ↔ 1102
    /// drifted paths, minutes of continuous rebuilding, and the guard's WARN
    /// never fired once.
    ///
    /// Sorted, because the fingerprint it doubles as is compared across
    /// passes and the log line names `first()`.
    pub drifted: std::collections::BTreeMap<String, Drift>,
    /// Entries skipped as offline — diagnostics only.
    pub offline: usize,
    /// Hash-confirmed matches whose stat identity moved (provider
    /// re-materialization): the fresh records to write back into the
    /// baseline so the hash cost is paid once, not once per pass.
    pub refreshed: Vec<(String, SourceMetadata)>,
    /// The compare ran out of the pass deadline. `drifted` covers only the
    /// files judged before the cutoff — those verdicts are real and usable;
    /// what is missing is coverage of the rest, so the caller must not treat
    /// the pass as a full-tree certification.
    pub deadline_blown: bool,
    /// The last file fully judged before a blown deadline — the caller's
    /// resume cursor, so a compare that keeps blowing at the same budget
    /// still advances across passes instead of re-judging the same head of
    /// the tree forever.
    pub judged_through: Option<String>,
}

impl DriftReport {
    pub fn is_quiet(&self) -> bool {
        self.drifted.is_empty()
    }

    /// Does any of `paths` move the site's structure? Asked of the batch
    /// actually being dispatched, never of the whole report — that
    /// distinction is the defect this shape exists to make unrepresentable.
    pub fn any_structural<'a>(&self, paths: impl IntoIterator<Item = &'a String>) -> bool {
        paths
            .into_iter()
            .any(|p| self.drifted.get(p).is_some_and(|d| d.kind == DriftKind::Structural))
    }
}

/// Condense a walk's stat into a [`DriftStamp`]. All zeroes when the walk
/// carried none — see the type's docs.
pub fn stamp_of(meta: Option<&std::fs::Metadata>) -> DriftStamp {
    let Some(m) = meta else {
        return (0, 0, 0, None, None);
    };
    use std::time::UNIX_EPOCH;
    let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let (ctime, inode) = crate::build::types::stat_identity(m);
    (
        m.len(),
        mtime.map(|d| d.as_secs() as i64).unwrap_or(0),
        mtime.map(|d| d.subsec_nanos()).unwrap_or(0),
        ctime,
        inode,
    )
}

/// Compare the walk's findings against the manifest baseline.
///
/// `offline_prefixes` are folder-relative keys of directories the walk could
/// not enumerate for cloud reasons; every manifest entry under one is exempt
/// from the deletion check (offline subtree, not a mass delete).
///
/// `probe` answers "what is at this absolute path right now?" for manifest
/// entries the walk did not yield — injected so the deletion rule is testable
/// without a cloud provider. Production passes [`probe_missing`].
///
/// `deadline`: the compare's own budget. The compare does real I/O — its
/// hash tier reads whole files, the probe lstats — so on the filesystems the
/// pass deadline exists for it can stall exactly like the walk. Checked per
/// file; on exceed the report carries what was judged plus a resume cursor.
///
/// `probe_deletions`: the deletion loop runs only over a FULL walk — a
/// partial (resumed or blown) walk's absence set says nothing about
/// deletions, and probing from it would re-probe the whole manifest on
/// every shard of a multi-pass sweep.
pub fn detect_drift(
    baseline: &SiteHashes,
    walked: &[WalkedFile],
    offline_prefixes: &[String],
    folder: &Path,
    probe: impl Fn(&Path) -> MissingProbe,
    deadline: Option<Instant>,
    probe_deletions: bool,
) -> DriftReport {
    let mut report = DriftReport::default();
    let mut seen: HashSet<&str> = HashSet::with_capacity(walked.len());

    for file in walked {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            report.deadline_blown = true;
            return report;
        }
        report.judged_through = Some(file.rel.clone());
        seen.insert(file.rel.as_str());
        if file.offline {
            report.offline += 1;
            continue;
        }
        match baseline.sources.get(&file.rel) {
            // New file: the manifest cannot represent a file that did not
            // exist last build, so presence alone is drift. The walk's filter
            // guarantees this set is ⊆ what a fresh build's manifest will
            // hold (the walk-⊆-manifest invariant test), so this cannot
            // become a permanent mismatch.
            None => {
                // A file the manifest cannot represent yet: a CREATE.
                report.drifted.insert(
                    file.rel.clone(),
                    Drift { kind: DriftKind::Structural, stamp: stamp_of(file.meta.as_ref()) },
                );
            }
            Some(meta) => {
                let stat;
                let md = match &file.meta {
                    Some(m) => Some(m),
                    None => {
                        stat = std::fs::metadata(&file.abs).ok();
                        stat.as_ref()
                    }
                };
                let verdict = match md {
                    Some(md) => {
                        source_metadata_verdict(meta, md, &file.abs, baseline.captured_at)
                    }
                    None => SourceVerdict::Unknown,
                };
                match verdict {
                    SourceVerdict::Changed => {
                        report.drifted.insert(
                            file.rel.clone(),
                            Drift { kind: DriftKind::Edited, stamp: stamp_of(file.meta.as_ref()) },
                        );
                    }
                    SourceVerdict::Unchanged { refreshed: Some(fresh) } => {
                        report.refreshed.push((file.rel.clone(), fresh));
                    }
                    SourceVerdict::Unchanged { refreshed: None } => {}
                    // Unknown = the stat or read failed under our feet. The
                    // gate fails OPEN on this (a missed rebuild is worse than
                    // a spare one, once). The sweep must fail QUIET instead:
                    // it re-runs every interval, so a transient error
                    // self-heals next pass, while failing open on a
                    // *persistent* error (permissions, provider refusal)
                    // would rebuild in a loop forever without ever fixing
                    // anything.
                    SourceVerdict::Unknown => {
                        report.offline += 1;
                        log::debug!(
                            target: "moss::build::watch",
                            "sweep: could not judge '{}' this pass — skipping, next sweep retries",
                            file.rel
                        );
                    }
                }
            }
        }
    }

    if !probe_deletions {
        return report;
    }

    // Deletions: manifest entries the walk did not yield.
    for rel in baseline.sources.keys() {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            report.deadline_blown = true;
            return report;
        }
        if seen.contains(rel.as_str()) {
            continue;
        }
        if offline_prefixes.iter().any(|p| is_under_prefix(rel, p)) {
            report.offline += 1;
            continue;
        }
        match probe(&folder.join(rel)) {
            MissingProbe::Gone => {
                // Gone: no walk entry, so no stat — the zero stamp, and a
                // reappearance moves it.
                report.drifted.insert(
                    rel.clone(),
                    Drift { kind: DriftKind::Structural, stamp: stamp_of(None) },
                );
            }
            // Present: the walk's scope simply does not cover this entry
            // (or it reappeared between walk and probe) — never drift.
            // Offline: dataless / pre-Sonoma stub / fail-fast read error —
            // offline is not absent.
            MissingProbe::Present | MissingProbe::Offline => {}
        }
    }

    report
}

/// What a direct look at a manifest entry's path found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MissingProbe {
    /// Something is there (any file type — the compare pass owns content).
    Present,
    /// Positively gone: `NotFound` with no cloud placeholder standing in.
    Gone,
    /// Unreadable for cloud reasons, or an error that is not proof of
    /// absence. Waiting may help; deleting pages would not.
    Offline,
}

/// The production probe: one `lstat`, classified by the icloud discipline
/// ("unreadable is not absent").
pub fn probe_missing(abs: &Path) -> MissingProbe {
    match std::fs::symlink_metadata(abs) {
        Ok(_) => MissingProbe::Present,
        Err(e) if crate::build::icloud::is_definitely_absent(abs, &e) => MissingProbe::Gone,
        Err(_) => MissingProbe::Offline,
    }
}

fn is_under_prefix(rel: &str, prefix: &str) -> bool {
    rel == prefix || rel.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
#[path = "drift_tests.rs"]
mod tests;
