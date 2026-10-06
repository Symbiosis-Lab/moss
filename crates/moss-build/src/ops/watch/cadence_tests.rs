//! Tests for the cadence signal and its shared ticker.
//!
//! Paused-clock throughout (`#[tokio::test(start_paused = true)]`) so the
//! 2s/30s intervals resolve instantly instead of costing real wall-clock
//! time — the first `tokio::time::pause` precedent in this crate outside
//! `seta/chunked_upload_tests.rs`.

use super::*;

#[test]
fn live_interval_is_two_seconds() {
    assert_eq!(Cadence::Live.interval(), Duration::from_secs(2));
}

#[test]
fn background_interval_is_thirty_seconds() {
    assert_eq!(Cadence::Background.interval(), Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn elapsed_fires_after_the_live_interval_and_rearms_for_the_next_one() {
    let (_tx, rx) = tokio::sync::watch::channel(Cadence::Live);
    let mut ticker = CadenceTicker::new(rx);

    // Racing next_tick() against an advance just short of 2s must not
    // resolve — proven by pairing the wait with a timeout rather than by
    // advancing past 2s first (which would trivially always fire).
    let almost = tokio::time::timeout(Duration::from_millis(1900), ticker.next_tick()).await;
    assert!(almost.is_err(), "should not have fired before the 2s Live interval elapsed");

    // Advancing the remaining ~100ms (the timeout above already consumed
    // 1900ms of virtual time) resolves it, and it rearms for another 2s.
    let tick = ticker.next_tick().await;
    assert_eq!(tick, Tick::Elapsed);

    let again = tokio::time::timeout(Duration::from_millis(1900), ticker.next_tick()).await;
    assert!(again.is_err(), "rearmed interval should also be ~2s, not one-shot");
    let tick = ticker.next_tick().await;
    assert_eq!(tick, Tick::Elapsed);
}

#[tokio::test(start_paused = true)]
async fn a_flip_to_background_does_not_fire_immediately_but_lengthens_the_next_wait() {
    let (tx, rx) = tokio::sync::watch::channel(Cadence::Live);
    let mut ticker = CadenceTicker::new(rx);

    tx.send(Cadence::Background).unwrap();
    // The change itself resolves next_tick() right away — no time advance
    // needed — because `next_tick` races the sleep against `changed()`.
    let tick = tokio::time::timeout(Duration::from_millis(1), ticker.next_tick())
        .await
        .expect("a cadence change must resolve next_tick without waiting out any of the old sleep");
    assert_eq!(tick, Tick::CadenceChanged(Cadence::Background));

    // Rearmed for Background's 30s, not Live's stale 2s: waiting only 2s
    // must not be enough now.
    let too_soon = tokio::time::timeout(Duration::from_secs(2), ticker.next_tick()).await;
    assert!(too_soon.is_err(), "rearmed interval should now be 30s, not the old 2s");
}

#[tokio::test(start_paused = true)]
async fn a_flip_to_live_fires_promptly_without_waiting_out_a_stale_background_sleep() {
    let (tx, rx) = tokio::sync::watch::channel(Cadence::Background);
    let mut ticker = CadenceTicker::new(rx);

    // Let a little time pass — well under both 2s and the 30s Background
    // interval — then flip. If the ticker still had to wait out the stale
    // 30s sleep, this would time out.
    tokio::time::advance(Duration::from_secs(1)).await;
    tx.send(Cadence::Live).unwrap();
    let tick = tokio::time::timeout(Duration::from_millis(1), ticker.next_tick())
        .await
        .expect("a flip to Live must not wait out the stale Background sleep");
    assert_eq!(tick, Tick::CadenceChanged(Cadence::Live));
}

#[tokio::test(start_paused = true)]
async fn cadence_accessor_reflects_the_latest_value_after_a_tick() {
    let (tx, rx) = tokio::sync::watch::channel(Cadence::Live);
    let mut ticker = CadenceTicker::new(rx);
    tx.send(Cadence::Background).unwrap();
    let _ = ticker.next_tick().await;
    assert_eq!(*ticker.cadence().borrow(), Cadence::Background);
}

/// A dropped sender (headless: nothing ever sends a second value) must not
/// turn `next_tick` into a busy loop. `watch::Receiver::changed()` resolves
/// immediately, forever, once its sender is gone — bounded by an outer
/// timeout in case a regression here really does spin instead of ticking.
#[tokio::test(start_paused = true)]
async fn a_dropped_sender_keeps_the_ticker_firing_at_the_fixed_interval_instead_of_busy_looping() {
    let (tx, rx) = tokio::sync::watch::channel(Cadence::Live);
    let mut ticker = CadenceTicker::new(rx);
    drop(tx);

    let start = tokio::time::Instant::now();
    let ran = tokio::time::timeout(Duration::from_secs(20), async {
        for _ in 0..3 {
            assert_eq!(ticker.next_tick().await, Tick::Elapsed);
        }
    })
    .await;
    assert!(ran.is_ok(), "a dropped sender must not hang next_tick forever");

    // The real proof against busy-looping: a spin would resolve all three
    // ticks near-instantly, at ~0 (virtual) elapsed time. Three genuine
    // Live-interval ticks cost ~6s.
    assert!(
        start.elapsed() >= Duration::from_millis(3 * 2000 - 50),
        "three ticks after the sender dropped should each still cost ~2s of \
         (virtual) time, not resolve instantly — got {:?}",
        start.elapsed()
    );
}
