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

/// `link_terms_in_bylines`'s guard 6, first half: a name declared through
/// two different fields (author: and editor:, both landing in one declared
/// kind) still links exactly once from a byline row, but warns once too —
/// this counter is the only thing that can see the warning actually fired,
/// since `terms.rs`'s own tests can't take `PROBLEMS_TEST_LOCK`.
#[test]
fn duplicate_name_in_one_row_warns() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let kinds = vec![crate::build::terms::TermKind {
        key: "people".to_string(),
        fields: vec!["author".to_string(), "editor".to_string()],
        title: "People".to_string(),
        is_place: false, parents: Default::default(),
    }];
    let mut docs = vec![crate::build::types::ParsedDocument {
        url_path: "posts/a/index.html".to_string(),
        title: "A".to_string(),
        author: vec!["Ada Lin".to_string()],
        editor: vec!["Ada Lin".to_string()],
        byline: vec!["文｜Ada Lin".to_string()],
        ..Default::default()
    }];
    let index = crate::build::terms::derive_terms(&mut docs, kinds);
    crate::build::terms::link_terms_in_bylines(&mut docs, &index);
    assert_eq!(take_cli_problems(), 1, "the duplicate declaration warns exactly once, not twice");
    assert_eq!(
        docs[0].byline[0], "文｜[Ada Lin](/people/ada-lin/)",
        "still links exactly once despite the duplicate declaration"
    );
}

/// Task A8: confirms `vault::places::parse_gazetteer`'s diagnostics go
/// through `log_warn_problem!`, the CLI-visible macro, not a bare
/// `log::warn!` — so a malformed `.moss/places.toml` entry counts toward
/// `--strict`'s exit code, the same guarantee every other build diagnostic
/// carries.
#[test]
fn gazetteer_diagnostics_count_as_cli_problems() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let table: toml::value::Table = toml::from_str(
        "[\"Osaka\"]\nlat = 34.6937\nprecision = \"city\"\n\n[\"Nara\"]\nlat = 34.6851\nlng = 135.8048\nprecision = \"neighborhood\"\n",
    )
    .unwrap();
    let gaz = crate::vault::places::parse_gazetteer(&table);
    assert_eq!(
        take_cli_problems(),
        2,
        "a missing lng and an invalid precision are each one CLI-visible problem"
    );
    assert_eq!(gaz.get("Osaka").unwrap().coords, None);
    assert_eq!(gaz.get("Nara").unwrap().precision, crate::vault::places::Precision::Country);
}

/// Task A6: every remaining `log::warn!` in `derive_terms`/`claimed_key`
/// renamed to `log_warn_problem!`, so a claimed term with no member pages
/// (a likely typo between the claim and the name authors actually wrote)
/// counts toward `--strict`'s exit code, not just the log stream.
#[test]
fn unclaimed_typo_diagnostic_counts_as_a_cli_problem() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let kinds = vec![crate::build::terms::TermKind {
        key: "authors".to_string(),
        fields: vec!["author".to_string()],
        title: "Authors".to_string(),
        is_place: false, parents: Default::default(),
    }];
    let mut docs = vec![crate::build::types::ParsedDocument {
        url_path: "about/kane/index.html".to_string(),
        title: "Kane".to_string(),
        author_page: Some(moss_core::terms::TermClaim::UseTitle),
        ..Default::default()
    }];
    crate::build::terms::derive_terms(&mut docs, kinds);
    assert_eq!(take_cli_problems(), 1, "a claim with zero member pages is a --strict problem");
}

/// `link_terms_in_bylines`'s guard 6, second half: a declared name that
/// never appears in any byline row warns once — the typo-catcher this
/// guard exists for.
#[test]
fn declared_name_absent_from_any_byline_row_warns() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let kinds = vec![crate::build::terms::TermKind {
        key: "authors".to_string(),
        fields: vec!["author".to_string()],
        title: "Authors".to_string(),
        is_place: false, parents: Default::default(),
    }];
    let mut docs = vec![crate::build::types::ParsedDocument {
        url_path: "posts/a/index.html".to_string(),
        title: "A".to_string(),
        author: vec!["Kaneda".to_string()],
        byline: vec!["文｜某人".to_string()],
        ..Default::default()
    }];
    let index = crate::build::terms::derive_terms(&mut docs, kinds);
    crate::build::terms::link_terms_in_bylines(&mut docs, &index);
    assert_eq!(
        take_cli_problems(), 1,
        "a declared name absent from every byline row warns once"
    );
}

/// A place name is never credited in a byline (a place-typed kind gets its
/// own automatic place line instead), so it must never become a guard-6
/// candidate: neither linked into a byline row nor warned about when it is
/// absent from one. A same-shaped person name, absent from the same byline,
/// still warns exactly as `declared_name_absent_from_any_byline_row_warns`
/// pins — this test only adds the located field beside it and checks the
/// count does NOT grow to 2.
#[test]
fn place_names_never_warn_or_link_in_bylines() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let kinds = vec![
        crate::build::terms::TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
            is_place: false, parents: Default::default(),
        },
        crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true, parents: Default::default(),
        },
    ];
    let mut docs = vec![crate::build::types::ParsedDocument {
        url_path: "posts/a/index.html".to_string(),
        title: "A".to_string(),
        author: vec!["Kaneda".to_string()],
        location: vec!["Kyoto".to_string()],
        byline: vec!["文｜某人".to_string()],
        ..Default::default()
    }];
    let index = crate::build::terms::derive_terms(&mut docs, kinds);
    crate::build::terms::link_terms_in_bylines(&mut docs, &index);
    assert_eq!(
        take_cli_problems(), 1,
        "only the person name warns; the place name is excluded at the source, not merely unlinked"
    );
    assert_eq!(
        docs[0].byline[0], "文｜某人",
        "the byline row is untouched — neither name appears in it, so neither ever links"
    );
}

/// `place_names_never_warn_or_link_in_bylines` only proves a place name
/// warns/links correctly when it is ABSENT from the byline row — it can't
/// tell "correctly excluded" from "would have linked if only the place were
/// there too". This test puts the place name literally in the row, beside a
/// person name: the person still becomes a markdown link (later rendered as
/// an `<a>` by the byline HTML pipeline this test doesn't reach), but the
/// place name is never wrapped — the `is_place` exclusion drops it from the
/// candidate list before `find_bounded` ever looks for it in the row, so a
/// literal "Kyoto" sitting right next to a linked name stays plain text.
#[test]
fn place_name_present_in_a_byline_row_is_left_unlinked() {
    let _guard = PROBLEMS_TEST_LOCK.lock().unwrap();
    take_cli_problems();
    let kinds = vec![
        crate::build::terms::TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
            is_place: false, parents: Default::default(),
        },
        crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true, parents: Default::default(),
        },
    ];
    let mut docs = vec![crate::build::types::ParsedDocument {
        url_path: "posts/a/index.html".to_string(),
        title: "A".to_string(),
        author: vec!["Kaneda".to_string()],
        location: vec!["Kyoto".to_string()],
        byline: vec!["文｜Kaneda, Kyoto".to_string()],
        ..Default::default()
    }];
    let index = crate::build::terms::derive_terms(&mut docs, kinds);
    crate::build::terms::link_terms_in_bylines(&mut docs, &index);
    assert_eq!(take_cli_problems(), 0, "both names are physically present in the row; neither warns");
    assert_eq!(
        docs[0].byline[0], "文｜[Kaneda](/authors/kaneda/), Kyoto",
        "the person name links; the place name, though sitting right beside it, is never wrapped in a link"
    );
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

/// The agent guidance says which level a `moss build` prints at when `MOSS_LOG_LEVEL` is
/// unset. It said `info` (the desktop app's release level) for as long as nothing tied it
/// to `headless_log_level`, and an agent that believed it looked for timing lines that only
/// appear when they are asked for. The default itself is pinned below.
#[test]
fn the_debugging_guide_says_a_build_defaults_to_warn() {
    let guide = include_str!("../assets/skills/moss/references/debugging.md");
    assert!(guide.contains("`warn` is the default"), "the guide does not name the default level:\n{guide}");
    assert!(!guide.contains("defaults to `info`"), "the guide gives the desktop app's default as a build's");
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
