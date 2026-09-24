//! Article metadata extracted from raw HTML.
//!
//! Walks `<script type="application/ld+json">` blocks (schema.org), then falls
//! through to OpenGraph `<meta property="og:…">`, `<meta name="…">`, and the
//! bare `<html lang>` attribute. Produces a struct aligned with moss's
//! frontmatter schema (`BUILTIN_FIELDS`).
//!
//! Resolution priority for each field is documented inline. Generally
//! JSON-LD beats OpenGraph beats `<meta name>` beats raw `<title>`.

use scraper::{Html, Selector};
use serde_json::Value;

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

    let title = pick_title(&doc, &og, article, webpage);
    let mut publisher = pick_publisher(&og, article, webpage, &entries);
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
) -> Option<String> {
    if let Some(a) = article {
        if let Some(h) = entry_string(a, "headline") {
            return Some(h);
        }
        if let Some(n) = entry_string(a, "name") {
            return Some(n);
        }
    }
    if let Some(v) = og.get("og:title") {
        return Some(v.clone());
    }
    if let Some(w) = webpage {
        if let Some(n) = entry_string(w, "name") {
            return Some(strip_site_suffix(&n));
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
        Some(strip_site_suffix(trimmed))
    }
}

/// "Article Title | Site Name" → "Article Title". Splits on the longest
/// delimiter (` — `, ` | `, ` - `) and drops the shorter trailing segment when
/// it looks like a site brand (i.e. when the article half is at least as long).
fn strip_site_suffix(s: &str) -> String {
    let candidates = [" — ", " – ", " | ", " - "];
    for delim in candidates {
        if let Some((head, tail)) = s.rsplit_once(delim) {
            let (head, tail) = (head.trim(), tail.trim());
            if !head.is_empty() && !tail.is_empty() && head.len() >= tail.len() {
                return head.to_string();
            }
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
        if let Some(name) = direct_name(pf) {
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

fn direct_name(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Object(map) => map
            .get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.trim().to_string()),
        _ => None,
    }
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
}
