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
//! - Always logged at INFO. Phase counts are bounded (~10 per build) and
//!   knowing where time was spent is universally useful for operators.
//! - Logged on `Drop` so an early `?` return still records the partial elapsed.
//! - `target: "phase"` so users can filter (`MOSS_LOG_LEVEL=info` already
//!   shows these; future: per-target level filter).
//! - Per-phase budgets (#579): a phase that overruns its budget additionally
//!   logs WARN at 1× and ERROR at 2× the budget. Observability ONLY — no
//!   panic, no changed return value, and explicitly NO CI wall-clock gate
//!   (shared-runner wall-clock tests are flake).

use std::time::{Duration, Instant};

/// Per-phase build-time budgets in milliseconds (#579).
///
/// A phase elapsed >= budget logs WARN; >= 2x budget logs ERROR. Phases not
/// listed here are unbudgeted (no threshold logging) — add a row when adding
/// a new `PhaseTrace::start(...)` call site.
///
/// These are INITIAL values to tune as real-world timing data accrues
/// (#579). Seeds: the observed timings in this module's doc examples
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
    // 216-page reference vault (moss#922 Stage 0); budget set loosely above
    // that until real-world timing data accrues across vault sizes.
    ("render_markdown", 60_000),
    // Loop B: HTML emission for every page (blocking.rs). Measured ~5-7s on
    // the same 216-page vault (moss#922 Stage 0).
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

/// Classify an elapsed duration against the phase's budget (#579).
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

/// RAII phase guard. Logs on creation and on drop.
pub struct PhaseTrace {
    name: &'static str,
    started: Instant,
}

impl PhaseTrace {
    pub fn start(name: &'static str) -> Self {
        log::info!(target: "phase", "{} started", name);
        Self {
            name,
            started: Instant::now(),
        }
    }
}

impl Drop for PhaseTrace {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        log::info!(target: "phase", "{} completed in {:?}", self.name, elapsed);
        // Budget overrun logging (#579). Observability only: never panics,
        // never changes control flow, and there is no CI wall-clock gate.
        match budget_class(self.name, elapsed) {
            BudgetClass::Ok => {}
            BudgetClass::Warn => log::warn!(
                target: "phase",
                "{} exceeded its {}ms budget ({:?}) — initial budget per #579, tune as timing data accrues",
                self.name,
                budget_ms(self.name).unwrap_or_default(),
                elapsed
            ),
            BudgetClass::Error => log::error!(
                target: "phase",
                "{} exceeded 2x its {}ms budget ({:?}) — initial budget per #579, tune as timing data accrues",
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
}
