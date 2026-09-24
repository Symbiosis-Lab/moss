//! Cloud-file materialization awareness, shared by the video pipeline and the
//! home-page bounded wait (`pipeline::build_inner`).
//!
//! Files managed by a File-Provider-style sync client (iCloud Drive on
//! macOS, OneDrive/Dropbox Files-On-Demand on Windows) can be *dataless*
//! (placeholder; content evicted or not-yet-downloaded) or *partially
//! materialized* (mid-resync / mid-write) even when "Keep Downloaded" is
//! set — eviction is prevented but the re-sync download window is not. This
//! module gates readers on readiness so callers materialize first instead of
//! reporting a transient sync gap as a hang or a hard failure.
//!
//! Pure decision logic (`classify`, `next_step`) is unit-tested; `sample`
//! delegates to `crate::build::icloud::is_evicted` (the single canonical
//! eviction check — see that module for the per-platform detail), and
//! `request_download` hands the path to `crate::build::cloud_prefetch`, the
//! bounded pool that downloads by reading on a materialization-enabled
//! thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::build::icloud;

/// Whether a source file is ready to be read/probed/converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Fully materialized and quiescent — safe to read.
    Ready,
    /// Cloud dataless placeholder — content absent, needs download.
    NotMaterialized,
    /// Present but still changing (mid-download / mid-write) — wait.
    Settling,
}

/// A cheap stat sample used to detect quiescence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatSample {
    /// True when the file is a cloud dataless placeholder — see
    /// `crate::build::icloud::is_evicted` for the per-platform detection.
    pub dataless: bool,
    pub size: u64,
    pub mtime: i64,
}

/// Decide readiness from two consecutive stat samples. Pure — no I/O.
pub fn classify(prev: StatSample, curr: StatSample) -> Readiness {
    if curr.dataless {
        return Readiness::NotMaterialized;
    }
    if prev.size != curr.size || prev.mtime != curr.mtime {
        return Readiness::Settling;
    }
    Readiness::Ready
}

/// Outcome of a bounded settle wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    Ready,
    TimedOut,
    Cancelled,
}

/// What the driver loop should do after a fresh sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Done(Settled),
    /// Still dataless — re-issue the download request, then keep polling.
    Retrigger,
    /// Present but settling — keep polling.
    Keep,
}

/// One pure step of the settle loop: decide based on the latest sample pair and
/// how much time has elapsed against the deadline.
pub fn next_step(prev: StatSample, curr: StatSample, elapsed: Duration, deadline: Duration) -> Step {
    match classify(prev, curr) {
        Readiness::Ready => Step::Done(Settled::Ready),
        _ if elapsed >= deadline => Step::Done(Settled::TimedOut),
        Readiness::NotMaterialized => Step::Retrigger,
        Readiness::Settling => Step::Keep,
    }
}

/// Cheap single stat snapshot of the file's cloud/quiescence state.
///
/// `is_still_in_the_cloud`, not `is_evicted`: on macOS 12-13 an evicted file
/// has no file at the real path at all, only a hidden `.name.icloud` sibling.
/// `is_evicted` stats the real path and answers `false` there — so a waiter
/// would see a stable absent file, call it quiescent, and return `Ready` for a
/// file whose bytes are still in the cloud.
///
/// Reads size/mtime via `symlink_metadata` — never triggers a download.
pub fn sample(path: &Path) -> StatSample {
    let dataless = icloud::is_still_in_the_cloud(path);
    match std::fs::symlink_metadata(path) {
        Ok(meta) => StatSample {
            dataless,
            size: meta.len(),
            mtime: mtime_secs(&meta),
        },
        Err(_) => StatSample { dataless, size: 0, mtime: 0 },
    }
}

#[cfg(unix)]
fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.mtime()
}

#[cfg(not(unix))]
fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The process-wide download pool. One per process, because there is one
/// user waiting and one set of provider connections to be careful with —
/// see `build::cloud_prefetch`.
///
/// Lazily created so a CLI build that never meets an evicted file never starts
/// eight threads, and so tests can exercise the pool directly without this
/// global existing at all.
static PREFETCH: std::sync::LazyLock<crate::build::cloud_prefetch::Prefetcher> =
    std::sync::LazyLock::new(crate::build::cloud_prefetch::Prefetcher::new);

/// Hand a dataless file to the cloud readers, so the OS downloads it.
/// Non-blocking and safe to call from anywhere in the build, including per-file
/// inside a rayon fan-out.
///
/// # Why there are no rate limits, priorities or retries here
///
/// This used to damp its callers two ways — a 2s floor per path and a
/// 16-per-2s budget across paths — and later grew a priority queue and a retry
/// ledger on top. All of it was moss reimplementing a scheduler the provider
/// already has, and has real information for: link speed, quota, what the user
/// pinned, what it can batch. moss has guesses.
///
/// What is left is structural rather than statistical: a path already queued or
/// being read is not queued twice, and at most
/// [`cloud_prefetch::READERS`] reads are outstanding — a blast-radius bound,
/// not a throughput one. Ordering is the order moss was told, and retry is the
/// next sweep naming the file again.
///
/// [`cloud_prefetch::READERS`]: crate::build::cloud_prefetch::READERS
pub fn request_download(path: &Path) {
    PREFETCH.read(path);
}

/// Queue depth and in-flight count. Not the user-facing progress — the sweep
/// owns that — but the diagnosis that separates "the provider went quiet" from
/// "moss never asked."
pub fn download_snapshot() -> crate::build::cloud_prefetch::Snapshot {
    PREFETCH.snapshot()
}

/// Default bound on how long the background worker waits for one file to
/// materialize before giving up and deferring it to the next preview.
pub const MATERIALIZE_DEADLINE: Duration = Duration::from_secs(90);
/// Poll cadence while waiting.
pub const POLL_INTERVAL: Duration = Duration::from_millis(750);

/// Block the **calling thread** until `path` is materialized and quiescent,
/// or `deadline` elapses, or `cancel()` returns true. Callers must only use
/// this from a thread that's safe to block (a dedicated background worker,
/// or a `spawn_blocking` thread) — never the async/UI thread. `on_wait`
/// fires exactly once, the first time we actually have to wait — use it to
/// surface a "Downloading…" message.
///
/// Fast path: two back-to-back `sample`s with no sleep; a present,
/// unchanging file returns `Ready` immediately (≈two stats, microseconds).
pub fn await_ready(
    path: &Path,
    deadline: Duration,
    poll: Duration,
    cancel: &dyn Fn() -> bool,
    on_wait: &dyn Fn(),
) -> Settled {
    let mut prev = sample(path);
    let curr = sample(path); // back-to-back, no sleep
    if classify(prev, curr) == Readiness::Ready {
        return Settled::Ready;
    }
    if prev.dataless || curr.dataless {
        request_download(path);
    }
    on_wait();
    prev = curr;

    let start = Instant::now();
    loop {
        if cancel() {
            return Settled::Cancelled;
        }
        std::thread::sleep(poll);
        let curr = sample(path);
        match next_step(prev, curr, start.elapsed(), deadline) {
            Step::Done(s) => return s,
            Step::Retrigger => request_download(path),
            Step::Keep => {}
        }
        prev = curr;
    }
}

/// Bound on interactive (user-initiated) reads — editor open, etc. Long
/// enough to cover the observed ~7.7s coordinated-download floor latency
/// with margin; short enough that a genuinely wedged file doesn't leave a
/// spinner up indefinitely.
pub const INTERACTIVE_DEADLINE: Duration = Duration::from_secs(15);

/// Runs `attempt`; if it failed only because `path` is cloud-evicted, asks for
/// the file back and runs it once more.
///
/// This is the counterpart to the process-wide fail-fast policy
/// (`platform::macos::iopolicy::set_dataless_fail_fast`). Failing fast is what
/// makes recovery possible — the first attempt returns EDEADLK instead of
/// disappearing into `fileproviderd` — and this is where the recovery happens,
/// bounded by `deadline` and never longer, on the same mechanism the video
/// pipeline has run in production since 2026-06-08.
///
/// Any other error passes straight through: a missing file, a permissions
/// problem, or a corrupt read is not something a download will fix, and
/// waiting on one only delays the report. On timeout the ORIGINAL error is
/// returned, so callers see why the file was unavailable, not merely that a
/// retry also failed.
///
/// **One wait per file per [`WAIT_MEMO_TTL`].** The first attempt always runs;
/// only the *waiting* is suppressed. A build reads `.moss/config.toml` through
/// a dozen small accessors, and each one waiting the full deadline turned a
/// 0.8-second build into a 226-second one — the same "moss freezes" symptom
/// this design exists to remove, repeated on every rebuild for as long as the
/// file stayed in the cloud. Suppressing the wait rather than the read keeps
/// the answer honest: the caller still gets the real error, so "unreadable"
/// can never be mistaken for "absent".
pub fn retry_after_materialize<T>(
    path: &Path,
    deadline: Duration,
    attempt: impl Fn() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let first = match attempt() {
        Ok(value) => return Ok(value),
        // `is_offline_not_absent`, not `is_dataless_unavailable`: the latter is
        // EDEADLK-only, which is the Sonoma+ shape. On macOS 12-13 — still
        // supported, still the `minimumSystemVersion` — an evicted file returns
        // a genuine ENOENT on the real path, so an EDEADLK-only test sends every
        // one of those straight down the `return Err` arm. Nothing is requested,
        // nothing is waited for, and the caller reports "no such file" for a file
        // that exists. That made this entire recovery layer a no-op on two OS
        // versions.
        Err(e) if icloud::is_offline_not_absent(path, &e) => e,
        Err(e) => return Err(e),
    };
    if !wait_budget_allows(path) {
        log::debug!(
            "{} is still in the cloud and was already waited out — failing fast",
            path.display()
        );
        return Err(first);
    }
    match await_ready(path, deadline, POLL_INTERVAL, &|| false, &|| {}) {
        Settled::Ready => {
            forget_wait(path);
            attempt()
        }
        Settled::TimedOut | Settled::Cancelled => Err(first),
    }
}

/// How long a timed-out wait suppresses further waits on the same path.
///
/// Matches the supervisor's `BACKOFF_CAP`, its slowest re-ask interval: waiting
/// again sooner than the OS is being asked again cannot discover anything new.
const WAIT_MEMO_TTL: Duration = Duration::from_secs(60);

/// Paths whose bounded wait ran out, and when.
static WAITED_OUT: std::sync::LazyLock<Mutex<HashMap<PathBuf, Instant>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// May this path be waited on? Records the attempt when it may.
fn wait_budget_allows(path: &Path) -> bool {
    let now = Instant::now();
    let Ok(mut waited) = WAITED_OUT.lock() else {
        // Poisoned: wait. The failure mode of waiting is slowness; the failure
        // mode of not waiting is a file that never becomes readable.
        return true;
    };
    if let Some(&last) = waited.get(path) {
        if now.duration_since(last) < WAIT_MEMO_TTL {
            return false;
        }
    }
    // Entries past the TTL suppress nothing, so they are dead weight.
    if waited.len() > 4096 {
        waited.retain(|_, &mut t| now.duration_since(t) < WAIT_MEMO_TTL);
    }
    waited.insert(path.to_path_buf(), now);
    true
}

/// Clear the memo after a successful wait, so a file that is evicted a second
/// time is waited for again rather than inheriting the first eviction's budget.
fn forget_wait(path: &Path) {
    if let Ok(mut waited) = WAITED_OUT.lock() {
        waited.remove(path);
    }
}

/// Reads a vault file, waiting for a cloud-evicted source to come back.
pub fn read_with_materialize_wait(path: &Path, deadline: Duration) -> std::io::Result<Vec<u8>> {
    retry_after_materialize(path, deadline, || std::fs::read(path))
}

/// Bring a vault input's bytes down **before** the caller opens the file by
/// hand, and say whether waiting helped.
///
/// The read-side counterpart to [`read_with_materialize_wait`] for callers that
/// cannot express their read as one closure — an `OpenOptions` handle that is
/// locked and then migrated in place (`identity::keypair::Identity::load`), or
/// a pre-flight that wants the verdict *before* committing the user to a long
/// operation (`deploy::preflight_publish_inputs`). Same mechanism, same bounded
/// wait, same "unreadable is not absent" rule; only the shape differs.
///
/// `Ok(())` means the bytes are local — or that the path is not in the cloud at
/// all, which includes "genuinely missing". Deciding *that* is the caller's
/// read to make (via [`icloud::is_definitely_absent`]); this function only ever
/// answers the cloud question, so a missing file costs two lstats and returns
/// `Ok(())` rather than a misleading error.
///
/// `Err` is always `EDEADLK`-shaped: still in the cloud after `deadline`. The
/// honest reading is "waiting would help, but not within the time this caller
/// can spend" — never "this file is broken".
///
/// **Blocks the calling thread** for up to `deadline`, exactly like
/// [`await_ready`]. Call it from a background worker or a `spawn_blocking`
/// thread, never from the async runtime's own thread when the deadline is long.
pub fn materialize_input(path: &Path, deadline: Duration) -> std::io::Result<()> {
    if !icloud::is_still_in_the_cloud(path) {
        return Ok(());
    }
    request_download(path);
    match await_ready(path, deadline, POLL_INTERVAL, &|| false, &|| {}) {
        Settled::Ready => Ok(()),
        Settled::TimedOut | Settled::Cancelled => Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "{} is still being downloaded from the cloud",
                path.display()
            ),
        )),
    }
}

/// Read a **non-regenerable** vault input, keeping "absent" and "offline"
/// apart.
///
/// `Ok(None)` is reserved for a file [`icloud::is_definitely_absent`] can prove
/// is gone. Everything else — evicted, half-synced, permissions, bad bytes —
/// is `Err`, after one bounded materialize wait.
///
/// The distinction is load-bearing wherever the caller's *next* move is a
/// write. `.moss/data/redirects.json` is a site's whole rename history, and
/// the caller that reads it merges the result and writes it back: an unreadable
/// file read as "empty" does not degrade the build, it **erases the history**.
/// Same shape at `.moss/.gitignore` (an "empty" read would drop the user's own
/// added lines) and at `.moss/state.toml` (an "empty" read makes a deployed
/// site look undeployed). `read_optional_build_input`, the build's version of
/// this question, is deliberately the opposite trade — a theme file that cannot
/// be read is simply skipped this round — because nothing downstream of it
/// writes.
pub fn read_input_if_present(path: &Path) -> std::io::Result<Option<String>> {
    match read_to_string_with_materialize_wait(path, INTERACTIVE_DEADLINE) {
        Ok(text) => Ok(Some(text)),
        Err(e) if icloud::is_definitely_absent(path, &e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// What a no-wait read of a vault input found.
///
/// The *classification* lives here once; callers supply only policy. Before
/// this existed, "cheap stat, then read, then re-classify the error" was
/// written out three times — in `read_page_source`, in
/// `read_optional_build_input`, and again at each ad-hoc call site — and the
/// three copies drifted: one tested `is_evicted` first and one did not, which
/// is the difference between failing fast and blocking the render pass on a
/// platform with no fail-fast policy. Three spellings of one question is how a
/// fix lands in two of them.
#[derive(Debug)]
pub enum Found {
    /// The bytes are local, and here they are.
    Bytes(Vec<u8>),
    /// Present, but the bytes are still in the cloud. A download has been
    /// requested; the caller decides what to do in the meantime.
    InCloud,
}

/// Read a vault input, without waiting, and say which of the two answers it is.
///
/// `Err` is a real error — including a genuinely missing file. "Still in the
/// cloud" never arrives that way, which is the whole point: *unreadable is not
/// absent*, and a caller that writes the file back must never confuse them.
///
/// Two probes, not one, because they catch different things. The `lstat` runs
/// first: without the fail-fast policy (non-macOS, or an older macOS) the read
/// would BLOCK on an evicted file rather than erroring, and several callers sit
/// on the render pass. The error check runs after: the stat misses pre-Sonoma
/// `.name.icloud` stubs, and an eviction landing between the two calls.
///
/// This is the no-wait half of the module. The waiting half is
/// [`retry_after_materialize`]; every other read helper here is one of those
/// two plus a named policy.
pub fn probe_input(path: &Path) -> std::io::Result<Found> {
    if icloud::is_evicted(path) {
        request_download(path);
        return Ok(Found::InCloud);
    }
    match std::fs::read(path) {
        Ok(bytes) => Ok(Found::Bytes(bytes)),
        Err(e) if icloud::is_offline_not_absent(path, &e) => {
            request_download(path);
            Ok(Found::InCloud)
        }
        Err(e) => Err(e),
    }
}

/// [`probe_input`] for the callers that want text. `Ok(None)` is `InCloud`.
fn probe_input_to_string(path: &Path) -> std::io::Result<Option<String>> {
    match probe_input(path)? {
        Found::Bytes(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Found::InCloud => Ok(None),
    }
}

/// Read an *optional* text build input — a user theme file — treating "still in
/// the cloud" as "not there this time round". `what` names the file the way the
/// user would recognise it.
///
/// Neither of the other two answers is acceptable for a theme file. Waiting
/// blocks the render pass — which holds the stage lock — for the full
/// materialize deadline on every build until the download lands. Failing takes
/// the *whole site* down over a stylesheet: no pages, no home, nothing to look
/// at, because one CSS file is offline. Deferring costs the user one build's
/// worth of styling, and the theme directory is inside the watcher's `.moss/`
/// allowlist (`build::watch::should_watch_moss_file`), so the arrival rebuilds
/// the site with the theme applied.
///
/// Any error that is *not* a cloud eviction still fails the build: a theme file
/// that is present and unreadable is a real problem and silently shipping an
/// unstyled site would hide it.
pub fn read_optional_build_input(path: &Path, what: &str) -> Result<Option<String>, String> {
    match probe_input_to_string(path) {
        Ok(Some(text)) => Ok(Some(text)),
        Ok(None) => {
            // Record it. "Building without it" was previously said only to the
            // log, so the gate could not tell an unstyled build from a finished
            // one and showed the user a site with no stylesheet, calling it
            // ready. The ledger is how that reaches `cloud_gate_should_hold`.
            crate::build::cloud_ledger::note_unavailable(path);
            log::warn!(
                "{} is still in the cloud — building without it; the download will trigger a rebuild",
                what
            );
            Ok(None)
        }
        Err(e) => Err(format!("Failed to read {}: {}", what, e)),
    }
}

/// Read a page's source for the render pass, recording it in `deferred`
/// instead of dropping it when the bytes are merely in the cloud.
///
/// `None` means this build cannot render the page. Whether it lands in
/// `deferred` is load-bearing, not bookkeeping: `remove_stale_html` deletes
/// every staged page the build did not just re-emit, so a page moss could not
/// read looks exactly like a page the user deleted. An offline page must be
/// recorded, or its still-good HTML is deleted for being offline. A page that
/// is provably gone — or genuinely broken — is deliberately *not* recorded:
/// keeping stale HTML alive for a page that is never coming back is worse.
///
/// This does not wait. The render pass is a rayon fan-out over the whole
/// vault; a wait here would serialize it behind the slowest download. Waiting
/// is the supervisor's job, and the next build picks the page up.
pub fn read_page_source(
    path: &Path,
    deferred: &std::sync::Mutex<Vec<std::path::PathBuf>>,
) -> Option<String> {
    match probe_input_to_string(path) {
        Ok(Some(text)) => Some(text),
        Ok(None) => {
            deferred.lock().unwrap().push(path.to_path_buf());
            // And record it where the *publish* decision can see it. `deferred`
            // is a per-render-pass list read by `remove_stale_html` to protect
            // this page's existing HTML; it never reached the gate, so a vault
            // whose page sources were all still downloading rendered titles
            // from directory names and dates as `Unknown`, and moss published
            // that over the good site it already had.
            crate::build::cloud_ledger::note_unavailable(path);
            None
        }
        Err(e) => {
            // Logged because the previous `.ok()?` made every one of these
            // indistinguishable from a page that rendered fine.
            log::warn!("Skipping page {}: {}", path.display(), e);
            None
        }
    }
}

/// Raise the cloud gate for a build that stopped because the cloud had not
/// handed a file back.
///
/// The gate verdict is emitted from inside `build_inner`, just before the
/// `complete` progress event, because the verdict is the build's own output.
/// That means any `?` in between silently skips it: no `icloud-sync` event is
/// emitted at all, and the user gets a raw errno string on the onboarding
/// overlay where the waiting screen belongs. `pipeline::run` calls this on the
/// failure path so the verdict survives the `?`.
///
/// The caller has already established the deferral from
/// `BuildStopped::is_deferred`, which a `?` can never set. In the pipeline it
/// comes only from `outcome::io_stop`, which classifies against the path and
/// reserves the deferred answer for files the watcher would rebuild on — so the
/// screen this raises is one a later build can lower. (`BuildStopped::deferred`
/// is `pub`, so that reservation is a convention at any *future* call site, not
/// a compiler guarantee; what the compiler guarantees is that `?` cannot do it.)
///
/// **There is deliberately no `evicted_count > 0` precondition.** The verdict is
/// unforgeable, so a corroborating count buys nothing — and it would silently
/// swallow the gate in the case that matters most: `evicted_count` comes from
/// the source scan, which prunes dot-directories, so it does not count anything
/// the provider evicted *after* the scan ran. The counts are still passed
/// because the waiting screen shows progress, and they are floored at 1: a build
/// that stopped on a file still in the cloud has at least one file outstanding,
/// whatever the scan saw earlier.
pub fn raise_gate_for_a_deferred_build(
    folder_path: &str,
    evicted_count: usize,
    evicted_paths: &[std::path::PathBuf],
    reporter: &dyn crate::build::ports::reporter::BuildReporter,
    message: &str,
) {
    // The same monotonicity rule the build's own gate follows: a
    // sealed generation never becomes unsealed, so once one exists the preview
    // has something to serve and a full-window screen is never the right answer.
    // Two producers of one gate must not disagree about that — this one fires
    // when a build STOPPED, which is more severe than the other's "finished
    // without a home page", but severity is not the question the gate asks.
    // What the user gets instead is the last good site plus the download
    // counter in the titlebar, which is the honest partial state.
    let has_a_servable_generation = crate::infra::moss_paths::MossPaths::new(
        std::path::Path::new(folder_path),
    )
    .current_ptr()
    .exists();
    if has_a_servable_generation {
        log::info!(
            "cloud-sync: build STOPPED on a file still in the cloud, but a previous \
             generation is servable — not raising the waiting screen over it: {message}"
        );
        return;
    }

    mark_gated(folder_path);
    if !reporter.shell_listening() {
        return;
    }
    let remaining = evicted_paths
        .iter()
        .filter(|p| icloud::is_still_in_the_cloud(p))
        .count()
        .max(1);
    let evicted_count = evicted_count.max(remaining);
    log::info!(
        "cloud-sync: build STOPPED on a file still in the cloud ({remaining} of {evicted_count} \
         remaining) — raising the waiting screen instead of reporting: {message}"
    );
    let provider = crate::build::cloud_provider::detect_from_path(std::path::Path::new(folder_path));
    reporter.cloud_sync(&crate::build::ports::reporter::CloudSync {
        folder: folder_path,
        phase: "home_waiting",
        provider,
        total: evicted_count,
        remaining,
        // The build does not compute the blocking subset; its own gate
        // verdict is the answer to that question.
        blocking: None,
        unavailable: &[],
    });
}

/// Folders whose last build raised the cloud gate and has not lowered it.
///
/// A `Vec` because at most a couple of folders are open at once, and because
/// `Vec::new()` is const so this needs no lazy init.
static GATED_FOLDERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Record that `folder`'s build ended without a home page it could serve.
pub fn mark_gated(folder: &str) {
    let mut gated = GATED_FOLDERS.lock().unwrap_or_else(|e| e.into_inner());
    if !gated.iter().any(|f| f == folder) {
        gated.push(folder.to_string());
    }
}

/// Clear `folder`'s gate, reporting whether it was up.
///
/// This is what makes the gate liftable. The build only knows to speak about
/// the cloud when its own scan found something evicted — and the build that
/// finally succeeds is, by construction, usually the one where nothing is
/// evicted any more. Keyed off this instead, the successful build knows there
/// is a screen waiting to hear from it. This is the *only* way down: the screen
/// carries no dismiss control, so without this the user sits in front of
/// "waiting for files to download" over a finished, served site until they
/// re-open the folder.
pub fn take_gate(folder: &str) -> bool {
    let mut gated = GATED_FOLDERS.lock().unwrap_or_else(|e| e.into_inner());
    let was = gated.iter().any(|f| f == folder);
    gated.retain(|f| f != folder);
    was
}

/// String-reading twin of [`read_with_materialize_wait`], for callers that
/// want `String` (front matter parsing, config files) instead of raw bytes.
pub fn read_to_string_with_materialize_wait(
    path: &Path,
    deadline: Duration,
) -> std::io::Result<String> {
    let bytes = read_with_materialize_wait(path, deadline)?;
    String::from_utf8(bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(dataless: bool, size: u64, mtime: i64) -> StatSample {
        StatSample { dataless, size, mtime }
    }

    /// The classifier every no-wait reader now shares. A file that is simply
    /// missing must come back as `Err`, never as `InCloud`: the callers that
    /// write their file back (`redirects.json`, `state.toml`) decide between
    /// merge and erase on exactly this distinction.
    #[test]
    fn the_probe_separates_readable_from_missing() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("here.txt");
        std::fs::write(&present, b"hello").unwrap();

        match probe_input(&present) {
            Ok(Found::Bytes(bytes)) => assert_eq!(bytes, b"hello"),
            other => panic!("a local file must read as Bytes, got {other:?}"),
        }

        let missing = dir.path().join("gone.txt");
        let err = probe_input(&missing).expect_err("a missing file is an error, not InCloud");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            probe_input_to_string(&missing).is_err(),
            "and the text form must agree — it is the one the writers call"
        );
    }

    /// The latch is what a later successful build reads to know there is a
    /// waiting screen to lift; raising it with no shell listening (CLI builds,
    /// tests) must still record it.
    #[test]
    fn a_deferred_build_latches_the_gate_even_with_no_shell() {
        let folder = "/tmp/moss-deferred-latch-probe";
        take_gate(folder);
        raise_gate_for_a_deferred_build(
            folder,
            3,
            &[],
            crate::build::ports::reporter::discarding(),
            "Failed to read posts/hello.md: Resource deadlock avoided (os error 11)",
        );
        assert!(take_gate(folder), "a deferred build must latch the gate");
        assert!(!take_gate(folder), "and taking it must clear it");
    }

    /// The scan prunes dot-directories and runs before the build, so a file the
    /// provider evicted afterwards contributes nothing to `evicted_count`. An
    /// `evicted_count > 0` precondition here would swallow the gate in exactly
    /// that case — which is most of them.
    #[test]
    fn a_deferred_build_is_gated_even_when_the_scan_counted_no_evictions() {
        let folder = "/tmp/moss-post-scan-eviction-probe";
        take_gate(folder);
        raise_gate_for_a_deferred_build(folder, 0, &[], crate::build::ports::reporter::discarding(), "evicted after the scan ran");
        assert!(take_gate(folder), "a zero scan count must not silence the verdict");
    }

    #[test]
    fn dataless_is_not_materialized() {
        // Only `curr.dataless` gates this branch; pass a non-dataless `prev` so
        // the test would catch an accidental read of `prev.dataless`.
        assert_eq!(classify(s(false, 100, 1), s(true, 100, 1)), Readiness::NotMaterialized);
    }

    #[test]
    fn growing_or_touched_is_settling() {
        assert_eq!(classify(s(false, 100, 1), s(false, 200, 1)), Readiness::Settling); // size grew
        assert_eq!(classify(s(false, 100, 1), s(false, 100, 2)), Readiness::Settling); // mtime moved
    }

    #[test]
    fn quiescent_present_file_is_ready() {
        assert_eq!(classify(s(false, 100, 1), s(false, 100, 1)), Readiness::Ready);
    }

    #[test]
    fn freshly_materialized_then_stable_is_ready() {
        // was dataless, now present and unchanged across the pair
        assert_eq!(classify(s(false, 500, 9), s(false, 500, 9)), Readiness::Ready);
    }

    #[test]
    fn ready_pair_finishes_even_before_deadline() {
        let step = next_step(
            s(false, 1, 1),
            s(false, 1, 1),
            std::time::Duration::from_secs(0),
            std::time::Duration::from_secs(90),
        );
        assert_eq!(step, Step::Done(Settled::Ready));
    }

    #[test]
    fn deadline_exceeded_while_unready_times_out() {
        let step = next_step(
            s(true, 1, 1),
            s(true, 1, 1),
            std::time::Duration::from_secs(91),
            std::time::Duration::from_secs(90),
        );
        assert_eq!(step, Step::Done(Settled::TimedOut));
    }

    #[test]
    fn dataless_before_deadline_retriggers() {
        let step = next_step(
            s(true, 1, 1),
            s(true, 1, 1),
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(90),
        );
        assert_eq!(step, Step::Retrigger);
    }

    #[test]
    fn settling_before_deadline_keeps_polling() {
        let step = next_step(
            s(false, 1, 1),
            s(false, 2, 1),
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(90),
        );
        assert_eq!(step, Step::Keep);
    }

    #[test]
    fn sample_of_real_local_file_is_not_dataless_and_classifies_ready() {
        // A freshly written local file is never dataless and is quiescent.
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let tmp = std::path::PathBuf::from(dir).join("../../target/test-tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("cloud_readiness_sample_probe.bin");
        std::fs::write(&f, b"hello moov").unwrap();

        let a = sample(&f);
        let b = sample(&f);
        assert!(!a.dataless, "local file must not be dataless");
        assert_eq!(a.size, 10);
        assert_eq!(classify(a, b), Readiness::Ready);

        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn await_ready_fast_path_on_quiescent_file() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let tmp = std::path::PathBuf::from(dir).join("../../target/test-tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("cloud_readiness_await_probe.bin");
        std::fs::write(&f, b"ready").unwrap();

        let waited = std::cell::Cell::new(false);
        let res = await_ready(
            &f,
            std::time::Duration::from_secs(5),
            std::time::Duration::from_millis(50),
            &|| false,
            &|| waited.set(true),
        );
        assert_eq!(res, Settled::Ready);
        assert!(!waited.get(), "quiescent local file must not trigger a wait");

        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn await_ready_does_not_panic_on_missing_path() {
        // A non-existent path samples as size 0 / not-dataless; the back-to-back
        // pair is equal → classify Ready → returns immediately. The point of this
        // test is that await_ready never panics on a missing path and respects an
        // immediate cancel without hanging.
        let missing = std::path::Path::new("/definitely/not/here/cloud_readiness_x.bin");
        let res = await_ready(
            missing,
            std::time::Duration::from_secs(60),
            std::time::Duration::from_millis(10),
            &|| true,
            &|| {},
        );
        assert!(matches!(res, Settled::Ready | Settled::Cancelled));
    }

    #[test]
    fn read_with_materialize_wait_passes_through_local_file() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let tmp = std::path::PathBuf::from(dir).join("../../target/test-tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("read_with_materialize_wait_local.bin");
        std::fs::write(&f, b"hello").unwrap();

        let bytes = read_with_materialize_wait(&f, Duration::from_secs(5)).unwrap();
        assert_eq!(bytes, b"hello");

        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn read_with_materialize_wait_returns_immediately_on_missing_file() {
        // ENOENT is not EDEADLK — must not enter the materialize-and-poll
        // path at all, and must return promptly.
        let missing = std::path::Path::new("/definitely/not/here/read_with_materialize_wait.bin");
        let start = Instant::now();
        let result = read_with_materialize_wait(missing, Duration::from_secs(30));
        assert!(result.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "a plain ENOENT must not wait for the deadline"
        );
    }

    #[test]
    fn read_to_string_with_materialize_wait_passes_through_local_file() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let tmp = std::path::PathBuf::from(dir).join("../../target/test-tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("read_to_string_with_materialize_wait_local.txt");
        std::fs::write(&f, "hello").unwrap();

        let text = read_to_string_with_materialize_wait(&f, Duration::from_secs(5)).unwrap();
        assert_eq!(text, "hello");

        let _ = std::fs::remove_file(&f);
    }

    /// The three outcomes `read_page_source` must keep apart, because
    /// `remove_stale_html` acts on the difference: a readable page renders, an
    /// offline page is recorded so its HTML survives, and a page that is
    /// provably gone is dropped so its HTML gets cleaned up.
    #[test]
    fn a_page_source_read_separates_offline_from_gone() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let tmp = std::path::PathBuf::from(dir).join("../target/test-tmp/read_page_source");
        std::fs::create_dir_all(&tmp).unwrap();
        let deferred = std::sync::Mutex::new(Vec::new());

        let present = tmp.join("present.md");
        std::fs::write(&present, "# Hi").unwrap();
        assert_eq!(read_page_source(&present, &deferred).as_deref(), Some("# Hi"));
        assert!(deferred.lock().unwrap().is_empty(), "a readable page is not deferred");

        let gone = tmp.join("gone.md");
        let _ = std::fs::remove_file(&gone);
        assert_eq!(read_page_source(&gone, &deferred), None);
        assert!(
            deferred.lock().unwrap().is_empty(),
            "a page that is provably gone must NOT be deferred, or its stale HTML lives forever"
        );

        // Pre-Sonoma eviction: the real name is gone, a placeholder stands in.
        #[cfg(target_os = "macos")]
        {
            let evicted = tmp.join("evicted.md");
            let _ = std::fs::remove_file(&evicted);
            std::fs::write(tmp.join(".evicted.md.icloud"), b"").unwrap();
            assert_eq!(read_page_source(&evicted, &deferred), None);
            assert_eq!(
                deferred.lock().unwrap().as_slice(),
                &[evicted],
                "an offline page must be deferred so its HTML is not deleted"
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
