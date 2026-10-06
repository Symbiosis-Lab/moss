//! Phase 2E v5 PR6 — `<img>` contract test, wikilinks half.
//!
//! Open-half twin of two tests in the desktop app's `img_contract_test.rs`.
//! That file's `all_parser_new_ext_sites_enable_wikilinks`
//! is a uniform `Parser::new_ext`/`ENABLE_WIKILINKS` source scan over
//! the desktop app's own source plus `open/crates/moss-core/src`; its
//! `shared_parser_options_grants_wikilinks` sub-check is entirely about
//! moss-core's own source and doesn't need desktop input at all.
//! `shared_parser_options_grants_wikilinks` moves here outright; the
//! wikilinks scan is split by root — this is moss-core's own copy, scoped to
//! `src`. moss-build carries the twin for its own `src`. The desktop half
//! keeps scanning its own source.
//!
//! Not carried here: `img_contract_holds_across_all_fixtures` and
//! `no_moss_prefix_titles_in_rendered_html`, which walk desktop's own
//! `tests/fixtures/*/expected` HTML and call
//! `moss::build::media::raw_img_warning` — desktop-only content and a
//! desktop-crate-only entry point respectively. Not class A/B by this row's
//! own classification (the row only names the wikilinks scan and the
//! `parser_options()` sub-check as splittable).

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect()
}

/// Phase 3 PR6 — guards every `Parser::new_ext` site in this crate against
/// forgetting `Options::ENABLE_WIKILINKS`.
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
        sites.len() >= 1,
        "Parser::new_ext audit found only {} call site(s) in this crate — \
         expected at least 1. Source moved, walker is broken, or all parsers were removed.",
        sites.len()
    );
}

/// Companion: the shared constructor must actually grant wikilinks, and
/// nothing may subtract the flag after construction anywhere in this crate.
///
/// Wholly this crate's own source — moved here outright rather than staying
/// split, per the plan doc's note that this sub-check "doesn't need desktop
/// input at all".
#[test]
fn shared_parser_options_grants_wikilinks() {
    let parser_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ast/parser.rs");
    let src = fs::read_to_string(&parser_rs)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", parser_rs.display()));

    let body_start = src
        .find("pub fn parser_options(")
        .expect("src/ast/parser.rs must define `pub fn parser_options(`");
    let body_end = src[body_start..]
        .find("\n}")
        .map(|off| body_start + off)
        .unwrap_or(src.len());
    let body = &src[body_start..body_end];

    assert!(
        body.contains("ENABLE_WIKILINKS"),
        "`parser_options` no longer inserts ENABLE_WIKILINKS. Every parser \
         site that derives its Options from this constructor just lost \
         wikilink parsing. Restore the insertion, or make the audit stop \
         accepting the constructor."
    );

    let own_src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut removals: Vec<String> = Vec::new();
    for path in rust_files(&own_src) {
        let Ok(file) = fs::read_to_string(&path) else { continue };
        for (idx, line) in file.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("remove(Options::ENABLE_WIKILINKS)") {
                removals.push(format!("  {}:{}", path.display(), idx + 1));
            }
        }
    }
    assert!(
        removals.is_empty(),
        "ENABLE_WIKILINKS is removed after construction, which defeats the \
         Parser::new_ext audit:\n{}",
        removals.join("\n")
    );
}
