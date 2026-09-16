//! Sync test: no raw file write inside this crate's build tree may land in the
//! regenerable output tree without going through `build::io_utils`.
//!
//! Twin of `src-tauri/tests/output_write_invariant_test.rs` (desktop repo) —
//! that file's `SOURCE_ROOTS` had narrowed to only
//! `../open/crates/moss-build/src`, so this is the wholly-open half moved
//! here verbatim, minus the desktop-relative path math (class A per
//! docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md).
//!
//! ## Why this exists
//!
//! `.moss/build/` lives inside the vault, so the sync client is free to evict
//! moss's own output. `std::fs::write` opens with `O_WRONLY|O_CREAT|O_TRUNC`,
//! and `O_TRUNC` *requires* materialization — against a cloud-evicted
//! (`SF_DATALESS`) destination, under the process-wide fail-fast policy, it
//! returns `EDEADLK`. Measured in moss#964 §1, alongside the two facts that
//! make a fix possible: `unlink` and `rename` over a dataless file both
//! succeed, because neither touches data extents.
//!
//! ADR-043 states the rule: **under `.moss/build/`, dataless is absent**, and
//! every write into that tree goes through `build::io_utils`, which writes to a
//! temp sibling and `rename(2)`s it into place.
//!
//! ## The marker convention
//!
//! Raw writes are not banned — plenty of writes in `src/build/` do not land in
//! the output tree at all. Every one MUST carry an
//! `// allow:raw_write <reason>` marker on the same line or the immediately
//! preceding line, and the reason must say why the destination is not
//! regenerable output.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;

/// What gets scanned: this crate's own build tree.
const SOURCE_ROOTS: &[&str] = &["src"];

/// Files exempt from the scan. `io_utils.rs` IS the primitive.
const EXEMPT_FILES: &[&str] = &["io_utils.rs"];

/// Call patterns that open a destination with `O_TRUNC`, directly or through a
/// third-party wrapper.
const WRITE_CALL_PATTERNS: &[&str] = &[
    "fs::write(",
    "fs::copy(",
    "File::create(",
    "save_png(",
    "OpenOptions::new(",
    "File::options(",
];

const MARKER: &str = "allow:raw_write";

fn is_call_site_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("*") {
        return false;
    }
    WRITE_CALL_PATTERNS.iter().any(|p| line.contains(p))
}

/// Line numbers (0-indexed) that sit inside a `#[cfg(test)]` item.
fn cfg_test_lines(lines: &[&str]) -> Vec<bool> {
    let mut skipped = vec![false; lines.len()];
    let mut idx = 0;
    while idx < lines.len() {
        if !lines[idx].contains("#[cfg(test)]") {
            idx += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut opened = false;
        let mut j = idx;
        while j < lines.len() {
            skipped[j] = true;
            let mut ended = false;
            for ch in lines[j].chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth = depth.saturating_sub(1),
                    ';' if !opened => ended = true,
                    _ => {}
                }
            }
            if ended || (opened && depth == 0) {
                break;
            }
            j += 1;
        }
        idx = j + 1;
    }
    skipped
}

/// Every unmarked raw-write call site under `SOURCE_ROOTS`, as
/// `(path, 1-indexed line, source)`.
fn scan(root: &Path, crate_root: &Path) -> Vec<(PathBuf, usize, String)> {
    let mut violations = Vec::new();
    walk_rust_files(root, &mut |path| {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with("_tests.rs") || EXEMPT_FILES.contains(&name) {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        let lines: Vec<&str> = content.lines().collect();
        let in_test = cfg_test_lines(&lines);
        for (idx, line) in lines.iter().enumerate() {
            if in_test[idx] || !is_call_site_line(line) {
                continue;
            }
            let marked = line.contains(MARKER)
                || (idx > 0 && lines[idx - 1].contains(MARKER))
                || (idx > 1 && lines[idx - 2].contains(MARKER));
            if !marked {
                violations.push((path.to_path_buf(), idx + 1, line.trim().to_string()));
            }
        }
        let _ = crate_root;
    });
    violations
}

#[test]
fn raw_writes_in_the_build_tree_carry_allow_marker() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    let mut scanned = 0usize;
    for rel in SOURCE_ROOTS {
        let root = crate_root.join(rel);
        assert!(root.exists(), "{} not found — did it move?", root.display());
        walk_rust_files(&root, &mut |_| scanned += 1);
        violations.extend(scan(&root, &crate_root));
    }
    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);
    if violations.is_empty() {
        return;
    }

    let mut msg = String::from(
        "Found raw file write(s) in a stage-writing module without an `// allow:raw_write <reason>` marker.\n\n\
         `.moss/build/` lives inside the vault, so the sync client evicts moss's own output.\n\
         `fs::write` / `fs::copy` / `File::create` open the destination with O_TRUNC, which\n\
         requires materialization and fails EDEADLK against a cloud-evicted file (moss#964, ADR-043).\n\n\
         Recovery:\n  \
         1. If the destination is under `.moss/build/`: use `crate::build::io_utils`\n     \
            (`write_output`, `write_output_if_changed`, `copy_output`).\n  \
         2. If it is NOT regenerable output — a temp file you just minted, `.moss/cache`,\n     \
            user state under `.moss/data` or in the vault — add a same-line or preceding-line\n     \
            `// allow:raw_write <reason>` saying which.\n\n\
         Violations:\n",
    );
    for (path, line_num, line) in &violations {
        let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();
        msg.push_str(&format!("  - {}:{}: {}\n", rel, line_num, line));
    }
    panic!("{}", msg);
}

/// Two-injection falsifier: the scanner must go red on a NEW unmarked write,
/// and it must not be satisfiable by a marker that is merely somewhere in
/// the file.
#[test]
fn scanner_catches_an_unmarked_write_and_accepts_a_marked_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("unmarked.rs"),
        "fn f(p: &Path) {\n    std::fs::write(p, b\"x\").unwrap();\n}\n",
    )
    .unwrap();
    let hits = scan(root, root);
    assert_eq!(hits.len(), 1, "unmarked write must be reported: {hits:?}");
    assert_eq!(hits[0].1, 2, "line number must point at the call");

    fs::write(
        root.join("unmarked.rs"),
        "fn f(p: &Path) {\n    // allow:raw_write a temp we just minted\n    std::fs::write(p, b\"x\").unwrap();\n}\n",
    )
    .unwrap();
    assert!(scan(root, root).is_empty(), "a preceding-line marker must satisfy the scanner");

    fs::write(
        root.join("unmarked.rs"),
        "// allow:raw_write not this one\nfn a() {}\nfn b() {}\nfn c() {}\nfn f(p: &Path) {\n    std::fs::write(p, b\"x\").unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(scan(root, root).len(), 1, "a distant marker must not count");

    fs::write(
        root.join("with_tests.rs"),
        "fn prod() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        std::fs::write(\"x\", b\"y\").unwrap();\n    }\n}\n",
    )
    .unwrap();
    fs::write(root.join("thing_tests.rs"), "fn t() { std::fs::write(\"x\", b\"y\").unwrap(); }\n").unwrap();
    fs::remove_file(root.join("unmarked.rs")).unwrap();
    assert!(scan(root, root).is_empty(), "test code must be exempt");

    fs::write(
        root.join("post_test_block.rs"),
        "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn prod(p: &Path) {\n    std::fs::write(p, b\"x\").unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(
        scan(root, root).len(),
        1,
        "the cfg(test) skip must end at the closing brace, not run to EOF"
    );
    fs::remove_file(root.join("post_test_block.rs")).unwrap();

    fs::write(
        root.join("brace_less.rs"),
        "#[cfg(test)]\n#[path = \"x_tests.rs\"]\nmod tests;\nfn prod(p: &Path) {\n    std::fs::write(p, b\"x\").unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(
        scan(root, root).len(),
        1,
        "a brace-less `mod tests;` must end at its semicolon, not run to EOF"
    );
    fs::remove_file(root.join("brace_less.rs")).unwrap();

    fs::write(
        root.join("open_options.rs"),
        "fn f(p: &Path) {\n    let f = OpenOptions::new().write(true).truncate(true).open(p).unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(scan(root, root).len(), 1, "a truncating OpenOptions open must be caught");
}

/// Walks a directory, or visits a single `.rs` file.
fn walk_rust_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    if dir.is_file() {
        visit(dir);
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue;
            }
            walk_rust_files(&path, visit);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            visit(&path);
        }
    }
}
