//! Class names the theme invents: classes in `.moss/theme/**/*.css` (and the
//! sheets they `@import`) that moss does not write itself, each with whether
//! the site's Markdown uses it.
//!
//! "Moss writes it" is read from what moss already keeps: the component
//! registry that `moss describe --json` prints as `components`, and the class
//! selectors of the stylesheets moss ships, which style markup moss emits
//! (`is-selected`, `nav-search-btn`). A theme that names only those styles
//! the page by structure and by moss's own markup.
//!
//! Two limits: a class that moss's stylesheets style but moss never emits
//! counts as moss's, and a class moss builds with `format!` and does not
//! register may be listed. Nested rules (`.a { &.b { } }`) are not read; only
//! top-level rules and those inside at-rules are.
//!
//! Class-like text in attribute selectors (`a[href$=".pdf"]`), strings,
//! comments and declarations is not a class; only selector preludes are read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser};
use regex::Regex;
use scraper::{Html, Selector};

use super::theme_rules::NOT_SELECTOR_BLOCKS;
use crate::build::theme_lint::{strip_comments, style_rules};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnClass {
    pub class: String,
    /// Stylesheet and line of the first rule that names the class.
    pub defined: (String, usize),
    /// How many pages use it; 0 when none does.
    pub pages: usize,
    /// First page, in sorted order, that uses it, and the line.
    pub example: Option<(String, usize)>,
}

/// Class names (without the dot) a stylesheet's selectors mention, each with
/// the line of its first rule.
fn selector_classes(css: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for rule in style_rules(css) {
        if rule.conditions.iter().any(|c| NOT_SELECTOR_BLOCKS.iter().any(|b| c.to_ascii_lowercase().contains(b))) {
            continue;
        }
        let selector = without_attribute_selectors(&strip_comments(rule.prelude));
        for class in class_idents(&selector) {
            out.entry(class).or_insert_with(|| css[..rule.start].matches('\n').count() + 1);
        }
    }
    out
}

/// The class names of a selector, CSS escapes decoded (`.sm\:flex` is `sm:flex`).
fn class_idents(selector: &str) -> Vec<String> {
    let chars: Vec<char> = selector.chars().collect();
    let (mut out, mut i, mut quote) = (Vec::new(), 0, None::<char>);
    while i < chars.len() {
        let c = chars[i];
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => i += 1,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '\\') => i += 1,
            (None, '.') => {
                let (mut name, mut j) = (String::new(), i + 1);
                while j < chars.len() {
                    match chars[j] {
                        '\\' if j + 1 < chars.len() => {
                            let hex: String = chars[j + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).take(6).collect();
                            if let Some(decoded) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                name.push(decoded);
                                j += 1 + hex.len();
                                if chars.get(j).is_some_and(|c| c.is_whitespace()) {
                                    j += 1;
                                }
                            } else {
                                name.push(chars[j + 1]);
                                j += 2;
                            }
                        }
                        c if c.is_alphanumeric() || c == '-' || c == '_' => {
                            name.push(c);
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) {
                    out.push(name);
                }
                i = j.max(i + 1);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// `selector` with every `[...]` group removed, brackets inside quotes aside.
fn without_attribute_selectors(selector: &str) -> String {
    let (mut out, mut depth, mut quote) = (String::new(), 0usize, None::<char>);
    for c in selector.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') if depth > 0 => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') if depth > 0 => depth -= 1,
            (None, c) if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Every class moss writes: the component registry plus the class selectors of
/// the stylesheets moss ships.
fn moss_classes() -> BTreeSet<String> {
    let mut out: BTreeSet<String> = moss_core::contract::components::COMPONENTS.iter().map(|c| c.class.to_string()).collect();
    for (_, css) in crate::cli::describe::css_query::sheets() {
        out.extend(selector_classes(css).into_keys());
    }
    out
}

/// The `@import`ed sheets of `sheets` that sit outside the list already, read
/// from inside `folder`, followed to any depth.
fn imported_sheets(folder: &Path, sheets: &[(String, String)]) -> Vec<(String, String)> {
    let import = Regex::new(r#"@import\s+(?:url\(\s*)?["']?([^"')\s;]+)"#).expect("static regex");
    let mut seen: BTreeSet<String> = sheets.iter().map(|(p, _)| p.clone()).collect();
    let mut queue: Vec<(String, String)> = sheets.to_vec();
    let mut out = Vec::new();
    while let Some((rel, css)) = queue.pop() {
        let dir = Path::new(&rel).parent().map(Path::to_path_buf).unwrap_or_default();
        for target in import.captures_iter(&strip_comments(&css)).map(|c| c[1].to_string()) {
            if target.contains("://") || target.starts_with('/') {
                continue;
            }
            let mut path = PathBuf::new();
            for part in dir.join(&target).components() {
                match part {
                    std::path::Component::ParentDir => {
                        path.pop();
                    }
                    std::path::Component::Normal(p) => path.push(p),
                    _ => {}
                }
            }
            let rel_import = path.to_string_lossy().replace('\\', "/");
            if !seen.insert(rel_import.clone()) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(folder.join(&path)) {
                queue.push((rel_import.clone(), text.clone()));
                out.push((rel_import, text));
            }
        }
    }
    out
}

/// Class names a page's Markdown writes, with the line of each: the classed
/// divs the build's extractor finds, and `class=` on HTML in the page. Code
/// blocks and spans are not HTML events, so they never count.
fn page_classes(content: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for region in super::page_css_regions(content) {
        for class in region.classes {
            out.entry(class).or_insert(region.line);
        }
    }
    let (body, offset) = super::super::page_body(content);
    let with_class = Selector::parse("[class]").expect("static selector");
    for (event, range) in Parser::new(&body).into_offset_iter() {
        let (Event::Html(html) | Event::InlineHtml(html)) = event else { continue };
        let line = offset + super::super::line_of(&body, range.start);
        for el in Html::parse_fragment(&html).select(&with_class) {
            for class in el.value().classes() {
                out.entry(class.to_string()).or_insert(line);
            }
        }
    }
    out
}

/// The classes the theme names that moss does not write, by name.
pub fn check(folder: &Path, sheets: &[(String, String)]) -> Vec<OwnClass> {
    let imported = imported_sheets(folder, sheets);
    let mut named: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (path, css) in sheets.iter().chain(&imported) {
        for (class, line) in selector_classes(css) {
            named.entry(class).or_insert_with(|| (path.clone(), line));
        }
    }
    if named.is_empty() {
        return Vec::new();
    }
    let moss = moss_classes();
    named.retain(|class, _| !moss.contains(class));

    let mut used: BTreeMap<String, (usize, (String, usize))> = BTreeMap::new();
    for page in super::super::markdown_files(folder) {
        let Ok(content) = std::fs::read_to_string(&page) else { continue };
        let rel = super::site_relative(folder, &page);
        for (class, line) in page_classes(&content) {
            if named.contains_key(&class) {
                used.entry(class).or_insert_with(|| (0, (rel.clone(), line))).0 += 1;
            }
        }
    }
    named
        .into_iter()
        .map(|(class, defined)| {
            let (pages, example) = used.remove(&class).map_or((0, None), |(n, e)| (n, Some(e)));
            OwnClass { class, defined, pages, example }
        })
        .collect()
}

#[cfg(test)]
#[path = "own_classes_tests.rs"]
mod tests;
