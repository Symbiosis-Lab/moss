//! Task 1.3 verification tests: tokens.json is the runtime source of the :root token block.
//!
//! Twin of the desktop app's `token_emit_test.rs` — that file's "Group 1"
//! tests only ever read this crate's own `src/assets/css/site.css` and are
//! moved here verbatim minus the desktop-relative path math.
//!
//! The desktop file's "Group 2" (`smoke_build_stylesheet_contains_generated_token_layer`)
//! is NOT twinned here: it drives `moss::build_sync`, a desktop-crate-only
//! entry point (`#[allow(dead_code)] // Used in tests and re-exported from
//! lib.rs for integration tests`) with no moss-build equivalent — that half
//! stays desktop-only.
//!
//! Tests assert:
//! 1. site.css SOURCE defines no `--moss-color-bg:` at :root (definition moved to generated prefix).
//! 2. site.css SOURCE does reference `var(--moss-color-bg)` (uses are retained).
//! 3. Regression guard: heading rules consume `var(--moss-font-heading-weight)`.

use std::fs;
use std::path::PathBuf;

/// Path to site.css source, relative to this crate's own root.
fn site_css_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/assets/css/site.css")
}

#[test]
fn site_css_defines_no_root_color_bg_token() {
    let css = fs::read_to_string(site_css_path()).expect("site.css must exist");

    let forbidden_definitions = [
        "--moss-color-bg:",
        "--moss-color-text:",
        "--moss-color-muted:",
        "--moss-color-surface:",
        "--moss-color-accent:",
        "--moss-color-accent-quiet:",
        "--moss-text-secondary:",
        "--moss-accent-hover:",
        "--moss-border-light:",
        "--moss-border-medium:",
        "--moss-code-background:",
        "--moss-code-border:",
        "--moss-code-accent-primary:",
        "--moss-code-accent-secondary:",
        "--moss-code-accent-tertiary:",
        "--moss-code-accent-quaternary:",
        "--moss-hl-keyword:",
        "--moss-hl-string:",
        "--moss-hl-comment:",
        "--moss-hl-number:",
        "--moss-hl-function:",
        "--moss-hl-type:",
        "--moss-hl-tag:",
        "--moss-hl-attr:",
        "--moss-hl-operator:",
        "--moss-hl-builtin:",
        "--moss-hl-meta:",
        "--moss-hl-deletion:",
        "--moss-hl-addition-bg:",
        "--moss-hl-deletion-bg:",
        "--moss-font-mono:",
        "--moss-font-weight:",
        "--moss-sidebar-width:",
        "--moss-site-max-width:",
        "--moss-font-2xs:",
        "--moss-font-xs:",
        "--moss-font-sm:",
        "--moss-font-lg:",
        "--moss-font-xl:",
        "--moss-font-heading-weight:",
        "--moss-space-xs:",
        "--moss-space-sm:",
        "--moss-space-md:",
        "--moss-space-lg:",
        "--moss-space-xl:",
        "--moss-space-2xl:",
    ];

    for def in &forbidden_definitions {
        assert!(
            !css.contains(def),
            "site.css must not contain '{}' — token definitions live in tokens.json and \
             are generated at build time. Remove the :root definition from site.css.",
            def
        );
    }
}

#[test]
fn site_css_references_var_moss_color_bg() {
    let css = fs::read_to_string(site_css_path()).expect("site.css must exist");
    assert!(
        css.contains("var(--moss-color-bg)"),
        "site.css must still reference var(--moss-color-bg) in rule blocks"
    );
}

#[test]
fn heading_rules_consume_font_heading_weight_token() {
    let css = fs::read_to_string(site_css_path()).expect("site.css must exist");

    assert!(
        css.contains("var(--moss-font-heading-weight)"),
        "site.css must reference var(--moss-font-heading-weight) in at least one heading rule. \
         The token was found to be a no-op (defined but never consumed). Wire it into the \
         h1-h6 base rule and .moss-article-title to fix the Yohaku heading-weight control."
    );

    let count = css.matches("var(--moss-font-heading-weight)").count();
    assert!(
        count >= 4,
        "Expected var(--moss-font-heading-weight) in at least 4 rules \
         (h1-h6, .moss-article-title, .moss-folder-title, .moss-card-title); \
         found {} reference(s). Some heading selectors may have regressed to a \
         hardcoded weight.",
        count
    );
}
