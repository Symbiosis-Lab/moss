//! Phase tracer for the build pipeline.
//!
//! ## Why
//!
//! When the build pipeline silently stalls (hung HTTP call, deadlock, slow
//! disk), the operator's only signal is "no log output for several minutes".
//! Distinguishing "still running phase X" from "exited phase X 30s ago and
//! hanging in unknown territory" used to require code reading.
//!
//! Wrapping each phase in `let _p = PhaseTrace::start("scan");` makes the
//! enter/exit boundaries unambiguous in the log:
//!
//! ```text
//! [phase] scan started
//! [phase] scan completed in 312ms
//! [phase] process_hooks started
//! [phase] process_hooks completed in 13.4s
//! [phase] native_process_spawn started
//! [phase] native_process_spawn completed in 1ms      ← non-blocking spawn
//! ```
//!
//! ## Use
//!
//! ```ignore
//! {
//!     let _p = PhaseTrace::start("phase_name");
//!     // ... work ...
//! } // _p drops here, logs completion + elapsed
//! ```
//!
//! Or with the macro form to avoid forgetting the `let _p =` pattern:
//!
//! ```ignore
//! phase!("phase_name", {
//!     // work
//! });
//! ```
//!
//! ## Design choices
//!
//! - Logged at DEBUG (demoted from INFO 2026-09-15):
//!   a real upload showed these start/completed pairs as noise once a build ran
//!   many times in a session, and the information they carry — which phase,
//!   how long — now survives in the one `build.summary` INFO line `run_pipeline`
//!   logs per build (see `record`/`log_build_summary` below).
//! - Logged on `Drop` so an early `?` return still records the partial elapsed.
//! - `target: "phase"` so users can filter (`MOSS_LOG_LEVEL=debug` shows
//!   these; future: per-target level filter).
//! - Per-phase budgets: a phase that overruns its budget additionally
//!   logs INFO at 1× and again at 2× the budget, worded more insistently the
//!   second time. Both tiers stay at INFO, never WARN or ERROR: a WARN line
//!   is a problem the author can fix and fails `--strict`, and machine speed
//!   is not one (the default CLI log level hides INFO; `MOSS_LOG_LEVEL=info`
//!   shows it). The budgets are
//!   initial guesses meant to be tuned as real timing data accrues (see
//!   `PHASE_BUDGETS_MS` below), so an overrun is a performance note, not a
//!   build failure, and it must never read as one — to a person scanning the
//!   log, or to a tool that treats a WARN or ERROR line as something to fix.
//!   It is still a real signal, not the noise the started/completed pair was,
//!   which is why it is logged at INFO rather than DEBUG. Observability ONLY — no panic, no changed
//!   return value, no bump to the CLI's problem count, and explicitly NO CI
//!   wall-clock gate (shared-runner wall-clock tests are flake).
//!
//! ## The `build.summary` collector
//!
//! Every `PhaseTrace` also tries to record its `(name, elapsed)` into
//! whichever build-scoped collector is active, so `run_pipeline`
//! (`crates/moss-build/src/build.rs`) can log one `build.summary` INFO line
//! per build instead of the old per-phase INFO pairs. Two carriers exist
//! because `run_pipeline` is async but part of the pipeline
//! (`pipeline::run`) executes inside a `tokio::task::spawn_blocking`
//! closure, which runs on an untracked OS thread and does NOT inherit a
//! task-local set on the calling task:
//!
//! - [`ASYNC_PHASE_COLLECTOR`] — a `tokio::task_local!`, set once by
//!   [`with_collector`] around `run_pipeline`'s whole body. Correct across
//!   the `.await` points between phases even when the tokio runtime resumes
//!   the task on a different worker thread, which a plain thread-local would
//!   not survive.
//! - [`BLOCKING_PHASE_COLLECTOR`] — a plain `thread_local!`, bridged in
//!   explicitly via [`enter_blocking_scope`] right where `build.rs` enters
//!   its `spawn_blocking` closure. Sound there specifically because that
//!   closure is synchronous top to bottom (the whole point of
//!   `spawn_blocking`) — no `.await` inside it that could hop threads.
//!
//! A phase whose `PhaseTrace` runs somewhere neither carrier reaches (for
//! example inside a `rayon` worker thread spawned deeper in the render loop)
//! is simply absent from the summary line rather than mis-attributed to it —
//! its DEBUG started/completed pair is still there for that level of detail.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Per-phase build-time budgets in milliseconds.
///
/// A phase elapsed >= budget logs INFO; >= 2x budget logs INFO, worded harder. Phases not
/// listed here are unbudgeted (no threshold logging) — add a row when adding
/// a new `PhaseTrace::start(...)` call site.
///
/// These are INITIAL values to tune as real-world timing data accrues.
/// Seeds: the observed timings in this module's doc examples
/// (scan ~312ms, process_hooks ~13.4s, native_process_spawn ~1ms) plus
/// generous headroom for network-bound phases (link-meta prewarm, plugin
/// process-hook syncs such as the Matters import legitimately run tens of
/// seconds).
const PHASE_BUDGETS_MS: &[(&str, u64)] = &[
    // Whole-pipeline envelope; every sub-phase below nests inside it.
    ("run_pipeline", 120_000),
    // Network-bound: parallel HTTP fetches of link-preview metadata.
    ("link_meta_prewarm", 30_000),
    // Filesystem walk + metadata extraction. Observed ~312ms on a typical site.
    ("scan", 2_000),
    // Blocking await of plugin process hooks (content download/import).
    // Observed ~13.4s for a Matters sync; generous — the pathology this
    // catches is the multi-minute hang class, not a slow-but-working sync.
    ("process_hooks_await", 60_000),
    // Same walk as "scan", re-run after content-producing hooks.
    ("scan_after_process_hooks", 2_000),
    // Non-blocking spawn — observed ~1ms. Anything slower means the spawn
    // path grew blocking work, which violates the minimal-blocking principle.
    ("native_process_spawn", 100),
    // Render + encode of the whole site.
    ("build", 30_000),
    ("media_prelude", 5_000),
    ("video_handling", 30_000),
    ("background_asset_copy", 10_000),
    ("stage_source_html", 10_000),
    ("search_index", 10_000),
    ("seal_wait", 10_000),
    ("seal_persist", 10_000),
    ("seal_promote", 10_000),
    // Waiting for another build's hold on the folder lock or the stage-write
    // guard. `seal_promote` starts after its waits; `build` and `seal_persist`
    // still contain theirs, so subtract `lock_wait` to read their work. The
    // lock itself costs well under a millisecond, so the budget is a tripwire.
    ("lock_wait", 200),
    // Native + plugin slot collection/injection before finalization.
    ("slot_resolution", 10_000),
    // Loop A: parse + resolve every markdown file (blocking.rs). Dominant
    // render-phase cost on image/link-heavy vaults — measured ~16-18s on a
    // 216-page reference vault; budget set loosely above
    // that until real-world timing data accrues across vault sizes.
    ("render_markdown", 60_000),
    // Loop B: HTML emission for every page (blocking.rs). Measured ~5-7s on
    // the same 216-page vault.
    ("render_html_pages", 20_000),
    ("render_homepage", 2_000),
    // Background sharing-card generation; diagnostic budget, not a preview deadline.
    ("render_sharing_cards", 20_000),
    // Shadow mode: facade hashing + DepGraph build + cache diff over
    // all documents. In-memory hashing/diffing only (no I/O beyond a small
    // JSON cache load/save), so this should stay far under render_html_pages
    // even at large vault sizes; budget is a generous placeholder pending
    // real measurement.
    ("shadow_facade_diff", 10_000),
];

/// Budget classification for a completed (or aborted) phase.
///
/// Both non-`Ok` tiers log at INFO (see `budget_log` below) — the name
/// `Severe` is a classification, not a log level, so a future match on this
/// enum can't reach for `log::error!` by reading the variant name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetClass {
    /// Under budget, or the phase has no budget row.
    Ok,
    /// Elapsed >= 1x budget.
    Warn,
    /// Elapsed >= 2x budget.
    Severe,
}

/// Look up a phase's budget. `None` for unbudgeted phases.
fn budget_ms(phase: &str) -> Option<u64> {
    PHASE_BUDGETS_MS
        .iter()
        .find(|(name, _)| *name == phase)
        .map(|&(_, ms)| ms)
}

/// Classify an elapsed duration against the phase's budget.
///
/// Pure function so the thresholds are unit-testable without log capture.
/// WARN at budget, Severe at 2x budget; unbudgeted phases are always `Ok`.
pub fn budget_class(phase: &str, elapsed: Duration) -> BudgetClass {
    let Some(budget) = budget_ms(phase) else {
        return BudgetClass::Ok;
    };
    let elapsed_ms = elapsed.as_millis();
    if elapsed_ms >= u128::from(budget) * 2 {
        BudgetClass::Severe
    } else if elapsed_ms >= u128::from(budget) {
        BudgetClass::Warn
    } else {
        BudgetClass::Ok
    }
}

/// The budget-overrun log line for a completed phase, plus the level it logs
/// at — `None` when the phase was within budget.
///
/// Pure, like [`budget_class`], so the level and wording are unit-testable
/// without a log-capture harness (none exists in this crate). Both overrun
/// tiers log at `Info`: the thresholds in `PHASE_BUDGETS_MS` are initial
/// guesses awaiting real timing data, so an overrun is a performance note,
/// not a failure, and must never print as a `[WARN]` or `[ERROR]` line — that reads as
/// "the build failed" to a person scanning the log and to a coding agent
/// that treats an ERROR line as something to fix, even though the build
/// keeps going, returns success, and never counts this toward the CLI's
/// `--strict` problem count.
pub(crate) fn budget_log(phase: &'static str, elapsed: Duration) -> Option<(log::Level, String)> {
    let budget = budget_ms(phase).unwrap_or_default();
    match budget_class(phase, elapsed) {
        BudgetClass::Ok => None,
        BudgetClass::Warn => Some((
            log::Level::Info,
            format!(
                "{phase} exceeded its {budget}ms budget ({elapsed:?}) — performance note, not \
                 a failure; initial budget, tune as timing data accrues"
            ),
        )),
        BudgetClass::Severe => Some((
            log::Level::Info,
            format!(
                "{phase} took {elapsed:?}, well past its {budget}ms budget — performance note, \
                 not a failure; initial budget, tune as timing data accrues"
            ),
        )),
    }
}

/// One build's accumulated phase timings and counts, behind a lock so it can
/// be shared into a `spawn_blocking` closure and read back after `.await`.
#[derive(Debug, serde::Serialize)]
pub struct PhaseSample {
    pub pass: usize,
    pub name: &'static str,
    pub ms: u128,
}

#[derive(Debug)]
pub struct BuildTelemetry {
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the first collector opened inside this one started.
    pub nested_started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub passes: usize,
    pub phases: Vec<PhaseSample>,
    pub counts: Vec<(&'static str, usize)>,
}

impl Default for BuildTelemetry {
    fn default() -> Self {
        Self { started_at: chrono::Utc::now(), nested_started_at: None, passes: 0, phases: Vec::new(), counts: Vec::new() }
    }
}

pub fn begin_pass() {
    if let Some(c) = current_collector() {
        if let Ok(mut t) = c.lock() { t.passes += 1; }
    }
}

pub type PhaseCollector = Arc<Mutex<BuildTelemetry>>;

tokio::task_local! {
    /// Set once, for the duration of one `run_pipeline` call, by
    /// [`with_collector`]. See the module-level "`build.summary` collector"
    /// doc for why this is a task-local rather than a thread-local.
    static ASYNC_PHASE_COLLECTOR: PhaseCollector;
}

thread_local! {
    /// Bridges a collector into a `spawn_blocking` closure's OS thread; see
    /// [`enter_blocking_scope`].
    static BLOCKING_PHASE_COLLECTOR: RefCell<Option<PhaseCollector>> = const { RefCell::new(None) };
}

/// RAII guard restoring the previous blocking-scope collector on drop, so
/// nested `enter_blocking_scope` calls (there are none today, but a future
/// caller nesting one blocking call inside another must not need to know
/// that) compose safely instead of leaking the outer scope's collector.
pub struct BlockingScopeGuard {
    previous: Option<PhaseCollector>,
}

impl Drop for BlockingScopeGuard {
    fn drop(&mut self) {
        BLOCKING_PHASE_COLLECTOR.with(|c| *c.borrow_mut() = self.previous.take());
    }
}

/// Bridge `collector` (typically [`current_collector`]'s result, captured
/// before entering a `spawn_blocking` closure) onto the current OS thread for
/// the life of the returned guard. Call this as the very first line inside
/// the blocking closure.
pub fn enter_blocking_scope(collector: Option<PhaseCollector>) -> BlockingScopeGuard {
    let previous = BLOCKING_PHASE_COLLECTOR.with(|c| c.replace(collector));
    BlockingScopeGuard { previous }
}

/// Snapshot the active async-task collector, if any, so it can be handed to
/// [`enter_blocking_scope`] before a `spawn_blocking` call. `None` outside a
/// [`with_collector`] scope (tests, CLI paths that call phase-guarded code
/// directly) — every caller here treats that as "nothing to record", never
/// an error.
pub fn current_collector() -> Option<PhaseCollector> {
    ASYNC_PHASE_COLLECTOR.try_with(|c| c.clone()).ok()
        .or_else(|| BLOCKING_PHASE_COLLECTOR.with(|c| c.borrow().clone()))
}

/// Capture before spawning: neither a new task nor its blocking thread inherits carriers.
pub(crate) fn inherit_blocking<F, T>(body: F) -> impl FnOnce() -> T + Send + 'static
where
    F: FnOnce() -> T + Send + 'static,
{
    let collector = current_collector();
    move || {
        let _scope = enter_blocking_scope(collector);
        body()
    }
}

/// Run `body` with a fresh collector active, then hand the collector back
/// alongside the body's result so the caller can log a `build.summary` line
/// from it. The one caller is `build::run_pipeline`.
pub async fn with_collector<F, T>(body: F) -> (T, PhaseCollector)
where
    F: std::future::Future<Output = T>,
{
    let telemetry = BuildTelemetry::default();
    // An enclosing deploy links its record to the build it ran by this time.
    if let Some(outer) = current_collector() {
        if let Ok(mut t) = outer.lock() { t.nested_started_at.get_or_insert(telemetry.started_at); }
    }
    let collector: PhaseCollector = Arc::new(Mutex::new(telemetry));
    let result = ASYNC_PHASE_COLLECTOR.scope(collector.clone(), body).await;
    (result, collector)
}

/// Record a phase's elapsed time into whichever collector (async-task or
/// blocking-thread) is active. A no-op outside any collector scope.
fn record_phase(collector: &Option<PhaseCollector>, pass: usize, name: &'static str, elapsed: Duration) {
    if let Some(c) = collector {
        if let Ok(mut t) = c.lock() {
            t.phases.push(PhaseSample { pass, name, ms: elapsed.as_millis() });
        }
    }
}

/// Counts are additive across workers and passes, just like the work they measure.
pub fn record_count(name: &'static str, value: usize) {
    store_count(name, value, |count, value| *count += value);
}

/// A fact about the build rather than work done (a flag, a sequence number, a corpus
/// size): the last pass's value stands.
pub fn record_value(name: &'static str, value: usize) {
    store_count(name, value, |count, value| *count = value);
}

fn store_count(name: &'static str, value: usize, merge: fn(&mut usize, usize)) {
    if let Some(c) = current_collector() {
        if let Ok(mut t) = c.lock() {
            match t.counts.iter_mut().find(|(key, _)| *key == name) {
                Some((_, count)) => merge(count, value),
                None => t.counts.push((name, value)),
            }
        }
    }
}

/// Pure formatter for the `build.summary` line — factored out of
/// [`log_build_summary`] so the shape is testable without a log-capture
/// harness (none exists in this crate; see `phase.rs`'s own tests below).
pub fn format_build_summary(
    phases: &[(&'static str, Duration)],
    counts: &[(&'static str, usize)],
    total: Duration,
    outcome: &str,
    trigger: &crate::build::BuildTrigger,
) -> String {
    use crate::build::BuildTrigger;
    // `changed` counts the watcher's paths for this build, after its filter; a
    // path the OS reported twice counts twice, so it is a size hint.
    let trigger = match trigger {
        BuildTrigger::Full => "trigger=full".to_string(),
        BuildTrigger::Structural(paths) => format!("trigger=structural changed={}", paths.len()),
        BuildTrigger::ContentOnly(paths) => format!("trigger=content changed={}", paths.len()),
    };
    let phases_str = phases
        .iter()
        .map(|(name, d)| format!("{name}={}ms", d.as_millis()))
        .collect::<Vec<_>>()
        .join(" ");
    let counts_str = counts
        .iter()
        .map(|(name, v)| format!("{name}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut line = format!("build.summary outcome={outcome} {trigger} total={}ms", total.as_millis());
    if !counts_str.is_empty() {
        line.push(' ');
        line.push_str(&counts_str);
    }
    if !phases_str.is_empty() {
        line.push(' ');
        line.push_str(&phases_str);
    }
    line
}

/// Log the single `build.summary` INFO line for one `run_pipeline` call:
/// every phase and count the collector saw, plus the caller-supplied total
/// wall time and outcome. Replaces the old per-phase started/completed INFO
/// pairs — see the module doc.
pub fn log_build_summary(
    collector: &PhaseCollector,
    total: Duration,
    outcome: &str,
    trigger: &crate::build::BuildTrigger,
) {
    let telemetry = collector.lock().map(|t| (t.phases.iter().map(|p| (p.name, Duration::from_millis(p.ms as u64))).collect::<Vec<_>>(), t.counts.clone()));
    let (phases, counts) = telemetry.unwrap_or_default();
    log::info!(target: "build", "{}", format_build_summary(&phases, &counts, total, outcome, trigger));
}

/// Counts every build record carries, so a build that did none of the work says 0
/// instead of dropping the key.
const BUILD_COUNTS: &[&str] = &[
    "pages_rendered", "pages_carried", "slot_html_scanned", "slot_html_rewritten",
    "background_assets_copied", "background_assets_hits", "background_assets_misses",
    "background_assets_linked", "background_assets_stat_only", "sized_raster_blob_checks",
    "images_unchanged", "images_dispatched", "image_variants_stat_only", "image_variants_blob_checks",
    "image_transform_records_loaded", "files_sealed", "hash_index_hits_process", "hash_index_rehashed_process",
];
/// The video counts, which also make up a detached encode's own record.
const VIDEO_COUNTS: &[&str] = &["videos_cache_hit", "video_blobs_validated", "videos_converted", "videos_detached"];

/// A profile record of `kind`, build or deploy: the fields every record opens
/// with, then `fields`.
pub(crate) fn profile_record_of(kind: &str, fields: serde_json::Value) -> serde_json::Value {
    let mut record = serde_json::json!({
        "schema": 1, "kind": kind, "moss_version": crate::system::app_version(), "build_profile": crate::system::build_profile(),
    });
    if let (Some(record), serde_json::Value::Object(fields)) = (record.as_object_mut(), fields) {
        record.extend(fields);
    }
    record
}

fn profile_record(t: &BuildTelemetry, kind: &str, keys: &[&[&'static str]], total: Duration, outcome: &str) -> serde_json::Value {
    let mut counts: std::collections::BTreeMap<_, _> = keys.iter().flat_map(|k| k.iter()).map(|&k| (k, 0)).collect();
    counts.extend(t.counts.iter().copied());
    let mut record = profile_record_of(kind, serde_json::json!({
        "started_at": t.started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "total_ms": total.as_millis(), "outcome": outcome,
        "passes": t.passes, "phases": t.phases, "counts": counts,
    }));
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
            let rss = unsafe { usage.assume_init() }.ru_maxrss as f64;
            let bytes_per_unit = if cfg!(target_os = "macos") { 1.0 } else { 1024.0 };
            record["peak_rss_mb"] = serde_json::json!(rss * bytes_per_unit / (1024.0 * 1024.0));
        }
    }
    record
}

fn build_record(t: &BuildTelemetry, total: Duration, outcome: &str, trigger: &crate::build::BuildTrigger) -> serde_json::Value {
    use crate::build::BuildTrigger;
    let (kind, changed) = match trigger {
        BuildTrigger::Full => ("full", 0),
        BuildTrigger::Structural(paths) => ("structural", paths.len()),
        BuildTrigger::ContentOnly(paths) => ("content", paths.len()),
    };
    let mut record = profile_record(t, "build", &[BUILD_COUNTS, VIDEO_COUNTS], total, outcome);
    record["trigger"] = serde_json::json!({ "kind": kind, "changed": changed });
    record
}

/// The profile is trimmed to its newest half past this size (about a thousand builds).
const PROFILE_TRIM_BYTES: u64 = 2 * 1024 * 1024;

/// Appends are whole lines under O_APPEND, so processes sharing a folder do not
/// interleave them; a trim that races another process's append can drop that
/// one line, which a best-effort profile accepts rather than take a file lock.
pub(crate) fn append_record(path: &std::path::Path, record: &serde_json::Value) -> std::io::Result<()> {
    // Builds of different folders can finish together; serialize append/trim pairs.
    static WRITER: Mutex<()> = Mutex::new(());
    let _writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || crate::build::icloud::is_dataless_unavailable(&e) => 0,
        Err(e) => return Err(e),
    };
    if size == 0 {
        return crate::build::io_utils::write_output(path, &line);
    }
    if size < PROFILE_TRIM_BYTES {
        return crate::build::io_utils::append_output(path, &line);
    }
    // Over the cap: keep the newest half, so this read happens once per many builds.
    let previous = std::fs::read_to_string(path)?;
    let mut kept: Vec<u8> = Vec::with_capacity(previous.len() / 2 + line.len());
    let mut from = previous.len().saturating_sub((PROFILE_TRIM_BYTES / 2) as usize);
    while !previous.is_char_boundary(from) { from += 1; }
    let bytes = previous.as_bytes();
    if let Some(newline) = bytes[from..].iter().position(|&b| b == b'\n') { from += newline + 1; }
    kept.extend_from_slice(&bytes[from..]);
    kept.extend_from_slice(&line);
    crate::build::io_utils::write_output(path, &kept)
}

pub(crate) fn append_profile(collector: &PhaseCollector, total: Duration, result: &Result<String, String>, config: &crate::build::PipelineConfig) {
    let record = collector.lock().map(|t| build_record(&t, total, if result.is_ok() { "ok" } else { "error" }, &config.trigger));
    let written = record.map_err(|e| std::io::Error::other(e.to_string()))
        .and_then(|record| append_record(&crate::moss_paths::MossPaths::new(config.source()).profile(), &record));
    if let Err(e) = written { log::debug!("could not append build profile: {e}"); }
}

/// Work a build starts but does not wait for (a video encode the app's watcher
/// picks up) finishes after that build's record is written, so it writes a
/// record of its own instead of adding to a closed one.
pub(crate) fn run_detached<T>(profile: std::path::PathBuf, body: impl FnOnce() -> T, outcome: impl FnOnce(&T) -> &'static str) -> T {
    let collector: PhaseCollector = Arc::new(Mutex::new(BuildTelemetry { passes: 1, ..BuildTelemetry::default() }));
    let started = Instant::now();
    let result = {
        let _scope = enter_blocking_scope(Some(collector.clone()));
        body()
    };
    if let Ok(t) = collector.lock() {
        if let Err(e) = append_record(&profile, &profile_record(&t, "video_encode", &[VIDEO_COUNTS], started.elapsed(), outcome(&result))) {
            log::debug!("could not append encode profile: {e}");
        }
    }
    result
}

/// RAII phase guard. Logs on creation and on drop.
pub struct PhaseTrace {
    name: &'static str,
    started: Instant,
    collector: Option<PhaseCollector>,
    pass: usize,
}

impl PhaseTrace {
    pub fn since(name: &'static str, started: Instant) -> Self {
        let mut trace = Self::start(name);
        trace.started = started;
        trace
    }

    pub fn start(name: &'static str) -> Self {
        log::debug!(target: "phase", "{} started", name);
        let collector = current_collector();
        let pass = collector.as_ref().and_then(|c| c.lock().ok().map(|t| t.passes)).unwrap_or(0);
        Self {
            name,
            started: Instant::now(),
            collector,
            pass,
        }
    }
}

impl Drop for PhaseTrace {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        log::debug!(target: "phase", "{} completed in {:?}", self.name, elapsed);
        record_phase(&self.collector, self.pass, self.name, elapsed);
        // Budget overrun logging. Observability only: never panics, never
        // changes control flow, never counts toward the CLI's problem count,
        // and there is no CI wall-clock gate. See `budget_log`'s doc for why
        // this never reaches `log::error!`.
        if let Some((level, message)) = budget_log(self.name, elapsed) {
            log::log!(target: "phase", level, "{}", message);
        }
    }
}

/// Sugar: `phase!("name", { ... })` — wraps a block in a `PhaseTrace` whose
/// lifetime is exactly the block. The block's value is returned as the
/// expression value.
///
/// Prefer over `let _p = PhaseTrace::start(...)` when the phase is a single
/// expression — the macro makes the scope unambiguous.
#[macro_export]
macro_rules! phase {
    ($name:expr, $body:block) => {{
        let _phase_trace = $crate::build::phase::PhaseTrace::start($name);
        $body
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn video_phase_reaches_collector_through_worker_and_nested_spawn() {
        let ((), collector) = with_collector(async {
            begin_pass();
            let mut workers = tokio::task::JoinSet::new();
            crate::build::background::spawn_traced_worker(&mut workers, || {
                std::thread::spawn(inherit_blocking(|| {
                    crate::build::media::video::run_video_conversion(
                        &crate::types::services::BuildServices::headless(),
                        &crate::types::services::BackgroundContext::for_test(), 0, None,
                    );
                })).join().unwrap();
                Ok(())
            });
            while let Some(result) = workers.join_next().await { result.unwrap().unwrap(); }
        }).await;
        let telemetry = collector.lock().unwrap();
        assert!(telemetry.phases.iter().any(|p| p.name == "video_handling" && p.pass == 1));
    }

    #[tokio::test]
    async fn profile_serializer_tags_passes_and_trims() {
        let ((), collector) = with_collector(async {
            begin_pass();
            { let _phase = PhaseTrace::start("search_index"); }
            record_count("search_pages", 2);
            begin_pass();
            tokio::task::spawn_blocking(inherit_blocking(|| {
                let _phase = PhaseTrace::start("search_index");
                record_count("search_pages", 3);
            })).await.unwrap();
        }).await;
        let record = build_record(&collector.lock().unwrap(), Duration::from_millis(42), "ok", &crate::build::BuildTrigger::ContentOnly(vec!["page.md".into()]));
        assert_eq!(record["schema"], 1);
        assert_eq!(record["kind"], "build");
        assert_eq!(record["moss_version"], crate::system::app_version());
        assert_eq!(record["build_profile"], crate::system::build_profile());
        chrono::DateTime::parse_from_rfc3339(record["started_at"].as_str().unwrap()).unwrap();
        assert_eq!(record["total_ms"], 42);
        assert_eq!(record["outcome"], "ok");
        assert_eq!(record["trigger"], serde_json::json!({"kind": "content", "changed": 1}));
        assert_eq!(record["passes"], 2);
        assert_eq!(record["phases"][0]["pass"], 1);
        assert_eq!(record["phases"][1]["pass"], 2);
        assert_eq!(record["phases"][1]["name"], "search_index");
        assert!(record["phases"][1]["ms"].is_number());
        assert_eq!(record["counts"]["search_pages"], 5);
        #[cfg(unix)]
        assert!(record["peak_rss_mb"].as_f64().unwrap() > 0.0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.jsonl");
        let pad = "x".repeat(2000);
        let mut n = 0;
        while std::fs::metadata(&path).map_or(0, |m| m.len()) < PROFILE_TRIM_BYTES {
            append_record(&path, &serde_json::json!({"n": n, "pad": pad})).unwrap();
            n += 1;
        }
        append_record(&path, &record).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size <= PROFILE_TRIM_BYTES / 2 + 4096, "trimmed to about half: {size}");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(lines.last().unwrap(), &record);
        let kept: Vec<u64> = lines[..lines.len() - 1].iter().map(|l| l["n"].as_u64().unwrap()).collect();
        assert_eq!(*kept.last().unwrap(), n - 1, "the newest lines are the ones kept");
        assert!(kept.windows(2).all(|w| w[1] == w[0] + 1));
        let before = lines.len();
        append_record(&path, &record).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), before + 1);
    }

    /// Time spent waiting for another holder of the folder is its own phase,
    /// not part of the work around it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_build_that_waits_for_the_folder_lock_records_the_wait() {
        use crate::build::{run_pipeline, PipelineConfig, PluginMode, BuildTrigger};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("index.md"), "---\ntitle: Garden\n---\n\nA quiet garden.\n").unwrap();
        let held = crate::infra::folder_lock::acquire(&root).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(held);
        });
        run_pipeline(PipelineConfig {
            root: crate::vault::paths::VaultRoot::resolve(&root),
            progress: Arc::new(crate::build::ports::reporter::NullReporter), plugins: PluginMode::Skip,
            watch: false, start_server: false,
            host: crate::build::ports::host::test_host_ports(),
            trigger: BuildTrigger::Full, exits_after_build: true,
            site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
        }).await.expect("fixture builds");
        release.join().unwrap();
        let text = std::fs::read_to_string(crate::moss_paths::MossPaths::new(&root).profile()).unwrap();
        let record: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        let waited: u64 = record["phases"].as_array().unwrap().iter()
            .filter(|p| p["name"] == "lock_wait").map(|p| p["ms"].as_u64().unwrap()).sum();
        assert!(waited >= 300, "the wait for the held folder lock is recorded as lock_wait: {record}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn profile_real_build_records_once_per_call() {
        use crate::build::{run_pipeline, PipelineConfig, PluginMode, BuildTrigger};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("index.md"), "---\ntitle: Garden\nuid: garden-home\n---\n\nA quiet garden.\n\n![Leaf](leaf.png)\n").unwrap();
        std::fs::write(root.join("notes.md"), "---\ntitle: Notes\nuid: garden-notes\n---\n\nGrowing seeds.\n").unwrap();
        image::RgbImage::from_pixel(1500, 900, image::Rgb([20, 90, 40])).save(root.join("leaf.png")).unwrap();
        let video = crate::build::media::ffmpeg::real_ffmpeg();
        if let Some(ref bin) = video {
            assert!(crate::build::media::ffmpeg::synthesise(bin, &root.join("clip.mov"), "320x240"));
        }
        struct Receipts(Mutex<Vec<u32>>);
        impl crate::build::ports::reporter::BuildReporter for Receipts {
            fn report(&self, event: &crate::build::progress::PipelineEvent) {
                if let crate::build::progress::PipelineEvent::BuildComplete { videos_converted, .. } = event {
                    self.0.lock().unwrap().push(*videos_converted);
                }
            }
        }
        let paths = crate::moss_paths::MossPaths::new(root);
        let mut first_output = None;
        for run in 1..=2 {
            let receipts = Arc::new(Receipts(Mutex::new(Vec::new())));
            let mut host = crate::build::ports::host::test_host_ports();
            host.services.reporter = receipts.clone();
            run_pipeline(PipelineConfig {
                root: crate::vault::paths::VaultRoot::resolve(root),
                progress: receipts.clone(), plugins: PluginMode::Skip,
                watch: false, start_server: false,
                host,
                trigger: BuildTrigger::Full, exits_after_build: true,
                site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
            }).await.expect("fixture builds");
            let generation = paths.generation_dir(&paths.current_generation_id().unwrap());
            let output: std::collections::BTreeMap<_, _> = walkdir::WalkDir::new(&generation).into_iter()
                .map(Result::unwrap).filter(|e| e.file_type().is_file())
                .map(|e| (e.path().strip_prefix(&generation).unwrap().to_path_buf(), std::fs::read(e.path()).unwrap()))
                .collect();
            assert!(!output.is_empty());
            if let Some(ref first) = first_output { assert_eq!(&output, first, "served bytes changed"); }
            else { first_output = Some(output); }
            let text = std::fs::read_to_string(paths.profile()).expect("build must write profile.jsonl");
            let records: Vec<serde_json::Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(records.len(), run, "one record per call, including media rerenders");
            let record = records.last().unwrap();
            assert_eq!(record["kind"], "build");
            assert_eq!(record["outcome"], "ok");
            let phases = record["phases"].as_array().unwrap();
            // The fixture has no search index, so search_index and its counts are covered by the serializer test.
            for name in ["media_prelude", "background_asset_copy", "seal_persist", "seal_promote"] {
                assert!(phases.iter().any(|p| p["name"] == name), "missing {name}: {record}");
            }
            assert!(phases.iter().all(|p| p["pass"].as_u64().unwrap() >= 1));
            for name in ["hash_index_hits_process", "hash_index_rehashed_process", "pages_rendered", "pages_carried", "slot_html_scanned", "slot_html_rewritten", "background_assets_copied", "background_assets_hits", "background_assets_misses", "images_unchanged", "images_dispatched", "files_sealed"] {
                assert!(record["counts"][name].is_number(), "missing {name}: {record}");
            }
            if video.is_some() {
                // The MP4 is probed while it is encoded and stamped then; the thumbnail
                // and poster are decoded once, on the rerender that reuses them.
                assert_eq!(record["counts"]["video_blobs_validated"], if run == 1 { 2 } else { 0 }, "{record}");
                assert_eq!(record["counts"]["videos_converted"], if run == 1 { 1 } else { 0 }, "{record}");
                // A fresh encode triggers another render pass, which reuses its cached output.
                assert_eq!(record["counts"]["videos_cache_hit"], record["passes"].as_u64().unwrap() - u64::from(run == 1), "{record}");
                assert_eq!(receipts.0.lock().unwrap().iter().sum::<u32>(), if run == 1 { 1 } else { 0 });
                eprintln!("video fixture build {run}: {}", record["counts"]);
            }
            if run == 2 {
                assert_eq!(record["counts"]["output_unchanged"], 1, "{record}");
                for name in ["background_assets_misses", "background_assets_linked", "sized_raster_blob_checks"] {
                    assert_eq!(record["counts"][name], 0, "{name}: {record}");
                }
                assert!(record["counts"]["background_assets_stat_only"].as_u64().unwrap() > 0);
                assert_eq!(record["counts"]["image_variants_blob_checks"], 0, "{record}");
                assert_eq!(record["counts"]["image_transform_records_loaded"], 1, "{record}");
                assert_eq!(record["counts"]["images_dispatched"], 0, "{record}");
                assert!(record["counts"]["image_variants_stat_only"].as_u64().unwrap() >= 2, "base and rung: {record}");
            }
        }
    }

    #[test]
    fn drop_records_elapsed() {
        // We can't assert the log output without capturing it, but we can
        // assert the API doesn't panic on drop or extreme elapsed values.
        {
            let _p = PhaseTrace::start("test_phase");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn budget_class_under_budget_is_ok() {
        // scan budget is 2000ms.
        assert_eq!(
            budget_class("scan", Duration::from_millis(1999)),
            BudgetClass::Ok
        );
        assert_eq!(budget_class("scan", Duration::ZERO), BudgetClass::Ok);
    }

    #[test]
    fn budget_class_at_budget_is_warn() {
        assert_eq!(
            budget_class("scan", Duration::from_millis(2_000)),
            BudgetClass::Warn
        );
        // Anywhere in [1x, 2x) stays WARN.
        assert_eq!(
            budget_class("scan", Duration::from_millis(3_999)),
            BudgetClass::Warn
        );
    }

    #[test]
    fn budget_class_at_double_budget_is_severe() {
        assert_eq!(
            budget_class("scan", Duration::from_millis(4_000)),
            BudgetClass::Severe
        );
        assert_eq!(
            budget_class("scan", Duration::from_secs(3600)),
            BudgetClass::Severe
        );
    }

    #[test]
    fn budget_class_unbudgeted_phase_is_always_ok() {
        // A phase with no row in PHASE_BUDGETS_MS has no thresholds.
        assert_eq!(
            budget_class("no_such_phase", Duration::from_secs(3600)),
            BudgetClass::Ok
        );
    }

    #[test]
    fn every_budgeted_phase_is_unique() {
        // A duplicate row would make the lookup silently prefer the first
        // entry; keep the table a proper map.
        let mut names: Vec<&str> = PHASE_BUDGETS_MS.iter().map(|&(n, _)| n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate phase name in PHASE_BUDGETS_MS");
    }

    #[test]
    fn over_budget_drop_takes_warn_log_path_without_panicking() {
        // Exercise the WARN branch of Drop end-to-end by backdating the start
        // instant past the "native_process_spawn" budget (100ms). The log line
        // itself isn't captured (no log-capture harness in this crate); the
        // threshold decision is covered by the pure budget_class tests above.
        let p = PhaseTrace {
            name: "native_process_spawn",
            started: Instant::now() - Duration::from_millis(150),
            collector: None,
            pass: 0,
        };
        assert_eq!(
            budget_class(p.name, p.started.elapsed()),
            BudgetClass::Warn
        );
        drop(p); // WARN branch runs here; must not panic.
    }

    #[test]
    fn over_double_budget_drop_takes_severe_log_path_without_panicking() {
        let p = PhaseTrace {
            name: "native_process_spawn",
            started: Instant::now() - Duration::from_millis(500),
            collector: None,
            pass: 0,
        };
        assert_eq!(
            budget_class(p.name, p.started.elapsed()),
            BudgetClass::Severe
        );
        drop(p); // Severe branch runs here; must not panic.
    }

    // --- budget_log: the level/wording a budget overrun logs at ---
    //
    // These are the tests that actually pin the fix: a build's timing
    // heuristic is a performance note, not a failure, so neither overrun
    // tier may reach `log::Level::Error` — that reads as "the build failed"
    // to a person scanning the log, and to a coding agent that treats an
    // ERROR line as something to fix, even though the build succeeded.

    #[test]
    fn budget_log_is_silent_within_budget() {
        assert!(budget_log("scan", Duration::from_millis(1_999)).is_none());
    }

    #[test]
    fn budget_log_notes_at_one_times_budget() {
        let (level, message) = budget_log("scan", Duration::from_millis(2_000)).unwrap();
        assert_eq!(level, log::Level::Info);
        assert!(message.contains("scan"));
        assert!(message.contains("performance note, not a failure"));
    }

    #[test]
    fn budget_log_at_two_times_budget_is_still_a_note() {
        // The line this whole fix is about: a phase running twice its
        // (initial, self-admittedly-untuned) budget must log at INFO, never
        // WARN or ERROR — before this fix, this branch logged `log::error!` and a
        // successful, large-site build read as failed.
        let (level, message) = budget_log("scan", Duration::from_millis(4_000)).unwrap();
        assert_eq!(
            level,
            log::Level::Info,
            "a 2x budget overrun is a performance note, not a build failure"
        );
        assert!(message.contains("scan"));
        assert!(message.contains("performance note, not a failure"));
    }

    // --- build.summary collector ---

    #[test]
    fn phase_trace_outside_any_collector_scope_is_a_silent_no_op() {
        // Every real call site in this crate uses PhaseTrace with no idea
        // whether a collector is active (tests, CLI paths outside
        // run_pipeline). If record_phase ever panicked or blocked absent a
        // scope, that would break unrelated callers far from this file.
        let _p = PhaseTrace::start("no_collector_here");
        drop(_p);
    }

    #[test]
    fn record_count_outside_any_collector_scope_is_a_silent_no_op() {
        record_count("pages", 3);
    }

    #[tokio::test]
    async fn with_collector_captures_a_phase_started_inside_it() {
        let ((), collector) = with_collector(async {
            let _p = PhaseTrace::start("test_async_phase");
        })
        .await;
        let telemetry = collector.lock().unwrap();
        assert_eq!(telemetry.phases.len(), 1);
        assert_eq!(telemetry.phases[0].name, "test_async_phase");
    }

    #[tokio::test]
    async fn with_collector_captures_a_count_recorded_inside_it() {
        let ((), collector) = with_collector(async {
            record_count("pages", 7);
        })
        .await;
        let telemetry = collector.lock().unwrap();
        assert_eq!(telemetry.counts, vec![("pages", 7)]);
    }

    #[tokio::test]
    async fn a_phase_across_an_await_point_is_still_recorded() {
        // The whole reason ASYNC_PHASE_COLLECTOR is a task-local and not a
        // thread-local: tokio's multi-thread runtime may resume this task on
        // a different worker thread after the await, and the collector must
        // still be reachable when the PhaseTrace guard drops.
        let ((), collector) = with_collector(async {
            let _p = PhaseTrace::start("spans_await");
            tokio::task::yield_now().await;
        })
        .await;
        let telemetry = collector.lock().unwrap();
        assert!(telemetry.phases.iter().any(|p| p.name == "spans_await"));
    }

    #[tokio::test]
    async fn enter_blocking_scope_bridges_the_collector_into_a_blocking_thread() {
        // spawn_blocking does not inherit task-locals — this is what proves
        // enter_blocking_scope's explicit bridge is actually doing the work,
        // not the task-local alone.
        let ((), collector) = with_collector(async {
            let bridged = current_collector();
            tokio::task::spawn_blocking(move || {
                let _guard = enter_blocking_scope(bridged);
                let _p = PhaseTrace::start("inside_blocking");
            })
            .await
            .unwrap();
        })
        .await;
        let telemetry = collector.lock().unwrap();
        assert!(telemetry.phases.iter().any(|p| p.name == "inside_blocking"));
    }

    #[tokio::test]
    async fn a_blocking_closure_never_bridged_records_nothing_for_that_phase() {
        // Ablation of the bridge itself: without enter_blocking_scope, a
        // phase inside spawn_blocking is silently absent from the summary
        // rather than mis-attributed or panicking.
        let ((), collector) = with_collector(async {
            tokio::task::spawn_blocking(|| {
                let _p = PhaseTrace::start("unbridged_blocking");
            })
            .await
            .unwrap();
        })
        .await;
        let telemetry = collector.lock().unwrap();
        assert!(!telemetry.phases.iter().any(|p| p.name == "unbridged_blocking"));
    }

    #[test]
    fn format_build_summary_includes_every_phase_and_count() {
        let phases = vec![("scan", Duration::from_millis(12)), ("build", Duration::from_millis(340))];
        let counts = vec![("pages", 7usize)];
        let edits = crate::build::BuildTrigger::ContentOnly(vec!["a.md".into(), "b.md".into()]);
        let line = format_build_summary(&phases, &counts, Duration::from_millis(400), "ok", &edits);
        assert!(line.contains("outcome=ok trigger=content changed=2 total=400ms"), "{line}");
        assert!(line.contains("outcome=ok"));
        assert!(line.contains("total=400ms"));
        assert!(line.contains("pages=7"));
        assert!(line.contains("scan=12ms"));
        assert!(line.contains("build=340ms"));
    }

    #[test]
    fn format_build_summary_with_nothing_recorded_still_reports_outcome_and_total() {
        let line = format_build_summary(&[], &[], Duration::from_millis(5), "error", &crate::build::BuildTrigger::Full);
        assert_eq!(line, "build.summary outcome=error trigger=full total=5ms");
    }
}
