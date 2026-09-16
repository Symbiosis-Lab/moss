//! The publish path's stall detector: one process-global progress clock.
//!
//! # Why this exists
//!
//! Invariant **I1** (`docs/archive/2026-08-04-publish-deadlines-vs-bandwidth.md`
//! §8.1): *no deadline in the publish path may be a function of the total amount
//! of work.* The deadline it replaces, `DEPLOY_TIMEOUT = 900 s`, was exactly
//! that: on 2026-08-04 a tester's 57.66 MB site over a ~50 KB/s uplink needed
//! ~1142 s of pure transfer, so the publish was impossible *before it started*
//! and was killed at second 900 while transferring steadily. Its own doc comment
//! said its purpose was to "fire on a stall"; this module implements that
//! literally instead of approximating it with a total-work budget.
//!
//! # The shape
//!
//! One process-global clock. Anything that *proves the publish is still moving*
//! calls [`bump`]. A [`watchdog`] samples the clock ~1 Hz and resolves once
//! nothing has bumped for [`STALL_TIMEOUT`]. Publish duration then becomes a
//! function of the user's link, and only a genuinely stuck publish fails.
//!
//! Process-global (rather than per-deploy) is sound because publish is already
//! single-flighted by `deploy::with_publish_guard`: at most one deploy is in
//! flight per process.
//!
//! # What may bump, and what may not
//!
//! **A bump must be evidence of progress, not evidence of a timer.** The upload
//! phase runs a 250 ms ticker that re-emits `Uploading` progress whether or not
//! the byte counters moved (`deploy.rs`); bumping from that emit would advance
//! the clock 4×/second for the whole upload phase and the watchdog could never
//! fire where stalls actually happen. So the upload phase bumps on the **byte
//! credit itself** (`deploy::credit_upload_bytes`), never on the emit — pinned
//! by `the_upload_progress_ticker_is_not_a_liveness_bump` in `deploy_tests.rs`.
//!
//! The same rule rejects `wait_for_in_flight_work`'s 500 ms
//! `DeployWaitingForBuild` heartbeat, which is why the app's
//! `deploy::activity::is_build_liveness_event` whitelists build-progress event
//! kinds instead of accepting any `MossEvent`.
//!
//! The clock lives here rather than app-side because the seta client bumps it
//! from every retry boundary and every uploaded chunk, and that client crossed
//! with ADR-078. Only the Tauri event listener that feeds it build progress
//! stayed behind, in `deploy::activity`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long the whole publish may go without a single proof of progress.
///
/// Justification against the worst *legitimate* quiet interval: a request that
/// goes silent fails at `upload_policy::UPLOAD_REQUEST_TIMEOUT` (150 s), and
/// `seta::client::retry_transient` bumps at every attempt boundary —
/// including a backpressure wait — so the interval is one request timeout, not
/// a chain of them. A publish that is alive re-proves it within 150 s, and
/// 300 s leaves ~2× margin. Pinned by
/// `stall_timeout_outlasts_the_worst_case_quiet_interval`.
///
/// The attempt bump is load-bearing rather than incidental: without it, a
/// window of small single-PUT files (no per-chunk byte credit) that each time
/// out twice in a row is legitimately quiet for 150 + backoff + 150 s, over
/// this bound — a healthy publish killed. Do not remove it without raising
/// this constant to cover the chain.
///
/// It is also *stricter* than the 900 s it replaces for the case the old
/// constant was actually written for: an iCloud Drive file fault inside
/// `spawn_blocking` credits nothing and now fails at 300 s, not 900 s.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(300);

/// Watchdog sampling period. The clock's resolution, not its accuracy: a stall
/// is detected between `STALL_TIMEOUT` and `STALL_TIMEOUT + WATCHDOG_POLL`.
const WATCHDOG_POLL: Duration = Duration::from_secs(1);

/// Process-start reference for the millisecond clock below. A monotonic
/// `Instant` cannot be stored in an atomic; the offset from a fixed origin can.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Milliseconds since [`ORIGIN`] at the last [`bump`].
static LAST_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

/// Monotonic count of bumps. Lets a poller ask "did anything happen since I
/// last looked?" without comparing wall-clock values (see `wait_until_quiet`).
static BUMPS: AtomicU64 = AtomicU64::new(0);

fn origin() -> Instant {
    *ORIGIN.get_or_init(Instant::now)
}

/// Record that the publish just made observable progress.
///
/// Cheap enough (two relaxed atomic stores) to call per uploaded chunk, per
/// hashed file, and per post-commit step.
pub fn bump() {
    LAST_ACTIVITY_MS.store(origin().elapsed().as_millis() as u64, Ordering::Relaxed);
    BUMPS.fetch_add(1, Ordering::Relaxed);
}

/// How long since the last [`bump`].
pub fn idle() -> Duration {
    let now = origin().elapsed().as_millis() as u64;
    Duration::from_millis(now.saturating_sub(LAST_ACTIVITY_MS.load(Ordering::Relaxed)))
}

/// Monotonic bump count — a change means progress happened.
pub fn bump_count() -> u64 {
    BUMPS.load(Ordering::Relaxed)
}

/// Resolve once nothing has bumped for `stall`, returning the observed idle
/// time. Never resolves while progress keeps arriving — that is the whole point.
pub async fn watchdog(stall: Duration) -> Duration {
    // Sub-second `stall` values exist only in tests; keep the poll under the
    // window so the detection delay stays proportional.
    let poll = std::cmp::min(WATCHDOG_POLL, stall / 4).max(Duration::from_millis(1));
    loop {
        tokio::time::sleep(poll).await;
        let idle = idle();
        if idle >= stall {
            return idle;
        }
    }
}

/// Run `fut` under the stall watchdog: its own result if it finishes, a stall
/// error if it goes quiet for `stall`.
///
/// The clock is bumped on entry so a caller never inherits a stale window.
pub async fn bounded_by_stall<T, F>(fut: F, stall: Duration, what: &str) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    bump();
    tokio::select! {
        result = fut => result,
        idle = watchdog(stall) => {
            log::warn!(target: "deploy", "{what}: no progress for {}s — aborting", idle.as_secs());
            Err(stalled_message(stall))
        }
    }
}

/// The user-visible stall failure.
///
/// Deliberately not "Check your internet connection" — the 2026-08-04 tester's
/// connection was not broken, it was slow, and that message sent them to
/// diagnose the wrong thing. A stall now means *stopped*, and the actionable
/// fact is that a re-publish resumes rather than restarts.
pub(crate) fn stalled_message(stall: Duration) -> String {
    format!(
        "Publishing stopped making progress for {} minutes, so it was cancelled. \
         Nothing was lost — publish again and moss continues from where it stopped.",
        stall.as_secs() / 60
    )
}

/// Serializes every test that reads the process-global clock. The clock is one
/// counter for the whole process, so a test asserting "nothing bumped for N ms"
/// observes any concurrently-running test that bumps.
///
/// The app declares its own twin in `deploy/activity.rs`. A `#[cfg(test)]` item
/// compiles only into its own crate's test binary, so neither crate can see the
/// other's lock and each needs one; they are never live together.
#[cfg(test)]
pub(crate) static CLOCK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
#[path = "liveness_tests.rs"]
mod tests;
