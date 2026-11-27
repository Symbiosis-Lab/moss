//! Tests for the upload sizing policy: each is an assertion about a request's
//! duration while it shares the link, which size-only routing cannot express.

use super::*;
use std::time::Duration;

/// Concurrencies a server may hint; the edge bound must hold at each.
const CONCURRENCIES: std::ops::RangeInclusive<usize> = 1..=12;

/// The whole point: with every slot busy, a planned request still finishes
/// inside the edge budget. Asserted against the *planned* size, since the
/// ceiling is only reachable on a link fast enough to make it fit.
///
/// The one exception is stated rather than hidden: below
/// `MIN_ESCALATION_SIZE`'s implied rate the floor request is larger than the
/// edge budget allows, and that link cannot publish at all.
#[test]
fn a_request_planned_at_any_concurrency_fits_the_edge_budget() {
    for rate in [8u64 * 1024, 25 * 1024, 28_190, 50 * 1024, 135 * 1024, 1 << 20, 1 << 26, u64::MAX] {
        for c in CONCURRENCIES {
            let planned = plan_request_size(rate, c);
            if planned == MIN_ESCALATION_SIZE {
                continue;
            }
            let worst = worst_case_request_seconds(c, planned, rate);
            assert!(
                worst <= EDGE_BUDGET.as_secs_f64(),
                "at {rate} B/s with {c} at once a planned request takes {worst:.0}s, and the edge cuts at {}s",
                EDGE_BUDGET.as_secs()
            );
        }
    }
    assert!(worst_case_request_seconds(6, MIN_ESCALATION_SIZE, 5 * 1024) > EDGE_BUDGET.as_secs_f64());
}

/// Sizing by the concurrency actually used: a deploy run at the server's
/// default hint of 6 must not be planned as if 3 shared the link.
#[test]
fn the_plan_divides_the_edge_budget_by_the_concurrency_in_use() {
    const SLOW: u64 = 50 * 1024;
    let at_hint = plan_request_size(SLOW, 6);
    assert!(at_hint < plan_request_size(SLOW, 3), "six at once must plan smaller requests than three");
    assert!(worst_case_request_seconds(6, at_hint, SLOW) <= EDGE_BUDGET.as_secs_f64());
}

/// One request alone on a slow link gets the whole link's share: 1 MiB at
/// 28 KB/s, not the 256 KiB floor a divided budget would give.
#[test]
fn a_solo_upload_is_sized_for_the_whole_link() {
    assert_eq!(plan_request_size(28_190, 1), 1024 * 1024);
}

/// Client CHUNK_SIZE_MAX must stay under the server's chunk cap, or every
/// PATCH 413s.
#[test]
fn chunk_size_stays_under_the_server_cap() {
    const SERVER_CHUNK_MAX_BYTES: usize = 50 * 1024 * 1024;
    assert!(CHUNK_SIZE_MAX <= SERVER_CHUNK_MAX_BYTES);
}

/// A request planned at 25 KB/s fits its target duration.
#[test]
fn a_request_on_a_25_kb_link_fits_its_target() {
    const SLOW: u64 = 25 * 1024;
    let planned = plan_request_size(SLOW, 1);
    assert!(planned as f64 / SLOW as f64 <= TARGET_REQUEST_SECONDS as f64);
}

/// A link measured to be doing fine still sends 4 MiB requests.
#[test]
fn a_healthy_link_still_gets_the_full_four_mib_request() {
    for c in 1..=6 {
        assert_eq!(plan_request_size(10 * 1024 * 1024, c), CHUNK_SIZE_MAX);
        assert_eq!(plan_request_size(u64::MAX, c), CHUNK_SIZE_MAX);
    }
}

/// The seed exists to make the opening request INITIAL_REQUEST_SIZE at every
/// concurrency; a seed that saturates the plan is decorative.
#[test]
fn the_first_request_of_a_publish_is_the_initial_size_not_the_ceiling() {
    assert!(INITIAL_REQUEST_SIZE < CHUNK_SIZE_MAX, "an initial size at the ceiling is no initial size");
    for c in CONCURRENCIES {
        assert_eq!(Throughput::new(c).plan_request_size(), INITIAL_REQUEST_SIZE, "at concurrency {c}");
    }
}

#[test]
fn planned_sizes_are_aligned_and_never_below_the_floor() {
    for bps in [0u64, 1, 1024, 25 * 1024, 50 * 1024, 135 * 1024, 1 << 30] {
        for c in 0..=6 {
            let size = plan_request_size(bps, c);
            assert!(size >= MIN_ESCALATION_SIZE, "{bps} x{c} -> {size}");
            assert!(size <= CHUNK_SIZE_MAX, "{bps} x{c} -> {size}");
            assert_eq!(size % (256 * 1024), 0, "{bps} x{c} -> {size} is not 256 KiB-aligned");
        }
    }
}

#[test]
fn the_estimate_converges_on_a_link_that_slowed_down() {
    let tp = Throughput::new(1);
    assert_eq!(tp.bytes_per_sec(), seed_bandwidth(1));
    // Ten 1 MiB requests each taking 40 s = ~26 KB/s.
    for _ in 0..10 {
        tp.observe(1024 * 1024, Duration::from_secs(40));
    }
    let seen = tp.bytes_per_sec();
    assert!((24 * 1024..30 * 1024).contains(&seen), "estimate settled at {seen} B/s, expected ~26 KB/s");
}

#[test]
fn overhead_sized_observations_do_not_move_the_estimate() {
    let tp = Throughput::new(1);
    tp.observe(4 * 1024, Duration::from_millis(40)); // 100 KB/s of handshake
    tp.observe(8 * 1024 * 1024, Duration::from_millis(3)); // implausible: cached/local
    assert_eq!(tp.bytes_per_sec(), seed_bandwidth(1));
}

#[test]
fn a_small_but_slow_request_is_a_valid_bandwidth_sample() {
    let tp = Throughput::new(1);
    for _ in 0..10 {
        tp.observe(200 * 1024, Duration::from_secs(8));
    }
    let seen = tp.bytes_per_sec();
    assert!((24 * 1024..29 * 1024).contains(&seen), "estimate settled at {seen} B/s; ten 200 KB requests of 8 s each are ~25 KB/s");
}

/// A site of small files learns the link before it meets a big one, so the
/// big one's chunks are sized by measurement.
#[test]
fn a_site_of_small_files_still_learns_the_link_before_it_meets_a_big_one() {
    const UPLINK: u64 = 25 * 1024;
    let tp = Throughput::new(1);
    for i in 0..200u64 {
        // 100-250 KB files, each timed at the real uplink plus one round trip.
        let bytes = 100 * 1024 + (i % 150) * 1024;
        let elapsed = Duration::from_secs_f64(bytes as f64 / UPLINK as f64) + Duration::from_millis(250);
        tp.observe(bytes, elapsed);
    }
    let seen = tp.bytes_per_sec();
    assert!((20 * 1024..32 * 1024).contains(&seen), "estimate settled at {seen} B/s on a 25 KB/s link");
    assert!(tp.plan_request_size() as f64 / seen as f64 <= TARGET_REQUEST_SECONDS as f64, "each request must fit the target");
}

// ── Deploy summary ───────────────────────────────────────────────────────────

#[test]
fn the_deploy_summary_reports_achieved_rate_and_retries() {
    let tp = Throughput::new(6);
    tp.note_chunked_file();
    for _ in 0..3 {
        tp.note_request();
    }
    tp.note_request_ok();
    tp.note_request_ok();
    tp.observe(1024 * 1024, Duration::from_secs(40));
    tp.observe(1024 * 1024, Duration::from_secs(40));
    let line = tp.deploy_summary(Duration::from_secs(100));
    assert!(line.contains("2097152 bytes"), "{line}");
    assert!(line.contains("= 20971 B/s achieved"), "{line}");
    assert!(line.contains("3 requests, 1 retried"), "{line}");
    assert!(line.contains("1 chunked files"), "{line}");
    assert!(line.contains("concurrency 6"), "{line}");
}

#[test]
fn the_deploy_summary_survives_zero_bytes_and_zero_elapsed() {
    let line = Throughput::new(1).deploy_summary(Duration::ZERO);
    assert!(line.contains("0 bytes"), "{line}");
}

#[test]
fn the_deploy_summary_reports_self_healed_files() {
    let tp = Throughput::new(1);
    tp.note_self_heal();
    tp.note_self_heal();
    let line = tp.deploy_summary(Duration::from_secs(1));
    assert!(line.contains("2 self-healed drift(s)"), "{line}");
}

#[test]
fn confirmed_bytes_count_even_below_the_sample_gate() {
    let tp = Throughput::new(1);
    tp.observe(4 * 1024, Duration::from_millis(40));
    assert_eq!(tp.bytes_per_sec(), seed_bandwidth(1));
    assert!(tp.deploy_summary(Duration::from_secs(1)).contains("4096 bytes"));
}

// ── Manifest deadlines scale with the manifest ───────────────────────────────

/// A 10k-file site's ~1 MB manifest could never be POSTed under a flat 150 s
/// deadline on a slow link — the same defect as the large-file case, one axis
/// over.
#[test]
fn the_manifest_deadline_grows_with_the_manifest() {
    let small = manifest_request_timeout(2 * 1024);
    let big = manifest_request_timeout(1024 * 1024);
    assert_eq!(small, UPLOAD_REQUEST_TIMEOUT, "a tiny body gets the plain floor");
    assert!(big > small);
    // 1 MB at the 5 KB/s the tester was NOT even as slow as: >= 200 s of pure
    // transfer, and the deadline must cover it.
    assert!(
        big.as_secs_f64() >= 1024.0 * 1024.0 / (5.0 * 1024.0),
        "{}s does not cover the transfer it is supposed to bound",
        big.as_secs()
    );
}

/// The floor still exists: a stalled *small* manifest must fail in bounded
/// time rather than inherit a deadline sized for a body it does not have.
#[test]
fn the_manifest_deadline_never_drops_below_the_request_timeout() {
    assert!(manifest_request_timeout(0) >= UPLOAD_REQUEST_TIMEOUT);
}

// ── Retry escalation ─────────────────────────────────────────────────────────

/// Retrying the identical request after a 524 is what spent that large site's 600 s
/// budget on three failures that each took exactly as long as the first.
#[test]
fn escalation_actually_shrinks_the_request() {
    assert!(escalate_down(CHUNK_SIZE_MAX) < CHUNK_SIZE_MAX);
}

#[test]
fn escalation_halves_until_it_reaches_the_floor_and_then_stops() {
    let mut size = CHUNK_SIZE_MAX;
    let mut steps = 0;
    loop {
        let next = escalate_down(size);
        if next == size {
            break;
        }
        assert!(next < size, "escalation must be monotonically decreasing");
        size = next;
        steps += 1;
        assert!(steps < 64, "escalation did not converge");
    }
    assert_eq!(size, MIN_ESCALATION_SIZE);
}

/// A size already at or below the floor must be a fixed point, not zero — a
/// zero-byte request would loop forever making no progress.
#[test]
fn escalation_never_reaches_zero() {
    assert_eq!(escalate_down(1), MIN_ESCALATION_SIZE);
    assert_eq!(escalate_down(0), MIN_ESCALATION_SIZE);
    assert_eq!(escalate_down(MIN_ESCALATION_SIZE), MIN_ESCALATION_SIZE);
}

/// Even the smallest request the escalation can produce must clear the edge
/// budget at the seed rate with room to spare — otherwise escalating is
/// theatre.
#[test]
fn the_escalation_floor_is_comfortably_sendable_on_the_slowest_link() {
    let worst =
        worst_case_request_seconds(6, MIN_ESCALATION_SIZE, seed_bandwidth(6));
    assert!(worst < EDGE_BUDGET.as_secs_f64() / 3.0, "{:.1}s", worst);
}

// ── Timeout ──────────────────────────────────────────────────────────────────

/// Above CF's cut, so the 524 stays the authoritative signal. Below it, moss
/// would abort requests CF was still happily proxying — including a large live
/// site's deploy's own successful 121 s PUT.
#[test]
fn the_request_timeout_sits_above_the_edge_budget() {
    assert!(
        UPLOAD_REQUEST_TIMEOUT > EDGE_BUDGET,
        "a timeout below Cloudflare's {}s cut would kill requests that were going to succeed \
         (the 2026-08-03 deploy contained a successful 121s PUT)",
        EDGE_BUDGET.as_secs()
    );
}

/// ...but not so far above that a dead connection ties up a window slot for
/// minutes. The old 300 s waited five minutes for a response that could not
/// arrive after 125 s.
#[test]
fn the_request_timeout_is_not_wildly_above_the_edge_budget() {
    assert!(UPLOAD_REQUEST_TIMEOUT < EDGE_BUDGET * 2);
}
