//! Tests for the tokens loader.

use moss_core::contract::tokens::load_tokens;

#[test]
fn load_tokens_parses_w3c_format() {
    let tokens = load_tokens().expect("tokens.json must parse");

    // Top-level groups are present in source order (from $order field)
    let group_names: Vec<&str> = tokens.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(group_names, vec!["typography", "color", "code", "syntax", "layout", "spacing"]);
}

#[test]
fn color_group_has_accent_token() {
    let tokens = load_tokens().expect("tokens.json must parse");
    let color = tokens.groups.iter().find(|g| g.name == "color")
        .expect("color group must exist");

    let accent = color.entries.iter().find(|t| t.name == "moss-color-accent")
        .expect("moss-color-accent must exist");
    assert_eq!(accent.value, "#2d5a2d");
    assert_eq!(accent.type_hint.as_deref(), Some("color"));
}

#[test]
fn entries_are_sorted_alphabetically_within_group() {
    let tokens = load_tokens().expect("tokens.json must parse");
    let color = tokens.groups.iter().find(|g| g.name == "color")
        .expect("color group must exist");

    let names: Vec<&str> = color.entries.iter().map(|t| t.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "entries within a group must be alphabetical");
}

#[test]
fn token_value_preserves_var_references() {
    // A token whose $value references others must round-trip verbatim (no
    // resolution/inlining at load time). moss-reading-size is the example: it
    // is the base size times the per-script multiplier, and both var() names
    // have to survive the loader for the CSS to mean anything. It replaced
    // moss-nav-width here when nav-width became an opt-in (unset-by-default)
    // escape hatch — see the .main-nav fallback in site.css.
    let tokens = load_tokens().expect("tokens.json must parse");
    let typography = tokens.groups.iter().find(|g| g.name == "typography")
        .expect("typography group must exist");

    let reading_size = typography.entries.iter().find(|t| t.name == "moss-reading-size")
        .expect("moss-reading-size must exist");
    assert_eq!(
        reading_size.value,
        "calc(var(--moss-reading-size-base) * var(--moss-reading-script-scale))"
    );
}

// Error-path tests using parse_tokens helper
use moss_core::contract::tokens::parse_tokens;

#[test]
fn parse_tokens_errors_when_order_missing() {
    let input = "{\n  \"color\": {\n    \"moss-color-accent\": {\"$type\": \"color\", \"$value\": \"#000\"}\n  }\n}";
    let err = parse_tokens(input).expect_err("must fail");
    assert!(err.contains("$order"), "error should mention $order: {}", err);
}

#[test]
fn parse_tokens_errors_when_group_named_in_order_is_missing() {
    let input = "{\n  \"$order\": [\"color\", \"spacing\"],\n  \"color\": {\n    \"moss-color-accent\": {\"$type\": \"color\", \"$value\": \"#000\"}\n  }\n}";
    let err = parse_tokens(input).expect_err("must fail");
    assert!(err.contains("spacing"), "error should mention missing group: {}", err);
}

#[test]
fn parse_tokens_errors_when_entry_missing_value() {
    let input = "{\n  \"$order\": [\"color\"],\n  \"color\": {\n    \"moss-color-accent\": {\"$type\": \"color\"}\n  }\n}";
    let err = parse_tokens(input).expect_err("must fail");
    assert!(err.contains("$value"), "error should mention missing $value: {}", err);
}

/// Task 3.1: bg_colors() must return the light and dark --moss-color-bg values
/// that are emitted into `<meta name="theme-color">` at build time.
/// Dark must be #1c1914 (NOT the old wrong literal #1a1816).
#[test]
fn bg_colors_returns_token_light_and_dark() {
    use moss_core::contract::tokens::bg_colors;
    let tokens = load_tokens().expect("tokens.json must parse");
    let (light, dark) = bg_colors(&tokens);
    assert_eq!(light, "#faf8f5", "light --moss-color-bg must be #faf8f5");
    assert_eq!(dark,  "#1c1914", "dark --moss-color-bg must be #1c1914 (not the old #1a1816)");
}

use moss_core::contract::tokens::format_root_block;

#[test]
fn format_root_block_produces_expected_shape() {
    let tokens = load_tokens().expect("tokens.json must parse");
    let css = format_root_block(&tokens);

    // Group comments appear
    assert!(css.contains("/* Typography */"));
    assert!(css.contains("/* Color */"));
    assert!(css.contains("/* Layout */"));
    assert!(css.contains("/* Spacing */"));

    // Tokens are present
    assert!(css.contains("--moss-color-accent: #2d5a2d;"));
    assert!(css.contains("--moss-content-width: calc(42 * var(--moss-reading-size));"));
    assert!(css.contains("--moss-space-xs: 0.5rem;"));

    // Two-space indent
    assert!(css.contains("\n  --moss-color-accent"));

    // Group order matches tokens.json source order
    let typo_idx = css.find("/* Typography */").unwrap();
    let color_idx = css.find("/* Color */").unwrap();
    let layout_idx = css.find("/* Layout */").unwrap();
    let spacing_idx = css.find("/* Spacing */").unwrap();
    assert!(typo_idx < color_idx);
    assert!(color_idx < layout_idx);
    assert!(layout_idx < spacing_idx);

    // Alphabetical within group: accent before bg before muted before surface before text
    let accent_idx = css.find("--moss-color-accent").unwrap();
    let bg_idx = css.find("--moss-color-bg").unwrap();
    let muted_idx = css.find("--moss-color-muted").unwrap();
    let surface_idx = css.find("--moss-color-surface").unwrap();
    let text_idx = css.find("--moss-color-text").unwrap();
    assert!(accent_idx < bg_idx);
    assert!(bg_idx < muted_idx);
    assert!(muted_idx < surface_idx);
    assert!(surface_idx < text_idx);

    // Wrapped in :root { ... }
    assert!(css.starts_with(":root {\n"));
    assert!(css.trim_end().ends_with("}"));
}

#[test]
fn format_root_block_normalizes_colors_to_lowercase_hex() {
    // tokens.json should already have lowercase hex, but the formatter
    // is the layer that enforces the rule. Verify all hex values in output
    // are lowercase 6-digit.
    let tokens = load_tokens().expect("tokens.json must parse");
    let css = format_root_block(&tokens);

    for line in css.lines() {
        if let Some(hash_idx) = line.find('#') {
            let after = &line[hash_idx + 1..];
            let hex_part: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            assert!(
                !hex_part.chars().any(|c| c.is_ascii_uppercase()),
                "found uppercase hex in: {}",
                line
            );
        }
    }
}

#[test]
fn format_root_block_normalizes_3digit_hex_to_6digit() {
    // Direct unit test on the helper (via the public format function).
    // tokens.json doesn't currently use 3-digit hex; this asserts the
    // expansion behavior in case the source ever changes.
    use moss_core::contract::tokens::{Tokens, TokenGroup, TokenEntry, format_root_block};

    let tokens = Tokens {
        groups: vec![TokenGroup {
            name: "color".to_string(),
            description: None,
            entries: vec![TokenEntry {
                name: "test-color".to_string(),
                value: "#FFF".to_string(),
                dark_value: None,
                type_hint: Some("color".to_string()),
                description: None,
            }],
        }],
    };
    let css = format_root_block(&tokens);
    assert!(css.contains("--test-color: #ffffff;"), "got: {}", css);
}

// ─── WCAG 2.1 AA contrast gate ───────────────────────────────────────────────
//
// moss tells institutional users its output conforms to WCAG 2.1 AA. These
// tests are what make that a checked claim rather than a remembered one: a
// token edit that drops a text color below 4.5:1 fails here, in a plain unit
// test, on every PR.
//
// Only tokens whose value is a literal hex are checked. `var()` aliases and
// `color-mix()` expressions need a cascade to resolve and belong in the
// render-gate suite, which has an engine; see .claude/CLAUDE.md.
//
// Provenance: the six values this locks in were derived in moss#1047.

/// Relative luminance per WCAG 2.1 §relative-luminance.
fn relative_luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    let channel = |i: usize| {
        let v = u8::from_str_radix(&h[i..i + 2], 16).expect("valid hex pair") as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4)
}

/// Contrast ratio per WCAG 2.1 §contrast-ratio. Order-independent.
fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// A six-digit literal hex, or None for var()/color-mix()/rgba() values.
fn literal_hex(value: &str) -> Option<&str> {
    let v = value.trim();
    let is_hex6 = v.len() == 7
        && v.starts_with('#')
        && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    is_hex6.then_some(v)
}

fn token_value(tokens: &moss_core::contract::tokens::Tokens, name: &str, dark: bool) -> String {
    let entry = tokens
        .groups
        .iter()
        .flat_map(|g| g.entries.iter())
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("token {} must exist", name));
    // A token with no dark variant emits no dark override, so the light value
    // IS what dark mode paints — checking it twice is correct, not a fallback
    // papering over a missing value.
    if dark {
        entry.dark_value.clone().unwrap_or_else(|| entry.value.clone())
    } else {
        entry.value.clone()
    }
}

/// Foreground tokens that carry body text, paired with every background they
/// are actually painted on. A pair here is a promise the CSS keeps — when a
/// token starts appearing on a new background, add the pair.
///
/// A few syntax tokens (hl-meta, hl-tag, hl-deletion) are defined in
/// tokens.json but not currently consumed by any rule. They stay listed on
/// purpose: the constraint should already hold on the day someone wires one
/// up, rather than being discovered afterwards.
const TEXT_ON_BACKGROUNDS: &[(&str, &[&str])] = &[
    ("moss-color-muted", &["moss-color-bg", "moss-color-surface", "moss-code-background"]),
    ("moss-color-text", &["moss-color-bg", "moss-color-surface"]),
    ("moss-color-text-secondary", &["moss-color-bg", "moss-color-surface"]),
    // Link and heading text (site.css:152, 553, 972 and ~15 more). ui-accent
    // aliases this via var(), so it resolves to the same colour and is skipped
    // by literal_hex rather than listed twice.
    ("moss-color-accent", &["moss-color-bg", "moss-color-surface"]),
    ("moss-hl-comment", &["moss-code-background"]),
    ("moss-hl-meta", &["moss-code-background"]),
    ("moss-hl-keyword", &["moss-code-background"]),
    ("moss-hl-string", &["moss-code-background"]),
    ("moss-hl-attr", &["moss-code-background"]),
    ("moss-hl-operator", &["moss-code-background"]),
    ("moss-hl-function", &["moss-code-background"]),
    ("moss-hl-number", &["moss-code-background"]),
    ("moss-hl-type", &["moss-code-background"]),
    ("moss-hl-builtin", &["moss-code-background"]),
    ("moss-hl-tag", &["moss-code-background"]),
    ("moss-hl-deletion", &["moss-code-background"]),
    ("moss-code-accent-secondary", &["moss-code-background"]),
    ("moss-code-accent-tertiary", &["moss-code-background"]),
    ("moss-code-accent-quaternary", &["moss-code-background"]),
];

#[test]
fn text_tokens_meet_wcag_aa_contrast() {
    let tokens = load_tokens().expect("tokens.json must parse");
    let mut failures = Vec::new();
    let mut checked = 0;

    for (fg_name, backgrounds) in TEXT_ON_BACKGROUNDS {
        for dark in [false, true] {
            let fg_value = token_value(&tokens, fg_name, dark);
            let Some(fg) = literal_hex(&fg_value) else { continue };
            for bg_name in *backgrounds {
                let bg_value = token_value(&tokens, bg_name, dark);
                let Some(bg) = literal_hex(&bg_value) else { continue };
                checked += 1;
                let ratio = contrast_ratio(fg, bg);
                if ratio < 4.5 {
                    failures.push(format!(
                        "  --{} ({}) on --{} ({}) in {} mode: {:.2}:1, needs 4.5:1",
                        fg_name, fg, bg_name, bg, if dark { "dark" } else { "light" }, ratio
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "tokens.json has text colors below the WCAG 2.1 AA contrast threshold.\n\
         moss claims AA conformance for its output; these pairs break that claim:\n\n{}\n\n\
         Fix the token value in crates/moss-core/src/contract/tokens.json, then run\n\
         `cargo run --bin generate-artifacts --features dev-tools -- contract-docs`.\n\
         If a pair is wrong — the token is not text, or never lands on that\n\
         background — remove the pair from TEXT_ON_BACKGROUNDS and say why.",
        failures.join("\n")
    );

    // Vacuous-pass guard: a refactor that stops resolving values must not read
    // as a pass. 18 foregrounds × 2 themes, most on one background.
    assert!(checked >= 40, "expected at least 40 fg/bg pairs checked, got {}", checked);
}

/// Boundaries a user must SEE to operate the control — 1.4.11, threshold 3:1.
/// Decorative hairlines (border-light / border-medium) are exempt and absent here.
const NON_TEXT_ON_BACKGROUNDS: &[(&str, &[&str])] = &[
    ("moss-border-strong", &["moss-color-bg", "moss-color-surface"]),
];

#[test]
fn non_text_ui_tokens_meet_wcag_aa_contrast() {
    let tokens = load_tokens().expect("tokens.json must parse");
    let mut failures = Vec::new();

    for (fg_name, backgrounds) in NON_TEXT_ON_BACKGROUNDS {
        for dark in [false, true] {
            let fg_value = token_value(&tokens, fg_name, dark);
            let Some(fg) = literal_hex(&fg_value) else { continue };
            for bg_name in *backgrounds {
                let bg_value = token_value(&tokens, bg_name, dark);
                let Some(bg) = literal_hex(&bg_value) else { continue };
                let ratio = contrast_ratio(fg, bg);
                if ratio < 3.0 {
                    failures.push(format!(
                        "  --{} ({}) on --{} ({}) in {} mode: {:.2}:1, needs 3:1",
                        fg_name, fg, bg_name, bg, if dark { "dark" } else { "light" }, ratio
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(),
        "tokens.json has UI boundary colors below WCAG 2.1 AA non-text contrast:\n\n{}",
        failures.join("\n"));
}

#[test]
fn contrast_ratio_matches_known_wcag_values() {
    // Anchors the formula itself: without these, a bug in relative_luminance
    // could make every contrast test above pass vacuously.
    assert!((contrast_ratio("#ffffff", "#000000") - 21.0).abs() < 0.01);
    assert!((contrast_ratio("#ffffff", "#ffffff") - 1.0).abs() < 0.001);
    // Order independence.
    assert!((contrast_ratio("#8a8580", "#faf8f5") - contrast_ratio("#faf8f5", "#8a8580")).abs() < 1e-9);
    // The value moss#1047 measured for the old muted token on the page background.
    assert!((contrast_ratio("#8a8580", "#faf8f5") - 3.45).abs() < 0.01);
}
