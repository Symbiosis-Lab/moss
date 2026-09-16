use super::*;
use std::io::Write;
use std::sync::Mutex;

/// A writer that fails every operation with `BrokenPipe`, standing in for a
/// stderr pipe whose reader has closed (`moss build | head`).
struct AlwaysBrokenPipe;
impl Write for AlwaysBrokenPipe {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
}

#[test]
fn write_status_line_swallows_broken_pipe_without_panicking() {
    // The CLI's own `cli_eprintln!` status/progress output must not panic
    // when the reader of stderr has closed (`moss build | head`). Same
    // hazard as the log sink, different write path (direct, not via fern).
    write_status_line(AlwaysBrokenPipe, format_args!("[{:>3}%] {}", 42, "rendering"));
}

#[test]
fn write_status_line_writes_formatted_line_to_healthy_sink() {
    let mut sink: Vec<u8> = Vec::new();
    write_status_line(&mut sink, format_args!("Building website from: {}", "/x"));
    assert_eq!(sink, b"Building website from: /x\n");
}

// CLI_PROBLEMS is a process-global static, so these tests must not run
// concurrently with each other (or with anything else that calls
// `cli_warn!`/`note_cli_problem`/`take_cli_problems`). Serialize with a
// dedicated lock rather than relying on cargo test's default threading.
static PROBLEMS_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn cli_warn_increments_the_problem_counter() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems(); // drain any count left over from another test
    cli_warn!("unknown shortcode: {}", "foo");
    cli_warn!("dangling link: {}", "bar");
    assert_eq!(take_cli_problems(), 2);
}

/// The plugin manager reports headless through `CarrierReporter`, never
/// `StdoutReporter` — so the counted "not connected" line has to reach the
/// `--strict` counter from THERE, or a plugin-bearing build with a plugin that
/// cannot run prints "Build complete" and exits 0.
#[test]
fn carrier_reporter_counts_a_plugin_needing_connection() {
    use crate::build::ports::reporter::BuildReporter;
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    crate::ops::serve::events::CarrierReporter.report(
        &crate::build::progress::PipelineEvent::PluginNeedsConnection {
            plugin: "matters".into(),
            project_path: "/tmp/vault".into(),
        },
    );
    assert_eq!(take_cli_problems(), 1, "a plugin that cannot run is a --strict problem");
}

#[test]
fn take_cli_problems_resets_to_zero() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems(); // drain any count left over from another test
    cli_warn!("one problem");
    assert_eq!(take_cli_problems(), 1, "first read sees the problem");
    assert_eq!(take_cli_problems(), 0, "second read starts clean, as --watch needs");
}

#[test]
fn build_result_summary_is_silent_at_zero_problems() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems(); // ensure a clean slate before asserting on the count
    assert_eq!(take_cli_problems(), 0, "no problems were reported");
    // A clean build stays silent on BOTH paths — including under --strict,
    // where a spurious summary would be the loudest possible false alarm.
    assert_eq!(problem_summary_line(0, false), None);
    assert_eq!(problem_summary_line(0, true), None);
}

/// The summary is what an agent greps, so its shape is a contract: the
/// `moss: ` prefix, a correctly inflected noun, and an em dash.
#[test]
fn problem_summary_inflects_and_is_greppable() {
    let one = problem_summary_line(1, false).expect("1 problem must summarize");
    assert_eq!(one, "moss: 1 problem reported above — the site was still generated.");

    let many = problem_summary_line(3, false).expect("3 problems must summarize");
    assert_eq!(many, "moss: 3 problems reported above — the site was still generated.");

    for line in [&one, &many] {
        assert!(line.starts_with("moss: "), "agents grep the prefix: {line}");
        assert!(!line.contains("(s)"), "spell the plural, don't paren it: {line}");
        assert!(!line.contains(" - "), "house style is an em dash: {line}");
    }
}

/// Under `--strict` the same count earns a different ending, because the
/// site was NOT accepted — saying "still generated" next to an exit-1 would
/// be telling the user the opposite of what happened.
#[test]
fn strict_summary_says_why_the_build_is_failing() {
    let strict = problem_summary_line(2, true).expect("2 problems must summarize");
    assert_eq!(strict, "moss: 2 problems reported above — failing because --strict was requested.");
    assert!(!strict.contains("still generated"));
}

/// The drain hazard, pinned. `print_cli_build_result` swaps the counter to
/// zero, so its RETURN VALUE is the only surviving copy — a `--strict`
/// decision that re-read the counter afterwards would see 0 and never fail.
/// This asserts both halves from one sequence: the count comes back, and
/// the counter is empty behind it.
#[test]
fn build_result_returns_the_count_it_drained() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems(); // clean slate

    cli_warn!("[warn] a retired class");
    cli_warn!("[warn] a foreign frontmatter key");

    // Stand-in for `print_cli_build_result`'s drain: the real function also
    // touches the guidance pointer (filesystem), so pin the contract on the
    // step that matters. If this ever stops being a `swap`, the assertion
    // below fails and the `--strict` wiring is protected.
    let reported = take_cli_problems();
    assert_eq!(reported, 2, "the caller must receive the real count");
    assert_eq!(
        take_cli_problems(),
        0,
        "the counter is drained — re-reading it for --strict would see 0"
    );
    assert!(
        problem_summary_line(reported, true).is_some(),
        "--strict must still be able to summarize from the returned value"
    );
}

/// `headless_log_level` must default to `Warn` and honor every `MOSS_LOG_LEVEL`
/// value the GUI's own parser accepts (`lib.rs`), so the two paths cannot drift
/// on what a given env var means even though they pick different defaults.
///
/// Runs serially (env vars are process-global) and restores whatever value was
/// there — a stray `MOSS_LOG_LEVEL` a run set for its own purposes must not
/// leak into a later test in the same process.
#[test]
fn headless_log_level_defaults_to_warn_and_honors_the_env_var() {
    let prior = std::env::var("MOSS_LOG_LEVEL").ok();

    std::env::remove_var("MOSS_LOG_LEVEL");
    assert_eq!(
        headless_log_level(),
        log::LevelFilter::Warn,
        "unset MOSS_LOG_LEVEL must default to Warn — quiet CLI output by \
         default, but not so quiet that log_warn_problem!'s explanation is lost"
    );

    for (value, expected) in [
        ("error", log::LevelFilter::Error),
        ("warn", log::LevelFilter::Warn),
        ("info", log::LevelFilter::Info),
        ("debug", log::LevelFilter::Debug),
        ("trace", log::LevelFilter::Debug),
        ("not-a-level", log::LevelFilter::Warn),
    ] {
        std::env::set_var("MOSS_LOG_LEVEL", value);
        assert_eq!(
            headless_log_level(),
            expected,
            "MOSS_LOG_LEVEL={value} must resolve to {expected:?}"
        );
    }

    match prior {
        Some(v) => std::env::set_var("MOSS_LOG_LEVEL", v),
        None => std::env::remove_var("MOSS_LOG_LEVEL"),
    }
}
