//! A clean build says what it covered: one closing line with the page count,
//! the partials it skipped and the problem count, so "zero problems" cannot be
//! mistaken for "nothing checked". A problem (the author can fix it in the
//! folder) fails `--strict`; a note is advice, is tallied on the line, and never
//! fails it. Run through the real CLI.

use std::process::{Command, Stdio};

#[test]
fn a_clean_strict_build_ends_with_what_it_built() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("about.md"), "---\ntitle: About\n---\n\nAbout us.\n").unwrap();
    std::fs::write(site.join("blurb.md"), "---\ntitle: Blurb\npartial: true\n---\n\nShared words.\n").unwrap();
    std::fs::write(site.join("footer.md"), "Closing words.\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build must be clean under --strict:\n{stderr}");
    assert!(stderr.contains("Built 2 pages (2 skipped) · 0 problems"), "{stderr}");
}

fn strict_build(index: &str) -> (std::process::Output, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), index).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out, stderr)
}

#[test]
fn a_dead_link_is_a_note_and_does_not_fail_strict() {
    let (out, stderr) = strict_build("---\ntitle: Home\n---\n\n[gone](/nowhere/)\n");
    assert!(stderr.contains("[NOTE]") && stderr.contains("Dead links"), "{stderr}");
    assert!(!stderr.contains("[WARN]"), "{stderr}");
    assert!(stderr.contains("Built 1 page · 0 problems · 1 note"), "{stderr}");
    assert_eq!(out.status.code(), Some(0), "a note must not fail --strict:\n{stderr}");
}

#[test]
fn an_unreadable_date_is_a_warn_problem_that_fails_strict() {
    let (out, stderr) = strict_build("---\ntitle: Home\ndate: sometime in spring\n---\n\nWelcome.\n");
    assert!(stderr.contains("[WARN]") && stderr.contains("invalid date format"), "{stderr}");
    assert!(stderr.contains("Built 1 page · 1 problem"), "{stderr}");
    assert_eq!(out.status.code(), Some(1), "an unreadable date must fail --strict:\n{stderr}");
}

#[test]
fn a_problem_the_author_can_fix_fails_strict() {
    let (out, stderr) = strict_build("---\ntitle: Home\n---\n\n<img src=\"a.png\">\n");
    assert!(stderr.contains("[WARN]") && stderr.contains("raw <img>"), "{stderr}");
    assert!(stderr.contains("Built 1 page · 1 problem"), "{stderr}");
    assert_eq!(out.status.code(), Some(1), "--strict must fail on a problem:\n{stderr}");
}

/// A partial is never emitted, so its address may be a real page's. The debug
/// build's source-mapping check must not report the real page as unmapped.
#[test]
fn a_partial_sharing_a_pages_address_does_not_trip_the_source_mapping_check() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("team.md"), "---\ntitle: Team\npartial: true\n---\n\nShared words.\n").unwrap();
    std::fs::create_dir(site.join("team")).unwrap();
    std::fs::write(site.join("team/index.md"), "---\ntitle: The team\n---\n\nWho we are.\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .env("MOSS_LOG_LEVEL", "info")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("Built 2 pages (1 skipped)"), "the partial and the page must both be seen: {stderr}");
    assert!(!stderr.contains("safety net"), "{stderr}");
}

/// Advice about a page that builds fine (an image with no alt text, a timed
/// event with no `timezone:`) is a note: printed, tallied, never a --strict failure.
#[test]
fn advice_about_a_working_page_is_a_note_not_a_strict_failure() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("pic.md"), "---\ntitle: Pic\n---\n\n![](logo.png)\n").unwrap();
    std::fs::write(site.join("logo.png"), [137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,2,0,0,0,144,119,83,222,0,0,0,12,73,68,65,84,120,156,99,248,207,192,0,0,3,1,1,0,201,254,146,239,0,0,0,0,73,69,78,68,174,66,96,130]).unwrap();
    std::fs::write(site.join("talk.md"), "---\ntitle: Talk\nstart: 2026-11-01 18:00\n---\n\nA talk.\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "advice must not fail --strict:\n{stderr}");
    assert!(stderr.contains("[NOTE]") && stderr.contains("has no alt text"), "{stderr}");
    assert!(stderr.contains("0 problems"), "{stderr}");
}

#[test]
fn a_raw_style_block_in_a_page_is_a_note_naming_the_page() {
    let (out, stderr) = strict_build("---\ntitle: Home\n---\n\nWelcome.\n\n<style>.moss-prose { color: red; }</style>\n");
    assert!(
        stderr.contains("[NOTE]") && stderr.contains("'index.md'") && stderr.contains("<style>") && stderr.contains("overrides the theme"),
        "{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "a note must not fail --strict:\n{stderr}");
}

/// The guide's way to keep a working file out of the site: a name starting
/// with a dot is never built, where a draft still is.
#[test]
fn a_dot_prefixed_file_is_not_built_but_a_draft_is() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join(".scratch.md"), "---\ntitle: Scratch\n---\n\nWorking.\n").unwrap();
    std::fs::write(site.join("drafty.md"), "---\ntitle: Drafty\ndraft: true\n---\n\nDraft body.\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins"])
        .env("MOSS_HOME", moss_home.path())
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let built = site.join(".moss/build.nosync/current");
    assert!(built.join("drafty/index.html").exists(), "a draft still builds into the preview output");
    assert!(!built.join("scratch").exists() && !built.join(".scratch").exists(), "a dot-prefixed file is never built");
}
