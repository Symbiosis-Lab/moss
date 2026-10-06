//! Sync test: every `fs::hard_link` invocation must carry a `// allow:hard_link <reason>` marker.
//!
//! Open-half twin of the desktop app's `hardlink_invariant_test.rs` — that
//! file's `SOURCE_ROOTS` scans `src`, `tests` (desktop) plus
//! `../open/crates/moss-build/src`, `../open/crates/moss-build/tests`
//! independently, with no cross-root comparison. This twin runs the same
//! lint over this crate's own `src` and `tests`; the desktop half keeps
//! scanning its own `src`/`tests`.
//!
//! ## Why this exists
//!
//! moss has been bitten three times by an interaction between APFS hardlinks
//! and iCloud Drive's "optimize storage" eviction. Files that land in a
//! user-visible output path (`.moss/build.nosync/site/`, `.moss/build.nosync/staging/`, and
//! anything the CAS hardlinks INTO) must NEVER be created via
//! `fs::hard_link`. Use `fs::copy` instead.
//!
//! ## The marker convention
//!
//! `fs::hard_link` is not banned outright — test scaffolds legitimately
//! synthesize the pre-fix shared-inode state to exercise recovery paths.
//! Every legitimate call MUST be paired with a `// allow:hard_link <reason>`
//! comment on the same line OR the immediately preceding line.

use std::fs;
use std::path::PathBuf;

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;

/// Roots to scan. Tests live alongside source, so this covers test scaffolds too.
const SOURCE_ROOTS: &[&str] = &["src", "tests"];

const HARDLINK_CALL_PATTERNS: &[&str] = &["fs::hard_link(", "hard_link("];

fn is_call_site_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("*") {
        return false;
    }
    HARDLINK_CALL_PATTERNS.iter().any(|p| line.contains(p))
}

fn has_allow_marker(this_line: &str, prev_line: Option<&str>) -> bool {
    let marker = "allow:hard_link";
    this_line.contains(marker) || prev_line.map_or(false, |p| p.contains(marker))
}

#[test]
fn hard_link_calls_carry_allow_marker() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut violations: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut scanned = 0usize;

    for root in SOURCE_ROOTS {
        let root_path = crate_root.join(root);
        assert!(root_path.exists(), "{} not found — did it move?", root_path.display());
        walk_rust_files(&root_path, &mut |_| scanned += 1);
        walk_rust_files(&root_path, &mut |path| {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n == "hardlink_invariant_test.rs")
            {
                return;
            }

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => return,
            };
            let lines: Vec<&str> = content.lines().collect();
            for (idx, line) in lines.iter().enumerate() {
                if !is_call_site_line(line) {
                    continue;
                }
                let prev = if idx > 0 { Some(lines[idx - 1]) } else { None };
                if !has_allow_marker(line, prev) {
                    violations.push((path.to_path_buf(), idx + 1, line.trim().to_string()));
                }
            }
        });
    }

    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);

    if !violations.is_empty() {
        let mut msg = String::from(
            "Found `fs::hard_link` call(s) without an `// allow:hard_link <reason>` marker.\n\n\
             moss must NOT create hardlinks at user-visible output paths — iCloud Drive's\n\
             \"optimize storage\" eviction zeros files via their shared inode.\n\n\
             Recovery:\n  \
             1. If the call is in PRODUCTION code: replace with `fs::copy`.\n  \
             2. If the call is in TEST code synthesizing the pre-fix state: add a same-line\n     \
                or preceding-line marker `// allow:hard_link <reason>`.\n\nViolations:\n",
        );
        for (path, line_num, line) in &violations {
            let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();
            msg.push_str(&format!("  - {}:{}: {}\n", rel, line_num, line));
        }
        panic!("{}", msg);
    }
}

fn walk_rust_files(dir: &std::path::Path, visit: &mut dyn FnMut(&std::path::Path)) {
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
