//! What kind of "no" was that?
//!
//! Pure classification of a [`SetaError`] — no I/O, no reqwest calls, no state.
//! `client.rs` owns transport and the retry loop; this owns the single question
//! that loop has to answer before it decides what to do next.
//!
//! # Why three classes and not two
//!
//! `SetaError::is_transient` answered a boolean: re-send, or give up. That
//! collapses two genuinely different situations into one.
//!
//! * A 502 from an origin that is restarting is a *fault*. The right response
//!   is a short backoff and one of a small number of attempts, because if it
//!   keeps happening the deploy is not going to succeed.
//! * A 429, or a 503 from a server at capacity, is the peer saying **"not
//!   now"**. Nothing is broken. Spending a retry slot on it — and then
//!   abandoning the file when the slots run out — punishes the client for the
//!   server's own admission control. `wrangler` (Cloudflare's own Pages
//!   deployer) treats these as their own case for exactly this reason.
//!
//! # Why 524 and timeouts are NOT backpressure
//!
//! Tempting, since both mean "too slow", and §8.6 of the design floated it. But
//! backpressure's remedy is *wait longer and send the same thing again*, and
//! that is precisely the move the okagaki failure indicts: a 524 means the body
//! outran Cloudflare's 125 s proxy read timeout, and replaying it identically
//! "spent okagaki's whole 600 s budget on three failures that each took exactly
//! as long as the first" (`upload_policy::escalate_down`). The client already
//! has a strictly better answer — send *less* — and it lives past the end of the
//! retry loop, so forgiving these here would only delay the escalation by three
//! more full-size replays.
//!
//! [`Backpressure`](FailureClass::Backpressure) is therefore forgiven a bounded
//! number of times per file — it waits longer and spends neither a retry slot
//! nor a failed-attempt slot. The bound lives in `client.rs` with the budget it
//! belongs to; this module only says which class a failure is in.

use super::client::SetaError;

/// How the retry loop should treat one failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureClass {
    /// The server's final answer, or a local defect. Re-sending an identical
    /// request cannot change the outcome — fail now and surface the reason.
    Fatal,
    /// A fault on the server or the wire that a later request can plausibly
    /// survive. Short backoff, bounded attempts. Includes "this request was too
    /// slow" (524, timeout), whose real remedy — a smaller request — is the
    /// chunk loop's business, not the retry loop's.
    Transient,
    /// "Not now." The peer is shedding load. Longer backoff, and forgiven (up to
    /// a cap) rather than counted against the file's retry allowance.
    Backpressure,
}

/// Which class is this failure in?
///
/// Statuses are grouped by *what the client should do*, not by RFC family:
///
/// | Signal | Class | Why |
/// |---|---|---|
/// | 429 | Backpressure | An explicit "slow down"; the request itself is fine. |
/// | 503 | Backpressure | Capacity, not correctness — the origin is asking for time. |
/// | 524 | Transient | Cloudflare cut a body that outran the 125 s proxy read timeout. Retryable, but the answer is a smaller request (`upload_policy::escalate_down`), so it must not buy extra full-size replays — see the module header. |
/// | other 5xx | Transient | A genuine origin fault; retry, but count it. |
/// | timeout | Transient | Indistinguishable from a 524 at the socket level, and treated the same for the same reason. |
/// | connect / request | Transient | The connection itself failed; a fault worth counting. |
/// | 4xx (except 429) | Fatal | The server's final answer, 413 quota rejections included. |
/// | `Parse` / `Io` / `Identity` | Fatal | A local defect; the network cannot fix it. |
/// | `NotAllowlisted` | Fatal | An actionable invite-gate message the user must see immediately. |
/// | `Challenge` | Fatal | Unsolvable by a native client **on every status**, including the 503 the row above calls Backpressure. Retrying only deepens the edge's reputation signal against the user (the 2026-07-21 Bot Fight Mode incident: ~21 futile manual retries). |
pub(super) fn classify(error: &SetaError) -> FailureClass {
    match error {
        // Spelled out first, and deliberately ahead of any status test: a
        // challenge can arrive as a 503 or a 429, the two statuses the `Api`
        // arm below would otherwise wave through as Backpressure.
        SetaError::Challenge { .. } => FailureClass::Fatal,
        SetaError::Api { status, .. } => match *status {
            429 | 503 => FailureClass::Backpressure,
            s if s >= 500 => FailureClass::Transient,
            _ => FailureClass::Fatal,
        },
        SetaError::Http(e) if e.is_timeout() || e.is_connect() || e.is_request() => {
            FailureClass::Transient
        }
        _ => FailureClass::Fatal,
    }
}

#[cfg(test)]
#[path = "failure_class_tests.rs"]
mod tests;
