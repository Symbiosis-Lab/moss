//! How big an upload request may be, how many may be in flight, and what to do
//! when one times out.
//!
//! Pure arithmetic over numbers — no I/O, no reqwest, no tokio. `client.rs`
//! owns transport; this owns the sizing decisions transport has to obey. The
//! split exists because these numbers are the whole substance of the
//! 2026-08-03 publish failure and they deserve to be readable and directly
//! testable rather than buried as constants in a 900-line HTTP client.
//!
//! # The constraint these numbers answer to
//!
//! Cloudflare's Proxy Read Timeout is **125 s** on every non-Enterprise plan.
//! When a single HTTP request's body takes longer than that to reach the
//! origin, CF cuts the connection and returns 524 to the client. The origin
//! sees a truncated body. Nothing about the file is wrong; it was simply too
//! big to send in the time allowed, **at the share of bandwidth it was given**.
//!
//! That last clause is the part moss got wrong. The old code routed by file
//! size alone (`>20 MB` → chunked) while running 20 uploads concurrently. Size
//! cannot predict duration when the divisor is a variable.
//!
//! # The measurement
//!
//! A live vault, 2026-08-03: 33.46 MB moved in 242 s, i.e. an aggregate
//! uplink of **~135 KB/s (1.11 Mbps)**. An 8.91 MB mp3 sharing that link 20
//! ways could not finish in 125 s and killed the whole 160-file, 49.6 MB
//! deploy — three times, because retry replayed the identical strategy.
//!
//! # The bound, and which quantity is which
//!
//! The slowest in-flight request competes with every other in-flight request,
//! so the duration to bound is
//!
//! ```text
//! worst_case_request_seconds = concurrency * request_bytes / aggregate_bandwidth
//! ```
//!
//! **not** `BYTE_BUDGET / bandwidth`. The byte budget caps memory; concurrency
//! is what divides the link.
//!
//! Which concurrency, though, is a **target vs. clamp** split, and getting it
//! wrong in either direction has a measured cost (2026-08-27, Shanghai):
//!
//! * **target** — a request is sized for the concurrency *actually in flight*
//!   ([`Throughput::in_flight`]), so it takes [`TARGET_REQUEST_SECONDS`] at the
//!   share of the link it is really getting. Dividing by the ceiling
//!   unconditionally — the model this replaced — sized every request of a
//!   one-file-in-flight deploy 3x too small, pinning a 99.9 MB video to the
//!   256 KiB floor: 262 requests where ~66 would do, each paying a TLS round
//!   trip, a schnorr signature and a server-side `getUsedBytes` walk before a
//!   byte of body counted.
//! * **clamp** — the target alone is unsafe, because admission is not frozen
//!   while a request is in flight: a request sized as if it owns the link,
//!   with two more files admitted after it, takes 3x its planned time —
//!   135 s against the 125 s [`EDGE_BUDGET`], and Cloudflare answers 524. So
//!   every request is clamped at `bytes_per_sec * EDGE_BUDGET / limit`: the
//!   largest request that still completes inside the edge deadline even if
//!   the window fills to its current concurrency ceiling immediately after
//!   this request was sized.
//!
//! The ceiling itself is **adaptive** ([`Throughput::effective_limit`]),
//! because on the Shanghai path above aggregate bandwidth is a *function of*
//! concurrency, not a fixed pie it divides: one stream measured 36,707 B/s
//! and four measured 188,877 B/s aggregate, each individually faster than the
//! lone stream. The limit starts at [`LIMIT_START`] and moves on measured
//! aggregate goodput — up while adding a connection kept paying, down when
//! goodput falls or the transient-failure rate climbs — bounded by
//! [`LIMIT_MIN`]/[`LIMIT_MAX`] so it can never grow into the memory or edge
//! guardrails, which cap different quantities and stay hard.
//!
//! Two different bandwidths appear in this module and conflating them is how
//! the bound gets silently violated, so they are named here once:
//!
//! * **aggregate** — the whole uplink. [`Throughput`] is *defined* to hold
//!   this quantity; [`plan_request_size`] divides it by the in-flight count
//!   (target) and the concurrency ceiling (clamp) as above.
//! * **per-connection share** — what one request actually observes while
//!   others run. This is what [`Throughput::observe`] can measure, and it is
//!   folded in *as if it were the aggregate*. That under-states the link when
//!   the window is shared, which errs toward **smaller** requests, never
//!   larger. Deliberate — a request that is too small costs a round trip, a
//!   request that is too big costs a 524 and a retry that carries no new
//!   information.
//!
//! `a_request_planned_at_any_occupancy_fits_the_edge_budget` in the sibling
//! test file pins the real bound — `limit * plan_request_size(..) / r` against
//! [`EDGE_BUDGET`] — across a sweep of measured rates, occupancies and limits.
//!
//! # Why the sizes below are derived and not constants (2026-08-04)
//!
//! One day after the numbers above were calibrated, the same tester measured
//! **~50 KB/s** — 2.7x below the "floor". A constant calibrated to one
//! observation cannot bound a quantity that varies by orders of magnitude
//! between users and by multiples within a session; every recalibration buys
//! until the next slower user.
//!
//! The sharpest consequence was on the *single-PUT* path, which has no
//! chunking, no `escalate_down` and no resume: a 3.9 MB file just under the
//! old fixed 4 MiB routing threshold needs ~156 s at 25 KB/s against a 150 s
//! [`UPLOAD_REQUEST_TIMEOUT`], so it could never succeed at any retry count.
//!
//! So request size is now *predicted from measured throughput*
//! ([`plan_request_size`]) and the routing threshold is that same number
//! ([`needs_chunking`]) — one number, so there is no second one to keep
//! consistent. [`escalate_down`] stays: it is the reactive half, applied after
//! a request has already failed.
//!
//! # Why the seed is derived from a size and not from a bandwidth (2026-08-04, second pass)
//!
//! The first implementation of the above seeded [`Throughput`] with the
//! measured 135 KB/s and left the plan to work it out. It did:
//! `135 KB/s x 45 s = 6.07 MB`, which clamps to [`CHUNK_SIZE_MAX`] — so the
//! **first request of every publish was the 4 MiB maximum**, and the design's
//! own `INITIAL_CHUNK_SIZE = 1 MiB` ("start small and grow from measurement
//! rather than discovering the link is slow by burning a 150 s timeout") was
//! never actually implemented. Any seed at or above ~93 KB/s saturates the
//! plan, so the seed was doing nothing except hiding that.
//!
//! The seed is therefore derived from [`INITIAL_REQUEST_SIZE`] — the size we
//! want the first request to be — rather than from a bandwidth guess whose
//! only effect was to saturate. The 135 KB/s figure keeps its place above as
//! provenance for [`LIMIT_START`] and the edge-budget arithmetic; it is no
//! longer a starting *value*.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Cloudflare's Proxy Read Timeout on all non-Enterprise plans. Documented, not
/// measured, and not adjustable except on Enterprise (`proxy_read_timeout`).
/// A request whose body takes longer than this gets a 524 and a truncated body
/// at the origin.
pub(crate) const EDGE_BUDGET: Duration = Duration::from_secs(125);

/// The size of the **first** request of a publish, before any measurement
/// exists.
///
/// Small on purpose. A publish that opens at [`CHUNK_SIZE_MAX`] discovers a
/// slow link by burning a 150 s [`UPLOAD_REQUEST_TIMEOUT`]; one that opens at
/// 1 MiB discovers it from a request that *completed*, which is both faster and
/// a usable measurement. 1 MiB needs ~45 s at 23 KB/s per connection — below
/// anything we have measured, so the opening request is expected to land even
/// on the worst link seen in production.
pub(crate) const INITIAL_REQUEST_SIZE: usize = 1024 * 1024;

/// The aggregate uplink a deploy *starts* by assuming — chosen as exactly the
/// rate at which [`plan_request_size`] produces [`INITIAL_REQUEST_SIZE`] for
/// the first, alone-in-the-window request, and for no other reason.
///
/// It is an input to one equation, not a belief about anyone's connection.
/// Seeding a *bandwidth* guess here is what silently disabled the small first
/// request (see the module header): every guess at or above ~98 KB/s saturates
/// the plan at [`CHUNK_SIZE_MAX`]. [`Throughput`] replaces it from the first
/// completed request either way.
///
/// Derived through the **edge clamp** (`EDGE_BUDGET / LIMIT_START`), which
/// is what binds a solo request — tighter than the 45 s target. Rounded
/// **up**: rounding down lands one byte under 1 MiB and the 256 KiB alignment
/// inside `plan_request_size` would then drop the opening request to 768 KiB.
pub const SEED_BANDWIDTH_BYTES_PER_SEC: u64 =
    ((INITIAL_REQUEST_SIZE as u64) * (LIMIT_START as u64)).div_ceil(EDGE_BUDGET.as_secs());

/// How long one upload request *should* take **at the share of the link it is
/// actually getting**, and therefore how many bytes it may carry at the
/// measured link speed ([`plan_request_size`]).
///
/// 45 s is chosen against [`EDGE_BUDGET`] (125 s), not against comfort: it
/// leaves the request room to finish even if bandwidth halves mid-flight
/// (90 s, still inside the budget). It is a duration at the *current* share;
/// the hazard of the share shrinking after sizing — more files admitted while
/// this request is in flight — is the clamp's job, not this number's.
pub(crate) const TARGET_REQUEST_SECONDS: u64 = 45;

/// Ceiling on bytes per request — per PATCH in the chunked protocol, and the
/// largest file that may still go out as a single PUT.
///
/// A ceiling rather than *the* size since 2026-08-04: [`plan_request_size`]
/// picks the actual size from measured throughput and clamps here. Must stay
/// <= seta's `CHUNK_MAX_BYTES` (50 MB, `moss-seta/src/config/limits.ts`).
pub(crate) const CHUNK_SIZE_MAX: usize = 4 * 1024 * 1024;

/// Every request size is a multiple of this, and so is [`MIN_ESCALATION_SIZE`].
///
/// Both GCS and Cloudflare Stream *require* 256 KiB-aligned resumable chunks.
/// moss fronts neither today, but the alignment is free and keeps the door
/// open to an object store behind the upload path.
const SIZE_ALIGNMENT: usize = 256 * 1024;

/// A shared, mutable estimate of the **aggregate** uplink, in bytes per second.
///
/// "Aggregate" is what [`plan_request_size`] reads it as; a *sample* is one
/// request's per-connection share. See the module header for why folding one
/// into the other is deliberate and which way it errs.
///
/// One instance per deploy (owned by `deploy::upload::UploadWindow`), shared by
/// every concurrent upload task — deliberately *not* a global static, which
/// would make the unit tests order-dependent and leak one publish's link
/// conditions into the next.
///
/// Exponentially weighted, alpha = 0.3: recent requests dominate within a few
/// samples (a link that halves is reflected inside ~3 requests) while a single
/// outlier cannot swing the estimate.
pub struct Throughput {
    bytes_per_sec: AtomicU64,
    /// Body-carrying attempts actually sent (chunk PATCHes and single-file
    /// PUTs, including every retry). Control-plane requests — session create,
    /// HEAD resync, complete, symlink PUTs — are not counted: the summary is
    /// about where the deploy's time and bytes went, and they carry neither.
    requests: AtomicU64,
    /// The subset of `requests` the server confirmed. `requests - requests_ok`
    /// is the retry count — attempts that bought nothing.
    requests_ok: AtomicU64,
    /// Bytes the server confirmed, fed by [`Self::observe`]. Excludes resumed
    /// prefixes (never re-sent) and failed attempts, so `bytes_confirmed /
    /// elapsed` is the deploy's achieved rate, not its optimistic one.
    bytes_confirmed: AtomicU64,
    /// Files routed to the chunked protocol vs. sent as one PUT.
    chunked_files: AtomicU64,
    single_put_files: AtomicU64,
    /// Files whose sealed-manifest hash didn't match what was actually on
    /// disk at upload time and got shipped anyway (self-healed) rather than
    /// failing the deploy — see `deploy::upload`'s drift handling. Folded
    /// into the summary line so a client hitting this repeatedly is visible
    /// in the log a human reads, not only in a per-file `log::warn!` that
    /// scrolls by.
    self_healed_files: AtomicU64,
    /// Upload tasks currently admitted to the window — the shared view of
    /// `UploadWindow`'s task count, maintained by it on the same
    /// spawn/harvest lifecycle as its byte accounting. Lives here because the
    /// tasks themselves size their requests by it ([`plan_request_size`]'s
    /// target divisor) and cannot see the window they run inside.
    in_flight: std::sync::atomic::AtomicUsize,
    /// The adaptive concurrency limit, `floor`ed from [`AdaptiveState::limit`]
    /// and cached here so `admits` and `plan_request_size` read it lock-free.
    effective_limit: std::sync::atomic::AtomicUsize,
    /// Controller state behind the limit. A `Mutex`, not atomics, because a
    /// window closes as one transaction; contention is one lock per completed
    /// request among at most [`LIMIT_MAX`] tasks.
    adaptive: std::sync::Mutex<AdaptiveState>,
    /// When this deploy began — supplies `at` to [`Self::fold_completion`] so
    /// the pure controller sees wall time without owning a clock.
    epoch: std::time::Instant,
}

/// Goodput-window state for the adaptive limit. See
/// [`Throughput::fold_completion`] for the control loop it carries.
struct AdaptiveState {
    /// The limit, fractional so the `sqrt` growth shape accumulates; the
    /// integer the rest of the module obeys is `effective_limit`.
    limit: f64,
    /// The open window's accumulator, replaced wholesale at close — so a
    /// field added to it can never be missed by the reset.
    window: WindowAccum,
    /// `requests - requests_ok` when the previous window closed; the delta at
    /// this window's close is its failure count. A close-time baseline (not an
    /// open-time snapshot) so failures landing between windows — a retry storm
    /// with no completion in flight — still count against the next verdict.
    failures_baseline: u64,
    /// The previous window's aggregate goodput, the baseline "improved" and
    /// "fell" are judged against.
    last_goodput: Option<f64>,
}

/// One goodput window's running totals. `Default` is the empty window.
#[derive(Default)]
struct WindowAccum {
    /// When the window's first completion arrived. `None` = no window open
    /// yet.
    opened_at: Option<Duration>,
    bytes: u64,
    completions: u32,
    /// Highest in-flight count seen during the window — growth demands the
    /// window actually saturated the limit, or the probe learned nothing
    /// about one more connection.
    peak_in_flight: usize,
}

impl Throughput {
    /// Seeded at [`SEED_BANDWIDTH_BYTES_PER_SEC`] — the last link we measured.
    pub fn new() -> Self {
        Self {
            bytes_per_sec: AtomicU64::new(SEED_BANDWIDTH_BYTES_PER_SEC),
            requests: AtomicU64::new(0),
            requests_ok: AtomicU64::new(0),
            bytes_confirmed: AtomicU64::new(0),
            chunked_files: AtomicU64::new(0),
            single_put_files: AtomicU64::new(0),
            self_healed_files: AtomicU64::new(0),
            in_flight: std::sync::atomic::AtomicUsize::new(0),
            effective_limit: std::sync::atomic::AtomicUsize::new(LIMIT_START),
            adaptive: std::sync::Mutex::new(AdaptiveState {
                limit: LIMIT_START as f64,
                window: WindowAccum::default(),
                failures_baseline: 0,
                last_goodput: None,
            }),
            epoch: std::time::Instant::now(),
        }
    }

    /// The concurrency ceiling the rest of the module obeys right now.
    pub fn effective_limit(&self) -> usize {
        self.effective_limit.load(Ordering::Relaxed)
    }

    /// Fold one confirmed request into the adaptive limit's goodput window.
    ///
    /// # The control loop
    ///
    /// Netflix's `concurrency-limits` infers capacity from latency because a
    /// server cannot observe its clients' throughput; moss can measure the
    /// quantity it actually cares about. Completions accumulate into a window;
    /// when the window holds at least `effective_limit` of them and has run at
    /// least [`ADAPT_WINDOW_MIN`], it closes and its aggregate goodput
    /// (`bytes / duration`) is compared against the previous window's:
    ///
    /// * transient-failure rate above [`ADAPT_FAILURE_LIMIT`], or goodput
    ///   fallen below [`ADAPT_DECAY`] of the baseline → the limit steps
    ///   **down** by `sqrt(limit)`, floored at [`LIMIT_MIN`].
    /// * goodput improved beyond [`ADAPT_GROWTH`] **and** the window actually
    ///   saturated the limit → the limit steps **up** by `sqrt(limit)`
    ///   (Gradient2's term: relatively fast at 3, cautious near
    ///   [`LIMIT_MAX`]), capped there.
    /// * otherwise → hold. On a clean uplink this is where the loop lives:
    ///   the second stream adds nothing, goodput plateaus immediately, and
    ///   the limit settles low. On the GFW path every added connection keeps
    ///   paying and the limit climbs. Same loop, opposite settings.
    ///
    /// The saturation requirement is load-bearing beyond probe hygiene: a
    /// deploy whose one large file holds the window alone (`in_flight == 1`
    /// for an hour) must not have its improving estimate read as "concurrency
    /// paid", because a grown limit shrinks [`plan_request_size`]'s edge clamp
    /// and would walk its chunks back toward the floor.
    ///
    /// Pure over its inputs — `at` is time since the deploy's epoch, supplied
    /// by [`Self::observe`] in production and directly by tests.
    pub(crate) fn fold_completion(&self, bytes: u64, at: Duration) {
        let effective = self.effective_limit();
        let mut w = self.adaptive.lock().unwrap();
        let failures_now = self
            .requests
            .load(Ordering::Relaxed)
            .saturating_sub(self.requests_ok.load(Ordering::Relaxed));
        let opened_at = *w.window.opened_at.get_or_insert(at);
        w.window.bytes += bytes;
        w.window.completions += 1;
        w.window.peak_in_flight = w.window.peak_in_flight.max(self.in_flight());
        let elapsed = at.saturating_sub(opened_at);
        if (w.window.completions as usize) < effective || elapsed < ADAPT_WINDOW_MIN {
            return;
        }

        let goodput = w.window.bytes as f64 / elapsed.as_secs_f64();
        let failures = failures_now.saturating_sub(w.failures_baseline);
        let failure_rate = failures as f64 / (w.window.completions as u64 + failures) as f64;
        let step = w.limit.sqrt();
        if failure_rate > ADAPT_FAILURE_LIMIT
            || w.last_goodput.is_some_and(|last| goodput < last * ADAPT_DECAY)
        {
            w.limit = (w.limit - step).max(LIMIT_MIN as f64);
        } else if w.window.peak_in_flight >= effective
            && w.last_goodput.is_some_and(|last| goodput > last * ADAPT_GROWTH)
        {
            w.limit = (w.limit + step).min(LIMIT_MAX as f64);
        }
        self.effective_limit.store(w.limit as usize, Ordering::Relaxed);
        w.last_goodput = Some(goodput);
        w.failures_baseline = failures_now;
        w.window = WindowAccum::default();
    }

    /// `UploadWindow` calls these on the same lifecycle as its byte
    /// accounting: enter at spawn, exit at harvest.
    pub fn enter_flight(&self) {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn exit_flight(&self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// [`plan_request_size`] over this deploy's current estimate, occupancy
    /// and limit.
    pub fn plan_request_size(&self) -> usize {
        plan_request_size(self.bytes_per_sec(), self.in_flight(), self.effective_limit())
    }

    /// [`needs_chunking`] over this deploy's current estimate, occupancy and
    /// limit.
    pub fn needs_chunking(&self, size: u64) -> bool {
        needs_chunking(size, self.bytes_per_sec(), self.in_flight(), self.effective_limit())
    }

    /// A body-carrying attempt is about to be sent. Call once per attempt,
    /// retries included.
    pub(crate) fn note_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    /// The server confirmed the attempt.
    pub(crate) fn note_request_ok(&self) {
        self.requests_ok.fetch_add(1, Ordering::Relaxed);
    }

    /// One file entered the chunked protocol / went out as a single PUT.
    pub(crate) fn note_chunked_file(&self) {
        self.chunked_files.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_single_put_file(&self) {
        self.single_put_files.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one self-heal (a file or symlink shipped with different bytes
    /// than the sealed manifest expected, after `deploy::upload`'s stability
    /// and corruption checks cleared it). Returns the new running total, so
    /// the caller can enforce a per-deploy cap without a second shared
    /// counter — see `deploy::upload::self_heal_cap`.
    pub(crate) fn note_self_heal(&self) -> u64 {
        self.self_healed_files.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The per-deploy roll-up behind the `upload summary` INFO line.
    ///
    /// Exists because the 2026-08-27 Shanghai baseline (28,190 B/s over 262
    /// requests) had to be reconstructed by grepping a rotated DEBUG log and
    /// differencing timestamps by hand. Everything a support log needs to say
    /// about upload performance is in this one greppable line; `elapsed` is the
    /// upload phase's wall time, supplied by the `UploadWindow` that owns it.
    ///
    /// `self-healed` is always printed, even at 0 — the whole point is to be
    /// greppable across a log history, and a metric that only appears when
    /// nonzero cannot be told apart from a build too old to have it.
    pub fn deploy_summary(&self, elapsed: Duration) -> String {
        let bytes = self.bytes_confirmed.load(Ordering::Relaxed);
        let requests = self.requests.load(Ordering::Relaxed);
        let ok = self.requests_ok.load(Ordering::Relaxed);
        let secs = elapsed.as_secs_f64();
        // max(1ms) so an empty or instant deploy divides by something.
        let rate = (bytes as f64 / secs.max(0.001)) as u64;
        format!(
            "[deploy] upload summary: {} bytes in {:.0}s = {} B/s achieved; \
             {} requests, {} retried; {} chunked + {} single-PUT files; \
             {} self-healed drift(s); final estimate {} B/s, limit {}",
            bytes,
            secs,
            rate,
            requests,
            requests.saturating_sub(ok),
            self.chunked_files.load(Ordering::Relaxed),
            self.single_put_files.load(Ordering::Relaxed),
            self.self_healed_files.load(Ordering::Relaxed),
            self.bytes_per_sec(),
            self.effective_limit(),
        )
    }

    /// Fold one completed request into the estimate.
    ///
    /// # Overhead is a function of elapsed time, not of size
    ///
    /// This used to ignore any request under [`MIN_ESCALATION_SIZE`], which
    /// made a whole class of site unmeasurable: 200 files of a few hundred KB
    /// each produce no qualifying sample, so the estimate never moved off the
    /// seed and one 4 MB image was routed by a guess. That is §9.1's failure
    /// with the fix applied and the measurement still missing.
    ///
    /// A 200 KB request that took 8 s is a perfectly good 25 KB/s reading. What
    /// makes a sample worthless is not being small, it is being *short*: a
    /// request costs a TLS round trip, a schnorr signature and a `getUsedBytes`
    /// walk on the server before a byte of body is credited, and that cost is
    /// the same for 4 KB and 4 MB. So the gate is on elapsed only, and
    /// [`REQUEST_OVERHEAD`] is subtracted from the elapsed time rather than
    /// left to bias the rate downward — which matters much more now that small
    /// requests count, since overhead is a larger fraction of their duration.
    pub fn observe(&self, bytes: u64, elapsed: Duration) {
        // Every observed request is confirmed bytes, whether or not it also
        // qualifies as a bandwidth sample below — and every one drives the
        // adaptive limit's goodput window.
        self.bytes_confirmed.fetch_add(bytes, Ordering::Relaxed);
        self.fold_completion(bytes, self.epoch.elapsed());
        // Below 2x the overhead the transfer term does not dominate, and the
        // sample is really a measurement of the round trip: a 4 KB PUT in 40 ms
        // is 100 KB/s of handshake, and believing it would shrink every
        // subsequent request for no reason.
        if elapsed < REQUEST_OVERHEAD * 2 {
            return;
        }
        let transfer = elapsed - REQUEST_OVERHEAD;
        let sample = (bytes as f64 / transfer.as_secs_f64()).max(1.0);
        let previous = self.bytes_per_sec.load(Ordering::Relaxed) as f64;
        const ALPHA: f64 = 0.3;
        let next = ALPHA * sample + (1.0 - ALPHA) * previous;
        self.bytes_per_sec.store(next.max(1.0) as u64, Ordering::Relaxed);
    }

    pub fn bytes_per_sec(&self) -> u64 {
        self.bytes_per_sec.load(Ordering::Relaxed)
    }
}

/// What one request costs before any of its body is credited: a TCP+TLS round
/// trip, a schnorr signature, and the server's own per-request work. Subtracted
/// from a sample's elapsed time by [`Throughput::observe`] so the rate it
/// derives is the transfer rate rather than the transfer rate diluted by a
/// fixed cost.
///
/// Not measured — an order-of-magnitude figure for a round trip to Cloudflare
/// plus origin work. It only has to be small against the samples it corrects
/// and large against nothing; getting it wrong low under-states the link, which
/// is the safe direction (see the module header).
const REQUEST_OVERHEAD: Duration = Duration::from_millis(250);

/// How many bytes one request may carry when the **aggregate** link is running
/// at `bytes_per_sec` and `in_flight` requests currently share it.
///
/// Target plus safety clamp — see the module header for the measured cost of
/// collapsing them into one number:
///
/// * target: `bytes_per_sec * TARGET_REQUEST_SECONDS / in_flight` — what this
///   request should be at the share of the link it is actually getting.
/// * clamp: `bytes_per_sec * EDGE_BUDGET / limit` — the largest request that
///   still completes inside the edge deadline even if the window fills to its
///   current ceiling immediately after this request was sized. The limit can
///   still rise *while* a request is in flight, but only by one windowed
///   `sqrt` step, and only because measured goodput said concurrency is
///   growing the pie rather than dividing it — and [`escalate_down`] remains
///   the reactive net for a request the edge does cut.
///
/// `in_flight` includes the request being planned, so it is at least 1 from
/// any admitted task; 0 (no window at all, as in tests driving the chunk loop
/// directly) plans as 1.
///
/// Pure — the atomics are [`Throughput`]'s business — so the arithmetic is
/// directly testable, which is where the correctness of this module actually
/// lives.
pub(crate) fn plan_request_size(bytes_per_sec: u64, in_flight: usize, limit: usize) -> usize {
    let target =
        bytes_per_sec.saturating_mul(TARGET_REQUEST_SECONDS) / in_flight.max(1) as u64;
    let clamp = bytes_per_sec.saturating_mul(EDGE_BUDGET.as_secs()) / limit.max(1) as u64;
    let predicted = usize::try_from(target.min(clamp)).unwrap_or(usize::MAX);
    let aligned = (predicted / SIZE_ALIGNMENT) * SIZE_ALIGNMENT;
    aligned.clamp(MIN_ESCALATION_SIZE, CHUNK_SIZE_MAX)
}

/// Where the adaptive concurrency limit starts.
///
/// The ceiling was 20, copied from Vercel's model — whose stated justification
/// is HTTP/2 multiplexing and *server* capacity, and which never mentions the
/// client's uplink — then a constant 3 (wrangler's number, sound at the
/// 135 KB/s aggregate measured on 2026-08-03). A constant assumes aggregate
/// bandwidth is fixed and concurrency merely divides it, which holds on a
/// clean link and inverts on a lossy high-RTT one: measured from Shanghai
/// 2026-08-27, one stream to Cloudflare ran 36,707 B/s and four ran
/// 188,877 B/s aggregate — each of the four individually faster than the lone
/// stream. So 3 is now where the limit *begins*; [`Throughput`] moves it on
/// measured goodput between [`LIMIT_MIN`] and [`LIMIT_MAX`].
pub const LIMIT_START: usize = 3;

/// Floor for the adaptive limit: below one connection there is no upload.
pub(crate) const LIMIT_MIN: usize = 1;

/// Minimum wall time a goodput window must span before it may close. Short
/// windows measure burstiness, not the link — and a deploy that finishes
/// before its first window closes (a healthy small publish) correctly never
/// adapts at all.
pub(crate) const ADAPT_WINDOW_MIN: Duration = Duration::from_secs(10);

/// A window must beat the previous one by this factor before the limit grows.
/// Below it, the improvement is EWMA warm-up or noise, and growing on noise
/// oscillates.
const ADAPT_GROWTH: f64 = 1.10;

/// A window falling below this factor of the previous one shrinks the limit.
/// Wider than the growth band on purpose: goodput on a lossy path is noisy
/// downward, and reacting to every dip would saw-tooth.
const ADAPT_DECAY: f64 = 0.75;

/// Transient-failure rate (failed attempts over attempts) above which a
/// window shrinks the limit regardless of goodput: failures are the earlier,
/// cheaper signal that the path is over-driven.
const ADAPT_FAILURE_LIMIT: f64 = 0.2;

/// Hard ceiling for the adaptive limit. Not politeness to the server — it is
/// what keeps the loop's reachable states inside territory the other
/// guardrails were audited for. [`BYTE_BUDGET`] (memory) and [`EDGE_BUDGET`]
/// (Cloudflare's proxy deadline) stay hard, independent caps that the limit
/// must never be able to grow into: an adaptive limit allowed to run until one
/// of them bites has rediscovered the okagaki incident of 2026-08-03.
pub(crate) const LIMIT_MAX: usize = 12;

/// Cap on bytes buffered across all in-flight uploads.
///
/// A separate knob from [`LIMIT_MAX`] because they bound different things:
/// this bounds memory, that bounds request duration. The old code had neither —
/// 20 concurrent x up to 20 MB buffered is up to 400 MB resident on a machine
/// that is also running a webview and a build. NORTH-STAR calls for auditing
/// the memory budget before parallelizing; this is that audit's answer.
pub const BYTE_BUDGET: u64 = 8 * 1024 * 1024;

/// Per-request ceiling for a single file upload, overriding the client's
/// default (120 s, sized for small control-plane JSON calls).
///
/// Deliberately **above** [`EDGE_BUDGET`], so that when Cloudflare is in the
/// path its 524 is the authority and this timeout only catches the no-CF case.
/// A tempting 120 s would have failed the okagaki deploy's own *successful*
/// 121 s PUT of `michael-performing-1970s.png`. The old 300 s was the opposite
/// error: waiting five minutes for a response CF killed at 125 s.
pub(crate) const UPLOAD_REQUEST_TIMEOUT: Duration = Duration::from_secs(150);

/// Whether `size` bytes should go over the chunked protocol rather than one PUT.
///
/// Exactly "does it fit in one request at the speed we are actually getting" —
/// the same [`plan_request_size`] the chunk loop sizes its PATCHes with, so
/// there is a single number and no gap between "too big for one PUT" and
/// "small enough to send". A file at 3.9 MB is a single PUT on a healthy link
/// and a chunked, resumable, escalatable upload at 25 KB/s, which is the case
/// that could not complete at all before 2026-08-04.
pub(crate) fn needs_chunking(size: u64, bytes_per_sec: u64, in_flight: usize, limit: usize) -> bool {
    size > plan_request_size(bytes_per_sec, in_flight, limit) as u64
}

/// Deadline for a request whose body size is set by how much *work* the site
/// represents rather than by any per-request policy — the `/sync` and
/// `/commit` manifest POSTs.
///
/// Invariant I1 forbids a
/// deadline that is a function of total work. A flat 150 s on a body that
/// grows with file count is exactly that: a 10k-file site's ~1 MB manifest
/// needs ~200 s at 5 KB/s, fails four times, and that site can never publish
/// at any bandwidth.
///
/// So the deadline is `floor + body / SLOW_LINK`, where `SLOW_LINK` is
/// deliberately far below any link we have ever measured (5 KB/s vs. the
/// slowest observed 50 KB/s). This is **not** a bandwidth prediction and must
/// not be tuned as one: the size term exists so the deadline can never be
/// smaller than the transfer, and the 150 s floor is what still bounds a
/// *stalled* small manifest. Erring slow costs a stuck publish an extra
/// minute; erring fast costs a large site every publish it will ever attempt.
pub(crate) fn manifest_request_timeout(body_bytes: u64) -> Duration {
    const SLOW_LINK_BYTES_PER_SEC: u64 = 5 * 1024;
    UPLOAD_REQUEST_TIMEOUT + Duration::from_secs(body_bytes / SLOW_LINK_BYTES_PER_SEC)
}

/// May a task uploading `size` bytes join a window that currently holds
/// `in_flight_count` tasks and `in_flight_bytes` bytes, under the adaptive
/// `limit` ([`Throughput::effective_limit`])?
///
/// Admission is by bytes AND by count. The byte cap is the one that maps to
/// the real constraint — and it is a **hard** cap the adaptive limit cannot
/// override, so a grown limit spends connections, never memory. The count cap
/// is what the goodput loop tunes.
///
/// The `in_flight_count == 0` escape hatch is required for progress: a single
/// file larger than [`BYTE_BUDGET`] would otherwise never be admitted and the
/// deploy would hang. It cannot be reached for a file above
/// [`CHUNK_SIZE_MAX`] (those upload chunk-by-chunk), so in practice it only
/// ever admits one sub-4 MB file alone.
pub const fn admits(
    in_flight_count: usize,
    in_flight_bytes: u64,
    size: u64,
    limit: usize,
) -> bool {
    if in_flight_count == 0 {
        return true;
    }
    in_flight_count < limit && in_flight_bytes + size <= BYTE_BUDGET
}

/// The next request size to try after a request of `previous` bytes timed out
/// or was cut by the edge.
///
/// Halve, floored at [`MIN_ESCALATION_SIZE`]. Replaying an identical request
/// after a 524 is what turned one slow file into three identical failures and
/// an exhausted 600 s budget: 524 is `>= 500` so `is_transient` retries it, and
/// every retry took exactly as long to fail as the first. Retrying *smaller* is
/// the only response that carries new information.
///
/// Marking 524 non-transient instead would merely convert the failure into a
/// faster whole-deploy abort, which is worse.
pub(crate) const fn escalate_down(previous: usize) -> usize {
    let halved = previous / 2;
    if halved < MIN_ESCALATION_SIZE {
        MIN_ESCALATION_SIZE
    } else {
        halved
    }
}

/// Floor for [`escalate_down`] and for [`plan_request_size`]. Below this the
/// per-request overhead (a fresh
/// schnorr signature, a TLS round trip, a `getUsedBytes` walk on the server)
/// dominates the payload, and a link too slow for 256 KB in 150 s is not going
/// to complete a publish at any chunk size.
pub(crate) const MIN_ESCALATION_SIZE: usize = 256 * 1024;

/// Worst-case seconds for one request of `chunk` bytes with `concurrent`
/// requests sharing an **aggregate** `bandwidth` bytes/sec. The bound the
/// constants above are chosen to satisfy; exposed so the test can assert it
/// rather than restate it.
///
/// `bandwidth` is the same quantity [`plan_request_size`] takes, which is what
/// lets the test compose the two: `worst_case_request_seconds(limit,
/// plan_request_size(r, n, limit), r) <= EDGE_BUDGET` for every rate `r`,
/// occupancy `n` and limit at which the plan is not pinned to its floor — the
/// clamp's bound. The tighter `<= TARGET_REQUEST_SECONDS` holds where the
/// target binds, i.e. with the window full.
#[cfg_attr(not(test), allow(dead_code))] // the bound, asserted by tests; not evaluated at runtime
pub(crate) fn worst_case_request_seconds(concurrent: usize, chunk: usize, bandwidth: u64) -> f64 {
    (concurrent as f64) * (chunk as f64) / (bandwidth as f64)
}

#[cfg(test)]
#[path = "upload_policy_tests.rs"]
mod tests;
