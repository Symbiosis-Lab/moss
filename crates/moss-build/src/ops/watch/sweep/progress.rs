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

use super::{MIN_REBUILD_INTERVAL, STALL_AFTER};

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

/// Feed the existing reader pool in bounded batches. The cursor gives every
/// pending file a turn without putting the entire site ahead of a newly
/// requested page input.
pub(crate) fn request_downloads(
    pending: &HashSet<PathBuf>,
    cursor: &mut usize,
) {
    if pending.is_empty() {
        *cursor = 0;
        return;
    }
    let waiting = crate::build::cloud_readiness::download_snapshot().waiting;
    let count = pending.len().min(6).min(12usize.saturating_sub(waiting));
    let mut paths: Vec<_> = pending.iter().collect();
    paths.sort();
    for offset in 0..count {
        crate::build::cloud_readiness::request_download(paths[(*cursor + offset) % paths.len()]);
    }
    *cursor = (*cursor + count) % paths.len();
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

/// Report the observed cloud set. Age changes the advisory phase, never the
/// pending count or the number observed to arrive.
pub(crate) fn emit_progress(
    reporter: &dyn BuildReporter,
    folder: &std::path::Path,
    total: usize,
    downloaded: usize,
    remaining: usize,
    blocking: usize,
    stalled: bool,
    pending: &HashSet<PathBuf>,
) {
    let (unavailable_count, unavailable) = crate::build::cloud_readiness::download_failures_for(folder, pending);
    // The CLI has no cloud-sync panel.
    if !reporter.shell_listening() {
        return;
    }
    let phase = phase_for(stalled);
    reporter.cloud_sync(&CloudSync {
        folder: &folder.to_string_lossy(),
        phase,
        provider: crate::build::cloud_provider::detect_from_path(folder),
        total,
        downloaded,
        remaining,
        blocking: Some(blocking),
        unavailable: &unavailable,
        unavailable_count,
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
pub(crate) fn blocking_count(pending: &HashSet<PathBuf>) -> usize {
    pending
        .iter()
        .filter(|p| crate::build::cloud_ledger::is_structural_source(p))
        .count()
}

/// A quiet provider is a waiting advisory, not evidence of failure.
pub(crate) fn phase_for(stalled: bool) -> &'static str {
    if stalled {
        "stalled"
    } else {
        "materializing"
    }
}
