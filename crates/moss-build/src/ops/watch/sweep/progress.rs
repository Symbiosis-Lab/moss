//! Rebuild pacing and cloud-sync progress reporting — the sweep's "tell the
//! host something" half, split out of `sweep.rs` by responsibility.
//!
//! Progress goes out through [`BuildReporter::cloud_sync`], the reporter
//! method the build pipeline already carries for this exact signal
//! (`build/ports/reporter.rs` documents the sweep as its second producer) —
//! not a new port, and not an `AppHandle`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::build::ports::reporter::{BuildReporter, CloudSync};

use super::{MIN_REBUILD_INTERVAL, REFUSED_RETRY, STALL_AFTER};

// ---------------------------------------------------------------------------
// Rebuild pacing (carried from the cloud supervisor, semantics unchanged)
// ---------------------------------------------------------------------------

/// Whether this tick should dispatch an arrival-driven rebuild. Pure; the
/// full rationale (self-tuning interval, measured-from-finish clock, first
/// batch not held back, no quiet-window shortcut) is in the tests below and
/// the design doc — this function IS the old supervisor's, moved.
pub(crate) fn should_rebuild(
    first_arrival_at: Option<Instant>,
    last_rebuild_at: Option<Instant>,
    last_rebuild_duration: Duration,
    now: Instant,
) -> bool {
    if first_arrival_at.is_none() {
        return false;
    }
    match last_rebuild_at {
        None => true,
        Some(at) => now.duration_since(at) >= MIN_REBUILD_INTERVAL.max(last_rebuild_duration),
    }
}

/// Hand the pending set to the cloud readers, so the OS downloads them —
/// the same trigger Finder uses; order, concurrency and retry are the
/// provider's business. Anything already queued or being read is
/// suppressed by the readers, so re-handing the set costs a hash lookup per
/// file. **Refused** files (in the cloud past `REFUSED_AFTER`) are the
/// exception: they ride `REFUSED_RETRY`'s slower clock instead of every
/// pass.
pub(crate) fn request_downloads(
    pending: &HashSet<PathBuf>,
    refused: &HashSet<PathBuf>,
    last_refused_ask: &mut Instant,
    now: Instant,
) {
    let ask_refused = now.duration_since(*last_refused_ask) >= REFUSED_RETRY;
    if ask_refused {
        *last_refused_ask = now;
    }
    for path in pending {
        if ask_refused || !refused.contains(path) {
            crate::build::cloud_readiness::request_download(path);
        }
    }
}

/// The stall WARN, with the pool snapshot that separates the three stalls a
/// user cannot tell apart.
pub(crate) fn report_stall(awaiting: usize) {
    let pool = crate::build::cloud_readiness::download_snapshot();
    let oldest = match &pool.oldest_read {
        Some((path, age)) => {
            format!("oldest {} outstanding {}s", path.display(), age.as_secs())
        }
        None => "no reads outstanding".to_string(),
    };
    log::warn!(
        "cloud-sync: no arrivals in {}s with {} file(s) still pending — reporting stalled \
         (pool: {} in flight, {} queued, {} finished; {})",
        STALL_AFTER.as_secs(),
        awaiting,
        pool.in_flight,
        pool.waiting,
        pool.done,
        oldest
    );
}

/// Three phases, and which one this is turns on what is left rather than on
/// how long it has been. `unavailable` is reported even though the download
/// is, in every sense that matters to the site, finished: the user is owed
/// the difference between "moss is still fetching this" and "this file is
/// missing and always will be".
pub(crate) fn emit_progress(
    reporter: &dyn BuildReporter,
    folder: &std::path::Path,
    total: usize,
    remaining: usize,
    blocking: usize,
    stalled: bool,
    refused: &[&PathBuf],
) {
    // No window, no browser, nobody to tell: skip the per-file name-trimming
    // work below, exactly the gate `cloud_readiness.rs` and `pipeline.rs`
    // already use for this same predicate.
    if !reporter.shell_listening() {
        return;
    }
    let phase = phase_for(remaining, stalled, refused.len());
    reporter.cloud_sync(&CloudSync {
        folder: &folder.to_string_lossy(),
        phase,
        provider: crate::build::cloud_provider::detect_from_path(folder),
        total,
        remaining,
        blocking: Some(blocking),
        unavailable: &unavailable_names(refused),
    });
}

/// How many of the outstanding files the render actually waits on.
///
/// The predicate the build path already gates on
/// (`crate::build::cloud_ledger::is_structural_source`), applied at the
/// second site that needed it: `cloud_gate_should_hold` asks it of the
/// ledger's set and so correctly declined to gate on a bookkeeping file,
/// while the progress stream asked nothing at all.
///
/// Refused files are excluded for the reason they are excluded from
/// `remaining`: once moss has stopped waiting for a file, nothing it holds up
/// could ever come down again.
pub(crate) fn blocking_count(pending: &HashSet<PathBuf>, refused: &[&PathBuf]) -> usize {
    pending
        .iter()
        .filter(|p| !refused.contains(p))
        .filter(|p| crate::build::cloud_ledger::is_structural_source(p))
        .count()
}

/// Which phase one tick describes. Pure, so the ordering is pinned by a test
/// rather than by reading the call site. `stalled` outranks `unavailable`:
/// while anything is still believed to be arriving, silence is the more
/// urgent fact.
pub(crate) fn phase_for(awaiting: usize, stalled: bool, unavailable: usize) -> &'static str {
    if stalled {
        "stalled"
    } else if awaiting == 0 && unavailable > 0 {
        "unavailable"
    } else {
        "materializing"
    }
}

/// File names for the notice — base name only, capped: the notice names
/// files to be actionable, and a wall of forty
/// hidden-ancestor paths is not. Full paths are in the log.
pub(crate) fn unavailable_names(refused: &[&PathBuf]) -> Vec<String> {
    const NAMED: usize = 3;
    refused
        .iter()
        .take(NAMED)
        .map(|p| p.file_name().unwrap_or(p.as_os_str()).to_string_lossy().into_owned())
        .collect()
}
