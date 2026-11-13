//! A site's look as its own stylesheets state it: fonts, colours, the type
//! scale and the measure. Pure: the stylesheets arrive already fetched, in the
//! home page's document order.
//!
//! A small cascade, not a layout engine. Each named element (the home page's
//! body; a post's title and section headings, an inline link and a date, read
//! from the home page when the crawl found no post; a code sample)
//! takes the declarations of every rule whose selector matches it, by
//! scraper's own matcher. Inherited properties come from the nearest ancestor
//! that states them, `var()` resolves through the custom properties the
//! element inherits, and `rem`, `em` and `%` resolve against the sizes the
//! stylesheets set. `@media (prefers-color-scheme: dark)` is the dark variant;
//! a rule under any other condition is skipped. Three limits are deliberate:
//! document order decides (no specificity, no `!important`), and the only
//! shorthands read are `font` and a single-colour `background`.

use std::collections::HashMap;

use scraper::{ElementRef, Html, Selector};
use url::Url;

use super::facts::sel;
use crate::build::components::color_extract::{colour, Rgba};
use crate::build::theme_lint::{declarations, split_top_level, strip_comments, style_rules, weight_of, ROOT_PX};

/// The browser's own defaults that change a fact when a site leaves them alone.
const USER_AGENT_SHEET: &str =
    "h1, h2, h3, h4, h5, h6 { font-weight: bold } code, kbd, pre, samp { font-family: monospace } a { color: #0000ee }";

/// How deep a `var()` chain is followed, which also ends a cycle.
const MAX_VAR_DEPTH: usize = 8;

/// One stylesheet of the home page and the address its `url()`s resolve against.
pub(crate) struct Sheet {
    pub css: String,
    pub base: Url,
}

/// The colours of one scheme.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Colours {
    pub text: Option<Rgba>,
    pub background: Option<Rgba>,
    pub link: Option<Rgba>,
    /// A root custom property the site names as its accent (`--ghost-accent-color`).
    pub accent: Option<Rgba>,
    /// The colour of a date, when it differs from the text.
    pub muted: Option<Rgba>,
}

/// The first family of a stack, unquoted, and the stack's generic family
/// (`sans-serif`, `serif`, `monospace`, lowercase) when it names one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Family {
    pub name: String,
    pub generic: Option<String>,
}

/// One `@font-face` of the source, its file chosen (WOFF2 when offered).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontFace {
    pub family: String,
    /// Every descriptor but `font-family` and `src`, as written.
    pub descriptors: Vec<(String, String)>,
    pub source: String,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StyleFacts {
    pub body_family: Option<Family>,
    pub heading_family: Option<Family>,
    pub mono_family: Option<Family>,
    pub light: Colours,
    /// `None` when the source has no dark scheme.
    pub dark: Option<Colours>,
    /// The size of a paragraph of running text.
    pub base_size_px: Option<f32>,
    /// That paragraph's line height, as a multiple of its size.
    pub line_height: Option<f32>,
    pub heading_weight: Option<u16>,
    /// The nearest `max-width` around the section heading.
    pub content_width_px: Option<f32>,
    pub font_faces: Vec<FontFace>,
}

struct Rule {
    selectors: Vec<Selector>,
    declarations: Vec<(String, String)>,
    dark_only: bool,
}

/// `post` is a page of the same site whose text holds a link, where a theme's
/// reading styles (the measure, link colour, headings in the text) show.
pub(crate) fn read(html: &str, post: Option<&str>, sheets: &[Sheet]) -> StyleFacts {
    let (author, font_faces) = collect(sheets.iter());
    // A page with no stylesheet of its own shows the browser's look, not a design.
    if author.is_empty() {
        return StyleFacts::default();
    }
    let user_agent = Sheet { css: USER_AGENT_SHEET.to_string(), base: Url::parse("about:blank").expect("static url") };
    let rules: Vec<Rule> = collect(std::iter::once(&user_agent)).0.into_iter().chain(author).collect();
    let doc = Html::parse_document(html);
    let post = post.map(Html::parse_document);
    let text = post.as_ref().unwrap_or(&doc);
    let Some(body) = first(&doc, "body") else { return StyleFacts::default() };
    // Without an h1, the title heading is an entry's: the first h2 or h3 that is a link.
    let is_link = |h: &ElementRef| h.select(&sel("a[href]")).next().is_some() || h.ancestors().filter_map(ElementRef::wrap).any(|a| a.value().name() == "a");
    let title = first(text, "h1").or_else(|| text.select(&sel("h2, h3")).find(is_link));
    let section = first(text, "h2").or_else(|| first(text, "h3"));
    let link = text_link(text, TEXT_LINK).or_else(|| text_link(text, "a[href]"));
    let code = first(text, "code").or_else(|| first(&doc, "code"));
    let date = first(text, "time");

    let light = Cascade { rules: &rules, dark: false };
    let body_chain = light.chain(body);
    let body_style = body_chain.last().expect("a chain ends at its element");
    let heading = title.map(|t| light.style(t));
    // The reading size and leading are those of the paragraph holding the text
    // link, else the body's: a home page's card blurbs and tags are no reading text.
    let paragraph = link.and_then(|a| a.ancestors().filter_map(ElementRef::wrap).find(|e| matches!(e.value().name(), "p" | "li")));
    let reading = paragraph.map_or_else(|| body_style.clone(), |p| light.style(p));
    let line_height = reading.inherited.get("line-height").and_then(|v| match v.strip_suffix("px") {
        Some(px) => px.parse::<f32>().ok().zip(reading.font_px).map(|(px, size)| px / size),
        None => v.parse().ok(),
    });
    let colours = |cascade: &Cascade| {
        let chain = cascade.chain(body);
        let (root, body) = (&chain[0], chain.last().expect("non-empty"));
        let colour_of = |el: Option<ElementRef>| el.and_then(|e| cascade.style(e).inherited.get("color").and_then(|c| colour(c)));
        let text = body.inherited.get("color").and_then(|c| colour(c));
        let background = [body, root].into_iter().find_map(|s| s.own.get("background-color").and_then(|c| colour(c)).filter(|c| c.alpha > 0.0));
        // Of the root properties a site names its accent, the one its rules use most.
        let uses = |name: &str| rules.iter().flat_map(|r| &r.declarations).filter(|(_, v)| v.contains(&format!("var({name}"))).count();
        let mut names: Vec<(usize, &String)> = root.inherited.keys().filter(|k| k.starts_with("--") && k.contains("accent")).map(|k| (uses(k), k)).collect();
        names.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        let accent = names.into_iter().find_map(|(_, k)| colour(&root.inherited[k]));
        let muted = colour_of(date).filter(|m| Some(m) != text.as_ref());
        Colours { text, background, link: colour_of(link), accent, muted }
    };
    let light_colours = colours(&light);
    let dark_colours = colours(&Cascade { rules: &rules, dark: true });
    StyleFacts {
        body_family: body_style.inherited.get("font-family").and_then(|f| family(f)),
        heading_family: heading.as_ref().and_then(|h| h.inherited.get("font-family")).and_then(|f| family(f)),
        mono_family: code.map(|c| light.style(c)).and_then(|c| c.inherited.get("font-family").and_then(|f| family(f))),
        base_size_px: reading.font_px,
        line_height,
        heading_weight: heading.as_ref().and_then(|h| h.inherited.get("font-weight")).and_then(|w| weight_of(w)),
        content_width_px: section.and_then(|s| light.chain(s).iter().rev().find_map(|c| c.own.get("max-width").and_then(|w| length_px(w, c.font_px, c.root_px)))),
        dark: (dark_colours != light_colours).then_some(dark_colours),
        light: light_colours,
        font_faces,
    }
}

/// A link inside a paragraph or list item: one in a page's running text.
const TEXT_LINK: &str = "p a[href], li a[href]";

/// The first `css` link outside the page's header, nav and footer, skipping
/// in-page links (a skip link).
fn text_link<'a>(doc: &'a Html, css: &str) -> Option<ElementRef<'a>> {
    let in_chrome = |a: &ElementRef| a.ancestors().filter_map(ElementRef::wrap).any(|e| matches!(e.value().name(), "nav" | "header" | "footer"));
    doc.select(&sel(css)).find(|a| !in_chrome(a) && !a.value().attr("href").unwrap_or_default().starts_with('#'))
}

/// Whether a page's text holds a link: a page that shows the reading styles.
pub(crate) fn has_text_link(html: &str) -> bool {
    text_link(&Html::parse_document(html), TEXT_LINK).is_some()
}

fn first<'a>(doc: &'a Html, css: &str) -> Option<ElementRef<'a>> {
    doc.select(&sel(css)).next()
}

fn collect<'s>(sheets: impl Iterator<Item = &'s Sheet>) -> (Vec<Rule>, Vec<FontFace>) {
    let (mut rules, mut faces) = (Vec::new(), Vec::new());
    for sheet in sheets {
        let css = strip_comments(&sheet.css);
        for rule in style_rules(&css) {
            let declarations = rule.declarations();
            if rule.prelude.eq_ignore_ascii_case("@font-face") {
                faces.extend(font_face(&declarations, &sheet.base));
                continue;
            }
            let Some(dark_only) = scheme_of(&rule.conditions) else { continue };
            let selectors: Vec<Selector> = split_top_level(rule.prelude, ',').into_iter().filter_map(|s| Selector::parse(s).ok()).collect();
            if !selectors.is_empty() {
                rules.push(Rule { selectors, declarations, dark_only });
            }
        }
    }
    (rules, faces)
}

/// `Some(true)` for a rule only the dark scheme applies, `Some(false)` for one
/// both apply, `None` for a rule under any other condition.
fn scheme_of(conditions: &[&str]) -> Option<bool> {
    let mut dark = false;
    for condition in conditions {
        let c: String = condition.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        match c.as_str() {
            _ if c.starts_with("@layer") => {}
            "@mediascreen" | "@mediaall" => {}
            "@media(prefers-color-scheme:dark)" | "@mediascreenand(prefers-color-scheme:dark)" => dark = true,
            _ => return None,
        }
    }
    Some(dark)
}

fn font_face(declarations: &[(String, String)], base: &Url) -> Option<FontFace> {
    let (mut family, mut chosen, mut descriptors) = (None, None, Vec::new());
    for (p, v) in declarations {
        match p.as_str() {
            "font-family" => family = Some(unquote(v)),
            "src" => {
                let sources: Vec<(String, Option<String>)> = split_top_level(v, ',')
                    .into_iter()
                    .filter_map(|part| Some((base.join(&function_arg(part, "url(")?).ok()?.to_string(), function_arg(part, "format("))))
                    .collect();
                let woff2 = |(url, format): &&(String, Option<String>)| format.as_deref() == Some("woff2") || Url::parse(url).is_ok_and(|u| u.path().ends_with(".woff2"));
                chosen = sources.iter().find(woff2).or(sources.first()).cloned();
            }
            _ => descriptors.push((p.clone(), v.clone())),
        }
    }
    let (source, format) = chosen?;
    Some(FontFace { family: family?, descriptors, source, format })
}

/// The unquoted argument of the first `name(` call in `s`.
fn function_arg(s: &str, name: &str) -> Option<String> {
    Some(unquote(s.split_once(name)?.1.split_once(')')?.0))
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn family(stack: &str) -> Option<Family> {
    let names: Vec<String> = split_top_level(stack, ',').into_iter().map(unquote).collect();
    let name = names.first().filter(|f| !f.is_empty())?.clone();
    let generic = names.iter().rev().map(|n| n.to_ascii_lowercase()).find(|n| matches!(n.as_str(), "sans-serif" | "serif" | "monospace"));
    Some(Family { name, generic })
}

/// A length in px: `px`, `rem` against the root size, `em` and `%` against
/// `em_px`. `None` for anything else (`calc()`, `clamp()`, viewport units) and
/// for a relative length against a size that is itself unknown.
fn length_px(value: &str, em_px: Option<f32>, root_px: Option<f32>) -> Option<f32> {
    let v = value.trim();
    let num = |suffix: &str| v.strip_suffix(suffix).and_then(|n| n.trim().parse::<f32>().ok());
    num("px")
        .or_else(|| Some(num("rem")? * root_px?))
        .or_else(|| Some(num("em")? * em_px?))
        .or_else(|| Some(num("%")? * em_px? / 100.0))
}

/// One element's values: the inherited ones (custom properties included) and its own.
#[derive(Clone)]
struct Computed {
    inherited: HashMap<String, String>,
    own: HashMap<String, String>,
    /// `None` once a size on the way down is one the reader cannot resolve.
    font_px: Option<f32>,
    root_px: Option<f32>,
}

struct Cascade<'r> {
    rules: &'r [Rule],
    dark: bool,
}

impl Cascade<'_> {
    fn style(&self, el: ElementRef) -> Computed {
        self.chain(el).pop().expect("a chain ends at its element")
    }

    /// The computed values from the root down to `el`.
    fn chain(&self, el: ElementRef) -> Vec<Computed> {
        let mut path: Vec<ElementRef> = el.ancestors().filter_map(ElementRef::wrap).collect();
        path.reverse();
        path.push(el);
        let mut out: Vec<Computed> = Vec::with_capacity(path.len());
        for e in path {
            let parent = out.last().cloned().unwrap_or(Computed { inherited: HashMap::new(), own: HashMap::new(), font_px: Some(ROOT_PX), root_px: Some(ROOT_PX) });
            let mut next = self.step(&parent, e);
            if out.is_empty() {
                next.root_px = next.font_px;
            }
            out.push(next);
        }
        out
    }

    /// The declarations matching `el`, the last of each property winning, with
    /// `font` and `background` expanded.
    fn declared(&self, el: ElementRef) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let style = el.value().attr("style").map(declarations).unwrap_or_default();
        let matching = self.rules.iter().filter(|r| (self.dark || !r.dark_only) && r.selectors.iter().any(|s| s.matches(&el)));
        for (p, v) in matching.flat_map(|r| r.declarations.iter()).chain(style.iter()) {
            match p.as_str() {
                "font" => out.extend(font_shorthand(v)),
                "background" => {
                    out.insert("background-color".to_string(), v.clone());
                }
                _ => {
                    out.insert(p.clone(), v.clone());
                }
            }
        }
        out
    }

    fn step(&self, parent: &Computed, el: ElementRef) -> Computed {
        let declared = self.declared(el);
        let mut inherited = parent.inherited.clone();
        for (p, v) in declared.iter().filter(|(p, _)| p.starts_with("--")) {
            inherited.insert(p.clone(), v.clone());
        }
        let customs: Vec<String> = declared.keys().filter(|p| p.starts_with("--")).cloned().collect();
        for p in customs {
            match substitute(&inherited[&p], &inherited, 0) {
                Some(v) => inherited.insert(p, v),
                None => inherited.remove(&p),
            };
        }
        let values: Vec<(String, String)> = declared
            .iter()
            .filter(|(p, _)| !p.starts_with("--"))
            .filter_map(|(p, v)| Some((p.clone(), substitute(v, &inherited, 0)?)))
            .filter(|(_, v)| v != "inherit" && v != "unset")
            .collect();
        let font_size = values.iter().find(|(p, _)| p == "font-size");
        // A size the reader cannot resolve is no fact, not the parent's size.
        let font_px = match font_size {
            Some((_, v)) => length_px(v, parent.font_px, parent.root_px),
            None => parent.font_px,
        };
        let mut own = HashMap::new();
        for (p, v) in values.into_iter().filter(|(p, _)| p != "font-size") {
            match p.as_str() {
                "line-height" => match length_px(&v, font_px, parent.root_px) {
                    Some(px) if v.parse::<f32>().is_err() => inherited.insert(p, format!("{px}px")),
                    _ => inherited.insert(p, v),
                },
                "color" | "font-family" | "font-weight" => inherited.insert(p, v),
                _ => own.insert(p, v),
            };
        }
        Computed { inherited, own, font_px, root_px: parent.root_px }
    }
}

/// `value` with every `var(--x, fallback)` replaced; `None` when one names
/// nothing and has no fallback, which makes the declaration invalid.
fn substitute(value: &str, vars: &HashMap<String, String>, depth: usize) -> Option<String> {
    let Some((head, tail)) = value.split_once("var(") else { return Some(value.to_string()) };
    if depth > MAX_VAR_DEPTH {
        return None;
    }
    let mut open = 1;
    let close = tail.char_indices().find_map(|(i, c)| {
        open += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        (open == 0).then_some(i)
    })?;
    let (inner, rest) = tail.split_at(close);
    let (name, fallback) = inner.split_once(',').map_or((inner, None), |(n, f)| (n, Some(f)));
    let replacement = match vars.get(name.trim()) {
        Some(v) => substitute(v, vars, depth + 1),
        None => fallback.and_then(|f| substitute(f.trim(), vars, depth + 1)),
    }?;
    let rest = substitute(rest.strip_prefix(')').unwrap_or(rest), vars, depth)?;
    Some(format!("{head}{replacement}{rest}"))
}

/// `font: [style] [weight] size[/line-height] family` as its longhands; a
/// keyword (`inherit`) sets all four.
fn font_shorthand(value: &str) -> Vec<(String, String)> {
    let longhands = ["font-family", "font-size", "font-weight", "line-height"];
    if !value.contains(' ') {
        return longhands.iter().map(|p| (p.to_string(), value.to_string())).collect();
    }
    let words: Vec<&str> = value.split_whitespace().collect();
    let Some(at) = words.iter().position(|w| w.starts_with(|c: char| c.is_ascii_digit() || c == '.') && !w.chars().all(|c| c.is_ascii_digit())) else {
        return Vec::new();
    };
    let mut out = vec![("font-family".to_string(), words[at + 1..].join(" "))];
    let (size, line) = words[at].split_once('/').map_or((words[at], None), |(s, l)| (s, Some(l)));
    out.push(("font-size".to_string(), size.to_string()));
    out.extend(line.map(|l| ("line-height".to_string(), l.to_string())));
    out.extend(words[..at].iter().find(|w| weight_of(w).is_some()).map(|w| ("font-weight".to_string(), w.to_string())));
    out
}

#[cfg(test)]
#[path = "style_tests.rs"]
mod tests;
