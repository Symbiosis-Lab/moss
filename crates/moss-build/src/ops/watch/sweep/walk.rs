//! The sweep's stat walk: one pass over the vault's disk state, and the
//! baseline it compares against.
//!
//! Split out of `sweep.rs` by responsibility (the size gate's own rule):
//! everything here is the MEASUREMENT half — what is on disk, what is still
//! in the cloud, what the last build recorded — with no loop-lifecycle state
//! of its own. `sweep.rs`'s `run()` loop is the only caller.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::build::scan::classify;
use crate::build::watch::drift;
use crate::build::watch::path_to_relative_key;
use crate::build::watch::scope;

use super::WALK_DEADLINE;

// ---------------------------------------------------------------------------
// The walk: one stat pass, three verdicts' raw material
// ---------------------------------------------------------------------------

/// What one stat walk of the source tree found. One pass feeds all three
/// verdicts: `dataless` → the cloud readers, `files`/`offline_prefixes` →
/// the drift compare, `root_unreadable`/`deadline_blown` → the unavailable
/// counter.
#[derive(Debug, Default)]
pub(crate) struct WalkOutcome {
    /// Source files still in the cloud — the download-request set
    /// ([`should_request`]'s materialization filter, NOT the drift filter).
    pub dataless: Vec<PathBuf>,
    /// Drift-eligible files on disk, keyed the manifest's way.
    pub files: Vec<drift::WalkedFile>,
    /// Folder-relative keys of directories the walk could not enumerate for
    /// cloud reasons — offline subtrees, exempt from the deletion check.
    pub offline_prefixes: Vec<String>,
    /// The vault root itself would not enumerate.
    pub root_unreadable: bool,
    /// The pass ran out of deadline mid-walk. Partial results are still
    /// usable: `dataless` feeds downloads and `files` feeds a
    /// modification-only compare; what a partial pass must NOT do is claim
    /// full-tree authority (deletions, pending-set replacement).
    pub deadline_blown: bool,
    /// Files the pass looked at, and how many the watcher may watch at all
    /// (both excluding `.moss/`). Seen-but-none-watchable is a folder moss is
    /// blind to; `supervision::note_blind_folder` is what says so.
    pub files_seen: usize,
    pub files_watchable: usize,
    /// Where to pick up next pass: the last file processed before the blow.
    /// `None` when the walk reached the end. Without this, a filesystem
    /// where every pass blows at the same budget point never checks the deep
    /// half of the tree for the life of the session (the incident's file was
    /// 8 directories deep).
    pub resume: Option<String>,
}

/// Whether the sweep should descend into a directory named `name` at
/// vault-relative path `rel`.
///
/// `is_excluded_dir_name` alone would be wrong here, because it excludes
/// every dot-prefixed name and that includes `.moss/` — the directory
/// holding `config.toml`, `theme/`, `data/`, `assets/`, `identity/` and
/// `keys/`, none of which moss can regenerate and all of which stop
/// something dead when they are offline. Pruning it left exactly the files
/// with the highest blast radius as the only ones nobody was reading.
///
/// The rest of `.moss/` stays pruned. It is moss's own output —
/// `build/generations/` alone runs to thousands of files — and re-reading
/// derived bytes moss can regenerate would spend the whole budget on them
/// (under `.moss/build.nosync/`, dataless is *absent*).
///
/// **The allowlist is `MOSS_PATH_RULES.materialized`, not the watcher's
/// `watched`.** Those answer different questions, and conflating them cost a real bug: nothing rebuilds when `.moss/identity/secret-key`
/// changes, so it is not watched — and it is the one file publish cannot
/// proceed without, so it must be materialized.
pub(crate) fn should_descend(rel: &str, name: &str) -> bool {
    // The walk root, whose name is the user's choice and not ours to judge —
    // a vault living at `~/Documents/.private-site` still gets swept.
    if rel.is_empty() {
        return true;
    }
    if rel.strip_prefix(".moss").is_none() {
        return !crate::build::scan::classify::is_excluded_dir_name(name);
    }
    // `.moss` itself: descend, so the allowlisted children below are reachable.
    if rel == ".moss" {
        return true;
    }
    // A directory is worth descending into if it is materialized itself, or
    // if it is a PARENT of something materialized — `.moss/data` is
    // materialized as a whole, `.moss/build.nosync` is not.
    crate::moss_paths::is_materialized_rel(rel)
}

/// Whether the sweep should ask the provider for this file (the download
/// half — distinct from [`drift_eligible`], the compare half).
///
/// Two disjoint populations, pruned for different reasons:
///
/// - **Outside `.moss/`** the watcher's filter is exactly right, and reusing
///   it is what keeps moss from reading files no build will ever want
///   (`.git/`, `node_modules/`, a stray `.DS_Store`).
/// - **Inside `.moss/`** the watcher's filter is the wrong question — it
///   answers "would an edit here rebuild the site?", and the answer for the
///   identity key is no. The registry's `materialized` flag
///   answers "must moss be able to read these bytes?" instead.
///
/// This is the ONE owner of the cloud-request decision; the cloud
/// supervisor it moved from is gone.
pub(crate) fn should_request(folder: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(folder) else {
        return false;
    };
    let rel = moss_core::slug::normalize_separators(&rel.to_string_lossy());
    if rel.starts_with(".moss/") {
        return crate::moss_paths::is_materialized_rel(&rel);
    }
    // Both predicates take the root and judge only what is inside it — a
    // dot-prefixed ancestor (a vault synced under a hidden folder of the provider)
    // must not condemn the vault. A nested site is left out like the scan
    // leaves it out: it is downloaded when THAT site is opened.
    scope::path_is_watchable(folder, path) && scope::path_passes_filter(folder, path)
}

/// Whether a file participates in the DRIFT compare — the same population
/// the build's scan consumes into the manifest's `sources` map, pinned by
/// the walk-⊆-scan invariant test rather than by hand upkeep. Narrower than
/// [`should_request`] on purpose:
///
/// - `.moss/` inputs (config, theme) are excluded: the manifest never
///   carries them, so "not in baseline" would read as new-file drift and
///   rebuild every pass forever. The watcher still covers them (advisory),
///   and their arrival from the cloud is still requested above.
/// - Root files moss writes itself (`AGENTS.md` and friends) are excluded
///   for the same reason the pump drops them.
/// - Root `style.css` / `script.js` are excluded: the asset pass
///   (`build::media::pipeline`) deliberately never copies them — the
///   canonical location is `.moss/theme/` — so neither lands in `sources`
///   nor `files`. Judging them here would drift every pass forever exactly
///   like the excluded cases above. Shares the one predicate
///   the asset pass uses, so the two cannot independently drift apart.
///
/// Self-write suppression (below) has no scan analog and stays hand-composed.
/// The extension vote now asks `classify::classify_extension` first, closing
/// gaps the old `path_passes_filter`-only list had (no
/// `.html`/`.pages`/`.docx`/`.doc`). Widens only — `path_passes_filter` still
/// owns the `Other`-bucket assets it has no structured answer for, and
/// `.xyz` still loses on both paths.
///
/// The walk-⊆-manifest invariant test pins this mirror against the real
/// scan on a fixture tree.
pub(crate) fn drift_eligible(folder: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(folder) else {
        return false;
    };
    let rel = moss_core::slug::normalize_separators(&rel.to_string_lossy());
    if rel == ".moss" || rel.starts_with(".moss/") {
        return false;
    }
    if crate::build::render::is_ignored_root_theme_file(&rel) {
        return false;
    }
    if scope::all_paths_moss_written(folder, std::slice::from_ref(&path.to_path_buf())) {
        return false;
    }
    let recognized = path.extension().and_then(|e| e.to_str()).is_some_and(|ext| {
        !matches!(classify::classify_extension(&ext.to_lowercase()), classify::ScanBucket::Other)
    });
    scope::path_is_watchable(folder, path) && (recognized || scope::path_passes_filter(folder, path))
}

/// Whether THIS pass is the one that should log "no baseline this pass" —
/// the first pass of a streak, not every pass in it. `passes_without_baseline`
/// is the post-increment count for the CURRENT pass (1 on the first pass of a
/// new streak), so a fresh streak always logs exactly once regardless of how
/// long the previous streak ran or how it ended.
///
/// Pure function so the "once per streak" gate is unit-testable without
/// driving the whole `run()` loop.
pub(crate) fn should_log_no_baseline_transition(passes_without_baseline: u64) -> bool {
    passes_without_baseline == 1
}

/// Walk `folder` once, stat-only, under `deadline`.
///
/// This is the sweep's measurement — the authority on what has not arrived
/// AND on what is present to compare. Download ordering is the provider's
/// job; the walk order is nevertheless DETERMINISTIC
/// (`sort_by_file_name`) because `resume_after` shards a too-slow tree
/// across passes, and resuming only means anything in a stable order. While
/// fast-forwarding to the cursor the per-file work (eviction probe, stat,
/// filters) is skipped — the point of resuming is not to re-spend the
/// budget on the half already covered. Panic-proof by construction: no
/// unwraps in the loop body; enumeration errors are classified, never
/// propagated.
pub(crate) fn walk(folder: &Path, deadline: Option<Instant>, resume_after: Option<&str>) -> WalkOutcome {
    let mut out = WalkOutcome::default();

    if std::fs::read_dir(folder).is_err() {
        out.root_unreadable = true;
        return out;
    }

    // `None` once the cursor has been passed (or was never set).
    let mut skipping_until = resume_after;
    let mut last_processed: Option<String> = None;

    for entry in walkdir::WalkDir::new(folder).sort_by_file_name().into_iter().filter_entry(|e| {
        if !e.file_type().is_dir() {
            return true;
        }
        let rel = e
            .path()
            .strip_prefix(folder)
            .map(|r| moss_core::slug::normalize_separators(&r.to_string_lossy()))
            .unwrap_or_default();
        if !should_descend(&rel, &e.file_name().to_string_lossy()) {
            return false;
        }
        // Outside `.moss/`, the scan's rule: a nested site's downloads and
        // drift are its own.
        rel == ".moss" || rel.starts_with(".moss/") || crate::build::scan::classify::left_out_of_site(e).is_none()
    }) {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            out.deadline_blown = true;
            out.resume = last_processed.take().or_else(|| resume_after.map(String::from));
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                // An unenumerable directory. Offline (dataless subtree) must
                // read as "offline", never as "everything under it was
                // deleted"; anything else is logged and skipped — the sweep
                // re-runs, and a rebuild would not fix an unreadable dir.
                if let (Some(path), Some(io)) = (err.path(), err.io_error()) {
                    if crate::build::icloud::is_offline_not_absent(path, io) {
                        if let Some(rel) = path_to_relative_key(folder, path) {
                            out.offline_prefixes.push(rel);
                            continue;
                        }
                    }
                    log::debug!(
                        target: "moss::build::watch",
                        "sweep: cannot enumerate {}: {}",
                        path.display(),
                        io
                    );
                }
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // Fast-forward: everything up to and including the cursor was
        // covered by the previous pass — no eviction probe, no stat, no
        // filter work. If the cursor entry vanished between passes the skip
        // runs to the end and the pass covers nothing; the NEXT pass starts
        // from the top and covers the deletion, which is itself the change.
        if let Some(cursor) = skipping_until {
            let rel = path
                .strip_prefix(folder)
                .map(|r| moss_core::slug::normalize_separators(&r.to_string_lossy()))
                .unwrap_or_default();
            if rel == cursor {
                skipping_until = None;
            }
            continue;
        }
        // The resume cursor: the raw entry's rel, in walk order (stub name,
        // not its target — a resumed walk compares against raw entries).
        last_processed = path
            .strip_prefix(folder)
            .ok()
            .map(|r| moss_core::slug::normalize_separators(&r.to_string_lossy()));
        // The pre-Sonoma stub form carries the real name; everything
        // downstream has to see that name rather than `.name.icloud`.
        let real = crate::build::icloud::icloud_stub_target(path);
        let effective = real.as_deref().unwrap_or(path);
        let offline = real.is_some() || crate::build::icloud::is_evicted(path);
        if offline && should_request(folder, effective) {
            out.dataless.push(effective.to_path_buf());
        }
        // Blind-folder counters; `.moss/` is moss's own, so off both sides.
        if !effective.strip_prefix(folder).is_ok_and(|r| r.starts_with(".moss")) {
            out.files_seen += 1;
            if scope::path_is_watchable(folder, effective) {
                out.files_watchable += 1;
            }
        }
        if drift_eligible(folder, effective) {
            if let Some(rel) = path_to_relative_key(folder, effective) {
                // Carry the stat: the drift compare's fast tiers and the
                // sticky fingerprint reuse it instead of re-stat'ing every
                // file up to twice more per pass.
                let meta = if offline { None } else { entry.metadata().ok() };
                out.files.push(drift::WalkedFile {
                    rel,
                    abs: effective.to_path_buf(),
                    offline,
                    meta,
                });
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Baseline: what the screen shows, read fail-safe
// ---------------------------------------------------------------------------

/// The in-memory half of the baseline: the AppState stash (race-free,
/// describes what the screen shows). Memory-only — safe to call on the
/// runtime thread; the disk fallback lives inside the pass closure, where
/// its I/O sits under the deadline.
pub(crate) fn stashed_baseline(folder: &str) -> Option<crate::types::content::SiteHashes> {
    crate::system::build_records::records().content_hashes(folder)
}

/// The disk fallback, and why it is NOT `load_previous_hashes`: that helper
/// returns `SiteHashes::default()` on any failure, which is right for the
/// gate (fail open, rebuild once) and catastrophic here — an EMPTY baseline
/// makes every file on disk read as new-file drift, and if the failure is an
/// eviction of `hashes.json` itself (it lives under an evictable
/// `.moss/build.nosync/`), that is a rebuild loop. `None` means "no drift verdict
/// this pass" — the sweep re-runs, and the next successful build re-stashes.
pub(crate) fn read_baseline_from_disk(folder: &Path) -> Option<crate::types::content::SiteHashes> {
    let path = crate::moss_paths::MossPaths::new(folder).hashes();
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(h) => Some(h),
            Err(e) => {
                log::debug!(
                    target: "moss::build::watch",
                    "sweep: manifest unparsable ({}) — no drift verdict until the next build rewrites it",
                    e
                );
                None
            }
        },
        Err(e) if crate::build::icloud::is_offline_not_absent(&path, &e) => {
            log::debug!(
                target: "moss::build::watch",
                "sweep: manifest is offline — no drift verdict this pass"
            );
            None
        }
        // Genuinely absent: nothing built yet, nothing to compare against.
        Err(_) => None,
    }
}

// ---------------------------------------------------------------------------
// One pass: walk + compare, fully inside the blocking-pool deadline
// ---------------------------------------------------------------------------

/// Everything one sweep pass concluded. Produced entirely inside the pass
/// closure — walk, compare, fingerprint, refresh application — so that ALL
/// of the pass's I/O sits on the blocking pool under the deadline. Before
/// this held, the compare's hash tier (whole-file reads), the deletion
/// probes and the fingerprint stats ran unbounded on the runtime thread —
/// on a stalling FUSE/SMB/NFS mount, the exact case the deadline exists
/// for, that wedged the runtime instead of a pool thread.
#[derive(Debug, Default)]
pub(crate) struct PassOutcome {
    pub walk: WalkOutcome,
    /// The drift verdict, when a baseline was available and the compare ran.
    pub drift: Option<drift::DriftReport>,
    /// The baseline used, with this pass's stat-identity refreshes applied —
    /// the next pass's cache.
    pub baseline: Option<crate::types::content::SiteHashes>,
    /// Where the next pass should resume, `None` for start-from-top.
    pub resume: Option<String>,
    /// This pass walked top-to-end unblown: its absence set is authoritative
    /// (deletion probes ran; the pending set may be replaced rather than
    /// merely extended).
    pub complete: bool,
}

impl PassOutcome {
    pub(crate) fn blown(&self) -> bool {
        self.walk.deadline_blown || self.drift.as_ref().is_some_and(|d| d.deadline_blown)
    }
}

/// One full pass on the blocking pool, bounded three ways: the walk checks
/// a deadline between entries, the compare checks its OWN equal budget per
/// file (a fresh budget, or a walk that blows every pass would starve the
/// compare into a livelock at the same cursor), and the await is timed out
/// as a backstop for a stat blocked in the kernel. A timed-out pass leaks
/// its blocking thread — which parks on the next syscall return and exits
/// at its own deadline check — and reports blown with no resume (the
/// closure's cursor is lost with it). Panic-proof: a panicked pass is a
/// blown pass, not a dead sweep.
///
/// `drift_allowed: false` suspends the compare outright (no rebuild worker
/// registered yet — dispatching drift before the worker exists would race
/// the first build inline). `stash` is the in-memory baseline the host
/// holds (`None` from a caller with no managed state); the disk read
/// happens HERE, under the pass's I/O discipline, not on the runtime
/// thread.
pub(crate) async fn run_pass(
    folder: &Path,
    stash: Option<crate::types::content::SiteHashes>,
    cache: Option<crate::types::content::SiteHashes>,
    cursor: Option<String>,
    drift_allowed: bool,
) -> PassOutcome {
    let f = folder.to_path_buf();
    let task = tokio::task::spawn_blocking(move || {
        let walk_deadline = Instant::now() + WALK_DEADLINE;
        let started_at_top = cursor.is_none();
        let walk_out = walk(&f, Some(walk_deadline), cursor.as_deref());
        if walk_out.root_unreadable {
            return PassOutcome { walk: walk_out, baseline: cache, ..PassOutcome::default() };
        }

        let mut baseline = if drift_allowed {
            drift::select_baseline(stash.as_ref(), cache, || read_baseline_from_disk(&f))
        } else {
            // Suspended: the cache rides along untouched for the pass that
            // is allowed to compare.
            cache
        };
        let compare = drift_allowed && baseline.is_some();

        let walk_complete = started_at_top && !walk_out.deadline_blown;
        let mut drift_rep = None;
        if let (true, Some(b)) = (compare, baseline.as_mut()) {
            let compare_deadline = Instant::now() + WALK_DEADLINE;
            let rep = drift::detect_drift(
                b,
                &walk_out.files,
                &walk_out.offline_prefixes,
                &f,
                drift::probe_missing,
                Some(compare_deadline),
                walk_complete,
            );
            for (rel, fresh) in &rep.refreshed {
                b.sources.insert(rel.clone(), fresh.clone());
            }
            drift_rep = Some(rep);
        }

        let compare_blown = drift_rep.as_ref().is_some_and(|r| r.deadline_blown);
        // Resume where coverage stopped: the walk's cursor if IT blew, else
        // the compare's judged-through if the compare blew, else done.
        let resume = if walk_out.deadline_blown {
            walk_out.resume.clone()
        } else if compare_blown {
            drift_rep.as_ref().and_then(|r| r.judged_through.clone())
        } else {
            None
        };
        PassOutcome {
            complete: walk_complete && !compare_blown,
            resume,
            walk: walk_out,
            drift: drift_rep,
            baseline,
        }
    });
    match tokio::time::timeout(WALK_DEADLINE * 2 + std::time::Duration::from_secs(5), task).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(join_err)) => {
            log::error!(
                target: "moss::build::watch",
                "sweep: pass panicked ({}) — treating as a blown pass",
                join_err
            );
            PassOutcome {
                walk: WalkOutcome { deadline_blown: true, ..WalkOutcome::default() },
                ..PassOutcome::default()
            }
        }
        Err(_) => PassOutcome {
            walk: WalkOutcome { deadline_blown: true, ..WalkOutcome::default() },
            ..PassOutcome::default()
        },
    }
}
