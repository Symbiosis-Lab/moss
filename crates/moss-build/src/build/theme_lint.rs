//! Lints user theme CSS against moss's contract vocabulary.
//!
//! Best-effort scan: strips CSS block comments, then checks three surfaces
//! against `moss-core`'s contract tables:
//!
//! 1. Retired class names — a closed list theme authors should migrate off.
//! 2. Custom-property references (`var(--moss-x)`) and declarations
//!    (`--moss-x: ...`) against the design-token table
//!    (`moss_core::contract::tokens`) and the escape-hatch table
//!    (`moss_core::contract::custom_props::CUSTOM_PROPS`).
//! 3. Class selectors (`.moss-x`) against the component contract
//!    (`moss_core::contract::components::COMPONENTS`).
//!
//! ## Why declarations and references are treated differently
//!
//! A theme is free to declare its own custom property — `--moss-my-brand: #c00`
//! — and read it back via `var(--moss-my-brand)`. That is ordinary CSS and must
//! not warn. So the lint collects every `--moss-*` property the theme itself
//! declares, and only warns on a `var(--moss-x)` reference when `x` is in
//! neither the known-token union nor the theme's own declarations. An unknown
//! declaration that is never read back locally is warned on separately, and
//! more softly, because that is the shape a hallucinated token name actually
//! takes (the value is inert — nothing in the theme ever reads it).
//!
//! ## Why CUSTOM_PROPS must be in the known-token union
//!
//! `CUSTOM_PROPS` lists escape hatches (`--moss-hero-max-height`,
//! `--moss-nav-width`, ...) that moss's own stylesheets read via
//! `var(--moss-foo, <fallback>)` but deliberately never declare in `:root`.
//! They are, in practice, the most common thing a real theme sets — see the
//! doc comment on `custom_props.rs`. Checking only the declared-token table
//! would make this lint fire on correct, unmodified themes, which is worse
//! than shipping no lint at all.
//!
//! ## Why plugin-namespaced classes are excluded from the class check
//!
//! Plugins inject their own markup under a `moss-embed-plugin-<name>` class
//! namespace (`src-tauri/src/plugins/embed_adapter.rs`). `COMPONENTS` has no
//! entries for plugin classes, so checking them against it would false-positive
//! on every themed plugin embed. Classes under that namespace are skipped.
//!
//! ## Limitations
//!
//! - No CSS parser dependency; regex/substring scan only. A class or property
//!   name inside a string literal (e.g. a `content` property) may produce a
//!   false positive — acceptable for v1, every warning here is informational
//!   and non-fatal.
//! - Nearest-name suggestions use plain Levenshtein distance over the known
//!   vocabulary; ties resolve to whichever candidate sorts first.

use std::collections::HashSet;
use std::sync::LazyLock;

use moss_core::contract::components::{retired_class_names, COMPONENTS};
use moss_core::contract::custom_props::CUSTOM_PROPS;
use moss_core::contract::tokens::load_tokens;

/// Matches a `var(--moss-x` reference, capturing the property name.
static VAR_REF: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"var\(\s*(--moss-[A-Za-z0-9_-]+)").unwrap());

/// Matches a `--moss-x:` declaration, capturing the property name.
static PROP_DECL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(--moss-[A-Za-z0-9_-]+)\s*:").unwrap());

/// Matches a `.moss-x` class selector, capturing the complete class name
/// (the character class already consumes every continuation character, so
/// no separate boundary check is needed the way the retired-class scan does).
static CLASS_REF: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\.(moss-[A-Za-z0-9_-]+)").unwrap());

/// Lint a theme CSS source against moss's contract vocabulary.
/// Returns a list of warning strings; empty if clean. Every warning is
/// informational — this never blocks a build.
pub fn lint_theme_css(css: &str) -> Vec<String> {
    let stripped = strip_comments(css);
    let mut warnings = Vec::new();

    lint_retired_classes(&stripped, &mut warnings);
    lint_custom_properties(&stripped, &mut warnings);
    lint_classes(&stripped, &mut warnings);

    warnings
}

fn lint_retired_classes(stripped: &str, warnings: &mut Vec<String>) {
    for class in retired_class_names() {
        let pattern = format!(".{}", class);
        if find_complete_class(stripped, &pattern) {
            warnings.push(format!(
                "theme uses pre-v1 class .{}: see https://landing.mosspub.com/contract/v1/migration.html",
                class
            ));
        }
    }
}

fn lint_custom_properties(stripped: &str, warnings: &mut Vec<String>) {
    let known_tokens = known_token_names();

    let declared: HashSet<String> = PROP_DECL
        .captures_iter(stripped)
        .map(|c| c[1].trim_start_matches("--").to_string())
        .collect();

    let referenced: HashSet<String> = VAR_REF
        .captures_iter(stripped)
        .map(|c| c[1].trim_start_matches("--").to_string())
        .collect();

    // References: warn only when the name is neither a known token/escape
    // hatch nor something the theme declares itself.
    let mut warned_refs: Vec<&String> = referenced
        .iter()
        .filter(|name| !known_tokens.contains(name.as_str()) && !declared.contains(*name))
        .collect();
    warned_refs.sort();
    for name in warned_refs {
        let suggestion = nearest_name(name, known_tokens.iter().map(|s| s.as_str()));
        warnings.push(format!(
            "theme references --{}, which is not a moss token or custom property; closest is --{}",
            name, suggestion
        ));
    }

    // Declarations: warn (softer) only when unknown AND never read back
    // locally — that is the hallucination shape, since a real escape hatch
    // the theme invents for its own use would be referenced somewhere.
    let mut warned_decls: Vec<&String> = declared
        .iter()
        .filter(|name| !known_tokens.contains(name.as_str()) && !referenced.contains(*name))
        .collect();
    warned_decls.sort();
    for name in warned_decls {
        let suggestion = nearest_name(name, known_tokens.iter().map(|s| s.as_str()));
        warnings.push(format!(
            "theme declares --{}, which moss does not read; did you mean --{}?",
            name, suggestion
        ));
    }
}

fn lint_classes(stripped: &str, warnings: &mut Vec<String>) {
    let known_classes: HashSet<&str> = COMPONENTS.iter().map(|e| e.class).collect();

    let mut unknown: Vec<String> = CLASS_REF
        .captures_iter(stripped)
        .map(|c| c[1].to_string())
        .filter(|class| !known_classes.contains(class.as_str()))
        .filter(|class| !class.starts_with("moss-embed-plugin-"))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unknown.sort();

    for class in unknown {
        let suggestion = nearest_name(&class, known_classes.iter().copied());
        warnings.push(format!(
            "theme uses class .{}, which is not a moss component; closest is .{}",
            class, suggestion
        ));
    }
}

/// The union of declared design tokens (`tokens.json`) and escape-hatch
/// custom properties (`CUSTOM_PROPS`), as bare names without the leading `--`.
/// See the module doc for why both tables are required.
fn known_token_names() -> HashSet<String> {
    let mut names = HashSet::new();

    if let Ok(tokens) = load_tokens() {
        for group in &tokens.groups {
            for entry in &group.entries {
                names.insert(entry.name.clone());
            }
        }
    }

    for prop in CUSTOM_PROPS {
        names.insert(prop.name.trim_start_matches("--").to_string());
    }

    names
}

/// Nearest candidate to `needle` by Levenshtein distance, ties broken by
/// lexical order. Panics-free: returns `needle` itself if `candidates` is empty
/// (should never happen — both known tables are non-empty).
fn nearest_name<'a>(needle: &str, candidates: impl Iterator<Item = &'a str>) -> String {
    candidates
        .map(|c| (levenshtein(needle, c), c))
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)))
        .map(|(_, c)| c.to_string())
        .unwrap_or_else(|| needle.to_string())
}

/// Classic dynamic-programming Levenshtein edit distance over bytes (all
/// candidate vocabulary here is ASCII, so byte distance == char distance).
fn levenshtein(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];

    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b.len()]
}

/// Does the CSS contain `pattern` as a complete class reference?
/// A complete reference is followed by a character that is NOT a CSS-class
/// continuation character (so `.moss-card-grid-overlay` does NOT match `.moss-card-grid`).
fn find_complete_class(css: &str, pattern: &str) -> bool {
    css.match_indices(pattern).any(|(idx, _)| {
        match css
            .get(idx + pattern.len()..)
            .and_then(|after| after.chars().next())
        {
            None => true,
            Some(c) => !c.is_ascii_alphanumeric() && c != '_' && c != '-',
        }
    })
}

/// Strip `/* ... */` block comments from CSS. CSS has no `//` line comments.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The critical false-positive guard: a theme that only sets escape-hatch
    /// custom properties (never declared in `:root`, only read via `var()`
    /// with a fallback) and overrides a real token must produce zero warnings.
    #[test]
    fn correct_theme_with_custom_props_has_no_warnings() {
        let css = r#"
            :root {
                --moss-color-accent: #c00;
            }
            .moss-hero {
                --moss-hero-max-height: none;
                --moss-hero-object-position: center;
            }
            .main-nav {
                width: var(--moss-nav-width, 60rem);
            }
            .moss-card {
                color: var(--moss-color-accent);
            }
        "#;
        let warnings = lint_theme_css(css);
        assert!(warnings.is_empty(), "expected no warnings, got: {:?}", warnings);
    }

    /// A theme may declare its own custom property and read it back locally;
    /// that is ordinary CSS, not a hallucinated token.
    #[test]
    fn self_declared_and_read_property_has_no_warning() {
        let css = r#"
            :root {
                --moss-my-brand: #336699;
            }
            .moss-card {
                border-color: var(--moss-my-brand);
            }
        "#;
        let warnings = lint_theme_css(css);
        assert!(warnings.is_empty(), "expected no warnings, got: {:?}", warnings);
    }

    /// A `var()` reference to a name that is neither a known token/escape
    /// hatch nor declared locally is a hallucinated token reference — the
    /// suggestion must name the (Levenshtein-)closest real token, e.g. a
    /// dropped letter in `--moss-color-accent`.
    #[test]
    fn hallucinated_token_reference_names_closest_real_token() {
        let css = r#"
            .moss-card {
                color: var(--moss-colr-accent);
            }
        "#;
        let warnings = lint_theme_css(css);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains("--moss-colr-accent"));
        assert!(warnings[0].contains("--moss-color-accent"));
    }

    /// An unknown declaration that is never read back anywhere in the theme
    /// is the inert-hallucination shape: the softer "did you mean" warning.
    #[test]
    fn hallucinated_declaration_never_read_gets_softer_warning() {
        let css = r#"
            :root {
                --moss-colr-accent: #c00;
            }
        "#;
        let warnings = lint_theme_css(css);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains("does not read"));
        assert!(warnings[0].contains("--moss-colr-accent"));
        assert!(warnings[0].contains("--moss-color-accent"));
    }

    /// Retired classes keep their existing, more specific migration-link
    /// warning rather than falling into the generic unknown-class bucket.
    #[test]
    fn retired_class_keeps_migration_warning() {
        let retired = retired_class_names().next().expect("at least one retired class");
        let css = format!(".{} {{ color: red; }}", retired);
        let warnings = lint_theme_css(&css);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains("pre-v1 class"));
        assert!(warnings[0].contains("migration.html"));
    }

    /// An unknown class (not retired, not a component, not plugin-namespaced)
    /// gets the generic unknown-class warning naming the closest real class.
    #[test]
    fn unknown_class_names_closest_component() {
        let css = ".moss-carrd { color: red; }";
        let warnings = lint_theme_css(css);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains(".moss-carrd"));
        assert!(warnings[0].contains(".moss-card"));
    }

    /// Plugin-namespaced classes are never checked against COMPONENTS —
    /// COMPONENTS has no entries for them by design.
    #[test]
    fn plugin_namespaced_class_has_no_warning() {
        let css = ".moss-embed-plugin-graphviz { border: 1px solid; }";
        let warnings = lint_theme_css(css);
        assert!(warnings.is_empty(), "expected no warnings, got: {:?}", warnings);
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("moss-color-primary", "moss-color-accent"), levenshtein("moss-color-primary", "moss-color-accent"));
        assert!(levenshtein("kitten", "sitting") == 3);
    }
}
