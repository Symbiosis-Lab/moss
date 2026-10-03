//! Read a home page's raw HTML for the facts a site's chrome is made of. Pure:
//! no network, no files. Every URL it returns is absolute.

use std::collections::HashSet;

use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use url::Url;

use super::super::crawler::host_of;
use super::super::extractor::LAZY_SRC_ATTRS;
use super::super::metadata::{entry_string, has_type, parse_schema_org, read_meta_map, SEPARATOR};

/// Default-icon URLs a site builder serves when its owner never uploaded one.
/// Rows are platform placeholders, not site icons: a match means "this site has
/// no icon of its own", so the candidate is dropped rather than imported.
/// Matched as a substring of `host + path`. Add a platform by adding a row.
const DEFAULT_FAVICON_PATTERNS: &[&str] = &[
    "pfavico",
    "parastorage.com",
    "assets.squarespace.com/universal/default-favicon",
    "s0.wp.com/i/favicon",
    "cdn.shopify.com/shopifycloud",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NavEntry {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IconKind {
    Svg,
    Png,
    Ico,
}

impl IconKind {
    pub(super) fn extension(self) -> &'static str {
        match self {
            IconKind::Svg => "svg",
            IconKind::Png => "png",
            IconKind::Ico => "ico",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Icon {
    pub url: String,
    pub kind: IconKind,
}

#[derive(Debug, Default)]
pub(super) struct Facts {
    pub site_name: Option<String>,
    pub nav: Vec<NavEntry>,
    pub logo: Option<String>,
    /// The footer as markdown, links absolute.
    pub footer: Option<String>,
    pub favicon: Option<Icon>,
}

/// A URL as the crawl keys it: no fragment, no trailing slash except the root.
pub(crate) fn norm_url(url: &str) -> Option<String> {
    let mut url = Url::parse(url).ok()?;
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { &path });
    Some(url.to_string())
}

pub(super) fn read(html: &str, page_url: &str) -> Facts {
    let Ok(base) = Url::parse(page_url) else {
        return Facts::default();
    };
    let doc = Html::parse_document(html);
    let nav = read_nav(&doc, &base);
    let nav_urls: HashSet<String> = nav.iter().filter_map(|e| norm_url(&e.url)).collect();
    Facts {
        site_name: read_site_name(&doc),
        logo: read_logo(&doc, &base),
        footer: super::footer::read(&doc, &base, &nav_urls),
        favicon: read_favicon(&doc, &base),
        nav,
    }
}

pub(super) fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("static selector")
}

pub(super) fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_platform_default(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    let target = format!("{}{}", u.host_str().unwrap_or(""), u.path());
    DEFAULT_FAVICON_PATTERNS.iter().any(|p| target.contains(p))
}

// ── site name ─────────────────────────────────────────────────────────────

fn read_site_name(doc: &Html) -> Option<String> {
    if let Some(name) = read_meta_map(doc).get("og:site_name") {
        return Some(collapse(name));
    }
    if let Some(name) = parse_schema_org(doc)
        .iter()
        .filter(|e| has_type(e, &["Organization", "WebSite"]))
        .find_map(|e| entry_string(e, "name"))
    {
        return Some(name);
    }
    let title = collapse(&doc.select(&sel("title")).next()?.text().collect::<String>());
    name_in_home_title(&title)
}

/// Words a home page is titled with instead of a name.
const GENERIC_TITLES: &[&str] = &["home", "homepage", "welcome", "index", "start", "main"];

/// Whether `title` says nothing about the site: empty, or every segment of it
/// a generic word or the site's own `name`.
pub(super) fn is_generic_title(title: &str, name: &str) -> bool {
    SEPARATOR.split(title).map(str::trim).filter(|s| !s.is_empty()).all(|s| {
        GENERIC_TITLES.contains(&s.to_lowercase().as_str()) || s.eq_ignore_ascii_case(name)
    })
}

/// The site's name out of its home page's `<title>`: the first segment (a home
/// title leads with the name, `Studio Name | Costume design`), unless that is
/// a generic word (`Home | Studio Name`), then the last. `None` when every
/// segment is generic.
fn name_in_home_title(title: &str) -> Option<String> {
    let segments: Vec<&str> = SEPARATOR.split(title).map(str::trim).filter(|s| !s.is_empty()).collect();
    let generic = |s: &&str| GENERIC_TITLES.contains(&s.to_lowercase().as_str());
    let first = segments.first().filter(|s| !generic(s));
    first.or_else(|| segments.iter().rev().find(|s| !generic(s))).map(|s| s.to_string())
}

// ── navigation ────────────────────────────────────────────────────────────

fn read_nav(doc: &Html, base: &Url) -> Vec<NavEntry> {
    let all: Vec<ElementRef> = doc.select(&sel("nav, [role=navigation]")).collect();
    let in_header = |el: &ElementRef| {
        el.ancestors().filter_map(ElementRef::wrap).any(|a| a.value().name() == "header")
    };
    let ordered = all
        .iter()
        .filter(|e| in_header(e))
        .chain(all.iter().filter(|e| !in_header(e) && e.value().name() == "nav"));
    for nav in ordered {
        let entries = nav_entries(*nav, base);
        if entries.len() >= 2 {
            return entries;
        }
    }
    Vec::new()
}

/// The same-host links of one nav in order, top level only: a dropdown's
/// submenu is not the site's primary navigation.
fn nav_entries(nav: ElementRef, base: &Url) -> Vec<NavEntry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for a in nav.select(&sel("a[href]")) {
        let depth = a
            .ancestors()
            .take_while(|n| n.id() != nav.id())
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "li")
            .count();
        if depth > 1 {
            continue;
        }
        let Some(url) = a
            .value()
            .attr("href")
            .map(str::trim)
            .filter(|h| !h.starts_with('#'))
            .and_then(|h| base.join(h).ok())
        else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || host_of(url.as_str()) != host_of(base.as_str()) {
            continue;
        }
        let label = collapse(&a.text().collect::<String>());
        let label = if label.is_empty() {
            ["aria-label", "title"]
                .iter()
                .find_map(|k| a.value().attr(k).map(collapse))
                .unwrap_or_default()
        } else {
            label
        };
        let Some(key) = norm_url(url.as_str()) else { continue };
        if label.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(NavEntry { label, url: url.to_string() });
    }
    out
}

// ── logo ──────────────────────────────────────────────────────────────────

fn read_logo(doc: &Html, base: &Url) -> Option<String> {
    let home = norm_url(base.as_str());
    let links_home = |a: ElementRef| {
        a.value().attr("href").and_then(|h| base.join(h).ok()).is_some_and(|u| {
            host_of(u.as_str()) == host_of(base.as_str()) && (u.path() == "/" || norm_url(u.as_str()) == home)
        })
    };
    let is_logo_marker = |e: ElementRef| {
        let v = e.value();
        v.attr("rel").is_some_and(|r| r.split_whitespace().any(|t| t == "home"))
            || v.attr("itemprop").is_some_and(|p| p.to_lowercase().contains("logo"))
            || ["class", "id"].iter().filter_map(|a| v.attr(a)).any(|names| {
                names
                    .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
                    .any(|w| w.eq_ignore_ascii_case("logo"))
            })
    };
    let mut candidates: Vec<String> = Vec::new();
    for img in doc.select(&sel("header img")) {
        let chain: Vec<ElementRef> = std::iter::once(img)
            .chain(img.ancestors().filter_map(ElementRef::wrap))
            .collect();
        let matches = chain.iter().any(|e| {
            is_logo_marker(*e) || (e.value().name() == "a" && links_home(*e))
        });
        let src = std::iter::once(&"src")
            .chain(LAZY_SRC_ATTRS)
            .filter_map(|k| img.value().attr(k))
            .map(str::trim)
            .find(|s| !s.is_empty() && !s.starts_with("data:"));
        if let (true, Some(src)) = (matches, src) {
            if let Ok(url) = base.join(src) {
                candidates.push(url.to_string());
            }
        }
    }
    if candidates.is_empty() {
        candidates.extend(schema_logo(doc, base));
    }
    candidates.retain(|u| !is_platform_default(u));
    let is_svg = |u: &String| Url::parse(u).is_ok_and(|u| u.path().to_lowercase().ends_with(".svg"));
    candidates
        .iter()
        .find(|u| is_svg(u))
        .or_else(|| candidates.first())
        .cloned()
}

fn schema_logo(doc: &Html, base: &Url) -> Option<String> {
    parse_schema_org(doc)
        .iter()
        .filter(|e| has_type(e, &["Organization", "WebSite"]))
        .find_map(|e| match e.get("logo")? {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o.get("url").or_else(|| o.get("contentUrl"))?.as_str().map(String::from),
            _ => None,
        })
        .and_then(|s| base.join(s.trim()).ok())
        .map(|u| u.to_string())
}

// ── favicon ───────────────────────────────────────────────────────────────

/// The best icon the page declares: SVG, else the largest declared PNG, else
/// an ICO. A platform's default placeholder is never a candidate.
fn read_favicon(doc: &Html, base: &Url) -> Option<Icon> {
    let mut best: Option<((u8, std::cmp::Reverse<u32>), Icon)> = None;
    for link in doc.select(&sel("link[rel][href]")) {
        let v = link.value();
        let rel = v.attr("rel").unwrap_or("").to_lowercase();
        let tokens: Vec<&str> = rel.split_whitespace().collect();
        if !tokens.iter().any(|t| t.contains("icon")) || tokens.contains(&"mask-icon") {
            continue;
        }
        let Some(url) = v.attr("href").and_then(|h| base.join(h.trim()).ok()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || is_platform_default(url.as_str()) {
            continue;
        }
        let ty = v.attr("type").unwrap_or("").to_lowercase();
        let path = url.path().to_lowercase();
        let kind = if ty.contains("svg") || path.ends_with(".svg") {
            IconKind::Svg
        } else if ty.contains("png") || path.ends_with(".png") {
            IconKind::Png
        } else if ty.contains("icon") || path.ends_with(".ico") {
            IconKind::Ico
        } else {
            continue;
        };
        let area = v.attr("sizes").map_or(0, largest_declared_size);
        let rank = (kind as u8, std::cmp::Reverse(area));
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, Icon { url: url.to_string(), kind }));
        }
    }
    best.map(|(_, icon)| icon)
}

/// Largest `WxH` area in a `sizes` attribute (`any` and junk count as 0).
fn largest_declared_size(sizes: &str) -> u32 {
    sizes
        .split_whitespace()
        .filter_map(|s| {
            let (w, h) = s.to_lowercase().split_once('x').map(|(w, h)| (w.to_string(), h.to_string()))?;
            Some(w.parse::<u32>().ok()? * h.parse::<u32>().ok()?)
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "facts_tests.rs"]
mod tests;
