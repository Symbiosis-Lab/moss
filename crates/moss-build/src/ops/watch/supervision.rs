//! Watcher supervision: the sweep judges the watcher by outcomes, and this
//! module carries the evidence between the two.
//!
//! Phase 3 of `docs/archive/2026-08-18-watcher-reliability-architecture.md`
//! ("The sweep is also the watcher's supervisor"). The incident that design
//! answers — notify's macOS FSEvents stream dying with no flag, no error, no
//! Rescan (LOG-8D03-T0942-08-18) — emits nothing a health check could poll,
//! so the watcher is judged by the one signal a silent-but-dead stream cannot
//! fake: **drift the sweep catches that the watcher never delivered.**
//!
//! ## The strike rule, exactly as the design states it
//!
//! - **A strike is a drift catch, aged sweep-relative.** When the sweep
//!   dispatches fresh drift it arms a candidate carrying this registry's
//!   sequence counters; the candidate matures at the NEXT walk pass. If no
//!   watcher batch arrived in that whole window — the change was on disk
//!   before one pass and the watcher stayed silent through another full
//!   interval — the event can no longer be "in flight" (the debouncer fires
//!   within 250ms), and the silence is the verdict. One aged strike →
//!   recreate.
//! - **A confessed loss is not a strike.** A rescan-flag batch
//!   (`watcher_lost_events`) means the watcher is alive and honest about an
//!   overflow; the pump already answers it with a full-scan rebuild, and
//!   tearing the watcher down would re-register watches at the worst moment.
//!   Confessions bump their own counter so a candidate spanning one is
//!   disarmed.
//! - **Backoff on repeated recreation.** A persistently broken environment
//!   (File-Provider death loop, FUSE mount with no event support) degrades to
//!   sweep-only operation with occasional retries: the n-th recreation is
//!   honored only after `min(2^(n-1) × 60s, 1h)`. Both constants are
//!   conservative on purpose — the design prefers a false "watcher healthy"
//!   (the sweep still bounds staleness) over recreate churn, since recreation
//!   is itself a brief event-loss window on macOS. Any delivered batch resets
//!   the count: a watcher that speaks is a watcher that works.
//!
//! ## Liveness is judged pre-filter
//!
//! `note_batch` is called on every batch the debouncer delivers, BEFORE the
//! pump's own filters. A batch the pump then suppresses (agent-config noise,
//! metadata-only events) still proves the stream is alive, and the incident's
//! failure mode is total silence, not selective loss — so pre-filter is the
//! conservative reading. A watcher that delivers noise while dropping content
//! events would evade the strike rule; no surveyed platform failure has that
//! shape, and the sweep still bounds the staleness it would cause.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Delay before honoring the n-th consecutive recreation (n >= 2; the first
/// is immediate — one aged strike is the design's whole threshold).
pub const RECREATE_BACKOFF_BASE: Duration = Duration::from_secs(60);

/// Ceiling for the recreation backoff: degraded-to-sweep-only still retries
/// the watcher about once an hour, forever.
pub const RECREATE_BACKOFF_CAP: Duration = Duration::from_secs(3600);

/// Backoff before honoring the `n`-th consecutive recreation: none for the
/// first, then 60s, 120s, … capped at [`RECREATE_BACKOFF_CAP`].
pub fn recreate_backoff(prior_recreations: u32) -> Duration {
    if prior_recreations == 0 {
        return Duration::ZERO;
    }
    let shift = prior_recreations.saturating_sub(1).min(31);
    RECREATE_BACKOFF_BASE
        .checked_mul(1u32.checked_shl(shift).unwrap_or(u32::MAX))
        .unwrap_or(RECREATE_BACKOFF_CAP)
        .min(RECREATE_BACKOFF_CAP)
}

/// The sweep's snapshot of the counters at the moment it arms a strike
/// candidate. Compared, never interpreted — see [`WatcherHealth::judge`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HealthMark {
    batches: u64,
    confessions: u64,
}

/// What a matured strike candidate concluded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrikeVerdict {
    /// A watcher batch arrived inside the window — the event was in flight,
    /// the watcher is alive. Not a strike.
    WatcherSpoke,
    /// The watcher confessed a loss inside the window (rescan flag). Healthy
    /// and honest; exempt by design.
    ConfessedLoss,
    /// Total silence across a full sweep interval over a change the sweep had
    /// already caught. The aged strike.
    AgedStrike,
}

/// One folder's watcher-health ledger, shared between the pump (writer of
/// liveness), the sweep (judge), and the watcher task (executor of the
/// recreate verdict).
pub struct WatcherHealth {
    /// Batches the debouncer delivered, pre-filter.
    batches: AtomicU64,
    /// Rescan-flag confessions (`watcher_lost_events`).
    confessions: AtomicU64,
    /// Wakes the watcher task to tear down and re-create its debouncer.
    recreate: tokio::sync::Notify,
    /// Consecutive recreations with no delivered batch in between — the
    /// backoff exponent. Reset by [`note_batch`](Self::note_batch).
    recreations: AtomicU32,
    /// When the last recreation was requested; the backoff clock.
    last_recreate_at: Mutex<Option<Instant>>,
    /// One log line per degraded episode, not one per suppressed strike.
    degraded_logged: std::sync::atomic::AtomicBool,
}

impl WatcherHealth {
    fn new() -> Self {
        WatcherHealth {
            batches: AtomicU64::new(0),
            confessions: AtomicU64::new(0),
            recreate: tokio::sync::Notify::new(),
            recreations: AtomicU32::new(0),
            last_recreate_at: Mutex::new(None),
            degraded_logged: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The watcher delivered a batch. Called pre-filter (see module doc);
    /// also the proof that a recreated watcher works, so the backoff resets.
    ///
    /// The reset happens OUTSIDE the `last_recreate_at` lock on purpose: a
    /// strike racing this store can read `recreations == 0` and grant one
    /// extra immediate recreation. Benign — a batch means the watcher is
    /// alive, so the "extra" recreation is at worst redundant, and taking
    /// the lock here would put a mutex on the watcher's hot path.
    pub fn note_batch(&self) {
        self.batches.fetch_add(1, Ordering::SeqCst);
        self.recreations.store(0, Ordering::SeqCst);
        self.degraded_logged.store(false, Ordering::SeqCst);
    }

    /// The watcher confessed an event loss (rescan flag on the batch).
    pub fn note_confession(&self) {
        self.confessions.fetch_add(1, Ordering::SeqCst);
    }

    /// Snapshot the counters — taken by the sweep when it arms a candidate.
    pub fn mark(&self) -> HealthMark {
        HealthMark {
            batches: self.batches.load(Ordering::SeqCst),
            confessions: self.confessions.load(Ordering::SeqCst),
        }
    }

    /// Judge a matured candidate against the counters as they stand now.
    pub fn judge(&self, armed: HealthMark) -> StrikeVerdict {
        let now = self.mark();
        if now.batches != armed.batches {
            StrikeVerdict::WatcherSpoke
        } else if now.confessions != armed.confessions {
            // Unreachable in production as wired today: `note_batch` fires on
            // every delivered batch BEFORE the pump can see the rescan flag,
            // so a confession always rides a batch and the arm above answers
            // first. Kept anyway — the exemption is the design's rule, not an
            // artifact of call order. Do NOT "fix" the ordering to reach it.
            StrikeVerdict::ConfessedLoss
        } else {
            StrikeVerdict::AgedStrike
        }
    }

    /// An aged strike landed: recreate the watcher, unless the backoff says
    /// this environment is persistently broken and the retry must wait.
    /// Returns whether a recreation was actually requested (the caller sweeps
    /// immediately when it was — recreation covers its own event-loss gap).
    pub fn strike(&self, folder: &str) -> bool {
        let mut last = self.last_recreate_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let prior = self.recreations.load(Ordering::SeqCst);
        let wait = recreate_backoff(prior);
        if let Some(at) = *last {
            if at.elapsed() < wait {
                if !self.degraded_logged.swap(true, Ordering::SeqCst) {
                    log::warn!(
                        target: "moss::build::watch",
                        "Watcher for '{}' keeps missing changes after {} recreation(s) — \
                         degraded to sweep-only operation; next watcher retry in ~{}s",
                        folder,
                        prior,
                        (wait - at.elapsed()).as_secs()
                    );
                }
                return false;
            }
        }
        *last = Some(Instant::now());
        self.recreations.fetch_add(1, Ordering::SeqCst);
        log::error!(
            target: "moss::build::watch",
            "Watcher for '{}' missed a change the sweep caught, and stayed silent for a \
             further sweep interval — recreating the watcher (recreation #{})",
            folder,
            prior + 1
        );
        self.recreate.notify_one();
        true
    }

    /// Completes when a recreation has been requested. `Notify::notify_one`
    /// stores a permit when nobody waits, so a strike landing while the
    /// watcher task is mid-batch is not lost.
    pub async fn recreate_requested(&self) {
        self.recreate.notified().await;
    }

    /// Whether this folder's watcher is in the backed-off "degraded to
    /// sweep-only operation" state `strike` already logs (above) — true from
    /// the first strike a backoff window suppresses, false again the moment
    /// a delivered batch proves the watcher alive. A single honored strike is
    /// NOT degraded: one recreation is the design's ordinary self-heal, and
    /// reporting it would flap a host's health surface on routine recovery.
    ///
    /// Nothing reads this today: the sweep already bounds preview staleness
    /// to one interval regardless of watcher health (a drift dispatch runs
    /// before a strike is even armed), so this state is silent everywhere
    /// except the ERROR log above — for up to [`RECREATE_BACKOFF_CAP`] at a
    /// time, a persistently broken watcher drops rename fidelity and
    /// responsiveness to `.moss/theme` and `.moss/config.toml` (paths only
    /// the watcher reports; see `drift_eligible` in the sweep) with no
    /// user-visible signal. A host can fold this into its own advisory
    /// surface — e.g. this crate's `FolderHealthChanged.degraded`, which
    /// today reflects only the rebuild worker's overdue-build watchdog.
    pub fn is_degraded(&self) -> bool {
        self.degraded_logged.load(Ordering::SeqCst)
    }
}

/// The sweep's half of the strike rule: the one armed candidate, and the two
/// moments that move it — a fresh drift dispatch arms it, the next walk pass
/// matures it. Extracted from the sweep loop so the arming → maturity →
/// recreate path is pinned by unit tests against a real ledger (spec review
/// should-fix 2); the end-to-end fault-injection replay (kill the event
/// stream under a live watcher) is tracked as moss#1076.
pub struct StrikeArm {
    candidate: Option<HealthMark>,
}

impl StrikeArm {
    pub fn new() -> Self {
        StrikeArm { candidate: None }
    }

    /// A fresh drift dispatch: arm the candidate with the counters as they
    /// stand — UNLESS the same tick's top-level `read_dir` diff already went
    /// dirty. The root's direct entries are legitimately unwatched on macOS
    /// (no root subscription), so a change the walk sees there too is one the
    /// watcher never could have delivered; striking on it would tear down a
    /// healthy watcher every time a file lands at the vault root — on a cloud
    /// root (walk every tick), nearly every time (adversarial review F3).
    pub fn arm(&mut self, folder: &str, top_level_dirty: bool) {
        self.candidate = if top_level_dirty {
            None
        } else {
            get(folder).map(|h| h.mark())
        };
    }

    /// Environment failure (unreadable root): the watcher is not on trial.
    pub fn disarm(&mut self) {
        self.candidate = None;
    }

    /// A walk pass reached with a readable root — the aging boundary. Judges
    /// and consumes the candidate; returns whether a recreation was actually
    /// requested (the sweep answers `true` with an immediate follow-up pass,
    /// covering the recreation's own event-loss gap).
    pub fn mature(&mut self, folder: &str) -> bool {
        let Some(armed) = self.candidate.take() else {
            return false;
        };
        let Some(h) = get(folder) else {
            return false;
        };
        // WatcherSpoke / ConfessedLoss: alive (and, for a rescan flag,
        // honest) — disarm silently, no strike.
        h.judge(armed) == StrikeVerdict::AgedStrike && h.strike(folder)
    }
}

// ---------------------------------------------------------------------------
// Registry: folder path → health ledger (same shape as the worker registry —
// process-global so the headless `moss build --serve --watch` path gets the
// same supervision without managed state)
// ---------------------------------------------------------------------------

static LEDGERS: LazyLock<Mutex<HashMap<String, Arc<WatcherHealth>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn ledgers() -> std::sync::MutexGuard<'static, HashMap<String, Arc<WatcherHealth>>> {
    LEDGERS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Create and register the ledger for `folder` (the watcher task owns its
/// lifecycle, mirroring `worker::register`).
pub fn register(folder: &str) -> Arc<WatcherHealth> {
    let health = Arc::new(WatcherHealth::new());
    ledgers().insert(folder.to_string(), health.clone());
    health
}

/// Remove `folder`'s ledger — only if it still maps to `health`, so a
/// replaced watcher's late deregistration cannot evict its successor's.
pub fn deregister(folder: &str, health: &Arc<WatcherHealth>) {
    let mut map = ledgers();
    if map.get(folder).is_some_and(|h| Arc::ptr_eq(h, health)) {
        map.remove(folder);
    }
}

/// The ledger for `folder`, if a watcher task is running for it.
pub fn get(folder: &str) -> Option<Arc<WatcherHealth>> {
    ledgers().get(folder).cloned()
}

#[cfg(test)]
#[path = "supervision_tests.rs"]
mod tests;

/// Folders already told about below, so the warning is said once per process
/// rather than once per sweep pass.
static BLIND_REPORTED: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// Say once that moss can see files in `folder` but may watch none of them.
///
/// A folder moss is blind to sweeps, logs and heartbeats **exactly** like a
/// quiet one: `585 walk pass(es), 0 drift catch(es)` is what a healthy sweep
/// over an untouched vault prints, and it is also what a sweep prints whose
/// eligible set is empty. Those two states were indistinguishable in the log,
/// so the heartbeat that exists to make a dead loop diagnosable reported full
/// health while the loop compared nothing at all, 585 times, over a client's
/// live folder with an unrebuilt edit sitting on disk (#1080).
///
/// A log line and nothing else. The vaults this fires on are real people's
/// folders, open in an editor right now; a mid-edit banner over a diagnostic
/// moss cannot act on for them is worse than the silence it replaces.
///
/// `files_seen` and the watchable count both exclude `.moss/` — moss's own
/// tree is not evidence about the user's. A folder of nothing but dotfiles,
/// `node_modules` and unsupported file types is the one innocent way to see
/// this; everything else means the vault root moss was given is wrong, or a
/// predicate is judging the path to the folder instead of the paths in it.
/// Returns whether it said anything — the once-per-folder decision, made
/// testable without reaching into the log.
pub fn note_blind_folder(folder: &str, files_seen: usize) -> bool {
    if files_seen == 0 {
        return false;
    }
    let first = BLIND_REPORTED
        .lock()
        .map(|mut set| set.insert(folder.to_string()))
        .unwrap_or(false);
    if !first {
        return false;
    }
    log::warn!(
        target: "moss::build::watch",
        "Nothing in '{}' is watchable: all {} file(s) are excluded, so no edit there will \
         rebuild the site. moss judges a file by its path INSIDE the folder — if this folder \
         has ordinary content in it, that judgement is what is wrong, not the folder.",
        folder,
        files_seen
    );
    true
}
