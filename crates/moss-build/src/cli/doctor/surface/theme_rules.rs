//! Theme rules that style nothing: rules in `.moss/theme/**/*.css` whose
//! selectors match no element of any page in the last build.
//!
//! A theme rule keyed on a class the pages no longer carry fails silently: the
//! build succeeds and the page just looks different. This finds those by the
//! method UnCSS uses: strip what a static page cannot show (`:hover`,
//! `::before`), then run real selector matching against every built page. A
//! selector the matcher cannot parse, or one whose stripped form would be
//! wrong (a state inside `:not(…)`), counts as matching, so an unfamiliar
//! syntax is never reported as dead.
//!
//! A script can add the class later (`classList.add("is-open")`). A rule that
//! matches nothing but names something a built script also mentions is
//! reported apart, as one that may apply once the script runs.

use std::collections::BTreeSet;
use std::path::Path;

use scraper::{Html, Selector};

use crate::build::theme_lint::style_rules;
use crate::cli::describe::css_query::mentions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmatchedRule {
    /// Site-relative path of the stylesheet.
    pub path: String,
    pub line: usize,
    /// The rule's selector list as written.
    pub selector: String,
    /// Class, id and attribute names of the rule that a built script mentions.
    pub scripted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeRules {
    /// No built page to match against.
    NoBuild,
    Checked { pages: usize, unmatched: Vec<UnmatchedRule> },
}

/// At-rule blocks whose inner preludes are not selectors.
const NOT_SELECTOR_BLOCKS: &[&str] = &["keyframes", "@font-face", "@page", "@counter-style", "@property"];

/// Pseudo-classes that depend on interaction or form state, which a built page
/// never has.
const STATE_PSEUDO_CLASSES: &[&str] = &[
    "active", "autofill", "checked", "default", "disabled", "enabled", "focus", "focus-visible", "focus-within",
    "fullscreen", "hover", "in-range", "indeterminate", "invalid", "link", "modal", "open", "out-of-range",
    "paused", "picture-in-picture", "placeholder-shown", "playing", "popover-open", "read-only", "read-write",
    "required", "optional", "target", "target-within", "user-invalid", "user-valid", "valid", "visited",
];

/// Pseudo-elements CSS2 wrote with one colon.
const LEGACY_PSEUDO_ELEMENTS: &[&str] = &["before", "after", "first-line", "first-letter"];

/// A theme rule whose selectors have not matched any page read so far.
struct Pending {
    path: String,
    line: usize,
    selector: String,
    statics: Vec<Selector>,
    names: Vec<String>,
}

/// Check every theme stylesheet against the pages built into `site`.
pub fn check(sheets: &[(String, String)], site: &Path) -> ThemeRules {
    let mut pending: Vec<Pending> = Vec::new();
    for (path, css) in sheets {
        for rule in style_rules(css) {
            let not_selectors = |c: &&str| NOT_SELECTOR_BLOCKS.iter().any(|b| c.to_ascii_lowercase().contains(b));
            if rule.conditions.iter().any(|c| not_selectors(c)) {
                continue;
            }
            let selector = without_comments(rule.prelude).split_whitespace().collect::<Vec<_>>().join(" ");
            let mut statics = Vec::new();
            let mut names_in = Vec::new();
            let mut unknown = false;
            for one in split_selector_list(&selector) {
                match static_form(one).and_then(|s| Selector::parse(&s).ok()) {
                    Some(parsed) => statics.push(parsed),
                    None => unknown = true,
                }
                names_in.extend(names(one));
            }
            if unknown || statics.is_empty() {
                continue;
            }
            pending.push(Pending { path: path.clone(), line: css[..rule.start].matches('\n').count() + 1, selector, statics, names: names_in });
        }
    }

    let inline_script = Selector::parse("script:not([src]):not([type]), script[type=module]:not([src]), script[type=\"text/javascript\"]:not([src])").expect("static selector");
    let redirect = Selector::parse("meta[http-equiv=refresh i]").expect("static selector");
    let mut pages = 0usize;
    let mut scripts: BTreeSet<String> = BTreeSet::new();
    let walk = walkdir::WalkDir::new(site).follow_links(true).into_iter().filter_entry(|e| e.depth() == 0 || e.file_name() != "_moss");
    for entry in walk.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry.path().extension().and_then(|x| x.to_str());
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        match ext {
            Some("html" | "htm") => {
                let page = Html::parse_document(&text);
                if page.select(&redirect).next().is_some() {
                    continue;
                }
                pages += 1;
                scripts.extend(page.select(&inline_script).map(|s| s.text().collect::<String>()));
                pending.retain(|rule| !rule.statics.iter().any(|s| page.select(s).next().is_some()));
            }
            Some("js" | "mjs") => {
                scripts.insert(text);
            }
            _ => {}
        }
    }
    if pages == 0 {
        return ThemeRules::NoBuild;
    }
    // Site scripts sit under `_moss/` too, which the page walk skips.
    for entry in walkdir::WalkDir::new(site.join("_moss")).follow_links(true).into_iter().filter_map(Result::ok) {
        if matches!(entry.path().extension().and_then(|x| x.to_str()), Some("js" | "mjs")) {
            if let Ok(js) = std::fs::read_to_string(entry.path()) {
                scripts.insert(js);
            }
        }
    }

    let scripts = scripts.into_iter().collect::<Vec<_>>().join("\n");
    let unmatched = pending
        .into_iter()
        .map(|rule| {
            let mut scripted: Vec<String> = rule.names.into_iter().filter(|n| script_mentions(&scripts, n)).collect();
            scripted.sort();
            scripted.dedup();
            UnmatchedRule { path: rule.path, line: rule.line, selector: rule.selector, scripted }
        })
        .collect();
    ThemeRules::Checked { pages, unmatched }
}

/// Whether a script names `name`, counting `data-foo-bar` as `dataset.fooBar`.
fn script_mentions(scripts: &str, name: &str) -> bool {
    if mentions(scripts, name) {
        return true;
    }
    let Some(rest) = name.strip_prefix("data-") else { return false };
    let mut camel = String::new();
    let mut upper = false;
    for c in rest.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            camel.extend(c.to_uppercase());
            upper = false;
        } else {
            camel.push(c);
        }
    }
    !camel.is_empty() && mentions(scripts, &format!("dataset.{camel}"))
}

fn without_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        rest = rest[open + 2..].find("*/").map_or("", |close| &rest[open + 2 + close + 2..]);
        out.push(' ');
    }
    out.push_str(rest);
    out
}

/// Where the identifier starting at `chars[from]` ends.
fn ident_end(chars: &[char], from: usize) -> usize {
    let mut end = from;
    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '-' || chars[end] == '_') {
        end += 1;
    }
    end
}

/// Calls `visit(i, c, depth)` for each char of `chars` outside quotes and
/// escapes, where `depth` counts the parentheses and brackets open before it.
/// A `Some(next)` from `visit` resumes the walk at `next`.
fn syntax_chars(chars: &[char], mut visit: impl FnMut(usize, char, i32) -> Option<usize>) {
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

/// The selectors of a list, split on commas outside parentheses, brackets and quotes.
fn split_selector_list(list: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = list.char_indices().collect();
    let plain: Vec<char> = chars.iter().map(|(_, c)| *c).collect();
    let mut cuts = Vec::new();
    syntax_chars(&plain, |i, c, depth| {
        if c == ',' && depth == 0 {
            cuts.push(chars[i].0);
        }
        None
    });
    let mut out = Vec::new();
    let mut from = 0;
    for cut in cuts {
        out.push(list[from..cut].trim());
        from = cut + 1;
    }
    out.push(list[from..].trim());
    out.retain(|s| !s.is_empty());
    out
}

/// `selector` with its pseudo-elements and state pseudo-classes removed, so
/// it matches whatever element the rule could ever style. A compound left
/// empty becomes `*`. `None` when a state sits inside a functional
/// pseudo-class (`:not(:hover)`), whose stripped form would match differently.
fn static_form(selector: &str) -> Option<String> {
    let chars: Vec<char> = selector.chars().collect();
    let mut cut: Vec<(usize, usize)> = Vec::new();
    let mut nested_state = false;
    syntax_chars(&chars, |i, c, depth| {
        if c != ':' {
            return None;
        }
        let double = chars.get(i + 1) == Some(&':');
        let name_from = i + if double { 2 } else { 1 };
        let mut end = ident_end(&chars, name_from);
        let name = chars[name_from..end].iter().collect::<String>().to_ascii_lowercase();
        if !(double || LEGACY_PSEUDO_ELEMENTS.contains(&name.as_str()) || STATE_PSEUDO_CLASSES.contains(&name.as_str())) {
            return None;
        }
        if depth > 0 {
            nested_state = true;
        }
        if chars.get(end) == Some(&'(') {
            let mut open = 0;
            while end < chars.len() {
                match chars[end] {
                    '(' => open += 1,
                    ')' => open -= 1,
                    _ => {}
                }
                end += 1;
                if open == 0 {
                    break;
                }
            }
        }
        cut.push((i, end));
        Some(end)
    });
    if nested_state {
        return None;
    }
    let mut out = String::new();
    let mut at = 0;
    for (from, to) in cut {
        out.extend(&chars[at..from]);
        let starts = out.is_empty() || out.ends_with(|c: char| c.is_whitespace() || ">+~".contains(c));
        let ends = to >= chars.len() || chars[to].is_whitespace() || ">+~".contains(chars[to]);
        if starts && ends {
            out.push('*');
        }
        at = to;
    }
    out.extend(&chars[at..]);
    Some(out.trim().to_string())
}

/// The class, id and attribute names a selector mentions.
fn names(selector: &str) -> Vec<String> {
    let chars: Vec<char> = selector.chars().collect();
    let mut out = Vec::new();
    syntax_chars(&chars, |i, c, _| {
        if !matches!(c, '.' | '#' | '[') {
            return None;
        }
        let end = ident_end(&chars, i + 1);
        if end > i + 1 {
            out.push(chars[i + 1..end].iter().collect());
        }
        None
    });
    out
}

#[cfg(test)]
#[path = "theme_rules_tests.rs"]
mod tests;
