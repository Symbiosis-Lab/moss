//! moss-seta HTTP client — struct, auth helpers, error types, base URL resolution.
//!
//! Split out of the app-side `domain/moss_seta_client.rs` (since deleted).

use crate::identity::Identity;
use crate::seta::signing::sign_request;
use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;

/// Time to wait for a TCP+TLS connection to establish. Catches dead or
/// half-open connections (e.g. a dropped network, a wedged edge) fast without
/// limiting the duration of legitimate large uploads.
const SETA_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Overall per-request ceiling. A request that connects but then goes silent
/// fails after this instead of hanging the publish forever. Generous enough
/// for a single-file upload over HTTP/2; the small JSON sync/commit calls
/// (the ones observed hanging) finish in well under a second. Per-request
/// `.timeout(..)` overrides on the auth endpoints still take precedence.
const SETA_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Tight per-request ceiling for the user-initiated "Send Logs" upload, applied
/// as a `.timeout(..)` override on that one POST. The payload is a small (<1 MB)
/// JSON log tail, so it never needs the 120s [`SETA_REQUEST_TIMEOUT`] meant for
/// large site uploads. Bounding it here means a connected-but-silent / black-holed
/// upload surfaces a classified `is_timeout()` error in ~30s instead of freezing
/// the "Sending logs…" spinner for two minutes (which reads to the user as a hang).
const SEND_LOGS_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Build the shared reqwest client with connect + request timeouts.
///
/// `reqwest::Client::new()` (the previous construction) installs **no**
/// timeout at all, so any stalled request — most visibly the deploy's
/// `sync_manifest`/`upload`/`commit` calls, which add no per-request timeout —
/// would await forever. Bounding it here means every seta call surfaces a
/// retryable error instead of an unrecoverable hang.
fn build_seta_http_client() -> Client {
    Client::builder()
        // Identify moss on every request — see `system::user_agent` for
        // why an absent UA is what got moss challenged in the first place, and
        // why the string is shared rather than written out here (the net
        // diagnosis probe HEADs the same Cloudflare zone, so a second literal
        // here is a drift waiting to happen).
        .user_agent(crate::system::user_agent())
        .connect_timeout(SETA_CONNECT_TIMEOUT)
        .timeout(SETA_REQUEST_TIMEOUT)
        // Behind the GFW a DIRECT connection to api.mosspub.com (Cloudflare) is
        // reset, so every deploy `sync`/`upload`/`commit` and "Send logs" call —
        // all of which flow through this one shared client — failed with
        // `error sending request for url (...)`. reqwest never received the
        // CFNetworkCopyProxiesForURL proxy that the ureq download path already
        // uses, so it connected directly while the WKWebView (and curl's IPv6
        // path) reached Cloudflare fine. Match the browser's proxy PER request:
        // `Proxy::custom` re-invokes the resolver for every request URL, so a
        // live proxy change (VPN / Clash / V2Ray toggled) is followed with no
        // caching even though this `Client` is cached inside `MossSetaClient`.
        .proxy(crate::system::proxy_reqwest::system_proxy())
        .build()
        // Mirror `Client::new()`'s panic-on-TLS-init-failure contract.
        .expect("failed to build seta HTTP client")
}

/// Total tries for a transient failure: 1 attempt + 3 retries.
pub(super) const RETRY_MAX_ATTEMPTS: u32 = 4;
/// Backoff ceiling for the first retry; doubles each subsequent retry.
///
/// There is deliberately no separate max-delay constant: `RETRY_MAX_ATTEMPTS`
/// already bounds the reachable ceilings at 500/1000/2000ms, and
/// [`UPLOAD_RETRY_BUDGET`] — not a delay cap — is what actually stops a long
/// publish stalling on one file. A cap above 2000ms would be dead code.
const RETRY_BASE_DELAY_MS: u64 = 500;

/// Ceiling on how long ONE file may spend inside [`retry_transient`] **without
/// moving any bytes**.
///
/// Attempt counting is not a bound: a stalled connection only fails when
/// [`MAX_ATTEMPT_DURATION`] fires, and that surfaces as a retryable timeout.
///
/// It measures *futile* time, not total time — [`RetryBudget::note_progress`]
/// restarts the stopwatch on every success. Until 2026-08-04 it measured total
/// time, which made a per-file deadline a function of file size, the shape
/// invariant I1 forbids. A 100 MB file at 50 KB/s needs ~2000 s of honest transfer; sharing one
/// 600 s stopwatch across its chunks it could not retry a single blip past
/// t=600 s no matter how well it was doing.
pub(crate) const UPLOAD_RETRY_BUDGET: Duration = Duration::from_secs(600);

/// Failed attempts ONE file may accumulate before it is abandoned, across every
/// chunk and every re-entry into [`retry_transient`]. **Never reset by
/// progress**, which is what makes it the bound that survives
/// [`RetryBudget::note_progress`].
///
/// Once progress restarts the stopwatch, the wall clock stops being an
/// unconditional stop: `chunked_upload.rs` re-enters `retry_transient` per chunk
/// *and* on every `escalate_down` retry, each entry granting a fresh
/// [`RETRY_MAX_ATTEMPTS`]. A file alternating "one chunk lands, three attempts
/// fail" satisfies the stopwatch (reset by the chunk) and the deploy stall
/// watchdog (bumped per attempt) forever. Counting failures terminates that,
/// while still letting an arbitrarily large file that transfers cleanly take as
/// long as its bytes honestly need.
///
/// 12 is 3 exhausted `retry_transient` rounds — at most 12 x
/// [`MAX_ATTEMPT_DURATION`] of futile time.
const MAX_FAILED_ATTEMPTS_PER_FILE: u32 = 12;

/// Backpressure responses forgiven per file before they count as ordinary
/// faults. A 429/503 is the peer saying "not now", not "you are broken"
/// (`failure_class`, which also explains why 524 is deliberately NOT in that
/// class); charging the file's allowance for the server's own admission control
/// abandons a publish that only needed to wait. Bounded so forgiveness cannot
/// itself become a way to run forever.
const BACKPRESSURE_GRACE: u32 = 3;

/// Floor for the first forgiven backpressure backoff, doubling per occurrence.
/// An order of magnitude above [`RETRY_BASE_DELAY_MS`]: a retry that returns
/// before the peer has shed any load is, to that peer, more of the load.
const BACKPRESSURE_BASE_DELAY_MS: u64 = 2_000;

/// The longest a single attempt can run before its own request timeout fires.
///
/// `upload_policy::UPLOAD_REQUEST_TIMEOUT` is the largest per-request
/// `.timeout(..)` any `retry_transient` caller installs, so it is the worst
/// case the budget arithmetic must reserve headroom for.
const MAX_ATTEMPT_DURATION: Duration = super::upload_policy::UPLOAD_REQUEST_TIMEOUT;

/// Retry allowance for one logical unit of upload work — one file.
///
/// A chunked upload creates ONE budget and passes it to every chunk, so retries
/// cannot compound per chunk (a 10-chunk file must not get 10 separate
/// 4-attempt allowances). A single PUT creates one for itself.
///
/// It bounds two different things, and the pair is the point:
/// [`UPLOAD_RETRY_BUDGET`] caps consecutive time spent achieving nothing (reset
/// by [`note_progress`](Self::note_progress)); [`MAX_FAILED_ATTEMPTS_PER_FILE`]
/// caps cumulative failures (reset by nothing). Either alone is unsound — time
/// alone scales the deadline with file size, failures alone would let a
/// slow-but-healthy file retry forever.
///
/// Interior mutability because the seta module holds this by shared reference
/// across `.await` points; the lock is taken and dropped inside one expression
/// and never held across an await.
pub(super) struct RetryBudget {
    /// When this file last achieved something. NOT when it started.
    started: std::sync::Mutex<tokio::time::Instant>,
    failed_attempts: std::sync::atomic::AtomicU32,
    backpressure_seen: std::sync::atomic::AtomicU32,
}

impl RetryBudget {
    pub(super) fn new() -> Self {
        Self {
            started: std::sync::Mutex::new(tokio::time::Instant::now()),
            failed_attempts: std::sync::atomic::AtomicU32::new(0),
            backpressure_seen: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// An attempt moved bytes — restart the futile-time stopwatch.
    ///
    /// Called by [`retry_transient`] on every success, which covers every upload
    /// path: chunk PATCHes, single PUTs and control-plane calls all return
    /// through it. The failure count is deliberately NOT reset — see
    /// [`MAX_FAILED_ATTEMPTS_PER_FILE`].
    pub(super) fn note_progress(&self) {
        *self.started.lock().unwrap_or_else(|e| e.into_inner()) = tokio::time::Instant::now();
    }

    /// The next attempt will be **different** — restart the futile-time
    /// stopwatch, the same way [`note_progress`](Self::note_progress) does.
    ///
    /// `UPLOAD_RETRY_BUDGET` bounds time spent achieving nothing, and a retry
    /// that repeats a failed request achieves nothing. A retry at half the size
    /// is not that: it is the one move that carries new information about a
    /// link too slow for the request it was given. Charging it to a stopwatch
    /// that only ever runs forward made `escalate_down` unreachable in exactly
    /// the case it exists for — three 150 s timeouts spend 450 s, leaving less
    /// than `MAX_ATTEMPT_DURATION` of the 600 s window, so the ladder never got
    /// a second rung on a slow link and fired only when failures were instant.
    ///
    /// Termination does not depend on this stopwatch:
    /// [`MAX_FAILED_ATTEMPTS_PER_FILE`] is reset by nothing, and the ladder is
    /// finite on its own (`CHUNK_SIZE_MAX` halves to `MIN_ESCALATION_SIZE` in
    /// four rungs, and a rung that cannot go smaller does not escalate).
    pub(super) fn note_escalation(&self) {
        self.note_progress();
    }

    fn futile_elapsed(&self) -> Duration {
        self.started.lock().unwrap_or_else(|e| e.into_inner()).elapsed()
    }

    fn note_failed_attempt(&self) {
        self.failed_attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Take one unit of backpressure forgiveness, returning which occurrence
    /// this is (so the caller can back off further each time), or `None` once
    /// [`BACKPRESSURE_GRACE`] is spent.
    fn forgive_backpressure(&self) -> Option<u32> {
        let n = self
            .backpressure_seen
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (n < BACKPRESSURE_GRACE).then_some(n)
    }

    /// May another attempt be launched after waiting `delay`?
    ///
    /// Only if the file has failures left AND the attempt can *finish* inside
    /// the futile-time budget — hence the [`MAX_ATTEMPT_DURATION`] headroom:
    /// every attempt this admits ends within `UPLOAD_RETRY_BUDGET` of the last
    /// progress, which makes the bound provable rather than approximate.
    /// `pub(super)` because [`retry_transient`] always grants one attempt: the
    /// chunk loop re-enters it and takes its own `continue` branches, so it must
    /// gate itself on both bounds.
    pub(super) fn allows_another_attempt(&self, delay: Duration) -> bool {
        self.failed_attempts.load(std::sync::atomic::Ordering::Relaxed)
            < MAX_FAILED_ATTEMPTS_PER_FILE
            && self.futile_elapsed() + delay + MAX_ATTEMPT_DURATION <= UPLOAD_RETRY_BUDGET
    }
}

/// "Full jitter" backoff: a uniform random wait in `[0, 500ms << n]`.
///
/// The jitter is load-bearing, not decoration. In the 2026-07-21 incident 8
/// uploads failed in the *same second* because seta prod restarted under ~20
/// in-flight upload tasks. An unjittered `500ms << n` would make every one of
/// those tasks retry at the identical instant, re-colliding on the origin that
/// is still coming up — the classic thundering herd. Spreading each task
/// uniformly across its window decorrelates them (AWS "Exponential Backoff and
/// Jitter"). Kept a plain sync fn so the RNG is never held across an `.await`.
fn full_jitter_delay(retry_index: u32) -> Duration {
    use rand::Rng;
    let ceiling = RETRY_BASE_DELAY_MS.saturating_mul(1u64 << retry_index.min(20));
    Duration::from_millis(rand::thread_rng().gen_range(0..=ceiling))
}

/// Backoff after a forgiven backpressure response: a floor of `2s <<
/// occurrence`, plus the same again as jitter.
///
/// Unlike [`full_jitter_delay`] this has a floor, because the two waits answer
/// different questions. Full jitter only has to *decorrelate* a herd that failed
/// at the same instant, so a draw near zero is fine. This one has to give the
/// peer time to recover, which a draw near zero would defeat — so the jitter
/// sits on top of a wait that is always long enough to matter.
fn backpressure_delay(occurrence: u32) -> Duration {
    use rand::Rng;
    let floor = BACKPRESSURE_BASE_DELAY_MS.saturating_mul(1u64 << occurrence.min(20));
    Duration::from_millis(floor + rand::thread_rng().gen_range(0..=floor))
}

/// Run `op`, retrying it while it fails *transiently*, up to
/// [`RETRY_MAX_ATTEMPTS`] tries with [`full_jitter_delay`] between them.
///
/// `op` must encapsulate the **whole sign-and-send unit**, not just the send:
/// `seta/signing.rs` signs `"{timestamp}:{payload}"` and seta rejects a
/// clock skew over 300s, so a cached `Authorization` header cannot be replayed
/// on a later attempt. That is why callers pass a closure that re-runs
/// `sign_and_build` per attempt instead of wrapping an already-built request.
///
/// `what` names the unit of work for the warn log so a retried publish is
/// diagnosable after the fact.
///
/// `budget` bounds the whole thing; pass the SAME budget for every chunk of one
/// file so retries can't compound across chunks.
pub(super) async fn retry_transient<F, Fut, T>(
    what: &str,
    budget: &RetryBudget,
    mut op: F,
) -> Result<T, SetaError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, SetaError>>,
{
    use super::failure_class::{FailureClass, classify};
    let mut retries = 0u32;
    loop {
        match op().await {
            Ok(value) => {
                // This attempt moved bytes. Restart the futile-time stopwatch
                // so the file's retry allowance is not consumed by its own
                // successful transfer (invariant I1).
                budget.note_progress();
                return Ok(value);
            }
            Err(e) => {
                let class = classify(&e);
                if class == FailureClass::Fatal {
                    return Err(e);
                }
                // "Not now" is not "you are broken": the first few cost neither
                // a retry slot nor a failed-attempt slot, and wait longer.
                let eased = (class == FailureClass::Backpressure)
                    .then(|| budget.forgive_backpressure())
                    .flatten();
                let delay = match eased {
                    Some(n) => backpressure_delay(n),
                    None => full_jitter_delay(retries),
                };
                if eased.is_none() {
                    budget.note_failed_attempt();
                    retries += 1;
                    if retries >= RETRY_MAX_ATTEMPTS {
                        return Err(e);
                    }
                }
                // Give up while the failure is still the ORIGINAL one: a retry
                // that cannot finish inside the budget would only trade a
                // reported blip for a whole-publish timeout.
                if !budget.allows_another_attempt(delay) {
                    log::warn!(
                        target: "seta",
                        "{} failed transiently ({}); out of retry budget ({}s idle or {} failed attempts), giving up",
                        what, e, UPLOAD_RETRY_BUDGET.as_secs(), MAX_FAILED_ATTEMPTS_PER_FILE
                    );
                    return Err(e);
                }
                log::warn!(
                    target: "seta",
                    "{} failed ({:?}: {}); retrying in {}ms ({} retries spent of {})",
                    what, class, e, delay.as_millis(), retries, RETRY_MAX_ATTEMPTS
                );
                // Deciding to try again IS liveness, and it is the only signal
                // this stretch of the publish emits: byte credits alone leave a
                // window of small files that each time out twice legitimately
                // quiet for 150 + 2 + 150 = 302 s, just over
                // `activity::STALL_TIMEOUT`, and the watchdog would kill a
                // healthy deploy. A backpressure wait counts the same way — the
                // publish is alive, it is being asked to slow down.
                crate::infra::liveness::bump();
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Default moss-seta URL - production backend.
pub(super) const DEFAULT_SETA_URL: &str = "https://api.mosspub.com";

/// Environment variable name for overriding the seta URL.
pub(super) const SETA_URL_ENV_VAR: &str = "MOSS_SETA_URL";

/// Get the seta URL, preferring environment variable over default.
///
/// Checks sources in order:
/// 1. Runtime `MOSS_SETA_URL` environment variable (for local dev)
/// 2. Compile-time `MOSS_SETA_URL` (for CI builds where runtime env isn't available)
/// 3. `DEFAULT_SETA_URL` as final fallback
pub fn get_seta_url() -> String {
    // First try runtime env var (for local development).
    // Sanitize at this boundary so a misconfigured env var doesn't crash
    // the app via `validate_url` panic — fall back to the default URL
    // with a warning instead.
    if let Ok(url) = std::env::var(SETA_URL_ENV_VAR) {
        if is_safe_base_url(&url) {
            return url;
        }
        log::warn!(
            "{} is set but fails the HTTPS/localhost check ({}); falling back to default",
            SETA_URL_ENV_VAR,
            url
        );
    }

    // Then try compile-time env var (for CI builds)
    if let Some(url) = option_env!("MOSS_SETA_URL") {
        return url.to_string();
    }

    // Fall back to production URL
    DEFAULT_SETA_URL.to_string()
}

/// Mirrors `MossSetaClient::validate_url`'s check without panicking.
/// Used by `get_seta_url()` to gate the runtime env var.
fn is_safe_base_url(url: &str) -> bool {
    let url = url.trim_end_matches('/');
    let is_localhost = url.contains("localhost") || url.contains("127.0.0.1");
    let is_https = url.starts_with("https://");
    is_localhost || is_https
}

/// moss-seta client for backend communication.
///
/// The client can be constructed with or without an `Identity`:
/// - **With identity** (`with_identity`): For authenticated endpoints (purchase, DNS, etc.)
/// - **Without identity** (`new`): For public endpoints (search, suggest)
///
/// When constructed with an identity, the identity is obtained from `AppState`
/// via `IdentityService`, not from disk.
#[derive(Clone)]
pub struct MossSetaClient {
    pub(super) client: Client,
    pub(super) base_url: String,
    pub(super) identity: Option<Identity>,
}

/// Error types for moss-seta operations.
#[derive(Debug, thiserror::Error)]
pub enum SetaError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Identity error: {0}")]
    Identity(String),

    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },

    #[error("Failed to parse response: {0}")]
    Parse(String),

    #[error("I/O error: {0}")]
    Io(String),

    /// The seta invite gate (`POST /api/sites`, gated by `INVITE_GATE_ENABLED`)
    /// rejected a verified email that isn't on the allowlist:
    /// `403 {error:'not_allowlisted', apply_url:'<url>'}`. Carries `apply_url`
    /// (may be empty when the server's `APPLY_URL` is unset) so the deploy call
    /// sites can surface the friendly invite message + apply link rather than an
    /// opaque error. The Display string is a diagnostic only — the user-facing,
    /// localized copy is built at the call sites via `app_advisory`.
    #[error("not allowlisted (apply at: {apply_url})")]
    NotAllowlisted { apply_url: String },

    /// An edge interstitial — in practice a Cloudflare bot/managed challenge —
    /// blocked the request before it ever reached seta.
    ///
    /// This is a SEPARATE variant, not an `Api { status: 403 }`, because "this
    /// was a challenge" is a fact every caller needs and only
    /// [`SetaError::from_failed_response`] can observe it (the `cf-mitigated`
    /// header and the body are gone by the time a caller sees the error).
    /// Folding it into `Api` threw that knowledge away the instant it was
    /// computed, and each caller then re-guessed from the status alone:
    /// `domain::commands` answered a challenge with "Try again in a moment",
    /// and `orchestrator` read a challenge 403 as "email verification
    /// required". A native client can never solve a managed challenge, so both
    /// guesses were wrong the same way — they send the user back to retry
    /// something that cannot succeed (the 2026-07-21 Bot Fight Mode incident:
    /// ~21 manual retries against a permanent block).
    ///
    /// Display is the authored user-facing copy itself ([`CHALLENGE_MESSAGE`]),
    /// so the call sites that already fall back to `e.to_string()` surface the
    /// right sentence with no change.
    #[error("{}", CHALLENGE_MESSAGE)]
    Challenge { status: u16 },
}

/// Parse a seta invite-gate `not_allowlisted` 403, returning `apply_url`.
///
/// Returns `Some(apply_url)` only when `status == 403` AND the body's `error`
/// field is exactly `"not_allowlisted"` — the shape the seta invite gate emits
/// from `POST /api/sites`: `{"error":"not_allowlisted","apply_url":"<url>"}`.
/// `apply_url` may be an empty string (the server sends `""` when its `APPLY_URL`
/// env var is unset); a missing `apply_url` key also yields `Some("")`. Every
/// other status / error reason returns `None` so the caller falls through to the
/// normal failure path. Pure (status + body in, `Option<String>` out) so it's
/// unit-testable without constructing a `reqwest::Response`.
pub(super) fn parse_not_allowlisted(status: u16, body: &str) -> Option<String> {
    if status != 403 {
        return None;
    }
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    if json.get("error").and_then(|v| v.as_str()) != Some("not_allowlisted") {
        return None;
    }
    Some(
        json.get("apply_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    )
}

/// Extract the user-facing message from a moss-seta error response body.
///
/// moss-seta returns errors as `{"error": "..."}` JSON; auth endpoints sometimes
/// use `{"message": "..."}`. If either key parses, that string is returned;
/// otherwise the raw body is returned (length-capped). Used wherever an error
/// surface reaches the UI, so we don't dump raw JSON or an HTML page.
pub(super) fn extract_api_error_message(body: &str) -> String {
    // Cap the raw fallback at 200 chars to avoid leaking full HTML error
    // pages (e.g. a proxy 502 page) into UI toasts or log lines. The JSON
    // `error`/`message` field, when present, is already authored and short.
    const MAX_RAW_LEN: usize = 200;
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .or_else(|| json.get("message"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            if body.len() > MAX_RAW_LEN {
                // char_indices to avoid slicing mid-UTF-8.
                let end = body
                    .char_indices()
                    .take_while(|(i, _)| *i < MAX_RAW_LEN)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}…", body.get(..end).unwrap_or(body))
            } else {
                body.to_string()
            }
        })
}

/// THE user-facing sentence for an edge-challenge block, in one place.
///
/// A native client can never solve a managed challenge, so the block is
/// PERMANENT for moss — the copy must not promise that waiting or retrying
/// helps. The old "usually temporary, please try again in a moment" wording
/// drove a user through ~21 futile manual retries in the 2026-07-21 Cloudflare
/// Bot Fight Mode incident. Point instead at the two things that can actually
/// change the outcome: updating moss (client-fingerprint changes ship in
/// releases) and switching networks (VPN / shared-IP reputation is a common
/// challenge trigger).
///
/// This is the *bare* reason: [`SetaError`] Display surfaces it as-is and the
/// toast layer adds any "Failed to …" prefix, so it must repeat neither a
/// status nor a prefix or the message compounds them.
pub(super) const CHALLENGE_MESSAGE: &str =
    "a network security check blocked the request — retrying won't fix this; \
     make sure moss is up to date, and if it persists try a different network \
     (VPNs and shared connections can trigger it)";

/// Was this non-success response an edge interstitial rather than a real seta
/// answer?
///
/// An edge challenge returns an HTML page our native HTTP client cannot solve.
/// Surfacing that markup floods the UI with an unreadable wall of HTML (it
/// differs on every request and is useless to the user), and — worse — the
/// status alone is indistinguishable from a genuine 403, so callers that read
/// only the status hand the user retry advice for a permanent block. Detect it
/// here, once, and let [`SetaError::Challenge`] carry the answer to every
/// caller.
///
/// Pure (header + body in, bool out) so the detection table is unit-testable
/// without constructing a `reqwest::Response`.
pub(super) fn is_edge_challenge(cf_mitigated: Option<&str>, body: &str) -> bool {
    const CHALLENGE_MARKERS: [&str; 3] =
        ["challenge-platform", "_cf_chl_opt", "Just a moment"];
    // `cf-mitigated: challenge` is Cloudflare's authoritative signal; the body
    // markers are a best-effort fallback for challenge variants that omit it. A
    // miss on both just falls through to the capped raw body — still safe.
    cf_mitigated == Some("challenge") || CHALLENGE_MARKERS.iter().any(|m| body.contains(m))
}

impl SetaError {
    /// Is this failure worth trying again, unchanged, in a moment?
    ///
    /// The boolean facade over [`super::failure_class::classify`], kept because
    /// callers outside the retry loop only ever need "re-send or don't" — the
    /// difference between a fault and backpressure is the retry loop's business
    /// alone. Derived rather than restated so the two answers cannot drift.
    pub(super) fn is_transient(&self) -> bool {
        super::failure_class::classify(self) != super::failure_class::FailureClass::Fatal
    }

    /// Classify a non-success response into a [`SetaError`], consuming it.
    ///
    /// The single failed-response path for every seta endpoint: read the status
    /// and the `cf-mitigated` header, then the body, and route to exactly one of
    /// three outcomes — [`SetaError::NotAllowlisted`] for the invite gate,
    /// [`SetaError::Challenge`] for an edge interstitial our native client can't
    /// solve, otherwise [`SetaError::Api`] carrying the authored JSON
    /// `error`/`message` or a length-capped raw body. Replaces the hand-rolled
    /// `is_success()` blocks across the seta module so no endpoint can leak a raw
    /// challenge page or an unbounded error body, and so no endpoint can lose the
    /// challenge classification that keeps retry advice off a permanent block.
    ///
    /// "Single" is meant literally and is worth re-checking on every new
    /// endpoint: `rg -n "SetaError::Api \{" crates/moss-build/src/seta/` must
    /// match only the construction below. Two endpoints had drifted —
    /// `fetch_stripe_publishable_key` and `link_custom_domain`'s 409 arm — and
    /// each quietly opted out of the ray-id log, the length cap, and the
    /// challenge check. A status-specific arm may still branch on the status
    /// (to log, or to pick a different variant), but it must hand the
    /// `Response` itself to this function.
    pub(super) async fn from_failed_response(response: reqwest::Response) -> SetaError {
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let cf_mitigated = response
            .headers()
            .get("cf-mitigated")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        // The Cloudflare ray id is the one token support can paste into the
        // Cloudflare dashboard to see exactly which edge rule fired (the
        // 2026-07-21 Bot Fight Mode diagnosis took an hour of forensics for
        // want of it). Log-only: it must never reach the user-facing
        // `SetaError::Api` message.
        let cf_ray = response
            .headers()
            .get("cf-ray")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let body = response.text().await.unwrap_or_default();
        // Invite gate: a 403 `not_allowlisted` is a distinct, actionable case —
        // route it to its own variant so the deploy call sites can surface the
        // friendly "申请免费试用" invite message + apply link instead of a generic
        // API error string. Everything else flows on to the challenge check.
        if let Some(apply_url) = parse_not_allowlisted(status, &body) {
            log::warn!(target: "seta", "request to {} rejected by invite gate (not allowlisted)", url);
            return SetaError::NotAllowlisted { apply_url };
        }
        let ray_suffix = cf_ray
            .as_ref()
            .map(|r| format!(" [cf-ray: {r}]"))
            .unwrap_or_default();
        // Classify BEFORE the message is built, so the fact survives into the
        // type instead of only shaping a string. `Challenge` deliberately
        // carries no body text: the page is unsolvable HTML, and its Display is
        // the authored copy.
        if is_edge_challenge(cf_mitigated.as_deref(), &body) {
            log::warn!(
                target: "seta",
                "request to {} blocked by an edge challenge ({}) — a native client cannot solve it{}",
                url, status, ray_suffix
            );
            return SetaError::Challenge { status };
        }
        let message = extract_api_error_message(&body);
        // One durable diagnostic line per failure, at `warn` so it survives the
        // release-default `Info` level: with the per-site `log::error!` lines
        // gone, this is the only seta failure record in pulled "Send logs"
        // telemetry (the deploy/payment paths don't re-log). The cleaned message
        // keeps challenge HTML / unbounded bodies out of the log; callers that
        // need a louder signal still escalate to `error` themselves.
        log::warn!(target: "seta", "request to {} failed ({}): {}{}", url, status, message, ray_suffix);
        SetaError::Api { status, message }
    }
}

/// Auth session status returned by the polling endpoint.
#[derive(Debug, Deserialize, Serialize, specta::Type)]
pub struct AuthSessionStatus {
    pub status: String,
    pub email: Option<String>,
}

/// Response from GET /auth/whoami — the email linked to the caller's pubkey,
/// or null if the pubkey has never been linked to an email on this server.
///
/// Since PR 5 (server-side pubkey-ACL), the response also carries the email-keyed
/// subscription so the client can resolve subscription status in a single round
/// trip without a separate pubkey-scoped lookup. `subscription` deserializes to
/// `None` when:
///   - the caller's email has no subscription record, or
///   - the server pre-dates PR 5 and omits the field entirely (backward-compat).
#[derive(Debug, Deserialize, Serialize, specta::Type)]
pub struct WhoamiResponse {
    pub pubkey: String,
    pub email: Option<String>,
    /// Email-keyed subscription status. `None` means "no active subscription"
    /// (includes the pre-PR-5 server case where the field is absent).
    #[serde(default)]
    pub subscription: Option<crate::seta::domains::SubscriptionStatus>,
}

impl MossSetaClient {
    /// Create a new unauthenticated client with the default seta URL.
    ///
    /// **Env-blind** — always targets production. Prefer
    /// [`MossSetaClient::new_for_environment`] in production code so staging
    /// and local environments are respected. Restricted to `pub(crate)` to
    /// prevent accidental usage outside tests.
    pub(crate) fn new() -> Self {
        Self::with_url(&get_seta_url())
    }

    /// Create an unauthenticated client with a custom moss-seta URL.
    ///
    /// # Panics
    /// Panics if a non-localhost HTTP URL is provided.
    pub fn with_url(base_url: &str) -> Self {
        let url = Self::validate_url(base_url);
        Self {
            client: build_seta_http_client(),
            base_url: url,
            identity: None,
        }
    }

    /// Create a new client with an identity and the default seta URL.
    ///
    /// **Env-blind** — always targets production. Prefer
    /// [`MossSetaClient::for_environment`] in production code so staging
    /// and local environments are respected. `pub(crate)` so the restriction
    /// steers callers toward the env-aware constructor; the only callers are
    /// in-crate tests.
    pub(crate) fn with_identity(identity: &Identity) -> Self {
        Self::with_identity_and_url(identity, &get_seta_url())
    }

    /// Create a client for a specific hosting environment.
    /// `MOSS_SETA_URL` (raw URL) still overrides for local dev.
    pub fn for_environment(identity: &Identity, env: &crate::config::environment::HostingEnvironment) -> Self {
        let base_url = match std::env::var("MOSS_SETA_URL") {
            Ok(url) => url,
            Err(_) => env.seta_url().to_string(),
        };
        Self::with_identity_and_url(identity, &base_url)
    }

    /// Create a client with a specific hosting environment and no identity (unauthenticated).
    pub fn new_for_environment(env: &crate::config::environment::HostingEnvironment) -> Self {
        let base_url = match std::env::var("MOSS_SETA_URL") {
            Ok(url) => url,
            Err(_) => env.seta_url().to_string(),
        };
        Self::with_url(&base_url)
    }

    /// Create a client with a custom moss-seta URL and identity.
    ///
    /// # Panics
    /// Panics if a non-localhost HTTP URL is provided.
    pub fn with_identity_and_url(identity: &Identity, base_url: &str) -> Self {
        let url = Self::validate_url(base_url);
        Self {
            client: build_seta_http_client(),
            base_url: url,
            identity: Some(identity.clone()),
        }
    }

    /// Validate and normalize a base URL. Enforces HTTPS for non-localhost URLs.
    ///
    /// # Panics
    /// Panics if a non-localhost HTTP URL is provided.
    fn validate_url(base_url: &str) -> String {
        let url = base_url.trim_end_matches('/');

        let is_localhost = url.contains("localhost") || url.contains("127.0.0.1");
        let is_https = url.starts_with("https://");

        if !is_localhost && !is_https {
            panic!(
                "Security error: moss-seta URL must use HTTPS for non-localhost addresses. \
                 Received: {}. Use https:// to ensure sensitive data is encrypted in transit.",
                url
            );
        }

        url.to_string()
    }

    /// Sign a request payload, returning the Authorization header value.
    ///
    /// # Errors
    /// Returns `SetaError::Identity` if the client was created without an identity.
    pub fn sign_request_payload(&self, payload: &str) -> Result<String, SetaError> {
        let identity = self.identity.as_ref().ok_or_else(|| {
            SetaError::Identity(
                "Cannot sign request: client created without identity. \
                 Use MossSetaClient::with_identity() for authenticated endpoints."
                    .to_string(),
            )
        })?;
        let signed = sign_request(identity, payload)
            .map_err(|e| SetaError::Identity(e.to_string()))?;
        Ok(signed.to_auth_header())
    }

    /// Build a URL, verifying it uses secure transport.
    pub(super) fn secure_url(&self, path: &str) -> String {
        let url = format!("{}{}", self.base_url, path);

        let is_localhost = self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1");
        let is_https = self.base_url.starts_with("https://");

        debug_assert!(
            is_localhost || is_https,
            "URL must use HTTPS for non-localhost. Base URL: {}",
            self.base_url
        );

        url
    }

    /// Build URL and Authorization header for an authenticated request.
    ///
    /// Takes the canonical wire-form path (already percent-encoded for any
    /// user-controlled segments) and uses the same string for both signing
    /// and URL construction. Known limitation: dot-segments in a path are not
    /// specially handled.
    ///
    /// # Errors
    /// Returns `SetaError::Identity` if the client was created without an identity.
    pub(super) fn sign_and_build(&self, path: &str) -> Result<(String, String), SetaError> {
        let auth = self.sign_request_payload(path)?;
        let url = self.secure_url(path);
        Ok((url, auth))
    }

    /// Signed GET → deserialize JSON response.
    ///
    /// Path-only signing (matches the existing `whoami` / `send_logs` pattern):
    /// the server authenticates via pubkey + path signature.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, SetaError> {
        let auth = self.sign_request_payload(path)?;
        let url = self.secure_url(path);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SetaError::from_failed_response(resp).await);
        }
        resp.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Signed POST with JSON body → deserialize JSON response.
    ///
    /// Note: the signature covers the path only (same pattern as
    /// `whoami` / `send_logs`). The body is sent in the clear; the server
    /// authenticates via pubkey + path signature and trusts the body content
    /// once auth passes.
    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, SetaError> {
        let auth = self.sign_request_payload(path)?;
        let url = self.secure_url(path);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SetaError::from_failed_response(resp).await);
        }
        resp.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    // ========== Auth Endpoints ==========

    /// Request email verification via magic link.
    pub async fn request_auth(
        &self,
        email: &str,
        pubkey: &str,
        session_id: &str,
    ) -> Result<(), SetaError> {
        let url = self.secure_url("/auth/request");

        log::debug!(target: "seta", "POST {} email={}", url, email);

        let response = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(15))
            .json(&serde_json::json!({
                "email": email,
                "pubkey": pubkey,
                "session_id": session_id,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }
        Ok(())
    }

    /// Request a destructive re-verify magic link.
    ///
    /// POSTs `/auth/request` with `mode='re_verify'` + `site_id`. The server
    /// sends an email that, when confirmed, replaces the active authorization
    /// for the site with the caller's pubkey — revoking every other device/
    /// folder currently linked to that site.
    pub async fn request_reverify(
        &self,
        email: &str,
        pubkey: &str,
        session_id: &str,
        site_id: &str,
    ) -> Result<(), SetaError> {
        let url = self.secure_url("/auth/request");

        log::debug!(target: "seta", "POST {} (re_verify) email={} site_id={}", url, email, site_id);

        let response = self.client
            .post(&url)
            .timeout(std::time::Duration::from_secs(15))
            .json(&serde_json::json!({
                "email": email,
                "pubkey": pubkey,
                "session_id": session_id,
                "mode": "re_verify",
                "site_id": site_id,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }
        Ok(())
    }

    /// Check email verification status by polling session.
    pub async fn check_auth_status(
        &self,
        session_id: &str,
    ) -> Result<AuthSessionStatus, SetaError> {
        let url = format!("{}/auth/status/{}", self.base_url, session_id);

        let response = self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Ask the server which email is linked to the caller's pubkey.
    pub async fn whoami(&self) -> Result<WhoamiResponse, SetaError> {
        let auth_header = self.sign_request_payload("/auth/whoami")?;
        let url = self.secure_url("/auth/whoami");

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Fetch the Stripe publishable key for THIS client's environment from the backend.
    ///
    /// Anonymous GET — no auth header required.  The endpoint is intentionally
    /// public so non-authenticated users can still reach the payment form.
    pub async fn fetch_stripe_publishable_key(&self) -> Result<String, SetaError> {
        #[derive(serde::Deserialize)]
        struct Resp { publishable_key: String }
        let url = self.secure_url("/api/config/stripe-publishable-key");
        let resp = self
            .client
            .get(&url)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SetaError::from_failed_response(resp).await);
        }
        let body: Resp = resp.json().await.map_err(|e| SetaError::Parse(e.to_string()))?;
        Ok(body.publishable_key)
    }

    /// Send application logs to moss-seta for debugging/support.
    /// Returns the server-assigned ticket id (e.g. "LOG-2A6D-T1820-04-26")
    /// so the caller can surface it to the user. Pre-Thread-5 servers
    /// return `{ success: true }` with no ticket; in that case we return None.
    pub async fn send_logs(&self, payload: &serde_json::Value) -> Result<Option<String>, SetaError> {
        let auth_header = self.sign_request_payload("/api/logs")?;
        let url = self.secure_url("/api/logs");

        let response = self.client
            .post(&url)
            // Fail fast: a small log upload must not inherit the 120s upload
            // ceiling and leave the "Sending logs…" spinner looking hung.
            .timeout(SEND_LOGS_REQUEST_TIMEOUT)
            .header("Authorization", auth_header)
            .json(payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        // Best-effort ticket extraction. Older seta returns `{success: true}`
        // without ticket_id; treat as None.
        let body: serde_json::Value = response.json().await
            .unwrap_or(serde_json::json!({}));
        let ticket = body.get("ticket_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(ticket)
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
