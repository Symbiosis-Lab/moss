//! How big an upload request may be, and what to do when one times out.
//!
//! Pure arithmetic over numbers: no I/O, no reqwest, no tokio. `client.rs`
//! owns transport; this owns the sizing decisions transport has to obey.
//!
//! # The constraint these numbers answer to
//!
//! Cloudflare's Proxy Read Timeout is **125 s** on every non-Enterprise plan.
//! When a single HTTP request's body takes longer than that to reach the
//! origin, CF cuts the connection and returns 524 to the client. Nothing about
//! the file is wrong; it was too big to send in the time allowed **at the
//! share of bandwidth it was given**.
//!
//! A deploy runs up to the server's `concurrency_hint` requests at once, and
//! the slowest competes with all the others, so the duration to bound is
//!
//! ```text
//! worst_case_request_seconds = concurrency * request_bytes / aggregate_bandwidth
//! ```
//!
//! [`plan_request_size`] sizes a request for [`TARGET_REQUEST_SECONDS`] on the
//! whole link, clamped so that `concurrency` of them still finish inside
//! [`EDGE_BUDGET`]. The measured rate is one request's share while others run,
//! folded in as if it were the whole link; that errs toward smaller requests,
//! which cost a round trip, never toward a 524.
//!
//! Sizes are predicted from measured throughput rather than fixed, because a
//! constant calibrated to one link cannot bound a quantity that varies by
//! orders of magnitude between users. [`escalate_down`] is the reactive half,
//! applied after a request has already failed. The seed is derived from
//! [`INITIAL_REQUEST_SIZE`], the size the first request should be, since any
//! bandwidth guess above ~100 KB/s saturates the plan at [`CHUNK_SIZE_MAX`].

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

/// How long one upload request *should* take on the whole link, and therefore
/// how many bytes it may carry at the measured speed ([`plan_request_size`]).
///
/// 45 s leaves a request room to finish even if bandwidth halves mid-flight
/// (90 s, still inside [`EDGE_BUDGET`]).
pub(crate) const TARGET_REQUEST_SECONDS: u64 = 45;

/// Ceiling on bytes per PATCH in the chunked protocol: [`plan_request_size`]
/// picks the actual size from measured throughput and clamps here. Must stay
/// <= the server's 50 MB chunk cap.
pub(crate) const CHUNK_SIZE_MAX: usize = 4 * 1024 * 1024;

/// Every request size is a multiple of this, and so is [`MIN_ESCALATION_SIZE`].
///
/// Both GCS and Cloudflare Stream *require* 256 KiB-aligned resumable chunks.
/// moss fronts neither today, but the alignment is free and keeps the door
/// open to an object store behind the upload path.
const SIZE_ALIGNMENT: usize = 256 * 1024;

/// A shared, mutable estimate of the **aggregate** uplink, in bytes per second,
/// and the deploy's upload counts.
///
/// One instance per deploy, shared by every concurrent send, and deliberately
/// not a global static, which would leak one publish's link conditions into
/// the next.
///
/// Exponentially weighted, alpha = 0.3: recent requests dominate within a few
/// samples while a single outlier cannot swing the estimate.
pub struct Throughput {
    bytes_per_sec: AtomicU64,
    /// How many requests the deploy runs at once; divides the edge budget.
    concurrency: usize,
    /// Body-carrying attempts actually sent, including every retry.
    requests: AtomicU64,
    /// The subset of `requests` the server confirmed; the rest are retries.
    requests_ok: AtomicU64,
    /// Bytes the server confirmed, fed by [`Self::observe`]. Excludes resumed
    /// prefixes and failed attempts, so `bytes_confirmed / elapsed` is the
    /// deploy's achieved rate.
    bytes_confirmed: AtomicU64,
    chunked_files: AtomicU64,
    /// Files whose sealed hash no longer matched the disk and were shipped as
    /// they are now rather than failing the deploy (`deploy::upload`).
    self_healed_files: AtomicU64,
}

impl Throughput {
    pub fn new(concurrency: usize) -> Self {
        let concurrency = concurrency.max(1);
        Self {
            bytes_per_sec: AtomicU64::new(seed_bandwidth(concurrency)),
            concurrency,
            requests: AtomicU64::new(0),
            requests_ok: AtomicU64::new(0),
            bytes_confirmed: AtomicU64::new(0),
            chunked_files: AtomicU64::new(0),
            self_healed_files: AtomicU64::new(0),
        }
    }

    pub fn plan_request_size(&self) -> usize {
        plan_request_size(self.bytes_per_sec(), self.concurrency)
    }

    pub(crate) fn note_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_request_ok(&self) {
        self.requests_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_chunked_file(&self) {
        self.chunked_files.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts one self-healed file and returns the count so far.
    pub(crate) fn note_self_heal(&self) -> u64 {
        self.self_healed_files.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The counts the deploy profile records for the upload.
    pub(crate) fn profile_counts(&self, elapsed: Duration) -> [(&'static str, u64); 6] {
        let bytes = self.bytes_confirmed.load(Ordering::Relaxed);
        let requests = self.requests.load(Ordering::Relaxed);
        [
            ("bytes_uploaded", bytes), ("body_requests", requests),
            ("retries", requests.saturating_sub(self.requests_ok.load(Ordering::Relaxed))),
            ("chunked_files", self.chunked_files.load(Ordering::Relaxed)),
            ("final_concurrency", self.concurrency as u64),
            ("achieved_bytes_per_s", (bytes as f64 / elapsed.as_secs_f64().max(0.001)) as u64),
        ]
    }

    /// One log line a person reads after a deploy: where its time and bytes went.
    pub fn deploy_summary(&self, elapsed: Duration) -> String {
        let bytes = self.bytes_confirmed.load(Ordering::Relaxed);
        let requests = self.requests.load(Ordering::Relaxed);
        let ok = self.requests_ok.load(Ordering::Relaxed);
        let secs = elapsed.as_secs_f64();
        // max(1ms) so an empty or instant deploy divides by something.
        let rate = (bytes as f64 / secs.max(0.001)) as u64;
        format!(
            "[deploy] upload summary: {} bytes in {:.0}s = {} B/s achieved; \
             {} requests, {} retried; {} chunked files; \
             {} self-healed drift(s); final estimate {} B/s, concurrency {}",
            bytes,
            secs,
            rate,
            requests,
            requests.saturating_sub(ok),
            self.chunked_files.load(Ordering::Relaxed),
            self.self_healed_files.load(Ordering::Relaxed),
            self.bytes_per_sec(),
            self.concurrency,
        )
    }

    /// Fold one completed request into the estimate.
    ///
    /// A small request is a good sample; a *short* one is not. A request costs
    /// a TLS round trip, a schnorr signature and server work before a byte of
    /// body counts, the same for 4 KB and 4 MB, so the gate is on elapsed
    /// time only and [`REQUEST_OVERHEAD`] is subtracted from it.
    pub fn observe(&self, bytes: u64, elapsed: Duration) {
        // Every observed request is confirmed bytes, whether or not it also
        // qualifies as a bandwidth sample below.
        self.bytes_confirmed.fetch_add(bytes, Ordering::Relaxed);
        // Below 2x the overhead the transfer term does not dominate, and the
        // sample is really a measurement of the round trip: a 4 KB request in 40 ms
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

/// How many bytes one request may carry when the **aggregate** link runs at
/// `bytes_per_sec` and `concurrency` requests may share it.
///
/// The smaller of the target, `bytes_per_sec * TARGET_REQUEST_SECONDS`, and the
/// clamp, `bytes_per_sec * EDGE_BUDGET / concurrency`: the largest request
/// that still completes inside the edge deadline while every other slot is
/// busy.
pub(crate) fn plan_request_size(bytes_per_sec: u64, concurrency: usize) -> usize {
    let target = bytes_per_sec.saturating_mul(TARGET_REQUEST_SECONDS);
    let clamp = bytes_per_sec.saturating_mul(EDGE_BUDGET.as_secs()) / concurrency.max(1) as u64;
    let predicted = usize::try_from(target.min(clamp)).unwrap_or(usize::MAX);
    let aligned = (predicted / SIZE_ALIGNMENT) * SIZE_ALIGNMENT;
    aligned.clamp(MIN_ESCALATION_SIZE, CHUNK_SIZE_MAX)
}

/// The aggregate uplink a deploy *starts* by assuming: the lowest rate at which
/// [`plan_request_size`] gives [`INITIAL_REQUEST_SIZE`] at `concurrency`. An
/// input to one equation, not a belief about anyone's connection; the first
/// completed request replaces it.
pub(crate) fn seed_bandwidth(concurrency: usize) -> u64 {
    let size = INITIAL_REQUEST_SIZE as u64;
    size.div_ceil(TARGET_REQUEST_SECONDS).max((size * concurrency.max(1) as u64).div_ceil(EDGE_BUDGET.as_secs()))
}

/// Per-request ceiling for a single file upload, overriding the client's
/// default (120 s, sized for small control-plane JSON calls).
///
/// Deliberately **above** [`EDGE_BUDGET`], so that when Cloudflare is in the
/// path its 524 is the authority and this timeout only catches the no-CF case.
/// Not far above it either: waiting longer only waits on a response the edge
/// has already cut.
pub(crate) const UPLOAD_REQUEST_TIMEOUT: Duration = Duration::from_secs(150);

/// Deadline for a request whose body size is set by how much *work* the site
/// represents rather than by any per-request policy — the deploy open, whose
/// body is the manifest.
///
/// A deadline must never be a function of total work. A flat 150 s on a body that
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

/// The next request size to try after a request of `previous` bytes timed out
/// or was cut by the edge.
///
/// Halve, floored at [`MIN_ESCALATION_SIZE`]. Replaying an identical request
/// after a 524 is what turned one slow file into three identical failures and
/// an exhausted 600 s budget: 524 is `>= 500` so `is_transient` retries it, and
/// every retry took exactly as long to fail as the first. Retrying *smaller* is
/// the only response that carries new information.
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
/// lets the test compose the two: `worst_case_request_seconds(c,
/// plan_request_size(r, c), r) <= EDGE_BUDGET` for every rate `r` and
/// concurrency `c` at which the plan is not pinned to its floor.
#[cfg_attr(not(test), allow(dead_code))] // the bound, asserted by tests; not evaluated at runtime
pub(crate) fn worst_case_request_seconds(concurrent: usize, chunk: usize, bandwidth: u64) -> f64 {
    (concurrent as f64) * (chunk as f64) / (bandwidth as f64)
}

#[cfg(test)]
#[path = "upload_policy_tests.rs"]
mod tests;
