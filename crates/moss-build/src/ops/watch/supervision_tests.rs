use super::*;

/// The exemptions, pinned as the design states them: an in-flight event (any
/// delivered batch) and a confessed loss both disarm the candidate — only
/// total silence across the full window is a strike.
#[test]
fn only_total_silence_across_the_window_is_a_strike() {
    let h = WatcherHealth::new();

    // Silence: the incident's shape.
    let armed = h.mark();
    assert_eq!(h.judge(armed), StrikeVerdict::AgedStrike);

    // A delivered batch — even one the pump would filter — is liveness.
    let armed = h.mark();
    h.note_batch();
    assert_eq!(h.judge(armed), StrikeVerdict::WatcherSpoke);

    // A rescan-flag confession means healthy-and-honest, never a teardown.
    let armed = h.mark();
    h.note_confession();
    assert_eq!(h.judge(armed), StrikeVerdict::ConfessedLoss);
}

/// Counters that moved BEFORE arming are not evidence: the mark is the
/// boundary, or a busy morning's batches would exempt an afternoon death.
#[test]
fn history_before_the_mark_does_not_exempt() {
    let h = WatcherHealth::new();
    h.note_batch();
    h.note_confession();
    let armed = h.mark();
    assert_eq!(h.judge(armed), StrikeVerdict::AgedStrike);
}

/// The first strike recreates immediately — one aged strike IS the design's
/// threshold — and the next is held for the backoff, so a persistently broken
/// environment cannot spin a teardown/re-register loop at walk cadence.
#[test]
fn repeated_strikes_back_off_instead_of_hot_recreating() {
    let h = WatcherHealth::new();
    assert!(h.strike("test"), "first strike recreates immediately");
    assert!(
        !h.strike("test"),
        "a second strike inside the backoff window is suppressed (sweep-only degradation)"
    );
}

/// A lone strike is an ordinary self-heal, not yet the state a host should
/// surface to the user — `is_degraded` must not flap on routine recovery.
#[test]
fn a_lone_strike_is_not_yet_degraded() {
    let h = WatcherHealth::new();
    assert!(h.strike("test"));
    assert!(!h.is_degraded(), "one recreation is a normal self-heal, not degraded");
}

/// A strike the backoff suppresses IS the "degraded to sweep-only operation"
/// state the log already names — `is_degraded` is the only way a host
/// outside this module can learn that, and nothing wired it in before this
/// method existed, leaving a backoff window as long as `RECREATE_BACKOFF_CAP`
/// (up to an hour) with no user-visible signal.
#[test]
fn a_backed_off_strike_reports_degraded() {
    let h = WatcherHealth::new();
    assert!(h.strike("test"));
    assert!(!h.strike("test"), "second strike inside the window is suppressed");
    assert!(h.is_degraded(), "the suppressed strike is the degraded state a host should surface");
}

/// A delivered batch is proof of life: it clears `degraded_logged` exactly
/// like it resets the backoff exponent, so a host's health surface heals the
/// instant the watcher does.
#[test]
fn a_delivered_batch_clears_degraded() {
    let h = WatcherHealth::new();
    assert!(h.strike("test"));
    assert!(!h.strike("test"));
    assert!(h.is_degraded());
    h.note_batch();
    assert!(!h.is_degraded(), "a live batch proves the watcher recovered");
}

/// A delivered batch is proof the recreated watcher works, so the backoff
/// exponent resets — the next genuine death gets a prompt recreation again
/// instead of inheriting an hour-long wait from a bad spell last week.
#[test]
fn a_delivered_batch_resets_the_recreation_backoff() {
    let h = WatcherHealth::new();
    assert!(h.strike("test"));
    h.note_batch();
    assert_eq!(h.recreations.load(std::sync::atomic::Ordering::SeqCst), 0);
    // The clock still applies (recreate_backoff(0) is zero wait), so the next
    // strike recreates immediately.
    assert!(h.strike("test"));
}

/// The backoff curve itself: immediate, then 60s doubling to the 1h cap —
/// "occasional watcher retries", never never-again.
#[test]
fn the_backoff_curve_doubles_to_a_cap_and_stays_there() {
    assert_eq!(recreate_backoff(0), Duration::ZERO);
    assert_eq!(recreate_backoff(1), RECREATE_BACKOFF_BASE);
    assert_eq!(recreate_backoff(2), RECREATE_BACKOFF_BASE * 2);
    assert_eq!(recreate_backoff(7), RECREATE_BACKOFF_CAP);
    assert_eq!(recreate_backoff(u32::MAX), RECREATE_BACKOFF_CAP, "no overflow at the tail");
}

/// A strike's notify permit survives the watcher task being busy: the request
/// is a stored permit, not an edge someone must be awaiting.
#[tokio::test]
async fn a_recreate_request_is_not_lost_on_a_busy_watcher() {
    let h = Arc::new(WatcherHealth::new());
    assert!(h.strike("test"));
    // Nobody was awaiting when the strike landed; the permit is stored.
    tokio::time::timeout(Duration::from_secs(1), h.recreate_requested())
        .await
        .expect("the stored permit completes the wait immediately");
}

/// Registry semantics mirror the worker's: a replaced ledger's late
/// deregistration cannot evict its successor.
#[test]
fn deregister_only_removes_its_own_ledger() {
    let first = register("/tmp/moss-supervision-test");
    let second = register("/tmp/moss-supervision-test");
    deregister("/tmp/moss-supervision-test", &first);
    assert!(
        get("/tmp/moss-supervision-test").is_some_and(|h| Arc::ptr_eq(&h, &second)),
        "the successor survives the predecessor's cleanup"
    );
    deregister("/tmp/moss-supervision-test", &second);
    assert!(get("/tmp/moss-supervision-test").is_none());
}

/// The sweep-side wiring, driven end to end at the unit level (spec review
/// should-fix 2): a fresh drift dispatch arms, the next pass matures, and
/// total silence produces a recreate request the watcher task can await.
/// The registry is the real one — each test uses its own folder key.
#[tokio::test]
async fn arming_then_silence_matures_into_a_recreate_request() {
    let folder = "/tmp/moss-strike-arm-silence";
    let h = register(folder);
    let mut arm = StrikeArm::new();

    // Pass N: the sweep catches drift the watcher never delivered.
    arm.arm(folder, false);
    // Pass N+1: nothing arrived in the whole window.
    assert!(arm.mature(folder), "silence over the window is the aged strike");
    tokio::time::timeout(Duration::from_secs(1), h.recreate_requested())
        .await
        .expect("the strike's recreate request reaches the watcher task");
    // The candidate is consumed: maturity is a one-shot verdict.
    assert!(!arm.mature(folder), "a consumed candidate cannot strike again");
    deregister(folder, &h);
}

/// A watcher batch between arming and maturity disarms — the change was in
/// flight, not lost.
#[test]
fn a_batch_inside_the_window_disarms_the_candidate() {
    let folder = "/tmp/moss-strike-arm-batch";
    let h = register(folder);
    let mut arm = StrikeArm::new();
    arm.arm(folder, false);
    h.note_batch();
    assert!(!arm.mature(folder));
    deregister(folder, &h);
}

/// A new top-level entry is a change the watcher never could have delivered
/// (the root is unwatched on macOS), so the tick that goes dirty from the
/// top-level diff must not arm — even when the same tick's walk reads those
/// entries as fresh drift (adversarial review F3: on a cloud root this fired
/// nearly every time a file landed at the vault root, tearing down a healthy
/// watcher under an idle user).
#[test]
fn a_top_level_change_never_arms_the_candidate() {
    let folder = "/tmp/moss-strike-arm-top-level";
    let h = register(folder);
    let mut arm = StrikeArm::new();
    arm.arm(folder, true);
    assert!(!arm.mature(folder), "top-level dirt suppresses the walk half of the arming too");
    assert_eq!(h.recreations.load(std::sync::atomic::Ordering::SeqCst), 0);
    deregister(folder, &h);
}

/// An unreadable root disarms: the environment is on trial, not the watcher.
#[test]
fn an_unreadable_root_disarms_the_candidate() {
    let folder = "/tmp/moss-strike-arm-unreadable";
    let h = register(folder);
    let mut arm = StrikeArm::new();
    arm.arm(folder, false);
    arm.disarm();
    assert!(!arm.mature(folder));
    deregister(folder, &h);
}

/// The heartbeat that exists to make a dead loop diagnosable printed full
/// health over a folder whose eligible set was empty — 585 passes of it, on a
/// client's live vault (#1080). So a blind folder says so, and says it once:
/// the condition is standing, and a line per sweep pass would bury the log it
/// exists to make readable.
#[test]
fn a_blind_folder_is_reported_once() {
    assert!(note_blind_folder("/vault/blind-once", 12), "the first pass says it");
    assert!(!note_blind_folder("/vault/blind-once", 12), "every later pass is quiet");
    assert!(
        note_blind_folder("/vault/blind-once-other", 3),
        "and it is per folder, not per process"
    );
}

/// An empty folder is not a blind one. Nothing is wrong with a vault the user
/// has not put anything in yet, and warning about it would train the warning
/// away.
#[test]
fn an_empty_folder_is_not_blind() {
    assert!(!note_blind_folder("/vault/blind-empty", 0));
}
