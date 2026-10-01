//! The generic debouncer's own properties, independent of any caller
//! (the seal, in `build.rs`, has its own tests exercising it end to end).
//! `u32` requests and a shared call log stand in for a real payload.

use super::*;
use std::path::PathBuf;
use std::sync::PoisonError;

static CALLS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
/// Serializes tests against the shared `CALLS` log — tokio tests otherwise
/// run concurrently and would interleave writes to it.
static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn record(req: u32) -> HandlerFuture {
    Box::pin(async move {
        CALLS.lock().unwrap_or_else(PoisonError::into_inner).push(req);
    })
}

fn calls() -> Vec<u32> {
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// A burst of requests inside one idle window must collapse to exactly one
/// handler call, over the LATEST request — never one per request, and never
/// a stale one. Mirrors the property `search_lane`'s own
/// `a_burst_of_requests_indexes_once_...` test pinned before this module
/// existed.
#[tokio::test(start_paused = true)]
async fn a_burst_of_requests_runs_the_handler_once_with_the_latest_value() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let lanes = Lanes::new(Duration::from_millis(200), Duration::from_secs(10), record);
    let key = PathBuf::from("/vault-a");

    for i in 0..5u32 {
        lanes.request(&key, i);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(calls().is_empty(), "no request may run before its own idle window elapses");

    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(calls(), vec![4], "a burst must collapse to one call, over the latest request");
}

/// `settle_now` must run the pending request immediately, without waiting
/// out `idle` — the whole point of a caller (deploy, folder close/switch,
/// app quit) that cannot afford the natural debounce delay.
#[tokio::test(start_paused = true)]
async fn settle_now_runs_immediately_without_waiting_for_idle() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record);
    let key = PathBuf::from("/vault-b");

    lanes.request(&key, 7);
    // No sleep at all: settle_now must not need the idle window to elapse.
    lanes.settle_now(&key).await;
    assert_eq!(calls(), vec![7]);
}

/// `settle_now` with nothing pending — either because no request was ever
/// made, or because a previous `settle_now` already consumed it — must not
/// re-run the handler over stale state.
#[tokio::test(start_paused = true)]
async fn settle_now_is_a_no_op_when_nothing_is_pending() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record);
    let key = PathBuf::from("/vault-c");

    lanes.settle_now(&key).await; // never requested: no lane exists yet
    assert!(calls().is_empty());

    lanes.request(&key, 1);
    lanes.settle_now(&key).await; // consumes it
    lanes.settle_now(&key).await; // nothing left
    assert_eq!(calls(), vec![1], "the second settle_now must not re-run the handler");
}

/// `settle_all` must complete every pending lane, not just one — the shape a
/// folder switch needs when it wants to finish whichever folder was
/// previously active without knowing its key in advance.
#[tokio::test(start_paused = true)]
async fn settle_all_completes_every_pending_lane() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record);

    lanes.request(&PathBuf::from("/vault-e1"), 11);
    lanes.request(&PathBuf::from("/vault-e2"), 22);
    lanes.request(&PathBuf::from("/vault-e3"), 33);

    lanes.settle_all().await;

    let mut seen = calls();
    seen.sort();
    assert_eq!(seen, vec![11, 22, 33], "every pending lane must have run, not just one");
}

/// Continuous requests, faster than `idle` ever elapses, must still settle no
/// less often than `max_defer` — the same guarantee `search_lane`'s own
/// `continuous_saving_indexes_no_less_often_than_max_defer` pinned before
/// this timing logic moved here.
#[tokio::test(start_paused = true)]
async fn continuous_requests_are_bounded_by_max_defer() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let idle = Duration::from_secs(20);
    let max_defer = Duration::from_secs(120);
    let lanes = Lanes::new(idle, max_defer, record);
    let key = PathBuf::from("/vault-d");

    // 30 requests, 5 s apart: 150 s of continuous activity under a 120 s cap.
    for i in 0..30u32 {
        lanes.request(&key, i);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    let seen = calls();
    assert_eq!(
        seen.len(),
        1,
        "150 s of continuous requests under a 120 s max_defer must force exactly one call, got {:?}",
        seen
    );
}

/// `evict` must never discard work: a lane with a request still pending is
/// left in place, and that request still runs. Once settled, the lane goes.
#[tokio::test(start_paused = true)]
async fn evict_keeps_a_pending_lane_and_removes_it_once_settled() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record);
    let key = PathBuf::from("/vault-f");

    lanes.request(&key, 5);
    lanes.evict(&key);
    assert!(lanes.has_lane(&key), "a pending lane must survive eviction");
    lanes.settle_now(&key).await;
    assert_eq!(calls(), vec![5], "the pending request must still run after an eviction attempt");

    lanes.evict(&key);
    assert!(!lanes.has_lane(&key), "a settled lane must be evicted");
}

// ── The derived-work gate ───────────────────────────────────────────────────

use crate::ops::watch::cadence::Cadence;

/// The cadence every gated test lane follows. Shared, so each test sets it
/// first and holds `TEST_LOCK` throughout.
static CADENCE: std::sync::LazyLock<watch::Sender<Cadence>> =
    std::sync::LazyLock::new(|| watch::channel(Cadence::Live).0);

fn follow_test_cadence(_: &u32) -> DerivedWorkGate {
    DerivedWorkGate::following(CADENCE.subscribe())
}

/// While nobody is looking, a quiesced request must stay pending however long
/// the wait, and newer requests must still replace it. The flip to `Live` then
/// runs exactly one pass, over the latest request, without a second idle wait.
#[tokio::test(start_paused = true)]
async fn a_closed_gate_holds_the_pass_until_live_then_runs_it_once_with_the_latest() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    CADENCE.send_replace(Cadence::Background);
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record)
        .with_gate(follow_test_cadence);
    let key = PathBuf::from("/vault-gated");

    for i in 0..3u32 {
        lanes.request(&key, i);
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
    tokio::time::sleep(Duration::from_secs(600)).await;
    assert!(calls().is_empty(), "nothing may run while the gate is closed");
    assert!(lanes.is_pending(&key), "the latest request must stay pending");

    CADENCE.send_replace(Cadence::Live);
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(calls(), vec![2], "the flip to Live must run one pass, over the latest request");

    tokio::time::sleep(Duration::from_secs(600)).await;
    assert_eq!(calls(), vec![2], "and only one");
}

/// A forced settle (publish, quit, folder switch) must not wait for the window
/// to become visible.
#[tokio::test(start_paused = true)]
async fn settle_now_runs_a_pending_request_while_the_gate_is_closed() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    CADENCE.send_replace(Cadence::Background);
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record)
        .with_gate(follow_test_cadence);
    let key = PathBuf::from("/vault-gated-settle");

    lanes.request(&key, 9);
    tokio::time::sleep(Duration::from_secs(60)).await;
    lanes.settle_now(&key).await;
    assert_eq!(calls(), vec![9]);

    CADENCE.send_replace(Cadence::Live);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(calls(), vec![9], "the settled request must not run again when the gate opens");
}

/// A lane evicted while its pass waits on a closed gate (a forced settle took
/// the request, then the session ended) must exit, not linger until the window
/// is visible again.
#[tokio::test(start_paused = true)]
async fn evicting_a_lane_that_waits_on_a_closed_gate_ends_its_task() {
    let _serialize = TEST_LOCK.lock().await;
    CALLS.lock().unwrap_or_else(PoisonError::into_inner).clear();
    CADENCE.send_replace(Cadence::Background);
    let lanes = Lanes::new(Duration::from_secs(20), Duration::from_secs(120), record)
        .with_gate(follow_test_cadence);
    let key = PathBuf::from("/vault-gated-evict");

    lanes.request(&key, 5);
    tokio::time::sleep(Duration::from_secs(60)).await;
    let lane = lanes.lane_handle(&key).expect("the lane exists");
    lanes.settle_now(&key).await;
    lanes.evict(&key);
    assert!(!lanes.has_lane(&key));

    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(lane.upgrade().is_none(), "the evicted lane's task must have exited");
}
