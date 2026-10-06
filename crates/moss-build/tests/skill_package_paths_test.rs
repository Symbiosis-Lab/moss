//! The shipped agent skill may not cite paths that only exist in moss's repo.
//!
//! Twin of the desktop app's `skill_package_paths_test.rs` — that file only
//! ever read this crate's own `src/assets/skills/moss/`, so it is a
//! wholly-open test moved here verbatim minus the desktop-relative path
//! math.
//!
//! `src/assets/skills/moss/` is `include_dir!`-embedded and copied
//! **verbatim** into a user's site on every full build (`cli::agents::sync`) —
//! `.claude/skills/moss/`, `.cursor/rules/moss.mdc`, `.moss/agents/SKILL.md`.
//! Nothing rewrites paths on the way out, so a sentence that reads correctly to
//! a moss contributor becomes a dead pointer for the agent that actually
//! receives it.

use std::path::{Path, PathBuf};

fn skill_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/assets/skills/moss")
}

/// Repo-root directories that have no meaning inside a user's site folder.
const REPO_DIRS: &[&str] = &[
    "crates/",
    "src-tauri/",
    "frontend/",
    "scripts/",
    "packages/",
    "plugins/",
    "tests/",
    ".githooks/",
];

/// Is the `docs/` at `idx` a repo-relative path rather than part of a URL?
fn is_bare_docs_path(text: &str, idx: usize) -> bool {
    let token_start = text[..idx]
        .rfind(|c: char| c.is_whitespace() || "([`\"'<".contains(c))
        .map(|p| p + 1)
        .unwrap_or(0);
    let token_before = &text[token_start..idx];
    !token_before.contains(".com/") && !token_before.contains("://")
}

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn shipped_skill_cites_no_repo_relative_paths() {
    let dir = skill_dir();
    assert!(dir.is_dir(), "skill package missing at {}", dir.display());

    let files = markdown_files(&dir);
    assert!(
        files.len() >= 5,
        "file discovery looks broken — found {} markdown files under {}",
        files.len(),
        dir.display()
    );

    let mut bad: Vec<String> = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let name = file
            .strip_prefix(&dir)
            .unwrap_or(file)
            .display()
            .to_string();

        for (lineno, line) in text.lines().enumerate() {
            for repo_dir in REPO_DIRS {
                if line.contains(repo_dir) {
                    bad.push(format!("  {name}:{}: {repo_dir}", lineno + 1));
                }
            }
            for (idx, _) in line.match_indices("docs/") {
                if is_bare_docs_path(line, idx) {
                    bad.push(format!("  {name}:{}: docs/", lineno + 1));
                }
            }
        }
    }
    bad.sort();
    bad.dedup();

    assert!(
        bad.is_empty(),
        "the shipped agent skill cites {} repo-relative path(s):\n{}\n\n\
         This package is copied verbatim into a user's site, where none of these \
         exist. Point at something reachable from a site instead: `moss describe \
         --json` for live vocabulary, a sibling `references/*.md` in this same \
         package, or `.moss/AGENTS.md` for site-specific notes. Notes meant for \
         moss contributors belong in the moss repo, not in prose that ships.",
        bad.len(),
        bad.join("\n")
    );
}
