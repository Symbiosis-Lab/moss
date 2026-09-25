//! Tests for the per-folder rebuild worker (phase 1a).
//!
//! The loop is generic over the admission attempt, so everything here runs
//! with closures — no AppHandle, no real build.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

fn content_only(path: &str) -> RebuildRequest {
    RebuildRequest {
        rename_pairs: Vec::new(),
        trigger: BuildTrigger::ContentOnly(vec![std::path::PathBuf::from(path)]),
        gate_paths: None,
    }
}

fn with_pairs(pairs: &[(&str, &str)]) -> RebuildRequest {
    RebuildRequest {
        rename_pairs: pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        trigger: BuildTrigger::Structural(vec![]),
        gate_paths: None,
    }
}

fn gated(paths: &[&str]) -> RebuildRequest {
    RebuildRequest {
        gate_paths: Some(paths.iter().map(std::path::PathBuf::from).collect()),
        ..RebuildRequest::full()
    }
}

// ── The merge rule ──────────────────────────────────────────────────────────

#[test]
fn a_lone_request_keeps_its_rename_pairs_and_trigger() {
    let handle = WorkerHandle::new();
    handle.enqueue(with_pairs(&[("a.md", "b.md")]));
    let got = handle.take().expect("slot should hold the request");
    assert_eq!(got.rename_pairs, vec![("a.md".to_string(), "b.md".to_string())]);
    assert!(matches!(got.trigger, BuildTrigger::Structural(_)));
}

/// The merge preserves the classification instead of degrading it to `Full`.
///
/// Replaces `merging_coalesces_to_full_with_no_pairs`, which pinned the old
/// rule ("two batches never classified together have no narrower honest
/// trigger than Full"). That rule cost every save landing mid-build its
/// incremental gates, which on a large vault is a 223-page render instead of a
/// one-page one — and a full build being slower is what makes the NEXT save
/// land mid-build too.
#[test]
fn merging_preserves_the_narrowest_honest_trigger() {
    let paths = |t: &BuildTrigger| match t {
        BuildTrigger::ContentOnly(p) | BuildTrigger::Structural(p) => p.clone(),
        BuildTrigger::Full => vec![],
    };

    // Two content-only batches union their paths and STAY content-only: the
    // gates are predicates over the path set, so the union carries exactly the
    // evidence a single batch holding both paths would have.
    let merged = merge_requests(content_only("a.md"), content_only("b.md"));
    assert!(matches!(merged.trigger, BuildTrigger::ContentOnly(_)), "got {:?}", merged.trigger);
    assert_eq!(
        paths(&merged.trigger),
        vec![std::path::PathBuf::from("a.md"), std::path::PathBuf::from("b.md")]
    );

    // A structural side wins — a create/remove anywhere in the union makes the
    // union structural — and rename pairs are still dropped, the manifest diff
    // being what surfaces the structural change downstream.
    let merged = merge_requests(with_pairs(&[("a.md", "b.md")]), content_only("c.md"));
    assert!(matches!(merged.trigger, BuildTrigger::Structural(_)), "got {:?}", merged.trigger);
    assert_eq!(paths(&merged.trigger), vec![std::path::PathBuf::from("c.md")]);
    assert!(merged.rename_pairs.is_empty());

    // `Full` carries no paths and therefore no evidence, so it absorbs both
    // sides in either order — the one place over-approximating is still right.
    assert_eq!(
        merge_requests(RebuildRequest::full(), content_only("a.md")).trigger,
        BuildTrigger::Full
    );
    assert_eq!(
        merge_requests(content_only("a.md"), RebuildRequest::full()).trigger,
        BuildTrigger::Full
    );
}

/// The admission gate only widens under merge: two suppressible requests
/// union their paths; one unconditional side voids the gate entirely.
#[test]
fn merging_unions_gate_paths_and_unconditional_wins() {
    let merged = merge_requests(gated(&["a.md"]), gated(&["b.md", "a.md"]));
    assert_eq!(
        merged.gate_paths,
        Some(vec![std::path::PathBuf::from("a.md"), std::path::PathBuf::from("b.md")])
    );

    let merged = merge_requests(gated(&["a.md"]), content_only("c.md"));
    assert_eq!(
        merged.gate_paths, None,
        "an unconditional request must never become suppressible by merging"
    );
}

#[test]
fn enqueue_onto_an_occupied_slot_merges() {
    let handle = WorkerHandle::new();
    handle.enqueue(content_only("a.md"));
    handle.enqueue(content_only("b.md"));
    // The slot's subject: ONE request comes out, carrying both batches. The
    // trigger it carries is `merge_requests`' business — asserted next door in
    // `merging_preserves_the_narrowest_honest_trigger` — but pinning it here
    // too is what would catch the slot silently substituting its own.
    assert_eq!(
        handle.take(),
        Some(RebuildRequest {
            rename_pairs: vec![],
            trigger: BuildTrigger::ContentOnly(vec![
                std::path::PathBuf::from("a.md"),
                std::path::PathBuf::from("b.md")
            ]),
            gate_paths: None,
        })
    );
    assert_eq!(handle.take(), None, "merge must not leave a second request");
}

#[test]
fn restore_preserves_a_deferred_request_and_merges_late_arrivals() {
    let handle = WorkerHandle::new();
    // Deferred alone: the request survives byte-for-byte (a frozen-out rename
    // keeps its pairs for the thaw's catch-up).
    let req = with_pairs(&[("a.md", "b.md")]);
    handle.restore(req.clone());
    assert_eq!(handle.take(), Some(req.clone()));
    // Deferred with a new arrival in between: merged, not lost.
    handle.enqueue(content_only("c.md"));
    handle.restore(req);
    let merged = handle.take().expect("restore onto an occupied slot must merge, not drop");
    // Structural (from the deferred rename) over content-only (the arrival),
    // with the arrival's path carried — not the `Full` this used to degrade to.
    assert!(matches!(merged.trigger, BuildTrigger::Structural(_)), "got {:?}", merged.trigger);
    assert!(merged.rename_pairs.is_empty(), "pairs are dropped in favor of the manifest diff");
}

/// BLOCKER regression (phase 1b review): admission clears `rebuild_pending`
/// at dequeue, and `deploy::wait_for_rebuild_quiet` reads only
/// `is_rebuilding || rebuild_pending` — so a lock-busy or failed-build park
/// that restored WITHOUT re-raising the flag let a publish read the window
/// as "quiet" and upload output missing the user's last save. Pre-1b the
/// build blocked on the lock with `is_rebuilding = true`, so this is new
/// surface, not old behavior.
#[test]
fn restore_visibly_reraises_the_pending_flag_a_dequeue_cleared() {
    let handle = WorkerHandle::new();
    // The state after dequeue: flag cleared, no build running.
    let rebuild_pending = AtomicBool::new(false);
    let is_rebuilding = false;

    handle.restore_visibly(Some(&rebuild_pending), content_only("a.md"));

    let quiet = !is_rebuilding && !rebuild_pending.load(Ordering::SeqCst);
    assert!(
        !quiet,
        "a parked request (lock-busy / failed-build backoff) must keep the quiet-wait waiting"
    );
    assert!(handle.slot_occupied(), "the request itself survives the park");

    // No flag wired (headless): still just a restore.
    handle.restore_visibly(None, content_only("b.md"));
    assert!(handle.slot_occupied());
}

// ── The no-lost-request guarantee ───────────────────────────────────────────

/// A request enqueued while the worker is mid-build MUST result in a second
/// build, with no further poke — the core property the slot replaces the old
/// `rebuild_pending` bit for.
#[tokio::test]
async fn a_request_arriving_mid_build_is_built_next() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let builds = std::sync::Arc::new(AtomicUsize::new(0));
    let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (second_done_tx, second_done_rx) = tokio::sync::oneshot::channel::<()>();

    let b = builds.clone();
    let mut first_started = Some(first_started_tx);
    let mut release = Some(release_rx);
    let mut second_done = Some(second_done_tx);
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |req| {
        let n = b.fetch_add(1, Ordering::SeqCst) + 1;
        let started = if n == 1 { first_started.take() } else { None };
        let gate = if n == 1 { release.take() } else { None };
        let done = if n == 2 { second_done.take() } else { None };
        async move {
            if n == 2 {
                assert_eq!(
                    req.gate_paths, None,
                    "a catch-up arriving during a build must not be discarded against that build's mixed source/output baseline"
                );
            }
            if let Some(tx) = started {
                tx.send(()).ok();
            }
            if let Some(rx) = gate {
                rx.await.ok(); // the first build is "in flight" until released
            }
            if let Some(tx) = done {
                tx.send(()).ok();
            }
            AttemptOutcome::Completed
        }
    }));

    handle.enqueue(content_only("a.md"));
    first_started_rx.await.expect("first build should start");
    // Mid-build: this must be picked up on the worker's next iteration.
    handle.enqueue(gated(&["b.md"]));
    release_tx.send(()).ok();

    tokio::time::timeout(Duration::from_secs(5), second_done_rx)
        .await
        .expect("the mid-build request must trigger a second build")
        .expect("second build should signal");
    assert_eq!(builds.load(Ordering::SeqCst), 2);

    handle.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker should exit on shutdown")
        .unwrap();
}

// ── Freeze at admission ─────────────────────────────────────────────────────

/// A deferred request (attempt returns false after restoring it) is not lost:
/// the worker parks until poked, then builds it.
#[tokio::test]
async fn a_frozen_out_request_survives_until_the_thaw_poke() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let frozen = std::sync::Arc::new(AtomicBool::new(true));
    let builds = std::sync::Arc::new(AtomicUsize::new(0));
    let deferrals = std::sync::Arc::new(AtomicUsize::new(0));
    let (built_tx, built_rx) = tokio::sync::oneshot::channel::<()>();

    let h = handle.clone();
    let f = frozen.clone();
    let b = builds.clone();
    let d = deferrals.clone();
    let mut built = Some(built_tx);
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |req| {
        let h = h.clone();
        let f = f.clone();
        let b = b.clone();
        let d = d.clone();
        let tx = if f.load(Ordering::SeqCst) { None } else { built.take() };
        async move {
            if f.load(Ordering::SeqCst) {
                d.fetch_add(1, Ordering::SeqCst);
                h.restore(req); // the freeze path: request goes back, no progress
                return AttemptOutcome::Parked;
            }
            b.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = tx {
                tx.send(()).ok();
            }
            AttemptOutcome::Completed
        }
    }));

    handle.enqueue(content_only("a.md"));
    // Give the worker time to dequeue, defer, and park. The request must
    // still be there (restored), and nothing built.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(builds.load(Ordering::SeqCst), 0, "no build may run while frozen");
    assert!(deferrals.load(Ordering::SeqCst) >= 1, "the worker should have deferred");
    assert!(handle.slot_occupied(), "the deferred request must stay in the slot");

    frozen.store(false, Ordering::SeqCst);
    handle.poke(); // the thaw
    tokio::time::timeout(Duration::from_secs(5), built_rx)
        .await
        .expect("the thaw poke must drain the deferred request")
        .expect("build should signal");
    assert_eq!(builds.load(Ordering::SeqCst), 1);

    handle.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker should exit on shutdown")
        .unwrap();
}

// ── Failed-build backoff (phase 1b) ─────────────────────────────────────────

#[test]
fn backoff_doubles_from_base_and_caps() {
    assert_eq!(backoff_delay(1), Duration::from_secs(2));
    assert_eq!(backoff_delay(2), Duration::from_secs(4));
    assert_eq!(backoff_delay(3), Duration::from_secs(8));
    assert_eq!(backoff_delay(6), Duration::from_secs(64));
    assert_eq!(backoff_delay(7), BACKOFF_CAP, "delay must cap, not overflow");
    assert_eq!(backoff_delay(1000), BACKOFF_CAP);
}

/// Scripted attempt: each call runs the step for its ordinal, restoring the
/// request itself when the outcome defers (mirroring the contract
/// `attempt_admitted_rebuild` keeps in production).
fn scripted_attempts(
    handle: &std::sync::Arc<WorkerHandle>,
    script: Vec<AttemptOutcome>,
    times: &std::sync::Arc<Mutex<Vec<tokio::time::Instant>>>,
    done_tx: tokio::sync::oneshot::Sender<()>,
) -> impl FnMut(RebuildRequest) -> std::pin::Pin<Box<dyn std::future::Future<Output = AttemptOutcome> + Send>> {
    let handle = handle.clone();
    let times = times.clone();
    let mut done_tx = Some(done_tx);
    let mut step = 0usize;
    move |req| {
        let outcome = script.get(step).copied().unwrap_or(AttemptOutcome::Completed);
        step += 1;
        times.lock().unwrap().push(tokio::time::Instant::now());
        if !matches!(outcome, AttemptOutcome::Completed) {
            handle.restore(req);
        }
        if step == script.len() {
            if let Some(tx) = done_tx.take() {
                tx.send(()).ok();
            }
        }
        Box::pin(async move { outcome })
    }
}

/// A deterministically failing build retries with exponentially growing gaps
/// (2s, 4s, 8s) instead of spinning at event cadence.
#[tokio::test(start_paused = true)]
async fn failed_builds_back_off_exponentially() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let times = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    use AttemptOutcome::*;
    let attempt = scripted_attempts(&handle, vec![Failed, Failed, Failed, Completed], &times, done_tx);
    let worker = tokio::spawn(run_worker_loop(handle.clone(), attempt));

    handle.enqueue(content_only("broken.md"));
    done_rx.await.expect("all scripted attempts should run");

    let t = times.lock().unwrap().clone();
    assert_eq!(t.len(), 4);
    assert_eq!(t[1] - t[0], Duration::from_secs(2), "first retry after 2^1 base");
    assert_eq!(t[2] - t[1], Duration::from_secs(4), "second retry doubles");
    assert_eq!(t[3] - t[2], Duration::from_secs(8), "third retry doubles again");

    handle.request_shutdown();
    worker.await.unwrap();
}

/// One success resets the backoff counter: a later failure starts again at
/// the base delay, not where the previous streak left off.
#[tokio::test(start_paused = true)]
async fn a_successful_build_resets_the_backoff_counter() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let times = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    use AttemptOutcome::*;
    // Fail, succeed (streak broken), fail again, final attempt. The enqueue
    // for the post-success request is injected below via a scripted wrapper.
    let mut inner = scripted_attempts(&handle, vec![Failed, Completed, Failed, Completed], &times, done_tx);
    let h = handle.clone();
    let mut step = 0usize;
    let attempt = move |req: RebuildRequest| {
        step += 1;
        if step == 2 {
            // The success disposes of its request; hand the worker a fresh
            // one so the next (failing) attempt has something to chew on.
            h.enqueue(content_only("still-broken.md"));
        }
        inner(req)
    };
    let worker = tokio::spawn(run_worker_loop(handle.clone(), attempt));

    handle.enqueue(content_only("broken.md"));
    done_rx.await.expect("all scripted attempts should run");

    let t = times.lock().unwrap().clone();
    assert_eq!(t.len(), 4);
    assert_eq!(
        t[3] - t[2],
        Duration::from_secs(2),
        "the failure after a success must wait the BASE delay again (counter reset)"
    );

    handle.request_shutdown();
    worker.await.unwrap();
}

/// A new change event landing during a backoff wait cuts it short — a fixed
/// site rebuilds promptly instead of serving out the failure's sentence.
#[tokio::test(start_paused = true)]
async fn a_new_enqueue_cuts_a_backoff_wait_short() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let times = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (first_tx, first_rx) = tokio::sync::oneshot::channel::<()>();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let h = handle.clone();
    let ts = times.clone();
    let mut first_tx = Some(first_tx);
    let mut done_tx = Some(done_tx);
    let mut step = 0usize;
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |req| {
        step += 1;
        ts.lock().unwrap().push(tokio::time::Instant::now());
        let outcome = if step == 1 {
            h.restore(req);
            if let Some(tx) = first_tx.take() {
                tx.send(()).ok();
            }
            AttemptOutcome::Failed
        } else {
            if let Some(tx) = done_tx.take() {
                tx.send(()).ok();
            }
            AttemptOutcome::Completed
        };
        async move { outcome }
    }));

    handle.enqueue(content_only("broken.md"));
    first_rx.await.expect("first attempt should run");
    // The user saves a fix while the worker is serving its 2s backoff.
    handle.enqueue(content_only("fixed.md"));
    done_rx.await.expect("second attempt should run");

    let t = times.lock().unwrap().clone();
    assert_eq!(t.len(), 2);
    assert!(
        t[1] - t[0] < BACKOFF_BASE,
        "the enqueue's poke must end the backoff wait early (waited {:?})",
        t[1] - t[0]
    );

    handle.request_shutdown();
    worker.await.unwrap();
}

/// A wake that arrives DURING attempt N is spent on attempt N+1's immediate
/// admission — its leftover `Notify` permit must not ALSO shorten attempt
/// N+1's own backoff wait. Pin: after the preempting enqueue cuts wait N
/// short, wait N+1 runs its full scheduled length.
#[tokio::test(start_paused = true)]
async fn a_spent_preempting_wake_does_not_shorten_the_next_backoff_wait() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let times = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let h = handle.clone();
    let ts = times.clone();
    let mut done_tx = Some(done_tx);
    let mut step = 0usize;
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |req| {
        step += 1;
        ts.lock().unwrap().push(tokio::time::Instant::now());
        if step == 1 {
            // A new change lands while attempt 1 is running: bumps wake_seq
            // AND stores a Notify permit nobody is waiting on yet.
            h.enqueue(content_only("more.md"));
        }
        let outcome = if step <= 2 {
            h.restore(req);
            AttemptOutcome::Failed
        } else {
            if let Some(tx) = done_tx.take() {
                tx.send(()).ok();
            }
            AttemptOutcome::Completed
        };
        async move { outcome }
    }));

    handle.enqueue(content_only("broken.md"));
    done_rx.await.expect("all attempts should run");

    let t = times.lock().unwrap().clone();
    assert_eq!(t.len(), 3);
    assert!(
        t[1] - t[0] < BACKOFF_BASE,
        "the mid-attempt enqueue must cut wait 1 short (waited {:?})",
        t[1] - t[0]
    );
    assert_eq!(
        t[2] - t[1],
        backoff_delay(2),
        "wait 2 must run FULL length — the spent wake's leftover permit is stale, not a pass"
    );

    handle.request_shutdown();
    worker.await.unwrap();
}

// ── Panic isolation ─────────────────────────────────────────────────────────

/// A panic anywhere in the admission attempt must not kill the worker: the
/// registry would still hand out the handle, so every later enqueue would
/// land in a slot nobody drains — a permanent silent dead preview, the exact
/// failure this worker exists to prevent. The panic is caught, the request
/// restored, and backoff absorbs it.
#[tokio::test(start_paused = true)]
async fn a_panicking_build_leaves_the_worker_alive_and_retrying() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let attempts = std::sync::Arc::new(AtomicUsize::new(0));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let a = attempts.clone();
    let mut done_tx = Some(done_tx);
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |req| {
        let n = a.fetch_add(1, Ordering::SeqCst) + 1;
        let tx = if n > 1 { done_tx.take() } else { None };
        async move {
            if n == 1 {
                panic!("build exploded before it could restore {:?}", req.trigger);
            }
            if let Some(tx) = tx {
                tx.send(()).ok();
            }
            AttemptOutcome::Completed
        }
    }));

    handle.enqueue(content_only("cursed.md"));
    done_rx
        .await
        .expect("the worker must survive the panic and retry the restored request");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    // Still a functioning worker: it hears shutdown.
    handle.request_shutdown();
    worker.await.unwrap();
}

// ── Stage-lock try-admission (phase 1b) ─────────────────────────────────────

/// A lock-busy deferral retries on its own tick — the seal tail that holds
/// the stage lock releases it without poking the slot, so the retry cannot
/// be poke-driven.
#[tokio::test(start_paused = true)]
async fn a_lock_busy_deferral_retries_on_the_tick_without_a_poke() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let times = std::sync::Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    use AttemptOutcome::*;
    let attempt = scripted_attempts(&handle, vec![LockBusy, LockBusy, Completed], &times, done_tx);
    let worker = tokio::spawn(run_worker_loop(handle.clone(), attempt));

    handle.enqueue(content_only("a.md"));
    done_rx.await.expect("all scripted attempts should run");

    let t = times.lock().unwrap().clone();
    assert_eq!(t.len(), 3);
    assert_eq!(t[1] - t[0], LOCK_RETRY_TICK, "retry after one tick, unpoked");
    assert_eq!(t[2] - t[1], LOCK_RETRY_TICK, "lock-busy retries do not back off");

    handle.request_shutdown();
    worker.await.unwrap();
}

// ── Overdue watchdog: diagnostics only (phase 1b) ───────────────────────────

/// The watchdog marks the handle degraded once the deadline passes, and ONLY
/// reports — the build it watches is never touched. Dropping it (the build
/// finished) clears the state.
#[tokio::test(start_paused = true)]
async fn the_watchdog_marks_overdue_builds_degraded_and_clears_on_finish() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let wd = OverdueWatchdog::arm(handle.clone(), "/tmp/slow-site", Duration::from_secs(5));

    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!handle.is_degraded(), "not degraded before the deadline");

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(handle.is_degraded(), "degraded once the deadline passes");

    drop(wd); // the build finally finished
    assert!(!handle.is_degraded(), "finishing clears the degraded state");
}

/// A build finishing EXACTLY at the deadline must not leave a stale verdict:
/// the timer body has no await after its sleep, so once the sleep completes
/// `abort()` can no longer stop it — whichever side of the race the fired
/// timer lands on (before the drop: degraded set, then cleared; after: it
/// must see the disarm flag and stand down), the end state is not-degraded.
/// Without the disarm flag this pinned a real bug: a false "overdue" ERROR
/// for a finished build plus `degraded` stuck true forever on an idle folder.
#[tokio::test(start_paused = true)]
async fn a_finish_at_the_deadline_never_leaves_degraded_behind() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let wd = OverdueWatchdog::arm(handle.clone(), "/tmp/edge-site", Duration::from_secs(5));
    // Land exactly on the deadline: the timer is due NOW, scheduled or even
    // already run, when the build "finishes".
    tokio::time::sleep(Duration::from_secs(5)).await;
    drop(wd);
    // Let any already-fired timer body run to completion.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(
        !handle.is_degraded(),
        "a finished build must never stay degraded, whichever side of the race the timer won"
    );
}

/// A build that finishes in time never degrades, even after its watchdog's
/// deadline would have elapsed — disarming must actually cancel the timer.
#[tokio::test(start_paused = true)]
async fn a_timely_finish_disarms_the_watchdog() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let wd = OverdueWatchdog::arm(handle.clone(), "/tmp/fast-site", Duration::from_secs(5));
    tokio::time::sleep(Duration::from_secs(1)).await;
    drop(wd);
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(!handle.is_degraded(), "a disarmed watchdog must never fire");
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn shutdown_wakes_an_idle_worker_and_it_exits() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    let worker = tokio::spawn(run_worker_loop(handle.clone(), |_req| async {
        AttemptOutcome::Completed
    }));
    // Let it reach the idle wait, then ask it to exit.
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("an idle worker must hear shutdown")
        .unwrap();
}

#[test]
fn deregister_only_removes_its_own_registration() {
    let folder = "/tmp/worker-registry-test";
    let first = register(folder);
    let second = register(folder); // replaces; asks `first` to shut down
    assert!(first.shutdown_requested(), "a replaced worker is asked to exit");
    deregister(folder, &first); // late deregistration of the replaced worker
    assert!(
        get(folder).is_some_and(|h| Arc::ptr_eq(&h, &second)),
        "the successor's registration must survive"
    );
    deregister(folder, &second);
    assert!(get(folder).is_none());
}

// ── The completion signal (phase 3) ─────────────────────────────────────────

/// A completed admission records when it finished and how long it ran — the
/// sweep's measured-from-finish pacing input. Failures record nothing: a
/// failing build's cadence is the backoff's business, and its duration must
/// not lengthen arrival pacing.
#[tokio::test]
async fn a_completed_admission_records_finish_time_and_duration() {
    let handle = std::sync::Arc::new(WorkerHandle::new());
    assert!(handle.last_completed().is_none(), "no signal before the first completion");

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let done_tx = std::sync::Arc::new(Mutex::new(Some(done_tx)));
    let worker = tokio::spawn(run_worker_loop(handle.clone(), move |_req| {
        let done_tx = done_tx.clone();
        async move {
            // A real (if tiny) build duration, so the recorded duration
            // provably measures the attempt rather than the enqueue.
            tokio::time::sleep(Duration::from_millis(30)).await;
            if let Some(tx) = done_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            AttemptOutcome::Completed
        }
    }));

    let before = std::time::Instant::now();
    handle.enqueue(RebuildRequest::full());
    done_rx.await.expect("the attempt should run");
    // The record lands on the loop's next poll after the attempt returns.
    let mut recorded = None;
    for _ in 0..200 {
        recorded = handle.last_completed();
        if recorded.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let (finished, duration) = recorded.expect("a completed admission records its finish");
    assert!(finished >= before, "finish time postdates the enqueue");
    assert!(
        duration >= Duration::from_millis(30),
        "duration measures the attempt's span, not the enqueue"
    );

    handle.request_shutdown();
    worker.await.unwrap();
}
