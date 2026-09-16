//! Phase 2E v5 PR6 — `<img>` contract test, wikilinks half.
//!
//! Open-half twin of `all_parser_new_ext_sites_enable_wikilinks` in
//! `src-tauri/tests/img_contract_test.rs` (desktop repo) — that test is
//! "mostly a uniform `Parser::new_ext`/`ENABLE_WIKILINKS` source scan over
//! `src-tauri/src` + `open/crates/moss-core/src` + `open/crates/moss-build/src`"
//! (class B per
//! docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md),
//! split by root. This is moss-build's own copy, scoped to `src`; moss-core
//! carries the twin (plus the wholly-open `shared_parser_options_grants_wikilinks`
//! sub-check) for its own `src`. The desktop half keeps scanning
//! `src-tauri/src`.
//!
//! Phase 3 PR2 (`ee102b7d1`) flipped `ENABLE_WIKILINKS` at every parser
//! site, retiring the Stage-1 wikilink rewriter in favor of native
//! pulldown-cmark `LinkType::WikiLink` events. A future site that forgets
//! the flag would silently drop wikilink processing in its path.

use std::fs;
use std::path::PathBuf;

fn rust_files(dir: &std::path::Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect()
}

#[test]
fn all_parser_new_ext_sites_enable_wikilinks() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut sites: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut violations: Vec<String> = Vec::new();

    for path in rust_files(&src) {
        let Ok(source) = fs::read_to_string(&path) else { continue };
        let lines: Vec<&str> = source.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if !line.contains("Parser::new_ext(") {
                continue;
            }

            let window_start = idx.saturating_sub(300);
            let window: &[&str] = &lines[window_start..=idx];
            let has_wikilinks = window.iter().any(|l| {
                let is_comment = l.trim_start().starts_with("//");
                !is_comment && (l.contains("ENABLE_WIKILINKS") || l.contains("parser_options"))
            });
            let has_allow_marker = window.iter().any(|l| l.contains("allow:no-wikilinks"));

            let status = if has_wikilinks {
                "WIKILINKS"
            } else if has_allow_marker {
                "allow:no-wikilinks"
            } else {
                "MISSING"
            };
            sites.push((path.clone(), idx + 1, status.to_string()));

            if status == "MISSING" {
                violations.push(format!(
                    "  {}:{} — Parser::new_ext( without nearby \
                     `Options::ENABLE_WIKILINKS` insertion or \
                     `// allow:no-wikilinks <reason>` marker.\n    line: {}",
                    path.display(),
                    idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "P3 violations — `Parser::new_ext(` site without a wikilink stance.\n\n{}",
        violations.join("\n")
    );

    assert!(
        sites.len() >= 3,
        "Parser::new_ext audit found only {} call site(s) in this crate — \
         expected at least 3. Source moved, walker is broken, or all parsers were removed.",
        sites.len()
    );
}
