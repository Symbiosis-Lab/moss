//! Lints user theme CSS against moss's contract vocabulary, and walks a
//! stylesheet into its rules ([`style_rules`]) for the build and the CLI.
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
//! namespace (the app's plugin embed adapter). `COMPONENTS` has no
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

use crate::infra::edit_distance::levenshtein;
use std::collections::{BTreeSet, HashSet};
use std::sync::LazyLock;

use crate::build::scan::section_text::Rgb;

use moss_core::contract::components::{retired_class_names, COMPONENTS};
use moss_core::contract::custom_props::CUSTOM_PROPS;
use moss_core::contract::tokens::load_tokens;

/// Matches a `var(--moss-x` reference, capturing the property name.
static VAR_REF: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"var\(\s*(--moss-[A-Za-z0-9_-]+)").unwrap());

/// Matches a `--moss-x:` declaration, capturing the property name.
pub(crate) static PROP_DECL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(--moss-[A-Za-z0-9_-]+)\s*:").unwrap());

/// Matches a `.moss-x` class selector, capturing the complete class name
/// (the character class already consumes every continuation character, so
/// no separate boundary check is needed the way the retired-class scan does).
static CLASS_REF: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\.(moss-[A-Za-z0-9_-]+)").unwrap());

/// Lint a theme CSS source against moss's contract vocabulary.
/// Returns a list of warning strings; empty if clean. Every warning is
/// informational — this never blocks a build.
/// A site fixed to one scheme (`color_scheme = "light"`) never shows the
/// other, so a colour set for one side only is not a gap there.
pub fn lint_theme_css(css: &str, scheme: crate::build::page::color_scheme::ColorScheme) -> Vec<String> {
    let stripped = strip_comments(css);
    let mut warnings = Vec::new();

    lint_retired_classes(&stripped, &mut warnings);
    lint_custom_properties(&stripped, &mut warnings);
    lint_classes(&stripped, &mut warnings);
    if scheme == crate::build::page::color_scheme::ColorScheme::Auto {
        lint_light_only_colours(&stripped, &mut warnings);
    }

    warnings
}

/// Where a declaration applies: `:root {}` wins in both themes, so it is the
/// light value; `:root[data-theme="dark"] {}` (also `html[...]` or a bare
/// `[data-theme=dark]`, quoted either way) is the dark one. Anything else
/// (another selector, or any at-rule such as `@media`) is `Other`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Light,
    Dark,
    Other,
}

/// Which theme a rule with this one selector sets, unconditioned or inside an at-rule.
fn scope_of(unconditioned: bool, selector: &str) -> Scope {
    let selector: String = selector.chars().filter(|c| !c.is_whitespace()).filter(|c| !matches!(c, '"' | '\'')).collect();
    match (unconditioned, selector.as_str()) {
        (true, ":root") => Scope::Light,
        (true, ":root[data-theme=dark]" | "html[data-theme=dark]" | "[data-theme=dark]") => Scope::Dark,
        _ => Scope::Other,
    }
}

/// A colour token on one side (light or dark) as the page shows it at its
/// root, following `var(--x)` through other custom properties (`None` where
/// the build can't read it), and whether the token or a property it goes
/// through is also set in another scope (a section, a media query), so that
/// some pages differ from the root. A `var(--x, fallback)` follows `--x` only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RootColour {
    pub root: Option<Rgb>,
    pub varies: bool,
}

/// `token` on the light and the dark side: the theme's value, else moss's own.
pub(crate) fn root_colours(theme_css: Option<&str>, token: &str) -> (RootColour, RootColour) {
    Layered::new(theme_css).colours(token)
}

/// One custom property declaration: where it applies, the property, its value.
type Decl = (Scope, String, String);

/// Every custom property declaration as (scope, property, value), once per
/// selector of its rule: `:root, main {}` sets the root and another scope.
/// With `lang`, a rule the root of a page in that language matches
/// (`html[lang="zh-Hant"]`, `html:is([lang|="zh"], …)`) also sets the root.
fn custom_properties(stripped: &str, lang: Option<&str>) -> Vec<Decl> {
    let mut out = Vec::new();
    for rule in style_rules(stripped) {
        let body = rule.declarations();
        let unconditioned = rule.conditions.is_empty();
        for selector in split_top_level(rule.prelude, ',') {
            let scope = match scope_of(unconditioned, selector) {
                Scope::Other if unconditioned && lang.is_some_and(|l| selects_root_in(selector, l)) => Scope::Light,
                scope => scope,
            };
            push_custom_properties(&mut out, scope, &body);
        }
    }
    out
}

fn push_custom_properties(out: &mut Vec<Decl>, scope: Scope, body: &[(String, String)]) {
    out.extend(body.iter().filter(|(p, _)| p.starts_with("--")).map(|(p, v)| (scope, p.clone(), v.clone())));
}

/// moss's own token values, the lowest layer of the page: the registry's light
/// values on the root and its dark values under the dark theme.
static REGISTRY_LAYER: LazyLock<Vec<Decl>> = LazyLock::new(|| {
    let registry = load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {e}"));
    registry.groups.iter().flat_map(|g| &g.entries).flat_map(|e| {
        let name = format!("--{}", e.name);
        let dark = e.dark_value.clone().map(|d| (Scope::Dark, name.clone(), d));
        std::iter::once((Scope::Light, name, e.value.clone())).chain(dark)
    }).collect()
});

/// The page stylesheet's own root rules for a page in `lang`, which sit between
/// the registry and the theme: the Han font stacks, and the swap to them on a
/// Chinese, Japanese or Korean page.
fn page_layer(lang: &str) -> Vec<Decl> {
    let css = strip_comments(crate::build::page::shell::DEFAULT_CSS);
    let mut out = Vec::new();
    for rule in style_rules(&css) {
        // A cascade layer orders rules; it does not condition them.
        if rule.conditions.iter().all(|c| c.starts_with("@layer")) && selects_root_in(rule.prelude, lang) {
            push_custom_properties(&mut out, Scope::Light, &rule.declarations());
        }
    }
    out
}

/// Whether a selector the page stylesheet or a theme writes for the root matches
/// a page in `lang`: `:root`, `html[lang="x"]`, `html:lang(x)` and
/// `html:is([lang|="x"], …)`.
fn selects_root_in(prelude: &str, lang: &str) -> bool {
    let sel: String = prelude.chars().filter(|c| !c.is_whitespace() && !matches!(c, '"' | '\'')).collect::<String>().to_ascii_lowercase();
    let lang = lang.to_ascii_lowercase();
    if sel == ":root" {
        return true;
    }
    if let Some(exact) = sel.strip_prefix("html[lang=").and_then(|s| s.strip_suffix(']')) {
        return exact == lang;
    }
    // `:lang(x)` matches x and every subtag of it, as `[lang|=x]` does.
    if let Some(range) = sel.strip_prefix("html:lang(").and_then(|s| s.strip_suffix(')')) {
        return range.split(',').any(|x| lang == x || lang.starts_with(&format!("{x}-")));
    }
    let Some(list) = sel.strip_prefix("html:is(").and_then(|s| s.strip_suffix(')')) else { return false };
    list.split(',').filter_map(|item| item.strip_prefix("[lang|=")?.strip_suffix(']')).any(|x| lang == x || lang.starts_with(&format!("{x}-")))
}

/// A `var()` chain is followed this many properties deep, which also ends a cycle.
const MAX_HOPS: usize = 8;

/// `token`'s value on one side at the page root, following `var(--x)` through
/// other custom properties, and whether the token or a property it goes
/// through is also set in another scope. `layers` are cascade layers, the
/// winning one first: a property set anywhere in a layer beats every later one.
fn resolve_value<'d>(layers: &[&'d [Decl]], token: &str, side: Scope) -> (Option<&'d str>, bool) {
    let mut varies = false;
    let mut name = token;
    for _ in 0..MAX_HOPS {
        varies |= layers.iter().any(|decls| decls.iter().any(|(scope, prop, _)| *scope == Scope::Other && prop == name));
        let set_in = |decls: &'d [Decl], want: Scope| decls.iter().rev().find(|(scope, prop, _)| *scope == want && prop == name).map(|d| d.2.as_str());
        // The dark side reads `:root` too, for whatever its own rule leaves.
        let value = layers.iter().find_map(|decls| set_in(decls, side).or_else(|| set_in(decls, Scope::Light)));
        match value.map(|v| (v, v.strip_prefix("var(").and_then(|v| v.strip_suffix(')')))) {
            Some((_, Some(inner))) => name = inner.split(',').next().unwrap_or_default().trim(),
            Some((v, None)) => return (Some(v), varies),
            None => return (None, varies),
        }
    }
    (None, varies)
}

/// A theme stylesheet's custom properties over moss's own, as the theme layer
/// sits over the page stylesheet and the tokens on the page. Parsed once;
/// [`Layered::values`] and [`Layered::colours`] look each property up.
pub(crate) struct Layered {
    theme: Vec<Decl>,
    page: Vec<Decl>,
}

impl Layered {
    /// The theme over the registry. Enough for colours: the page stylesheet's
    /// root rules set fonts only.
    pub(crate) fn new(theme_css: Option<&str>) -> Self {
        Self { theme: custom_properties(&strip_comments(theme_css.unwrap_or_default()), None), page: Vec::new() }
    }

    /// The theme over the page stylesheet's root rules for `lang` and the registry.
    pub(crate) fn for_page(theme_css: Option<&str>, lang: &str) -> Self {
        Self { theme: custom_properties(&strip_comments(theme_css.unwrap_or_default()), Some(lang)), page: page_layer(lang) }
    }

    fn resolve(&self, token: &str, side: Scope) -> (Option<&str>, bool) {
        resolve_value(&[&self.theme, &self.page, &REGISTRY_LAYER], token, side)
    }

    /// `token`'s value on the light and the dark side at the page root, after
    /// following `var()`; `None` where no layer sets it there.
    pub(crate) fn values(&self, token: &str) -> (Option<String>, Option<String>) {
        let side = |scope| self.resolve(token, scope).0.map(str::to_string);
        (side(Scope::Light), side(Scope::Dark))
    }

    pub(crate) fn colours(&self, token: &str) -> (RootColour, RootColour) {
        let side = |scope| {
            let (value, varies) = self.resolve(token, scope);
            RootColour { root: value.and_then(crate::build::components::color_extract::parse_css_color), varies }
        };
        (side(Scope::Light), side(Scope::Dark))
    }
}

/// A colour token set on `:root` with no dark value stays at its light value
/// in dark mode, so the page ships unreadable.
fn lint_light_only_colours(stripped: &str, warnings: &mut Vec<String>) {
    let declared = custom_properties(stripped, None);
    let colours = |scope: Scope| declared.iter().filter(move |(s, p, _)| *s == scope && p.starts_with("--moss-color-"));
    let dark: BTreeSet<&str> = colours(Scope::Dark).map(|(_, p, _)| p.as_str()).collect();
    // A `var()` value resolves on the element, so it follows into dark mode.
    let missing: BTreeSet<&str> = colours(Scope::Light)
        .filter(|(_, _, v)| !v.contains("var("))
        .map(|(_, p, _)| p.as_str())
        .filter(|p| !dark.contains(p))
        .collect();
    if missing.is_empty() {
        return;
    }
    let (noun, pronoun) = if missing.len() == 1 { ("token", "it") } else { ("tokens", "them") };
    warnings.push(format!(
        ".moss/theme/style.css sets the colour {noun} {} on :root but not for dark mode, so dark mode keeps the light value. Declare {pronoun} again under :root[data-theme=\"dark\"] {{}}.",
        missing.into_iter().collect::<Vec<_>>().join(", ")
    ));
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

/// The browser's root font size, which moss's stylesheet leaves as it is.
pub(crate) const ROOT_PX: f32 = 16.0;

/// `1.5rem` or `24px` as px at the [`ROOT_PX`] root.
pub(crate) fn plain_px(value: &str) -> Option<f32> {
    let value = value.trim().trim_end_matches("!important").trim();
    match value.strip_suffix("rem") {
        Some(n) => n.trim().parse::<f32>().ok().map(|n| n * ROOT_PX),
        None => value.strip_suffix("px")?.trim().parse().ok(),
    }
}

/// The weight a `font-weight` value names, `None` for one the build cannot read.
pub(crate) fn weight_of(value: &str) -> Option<u16> {
    match value.trim().trim_end_matches("!important").trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        "lighter" | "light" | "thin" => Some(300),
        n => n.parse().ok(),
    }
}

/// Strip `/* ... */` block comments from CSS. CSS has no `//` line comments.
pub(crate) fn strip_comments(css: &str) -> String {
    let b = css.as_bytes();
    let (mut out, mut copied, mut i) = (String::with_capacity(css.len()), 0, 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => {
                out.push_str(&css[copied..i]);
                i = comment_end(b, i);
                copied = i;
            }
            q @ (b'"' | b'\'') => i = (string_end(b, i, q) + 1).min(b.len()),
            _ => i += 1,
        }
    }
    out.push_str(&css[copied..]);
    out
}

/// One past the `*/` closing the comment that opens at `start`, or the end.
/// Every CSS reader here steps over comments and quoted strings with this and
/// [`string_end`], so a `/*` inside a string (`url("a/*b")`) opens no comment.
pub(crate) fn comment_end(b: &[u8], start: usize) -> usize {
    b[start + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |p| start + 2 + p + 2)
}

/// Index of the quote closing the string that opens at `start`, or the end.
pub(crate) fn string_end(b: &[u8], start: usize, quote: u8) -> usize {
    let mut j = start + 1;
    while j < b.len() && b[j] != quote {
        j += if b[j] == b'\\' { 2 } else { 1 };
    }
    j.min(b.len())
}

/// A declaration block's `property: value` pairs, split on `;` outside
/// parentheses and quotes (`url(data:…;…)` stays one value). A property is
/// lowercased unless it is a custom one; `!important` is dropped.
pub(crate) fn declarations(block: &str) -> Vec<(String, String)> {
    split_top_level(block, ';')
        .into_iter()
        .filter_map(|d| d.split_once(':'))
        .map(|(p, v)| {
            let p = p.trim();
            let p = if p.starts_with("--") { p.to_string() } else { p.to_ascii_lowercase() };
            (p, v.trim().trim_end_matches("!important").trim().to_string())
        })
        .collect()
}

/// One style rule of a stylesheet, with the context needed to reproduce its effect.
pub(crate) struct StyleRule<'a> {
    /// The `@media`/`@supports`/`@layer` conditions the rule sits inside,
    /// outermost first. Without these a copied rule silently applies at every
    /// width — the single most common way a theme override goes wrong.
    pub conditions: Vec<&'a str>,
    /// The comment block immediately above the rule, if any.
    pub comment: Option<&'a str>,
    /// The selector list, trimmed.
    pub prelude: &'a str,
    /// Selector list plus declaration block, verbatim.
    pub rule: &'a str,
    /// Byte offset of the selector list in the sheet.
    pub start: usize,
}

/// At-rules whose block holds declarations, not rules: each is one rule, its
/// at-keyword the prelude.
const DECLARATION_AT_RULES: &[&str] = &["@font-face", "@page", "@property", "@counter-style", "@font-palette-values"];

fn is_declaration_at_rule(prelude: &str) -> bool {
    let keyword = prelude.split(|c: char| c.is_whitespace() || c == '{').next().unwrap_or_default();
    DECLARATION_AT_RULES.iter().any(|k| keyword.eq_ignore_ascii_case(k))
}

/// Calls `visit(i, c, depth)` for each char of `chars` outside quotes and
/// escapes, where `depth` counts the parentheses and brackets open before it.
/// A `Some(next)` from `visit` resumes the walk at `next`.
pub(crate) fn syntax_chars(chars: &[char], mut visit: impl FnMut(usize, char, i32) -> Option<usize>) {
    let (mut i, mut depth, mut quote) = (0usize, 0i32, None::<char>);
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, c) => {
                let at = depth;
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth -= 1,
                    _ => {}
                }
                if let Some(next) = visit(i, c, at) {
                    i = next;
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// `list` split on `sep` outside parentheses, brackets and quotes, each part
/// trimmed and empty ones dropped: a selector list on `,`, a declaration block
/// on `;`, a font stack on `,`.
pub(crate) fn split_top_level(list: &str, sep: char) -> Vec<&str> {
    let chars: Vec<(usize, char)> = list.char_indices().collect();
    let plain: Vec<char> = chars.iter().map(|(_, c)| *c).collect();
    let mut cuts = Vec::new();
    syntax_chars(&plain, |i, c, depth| {
        if c == sep && depth == 0 {
            cuts.push(chars[i].0);
        }
        None
    });
    let mut out = Vec::new();
    let mut from = 0;
    for cut in cuts {
        out.push(list[from..cut].trim());
        from = cut + sep.len_utf8();
    }
    out.push(list[from..].trim());
    out.retain(|s| !s.is_empty());
    out
}

impl StyleRule<'_> {
    /// The rule's declarations (see [`declarations`]).
    pub(crate) fn declarations(&self) -> Vec<(String, String)> {
        declarations(self.rule.split_once('{').map_or("", |(_, block)| block).trim_end_matches('}'))
    }
}

/// Walk a stylesheet by brace depth, collecting every style rule along with the
/// at-rule conditions enclosing it.
///
/// Deliberately not a real CSS parser. It has one job — slice out source spans
/// — and a parser would drop the comments, which are half the value.
// `i` walks bytes and can sit mid-character, but every slice below is taken at a
// point where `bytes[i]` matched an ASCII delimiter (`/` `*` `{` `}` `;`) or at
// `bytes.len()`. An ASCII byte never occurs inside a multi-byte UTF-8 sequence,
// so those indices — and `prelude_start`, which is only ever assigned one of
// them — are always char boundaries. None of these can panic.
#[allow(clippy::string_slice)]
pub(crate) fn style_rules<'a>(css: &'a str) -> Vec<StyleRule<'a>> {
    let mut out = Vec::new();
    let bytes = css.as_bytes();
    // Stack of open at-rule preludes (`@media (max-width: 32rem)`), so a nested
    // rule reports every condition it is under.
    let mut conditions: Vec<&'a str> = Vec::new();
    // Start of the current prelude — selector list or at-rule header.
    let mut prelude_start = 0usize;
    // End of the last comment block seen at the current position, and its span.
    let mut pending_comment: Option<&'a str> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let start = i;
                i = comment_end(bytes, i);
                // Only a comment sitting alone above a rule is context for it.
                // One trailing a declaration (`.a { … } /* note */`) belongs to
                // the line it is on, and hoisting it above the next rule would
                // put unrelated prose in front of half the output. The tell is
                // a newline between the previous `}` and the comment.
                // A comment inside a selector list stays where it is.
                let gap = &css[prelude_start..start];
                if gap.trim().is_empty() {
                    pending_comment = (prelude_start == 0 || gap.contains('\n')).then(|| css[start..i].trim_end());
                    prelude_start = i;
                }
            }
            b'{' => {
                let prelude = css[prelude_start..i].trim();
                let rule_start = prelude_start + (css[prelude_start..].len() - css[prelude_start..].trim_start().len());
                if prelude.starts_with('@') && !is_declaration_at_rule(prelude) {
                    // An at-rule block: descend, remembering the condition.
                    conditions.push(prelude);
                    i += 1;
                    prelude_start = i;
                    pending_comment = None;
                    continue;
                }
                // A style rule: find its matching close brace. Declaration
                // blocks do not nest in moss's sheets, so depth-1 suffices.
                let mut depth = 1;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    i = match bytes[i] {
                        b'/' if bytes.get(i + 1) == Some(&b'*') => comment_end(bytes, i),
                        q @ (b'"' | b'\'') => string_end(bytes, i, q) + 1,
                        b'{' => {
                            depth += 1;
                            i + 1
                        }
                        b'}' => {
                            depth -= 1;
                            i + 1
                        }
                        _ => i + 1,
                    };
                }
                i = i.min(bytes.len());
                out.push(StyleRule {
                    conditions: conditions.clone(),
                    comment: pending_comment,
                    prelude,
                    rule: css[rule_start..i].trim(),
                    start: rule_start,
                });
                prelude_start = i;
                pending_comment = None;
            }
            b'}' => {
                conditions.pop();
                i += 1;
                prelude_start = i;
                pending_comment = None;
            }
            q @ (b'"' | b'\'') => i = (string_end(bytes, i, q) + 1).min(bytes.len()),
            b';' => {
                // A statement at this level (`@import`, `@layer a, b;`).
                i += 1;
                prelude_start = i;
                pending_comment = None;
            }
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::page::color_scheme::ColorScheme;

    #[test]
    fn a_lang_list_selects_each_of_its_languages() {
        for lang in ["zh-Hant", "ja"] {
            assert!(selects_root_in("html:lang(zh, \"ja\")", lang), "{lang}");
        }
        assert!(!selects_root_in("html:lang(zh, ja)", "en"));
    }

    #[test]
    fn a_theme_may_stand_its_font_in_front_of_moss_s_han_stacks() {
        let css = "html:is([lang|=\"zh\"]) { --moss-font-body: 'Field Sans', var(--moss-font-body-han); --moss-font-heading: var(--moss-font-heading-han); }";
        assert!(lint_theme_css(css, ColorScheme::Auto).is_empty(), "{:?}", lint_theme_css(css, ColorScheme::Auto));
    }

    #[test]
    fn a_semicolon_inside_a_quoted_value_ends_no_declaration() {
        let css = ":root { --moss-color-bg: #fafafa; --moss-hero-bg: url(\"data:image/svg+xml;--moss-color-bg: red\"); }";
        assert_eq!(root_colours(Some(css), "--moss-color-bg").0.root, Some((0xfa, 0xfa, 0xfa)));
    }

    #[test]
    fn a_declaration_at_rule_is_one_rule_and_a_split_keeps_nested_separators() {
        let rules = style_rules("@font-face { font-family: A; src: url(a.woff2) }\n@media (min-width: 1px) { .b { x: 1 } }\n");
        let shape: Vec<(&str, usize)> = rules.iter().map(|r| (r.prelude, r.conditions.len())).collect();
        assert_eq!(shape, vec![("@font-face", 0), (".b", 1)]);
        assert_eq!(split_top_level(":is(.a, .b) p, .c", ','), vec![":is(.a, .b) p", ".c"]);
        assert_eq!(split_top_level("src: url(data:x;y); color: red;", ';'), vec!["src: url(data:x;y)", "color: red"]);
    }

    #[test]
    fn a_comment_opener_or_brace_inside_a_string_ends_no_rule() {
        let css = "a[title=\"/*\"] .x { content: \"}\"; color: red }\n.y { }\n";
        let preludes: Vec<&str> = style_rules(css).iter().map(|r| r.prelude).collect();
        assert_eq!(preludes, vec!["a[title=\"/*\"] .x", ".y"]);
    }

    fn dark_gap(css: &str) -> Vec<String> {
        lint_theme_css(css, ColorScheme::Auto).into_iter().filter(|w| w.contains("dark mode keeps")).collect()
    }

    #[test]
    fn a_site_fixed_to_one_scheme_has_no_dark_gap() {
        let css = ":root { --moss-color-bg: #fafafa; }";
        assert!(lint_theme_css(css, ColorScheme::Light).is_empty());
        assert!(lint_theme_css(css, ColorScheme::Dark).is_empty());
        assert_eq!(dark_gap(css).len(), 1, "a site that follows the reader's scheme does show the other side");
    }

    #[test]
    fn light_token_without_dark_value_warns_once_naming_every_token() {
        let found = dark_gap(":root { --moss-color-bg: #fafafa; --moss-color-accent: #225; }");
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("--moss-color-accent, --moss-color-bg"), "{found:?}");
        assert!(found[0].contains(":root[data-theme=\"dark\"]"), "{found:?}");
    }

    #[test]
    fn a_selector_list_sets_each_of_its_selectors() {
        assert_eq!(dark_gap(":root, main { --moss-color-accent: #225; }").len(), 1, "it sets the root");
        assert!(dark_gap(":root { --moss-color-accent: #225; } :root[data-theme=dark], .x { --moss-color-accent: #99f; }").is_empty(), "and the dark root");
    }

    #[test]
    fn a_root_colour_follows_var_and_says_when_a_section_changes_it() {
        const BG: &str = "--moss-color-bg";
        let sectioned = ":root { --paper: #fbfbfb; }\n:root, body[data-section] { --moss-color-bg: var(--paper); }\nbody[data-section] { --paper: var(--section); }";
        let (light, dark) = root_colours(Some(sectioned), BG);
        assert_eq!(light, RootColour { root: Some((251, 251, 251)), varies: true });
        assert_eq!(dark, light, "no dark rule: the root holds on both sides");
        let steady = root_colours(Some(":root { --day: #fff; --paper: var(--day); --moss-color-bg: var(--paper); }"), BG);
        assert_eq!(steady.0, RootColour { root: Some((255, 255, 255)), varies: false });
        let (l, d) = *crate::build::scan::section_text::BACKGROUNDS;
        assert_eq!(root_colours(None, BG), (RootColour { root: Some(l), varies: false }, RootColour { root: Some(d), varies: false }), "unset: moss's own");
        for unreadable in [":root { --moss-color-bg: var(--a); --a: var(--b); --b: var(--a); }", ":root { --moss-color-bg: var(--nowhere); }"] {
            assert_eq!(root_colours(Some(unreadable), BG).0.root, None, "{unreadable}");
        }
    }

    #[test]
    fn a_theme_value_that_names_a_moss_token_takes_that_token_s_value() {
        let (surface, _) = root_colours(None, "--moss-color-surface");
        let (light, _) = root_colours(Some(":root { --moss-color-bg: var(--moss-color-surface); }"), "--moss-color-bg");
        assert_eq!(light.root, surface.root);
        assert!(light.root.is_some());
    }

    #[test]
    fn a_han_page_takes_the_han_font_stacks_and_a_theme_font_still_wins() {
        let latin = Layered::for_page(None, "en").values("--moss-font-heading").0.unwrap();
        let han = Layered::for_page(None, "zh-Hans").values("--moss-font-heading").0.unwrap();
        let hant = Layered::for_page(None, "zh-Hant").values("--moss-font-body").0.unwrap();
        assert!(!latin.contains("Songti SC") || latin.find("Source Serif") < latin.find("Songti SC"), "{latin}");
        assert!(han.trim_start().starts_with("'Source Han Serif SC'"), "{han}");
        assert!(hant.contains("'PingFang TC'"), "{hant}");
        let themed = Layered::for_page(Some(":root { --moss-font-heading: Georgia, serif; }"), "ja").values("--moss-font-heading").0;
        assert_eq!(themed.as_deref(), Some("Georgia, serif"));
    }

    #[test]
    fn only_the_token_missing_a_dark_value_is_named() {
        let css = ":root { --moss-color-bg: #fafafa; --moss-color-accent: #225; } :root[data-theme=\"dark\"] { --moss-color-bg: #111; }";
        let found = dark_gap(css);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("--moss-color-accent") && !found[0].contains("--moss-color-bg"), "{found:?}");
    }

    #[test]
    fn tokens_declared_for_both_schemes_are_silent() {
        let css = ":root { --moss-color-bg: #fafafa; } :root[data-theme='dark'] { --moss-color-bg: #111; }";
        assert!(dark_gap(css).is_empty());
    }

    #[test]
    fn a_var_reference_follows_into_dark_mode() {
        let css = ":root { --moss-color-text-secondary: var(--moss-color-muted); } :root[data-theme=\"dark\"] { --moss-color-muted: #aaa; }";
        assert!(dark_gap(css).is_empty());
    }

    #[test]
    fn other_dark_selector_forms_count_as_dark() {
        for dark in ["html[data-theme=\"dark\"]", "[data-theme=\"dark\"]", ":root[data-theme=dark]", ":root[data-theme='dark']"] {
            let css = format!(":root {{ --moss-color-bg: #fafafa; }} {dark} {{ --moss-color-bg: #111; }}");
            assert!(dark_gap(&css).is_empty(), "{dark}");
        }
    }

    #[test]
    fn a_comment_marker_inside_a_string_does_not_hide_later_tokens() {
        let css = ".a { background: url(\"a/*b\"); } :root { --moss-color-bg: #fafafa; }";
        assert_eq!(dark_gap(css).len(), 1);
    }

    #[test]
    fn a_media_dark_block_is_not_a_dark_value() {
        let css = ":root { --moss-color-bg: #fafafa; } @media (prefers-color-scheme: dark) { :root { --moss-color-bg: #111; } }";
        assert_eq!(dark_gap(css).len(), 1);
    }

    #[test]
    fn commented_out_and_non_colour_tokens_are_silent() {
        let css = ":root {\n  /* --moss-color-accent: #2d5a2d; */\n  --moss-font-body: serif;\n}";
        assert!(dark_gap(css).is_empty());
    }

    /// The critical false-positive guard: a theme that only sets escape-hatch
    /// custom properties (never declared in `:root`, only read via `var()`
    /// with a fallback) and overrides a real token must produce zero warnings.
    #[test]
    fn correct_theme_with_custom_props_has_no_warnings() {
        let css = r#"
            :root {
                --moss-color-accent: #c00;
            }
            :root[data-theme="dark"] {
                --moss-color-accent: #f66;
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
        let warnings = lint_theme_css(css, ColorScheme::Auto);
        assert!(warnings.is_empty(), "expected no warnings, got: {:?}", warnings);
    }

    /// `--moss-footer-end` is moss's own footer end-space hook, so a theme
    /// that sets it (here to end the page on a picture) is not flagged.
    #[test]
    fn footer_end_space_property_has_no_warning() {
        let css = "footer.container { --moss-footer-end: 0; }";
        let warnings = lint_theme_css(css, ColorScheme::Auto);
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
        let warnings = lint_theme_css(css, ColorScheme::Auto);
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
        let warnings = lint_theme_css(css, ColorScheme::Auto);
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
        let warnings = lint_theme_css(css, ColorScheme::Auto);
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
        let warnings = lint_theme_css(&css, ColorScheme::Auto);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains("pre-v1 class"));
        assert!(warnings[0].contains("migration.html"));
    }

    /// An unknown class (not retired, not a component, not plugin-namespaced)
    /// gets the generic unknown-class warning naming the closest real class.
    #[test]
    fn unknown_class_names_closest_component() {
        let css = ".moss-carrd { color: red; }";
        let warnings = lint_theme_css(css, ColorScheme::Auto);
        assert_eq!(warnings.len(), 1, "warnings: {:?}", warnings);
        assert!(warnings[0].contains(".moss-carrd"));
        assert!(warnings[0].contains(".moss-card"));
    }

    /// Plugin-namespaced classes are never checked against COMPONENTS —
    /// COMPONENTS has no entries for them by design.
    #[test]
    fn plugin_namespaced_class_has_no_warning() {
        let css = ".moss-embed-plugin-graphviz { border: 1px solid; }";
        let warnings = lint_theme_css(css, ColorScheme::Auto);
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
