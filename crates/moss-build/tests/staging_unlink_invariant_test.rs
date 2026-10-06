//! Sync test: nothing in the build unlinks or renames away a path without a
//! reason the scan can see.
//!
//! Staging is the tree the preview serves. A `lifecycle::SweepPermit` proves a
//! build may unlink from it — the preview is parked on a generation holding
//! everything staging showed, and no other build's workers are writing — but a
//! permit only constrains the functions that ask for one. Before this scan, four
//! unlinks nobody had routed through the rule were found by reading: a symlink
//! sweep in the background asset worker, a remove-then-recreate of every
//! preserved alias on every build, and two temp-sibling sweeps that could take a
//! concurrent writer's temp mid-rename.
//!
//! A `remove_file(` / `remove_dir_all(` / `remove_dir(` / `rename(` call in the
//! scanned roots passes only if
//! 1. its enclosing `fn`'s signature names `SweepPermit`;
//! 2. the enclosing `(file, fn)` is a named exemption ([`EXEMPT_FNS`]);
//! 3. the file is `build/io_utils.rs`, the write primitive; or
//! 4. an `// allow:unlink <reason>` marker sits on the line or one of the two
//!    before it, saying why the path is not under staging or why the call
//!    cannot remove a served entry (a rename into place).

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;
#[path = "support/rust_scan.rs"]
mod rust_scan;
use rust_scan::{cfg_test_lines, walk_rust_files};

/// Relative to the crate's `src/`.
const SOURCE_ROOTS: &[&str] = &["build.rs", "build", "moss_paths.rs", "ops"];

/// `io_stop`'s Discard unlinks a dataless staged file: it is unservable already,
/// and the next build re-renders it.
const EXEMPT_FNS: &[(&str, &str)] = &[("build/outcome.rs", "io_stop")];

const PRIMITIVE: &str = "build/io_utils.rs";
const MARKER: &str = "allow:unlink";
// `remove_output_dir_all(` is the io_utils door for directories; routing a
// removal through it must not hide the removal from this scan.
const UNLINK_CALLS: &[&str] = &["remove_file(", "remove_dir_all(", "remove_dir(", "remove_output_dir_all("];

#[derive(Debug)]
struct Hit {
    file: String,
    line: usize,
    func: String,
    text: String,
}

/// `line` with string and char literals and a trailing `//` comment blanked, so
/// braces and call names inside them are not read as code.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => {
                    in_str = false;
                    out.push('"');
                }
                _ => out.push(' '),
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push('"');
            }
            '/' if chars.peek() == Some(&'/') => break,
            '\'' => {
                // A char literal ('{', '\'', '\n') or a lifetime ('a).
                let rest: String = chars.clone().take(3).collect();
                if let Some(escaped) = rest.strip_prefix('\\') {
                    if let Some(end) = escaped.find('\'') {
                        for _ in 0..end + 2 {
                            chars.next();
                        }
                    }
                    out.push_str("' '");
                } else if rest.chars().nth(1) == Some('\'') {
                    chars.next();
                    chars.next();
                    out.push_str("' '");
                } else {
                    out.push('\'');
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn is_unlink_call(code: &str) -> bool {
    if UNLINK_CALLS.iter().any(|p| code.contains(p)) {
        return true;
    }
    code.match_indices("rename(").any(|(i, _)| {
        !code[..i].chars().last().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The name a `fn` item declares on this line, if it declares one.
fn fn_name(code: &str) -> Option<String> {
    let idx = code.split_whitespace().position(|w| w == "fn")?;
    let name = code.split_whitespace().nth(idx + 1)?;
    let name: String = name.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

/// Every unlink in `file` that none of the four rules licenses. `rel` is the
/// path relative to `src/`.
fn scan_file(rel: &str, content: &str) -> Vec<Hit> {
    let lines: Vec<&str> = content.lines().collect();
    let in_test = cfg_test_lines(&lines);
    // (name, signature names SweepPermit, brace depth its body opened at)
    let mut stack: Vec<(String, bool, usize)> = Vec::new();
    // A signature still being read, up to its `{` or `;`.
    let mut pending: Option<(String, String)> = None;
    let mut depth = 0usize;
    let mut hits = Vec::new();
    for (idx, raw) in lines.iter().enumerate() {
        let code = code_of(raw);
        if let Some(name) = fn_name(&code) {
            pending = Some((name, String::new()));
        }
        if let Some((_, sig)) = pending.as_mut() {
            let end = code.find(['{', ';']).unwrap_or(code.len());
            sig.push_str(&code[..end]);
        }
        let is_call = !in_test[idx] && is_unlink_call(&code) && fn_name(&code).is_none();
        if is_call {
            let (func, permitted) = stack
                .last()
                .map(|(name, permit, _)| (name.clone(), *permit))
                .unwrap_or_else(|| ("<top level>".to_string(), false));
            let exempt = rel == PRIMITIVE || EXEMPT_FNS.iter().any(|(f, n)| *f == rel && *n == func);
            let marked = (idx.saturating_sub(2)..=idx).any(|i| lines[i].contains(MARKER));
            if !(permitted || exempt || marked) {
                hits.push(Hit { file: rel.to_string(), line: idx + 1, func, text: raw.trim().to_string() });
            }
        }
        for c in code.chars() {
            match c {
                '{' => {
                    depth += 1;
                    if let Some((name, sig)) = pending.take() {
                        stack.push((name, sig.contains("SweepPermit"), depth));
                    }
                }
                '}' => {
                    if stack.last().is_some_and(|(_, _, d)| *d == depth) {
                        stack.pop();
                    }
                    depth = depth.saturating_sub(1);
                }
                ';' => {
                    // A bodiless declaration (`fn f();` in a trait).
                    if pending.as_ref().is_some_and(|(_, sig)| !sig.contains('(') || sig.contains(')')) {
                        pending = None;
                    }
                }
                _ => {}
            }
        }
    }
    hits
}

fn scan(src: &Path, roots: &[&str]) -> (Vec<Hit>, usize) {
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        walk_rust_files(&src.join(root), &mut |path| {
            let rel = path.strip_prefix(src).unwrap_or(path).to_string_lossy().replace('\\', "/");
            if rel.ends_with("_tests.rs") {
                return;
            }
            scanned += 1;
            if let Ok(content) = fs::read_to_string(path) {
                hits.extend(scan_file(&rel, &content));
            }
        });
    }
    (hits, scanned)
}

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn every_unlink_in_the_build_is_permitted_exempt_or_explained() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (hits, scanned) = scan(&src_dir(), SOURCE_ROOTS);
    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);
    if hits.is_empty() {
        return;
    }
    let mut msg = String::from(
        "Found an unlink or rename-away with no permit, exemption or reason.\n\n\
         Staging is the tree the preview serves. Either take `&lifecycle::SweepPermit` in the \
         enclosing fn signature (the caller must hold one), or add `// allow:unlink <reason>` on \
         the line or up to two lines above, saying why the path is not under staging or why the \
         call cannot remove a served entry.\n\nUnlinks:\n",
    );
    for hit in &hits {
        msg.push_str(&format!("  - src/{}:{} (in fn {}): {}\n", hit.file, hit.line, hit.func, hit.text));
    }
    panic!("{msg}");
}

/// The one-shot build's reclaim is the one permit minted without a park, so it
/// must stay at the one call site that proves nothing reads staging again.
#[test]
fn a_final_build_permit_is_minted_in_one_place() {
    let mut callers = Vec::new();
    walk_rust_files(&src_dir(), &mut |path| {
        let rel = path.strip_prefix(src_dir()).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if rel == "build/lifecycle.rs" {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else { return };
        for (idx, line) in content.lines().enumerate() {
            if code_of(line).contains("final_build_permit(") {
                callers.push(format!("src/{rel}:{}", idx + 1));
            }
        }
    });
    assert_eq!(callers.len(), 1, "final_build_permit( must have exactly one caller: {callers:?}");
}

/// A file outside the roots that names the build's staging tree (`MossPaths`'s
/// `.staging_dir()`, or a `stage_dir`) and unlinks something is a file the scan
/// cannot see.
#[test]
fn no_file_outside_the_roots_both_names_staging_and_unlinks() {
    let mut outside = Vec::new();
    walk_rust_files(&src_dir(), &mut |path| {
        let rel = path.strip_prefix(src_dir()).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let in_roots = SOURCE_ROOTS.iter().any(|r| rel == *r || rel.starts_with(&format!("{r}/")));
        if in_roots || rel.ends_with("_tests.rs") {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else { return };
        let lines: Vec<&str> = content.lines().collect();
        let in_test = cfg_test_lines(&lines);
        let code: Vec<String> =
            lines.iter().enumerate().filter(|(i, _)| !in_test[*i]).map(|(_, l)| code_of(l)).collect();
        let names_staging = code.iter().any(|l| l.contains(".staging_dir(") || l.contains("stage_dir"));
        if names_staging && code.iter().any(|l| is_unlink_call(l)) {
            outside.push(format!("src/{rel}"));
        }
    });
    assert!(outside.is_empty(), "add these files to SOURCE_ROOTS: {outside:?}");
}

/// Each rule, injected on its own, so the scan cannot pass by matching nothing.
#[test]
fn the_scan_resolves_permits_per_fn_and_markers_per_line() {
    let count = |rel: &str, src: &str| scan_file(rel, src).len();

    assert_eq!(count("build/x.rs", "fn f(p: &Path) {\n    let _ = std::fs::remove_file(p);\n}\n"), 1);
    assert_eq!(
        count(
            "build/x.rs",
            "pub(crate) fn f(\n    stage: &Path,\n    permit: &SweepPermit,\n) -> usize {\n    let _ = std::fs::remove_file(stage);\n    0\n}\n",
        ),
        0,
        "a multi-line signature naming SweepPermit permits"
    );
    assert_eq!(
        count(
            "build/x.rs",
            "fn a(_p: &SweepPermit) {}\nfn b(p: &Path) {\n    let _ = std::fs::remove_file(p);\n}\n",
        ),
        1,
        "a permit in an earlier fn licenses nothing in a later one"
    );
    assert_eq!(
        count("build/x.rs", "fn f(p: &Path) {\n    // allow:unlink too far\n\n\n    let _ = std::fs::remove_file(p);\n}\n"),
        1,
        "a marker three lines up does not count"
    );
    assert_eq!(
        count("build/x.rs", "fn f(p: &Path) {\n    // allow:unlink a temp this fn minted\n    let _ = std::fs::remove_file(p);\n}\n"),
        0
    );
    assert_eq!(
        count("build/x.rs", "fn prod() {}\n#[cfg(test)]\nmod tests {\n    fn t(p: &Path) {\n        std::fs::remove_file(p).unwrap();\n    }\n}\n"),
        0,
        "test code is not scanned"
    );
    assert_eq!(count("build/x.rs", "fn f(a: &Path, b: &Path) {\n    std::fs::rename(a, b).unwrap();\n}\n"), 1);
    assert_eq!(count("build/x.rs", "fn f() {\n    let s = \"remove_file(\";\n    t.try_rename(x);\n}\n"), 0);
    assert_eq!(
        count("build/outcome.rs", "fn io_stop(p: &Path) {\n    let _ = std::fs::remove_file(p);\n}\nfn other(p: &Path) {\n    let _ = std::fs::remove_file(p);\n}\n"),
        1,
        "the exemption is per fn, not per file"
    );
    assert_eq!(
        count("build/x.rs", "fn outer(_p: &SweepPermit) {\n    fn inner(p: &Path) {\n        let _ = std::fs::remove_file(p);\n    }\n}\n"),
        1,
        "a nested fn is its own fn"
    );
}
