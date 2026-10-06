//! The sweep: ONE periodic task per folder answering "does the disk still
//! match what we built?" — the correctness backbone that backstops the file
//! watcher. Events are the accelerator; this is what keeps a synced folder
//! (iCloud, Drive) or an atomic-rename save from going silently stale.
//!
//! It absorbs the watcher loop's target reconciler and the cloud supervisor
//! (dataless-set measurement, download requests, arrival-paced rebuilds). Three
//! host-specific seams are replaced by engine ones:
//!
//! - **Folder-health transitions** (`FolderHealthChanged`) go out through the
//!   host's [`EventRelay`] — the one the watcher already takes (the carrier's
//!   `publish` headless, the typed Tauri bus plus the carrier on the desktop) —
//!   instead of a window-labeled `emit_to("preview", …)`.
//!   `MossEvent::FolderHealthChanged` already lives in the shared enum; this
//!   is the first host-agnostic *emitter* of it, not a new variant.
//! - **Cloud-sync progress** goes out through
//!   [`BuildReporter::cloud_sync`](crate::build::ports::reporter::BuildReporter),
//!   the reporter method that already exists for exactly this signal — its
//!   own doc names this module as the second producer. No new port method:
//!   `BuildReporter` is at its four-method abort threshold.
//! - **Rebuild dispatch** takes a [`crate::ops::watch::RebuildDispatch`]
//!   instead of a host-owned rebuild handle — the same enqueue closure
//!   `ops/watch/headless.rs` already builds for the watcher, so a sweep
//!   trigger and a watcher trigger take the identical path (worker slot →
//!   admission → content-hash gate → `run_pipeline`), including whatever
//!   cross-process build lock sits inside it.
//!
//! `deploy::publish_in_flight()` needed no seam at all: it was already a
//! process-global query in this crate (`deploy::freeze`), not an app-side
//! callback.
//!
//! ## Shape rules, each load-bearing
//!
//! - **Owned by the `FolderSession`, registered in its `tasks` JoinSet** — a
//!   sweep not registered there survives its own folder switch, which is
//!   its own bug class. Cancellation is re-checked immediately before every
//!   dispatch.
//! - **Every verdict dispatches through the phase-1a worker**
//!   (the [`crate::ops::watch::RebuildDispatch`] closure). The sweep never
//!   touches the debouncer. Under `MOSS_REBUILD_WORKER=off` (or before the
//!   worker registers) that closure degrades to its own inline fallback —
//!   the sweep would await builds in its own loop — so drift verdicts are
//!   suspended until a worker exists; arrival dispatches deliberately keep
//!   the inline path, because on a cloud-gate-deferred first build it is the
//!   only build path there is.
//! - **The walk is skipped while a build runs or is owed.** Comparing disk
//!   against a baseline mid-replacement races the thing it verifies, and a
//!   fresh enqueue's poke would cut the failed-build backoff short — turning
//!   a deterministic build failure into a walk-cadence retry loop.
//! - **Every pass runs under its own deadline, on the blocking pool,
//!   panic-proof.** A blown pass is an ERROR naming the folder (once per
//!   episode), keeps the shard it did judge, and leaves a resume cursor so
//!   consecutive blown passes still cover the whole tree; enough of them in
//!   a row is its own unavailability verdict, root readability
//!   notwithstanding. A panicked tick is skipped, never fatal, and a
//!   low-frequency heartbeat makes a dead loop diagnosable. The sweep must
//!   not be able to wedge silently.
//! - **Root-gone is a verdict, not a mechanism**: consecutive unreadable
//!   passes set `FolderSession::unavailable` — surfaced as a full-window
//!   state via `FolderHealthChanged`, alongside the combined `degraded` flag
//!   (worker watchdog OR watcher backoff, see `tick::folder_degraded`) — and
//!   mask every disappearance; never a rebuild storm.
//! - **The sweep is also the watcher's supervisor**: every drift catch is a
//!   change the watcher failed to deliver, so catches double as health
//!   evidence. A fresh drift dispatch arms a strike candidate carrying the
//!   watcher-health counters (`ops/watch/supervision.rs`); it matures at the
//!   next walk pass, and total watcher silence across that whole window is
//!   the aged strike that recreates the watcher (backed off, so a broken
//!   environment degrades to sweep-only instead of churning). The watcher —
//!   on cloud roots above all — is thereby ADVISORY: its death costs latency
//!   and niceties until the strike rule recreates it, never correctness.
//!
//! Split across sibling files by responsibility, not by line count: the walk
//! and baseline machinery (`sweep/walk.rs`), pacing and progress reporting
//! (`sweep/progress.rs`), and the per-folder tick loop (`sweep/tick.rs`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use crate::build::ports::reporter::BuildReporter;
use crate::ops::watch::{EventRelay, RebuildDispatch};
use crate::system::folder_session::FolderSession;

mod progress;
mod tick;
mod walk;

/// The sweep's tick. Every tick runs the cheap work (top-level `read_dir`
/// diff, evicted-set arrival scan); every Nth tick runs the stat walk.
pub(crate) const TICK: Duration = Duration::from_secs(2);

/// Walk cadence on a cloud root: every tick (~2s). Presumes the dataless
/// fail-fast io policy — under it a stat materializes nothing and cannot
/// block on `fileproviderd` (Apple TN3150), and bulk `lstat` runs ~8µs/file,
/// so a 5,000-file vault costs single-digit milliseconds per pass.
pub(crate) const CLOUD_WALK_EVERY_TICKS: u64 = 1;

/// Walk cadence on a local root: every 10th tick (~20s) — the design's
/// "10–30s local" band. Local filesystems' watchers are far more reliable,
/// so the sweep is a slower backstop there.
pub(crate) const LOCAL_WALK_EVERY_TICKS: u64 = 10;

/// Per-pass deadline. The pass is stat-only, so seconds is generous; on
/// filesystems where stat is a network RPC (FUSE/SMB/NFS) a pass CAN stall,
/// and stalling silently is the one thing the sweep is not allowed to do.
pub(crate) const WALK_DEADLINE: Duration = Duration::from_secs(15);

/// Consecutive root-unreadable ticks before the folder is declared
/// unavailable — rides out provider flaps (design: "2–3 sweeps").
pub(crate) const UNAVAILABLE_AFTER: u32 = 3;

/// One INFO line every 5 minutes of wall clock, whatever the cadence, so "the
/// sweep is alive and has caught N drifts over M passes" is answerable from a
/// support log — and a silent log means a dead loop.
pub(crate) const HEARTBEAT_EVERY: Duration = Duration::from_secs(300);

/// Floor on the gap between two arrival-driven rebuilds. Carried over from
/// the cloud supervisor verbatim, rationale and all: a bulk download is not
/// an edit, the real interval is `max(floor, last build duration)`, and
/// there is deliberately no "drain went quiet" shortcut (bursty providers
/// look quiet between bursts at any sampling rate). See `should_rebuild`.
pub(crate) const MIN_REBUILD_INTERVAL: Duration = Duration::from_secs(8);

/// Silence across the whole pending set before the user is told.
pub(crate) const STALL_AFTER: Duration = Duration::from_secs(90);

/// The tick the loop actually sleeps. `MOSS_TEST_SWEEP_TICK_MS` is a test seam: it
/// shortens the tick and makes every tick a walk, so a process-level test sees
/// a reconciliation in seconds instead of a local root's ~20s cadence.
pub(crate) fn tick_and_walk_every(default_walk_every: u64) -> (Duration, u64) {
    match std::env::var("MOSS_TEST_SWEEP_TICK_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
        Some(ms) if ms > 0 => (Duration::from_millis(ms), 1),
        _ => (TICK, default_walk_every),
    }
}

// ---------------------------------------------------------------------------
// Lifecycle: one sweep per folder, owned by the FolderSession
// ---------------------------------------------------------------------------

/// folder-key → generation and cancel token of the live sweep. Process-global
/// (like the worker registry) so the headless `moss build --serve --watch`
/// path gets the same dedupe without managed state. The token is what lets a
/// start tell a live claim from one whose session was already cancelled.
/// What the registry remembers of a live sweep: its generation, and the cancel
/// token of the session that owns it.
pub(crate) struct ClaimInfo {
    pub(crate) gen: u64,
    pub(crate) token: tokio_util::sync::CancellationToken,
}

static SWEEPS: LazyLock<Mutex<HashMap<String, ClaimInfo>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_GEN: AtomicU64 = AtomicU64::new(1);

fn sweeps() -> std::sync::MutexGuard<'static, HashMap<String, ClaimInfo>> {
    SWEEPS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes the registry entry when the sweep future ends — HOWEVER it ends,
/// including being dropped by the session's cancel select or `abort_all`.
struct Claim {
    key: String,
    gen: u64,
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut map = sweeps();
        if map.get(&self.key).map(|c| c.gen) == Some(self.gen) {
            map.remove(&self.key);
        }
    }
}

/// The host's side of the sweep, as one value.
pub struct SweepHost {
    /// Routes rebuild triggers through the host's rebuild path.
    pub dispatch: RebuildDispatch,
    /// Carries cloud-sync progress to whatever surface the host gives it (a
    /// no-op reporter if nobody is listening).
    pub reporter: Arc<dyn BuildReporter>,
    /// The host's event relay — the one the watcher takes — carrying
    /// folder-health changes.
    pub emit: EventRelay,
    /// The host's Live/Background signal (a constant-`Live` receiver where
    /// there is no visibility signal), the one the watcher takes.
    pub cadence: tokio::sync::watch::Receiver<crate::ops::watch::cadence::Cadence>,
}

/// Start the folder's sweep if none is running for it. The check and the
/// insert happen under ONE lock hold, so two concurrent starts cannot both
/// pass a pre-check and double-start. The sweep ends when `session.cancel`
/// fires (or the future is dropped), and its [`Claim`] then frees the key so
/// a later start — a resumed process — begins a fresh one.
///
/// A claim whose session is already cancelled does not count as running: its
/// task is only about to be dropped, and a reopen builds its new session
/// before the old one's cancellation has been polled, so honouring the stale
/// claim would leave the reopened folder with no sweep at all. The stale
/// task's own [`Claim`] leaves the replacement alone (generation check). The old
/// task may run until its next cancellation poll, so for at most one tick it
/// overlaps the new sweep; that is benign because both read the same baseline
/// and dispatch through the same build lock.
///
/// `host` is everything the host supplies: see [`SweepHost`].
pub async fn start(session: Arc<FolderSession>, host: SweepHost) {
    let key = session.folder.to_string_lossy().to_string();
    let gen = NEXT_GEN.fetch_add(1, Ordering::SeqCst);
    {
        let mut map = sweeps();
        if map.get(&key).is_some_and(|c| !c.token.is_cancelled()) {
            return;
        }
        map.insert(key.clone(), ClaimInfo { gen, token: session.cancel.clone() });
    }
    let claim = Claim { key, gen };
    let fut = {
        let session = session.clone();
        async move {
            let _claim = claim; // released whenever the future ends or drops
            tick::run(session, host).await;
        }
    };
    session.spawn_ui_bound(fut).await;
}

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;
