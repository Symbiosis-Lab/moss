//! Sync test: no raw file read of a **vault input** in this crate may bypass
//! `build::cloud_readiness`.
//!
//! Open-half twin of the desktop app's `read_input_invariant_test.rs` —
//! that file's `SOURCE_ROOTS` mixes `src/*` (desktop) and
//! `../open/crates/moss-build/*` rows, one rule applied per path
//! independently. This twin carries only the open-half roots; the desktop
//! half keeps its own.
//!
//! ## Why this exists
//!
//! Under the process-wide dataless fail-fast policy, any read of a
//! cloud-evicted file returns `Resource deadlock avoided (os error 11)`
//! instead of blocking, and pre-Sonoma macOS evicts by *replacing* the file
//! with a hidden `.name.icloud` sibling, so an evicted input answers
//! `path.exists() == false` — every `if !path.exists() { treat as fresh }` in
//! front of a read-modify-write is therefore an erase.
//!
//! ## The marker convention
//!
//! Raw reads are not banned. Each unmarked one must carry an
//! `// allow:raw_read <reason>` marker on the same or one of the two
//! preceding lines, and the reason must say *why the path is not a vault
//! input*.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;
#[path = "support/rust_scan.rs"]
mod rust_scan;
use rust_scan::{cfg_test_lines, walk_rust_files};

/// What gets scanned: the modules that read vault inputs outside the build,
/// scoped to this crate's own root.
const SOURCE_ROOTS: &[&str] = &["src/deploy", "src/deploy.rs", "src/vault", "src/identity"];

/// Files exempt from the scan. `cloud_readiness.rs` IS the primitive.
const EXEMPT_FILES: &[&str] = &["cloud_readiness.rs"];

const READ_CALL_PATTERNS: &[&str] = &["fs::read_to_string(", "fs::read(", "File::open("];

const MARKER: &str = "allow:raw_read";

fn is_call_site_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("*") {
        return false;
    }
    READ_CALL_PATTERNS.iter().any(|p| line.contains(p))
}


fn scan(root: &Path) -> Vec<(PathBuf, usize, String)> {
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
    });
    violations
}

#[test]
fn raw_reads_outside_the_build_carry_allow_marker() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    let mut scanned = 0usize;
    for rel in SOURCE_ROOTS {
        let root = crate_root.join(rel);
        assert!(root.exists(), "{} not found — did it move?", root.display());
        walk_rust_files(&root, &mut |_| scanned += 1);
        violations.extend(scan(&root));
    }
    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);
    if violations.is_empty() {
        return;
    }

    let mut msg = String::from(
        "Found raw file read(s) without an `// allow:raw_read <reason>` marker.\n\n\
         A file inside the user's vault can be cloud-evicted.\n\n\
         Recovery:\n  \
         1. If the path is a vault input: use `crate::build::cloud_readiness`.\n  \
         2. If it is NOT a vault input — add a same-line or preceding-line\n     \
            `// allow:raw_read <reason>` saying which.\n\nViolations:\n",
    );
    for (path, line_num, line) in &violations {
        let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();
        msg.push_str(&format!("  - {}:{}: {}\n", rel, line_num, line));
    }
    panic!("{}", msg);
}

/// Two-injection falsifier: the scanner must go red on a NEW unmarked read,
/// and must not be satisfiable by a marker that is merely somewhere in the
/// file.
#[test]
fn scanner_catches_an_unmarked_read_and_accepts_a_marked_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::write(
        root.join("unmarked.rs"),
        "fn f(p: &Path) {\n    let s = std::fs::read_to_string(p).unwrap();\n}\n",
    )
    .unwrap();
    let hits = scan(root);
    assert_eq!(hits.len(), 1, "unmarked read must be reported: {hits:?}");
    assert_eq!(hits[0].1, 2, "line number must point at the call");

    fs::write(
        root.join("unmarked.rs"),
        "fn f(p: &Path) {\n    // allow:raw_read app-support dir, never synced\n    let s = std::fs::read_to_string(p).unwrap();\n}\n",
    )
    .unwrap();
    assert!(scan(root).is_empty(), "a preceding-line marker must satisfy the scanner");

    fs::write(
        root.join("unmarked.rs"),
        "// allow:raw_read not this one\nfn a() {}\nfn b() {}\nfn c() {}\nfn f(p: &Path) {\n    let s = std::fs::read_to_string(p).unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(scan(root).len(), 1, "a distant marker must not count");
    fs::remove_file(root.join("unmarked.rs")).unwrap();

    fs::write(
        root.join("with_tests.rs"),
        "fn prod() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        let s = std::fs::read_to_string(\"x\").unwrap();\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("thing_tests.rs"),
        "fn t() { let s = std::fs::read_to_string(\"x\").unwrap(); }\n",
    )
    .unwrap();
    assert!(scan(root).is_empty(), "test code must be exempt");

    fs::write(
        root.join("post_test_block.rs"),
        "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn prod(p: &Path) {\n    let s = std::fs::read_to_string(p).unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(
        scan(root).len(),
        1,
        "the cfg(test) skip must end at the closing brace, not run to EOF"
    );
    fs::remove_file(root.join("post_test_block.rs")).unwrap();

    fs::write(
        root.join("brace_less.rs"),
        "#[cfg(test)]\n#[path = \"x_tests.rs\"]\nmod tests;\nfn prod(p: &Path) {\n    let s = std::fs::read_to_string(p).unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(
        scan(root).len(),
        1,
        "a brace-less `mod tests;` must end at its semicolon, not run to EOF"
    );
    fs::remove_file(root.join("brace_less.rs")).unwrap();

    fs::write(
        root.join("opened.rs"),
        "fn f(p: &Path) {\n    let f = std::fs::File::open(p).unwrap();\n}\n",
    )
    .unwrap();
    assert_eq!(scan(root).len(), 1, "a File::open must be caught");
}
