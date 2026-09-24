//! The publish latch, and the build freeze it doubles as.
//!
//! One process-global piece of state answers two questions:
//!
//! - *May this publish start?* — at most one publish runs per process
//!   (2026-07-21 incident, see `with_publish_guard`).
//! - *May the build admit new work?* — no, while a publish is in flight.
//!
//! The second is the first read from the other side. Two separate flags could
//! disagree; one cannot. `PublishGuard` is the only type that may write it, so
//! the freeze lifts on every exit a publish has — success, error, `?`, user
//! cancel, window close, a future dropped mid-await, and panic-unwind. A leaked
//! freeze would stop the folder rebuilding for the life of the process, which
//! is worse than the hang it prevents.

use super::progress::{DeploySink, DeployStage};
use std::sync::Arc;

/// Rejection returned when a publish is requested while another is in flight.
/// Compared by value at the call sites so the rejection is LOGGED rather than
/// reported as this publish failing. That a rejected request cannot paint the
/// RUNNING deploy's task row failed is now structural, not a convention: the
/// guard returns above its own terminal emit.
pub const PUBLISH_IN_PROGRESS_MSG: &str =
    "A publish is already in progress. Wait for it to finish, then publish again.";

/// Single-flight latch AND build freeze for THIS PROCESS — not per-folder (a
/// publish saturates network and CPU). Shared by both in-process entry points,
/// `push_site` and `push_prebuilt`. A separate `moss deploy` OS process is NOT
/// serialized.
///
/// Holding it also FREEZES the build. The rebuild
/// worker checks this at ADMISSION (`build_shell/watch.rs::
/// attempt_admitted_rebuild`): a frozen-out request stays parked in its own
/// folder's request slot, and the thaw pokes every slot. The old global
/// `deferred_rebuild: Option<String>` side channel this replaces could lose
/// one of two folders' catch-ups — a single slot, last writer wins.
static PUBLISH: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

fn publish_lock() -> std::sync::MutexGuard<'static, bool> {
    PUBLISH.lock().unwrap_or_else(|e| e.into_inner())
}

/// Is a publish in flight — i.e. is the build frozen? The freeze is not a
/// second mechanism bolted on: it IS this latch, read from the other side.
pub fn publish_in_flight() -> bool {
    *publish_lock()
}

/// RAII half of the latch. Drop — not an explicit release — is what makes the
/// guard leak-proof: it fires on early return, `?`, panic-unwind, and on the
/// future being dropped mid-await (stall-watchdog cancel, window close, task
/// abort). A leaked latch would block publishing until the app restarts — and
/// since 959 it would also stop the folder rebuilding for the life of the
/// process, which is why nothing outside this type may set `in_flight`.
struct PublishGuard;

impl PublishGuard {
    fn try_acquire() -> Result<Self, String> {
        let mut state = publish_lock();
        if *state {
            return Err(PUBLISH_IN_PROGRESS_MSG.to_string());
        }
        *state = true;
        Ok(PublishGuard)
    }
}

impl Drop for PublishGuard {
    fn drop(&mut self) {
        *publish_lock() = false;
        // THE THAW. Requests frozen out at admission are parked in their own
        // folder's request slot; wake every worker to re-check. A poke over
        // an empty slot costs one wake, so no bookkeeping of WHICH folders
        // deferred is needed — that bookkeeping (one global `deferred_rebuild`
        // slot) is exactly what used to lose one of two folders' catch-ups.
        //
        // Ordering: the freeze is lifted BEFORE the poke, so a woken worker's
        // admission check cannot re-defer against a latch that is about to
        // clear anyway; and a watcher batch arriving in between enqueues and
        // wakes its own worker. Never neither.
        log::info!(target: "deploy", "publish finished — thawing the build");
        crate::ops::watch::worker::poke_all();
    }
}

/// Run `body` only if no other publish is running (2026-07-21 incident: two
/// full deploys, `160608cc` and `53283b5b` 12s apart, each shipped the same 761
/// files and reported into one UI counter, so the on-screen count jumped around
/// while ~1522 uploads and two builds pegged the CPU). Rejecting rather than
/// coalescing is deliberate: the user learns their newer edits were NOT shipped.
///
/// Holding the guard also **freezes the build** for the duration;
/// the thaw on drop pokes every folder's rebuild worker so frozen-out
/// requests catch up. `sink` is only for the terminal progress emits; a
/// path with nobody watching passes [`progress::silent`].
///
/// The guard is acquired *inside* the wrapper future, so nothing is latched
/// until this is awaited, and the latch drops with the future.
///
/// Generic over the body's success type on purpose: a publish is a publish
/// whoever ships it. The seta paths return `PushResult`; the plugin path
/// (`preview::commands::deploy_site`, which is how an OnionPress publish
/// reaches the network) returns `DeployResult`. Nothing in the guard inspects
/// the value — it only needs the future to resolve — so freezing one and not
/// the other was an accident of typing, not a decision.
///
/// ## The guard owns the publish's terminal stage
///
/// Every publish enters the stage machine through a SHARED door —
/// `MossEvent::DeployWaitingForBuild`, emitted from `build_shell::wait_for_in_flight_work`,
/// which is generic build machinery — but the only exits used to be hand-written
/// inside the seta bodies. The plugin path (an OnionPress publish) tripped the
/// shared entrance and had no exit at all, so the panel sat on
/// "Waiting for build… rebuild paused" forever after a publish that had in fact
/// succeeded, and the nav-pill hairline never completed.
///
/// So the terminal stage is emitted HERE, at the one choke point every publish
/// passes through, instead of at each body's return sites. One entrance, one
/// exit, both owned by shared code.
///
/// This also makes a carve-out that used to be comment-enforced structural: a
/// publish rejected as a duplicate must NOT emit `Failed`, because no work
/// started and the in-flight publish owns the panel — marking it failed would
/// mark the OTHER one failed. `try_acquire` returns `Err` before `body.await`,
/// so a rejected duplicate returns above the emit and cannot reach it.
pub async fn with_publish_guard<T, F: std::future::Future<Output = Result<T, String>>>(
    sink: &Arc<dyn DeploySink>,
    body: F,
) -> Result<T, String> {
    // Above the emit ON PURPOSE — see the carve-out note above.
    let _guard = PublishGuard::try_acquire()?;

    let result = body.await;

    // The frontend reads no fields off `complete` (progress-panel returns
    // immediately on it, and the hairline only branches on the stage), so the
    // counts carry nothing here. On `failed` the message IS the payload — it is
    // what threads the real error into the advisory system — and the guard has
    // it, because the error string is the message.
    match &result {
        Ok(_) => sink.stage(
            DeployStage::Complete,
            0,
            0,
            &crate::infra::app_advisory::t("published"),
        ),
        Err(e) => sink.stage(DeployStage::Failed, 0, 0, e),
    }
    result
}

#[cfg(test)]
pub(crate) mod single_flight_tests {
    use super::*;
    use crate::deploy::progress::{silent, RecordingSink};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Serializes every test that touches the publish latch. The latch is
    /// process-global, so a test that reads it observes any other running
    /// concurrently.
    ///
    /// The desktop app's own publish tests take a twin of this. That is not
    /// duplication to fold: a `#[cfg(test)]` static compiles only into its own
    /// crate's test binary, so the two are never live in the same process.
    pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// The guard inspects nothing about the value — it only needs the future
    /// to resolve — so what a publish returns is irrelevant here.
    fn ok() -> Result<&'static str, String> {
        Ok("https://x.mosspub.com")
    }

    /// Only terminal stages; the bodies here emit nothing else, but this keeps
    /// the assertions about the exit rather than about event volume.
    fn terminal(sink: &RecordingSink) -> Vec<(DeployStage, String)> {
        sink.drain()
            .into_iter()
            .filter(|(stage, _)| matches!(stage, DeployStage::Complete | DeployStage::Failed))
            .collect()
    }

    /// The rejection contract, all of it: the duplicate's body never runs, the
    /// caller is told why, and — the part that regressed a publish on screen —
    /// NOTHING is emitted. The in-flight publish owns the progress panel, so a
    /// `Failed` for the rejected duplicate would mark the RUNNING one failed.
    /// `try_acquire` returns above the guard's terminal emit, which is what
    /// makes that structural rather than a comment someone has to remember.
    ///
    /// The rejected publish gets its OWN sink, so "emitted nothing" is read off
    /// a sink the in-flight publish never touched — and it is read while the
    /// first publish is still parked, so a terminal stage belonging to the
    /// first cannot be mistaken for the second's silence.
    #[tokio::test]
    async fn second_concurrent_publish_is_rejected_runs_nothing_and_emits_nothing() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let runs = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();

        let r1 = runs.clone();
        let first_sink = silent();
        let first = tokio::spawn(async move {
            with_publish_guard(&first_sink, async move {
                r1.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).ok();
                finish_rx.await.ok();
                ok()
            })
            .await
        });
        started_rx.await.expect("first publish should start");

        let recorder = Arc::new(RecordingSink::default());
        let rejected: Arc<dyn DeploySink> = recorder.clone();
        let r2 = runs.clone();
        let second = with_publish_guard(&rejected, async move {
            r2.fetch_add(1, Ordering::SeqCst);
            ok()
        })
        .await;
        assert!(
            recorder.drain().is_empty(),
            "a rejected duplicate must not touch the in-flight publish's panel"
        );

        let err = second.expect_err("second concurrent publish must be rejected");
        assert!(
            err.contains("already in progress"),
            "unexpected rejection message: {err}"
        );

        finish_tx.send(()).ok();
        first.await.unwrap().expect("first publish should succeed");
        assert_eq!(runs.load(Ordering::SeqCst), 1, "only ONE deploy may run");

        // Guard released on the success path.
        assert!(with_publish_guard(&silent(), async { ok() }).await.is_ok());
    }

    #[tokio::test]
    async fn guard_is_released_after_a_failed_publish() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let failed =
            with_publish_guard(&silent(), async { Err::<&str, _>("boom".to_string()) }).await;
        assert_eq!(failed.unwrap_err(), "boom");
        assert!(
            with_publish_guard(&silent(), async { ok() }).await.is_ok(),
            "a failed publish must not leave the guard held"
        );
    }

    // ── The guard owns the publish's terminal stage ─────────────────────────
    //
    // Before this, every body wrote its own terminal emit, so the plugin
    // publish path — which had none — left the progress panel stuck on
    // "Waiting for build" forever after a publish that succeeded.

    #[tokio::test]
    async fn a_successful_publish_ends_with_complete() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let recorder = Arc::new(RecordingSink::default());
        let sink: Arc<dyn DeploySink> = recorder.clone();

        with_publish_guard(&sink, async { ok() }).await.unwrap();

        let events = terminal(&recorder);
        assert_eq!(events.len(), 1, "exactly one terminal stage: {events:?}");
        assert_eq!(events[0].0, DeployStage::Complete);
    }

    #[tokio::test]
    async fn a_failed_publish_ends_with_failed_carrying_the_error() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let recorder = Arc::new(RecordingSink::default());
        let sink: Arc<dyn DeploySink> = recorder.clone();

        let err = with_publish_guard(&sink, async {
            Err::<&str, _>("the receiver refused the commit".to_string())
        })
        .await
        .unwrap_err();

        let events = terminal(&recorder);
        assert_eq!(events.len(), 1, "exactly one terminal stage: {events:?}");
        assert_eq!(events[0].0, DeployStage::Failed);
        assert_eq!(
            events[0].1, err,
            "the message IS the payload on failure — it is what reaches the advisory"
        );
    }

    #[tokio::test]
    async fn guard_is_released_when_the_publish_future_is_dropped() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let sink = silent();
        let handle = tokio::spawn(async move {
            with_publish_guard(&sink, async move {
                started_tx.send(()).ok();
                std::future::pending::<()>().await;
                ok()
            })
            .await
        });
        started_rx.await.expect("publish should start");
        handle.abort();
        let _ = handle.await;

        assert!(
            with_publish_guard(&silent(), async { ok() }).await.is_ok(),
            "a cancelled publish must not permanently block publishing"
        );
    }

    #[tokio::test]
    async fn guard_is_released_when_the_publish_body_panics() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let sink = silent();
        let handle = tokio::spawn(async move {
            with_publish_guard(&sink, async {
                panic!("deploy blew up");
                #[allow(unreachable_code)]
                ok()
            })
            .await
        });
        assert!(handle.await.is_err(), "body should have panicked");

        assert!(
            with_publish_guard(&silent(), async { ok() }).await.is_ok(),
            "a panicking publish must not permanently block publishing"
        );
    }

    // ── The freeze ───────────────────────────────────────────────
    //
    // These pin the property the whole fix rests on: while the guard is held,
    // `publish_in_flight()` reads true, so the rebuild worker's admission
    // check refuses every rebuild and no new UiBound work can be admitted
    // behind the publish's drain (the request-parks-until-poked half lives in
    // `moss_build::ops::watch::worker` and is tested there). And the freeze lifts
    // on EVERY exit — a leaked freeze stops the folder rebuilding for the
    // life of the process, which is strictly worse than the hang it prevents.

    #[tokio::test]
    async fn the_build_is_frozen_for_the_duration_of_a_publish() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            !publish_in_flight(),
            "nothing is publishing — the watcher must rebuild normally"
        );

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let sink = silent();
        let publish = tokio::spawn(async move {
            with_publish_guard(&sink, async move {
                started_tx.send(()).ok();
                finish_rx.await.ok();
                ok()
            })
            .await
        });
        started_rx.await.expect("publish should start");

        assert!(
            publish_in_flight(),
            "no rebuild may be admitted while a publish is in flight"
        );

        finish_tx.send(()).ok();
        publish.await.unwrap().expect("publish should succeed");

        assert!(!publish_in_flight(), "the freeze must lift with the guard");
    }

    /// The freeze rides the same RAII guard as the single-flight latch, so it
    /// is released on error, on cancel (future dropped mid-await) and on
    /// panic-unwind. A leaked freeze would stop the folder rebuilding forever.
    #[tokio::test]
    async fn the_freeze_lifts_on_error_cancel_and_panic() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // Error.
        let _ = with_publish_guard(&silent(), async { Err::<&str, _>("boom".to_string()) }).await;
        assert!(!publish_in_flight(), "an errored publish must thaw the build");

        // Cancel: the future is dropped mid-await.
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let sink = silent();
        let handle = tokio::spawn(async move {
            with_publish_guard(&sink, async move {
                started_tx.send(()).ok();
                std::future::pending::<()>().await;
                ok()
            })
            .await
        });
        started_rx.await.expect("publish should start");
        assert!(publish_in_flight(), "frozen while running");
        handle.abort();
        let _ = handle.await;
        assert!(!publish_in_flight(), "a cancelled publish must thaw the build");

        // Panic-unwind.
        let sink = silent();
        let handle = tokio::spawn(async move {
            with_publish_guard(&sink, async {
                panic!("deploy blew up");
                #[allow(unreachable_code)]
                ok()
            })
            .await
        });
        assert!(handle.await.is_err(), "body should have panicked");
        assert!(!publish_in_flight(), "a panicking publish must thaw the build");
    }
}
