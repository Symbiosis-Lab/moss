//! What a `moss build` says on the terminal, and how many problems it said.
//!
//! Two things live here: a `println!` replacement that cannot kill the app when
//! its output pipe closes, and the per-build count of problems that `--strict`
//! turns into an exit code. They are one module because the count is only
//! correct if every problem line goes through the macro that bumps it.
//!
//! This sits **inside the build tree on purpose**. It was the CLI half of
//! `crate::diagnostics`, which also owns the `tauri_plugin_log` sinks and the
//! Sentry breadcrumb ring — app-side code the build must not name. The macros
//! below are called 27 times from `build/`, and a `pub(crate)` macro does not
//! cross a crate boundary, so leaving them there blocked the `moss-build`
//! extraction twice over.
//! `crate::diagnostics` keeps the ring and the log targets and nothing else.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Write one status/progress line, **swallowing any write error**.
///
/// This is for the CLI's *own* human-readable output (`start_cli_build`
/// messages, the `crate::build::stdout_sink()` progress stream) — the direct
/// `eprintln!` calls, distinct from the `tauri-plugin-log` sink that
/// [`crate::diagnostics::stdout_target`] wraps. Same hazard, same fix: with
/// `panic = "abort"`, an `eprintln!` that hits a closed stderr pipe
/// (`moss build | head`, a disconnected harness) panics and aborts the whole
/// app. Routing through `writeln!` and discarding the `Result` makes a closed
/// pipe a no-op.
///
/// Split from [`eprint_status_line`] so the swallow is unit-testable against
/// a writer that always fails.
pub(crate) fn write_status_line(mut w: impl std::io::Write, args: std::fmt::Arguments<'_>) {
    let _ = writeln!(w, "{args}");
}

/// Broken-pipe-safe replacement for `eprintln!` in the CLI build path.
/// Prefer the [`cli_eprintln!`] macro at call sites.
pub fn eprint_status_line(args: std::fmt::Arguments<'_>) {
    write_status_line(std::io::stderr(), args);
}

/// Drop-in for `eprintln!` in CLI status/progress output that must not abort
/// the app on a closed stderr pipe. Same formatting syntax as `eprintln!`.
#[macro_export]
macro_rules! cli_eprintln {
    ($($arg:tt)*) => {
        $crate::build::cli_output::eprint_status_line(std::format_args!($($arg)*))
    };
}
pub use crate::cli_eprintln;

/// Per-build count of problems printed via [`cli_warn!`]. A "problem" here is
/// a warning printed straight to stderr through `cli_eprintln!` — those lines
/// never pass through `log`/`crate::diagnostics::diag_ring`, so this counter is
/// the only thing that can tell [`print_cli_build_result`] whether the build it
/// is about to summarize as "complete" actually had any. `Relaxed` is enough:
/// this is a plain counter, not a synchronization point with other memory.
#[cfg(not(test))]
static CLI_PROBLEMS: AtomicUsize = AtomicUsize::new(0);

// Unit tests count per thread instead. The test harness runs every test on its
// own fresh thread, all in one process, and dozens of tests drive code that
// logs an intended problem, so a process-wide count let one test's warning land
// inside another test's assertion. Each counting test calls the code it checks
// on its own thread, so its thread's count is exactly its own. The one blind
// spot: a problem logged from a worker thread the test spawned (rayon,
// `spawn_blocking`) is not seen by that test.
#[cfg(test)]
thread_local! {
    static CLI_PROBLEMS: AtomicUsize = const { AtomicUsize::new(0) };
}

#[cfg(not(test))]
fn with_problem_count<R>(f: impl FnOnce(&AtomicUsize) -> R) -> R {
    f(&CLI_PROBLEMS)
}

#[cfg(test)]
fn with_problem_count<R>(f: impl FnOnce(&AtomicUsize) -> R) -> R {
    CLI_PROBLEMS.with(f)
}

/// Record that one CLI-visible problem line was printed. Called by
/// [`cli_warn!`] and [`log_warn_problem!`] — never call this directly and
/// print separately, or the count can drift from what was actually shown.
pub fn note_cli_problem() {
    with_problem_count(|n| n.fetch_add(1, Ordering::Relaxed));
}

/// Read and reset the problem count. `swap`, not `load`: each build (and each
/// `--watch` rebuild) must start counting from zero, or problems from an
/// earlier build would be double-reported in a later summary.
pub fn take_cli_problems() -> usize {
    with_problem_count(|n| n.swap(0, Ordering::Relaxed))
}

/// Drop-in for `cli_eprintln!` at call sites that report a build *problem*
/// (a warning the user or an agent should notice) rather than routine status.
/// Bumps [`CLI_PROBLEMS`] and then prints exactly the same way
/// `cli_eprintln!` would — the count and the line are the same event by
/// construction, so they cannot disagree.
///
/// This macro alone never passes through `log` (see [`log_warn_problem!`]'s
/// doc), which is why the double-print bug happened: a call site that also
/// wanted the `log` stream hand-wrote a `log::warn!` beside it and printed
/// the same warning twice. A NEW problem call site almost always wants
/// `log_warn_problem!` instead — reach for this one only when the log stream
/// is genuinely not wanted for this warning.
macro_rules! cli_warn {
    ($($arg:tt)*) => {{
        $crate::build::cli_output::note_cli_problem();
        $crate::build::cli_output::eprint_status_line(std::format_args!($($arg)*))
    }};
}
pub(crate) use cli_warn;

/// A build problem that must reach **both** surfaces: the `log` stream (so the
/// diag ring, Send Logs and Sentry breadcrumbs carry it) and the problem count
/// `--strict` decides its exit code from.
///
/// Neither existing macro can do this. `cli_warn!` writes straight to stderr
/// and never passes through `log` (module docs above), so a diagnostic routed
/// through it vanishes from the ring. A bare `log::warn!` prints but is
/// invisible to `--strict`.
///
/// Unresolved wikilinks and asset references sat in that second gap: moss's own
/// guidance tells an agent to add `--strict` as its self-check, and moss's hard
/// rules make `[[wikilink]]` the only sanctioned way to write a link or embed an
/// image — so the single most likely authoring mistake in a moss site printed a
/// `[WARN]` line and still exited 0. Found by an agent-surface audit,
/// 2026-08-06.
macro_rules! log_warn_problem {
    ($($arg:tt)*) => {{
        $crate::build::cli_output::note_cli_problem();
        log::warn!($($arg)*)
    }};
}
pub(crate) use log_warn_problem;

/// Report a finished CLI build: the pipeline's own summary, then the problem
/// count if there was one.
///
/// One line the user also needs is deliberately NOT printed here — where a
/// coding agent should read to orient itself in this project. moss writes that
/// guidance during the build, and an agent has no reason to open a dot
/// directory it did not create, so something must say where it is; but "where
/// the guidance lives" is a fact about the CLI's own installation
/// (`cli::agents::sync` resolves it from `current_exe()` and `$HOME`), not
/// about the build that just ran. Each CLI entry point prints it immediately
/// after calling this — see `startup::headless` and `lib::run_cli_build`.
///
/// **Returns the problem count it drained**, and that return value is the only
/// way to learn it. [`take_cli_problems`] is a `swap`, so by the time this
/// function returns the counter reads zero — a caller that decides `--strict`'s
/// exit code by calling `take_cli_problems()` *after* this would always see 0
/// and `--strict` would be green forever. Decide the exit code from the value
/// returned here.
#[must_use = "the returned problem count is the only surviving copy — --strict's exit code depends on it"]
pub fn print_cli_build_result(message: &str, strict: bool) -> usize {
    cli_eprintln!("{}", message);
    let n = take_cli_problems();
    if let Some(line) = problem_summary_line(n, strict) {
        cli_eprintln!("{}", line);
    }
    n
}

/// The one-line problem summary, or `None` when a clean build should stay
/// silent.
///
/// Pure so the wording — singular/plural, and which of the two endings the
/// `--strict` flag earns — is unit-testable without capturing stderr. An agent
/// greps for the leading `moss: `, so that prefix is load-bearing: it is what
/// separates this line from the build's own status chatter on the same stream.
pub(crate) fn problem_summary_line(n: usize, strict: bool) -> Option<String> {
    if n == 0 {
        return None;
    }
    let noun = if n == 1 { "problem" } else { "problems" };
    let ending = if strict {
        "failing because --strict was requested"
    } else {
        "the site was still generated"
    };
    Some(format!("moss: {n} {noun} reported above — {ending}."))
}

#[cfg(test)]
#[path = "cli_output_tests.rs"]
mod tests;

// ─── The headless process logger (crossed from the desktop app's startup path) ─

/// The only `log::Log` a headless build ever installs.
///
/// The app's `tauri_plugin_log` is built inside the `tauri::Builder` chain a
/// headless build skips, and moss-cli has no such chain at all — without this,
/// both run with `log`'s default no-op logger and every `log::warn!` under
/// `build/` (including `log_warn_problem!`, whose entire purpose is to reach
/// both this stream and the `--strict` problem counter) is formatted, then
/// discarded. A disposable, stderr-only sink scoped to the one
/// process the CLI runs.
struct HeadlessLogger {
    level: log::LevelFilter,
}

impl log::Log for HeadlessLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Some((line, counts)) = log_line_for(record) {
            if counts {
                note_cli_problem();
            }
            cli_eprintln!("{}", line);
        }
    }

    fn flush(&self) {}
}

/// What one `log::Record` earns: the line to print (`None` drops it
/// entirely) and whether it counts toward `--strict`. Pulled out of the
/// `log::Log` impl so the decision is testable without installing a
/// process-global logger.
///
/// usvg/resvg (the SVG-rasterization crates moss vendors for favicons, OG
/// cards and math-fallback PNGs) log through this same global `log` facade.
/// A line like `usvg::parser::units: Invalid 'font-size' value: 'huge'.`
/// names no moss file on its own — but which of the three call sites fired
/// it is knowable: [`suppress_renderer_warnings`] is held for the two that
/// rasterize a SVG moss generated itself (the OG card, a math-fallback PNG),
/// so a renderer warning arriving while it is held is dropped from both the
/// printed list and the count (the same call `svg_util::strip_media_queries`
/// already makes for usvg's `@media` warning specifically), and one arriving
/// while it is NOT held is exactly `site_meta::favicon::generate_favicons`
/// rasterizing an author's own SVG — attributed to that file, printed, and
/// counted, rather than left uncounted like before.
fn log_line_for(record: &log::Record) -> Option<(String, bool)> {
    if is_dependency_svg_renderer_warning(record) {
        if renderer_warnings_suppressed() {
            return None;
        }
        return Some((
            format!("[{}] assets/favicon.svg: {}", record.level(), record.args()),
            true,
        ));
    }
    Some((format!("[{}] {}: {}", record.level(), record.target(), record.args()), false))
}

/// Whether `record` came from usvg or resvg.
///
/// A namespace match (`target == dep` or `target` starts with `"dep::"`),
/// never a substring one — a crate that merely starts with the same letters
/// (`usvgutil`, say) is a different crate and must not be swept up.
fn is_dependency_svg_renderer_warning(record: &log::Record) -> bool {
    const DEPENDENCIES: [&str; 2] = ["usvg", "resvg"];
    record.level() == log::Level::Warn
        && DEPENDENCIES.iter().any(|dep| {
            let target = record.target();
            target == *dep || target.starts_with(dep) && target[dep.len()..].starts_with("::")
        })
}

thread_local! {
    /// True while rendering one of moss's OWN generated SVGs through
    /// usvg/resvg (the OG card, a math-fallback PNG) — never while
    /// rasterizing an author-supplied one. See [`suppress_renderer_warnings`].
    static SUPPRESS_RENDERER_WARNINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn renderer_warnings_suppressed() -> bool {
    SUPPRESS_RENDERER_WARNINGS.with(std::cell::Cell::get)
}

/// Silence usvg/resvg warnings for the lifetime of the returned guard —
/// wrap every call that rasterizes a moss-GENERATED SVG (never an
/// author-supplied one) in `let _s = suppress_renderer_warnings();`. Such a
/// warning names no moss file (`HeadlessLogger::log`'s doc explains why), so
/// it is dropped rather than printed uncounted.
///
/// Restores the PREVIOUS value on drop rather than hard-resetting to
/// `false`, so a nested call (none exist today) cannot un-suppress an outer
/// one, and drops it even if the wrapped render call panics.
pub(crate) fn suppress_renderer_warnings() -> impl Drop {
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            SUPPRESS_RENDERER_WARNINGS.with(|c| c.set(self.0));
        }
    }
    Guard(SUPPRESS_RENDERER_WARNINGS.with(|c| c.replace(true)))
}

/// `MOSS_LOG_LEVEL` parsing for the headless path, defaulting to `Warn`.
///
/// A quieter default than the GUI's (Debug in a dev build, Info in release)
/// is deliberate — CLI stderr is output an agent or a script reads, not a log
/// pane a developer opens on demand. `Warn` is also the level
/// `log_warn_problem!` and the `[phase]` hang-detector traces are pitched at,
/// so the default is exactly the level that keeps the log stream and the
/// `--strict` counter working, with `MOSS_LOG_LEVEL` still available to turn
/// the rest back on.
fn headless_log_level() -> log::LevelFilter {
    match std::env::var("MOSS_LOG_LEVEL").ok().as_deref() {
        Some("error") => log::LevelFilter::Error,
        Some("warn") => log::LevelFilter::Warn,
        Some("info") => log::LevelFilter::Info,
        Some("debug") | Some("trace") => log::LevelFilter::Debug,
        _ => log::LevelFilter::Warn,
    }
}

static INSTALL_HEADLESS_LOGGER: std::sync::Once = std::sync::Once::new();

/// Install [`HeadlessLogger`] once per process. Idempotent so a `--watch`
/// rebuild loop (which re-enters the pipeline, not this function) and any
/// future second call cannot race `log::set_boxed_logger`'s single-assignment
/// contract.
pub fn install_headless_logger() {
    INSTALL_HEADLESS_LOGGER.call_once(|| {
        let level = headless_log_level();
        log::set_max_level(level);
        let _ = log::set_boxed_logger(Box::new(HeadlessLogger { level }));
    });
}

/// Shared prologue of every CLI build entry point (the app binary's
/// `moss build` and moss-cli): folder existence checks plus the unowned-site
/// guard, exiting non-zero on refusal. Exists so the two `main`s cannot
/// drift on what a buildable folder is.
pub fn preflight_cli_build(folder_path: &str) {
    let path = std::path::Path::new(folder_path);
    if !path.exists() {
        cli_eprintln!("Error: Folder does not exist: {}", folder_path);
        std::process::exit(1);
    }
    if !path.is_dir() {
        cli_eprintln!("Error: Path is not a directory: {}", folder_path);
        std::process::exit(1);
    }
    if let Err(msg) = crate::cli::site_guard::guard_cli_open(folder_path, "build") {
        cli_eprintln!("Error: {msg}");
        std::process::exit(1);
    }
    cli_eprintln!("Building website from: {}", folder_path);
}

/// Shared epilogue of a successful CLI build: drain-print the result
/// (printing DRAINS the problem counter — the returned count is the only
/// surviving copy), print the agent-guidance pointer, then wait out any
/// ui-bound background tasks with the cancel-aware poll. Returns the
/// problem count for the caller's strict-mode exit decision. Serve/watch
/// behaviour stays with the caller — it is host policy, not build epilogue.
pub async fn finish_cli_build(message: &str, folder: &str, strict: bool) -> usize {
    let problems = print_cli_build_result(message, strict);
    crate::cli::agents::sync::print_guidance_pointer(std::path::Path::new(folder));

    if let Some(session) = crate::system::folder_session::registry().get(folder) {
        if session.has_ui_bound() {
            cli_eprintln!("Waiting for background tasks...");
            while session.has_ui_bound() {
                if session.cancel.is_cancelled() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            cli_eprintln!("Background tasks complete");
        }
    }
    problems
}
