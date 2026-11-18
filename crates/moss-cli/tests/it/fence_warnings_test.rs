//! A stray or unclosed `:::` fence is a build warning that names the line of
//! the author's file, with the frontmatter and any blank lines after it
//! counted. Built through the real CLI, because the line arithmetic lives in
//! the markdown pipeline, not in the parser.

use std::path::Path;
use std::process::{Command, Stdio};

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// Build a one-page site and return (exit success, stdout + stderr).
fn build(index_md: &str, strict: bool) -> (bool, String) {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    write(&site, "index.md", index_md);
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

#[test]
fn a_stray_closer_names_its_file_line_whatever_follows_the_frontmatter() {
    // The closer is on file line 6 + the number of blank lines after `---`.
    for blanks in 0..=2 {
        let md = format!("---\ntitle: T\n---\n{}text\n:::\n", "\n".repeat(blanks));
        let line = 5 + blanks;
        let (_, log) = build(&md, false);
        assert!(
            log.contains(&format!("on line {line} closes nothing")),
            "{blanks} blank line(s) after frontmatter, expected line {line}:\n{log}"
        );
    }
}

#[test]
fn an_unclosed_opener_names_its_file_line() {
    let (_, log) = build("---\ntitle: T\n---\n\nintro\n\n:::grid 2\na\n", false);
    assert!(log.contains("on line 7 is never closed"), "{log}");
}

#[test]
fn strict_fails_on_a_stray_closer_but_not_on_one_inside_html_or_math() {
    let (ok, log) = build("---\ntitle: T\n---\n\ntext\n:::\n", true);
    assert!(!ok, "a stray closer must fail --strict:\n{log}");
    for body in ["<div>\n:::\n</div>\n", "$$\n:::\n$$\n"] {
        let (ok, log) = build(&format!("---\ntitle: T\n---\n\n{body}"), true);
        assert!(ok, "{body:?} must not fail --strict:\n{log}");
    }
}
