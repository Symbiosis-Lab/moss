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
static CLI_PROBLEMS: AtomicUsize = AtomicUsize::new(0);

/// Record that one CLI-visible problem line was printed. Called by
/// [`cli_warn!`] and [`log_warn_problem!`] — never call this directly and
/// print separately, or the count can drift from what was actually shown.
pub fn note_cli_problem() {
    CLI_PROBLEMS.fetch_add(1, Ordering::Relaxed);
}

/// Read and reset the problem count. `swap`, not `load`: each build (and each
/// `--watch` rebuild) must start counting from zero, or problems from an
/// earlier build would be double-reported in a later summary.
pub fn take_cli_problems() -> usize {
    CLI_PROBLEMS.swap(0, Ordering::Relaxed)
}

/// Drop-in for `cli_eprintln!` at call sites that report a build *problem*
/// (a warning the user or an agent should notice) rather than routine status.
/// Bumps [`CLI_PROBLEMS`] and then prints exactly the same way
/// `cli_eprintln!` would — the count and the line are the same event by
/// construction, so they cannot disagree.
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
        if self.enabled(record.metadata()) {
            cli_eprintln!("[{}] {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
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
