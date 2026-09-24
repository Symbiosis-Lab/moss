//! The open crates (`moss-core`, `moss-build`, `moss-cli`) must build and
//! test from `open/` alone — the public repo the F-track flip exports has
//! nothing above `open/`. F5's dry run found three tests that violated this
//! by joining a `..`-laden literal onto `env!("CARGO_MANIFEST_DIR")` (which
//! is the *crate* root, e.g. `open/crates/moss-build`) and climbing out of
//! `open/` entirely to read desktop-only fixtures (a Playwright nav-island
//! fixture, the frontend design system's index page, and the app's own
//! tauri config); all three moved into the desktop app's own tests (calling
//! the open crates' public API or reading their source text, same as the F1
//! plugin-manifest precedent).
//!
//! This is the guard that stops the class from coming back. It resolves
//! every `..`-containing quoted string literal against the base its call
//! site actually uses — `CARGO_MANIFEST_DIR` (the crate root) if that's what
//! the line joins against, otherwise the source file's own directory, which
//! is what `include_str!`/`include_bytes!`/`include_dir!` resolve relative
//! to — and fails only if the resolved path lands above `open/`. Two known
//! limits, both measured 2026-09-13: a bare runtime path (no
//! `CARGO_MANIFEST_DIR`) is resolved from the file's directory although
//! cargo's test cwd is the crate root, which is more lenient for nested
//! files — the tree's five bare `../../../` literals are all traversal-defense
//! fixtures, not reads, and a stricter base flags four of them; and the TS
//! packages under `open/packages` are out of scope, since they climb via
//! `import.meta.url` under vitest, not cargo. A dumb
//! "contains `../../../`" substring check was tried first and rejected: the
//! tree already has ~15 legitimate 3+-`..` literals (`include_str!` reaching
//! a crate's own `assets/` from three directories down, `target/test-tmp`
//! helpers, traversal-defense test fixtures) that all resolve safely inside
//! `open/`, and gating on the substring alone would have flagged every one
//! of them.

use std::path::{Component, Path, PathBuf};

fn open_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `open/crates/moss-cli`; two levels up is `open`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("moss-cli sits under open/crates")
        .to_path_buf()
}

fn rust_files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let entry = entry.expect("readable dir entry");
        let path = entry.path();
        if path.is_dir() {
            rust_files_under(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Every quoted string literal on the line that contains at least one `..`
/// component. Deliberately crude (splits on `"`, ignores escapes) — good
/// enough to find path-shaped literals without a full Rust lexer, and
/// over-matching here only means checking a few extra literals, never
/// missing one.
fn dotdot_literals_in(line: &str) -> Vec<&str> {
    line.split('"')
        .skip(1)
        .step_by(2)
        .filter(|lit| lit.split('/').any(|seg| seg == ".."))
        .collect()
}

/// Lexically joins `literal`'s components onto `base`'s (no filesystem
/// access — the target path need not exist), the same normalization
/// `Path::join` plus `..`-collapsing would do. Returns `false` if a `..`
/// tries to pop `base[0]` (`"open"`) itself, which is exactly the escape
/// this guard exists to catch.
fn resolves_inside_open(base: &[String], literal: &str) -> bool {
    let mut stack: Vec<&str> = base.iter().map(String::as_str).collect();
    for seg in literal.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                // `stack[0]` is always `"open"` (see callers) — popping it
                // is the escape itself, not a state to keep resolving from,
                // so it must be refused here rather than only detected once
                // the stack later goes empty (a literal that later pushes
                // components back on, like `../../../etc/passwd`, would
                // otherwise leave the stack non-empty and hide the escape).
                if stack.len() <= 1 {
                    return false;
                }
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    true
}

#[test]
fn no_source_file_climbs_above_open() {
    let root = open_root();
    let crates_dir = root.join("crates");
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", crates_dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    crate_dirs.sort();
    assert!(!crate_dirs.is_empty(), "expected at least one crate under {}", crates_dir.display());

    let mut offenders: Vec<String> = Vec::new();

    for crate_dir in &crate_dirs {
        let crate_name = crate_dir.file_name().unwrap().to_string_lossy().into_owned();
        let manifest_relative: Vec<String> = vec!["open".into(), "crates".into(), crate_name];

        let mut files = Vec::new();
        rust_files_under(&crate_dir.join("src"), &mut files);
        rust_files_under(&crate_dir.join("tests"), &mut files);

        for file in files {
            let text = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));

            // The file's own dir, expressed the same open-relative way, for
            // include_str!/include_bytes!/include_dir!-style resolution.
            let file_dir_relative: Vec<String> = {
                let rel = file.strip_prefix(&root).expect("file is under open/");
                let mut v = vec!["open".to_string()];
                v.extend(
                    rel.parent()
                        .unwrap_or(Path::new(""))
                        .components()
                        .filter_map(|c| match c {
                            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                            _ => None,
                        }),
                );
                v
            };

            for (i, line) in text.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue; // doc/line comments carry markdown links, not real fs reads
                }
                let base = if line.contains("CARGO_MANIFEST_DIR") {
                    &manifest_relative
                } else {
                    &file_dir_relative
                };
                for literal in dotdot_literals_in(line) {
                    if !resolves_inside_open(base, literal) {
                        offenders.push(format!("{}:{}", file.display(), i + 1));
                        break;
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these lines resolve above open/, which cannot exist in the standalone public export:\n{}",
        offenders.join("\n")
    );
}
