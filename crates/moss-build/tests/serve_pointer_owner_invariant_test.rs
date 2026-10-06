//! Sync test: the preview's served pointer has one writer, `build::lifecycle`.
//!
//! The pointer used to have four: a seed at every `run_pipeline`, a switch to
//! `current` at the top of every build, a switch to staging at render end, and
//! the desktop's `initial-build-complete` listener. None knew what the others
//! had shown, so a rebuild that started before the last render was promoted
//! moved the preview back to an older generation and 404'd the page the author
//! had just been shown, and the unordered listener could put the preview back
//! on staging after a later build had parked it — the moment that build's
//! sweep unlinked from the tree being served. `lifecycle` keeps the rule that
//! prevents both; a writer anywhere else would not.
//!
//! The desktop tree has its own half of this test.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "support/scanned_roots_nonempty.rs"]
mod support;
use support::scanned_roots_nonempty;
#[path = "support/rust_scan.rs"]
mod rust_scan;
use rust_scan::{cfg_test_lines, walk_rust_files};

const SOURCE_ROOTS: &[&str] = &["src"];

/// The owner, and the one type whose own impl is the write it wraps.
const OWNER: &str = "build/lifecycle.rs";
const CELL_TYPE_FILE: &str = "types/runtime.rs";

const WRITE_PATTERNS: &[&str] = &["switch_to(", "current_dir.write("];

fn scan(src: &Path) -> Vec<(PathBuf, usize, String)> {
    let mut hits = Vec::new();
    walk_rust_files(src, &mut |path| {
        let rel = path.strip_prefix(src).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if rel.ends_with("_tests.rs") || rel == OWNER {
            return;
        }
        let Ok(content) = fs::read_to_string(path) else {
            return;
        };
        let lines: Vec<&str> = content.lines().collect();
        let in_test = cfg_test_lines(&lines);
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if in_test[idx] || trimmed.starts_with("//") || trimmed.starts_with('*') {
                continue;
            }
            if line.contains("fn switch_to(") {
                continue;
            }
            let own_impl = rel == CELL_TYPE_FILE && line.contains("current_dir.write(");
            if !own_impl && WRITE_PATTERNS.iter().any(|p| line.contains(p)) {
                hits.push((path.to_path_buf(), idx + 1, line.trim().to_string()));
            }
        }
    });
    hits
}

#[test]
fn only_lifecycle_moves_the_served_pointer() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    let mut scanned = 0usize;
    for rel in SOURCE_ROOTS {
        let root = crate_root.join(rel);
        walk_rust_files(&root, &mut |_| scanned += 1);
        hits.extend(scan(&root));
    }
    scanned_roots_nonempty(&crate_root, SOURCE_ROOTS, scanned);
    if hits.is_empty() {
        return;
    }
    let mut msg = String::from(
        "Found a served-pointer write outside `build::lifecycle`.\n\n\
         Move the preview through `lifecycle::{adopt_server, park_for_rebuild, show_render, \
         withdraw_render}`; each keeps the rule that the preview never steps back to a \
         generation older than the render on screen.\n\nWrites:\n",
    );
    for (path, line, text) in &hits {
        let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();
        msg.push_str(&format!("  - {rel}:{line}: {text}\n"));
    }
    panic!("{msg}");
}
