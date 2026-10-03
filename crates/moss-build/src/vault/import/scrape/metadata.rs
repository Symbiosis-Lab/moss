//! Article metadata extracted from raw HTML.
//!
//! Walks `<script type="application/ld+json">` blocks (schema.org), then falls
//! through to OpenGraph `<meta property="og:…">`, `<meta name="…">`, and the
//! bare `<html lang>` attribute. Produces a struct aligned with moss's
//! frontmatter schema (`BUILTIN_FIELDS`).
//!
//! Resolution priority for each field is documented inline. Generally
//! JSON-LD beats OpenGraph beats `<meta name>` beats raw `<title>`.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use url::Url;

use super::converter::{resolve_url, IMG_PATTERN};

mod event;
pub use event::{derive_event, EventMetadata};

/// Metadata ready to drop into YAML frontmatter, with keys aligned to
/// moss-core's `BUILTIN_FIELDS` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArticleMetadata {
    /// Article title (with `| Site Name` suffix stripped when needed).
    pub title: Option<String>,
    /// Short description / dek.
    pub description: Option<String>,
    /// Author name (or "A and B" for two; "A, B, and C" for three+).
    pub author: Option<String>,
    /// Publication date normalized to `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Publishing outlet name (resolved from schema.org `publisher` ref).
    pub publisher: Option<String>,
    /// Two-letter language code (`en`, `zh`, `de`).
    pub lang: Option<String>,
    /// Local path of the cover image, set by the import pipeline AFTER the
    /// preferred cover image is downloaded (so the path is a real on-disk file).
    /// The metadata adapter itself never sets this — it's a downstream
    /// concern of `scrape::run`. `derive()` always returns `None`.
    pub cover: Option<String>,
    /// Raw absolute URL from `og:image`, populated by `derive()` when the tag
    /// is present. The import pipeline prefers this over the first body image
    /// when selecting `cover:`. Never written to YAML — only used internally
    /// by `scrape::run` to select and download the cover asset.
    pub og_image: Option<String>,
    /// Raw `src` of every image that must never become the fallback cover:
    /// inside `header`/`nav`/`footer`/`aside`, or declared under 64 px. Read
    /// from the full document, since the extractor has already stripped the
    /// chrome by the time the cover is chosen. Never written to YAML.
    pub chrome_images: Vec<String>,
    /// Fields read from a schema.org `Event` block; never touches `date`.
    pub event: EventMetadata,
}

const ARTICLE_TYPES: &[&str] = &[
    "Article",
    "NewsArticle",
    "BlogPosting",
    "Report",
    "OpinionNewsArticle",
    "ReportageNewsArticle",
    "AnalysisNewsArticle",
    "BackgroundNewsArticle",
    "ReviewNewsArticle",
];

/// Build [`ArticleMetadata`] from raw HTML.
pub fn derive(html: &str) -> ArticleMetadata {
    let doc = Html::parse_document(html);
    let entries = parse_schema_org(&doc);
    let article = entries.iter().find(|e| has_type(e, ARTICLE_TYPES));
    let webpage = entries.iter().find(|e| has_type(e, &["WebPage"]));

    let og = read_meta_map(&doc);

    let mut publisher = pick_publisher(&og, article, webpage, &entries);
    // The site's own name, as the page declares it: strippable from the title.
    let site_names: Vec<&str> =
        publisher.iter().map(String::as_str).chain(og.get("og:site_name").map(String::as_str)).collect();
    let title = pick_title(&doc, &og, article, webpage, &site_names);
    // A publisher identical to the title is never information (seen on
    // Strikingly blog pages, where og:site_name repeats the post title).
    if publisher.is_some() && publisher == title {
        publisher = None;
    }

    ArticleMetadata {
        title,
        description: pick_description(&og, article, webpage),
        author: pick_author(&og, article),
        date: pick_date(&og, article, webpage),
        publisher,
        lang: pick_lang(&doc, article, webpage),
        cover: None,
        og_image: og.get("og:image").cloned(),
        chrome_images: chrome_images(&doc),
        event: derive_event(&entries),
    }
}

/// Read every `<script type="application/ld+json">` block, parse each as JSON,
/// and flatten `@graph`-wrapped objects into a single Vec.
pub fn parse_schema_org(doc: &Html) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let sel = match Selector::parse(r#"script[type="application/ld+json"]"#) {
        Ok(s) => s,
        Err(_) => return out,
    };
    for el in doc.select(&sel) {
        let text: String = el.text().collect();
        // Some sites wrap JSON-LD in CDATA / HTML comments; strip basic noise.
        let text = text.trim();
        let text = text
            .strip_prefix("//<![CDATA[")
            .or_else(|| text.strip_prefix("<![CDATA["))
            .unwrap_or(text)
            .trim();
        let text = text
            .strip_suffix("//]]>")
            .or_else(|| text.strip_suffix("]]>"))
            .unwrap_or(text)
            .trim();
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        flatten_into(value, &mut out);
    }
    out
}

fn flatten_into(value: Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(arr) => {
            for v in arr {
                flatten_into(v, out);
            }
        }
        Value::Object(mut map) => {
            if let Some(graph) = map.remove("@graph") {
                if let Value::Array(items) = graph {
                    for v in items {
                        flatten_into(v, out);
                    }
                }
            }
            if !map.is_empty() {
                out.push(Value::Object(map));
            }
        }
        _ => {}
    }
}

fn has_type(entry: &Value, types: &[&str]) -> bool {
    let Some(t) = entry.get("@type") else {
        return false;
    };
    match t {
        Value::String(s) => types.iter().any(|x| s.eq_ignore_ascii_case(x)),
        Value::Array(arr) => arr.iter().any(|v| {
            v.as_str()
                .map(|s| types.iter().any(|x| s.eq_ignore_ascii_case(x)))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn entry_string(entry: &Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| moss_core::html_entities::decode(s.trim()))
        .filter(|s| !s.is_empty())
}

/// The first schema.org `email` the page declares (an Organization or Person
/// entry), without any `mailto:` prefix.
pub fn schema_email(doc: &Html) -> Option<String> {
    parse_schema_org(doc)
        .iter()
        .find_map(|entry| entry_string(entry, "email"))
        .map(|e| e.trim_start_matches("mailto:").to_string())
}

/// Read OpenGraph + standard meta tags into a map keyed by `og:title`,
/// `og:description`, `og:site_name`, `og:image`, `article:published_time`,
/// `article:author`, `description`, `author`. Values are trimmed.
fn read_meta_map(doc: &Html) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    let sel = Selector::parse("meta").unwrap();
    for el in doc.select(&sel) {
        let attrs = el.value();
        let key = attrs
            .attr("property")
            .or_else(|| attrs.attr("name"))
            .or_else(|| attrs.attr("itemprop"));
        let content = attrs.attr("content");
        if let (Some(k), Some(v)) = (key, content) {
            let v = v.trim();
            if !v.is_empty() {
                out.entry(k.to_lowercase()).or_insert(v.to_string());
            }
        }
    }
    out
}

fn pick_title(
    doc: &Html,
    og: &std::collections::HashMap<String, String>,
    article: Option<&Value>,
    webpage: Option<&Value>,
    site_names: &[&str],
) -> Option<String> {
    let clean = |t: String| Some(strip_site_name(&t, site_names));
    // The known site name wins over the brand-looking-suffix guess, which would
    // otherwise cut `Studio Name | Home` down to the brand.
    let clean_guessing = |t: &str| {
        let named = strip_site_name(t, site_names);
        if named != t { Some(named) } else { Some(strip_site_suffix(t)) }
    };
    if let Some(a) = article {
        if let Some(h) = entry_string(a, "headline") {
            return clean(h);
        }
        if let Some(n) = entry_string(a, "name") {
            return clean(n);
        }
    }
    // `og:title` before `<title>`: it is the heading the author chose for
    // sharing, where `<title>` is often a builder's internal page name plus
    // the site name.
    if let Some(v) = og.get("og:title") {
        return clean(v.clone());
    }
    if let Some(w) = webpage {
        if let Some(n) = entry_string(w, "name") {
            return clean_guessing(&n);
        }
    }
    // Final fallback: <title> tag, with site suffix stripped.
    let sel = Selector::parse("title").ok()?;
    let t = doc.select(&sel).next()?;
    let text: String = t.text().collect();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        clean_guessing(trimmed)
    }
}

/// Title segment separators: `|`, `·`, `::`, and spaced `-`, `—`, `–`. A bare
/// dash stays part of the words (`Spider-Man`, `A—B`). The one definition both
/// site-name rules below split on.
pub(crate) static SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\||·|::)\s*|\s+[-—–]\s+").expect("title separator regex"));

pub(crate) fn same_name(segment: &str, name: &str) -> bool {
    segment.trim().to_lowercase() == name.trim().to_lowercase()
}

/// Drop a trailing or leading segment of `title` that equals one of `names`.
/// Never returns an empty title: a title that is only the site name, or whose
/// stripping would leave nothing, comes back unchanged.
pub(crate) fn strip_site_name(title: &str, names: &[&str]) -> String {
    let mut out = title.trim();
    let matches_name = |seg: &str| names.iter().any(|n| !n.trim().is_empty() && same_name(seg, n));
    if let Some(last) = SEPARATOR.find_iter(out).last() {
        let tail = &out[last.end()..];
        let head = out[..last.start()].trim();
        if matches_name(tail) && !head.is_empty() {
            out = head;
        }
    }
    if let Some(first) = SEPARATOR.find(out) {
        let head = &out[..first.start()];
        let rest = out[first.end()..].trim();
        if matches_name(head) && !rest.is_empty() {
            out = rest;
        }
    }
    out.to_string()
}

/// "Article Title | Site Name" → "Article Title". Splits at the last
/// separator, since a site brand is always the final segment, and drops that
/// segment when it looks like a brand (i.e. when the article half is at least
/// as long). The guess for a page that does not name its site.
fn strip_site_suffix(s: &str) -> String {
    if let Some(last) = SEPARATOR.find_iter(s).last() {
        let (head, tail) = (s[..last.start()].trim(), s[last.end()..].trim());
        if !head.is_empty() && !tail.is_empty() && head.len() >= tail.len() {
            return head.to_string();
        }
    }
    s.to_string()
}

fn pick_description(
    og: &std::collections::HashMap<String, String>,
    article: Option<&Value>,
    webpage: Option<&Value>,
) -> Option<String> {
    if let Some(a) = article {
        if let Some(d) = entry_string(a, "description") {
            return Some(d);
        }
    }
    if let Some(v) = og.get("og:description") {
        return Some(v.clone());
    }
    if let Some(w) = webpage {
        if let Some(d) = entry_string(w, "description") {
            return Some(d);
        }
    }
    og.get("description").cloned()
}

fn pick_author(
    og: &std::collections::HashMap<String, String>,
    article: Option<&Value>,
) -> Option<String> {
    if let Some(a) = article {
        let v = a.get("author");
        if let Some(v) = v {
            let names: Vec<String> = match v {
                Value::String(s) => vec![s.trim().to_string()],
                Value::Object(_) => name_from_value(v).into_iter().collect(),
                Value::Array(arr) => arr.iter().flat_map(name_from_value).collect(),
                _ => Vec::new(),
            };
            let names: Vec<String> = names.into_iter().filter(|s| !s.is_empty()).collect();
            if let Some(joined) = join_names(&names) {
                return Some(joined);
            }
        }
    }
    og.get("article:author")
        .or_else(|| og.get("author"))
        .cloned()
}

fn name_from_value(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Object(map) => map
            .get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.trim().to_string()),
        _ => None,
    }
}

fn join_names(names: &[String]) -> Option<String> {
    match names.len() {
        0 => None,
        1 => Some(names[0].clone()),
        2 => Some(format!("{} and {}", names[0], names[1])),
        _ => {
            let (last, rest) = names.split_last().unwrap();
            Some(format!("{}, and {}", rest.join(", "), last))
        }
    }
}

fn pick_date(
    og: &std::collections::HashMap<String, String>,
    article: Option<&Value>,
    webpage: Option<&Value>,
) -> Option<String> {
    let raw = article
        .and_then(|a| {
            entry_string(a, "datePublished").or_else(|| entry_string(a, "dateCreated"))
        })
        .or_else(|| og.get("article:published_time").cloned())
        .or_else(|| og.get("og:published_time").cloned())
        .or_else(|| webpage.and_then(|w| entry_string(w, "datePublished")))?;
    normalize_date(&raw)
}

/// Normalize ISO-8601-ish strings down to `YYYY-MM-DD`.
fn normalize_date(s: &str) -> Option<String> {
    if is_bare_iso_date(s) {
        return Some(s.to_string());
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|d| d.format("%Y-%m-%d").to_string())
        })
        .or_else(|| {
            // Human-readable month-name forms (Strikingly JSON-LD:
            // "Dec 24, 2022 at 07:54"). Try datetime first, then bare date.
            const MONTH_NAME_FORMATS: &[&str] = &[
                "%b %d, %Y at %H:%M",
                "%B %d, %Y at %H:%M",
                "%b %d, %Y",
                "%B %d, %Y",
            ];
            let try_formats = |candidate: &str| {
                MONTH_NAME_FORMATS.iter().find_map(|fmt| {
                    chrono::NaiveDateTime::parse_from_str(candidate, fmt)
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .ok()
                        .or_else(|| {
                            chrono::NaiveDate::parse_from_str(candidate, fmt)
                                .map(|d| d.format("%Y-%m-%d").to_string())
                                .ok()
                        })
                })
            };
            // chrono's `%d` parser requires a zero-padded day; Strikingly
            // emits unpadded ones ("Mar 6, 2023 at 07:13"). Retry with the
            // day zero-padded before giving up.
            try_formats(s).or_else(|| try_formats(&zero_pad_day(s)))
        })
        .or_else(|| {
            // `get(..10)` is `None` unless the cut lands on a char boundary,
            // so a leading multi-byte char can't be split here.
            s.get(..10).filter(|head| {
                let bytes = head.as_bytes();
                bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') && head.is_ascii()
            }).map(str::to_string)
        })
}

/// Zero-pad a single-digit day preceding the first comma: "Mar 6, 2023" →
/// "Mar 06, 2023". No-op when the token before the comma isn't a lone digit.
fn zero_pad_day(s: &str) -> String {
    let Some(comma_idx) = s.find(',') else {
        return s.to_string();
    };
    let (head, tail) = s.split_at(comma_idx);
    let Some((before_day, day_part)) = head.rsplit_once(' ') else {
        return s.to_string();
    };
    if day_part.len() == 1 && day_part.chars().all(|c| c.is_ascii_digit()) {
        format!("{before_day} 0{day_part}{tail}")
    } else {
        s.to_string()
    }
}

fn is_bare_iso_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 10
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.iter().all(|b| b.is_ascii())
}

/// Resolve `publisher` from an Article entry — usually a `{@id: ...}` ref to
/// an `Organization` elsewhere in the graph.
fn pick_publisher(
    og: &std::collections::HashMap<String, String>,
    article: Option<&Value>,
    webpage: Option<&Value>,
    entries: &[Value],
) -> Option<String> {
    let publisher_field = article
        .or(webpage)
        .and_then(|e| e.get("publisher"));
    if let Some(pf) = publisher_field {
        if let Some(name) = name_from_value(pf) {
            return Some(name);
        }
        if let Some(target_id) = pf.get("@id").and_then(|v| v.as_str()) {
            let matched = entries
                .iter()
                .find(|e| e.get("@id").and_then(|v| v.as_str()) == Some(target_id));
            if let Some(m) = matched {
                if let Some(n) = entry_string(m, "name") {
                    return Some(n);
                }
            }
        }
    }
    og.get("og:site_name").cloned()
}

fn pick_lang(doc: &Html, article: Option<&Value>, webpage: Option<&Value>) -> Option<String> {
    article
        .and_then(|a| entry_string(a, "inLanguage"))
        .or_else(|| webpage.and_then(|w| entry_string(w, "inLanguage")))
        .or_else(|| {
            let sel = Selector::parse("html").ok()?;
            doc.select(&sel)
                .next()
                .and_then(|e| e.value().attr("lang"))
                .map(|s| s.to_string())
        })
        .map(|s| normalize_lang(&s))
}

/// "en-US" → "en"; "en" → "en" — region subtags are noise for most languages.
/// Chinese is the exception: the subtag encodes Traditional vs Simplified
/// (zh-TW/zh-Hant vs zh-CN/zh-Hans), so zh-* tags pass through lowercased.
fn normalize_lang(s: &str) -> String {
    let lower = s.to_lowercase().replace('_', "-");
    let base = lower.split('-').next().unwrap_or("").to_string();
    if base == "zh" && lower != "zh" {
        lower
    } else {
        base
    }
}

// ---- Fallback cover -------------------------------------------------------
//
// `og:image` is the cover when the page declares one. Without it the cover
// falls back to the first image of the extracted body, which on a site whose
// header logo survives extraction made that logo the cover of every page.
// `chrome_images` records, from the full document, the images that are not
// content, and `fallback_cover` skips them.

/// Declared `width`/`height` below this makes an image an icon, not a cover.
const MIN_COVER_PX: u32 = 64;

const CHROME_TAGS: &[&str] = &["header", "nav", "footer", "aside"];
const CHROME_ROLES: &[&str] = &["banner", "navigation", "contentinfo", "complementary"];
// Class/id words a builder uses for the same regions when it has no semantic
// element (`<div class="site-header">`), which the extractor cannot tell from
// content either.
const CHROME_WORDS: &[&str] = &["header", "masthead", "nav", "navbar", "footer", "sidebar", "logo"];

/// `src` of every `<img>` in `doc` that must never be a cover.
pub(crate) fn chrome_images(doc: &Html) -> Vec<String> {
    let Ok(sel) = Selector::parse("img[src]") else {
        return Vec::new();
    };
    doc.select(&sel)
        .filter(|img| in_chrome(img) || declared_small(img))
        .filter_map(|img| img.value().attr("src").map(|s| s.trim().to_string()))
        .collect()
}

fn in_chrome(img: &ElementRef) -> bool {
    std::iter::once(*img)
        .chain(img.ancestors().filter_map(ElementRef::wrap))
        .any(is_chrome_element)
}

fn is_chrome_element(el: ElementRef) -> bool {
    let v = el.value();
    CHROME_TAGS.contains(&v.name())
        || v.attr("role").is_some_and(|r| CHROME_ROLES.contains(&r.trim()))
        || ["class", "id"].iter().filter_map(|a| v.attr(a)).any(|names| {
            names
                .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
                .any(|w| CHROME_WORDS.contains(&w.to_lowercase().as_str()))
        })
}

fn declared_small(img: &ElementRef) -> bool {
    ["width", "height"].iter().any(|attr| {
        img.value()
            .attr(attr)
            .and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
            .is_some_and(|px| px < MIN_COVER_PX)
    })
}

/// The first non-`data:` image of `markdown` that is not in `excluded` (raw
/// `src` values from [`chrome_images`]), resolved against `base_url`. The
/// converter has already normalized the markdown's own URLs to that form, so
/// the exclusions are resolved the same way before comparing.
pub(crate) fn fallback_cover(markdown: &str, excluded: &[String], base_url: &str) -> Option<String> {
    let base = Url::parse(base_url).ok();
    let skip: HashSet<String> = excluded
        .iter()
        .filter_map(|raw| resolve_url(raw, &base))
        .flat_map(|abs| {
            let original = crate::vault::import::media::original_media_url(&abs);
            [Some(abs), original]
        })
        .flatten()
        .collect();
    IMG_PATTERN.captures_iter(markdown).find_map(|c| {
        let abs = resolve_url(&c[2], &base)?;
        (!c[2].starts_with("data:") && !skip.contains(&abs)).then_some(abs)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(json: &str) -> Value {
        serde_json::from_str(json).expect("valid json")
    }

    #[test]
    fn strip_site_suffix_pipe() {
        assert_eq!(strip_site_suffix("Article Title | Site Name"), "Article Title");
    }

    #[test]
    fn strip_site_suffix_em_dash() {
        assert_eq!(
            strip_site_suffix("A Reporter's Voice — Abroad - Example Times"),
            "A Reporter's Voice — Abroad"
        );
    }

    #[test]
    fn strip_site_suffix_keeps_an_em_dash_inside_a_long_title() {
        // The suffix is the last segment. Splitting at the em dash first cut
        // this title to its first half whenever that half outweighed the rest.
        assert_eq!(
            strip_site_suffix("The Reporter's Notebook — Abroad - Example Times"),
            "The Reporter's Notebook — Abroad"
        );
    }

    #[test]
    fn strip_site_suffix_leaves_short_titles_alone() {
        assert_eq!(strip_site_suffix("AI | MIT"), "AI | MIT");
    }

    #[test]
    fn join_names_handles_arity() {
        assert_eq!(join_names(&[]), None);
        assert_eq!(join_names(&["A".into()]), Some("A".into()));
        assert_eq!(join_names(&["A".into(), "B".into()]), Some("A and B".into()));
        assert_eq!(
            join_names(&["A".into(), "B".into(), "C".into()]),
            Some("A, B, and C".into())
        );
    }

    #[test]
    fn normalize_date_iso() {
        assert_eq!(
            normalize_date("2024-12-23T00:00:00+00:00").as_deref(),
            Some("2024-12-23")
        );
        assert_eq!(normalize_date("2026-05-19T11:55:00+00:00").as_deref(), Some("2026-05-19"));
        assert_eq!(normalize_date("2024-12-23").as_deref(), Some("2024-12-23"));
        assert_eq!(normalize_date("2024-12-23T12:00:00").as_deref(), Some("2024-12-23"));
    }

    #[test]
    fn normalize_date_month_name_formats() {
        // Strikingly JSON-LD emits "Mon D, YYYY at HH:MM"
        assert_eq!(
            normalize_date("Dec 24, 2022 at 07:54").as_deref(),
            Some("2022-12-24")
        );
        assert_eq!(normalize_date("Mar 6, 2023 at 07:13").as_deref(), Some("2023-03-06"));
        assert_eq!(normalize_date("December 24, 2022").as_deref(), Some("2022-12-24"));
        assert_eq!(normalize_date("not a date").as_deref(), None);
    }

    #[test]
    fn normalize_lang_strips_region() {
        assert_eq!(normalize_lang("en-US"), "en");
        // zh-Hans is the Simplified-Chinese subtag; it must survive as
        // "zh-hans" (see normalize_lang_preserves_chinese_subtags below) —
        // collapsing it to "zh" loses the Traditional/Simplified distinction.
        assert_eq!(normalize_lang("zh-Hans"), "zh-hans");
        assert_eq!(normalize_lang("en"), "en");
    }

    #[test]
    fn normalize_lang_preserves_chinese_subtags() {
        // zh subtags distinguish Traditional vs Simplified — must survive.
        // moss accepts zh-tw / zh-hant / zh-cn / zh-hans (i18n.rs from_code).
        assert_eq!(normalize_lang("zh-TW"), "zh-tw");
        assert_eq!(normalize_lang("zh-Hant"), "zh-hant");
        assert_eq!(normalize_lang("zh-Hans"), "zh-hans");
        assert_eq!(normalize_lang("zh"), "zh");
        // Non-Chinese tags keep the existing base-only behavior.
        assert_eq!(normalize_lang("en-US"), "en");
    }

    #[test]
    fn author_from_array_of_persons() {
        let article = entry(
            r#"{
              "@type": "Article",
              "author": [
                {"@type": "Person", "name": "Jane Doe"},
                {"@type": "Person", "name": "John Roe"}
              ]
            }"#,
        );
        let empty: std::collections::HashMap<String, String> = Default::default();
        assert_eq!(
            pick_author(&empty, Some(&article)).as_deref(),
            Some("Jane Doe and John Roe")
        );
    }

    #[test]
    fn author_from_single_object() {
        let article = entry(r#"{"@type": "Article", "author": {"name": "Jane Doe"}}"#);
        let empty: std::collections::HashMap<String, String> = Default::default();
        assert_eq!(pick_author(&empty, Some(&article)).as_deref(), Some("Jane Doe"));
    }

    #[test]
    fn author_from_bare_string() {
        let article = entry(r#"{"@type": "Article", "author": "Jane Doe"}"#);
        let empty: std::collections::HashMap<String, String> = Default::default();
        assert_eq!(pick_author(&empty, Some(&article)).as_deref(), Some("Jane Doe"));
    }

    #[test]
    fn publisher_resolved_via_at_id() {
        let entries = vec![
            entry(r#"{"@type":"Article","publisher":{"@id":"https://x.example/#org"}}"#),
            entry(r#"{"@type":"Organization","@id":"https://x.example/#org","name":"The X Times"}"#),
        ];
        let empty: std::collections::HashMap<String, String> = Default::default();
        assert_eq!(
            pick_publisher(&empty, Some(&entries[0]), None, &entries).as_deref(),
            Some("The X Times")
        );
    }

    #[test]
    fn parses_json_ld_graph_wrapped() {
        let html = r##"<html><head>
          <script type="application/ld+json">
          {"@context":"https://schema.org","@graph":[
            {"@type":"Article","headline":"Hello","author":{"name":"Jane Doe"}},
            {"@type":"Organization","@id":"#org","name":"The Outlet"}
          ]}
          </script>
        </head><body></body></html>"##;
        let doc = Html::parse_document(html);
        let entries = parse_schema_org(&doc);
        assert!(entries.iter().any(|e| has_type(e, ARTICLE_TYPES)));
        assert!(entries.iter().any(|e| has_type(e, &["Organization"])));
    }

    #[test]
    fn falls_back_to_html_lang_attribute() {
        let html = r#"<html lang="zh-Hans"><head></head><body></body></html>"#;
        let derived = derive(html);
        // zh-Hans (Simplified) must survive distinct from zh-Hant
        // (Traditional) — see normalize_lang_preserves_chinese_subtags.
        assert_eq!(derived.lang.as_deref(), Some("zh-hans"));
    }

    #[test]
    fn end_to_end_extraction_from_minimal_html() {
        let html = r##"<!doctype html><html lang="en"><head>
          <title>Real Title - The Site</title>
          <meta property="og:title" content="Real Title">
          <meta property="og:description" content="A short dek.">
          <script type="application/ld+json">
          {"@type":"NewsArticle",
           "headline":"Real Title",
           "datePublished":"2024-12-22T00:00:00Z",
           "author":{"@type":"Person","name":"Jane Doe"},
           "publisher":{"@id":"#org"}}
          </script>
          <script type="application/ld+json">
          {"@type":"Organization","@id":"#org","name":"The Site"}
          </script>
        </head><body><h1>Real Title</h1><p>Body.</p></body></html>"##;
        let m = derive(html);
        assert_eq!(m.title.as_deref(), Some("Real Title"));
        assert_eq!(m.author.as_deref(), Some("Jane Doe"));
        assert_eq!(m.date.as_deref(), Some("2024-12-22"));
        assert_eq!(m.publisher.as_deref(), Some("The Site"));
        assert_eq!(m.lang.as_deref(), Some("en"));
        assert_eq!(m.description.as_deref(), Some("A short dek."));
    }

    #[test]
    fn publisher_equal_to_title_is_dropped() {
        let html = r#"<html><head>
            <meta property="og:title" content="My Post">
            <meta property="og:site_name" content="My Post">
            </head><body></body></html>"#;
        let m = derive(html);
        assert_eq!(m.title.as_deref(), Some("My Post"));
        assert_eq!(m.publisher, None, "title-as-publisher is noise");
    }


    fn cover_of(body: &str) -> Option<String> {
        let html = format!("<html><head><title>T</title></head><body>{body}</body></html>");
        let article = super::super::converter::extract_article(&html, "https://example.com/page");
        fallback_cover(&article.markdown, &article.metadata.chrome_images, "https://example.com/page")
    }

    const TEXT: &str = "<p>A paragraph of real article text, long enough that the extractor \
                        keeps this block as the page's main content.</p>";

    #[test]
    fn a_header_logo_is_skipped_for_the_content_image() {
        let cover = cover_of(&format!(
            "<div class=\"site-header\"><img src=\"/logo.png\"></div>\
             {TEXT}<img src=\"/photo.jpg\">{TEXT}"
        ));
        assert_eq!(cover.as_deref(), Some("https://example.com/photo.jpg"));
    }

    #[test]
    fn a_cover_url_with_parentheses_survives_whole() {
        let md = "![](https://cdn.example.com/photo(2024).jpg)\n";
        assert_eq!(
            fallback_cover(md, &[], "https://example.com/page").as_deref(),
            Some("https://cdn.example.com/photo(2024).jpg")
        );
    }

    #[test]
    fn a_page_with_only_a_header_logo_has_no_cover() {
        let cover = cover_of(&format!(
            "<div class=\"site-header\"><img src=\"/logo.png\"></div>{TEXT}"
        ));
        assert_eq!(cover, None);
    }

    #[test]
    fn header_nav_footer_aside_banner_and_tiny_images_are_chrome() {
        let doc = Html::parse_document(
            "<body><header><img src=\"/h\"></header><nav><a><img src=\"/n\"></a></nav>\
             <footer><img src=\"/f\"></footer><aside><img src=\"/a\"></aside>\
             <div role=\"banner\"><img src=\"/b\"></div>\
             <main><img src=\"/icon\" width=\"32\"><img src=\"/flat\" height=\"20px\">\
             <img src=\"/big\" width=\"800\" height=\"600\"><img src=\"/plain\"></main></body>",
        );
        let mut chrome = chrome_images(&doc);
        chrome.sort();
        assert_eq!(chrome, ["/a", "/b", "/f", "/flat", "/h", "/icon", "/n"]);
    }

    fn title_of(head: &str) -> Option<String> {
        derive(&format!("<html><head>{head}</head><body></body></html>")).title
    }

    #[test]
    fn og_title_beats_a_builders_internal_page_name_in_title() {
        let t = title_of(
            r#"<title>Reviews 1 | Studio Name</title><meta property="og:title" content="Press">"#,
        );
        assert_eq!(t.as_deref(), Some("Press"));
    }

    #[test]
    fn a_trailing_site_name_is_stripped_when_the_page_names_its_site() {
        let t = title_of(
            r#"<title>About | Studio Name</title><meta property="og:site_name" content="Studio Name">"#,
        );
        assert_eq!(t.as_deref(), Some("About"));
    }

    #[test]
    fn a_site_name_in_og_title_is_stripped_too() {
        let t = title_of(
            r#"<meta property="og:title" content="Press — Studio Name"><meta property="og:site_name" content="Studio Name">"#,
        );
        assert_eq!(t.as_deref(), Some("Press"));
    }

    #[test]
    fn a_leading_site_name_goes_only_when_the_site_name_is_known() {
        let known = title_of(
            r#"<title>Studio Name | Home</title><meta property="og:site_name" content="Studio Name">"#,
        );
        assert_eq!(known.as_deref(), Some("Home"));
        let unknown = title_of("<title>Studio Name | Home</title>");
        assert_ne!(
            unknown.as_deref(),
            Some("Home"),
            "a leading segment is not a site name by position alone"
        );
    }

    #[test]
    fn a_title_that_is_only_the_site_name_stays() {
        let t = title_of(
            r#"<title>Studio Name</title><meta property="og:site_name" content="Studio Name">"#,
        );
        assert_eq!(t.as_deref(), Some("Studio Name"));
        assert_eq!(strip_site_name("Studio Name", &["Studio Name"]), "Studio Name");
    }

    #[test]
    fn every_separator_is_recognised_but_a_dash_inside_a_word_is_not() {
        for sep in [" | ", " — ", " – ", " - ", " · ", " :: "] {
            assert_eq!(strip_site_name(&format!("About{sep}Studio Name"), &["Studio Name"]), "About");
        }
        assert_eq!(strip_site_name("Spider-Man", &["Man"]), "Spider-Man");
    }
}
