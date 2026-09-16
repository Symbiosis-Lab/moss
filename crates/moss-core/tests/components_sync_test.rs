//! Source-scanner sync tests for the moss component contract, open half.
//!
//! Open-half twin of two checks in `src-tauri/tests/components_sync_test.rs`
//! (desktop repo) (class B per
//! docs/archive/2026-09-16-boundary-gates-remeasured-for-dependency-model.md):
//!
//! 1. `css_selectors_match_components_table` — desktop's version "only
//!    touches open files (site.css + `COMPONENTS`) and could move wholesale."
//!    Moved here outright.
//! 2. `emitter_classes_match_components_table` — desktop's `EMITTER_ROOTS`
//!    scans `src-tauri/src/build` and `open/crates/moss-core/src`
//!    independently for class-emission call sites against the same
//!    `COMPONENTS` table. This is the moss-core half; the desktop half keeps
//!    scanning `src-tauri/src/build`.
//!
//! Not carried here: `site_js_selectors_match_components_table` (reads
//! `frontend/site`, a desktop-tree path outside this row's classification),
//! `every_escape_hatch_is_declared` / `every_declared_custom_prop_is_read` /
//! `nav_width_is_a_custom_prop_not_a_token` (not named by this row — though
//! they also only read open files, they are a separate concern this
//! landing-order step does not cover).

use moss_core::contract::components::{Status, COMPONENTS};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn files_with_ext(dir: &std::path::Path, ext: &str) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
        .collect()
}

fn rust_files(dir: &std::path::Path) -> Vec<PathBuf> {
    files_with_ext(dir, "rs")
}

fn css_files(dir: &std::path::Path) -> Vec<PathBuf> {
    files_with_ext(dir, "css")
}

/// Classes moss emits that are deliberately NOT part of the site contract.
const EMITTER_ALLOWLIST: &[(&str, &str)] = &[];

/// This crate's own emitter root.
const EMITTER_ROOT: &str = "src";

/// Files / paths to skip even within the scan root (substring matches).
const SKIP_PATHS: &[&str] = &["contract/components.rs", "tests/fixtures", "_tests.rs"];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn is_valid_css_class(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.ends_with('-') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[test]
fn emitter_classes_match_components_table() {
    let class_re = Regex::new(r#"class="([^"]+)""#).unwrap();
    let root = workspace_root();
    let mut emitted: HashSet<String> = HashSet::new();

    let scan_dir = root.join(EMITTER_ROOT);
    for path in rust_files(&scan_dir) {
        let path_str = path.to_string_lossy().replace('\\', "/");
        if SKIP_PATHS.iter().any(|skip| path_str.contains(skip)) {
            continue;
        }
        let Ok(raw_content) = fs::read_to_string(&path) else { continue };
        let content = unescape_quotes(&strip_line_comments(&strip_cfg_test_blocks(&raw_content)));
        for cap in class_re.captures_iter(&content) {
            for class in cap[1].split_whitespace() {
                if !is_valid_css_class(class) {
                    continue;
                }
                if EMITTER_ALLOWLIST.iter().any(|(c, _)| *c == class) {
                    continue;
                }
                emitted.insert(class.to_string());
            }
        }
    }

    let declared: HashSet<String> = COMPONENTS.iter().map(|e| e.class.to_string()).collect();

    let missing_from_table: Vec<&String> = emitted.difference(&declared).collect();
    if !missing_from_table.is_empty() {
        let list = missing_from_table
            .iter()
            .map(|s| format!("  - {}", s))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "Emitter source emits classes not declared in COMPONENTS:\n{}\n\nAdd them to src/contract/components.rs",
            list
        );
    }
}

/// Path to the default theme CSS, relative to the sibling `moss-build` crate.
const SITE_CSS_DIR: &str = "../moss-build/src/assets/css";

const CSS_SELECTOR_ALLOWLIST: &[(&str, &str)] = &[];

#[test]
fn css_selectors_match_components_table() {
    let workspace_root = workspace_root();

    let css_dir = workspace_root.join(SITE_CSS_DIR);
    let mut sheets: Vec<PathBuf> = css_files(&css_dir);
    sheets.sort();
    assert!(
        sheets.len() >= 4,
        "expected at least site.css plus partials under {}, found {}",
        css_dir.display(),
        sheets.len()
    );
    let mut css = String::new();
    for path in &sheets {
        css.push('\n');
        css.push_str(
            &fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e)),
        );
    }

    let css_no_comments = strip_css_comments(&css);

    let selector_re = Regex::new(r"\.moss-[a-z][a-z0-9-]*").unwrap();
    let mut selectors: HashSet<String> = HashSet::new();
    for m in selector_re.find_iter(&css_no_comments) {
        let raw = m.as_str();
        let class = &raw[1..];
        if !is_valid_css_class(class) {
            continue;
        }
        selectors.insert(class.to_string());
    }

    let declared_live: HashSet<&str> = COMPONENTS
        .iter()
        .filter(|e| e.status != Status::Retired)
        .map(|e| e.class)
        .collect();
    let declared_retired: HashSet<&str> = COMPONENTS
        .iter()
        .filter(|e| e.status == Status::Retired)
        .map(|e| e.class)
        .collect();
    let allowlisted: HashSet<&str> = CSS_SELECTOR_ALLOWLIST.iter().map(|(k, _)| *k).collect();

    let mut missing: Vec<String> = Vec::new();
    let mut only_retired: Vec<String> = Vec::new();
    for class in &selectors {
        if allowlisted.contains(class.as_str()) {
            continue;
        }
        if declared_live.contains(class.as_str()) {
            continue;
        }
        if declared_retired.contains(class.as_str()) {
            only_retired.push(class.clone());
            continue;
        }
        missing.push(class.clone());
    }

    if !only_retired.is_empty() {
        only_retired.sort();
        eprintln!(
            "WARNING: site.css has selectors only present in COMPONENTS as Retired (dead CSS — schedule removal):\n  - {}",
            only_retired.join("\n  - ")
        );
    }

    if !missing.is_empty() {
        missing.sort();
        let list = missing
            .iter()
            .map(|s| format!("  - .{}", s))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "site.css has `.moss-*` selectors with no contract entry (or no live entry — only Retired):\n{}\n\n\
             Add a ComponentEntry for each class to `src/contract/components.rs` with a substantive \
             `description`, correct `parent`, and `Status::Confirmed` (or `Emerging` for in-flight \
             work). If a class is genuinely a non-COMPONENTS surface, add it to \
             `CSS_SELECTOR_ALLOWLIST` with a one-line justification.",
            list
        );
    }
}

fn unescape_quotes(src: &str) -> String {
    src.replace("\\\"", "\"")
}

fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| if line.trim_start().starts_with("//") { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_cfg_test_blocks(src: &str) -> String {
    let marker = "#[cfg(test)]";
    let mut out = String::with_capacity(src.len());
    let mut rest = src;

    while let Some(marker_pos) = rest.find(marker) {
        out.push_str(&rest[..marker_pos]);
        rest = &rest[marker_pos + marker.len()..];

        let next_brace = rest.find('{');
        if let Some(semi) = rest.find(';') {
            if next_brace.map_or(true, |b| semi < b) {
                rest = &rest[semi + 1..];
                continue;
            }
        }
        let brace_start = match next_brace {
            Some(pos) => pos,
            None => {
                out.push_str(rest);
                return out;
            }
        };

        let bytes = rest.as_bytes();
        let mut depth = 0usize;
        let mut end = brace_start;
        for (i, &b) in bytes[brace_start..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = brace_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        rest = &rest[end + 1..];
    }

    out.push_str(rest);
    out
}

fn strip_css_comments(css: &str) -> String {
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(css.len());
    let mut copy_from = 0;
    let mut i = 0;
    while i < len {
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            out.push_str(&css[copy_from..i]);
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            copy_from = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&css[copy_from..len.min(css.len())]);
    out
}
