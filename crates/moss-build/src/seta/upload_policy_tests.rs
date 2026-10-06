//! Tests for the upload sizing policy.
//!
//! These are the tests that would have caught the 2026-08-03 publish failure:
//! every one of them is an assertion about *duration under contention*, which
//! is the thing the old size-only routing could not express.

use super::*;
use std::time::Duration;

/// The whole point. Every constant in this module exists to make this hold.
///
/// The quantity being bounded is one request's duration **while the window is
/// full**, since that is the only situation in which the edge's 125 s read
/// timeout can be reached: `LIMIT_START * planned / aggregate`. The clamp
/// makes it hold *by construction* at every occupancy — including a request
/// sized while alone that the window then fills in behind — rather than by the
/// accident of the floor. It is asserted against the *planned* size rather
/// than against CHUNK_SIZE_MAX, because the ceiling is only reachable on a
/// link fast enough to make it fit — that composition is the actual theorem,
/// and pinning the ceiling alone is what let a saturating seed slip through
/// review.
///
/// The one exception is stated rather than hidden: below
/// `MIN_ESCALATION_SIZE`'s implied rate the floor request is larger than the
/// edge budget allows, and that link cannot publish at all (see the constant's
/// own doc).
#[test]
fn a_request_planned_at_any_occupancy_fits_the_edge_budget() {
    let floor_rate =
        (LIMIT_START * MIN_ESCALATION_SIZE) as u64 / EDGE_BUDGET.as_secs();
    for rate in [
        8u64 * 1024,
        25 * 1024,
        28_190, // the 2026-08-27 Shanghai baseline
        50 * 1024,
        135 * 1024,
        SEED_BANDWIDTH_BYTES_PER_SEC,
        1 << 20,
        1 << 26,
        u64::MAX,
    ] {
        assert!(rate > floor_rate, "sweep must stay above the unpublishable floor");
        for limit in LIMIT_MIN..=LIMIT_MAX {
            for in_flight in 0..=limit {
                let planned = plan_request_size(rate, in_flight, limit);
                let worst = worst_case_request_seconds(limit, planned, rate);
                // The floor exception widens with the limit: the smallest
                // possible request over more concurrent peers takes longer.
                if planned == MIN_ESCALATION_SIZE {
                    continue;
                }
                assert!(
                    worst <= EDGE_BUDGET.as_secs_f64(),
                    "at {rate} B/s with {in_flight} in flight under limit {limit} a planned \
                     request takes {worst:.0}s if the window then fills, and the edge cuts at {}s",
                    EDGE_BUDGET.as_secs()
                );
            }
        }
        // With the window full the tighter target bound applies: each request
        // is sized for its real one-third share, so its own duration is
        // TARGET_REQUEST_SECONDS — unless it is pinned to the floor, where the
        // weaker edge bound above is all there is.
        let planned = plan_request_size(rate, LIMIT_START, LIMIT_START);
        if planned > MIN_ESCALATION_SIZE {
            assert!(
                worst_case_request_seconds(LIMIT_START, planned, rate)
                    <= TARGET_REQUEST_SECONDS as f64,
                "at {rate} B/s a full-window request misses its target duration"
            );
        }
    }
    // Stated, not asserted away: at 5 KB/s even the 256 KiB floor overruns the
    // edge budget. Nothing in this module can fix that — it is what
    // MIN_ESCALATION_SIZE means.
    assert!(
        worst_case_request_seconds(LIMIT_START, MIN_ESCALATION_SIZE, 5 * 1024)
            > EDGE_BUDGET.as_secs_f64()
    );
}

/// **The defect the occupancy divisor fixes.** The 2026-08-27 Shanghai deploy
/// ran one 99.9 MB video alone in the window for 59 minutes at 28,190 B/s;
/// dividing by the ceiling unconditionally planned 412 KB and floored every
/// chunk at 256 KiB — 262 requests. Planned by actual occupancy and clamped at
/// the edge bound, the same link gets 1 MiB chunks: ~96 requests, each still
/// inside the edge deadline even if the window fills right after sizing.
#[test]
fn a_solo_upload_is_sized_for_the_whole_link_not_a_third_of_it() {
    const SHANGHAI: u64 = 28_190;
    assert_eq!(plan_request_size(SHANGHAI, 1, LIMIT_START), 1024 * 1024);
    // The pre-fix shape, kept so nobody re-derives it: the ceiling divisor
    // landed under the floor and the floor chose every chunk.
    let ceiling_divided = SHANGHAI * TARGET_REQUEST_SECONDS / (LIMIT_START as u64);
    assert!(ceiling_divided < (2 * MIN_ESCALATION_SIZE) as u64);
}

/// The file that killed the deploy must take the chunked path even on a link
/// fast enough to justify the maximum request size.
///
/// 8,910,888 bytes is `audio/dreamin.mp3` exactly. Under the old 20 MB
/// threshold it went out as one PUT and could not finish inside 125s.
#[test]
fn the_mp3_that_broke_sample_site_now_chunks() {
    for in_flight in 0..=LIMIT_START {
        assert!(needs_chunking(8_910_888, SEED_BANDWIDTH_BYTES_PER_SEC, in_flight, LIMIT_START));
    }
}

/// The one file in that deploy that *barely* succeeded — 3,824,064 bytes in
/// 121 s — stays a single PUT once the link has been **measured** as healthy,
/// so single-PUT remains the common path for ordinary page assets. A threshold
/// that swept everything into sessions would add a `getUsedBytes` recursive
/// stat walk per file on the server.
///
/// Note it does NOT stay a single PUT at the seed: before any measurement
/// exists the deploy sends 1 MiB requests, which is the whole point of
/// [`INITIAL_REQUEST_SIZE`] — the routing is by evidence, and at request one
/// there is none.
#[test]
fn ordinary_page_assets_use_a_single_put_once_the_link_is_measured() {
    const HEALTHY: u64 = 1024 * 1024; // 1 MB/s aggregate
    for in_flight in 0..=LIMIT_START {
        assert!(!needs_chunking(3_824_064, HEALTHY, in_flight, LIMIT_START));
        assert!(!needs_chunking(0, SEED_BANDWIDTH_BYTES_PER_SEC, in_flight, LIMIT_START));
    }
}

/// Client CHUNK_SIZE_MAX must stay under seta's CHUNK_MAX_BYTES, or every
/// PATCH 413s. Cross-repo contract: moss-seta/src/config/limits.ts.
#[test]
fn chunk_size_stays_under_the_server_cap() {
    const SERVER_CHUNK_MAX_BYTES: usize = 50 * 1024 * 1024;
    assert!(CHUNK_SIZE_MAX <= SERVER_CHUNK_MAX_BYTES);
}

// ── Throughput-derived request sizing (2026-08-04) ───────────────────────────

/// **The bug this replaced a constant to fix.** `assets/95c91eea845cba75.png`
/// is 3,911,241 bytes; at 25 KB/s it needs ~156 s, past the 150 s request
/// timeout. Under the old fixed 4 MiB threshold it took the single-PUT path,
/// which has no chunking, no escalation and no resume — so it failed at
/// exactly 150.0 s, twice, and could not have succeeded at any retry count.
#[test]
fn a_39_mb_file_on_a_25_kb_link_is_chunked_not_single_put() {
    const SLOW: u64 = 25 * 1024;
    // in_flight = 1 is the hardest case: it is where the plan is largest.
    assert!(
        needs_chunking(3_911_241, SLOW, 1, LIMIT_START),
        "a request that cannot finish inside UPLOAD_REQUEST_TIMEOUT must not go out as \
         an unresumable single PUT"
    );
    let planned = plan_request_size(SLOW, 1, LIMIT_START);
    let seconds = planned as f64 / SLOW as f64;
    assert!(
        seconds <= TARGET_REQUEST_SECONDS as f64,
        "a planned request takes {:.0}s at 25 KB/s",
        seconds
    );
}

/// Nothing changes for a link *measured* to be doing fine: the plan saturates
/// at the ceiling, so a healthy publish still sends 4 MiB requests.
#[test]
fn a_healthy_link_still_gets_the_full_four_mib_request() {
    for in_flight in 0..=LIMIT_START {
        assert_eq!(plan_request_size(10 * 1024 * 1024, in_flight, LIMIT_START), CHUNK_SIZE_MAX);
        assert_eq!(plan_request_size(u64::MAX, in_flight, LIMIT_START), CHUNK_SIZE_MAX);
    }
}

/// **The seed must not saturate the plan.** Seeding a bandwidth *guess* is how
/// the design's `INITIAL_CHUNK_SIZE = 1 MiB` came to be written down and never
/// implemented: 135 KB/s x 45 s = 6.07 MB clamps to the 4 MiB ceiling, so the
/// first request of every publish was the largest one the client can make — the
/// opposite of "start small and grow from measurement", and a guaranteed 150 s
/// timeout on the link that large site actually had.
///
/// Any seed at or above ~98 KB/s has this property, which is why the seed is
/// now derived from the size instead of chosen as a rate.
#[test]
fn the_first_request_of_a_publish_is_the_initial_size_not_the_ceiling() {
    assert!(INITIAL_REQUEST_SIZE < CHUNK_SIZE_MAX, "an initial size at the ceiling is no initial size");
    assert_eq!(
        plan_request_size(SEED_BANDWIDTH_BYTES_PER_SEC, 1, LIMIT_START),
        INITIAL_REQUEST_SIZE,
        "the seed exists to make the opening request INITIAL_REQUEST_SIZE; if this fails the \
         plan is saturating and the seed is decorative"
    );
    // The shape of the original defect, kept so nobody re-derives it: the old
    // seed x the target time overran the ceiling on its own, so the size
    // ceiling — not the measurement — chose every opening request.
    assert!(135 * 1024 * TARGET_REQUEST_SECONDS as usize > CHUNK_SIZE_MAX);
}

/// Every planned size is 256 KiB-aligned (GCS and Cloudflare Stream both
/// require it of resumable chunks) and never collapses to a size where
/// per-request overhead dominates the payload.
#[test]
fn planned_sizes_are_aligned_and_never_below_the_floor() {
    for bps in [0u64, 1, 1024, 25 * 1024, 50 * 1024, 135 * 1024, 1 << 30] {
        for in_flight in 0..=LIMIT_START {
            let size = plan_request_size(bps, in_flight, LIMIT_START);
            assert!(size >= MIN_ESCALATION_SIZE, "{bps} x{in_flight} -> {size}");
            assert!(size <= CHUNK_SIZE_MAX, "{bps} x{in_flight} -> {size}");
            assert_eq!(size % (256 * 1024), 0, "{bps} x{in_flight} -> {size} is not 256 KiB-aligned");
        }
    }
}

/// A request that fits at the current estimate is not chunked; one byte more
/// is. The routing threshold and the chunk size are the same number, so a file
/// just over the line becomes exactly two requests of a size already planned
/// to fit.
#[test]
fn the_routing_threshold_is_exactly_the_planned_request_size() {
    const SLOW: u64 = 25 * 1024;
    for in_flight in 0..=LIMIT_START {
        let planned = plan_request_size(SLOW, in_flight, LIMIT_START) as u64;
        assert!(!needs_chunking(planned, SLOW, in_flight, LIMIT_START));
        assert!(needs_chunking(planned + 1, SLOW, in_flight, LIMIT_START));
    }
}

/// The estimate has to *move* toward what the link is actually doing, or the
/// whole exercise is a differently-spelled constant.
#[test]
fn the_estimate_converges_on_a_link_that_slowed_down() {
    let tp = Throughput::new();
    assert_eq!(tp.bytes_per_sec(), SEED_BANDWIDTH_BYTES_PER_SEC);
    // Ten 1 MiB requests each taking 40 s = ~26 KB/s.
    for _ in 0..10 {
        tp.observe(1024 * 1024, Duration::from_secs(40));
    }
    let seen = tp.bytes_per_sec();
    assert!(
        (24 * 1024..30 * 1024).contains(&seen),
        "estimate settled at {} B/s, expected ~26 KB/s",
        seen
    );
    assert!(needs_chunking(3_911_241, seen, 1, LIMIT_START), "the 3.9 MB PNG must now chunk");
}

/// A *short* request measures TLS + signature overhead, not bandwidth. Folding
/// it in would shrink every subsequent request for no reason.
#[test]
fn overhead_sized_observations_do_not_move_the_estimate() {
    let tp = Throughput::new();
    tp.observe(4 * 1024, Duration::from_millis(40)); // 100 KB/s of handshake
    tp.observe(8 * 1024 * 1024, Duration::from_millis(3)); // implausible: cached/local
    assert_eq!(tp.bytes_per_sec(), SEED_BANDWIDTH_BYTES_PER_SEC);
}

/// **A small request is not a bad sample; a short one is.** The estimate used
/// to ignore anything under MIN_ESCALATION_SIZE, which made a site of small
/// files unmeasurable: no sample ever qualified, the estimate stayed at the
/// seed forever, and the one big file in the site was routed by a guess. A
/// 200 KB request that took 8 s is a 25 KB/s reading and there is nothing wrong
/// with it.
#[test]
fn a_small_but_slow_request_is_a_valid_bandwidth_sample() {
    let tp = Throughput::new();
    for _ in 0..10 {
        tp.observe(200 * 1024, Duration::from_secs(8));
    }
    let seen = tp.bytes_per_sec();
    assert!(
        (24 * 1024..29 * 1024).contains(&seen),
        "estimate settled at {seen} B/s; ten 200 KB requests of 8 s each are ~25 KB/s"
    );
}

/// **§9.1's reproduction, end to end in arithmetic.** 200 files under 256 KB
/// plus one 4.0 MB image on a 25 KB/s uplink. Every one of the small files is
/// an honest measurement; if they are discarded the estimate never leaves the
/// seed, `needs_chunking(4_000_000)` is false, the image goes out as one
/// unresumable PUT that needs 160 s against a 150 s timeout, and that publish
/// can never complete at any retry count or on any number of republishes.
#[test]
fn a_site_of_small_files_still_learns_the_link_before_it_meets_a_big_one() {
    const UPLINK: u64 = 25 * 1024;
    let tp = Throughput::new();
    for i in 0..200u64 {
        // 100-250 KB files, each timed at the real uplink plus one round trip.
        let bytes = 100 * 1024 + (i % 150) * 1024;
        let elapsed = Duration::from_secs_f64(bytes as f64 / UPLINK as f64) + Duration::from_millis(250);
        tp.observe(bytes, elapsed);
    }
    let seen = tp.bytes_per_sec();
    assert!(
        (20 * 1024..32 * 1024).contains(&seen),
        "estimate settled at {seen} B/s on a 25 KB/s link"
    );
    assert!(
        needs_chunking(4_000_000, seen, 1, LIMIT_START),
        "a 4.0 MB file at {seen} B/s cannot be a single PUT — it needs {:.0}s against a {}s \
         request timeout",
        4_000_000.0 / seen as f64,
        UPLOAD_REQUEST_TIMEOUT.as_secs()
    );
    let planned = plan_request_size(seen, 1, LIMIT_START) as f64;
    assert!(
        planned / seen as f64 <= TARGET_REQUEST_SECONDS as f64,
        "and each of its requests must fit the target"
    );
}

// ── Deploy summary ───────────────────────────────────────────────────────────

/// The summary line is the deploy's feedback loop: the 2026-08-27 baseline had
/// to be reconstructed from a rotated DEBUG log, and this line is what replaces
/// that. Achieved rate is confirmed bytes over the upload phase's wall time,
/// and retried = attempts the server never confirmed.
#[test]
fn the_deploy_summary_reports_achieved_rate_and_retries() {
    let tp = Throughput::new();
    tp.note_chunked_file();
    tp.note_single_put_file();
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
    assert!(line.contains("1 chunked + 1 single-PUT"), "{line}");
}

/// An empty deploy (all symlinks, or nothing to upload) must produce a line,
/// not a divide-by-zero.
#[test]
fn the_deploy_summary_survives_zero_bytes_and_zero_elapsed() {
    let line = Throughput::new().deploy_summary(Duration::ZERO);
    assert!(line.contains("0 bytes"), "{line}");
}

/// A drifted-hash self-heal (`deploy::upload`) must be visible in the one
/// line a human actually reads at the end of a deploy, not just in a
/// per-file `log::warn!` that can scroll by. Printed even at 0 (the other
/// test above), so the metric's absence is never confusable with "too old a
/// build to have it".
#[test]
fn the_deploy_summary_reports_self_healed_files() {
    let tp = Throughput::new();
    tp.note_self_heal();
    tp.note_self_heal();
    let line = tp.deploy_summary(Duration::from_secs(1));
    assert!(line.contains("2 self-healed drift(s)"), "{line}");
}

/// Confirmed bytes count even when the request is too short to be a bandwidth
/// sample — the summary reports what transferred, the EWMA reports what it
/// believes, and the short-request gate applies only to the latter.
#[test]
fn confirmed_bytes_count_even_below_the_sample_gate() {
    let tp = Throughput::new();
    tp.observe(4 * 1024, Duration::from_millis(40));
    assert_eq!(tp.bytes_per_sec(), SEED_BANDWIDTH_BYTES_PER_SEC);
    assert!(tp.deploy_summary(Duration::from_secs(1)).contains("4096 bytes"));
}

// ── Adaptive concurrency ──────────────────────────────────────────────────────

/// Drive one goodput window: pin occupancy to `occ`, then feed `n` completions
/// of `bytes` each spread across `span`, advancing `*t`. With `n` at the
/// effective limit and `span` well past ADAPT_WINDOW_MIN, the window closes on
/// the last completion.
fn drive_window(
    tp: &Throughput,
    occ: usize,
    n: usize,
    bytes: u64,
    t: &mut Duration,
) {
    let span = Duration::from_secs(30);
    while tp.in_flight() < occ {
        tp.enter_flight();
    }
    while tp.in_flight() > occ {
        tp.exit_flight();
    }
    for _ in 0..n {
        *t += span / n as u32;
        tp.fold_completion(bytes, *t);
    }
}

/// The GFW-boundary shape, in arithmetic: every added connection keeps adding
/// aggregate goodput (measured 2026-08-27: one stream 36,707 B/s, four
/// streams 188,877 B/s), so each closed window beats the last and the limit
/// climbs to its cap. Together with the flat-link test below this is the
/// plan's acceptance criterion: the same loop, fed the two measured link
/// shapes, converges to materially different limits.
#[test]
fn the_limit_climbs_while_added_concurrency_keeps_paying() {
    let tp = Throughput::new();
    assert_eq!(tp.effective_limit(), LIMIT_START);
    let mut t = Duration::ZERO;
    const PER_CONNECTION: u64 = 500 * 1024;
    drive_window(&tp, LIMIT_START, LIMIT_START, PER_CONNECTION, &mut t);
    for _ in 0..8 {
        let eff = tp.effective_limit();
        // Per-connection rate holds as connections are added — the "grows the
        // pie" link. (The first improvement over the opening window models the
        // estimate warming up, which is what bootstraps the probe.)
        drive_window(&tp, eff, eff, 2 * PER_CONNECTION, &mut t);
    }
    assert_eq!(
        tp.effective_limit(),
        LIMIT_MAX,
        "on a link where concurrency multiplies goodput the limit must reach its cap"
    );
}

/// The clean-uplink shape: aggregate goodput is the same number no matter how
/// many connections carry it, so after the first window there is never an
/// improvement and the limit stays where it started. No flag, no knob — the
/// convergence itself is what distinguishes NYC from Shanghai.
#[test]
fn the_limit_settles_where_concurrency_adds_nothing() {
    let tp = Throughput::new();
    let mut t = Duration::ZERO;
    const AGGREGATE_PER_WINDOW: u64 = 6 * 1024 * 1024;
    for _ in 0..8 {
        let eff = tp.effective_limit();
        drive_window(&tp, eff, eff, AGGREGATE_PER_WINDOW / eff as u64, &mut t);
    }
    assert_eq!(tp.effective_limit(), LIMIT_START);
}

/// **The solo-video guard.** A deploy whose one large file holds the window
/// alone shows improving goodput as the estimate warms up, but the window
/// never saturates the limit — so nothing about one more connection was
/// learned, and the limit must hold. If it grew, the edge clamp would shrink
/// and walk the file's chunks back toward the 256 KiB floor: the 2026-08-27
/// defect, reintroduced by the mechanism meant to fix it.
#[test]
fn the_limit_never_grows_without_saturating_the_window() {
    let tp = Throughput::new();
    let mut t = Duration::ZERO;
    let mut bytes = 500 * 1024u64;
    for _ in 0..8 {
        // Completions keep coming faster (warm-up), but only ever one at a time.
        drive_window(&tp, 1, LIMIT_START, bytes, &mut t);
        bytes *= 2;
    }
    assert_eq!(tp.effective_limit(), LIMIT_START);
}

/// A transient-failure spike shrinks the limit regardless of goodput, floored
/// at LIMIT_MIN — failures are the earlier signal that the path is over-driven.
#[test]
fn a_failure_spike_shrinks_the_limit_to_its_floor() {
    let tp = Throughput::new();
    let mut t = Duration::ZERO;
    for _ in 0..6 {
        for _ in 0..8 {
            tp.note_request(); // attempts that never confirm
        }
        let eff = tp.effective_limit();
        drive_window(&tp, eff, eff, 500 * 1024, &mut t);
    }
    assert_eq!(tp.effective_limit(), LIMIT_MIN);
}

/// The guardrails are not the loop's to spend: a limit at its cap still cannot
/// admit a byte past BYTE_BUDGET.
#[test]
fn a_grown_limit_spends_connections_never_memory() {
    assert!(!admits(1, BYTE_BUDGET, 1, LIMIT_MAX));
    assert!(!admits(LIMIT_MAX, 1024, 1024, LIMIT_MAX));
}

// ── Manifest deadlines scale with the manifest (I1) ──────────────────────────

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

// ── Window admission ─────────────────────────────────────────────────────────

#[test]
fn an_empty_window_admits_anything_so_a_huge_file_cannot_deadlock() {
    assert!(admits(0, 0, u64::MAX, LIMIT_START));
}

#[test]
fn admission_is_capped_by_count_even_when_the_bytes_would_fit() {
    // Three 1 KB files: bytes are nowhere near BYTE_BUDGET, but the link is
    // already divided LIMIT_START ways.
    assert!(!admits(LIMIT_START, 3 * 1024, 1024, LIMIT_START));
    assert!(admits(LIMIT_START - 1, 3 * 1024, 1024, LIMIT_START));
}

#[test]
fn admission_is_capped_by_bytes_even_when_the_count_would_fit() {
    // One in-flight task holding the whole budget: a second is refused despite
    // count < LIMIT_START. This is the memory bound doing its job.
    assert!(!admits(1, BYTE_BUDGET, 1, LIMIT_START));
    assert!(admits(1, BYTE_BUDGET - 1, 1, LIMIT_START));
}

/// The old code used `.chunks(20)` — a hard barrier that made nineteen fast
/// files wait for one slow one. A sliding window must let a finished slot be
/// reused immediately, which is exactly "admission depends only on what is
/// currently in flight, never on batch position".
#[test]
fn admission_depends_only_on_current_occupancy() {
    // Same occupancy → same answer, regardless of how many completed before.
    assert_eq!(admits(1, 1024, 1024, LIMIT_START), admits(1, 1024, 1024, LIMIT_START));
    // A slot freeing up re-admits immediately.
    assert!(!admits(LIMIT_START, 1024, 1024, LIMIT_START));
    assert!(admits(LIMIT_START - 1, 1024, 1024, LIMIT_START));
}

/// Peak resident bytes are bounded by BYTE_BUDGET plus one over-budget file
/// admitted by the empty-window escape hatch. The old worst case was
/// 20 x 20 MB = 400 MB.
#[test]
fn the_window_bounds_resident_bytes_far_below_the_old_400_mb() {
    let peak = BYTE_BUDGET + CHUNK_SIZE_MAX as u64; // escape hatch admits < ceiling alone
    assert!(peak < 20 * 1024 * 1024, "peak resident {} bytes", peak);
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
        worst_case_request_seconds(LIMIT_START, MIN_ESCALATION_SIZE, SEED_BANDWIDTH_BYTES_PER_SEC);
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
