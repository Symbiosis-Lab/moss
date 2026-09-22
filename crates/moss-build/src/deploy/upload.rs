//! The one place a file gets turned into upload requests, and the window that
//! decides how many may be in flight.
//!
//! # Why this module exists
//!
//! moss has two upload loops — the interactive publish in `deploy.rs` and
//! `moss deploy --prebuilt` in `deploy/prebuilt.rs`. They were copies, and they
//! had drifted: `prebuilt.rs` had **no size routing at all** and would send a
//! 100 MB video as a single PUT. A fix applied to one was not a fix.
//!
//! They are not identical, and pretending otherwise is how the drift started.
//! What they genuinely share is *"given a file on disk, produce the right
//! requests"* and *"admit work at a rate the uplink can sustain"*. That is what
//! lives here. Progress reporting and symlink handling stay with their callers,
//! because those really do differ.
//!
//! # The two things that differ, and are therefore parameters
//!
//! **Hash algorithm.** `deploy.rs` manifests carry xxh3_64 (16 hex chars);
//! `prebuilt.rs` builds its manifest with Sha256 (64 hex). A shared function
//! that hard-coded either one would fail 100% of the other's uploads. Hence
//! [`HashAlgo`] — the caller names its algorithm and this module decides
//! whether to verify buffered or streaming.
//!
//! **Byte accounting.** The interactive path drives a 4 Hz byte-based progress
//! ticker; the CLI path counts files. Hence the optional `on_bytes` callback.
//!
//! # Sizing
//!
//! Every number is in `domain::seta::upload_policy`, with the measurement that
//! justifies it. Diagnosis:
//! `docs/archive/2026-08-03-publish-resilience-slow-uplinks.md`.

use std::path::Path;

use crate::seta::upload_policy;
use crate::seta::client::MossSetaClient;

/// Which digest the caller's manifest entries carry.
///
/// Not a stylistic choice — the two deploy paths genuinely use different
/// algorithms, and conflating them is a whole-feature outage rather than a
/// subtle bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgo {
    /// `deploy.rs` — the sealed build manifest (`build/assets/paths.rs`).
    Xxh3,
    /// `deploy/prebuilt.rs` — the manifest it walks the prebuilt dir to build.
    Sha256,
}

impl HashAlgo {
    /// Digest of an in-memory buffer, as lowercase hex.
    fn hash_bytes(self, bytes: &[u8]) -> String {
        match self {
            HashAlgo::Xxh3 => crate::build::assets::paths::compute_binary_hash(bytes),
            HashAlgo::Sha256 => {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(bytes);
                hex::encode(h.finalize())
            }
        }
    }

    /// Digest of a file read incrementally — identical output to
    /// [`Self::hash_bytes`] over the same content, without buffering it.
    ///
    /// Needed because lowering the chunking threshold to 4 MB would otherwise
    /// *widen* the band of files that go out unverified. Before, only files
    /// >20 MB skipped the integrity check; a naive threshold change would have
    /// made that >4 MB. Streaming keeps the guarantee at every size.
    ///
    /// Both arms bump the stall clock per buffer. This runs for every chunked
    /// file *before* its first request, so it credits no bytes and emits no
    /// progress: without the bump, hashing a large video on a slow disk (or an
    /// iCloud file that is materialising) would be indistinguishable from a
    /// wedged publish and `activity::watchdog` would cancel it during work that
    /// is going perfectly well.
    fn hash_file(self, path: &Path) -> Result<String, String> {
        match self {
            HashAlgo::Xxh3 => crate::build::assets::paths::compute_binary_hash_file_with_heartbeat(
                path,
                &|| crate::infra::liveness::bump(),
            ),
            HashAlgo::Sha256 => {
                use sha2::{Digest, Sha256};
                use std::io::Read;
                // allow:raw_read built output being hashed for upload — dataless is absent (ADR-043)
                let mut file = std::fs::File::open(path)
                    .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
                let mut hasher = Sha256::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = file
                        .read(&mut buf)
                        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                    crate::infra::liveness::bump();
                }
                Ok(hex::encode(hasher.finalize()))
            }
        }
    }
}

/// Compare a digest against the manifest's, tolerating a manifest entry with no
/// hash at all.
///
/// An empty `expected` means a legacy or regressed manifest entry that carries
/// no hash. Skipping is deliberate: failing the deploy would strand a user
/// behind a stale build cache with no way forward, and the server's own
/// completeness gate still applies.
fn compare(file_path: &str, expected: &str, actual: &str) -> Result<(), String> {
    if expected.is_empty() {
        log::debug!(
            "[deploy] no hash in manifest entry for {file_path} — skipping integrity check \
             (re-build to generate one)"
        );
        return Ok(());
    }
    if expected != actual {
        return Err(format!(
            "Deploy integrity error: file bytes do not match sealed manifest for {file_path} \
             (expected {expected}, got {actual})"
        ));
    }
    Ok(())
}

/// Verify an in-memory body against its manifest hash.
///
/// Kept as the buffered form for the single-PUT path, which already holds the
/// exact bytes it is about to send. That is a strictly stronger check than
/// re-reading the path: it cannot be defeated by the file changing between the
/// hash and the send.
pub(crate) fn verify_bytes(
    file_path: &str,
    bytes: &[u8],
    expected: &str,
    algo: HashAlgo,
) -> Result<(), String> {
    compare(file_path, expected, &algo.hash_bytes(bytes))
}

/// Settle pause between the first and second read of a file whose hash
/// didn't match the sealed manifest, before trusting it enough to self-heal.
///
/// A single fresh read is not proof of anything: a torn read mid-write looks
/// exactly like a finished rebuild's bytes, and only re-checking after a
/// pause tells the two apart. 250ms mirrors the file watcher's own debounce
/// window (`build/watch.rs`: "a fixed 250ms per-event delay via
/// notify-debouncer-full") — the same "how long until a write has almost
/// certainly settled" number this codebase already trusts elsewhere, rather
/// than a new one invented for this check.
pub(crate) const DRIFT_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// How many leading bytes to sample when checking whether a file looks like
/// a zeroed-out iCloud eviction stub, without reading a large chunked file in
/// full. Matches the streaming hasher's own read buffer size ([`HashAlgo::hash_file`]).
const ZEROED_STUB_SAMPLE_BYTES: usize = 64 * 1024;

/// The one corruption shape this module knows how to recognize by content:
/// iCloud's "optimize storage" eviction zeroes a file in place, leaving a
/// stub of the right length and the wrong (all-zero) bytes —
/// `a_zeroed_icloud_stub_is_rejected_before_it_is_uploaded` is the fixture
/// this exists for. A torn write mid-rebuild does not look like this: it
/// leaves whatever partial content the writer had actually flushed, which is
/// essentially never all zero, so this check does not fire on the race this
/// module exists to tolerate.
fn looks_like_zeroed_stub(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b == 0)
}

/// Read up to `n` leading bytes of `path` — a cheap corruption sniff that
/// does not require loading a large chunked file in full just to check
/// whether it looks like a zeroed stub.
async fn read_leading_bytes(path: &Path, n: usize) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    // `path` is always the sealed generation's `canonical` file under
    // `.moss/build.nosync/generations/`, not a vault input; the bytes only feed
    // looks_like_zeroed_stub below, so a torn or corrupted read is caught by
    // content rather than trusted as the file's real state.
    // allow:raw_read built output — dataless is absent (ADR-043)
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut buf = vec![0u8; n];
    let read = file
        .read(&mut buf)
        .await
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    buf.truncate(read);
    Ok(buf)
}

/// How many self-heals one deploy tolerates before treating the pattern
/// itself as the problem.
///
/// A drifted hash or two in one deploy is exactly the race this module
/// exists to tolerate — a background rebuild racing the first one. More than
/// that, in the SAME deploy, is not the same failure: it is evidence of
/// something systemic (a stale or wrong stage directory, a build that never
/// finished) that healing file-by-file would otherwise paper over silently,
/// one warning at a time, until nobody notices the publish shipped mostly
/// self-healed content. Floor of 3 so a small publish isn't capped at zero; 5%
/// beyond that so a large one's tolerance scales with it.
///
/// `uploading` is how many files THIS deploy uploads (`diff.need.len()`, what
/// the server asked for), not how many the site has: an incremental publish
/// uploads a handful of files, so its cap is the floor of 3 however large the
/// site is. That is deliberate — the cap is a share of what this deploy is
/// moving, since only those files can drift from the sealed manifest in it.
pub(crate) fn self_heal_cap(uploading: usize) -> usize {
    (uploading / 20).max(3)
}

/// Enforce [`self_heal_cap`] against one more self-heal. Bumps the shared
/// counter and, once the cap is crossed, returns the loud failure this
/// module falls back to instead of quietly healing every remaining file —
/// `UploadWindow`'s existing first-error-aborts-everything policy takes it
/// from there.
///
/// `pub(crate)` because `deploy::push`'s symlink check (not routed through
/// [`upload_regular_file`]) needs the identical cap discipline and must not
/// grow a second copy of it.
pub(crate) fn charge_self_heal(
    throughput: &upload_policy::Throughput,
    cap: usize,
    file_path: &str,
) -> Result<(), String> {
    let n = throughput.note_self_heal();
    if n as usize > cap {
        return Err(format!(
            "deploy: {n} files have drifted from the sealed manifest in this publish (cap {cap}) \
             — likely something systemic (a stale build stage, a wrong directory) rather than an \
             isolated race; aborting instead of self-healing the rest (triggered by {file_path})"
        ));
    }
    Ok(())
}

/// The tail shared by both branches of [`upload_regular_file`] once a drift
/// has cleared the settle-and-corruption checks: charge it against the
/// deploy's cap, then log it. Not shared with `deploy::push`'s symlink
/// check, which has its own message shape — only the two branches here are
/// byte-for-byte identical past this point.
fn accept_self_heal(
    throughput: &upload_policy::Throughput,
    cap: usize,
    file_path: &str,
    mismatch: &str,
) -> Result<(), String> {
    charge_self_heal(throughput, cap, file_path)?;
    log::warn!(
        "[deploy] {mismatch} — stable after a {:?} settle pause, uploading the bytes actually \
         on disk instead",
        DRIFT_SETTLE_DELAY
    );
    Ok(())
}

/// Upload one regular file, choosing single-PUT or chunked by size.
///
/// This is the routing decision that was silently deleted by a refactor once
/// (`b1df2298a`) and cost liu-guo.com six 100 MB videos, and that
/// `deploy/prebuilt.rs` never had at all. One implementation, one place to pin
/// with a test.
///
/// `size` is the caller's already-stat'd file length; taking it as a parameter
/// avoids a second syscall on a path the caller just measured.
/// `throughput` is the deploy's shared link estimate. It decides the routing —
/// "does this file fit in one request at the speed we are actually getting" —
/// and every completed upload feeds it. Routing on a *fixed* size was the
/// 2026-08-04 bug: a 3.9 MB file at 25 KB/s sat just under the fixed 4 MiB
/// threshold, so it took the single-PUT path, which has no chunking, no
/// escalation and no resume, and timed out at 150 s on every attempt forever.
/// `self_heal_cap` is the deploy-wide budget from [`self_heal_cap`] (the
/// function) — every caller computes it once from its own file count and
/// passes the same value into every call this deploy makes.
///
/// Returns `Ok(Some(actual_hash))` when the file was shipped with a hash
/// different from `expected_hash` (a self-heal). The server has no
/// independent source of truth for these hashes — `commit_sync` sends
/// whatever the client asserts — so a caller that gets `Some` back MUST
/// correct its own manifest entry for `file_path` to
/// `content::file_entry(&actual_hash)` before it commits, or the server's
/// committed record for this generation is permanently wrong for a file that
/// was, in fact, uploaded correctly.
pub async fn upload_regular_file(
    client: &MossSetaClient,
    site_id: &str,
    file_path: &str,
    canonical: &Path,
    size: u64,
    generation_id: &str,
    expected_hash: &str,
    algo: HashAlgo,
    throughput: &upload_policy::Throughput,
    self_heal_cap: usize,
    on_bytes: Option<&(dyn Fn(u64) + Send + Sync)>,
) -> Result<Option<String>, String> {
    let mut healed_hash: Option<String> = None;
    if throughput.needs_chunking(size) {
        // Verify BEFORE uploading. The bytes are never buffered on this path,
        // so the check has to stream the file — which also means it costs one
        // local read of a file we are about to spend far longer sending.
        let first_hash = algo.hash_file(canonical)?;
        if let Err(e) = compare(file_path, expected_hash, &first_hash) {
            // On a live-edited, sync-backed vault (Google Drive) a
            // raw/background asset can legitimately change on disk between
            // the manifest being sealed and this upload running — a second
            // build racing the first, not corruption. But a single fresh
            // read cannot tell that apart from a torn read mid-write, which
            // looks identical. Settle, then re-hash: only a file reporting
            // the SAME hash on both reads has actually stopped changing.
            tokio::time::sleep(DRIFT_SETTLE_DELAY).await;
            let second_hash = algo.hash_file(canonical)?;
            if second_hash != first_hash {
                return Err(format!(
                    "{e} (still changing {:?} later — not self-healing a moving target)",
                    DRIFT_SETTLE_DELAY
                ));
            }
            // Stable is necessary but not sufficient: a zeroed iCloud
            // eviction stub is perfectly stable across two reads while still
            // being corruption, not content.
            let sample = read_leading_bytes(canonical, ZEROED_STUB_SAMPLE_BYTES).await?;
            if looks_like_zeroed_stub(&sample) {
                return Err(format!(
                    "{e} (looks like a zeroed stub, not a rebuilt file — refusing to self-heal)"
                ));
            }
            accept_self_heal(throughput, self_heal_cap, file_path, &e)?;
            healed_hash = Some(second_hash);
        }
        client
            .upload_file_chunked(
                site_id,
                file_path,
                canonical,
                size,
                generation_id,
                throughput,
                on_bytes,
            )
            .await
            .map_err(|e| format!("Failed to upload {}: {}", file_path, e))?;
    } else {
        // allow:raw_read built output being uploaded — dataless is absent (ADR-043)
        let mut body = tokio::fs::read(canonical)
            .await
            .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;
        if let Err(e) = verify_bytes(file_path, &body, expected_hash, algo) {
            // Same self-heal discipline as the chunked branch: settle, then
            // re-read from disk — `body` alone is only one sample and cannot
            // tell a finished rebuild from a torn read mid-write.
            tokio::time::sleep(DRIFT_SETTLE_DELAY).await;
            // Same `canonical` sealed-generation file as the first read above.
            // This is the re-read looks_like_zeroed_stub below checks before
            // trusting the drift as a real, finished rebuild.
            // allow:raw_read built output — dataless is absent (ADR-043)
            let resettled = tokio::fs::read(canonical)
                .await
                .map_err(|e| format!("Failed to re-read {}: {}", file_path, e))?;
            let first_hash = algo.hash_bytes(&body);
            let second_hash = algo.hash_bytes(&resettled);
            if second_hash != first_hash {
                return Err(format!(
                    "{e} (still changing {:?} later — not self-healing a moving target)",
                    DRIFT_SETTLE_DELAY
                ));
            }
            if looks_like_zeroed_stub(&resettled) {
                return Err(format!(
                    "{e} (looks like a zeroed stub, not a rebuilt file — refusing to self-heal)"
                ));
            }
            accept_self_heal(throughput, self_heal_cap, file_path, &e)?;
            body = resettled;
            healed_hash = Some(second_hash);
        }
        throughput.note_single_put_file();
        // Timed across `upload_file`'s whole retry loop rather than one attempt,
        // because that is all this path can see. It biases the estimate
        // *downward* after a failed attempt, which is the direction that helps:
        // the next file is routed as if the link were as slow as this file
        // actually experienced.
        let started = std::time::Instant::now();
        client
            .upload_file(site_id, file_path, body, generation_id, Some(throughput))
            .await
            .map_err(|e| format!("Failed to upload {}: {}", file_path, e))?;
        throughput.observe(size, started.elapsed());
        // The chunked branch reports per-chunk; this branch reports once.
        if let Some(cb) = on_bytes {
            cb(size);
        }
    }
    Ok(healed_hash)
}

/// A sliding window of in-flight upload tasks, admitting by bytes and by count.
///
/// Replaces `for chunk in need.chunks(20)`, which was a hard **barrier**: every
/// batch waited for its slowest member, so one file on a slow link stalled
/// nineteen that were already done. A window reuses a slot the moment it frees.
///
/// It is also the memory bound. The old shape could hold 20 concurrent uploads
/// of up to 20 MB each — 400 MB resident on a machine also running a webview
/// and a build. Admission is capped at `upload_policy::BYTE_BUDGET`.
///
/// Failure policy is unchanged and deliberate: the first error aborts every
/// in-flight task and fails the whole deploy. A partial upload is not a partial
/// publish — the server's generation stays unpromoted, and since 2026-08-03 the
/// staged files survive so the retry resumes rather than restarting.
pub struct UploadWindow {
    tasks: tokio::task::JoinSet<(u64, Result<(), String>)>,
    in_flight_bytes: u64,
    throughput: std::sync::Arc<upload_policy::Throughput>,
    /// When the window opened, which is when the upload phase began — the
    /// wall clock behind the summary line's achieved rate.
    started: std::time::Instant,
}

impl UploadWindow {
    pub fn new() -> Self {
        Self {
            tasks: tokio::task::JoinSet::new(),
            in_flight_bytes: 0,
            throughput: std::sync::Arc::new(upload_policy::Throughput::new()),
            started: std::time::Instant::now(),
        }
    }


    /// The link estimate every task admitted through this window shares.
    ///
    /// Owned here because a `UploadWindow` is created once per deploy, which is
    /// exactly the scope a bandwidth estimate is meaningful over. Deliberately
    /// **not** a global: one publish's link conditions have no business seeding
    /// the next one's, and a global would make the unit tests order-dependent.
    pub fn throughput(&self) -> std::sync::Arc<upload_policy::Throughput> {
        std::sync::Arc::clone(&self.throughput)
    }

    /// What one file costs this window's byte budget.
    ///
    /// A chunked upload streams: it seeks and reads one planned request at a
    /// time, so its resident cost is that request, not the file. Charging the
    /// whole file made every asset over `BYTE_BUDGET` fail `admits` against any
    /// other in-flight work and run alone through the `in_flight_count == 0`
    /// escape hatch — three videos on a GFW-boundary link uploaded strictly
    /// end-to-end and reached 71 KB/s aggregate where each alone had managed
    /// 77-84, so the concurrency the window exists to provide was worth less
    /// than nothing. It also starved the adaptive limit: with the window never
    /// holding more than one request, the goodput probe had no improvement to
    /// find and settled at 2.
    pub fn admission_cost(&self, size: u64) -> u64 {
        if self.throughput.needs_chunking(size) {
            self.throughput.plan_request_size() as u64
        } else {
            size
        }
    }

    /// Block until a task of `size` bytes may be admitted.
    ///
    /// Returns `Err` as soon as any already-spawned task has failed, with every
    /// remaining task aborted.
    pub async fn reserve(&mut self, size: u64) -> Result<(), String> {
        while !upload_policy::admits(
            self.tasks.len(),
            self.in_flight_bytes,
            size,
            self.throughput.effective_limit(),
        ) {
            self.harvest_one().await?;
        }
        Ok(())
    }

    /// Admit a task. Call [`Self::reserve`] first.
    pub fn spawn<F>(&mut self, size: u64, fut: F)
    where
        F: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        self.in_flight_bytes += size;
        // Mirror the task count into the shared Throughput: the tasks size
        // their requests by how many share the link (upload_policy), and they
        // cannot see this window.
        self.throughput.enter_flight();
        self.tasks.spawn(async move { (size, fut.await) });
    }

    /// Wait for every remaining task, failing on the first error.
    ///
    /// On success this also emits the one-line upload roll-up (#1133) — the
    /// window spans exactly the upload phase, and logging here means neither
    /// deploy loop can forget it.
    pub async fn drain(&mut self) -> Result<(), String> {
        while !self.tasks.is_empty() {
            self.harvest_one().await?;
        }
        log::info!("{}", self.throughput.deploy_summary(self.started.elapsed()));
        Ok(())
    }

    /// Await exactly one completion and release its bytes.
    ///
    /// On failure the window aborts everything still running: once the deploy
    /// is going to fail, continuing to spend a slow user's uplink is pure cost.
    async fn harvest_one(&mut self) -> Result<(), String> {
        let Some(joined) = self.tasks.join_next().await else {
            // Empty JoinSet — nothing to wait for. `admits` always returns true
            // at count 0, so `reserve` cannot spin here.
            return Ok(());
        };
        match joined {
            Ok((size, result)) => {
                self.in_flight_bytes = self.in_flight_bytes.saturating_sub(size);
                self.throughput.exit_flight();
                if let Err(e) = result {
                    self.tasks.abort_all();
                    self.in_flight_bytes = 0;
                    return Err(e);
                }
                Ok(())
            }
            Err(join_err) => {
                // A panicking task never returns its size, so the byte counter
                // would leak. We are failing the deploy anyway; reset it.
                self.tasks.abort_all();
                self.in_flight_bytes = 0;
                Err(format!("Upload task panicked: {}", join_err))
            }
        }
    }
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
