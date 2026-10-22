//! `## Title {.class}` prints the braces into the heading and its anchor id.
//! The build says so, naming the file and the line of the author's file, and
//! `--strict` counts it. A lone `{#id}` is read as the anchor and does not
//! warn. Built through the real CLI, because the line arithmetic spans the
//! frontmatter, shortcode extraction and the pipeline.

use std::path::Path;
use std::process::{Command, Stdio};

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// Build a one-page site and return (exit success, stdout + stderr).
fn build(page: &str, strict: bool) -> (bool, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "notes/essay.md", page);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    cmd.args(["build", site.to_str().unwrap(), "--no-plugins"]);
    if strict {
        cmd.arg("--strict");
    }
    let out = cmd
        .env("MOSS_HOME", moss_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn a_heading_attribute_block_warns_with_file_line_and_remedy() {
    let (_, log) = build("---\ntitle: Essay\n---\n\nIntro.\n\n## Pictures {.pictures}\n\nText.\n", false);
    assert!(log.contains("notes/essay.md"), "{log}");
    assert!(log.contains("line 7"), "{log}");
    assert!(log.contains("printed as text"), "{log}");
    assert!(log.contains(":::{.pictures}"), "the remedy names the fenced div:\n{log}");

    // A lone `{#id}` is the anchor, so it is not warned about; mixed with a class it still is.
    let (_, log) = build("---\ntitle: Essay\n---\n\n## Origins {#a .b}\n", false);
    assert!(log.contains("line 5") && log.contains(":::{#a .b}"), "{log}");
}

#[test]
fn the_line_survives_a_shortcode_above_and_a_fenced_div_around() {
    let above = "---\ntitle: Essay\n---\n\n:::grid 2\none\n\n+++\ntwo\n\nthree\n:::\n\n## Pictures {.pictures}\n";
    let (_, log) = build(above, false);
    assert!(log.contains("line 14"), "a heading below a multi-line grid:\n{log}");

    let inside = "---\ntitle: Essay\n---\n\nIntro.\n\n:::{.wide}\n## Pictures {.pictures}\n:::\n";
    let (_, log) = build(inside, false);
    assert!(log.contains("line 8"), "a heading inside a fenced div:\n{log}");

    let in_cell = "---\ntitle: Essay\n---\n\n:::grid 2\none\n\n+++\n## Pictures {.pictures}\n:::\n";
    let (_, log) = build(in_cell, false);
    assert!(log.contains("line 9"), "a heading inside a grid cell:\n{log}");
}

#[test]
fn strict_fails_on_an_attribute_block_but_not_on_braces_that_are_text() {
    let (ok, log) = build("---\ntitle: Essay\n---\n\n## Pictures {.pictures}\n", true);
    assert!(!ok, "an attribute block must fail --strict:\n{log}");

    for heading in ["## Pictures", "## The `{x}` field", "## The `{.x}` rule", "## The field `{.x}`", "## Sets {x}", "## Two {a} and {b}", "```\n## Pictures {.pictures}\n```", "## Origins {#origins}", "## Origins {#origins contents=end}"] {
        let (ok, log) = build(&format!("---\ntitle: Essay\n---\n\n{heading}\n"), true);
        assert!(ok, "{heading:?} must not fail --strict:\n{log}");
        assert!(!log.contains("printed as text"), "{heading:?} must not warn:\n{log}");
    }
}
