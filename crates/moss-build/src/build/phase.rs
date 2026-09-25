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
//!   logs WARN at 1× and ERROR at 2× the budget. Observability ONLY — no
//!   panic, no changed return value, and explicitly NO CI wall-clock gate
//!   (shared-runner wall-clock tests are flake). Budget overrun lines stay at
//!   WARN/ERROR — they are a real signal, not the noise the started/completed
//!   pair was.
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
/// A phase elapsed >= budget logs WARN; >= 2x budget logs ERROR. Phases not
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
    // Stage 5a shadow-mode: facade hashing + DepGraph build + cache diff over
    // all documents. In-memory hashing/diffing only (no I/O beyond a small
    // JSON cache load/save), so this should stay far under render_html_pages
    // even at large vault sizes; budget is a generous placeholder pending
    // real measurement.
    ("shadow_facade_diff", 10_000),
];

/// Budget classification for a completed (or aborted) phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetClass {
    /// Under budget, or the phase has no budget row.
    Ok,
    /// Elapsed >= 1x budget.
    Warn,
    /// Elapsed >= 2x budget.
    Error,
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
/// WARN at budget, ERROR at 2x budget; unbudgeted phases are always `Ok`.
pub fn budget_class(phase: &str, elapsed: Duration) -> BudgetClass {
    let Some(budget) = budget_ms(phase) else {
        return BudgetClass::Ok;
    };
    let elapsed_ms = elapsed.as_millis();
    if elapsed_ms >= u128::from(budget) * 2 {
        BudgetClass::Error
    } else if elapsed_ms >= u128::from(budget) {
        BudgetClass::Warn
    } else {
        BudgetClass::Ok
    }
}

/// One build's accumulated phase timings and counts, behind a lock so it can
/// be shared into a `spawn_blocking` closure and read back after `.await`.
#[derive(Debug, Default)]
pub struct BuildTelemetry {
    pub phases: Vec<(&'static str, Duration)>,
    pub counts: Vec<(&'static str, usize)>,
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
}

/// Run `body` with a fresh collector active, then hand the collector back
/// alongside the body's result so the caller can log a `build.summary` line
/// from it. The one caller is `build::run_pipeline`.
pub async fn with_collector<F, T>(body: F) -> (T, PhaseCollector)
where
    F: std::future::Future<Output = T>,
{
    let collector: PhaseCollector = Arc::new(Mutex::new(BuildTelemetry::default()));
    let result = ASYNC_PHASE_COLLECTOR.scope(collector.clone(), body).await;
    (result, collector)
}

/// Record a phase's elapsed time into whichever collector (async-task or
/// blocking-thread) is active. A no-op outside any collector scope.
fn record_phase(name: &'static str, elapsed: Duration) {
    let recorded = ASYNC_PHASE_COLLECTOR
        .try_with(|c| {
            if let Ok(mut t) = c.lock() {
                t.phases.push((name, elapsed));
            }
        })
        .is_ok();
    if recorded {
        return;
    }
    BLOCKING_PHASE_COLLECTOR.with(|c| {
        if let Some(collector) = c.borrow().as_ref() {
            if let Ok(mut t) = collector.lock() {
                t.phases.push((name, elapsed));
            }
        }
    });
}

/// Record a named count (pages rendered, missing-reference entries, …) into
/// whichever collector is active. Same no-op-outside-a-scope behavior as
/// [`record_phase`]. Public so `build.rs` can call it directly at the point
/// values like `build_documents.len()` are already in scope, rather than
/// threading them back out through `run_pipeline`'s return type.
pub fn record_count(name: &'static str, value: usize) {
    let recorded = ASYNC_PHASE_COLLECTOR
        .try_with(|c| {
            if let Ok(mut t) = c.lock() {
                t.counts.push((name, value));
            }
        })
        .is_ok();
    if recorded {
        return;
    }
    BLOCKING_PHASE_COLLECTOR.with(|c| {
        if let Some(collector) = c.borrow().as_ref() {
            if let Ok(mut t) = collector.lock() {
                t.counts.push((name, value));
            }
        }
    });
}

/// Pure formatter for the `build.summary` line — factored out of
/// [`log_build_summary`] so the shape is testable without a log-capture
/// harness (none exists in this crate; see `phase.rs`'s own tests below).
pub fn format_build_summary(
    phases: &[(&'static str, Duration)],
    counts: &[(&'static str, usize)],
    total: Duration,
    outcome: &str,
) -> String {
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
    let mut line = format!("build.summary outcome={outcome} total={}ms", total.as_millis());
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
pub fn log_build_summary(collector: &PhaseCollector, total: Duration, outcome: &str) {
    let telemetry = collector.lock().map(|t| (t.phases.clone(), t.counts.clone()));
    let (phases, counts) = telemetry.unwrap_or_default();
    log::info!(target: "build", "{}", format_build_summary(&phases, &counts, total, outcome));
}

/// RAII phase guard. Logs on creation and on drop.
pub struct PhaseTrace {
    name: &'static str,
    started: Instant,
}

impl PhaseTrace {
    pub fn start(name: &'static str) -> Self {
        log::debug!(target: "phase", "{} started", name);
        Self {
            name,
            started: Instant::now(),
        }
    }
}

impl Drop for PhaseTrace {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        log::debug!(target: "phase", "{} completed in {:?}", self.name, elapsed);
        record_phase(self.name, elapsed);
        // Budget overrun logging. Observability only: never panics,
        // never changes control flow, and there is no CI wall-clock gate.
        match budget_class(self.name, elapsed) {
            BudgetClass::Ok => {}
            BudgetClass::Warn => log::warn!(
                target: "phase",
                "{} exceeded its {}ms budget ({:?}) — initial budget, tune as timing data accrues",
                self.name,
                budget_ms(self.name).unwrap_or_default(),
                elapsed
            ),
            BudgetClass::Error => log::error!(
                target: "phase",
                "{} exceeded 2x its {}ms budget ({:?}) — initial budget, tune as timing data accrues",
                self.name,
                budget_ms(self.name).unwrap_or_default(),
                elapsed
            ),
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
    fn budget_class_at_double_budget_is_error() {
        assert_eq!(
            budget_class("scan", Duration::from_millis(4_000)),
            BudgetClass::Error
        );
        assert_eq!(
            budget_class("scan", Duration::from_secs(3600)),
            BudgetClass::Error
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
        };
        assert_eq!(
            budget_class(p.name, p.started.elapsed()),
            BudgetClass::Warn
        );
        drop(p); // WARN branch runs here; must not panic.
    }

    #[test]
    fn over_double_budget_drop_takes_error_log_path_without_panicking() {
        let p = PhaseTrace {
            name: "native_process_spawn",
            started: Instant::now() - Duration::from_millis(500),
        };
        assert_eq!(
            budget_class(p.name, p.started.elapsed()),
            BudgetClass::Error
        );
        drop(p); // ERROR branch runs here; must not panic.
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
        assert_eq!(telemetry.phases[0].0, "test_async_phase");
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
        assert!(telemetry.phases.iter().any(|(n, _)| *n == "spans_await"));
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
        assert!(telemetry.phases.iter().any(|(n, _)| *n == "inside_blocking"));
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
        assert!(!telemetry.phases.iter().any(|(n, _)| *n == "unbridged_blocking"));
    }

    #[test]
    fn format_build_summary_includes_every_phase_and_count() {
        let phases = vec![("scan", Duration::from_millis(12)), ("build", Duration::from_millis(340))];
        let counts = vec![("pages", 7usize)];
        let line = format_build_summary(&phases, &counts, Duration::from_millis(400), "ok");
        assert!(line.contains("outcome=ok"));
        assert!(line.contains("total=400ms"));
        assert!(line.contains("pages=7"));
        assert!(line.contains("scan=12ms"));
        assert!(line.contains("build=340ms"));
    }

    #[test]
    fn format_build_summary_with_nothing_recorded_still_reports_outcome_and_total() {
        let line = format_build_summary(&[], &[], Duration::from_millis(5), "error");
        assert_eq!(line, "build.summary outcome=error total=5ms");
    }
}
