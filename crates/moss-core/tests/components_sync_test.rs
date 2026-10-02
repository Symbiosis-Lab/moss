//! Source-scanner sync tests for the moss component contract, open half.
//!
//! Checks that only need files inside this repository (site.css,
//! `COMPONENTS`, `CUSTOM_PROPS`, `tokens.json`, and this crate's own source
//! tree) belong here.
//!
//! 1. `css_selectors_match_components_table`.
//! 2. `emitter_classes_match_components_table` — scans
//!    `crates/moss-core/src` for class-emission call sites against the
//!    `COMPONENTS` table.
//! 3. `every_escape_hatch_is_declared` — reads every shipped `.css` under
//!    `moss-build/src/assets/css` plus `CUSTOM_PROPS` and `tokens.json`.
//!
//! Not carried here: `site_js_selectors_match_components_table` (needs the
//! frontend site tree), `every_declared_custom_prop_is_read` and
//! `nav_width_is_a_custom_prop_not_a_token` (a separate concern from this
//! file's three checks).

use moss_core::contract::components::{Status, COMPONENTS};
use moss_core::contract::custom_props::CUSTOM_PROPS;
use moss_core::contract::tokens::load_tokens;
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

/// `moss-grid`'s `data-columns` description must agree with what site.css
/// actually does at the mobile breakpoint, not contradict it. The
/// description used to say "there is no mobile collapse to a single
/// column", while `@media (max-width: 768px) { .moss-grid[data-columns] {
/// grid-template-columns: 1fr; } }` has always collapsed a plain grid there
/// — the sibling `data-fits` attr's own description already said as much
/// ("the same breakpoint that would otherwise collapse a plain grid to one
/// column").
#[test]
fn moss_grid_data_columns_description_agrees_with_the_stylesheets_mobile_collapse() {
    let workspace_root = workspace_root();
    let css_path = workspace_root.join(SITE_CSS_DIR).join("site.css");
    let css = fs::read_to_string(&css_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", css_path.display(), e));

    assert!(
        css.contains(
            "@media (max-width: 768px) {\n  .moss-grid[data-columns] {\n    grid-template-columns: 1fr;"
        ),
        "site.css no longer collapses .moss-grid[data-columns] to one column below 768px — this \
         test and the moss-grid data-columns description in components.rs both assume it does; \
         update both together if the stylesheet's own behavior changed"
    );

    let entry = COMPONENTS
        .iter()
        .find(|c| c.class == "moss-grid")
        .expect("moss-grid must be in COMPONENTS");
    let data_columns = entry
        .data_attrs
        .iter()
        .find(|a| a.name == "data-columns")
        .expect("moss-grid must declare data-columns");

    assert!(
        !data_columns.description.contains("no mobile collapse"),
        "data-columns description still claims no mobile collapse, but the stylesheet collapses \
         it below 768px: {}",
        data_columns.description
    );
    assert!(
        data_columns.description.contains("768px"),
        "data-columns description should name the real mobile-collapse breakpoint: {}",
        data_columns.description
    );
}

/// A `var(--moss-x, <fallback>)` read is the syntactic shape custom_props.rs's
/// own doc names as an escape hatch ("read from a stylesheet as
/// `var(--moss-foo, <fallback>)`"), as opposed to a design token, which a
/// rule reads bare (`var(--moss-color-text)`) because it is always defined at
/// `:root`. A read with a fallback that names no `CUSTOM_PROPS` entry is a
/// hook no agent can find — exactly what the table exists to prevent.
///
/// A bare `var(--moss-x)` that names neither a token nor a `CUSTOM_PROPS`
/// entry is deliberately not scanned for. Unlike the fallback form, a bare
/// read carries no default value to document, so it is not this table's
/// "hook no agent can find" failure mode — and two bare reads sampled from
/// site.css confirm why: `--moss-colophon-lead` and `--moss-ease-subscribe`
/// are each declared with `--name: value;` earlier in the same file and
/// consumed bare later, ordinary same-file custom-property plumbing, not a
/// theme hook. Catching only the undeclared subset of bare reads would need
/// a second scanner tracking every in-stylesheet `--name:` declaration (and
/// every build-time-injected inline `style="--name:…"` the Rust renderer
/// emits, e.g. a per-cover `--moss-cover-color`) to avoid flagging that
/// entire ordinary category — out of scope for the fallback-hook table this
/// test polices.
const CUSTOM_PROP_ALLOWLIST: &[(&str, &str)] = &[];

#[test]
fn every_escape_hatch_is_declared() {
    let workspace_root = workspace_root();
    let css_dir = workspace_root.join(SITE_CSS_DIR);
    let sheets: Vec<PathBuf> = css_files(&css_dir);
    assert!(
        sheets.len() >= 4,
        "expected at least site.css plus partials under {}, found {}",
        css_dir.display(),
        sheets.len()
    );

    let var_with_fallback = Regex::new(r"var\(\s*(--moss-[A-Za-z0-9_-]+)\s*,").unwrap();
    let mut read: HashSet<String> = HashSet::new();
    for path in &sheets {
        let css = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let css = strip_css_comments(&css);
        for cap in var_with_fallback.captures_iter(&css) {
            read.insert(cap[1].to_string());
        }
    }
    assert!(!read.is_empty(), "expected at least one var(--moss-x, fallback) read across the shipped stylesheets");

    let declared: HashSet<&str> = CUSTOM_PROPS.iter().map(|p| p.name).collect();
    let tokens = load_tokens().expect("tokens.json parses");
    let token_names: HashSet<String> = tokens
        .groups
        .iter()
        .flat_map(|g| g.entries.iter())
        .map(|e| format!("--{}", e.name))
        .collect();
    let allowlisted: HashSet<&str> = CUSTOM_PROP_ALLOWLIST.iter().map(|(k, _)| *k).collect();

    let mut undeclared: Vec<&String> = read
        .iter()
        .filter(|name| {
            !declared.contains(name.as_str())
                && !token_names.contains(name.as_str())
                && !allowlisted.contains(name.as_str())
        })
        .collect();
    undeclared.sort();

    assert!(
        undeclared.is_empty(),
        "stylesheets read these as var(--x, fallback) (an escape hatch) but no CUSTOM_PROPS \
         entry or token declares them:\n{}\n\n\
         Add a CustomProp to src/contract/custom_props.rs with the default copied verbatim \
         from the call site, or add it to CUSTOM_PROP_ALLOWLIST with a one-line reason if it \
         is genuinely internal plumbing rather than a theme hook.",
        undeclared.iter().map(|s| format!("  - {s}")).collect::<Vec<_>>().join("\n")
    );
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
