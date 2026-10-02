use super::*;
use std::io::Write;

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

#[test]
fn cli_warn_increments_the_problem_counter() {
    cli_warn!("unknown shortcode: {}", "foo");
    cli_warn!("dangling link: {}", "bar");
    assert_eq!(take_cli_problems(), 2);
}

/// `link_terms_in_bylines`'s guard 6, first half: a name declared through
/// two different fields (author: and editor:, both landing in one declared
/// kind) still links exactly once from a byline row, but warns once too —
/// this counter is the only thing that can see the warning actually fired.
#[test]
fn duplicate_name_in_one_row_warns() {
    let kinds = vec![crate::build::terms::TermKind {
        key: "people".to_string(),
        fields: vec!["author".to_string(), "editor".to_string()],
        title: "People".to_string(),
        is_place: false, parents: Default::default(), explorer: None, line: None,
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

/// Confirms `vault::places::parse_gazetteer`'s diagnostics go
/// through `log_warn_problem!`, the CLI-visible macro, not a bare
/// `log::warn!` — so a malformed `.moss/places.toml` entry counts toward
/// `--strict`'s exit code, the same guarantee every other build diagnostic
/// carries.
#[test]
fn gazetteer_diagnostics_count_as_cli_problems() {
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

/// Every remaining `log::warn!` in `derive_terms`/`claimed_key`
/// renamed to `log_warn_problem!`, so a claimed term with no member pages
/// (a likely typo between the claim and the name authors actually wrote)
/// counts toward `--strict`'s exit code, not just the log stream.
#[test]
fn unclaimed_typo_diagnostic_counts_as_a_cli_problem() {
    let kinds = vec![crate::build::terms::TermKind {
        key: "authors".to_string(),
        fields: vec!["author".to_string()],
        title: "Authors".to_string(),
        is_place: false, parents: Default::default(), explorer: None, line: None,
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
    let kinds = vec![crate::build::terms::TermKind {
        key: "authors".to_string(),
        fields: vec!["author".to_string()],
        title: "Authors".to_string(),
        is_place: false, parents: Default::default(), explorer: None, line: None,
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
    let kinds = vec![
        crate::build::terms::TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
            is_place: false, parents: Default::default(), explorer: None, line: None,
        },
        crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true, parents: Default::default(), explorer: None, line: None,
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
    let kinds = vec![
        crate::build::terms::TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
            is_place: false, parents: Default::default(), explorer: None, line: None,
        },
        crate::build::terms::TermKind {
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            is_place: true, parents: Default::default(), explorer: None, line: None,
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
    crate::ops::serve::events::CarrierReporter.report(
        &crate::build::progress::PipelineEvent::PluginNeedsConnection {
            plugin: "matters".into(),
            project_path: "/tmp/vault".into(),
        },
    );
    assert_eq!(take_cli_problems(), 1, "a plugin that cannot run is a --strict problem");
}

/// A build-time phase that overruns its timing budget (`phase.rs`) is
/// observability only: it must never count toward `--strict`'s problem
/// summary, which is what turns a slow-but-successful build into one that
/// reads as failed. Sleeping past a tight budget (`native_process_spawn`'s
/// 100ms) exercises the real `Drop` path, not just the pure classifier.
#[test]
fn phase_budget_overrun_does_not_count_as_a_cli_problem() {
    {
        let _p = crate::build::phase::PhaseTrace::start("native_process_spawn");
        std::thread::sleep(std::time::Duration::from_millis(250)); // > 2x the 100ms budget
    }
    assert_eq!(
        take_cli_problems(),
        0,
        "a budget overrun is a performance note, not a --strict problem"
    );
}

/// Tests run on parallel threads of one process, and dozens of them drive code
/// that logs an intended problem. A problem logged on another thread must not
/// reach this thread's count, or one test's warning lands inside another
/// test's assertion and fails it at random.
#[test]
fn a_problem_logged_on_another_thread_is_not_counted_here() {
    std::thread::spawn(|| cli_warn!("a problem a concurrent test logged")).join().unwrap();
    assert_eq!(take_cli_problems(), 0, "another thread's problem leaked into this test's count");
}

#[test]
fn take_cli_problems_resets_to_zero() {
    cli_warn!("one problem");
    assert_eq!(take_cli_problems(), 1, "first read sees the problem");
    assert_eq!(take_cli_problems(), 0, "second read starts clean, as --watch needs");
}

#[test]
fn build_result_summary_is_silent_at_zero_problems() {
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

/// A `redirects.json` entry whose key has a leading slash is not a valid
/// redirect source (`ServedPath::from_source` rejects an absolute path). The
/// rejection has to reach `--strict` the same way every other build
/// diagnostic does, and a well-formed entry sitting beside the bad one must
/// still get its stub — one mistyped line must not silently drop every
/// redirect after it in the file.
#[test]
fn a_redirect_entry_with_a_leading_slash_counts_as_a_cli_problem() {
    let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let tmp = tempfile::TempDir::new_in(&test_tmp).unwrap();
    let moss_dir = tmp.path().join(".moss");
    let data_dir = moss_dir.join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(moss_dir.join("deploy")).unwrap();
    let output_dir = tmp.path().join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // A mistyped entry (leading slash) beside a well-formed one.
    let mut redirects = std::collections::BTreeMap::new();
    redirects.insert("/old-page/".to_string(), "new-page/".to_string());
    redirects.insert("good-page/".to_string(), "new-page/".to_string());
    crate::build::feeds::redirects::save_redirects(&data_dir, &redirects).unwrap();

    let paths = crate::moss_paths::MossPaths::from_moss_dir(moss_dir);
    let current_map = crate::build::scan::article_map::ArticleMap::new();
    let mut pending =
        crate::build::manifest::PendingManifest::new(crate::types::content::SiteHashes::default());

    crate::build::feeds::redirects::emit_redirect_stubs(
        &paths,
        &current_map,
        &output_dir,
        &mut pending,
    )
    .expect("one bad entry must not abort the whole site's redirects");

    assert_eq!(
        take_cli_problems(),
        1,
        "the leading-slash entry is exactly one --strict problem; the good entry beside it is not"
    );

    let sealed = pending.seal();
    assert!(
        sealed.files().contains_key("good-page/index.html"),
        "a well-formed entry beside a rejected one must still get its stub: {:?}",
        sealed.files().keys().collect::<Vec<_>>()
    );
    assert!(
        !sealed.files().keys().any(|k| k.contains("old-page")),
        "a rejected entry must not silently produce a stub of its own"
    );
}

/// `generate_favicons` rasterizes an author-supplied SVG through usvg/resvg
/// built WITHOUT raster-image decoding (`Cargo.toml`'s `default-features =
/// false` for both crates) — an embedded `<image>` element parses but never
/// draws, so the icon renders blank with nothing but resvg's own generic
/// "decoding was disabled" warning to go on, and that warning would fire once
/// per rasterized size (three times) if left unsuppressed. Detecting the
/// cause up front and reporting it once, by name, replaces three uncounted,
/// unattributed lines with one an author can act on.
#[test]
fn a_favicon_with_an_embedded_bitmap_is_one_counted_problem() {
    let dir = tempfile::tempdir().unwrap();
    let svg_path = dir.path().join("favicon.svg");
    std::fs::write(
        &svg_path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><image href="data:image/png;base64,AAAA" width="10" height="10"/></svg>"#,
    )
    .unwrap();

    crate::build::site_meta::favicon::generate_favicons(&svg_path, dir.path())
        .expect("a blank-rendering favicon is not a build failure");

    assert_eq!(
        take_cli_problems(),
        1,
        "one embedded bitmap must be exactly one --strict problem, not zero, and not one per rasterized size"
    );
}

/// The literal `"[warn] "` prefix baked into a `cli_warn!` call site was half
/// of the double-print bug: pairing it with a preceding `log::warn!` of the
/// SAME message printed the one warning twice on a real CLI run — once as
/// `[WARN] module::path: message` (through the installed `HeadlessLogger`)
/// and once as `[warn] message` (`cli_warn!` writing straight to stderr) —
/// while only the `cli_warn!` half counted toward `--strict`, so the closing
/// count never matched the number of lines printed. `log_warn_problem!`
/// already prints and counts a warning in one call; a site that still
/// hand-writes `"[warn] "` beside its own `log::warn!` is re-deriving that
/// formatting by hand and duplicating the line when it does. Confirmed live
/// with a misplaced `style.css`: the pair printed both lines and the summary
/// still read "moss: 3 problems", not 4.
#[test]
fn no_call_site_hand_writes_a_warn_prefix_for_cli_warn() {
    let src = include_str!("render/blocking.rs");
    assert!(
        !src.contains("cli_warn!(\"[warn] "),
        "a `cli_warn!(\"[warn] ...\")` call site duplicates whatever `log::warn!` \
         already printed for the same event — use `log_warn_problem!` instead, \
         which prints and counts the warning exactly once"
    );
}

/// usvg/resvg (the SVG renderer behind favicons, OG cards and math PNGs) log
/// through the same global `log` facade moss's own code does. Recognizing
/// their target namespace is the first half of attributing or dropping such
/// a warning correctly — see the two `log_line_for` tests below for the
/// policy this feeds.
#[test]
fn dependency_svg_renderer_warnings_are_recognized_by_target_namespace() {
    let usvg_record = log::Record::builder()
        .level(log::Level::Warn)
        .target("usvg::parser::units")
        .args(format_args!("Invalid 'font-size' value: 'huge'."))
        .build();
    assert!(is_dependency_svg_renderer_warning(&usvg_record));

    let resvg_record = log::Record::builder()
        .level(log::Level::Warn)
        .target("resvg::render")
        .args(format_args!("some resvg warning"))
        .build();
    assert!(is_dependency_svg_renderer_warning(&resvg_record));

    // A moss-sourced warning, even one that happens to be about SVG handling,
    // must never be swept up by the check — only the DEPENDENCY's own
    // target namespace matches.
    let moss_record = log::Record::builder()
        .level(log::Level::Warn)
        .target("moss_build::build::render::blocking")
        .args(format_args!("Found style.css at project root"))
        .build();
    assert!(!is_dependency_svg_renderer_warning(&moss_record));

    // A crate whose name merely starts with the same letters (not the exact
    // dependency, and not a `dep::` submodule of it) must not match — the
    // check is a namespace match, not a substring one.
    let lookalike_record = log::Record::builder()
        .level(log::Level::Warn)
        .target("usvgutil")
        .args(format_args!("unrelated warning"))
        .build();
    assert!(!is_dependency_svg_renderer_warning(&lookalike_record));
}

fn usvg_font_size_warning() -> log::Record<'static> {
    log::Record::builder()
        .level(log::Level::Warn)
        .target("usvg::parser::units")
        .args(format_args!("Invalid 'font-size' value: 'huge'."))
        .build()
}

/// A usvg/resvg warning fired while [`suppress_renderer_warnings`] is held —
/// rendering one of moss's OWN generated SVGs (the OG card, a math-fallback
/// PNG) — names no moss file, so it must vanish from both the printed list
/// and the `--strict` count rather than sit there uncounted.
#[test]
fn a_renderer_warning_during_moss_own_svg_render_is_dropped() {
    let _suppress = suppress_renderer_warnings();
    assert!(
        log_line_for(&usvg_font_size_warning()).is_none(),
        "a renderer warning while suppressed must print nothing and count nothing"
    );
}

/// The complementary case: the SAME warning, with no suppression in effect —
/// exactly what happens when `site_meta::favicon::generate_favicons`
/// rasterizes an author's SVG — is the one usvg/resvg call site that IS
/// attributable, and must be printed, counted, and named as the favicon.
#[test]
fn a_renderer_warning_during_favicon_rasterization_counts_and_is_attributed() {
    assert!(
        !renderer_warnings_suppressed(),
        "no favicon render wraps itself in suppress_renderer_warnings"
    );
    let (line, counts) =
        log_line_for(&usvg_font_size_warning()).expect("an unsuppressed renderer warning must print");
    assert!(counts, "an unsuppressed renderer warning is exactly a favicon warning, and must count");
    assert_eq!(line, "[WARN] assets/favicon.svg: Invalid 'font-size' value: 'huge'.");
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
