use super::*;

/// Take the global clock lock, tolerating a previous test's panic.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    CLOCK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ── the bound itself ─────────────────────────────────────────

/// The replacement for the deleted `deploy_timeout_*` pins. Those encoded a
/// *total-work* inequality (600 s of upload retries inside a 900 s deploy),
/// which stops being load-bearing once no deadline bounds the whole publish.
/// What must hold now is that the stall window outlasts the longest interval a
/// HEALTHY publish can legitimately stay quiet: one request that goes silent
/// fails at `UPLOAD_REQUEST_TIMEOUT`, and the retry that follows bumps at its
/// attempt boundary, so the interval never chains. The 2 s below is slack for
/// the wait before that bump, not the whole backoff ceiling.
#[test]
fn stall_timeout_outlasts_the_worst_case_quiet_interval() {
    let quiet = crate::seta::upload_policy::UPLOAD_REQUEST_TIMEOUT
        + Duration::from_secs(2);
    assert!(
        STALL_TIMEOUT > quiet,
        "a publish that is alive re-proves it within {quiet:?}; a {STALL_TIMEOUT:?} \
         stall window would kill healthy publishes"
    );
    // Margin, not just ordering: a bound only 1 s above the quiet interval
    // would false-positive on any jitter. The design calls for ~2x.
    assert!(
        STALL_TIMEOUT >= quiet * 2 - Duration::from_secs(5),
        "stall window {STALL_TIMEOUT:?} leaves too little margin over {quiet:?}"
    );
}

// ── the watchdog ─────────────────────────────────────────────

#[tokio::test]
async fn watchdog_fires_once_nothing_bumps() {
    let _g = lock();
    bump();
    let idle = tokio::time::timeout(
        Duration::from_secs(5),
        watchdog(Duration::from_millis(120)),
    )
    .await
    .expect("watchdog must fire on a silent clock");
    assert!(idle >= Duration::from_millis(100), "reported idle: {idle:?}");
}

#[tokio::test]
async fn watchdog_never_fires_while_progress_keeps_arriving() {
    let _g = lock();
    let stall = Duration::from_millis(120);
    let keep_alive = tokio::spawn(async {
        for _ in 0..40 {
            bump();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    // 800 ms of steady progress against a 120 ms window: a total-duration
    // deadline would have fired six times over. This is the 2026-08-04 case —
    // slow but moving must not be cancelled.
    let fired = tokio::time::timeout(Duration::from_millis(600), watchdog(stall)).await;
    assert!(fired.is_err(), "watchdog fired while progress was being credited");
    keep_alive.abort();
}

// ── bounded_by_stall ─────────────────────────────────────────

#[tokio::test]
async fn a_body_that_finishes_returns_its_own_result() {
    let _g = lock();
    let ok: Result<u8, String> =
        bounded_by_stall(async { Ok(7u8) }, Duration::from_millis(200), "test").await;
    assert_eq!(ok.unwrap(), 7);

    let err: Result<u8, String> = bounded_by_stall(
        async { Err("body failed".to_string()) },
        Duration::from_millis(200),
        "test",
    )
    .await;
    assert_eq!(err.unwrap_err(), "body failed");
}

#[tokio::test]
async fn a_quiet_body_is_cancelled_with_the_stall_message() {
    let _g = lock();
    let err: Result<(), String> = bounded_by_stall(
        std::future::pending::<Result<(), String>>(),
        Duration::from_millis(120),
        "test",
    )
    .await;
    let msg = err.expect_err("a body that never progresses must be cancelled");
    assert!(
        msg.contains("publish again"),
        "the stall message must tell the user a retry resumes: {msg}"
    );
    assert!(
        !msg.contains("internet connection"),
        "a slow link is not a broken link — do not send the user to diagnose it: {msg}"
    );
}

#[tokio::test]
async fn a_slow_but_progressing_body_completes() {
    let _g = lock();
    // The regression this whole module exists for: total duration (600 ms)
    // far exceeds the stall window (120 ms), but every 20 ms of it is credited.
    let result: Result<(), String> = bounded_by_stall(
        async {
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                bump();
            }
            Ok(())
        },
        Duration::from_millis(120),
        "test",
    )
    .await;
    assert!(result.is_ok(), "a slow-but-moving publish must not be cancelled");
}
