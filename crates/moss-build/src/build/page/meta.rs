//! Semantic markup generation for articles.
//!
//! Generates Open Graph and Schema.org JSON-LD metadata for article pages
//! to help crawlers and social platforms identify content correctly.

use crate::build::served_path::ServedPath;
use crate::build::site_url::SiteUrl;
use crate::i18n::link::TranslationLink;
pub use moss_core::media::html_escape_attr as escape_html_attr;

/// A cover image reference. Two variants because the cover field is
/// uniquely allowed to be an external HTTPS URL (e.g., a stock-photo CDN
/// in frontmatter `cover:`), in addition to the typed local case.
///
/// Every other URL emitted by moss goes through `ServedPath` directly;
/// this enum is the one place where an "either" sum type is justified.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverRef {
    /// A typed served path: the cover lives inside the build output.
    Local(ServedPath),
    /// Already-absolute http(s):// URL: the cover lives on a CDN or
    /// other external host. Pass-through as-is.
    External(String),
}

/// The share image with what is known about it: its dimensions, and the
/// author's alt text when it is a body picture that has one.
#[derive(Debug, Clone)]
pub struct OgCover {
    pub cover: CoverRef,
    pub dims: Option<(u32, u32)>,
    pub alt: Option<String>,
}

impl CoverRef {
    /// Resolve to the absolute URL form for emission in og:image,
    /// twitter:image, JSON-LD, RSS, etc.
    pub fn to_absolute_url(&self, site_url: &SiteUrl) -> String {
        match self {
            Self::Local(sp) => sp.to_absolute_url(site_url),
            Self::External(url) => url.clone(),
        }
    }

    /// Resolve to a URL for embedding in long-lived HTML (og:image,
    /// twitter:image, JSON-LD `image`, RSS, sitemap). Deployed sites
    /// (https `SiteUrl`) get absolute URLs — crawlers have no document
    /// base to resolve relative paths against. Preview / unconfigured
    /// builds (http localhost fallback) get the served-path's relative
    /// form so the dev port doesn't bake into static HTML. `External`
    /// covers pass through (already absolute).
    pub fn to_meta_url(&self, site_url: &SiteUrl) -> String {
        match self {
            Self::Local(sp) => {
                if site_url.is_deployed() {
                    sp.to_absolute_url(site_url)
                } else {
                    sp.to_relative_url()
                }
            }
            Self::External(url) => url.clone(),
        }
    }
}

/// Generate Open Graph meta tags for an article.
///
/// # Arguments
/// * `title` - Article title
/// * `description` - Article description (auto-generated if not provided)
/// * `url` - Full canonical URL of the article
/// * `site_name` - Name of the website
/// * `date` - Publication date in ISO 8601 format
/// * `cover` - Optional typed CoverRef (Local served path or External URL)
/// * `cover_dims` - Optional `(width, height)` for the cover image. Should only be
///   `Some` when the cover came from the auto-generated card (where dimensions are
///   known to be 1200x630); `None` for user-supplied covers of unknown size.
/// * `tags` - Article tags
/// * `locale` - BCP 47-style locale, e.g. "en_US". Empty string skips `og:locale` emission.
/// * `site_url` - Base URL used to resolve cover paths via
///   [`CoverRef::to_meta_url`]: absolute on deployed sites (https; per the
///   Open Graph spec crawlers have no document base), root-relative on
///   preview / unconfigured builds (so the dev-server URL doesn't bake
///   into static HTML).
///
/// # Returns
/// HTML string containing Open Graph meta tags
pub fn build_og_tags(
    title: &str,
    description: &str,
    url: &str,
    site_name: &str,
    date: Option<&str>,
    cover: Option<&OgCover>,
    tags: &[String],
    locale: &str,
    site_url: &SiteUrl,
) -> String {
    og_tags(
        "article",
        title,
        url,
        site_name,
        description,
        cover,
        locale,
        site_url,
        Some((date, tags)),
    )
}

/// Generate Open Graph meta tags for a website homepage.
///
/// Uses `og:type = "website"` instead of `"article"`, and omits
/// article-specific fields like `published_time` and `article:tag`.
///
/// `locale` empty string skips `og:locale` emission.
/// `site_url` resolves cover paths via [`CoverRef::to_meta_url`]: absolute
/// on deployed sites (https; OG spec requires it for crawlers), relative
/// on preview / unconfigured builds.
pub fn build_og_tags_website(
    title: &str,
    site_name: &str,
    description: &str,
    url: &str,
    cover: Option<&OgCover>,
    locale: &str,
    site_url: &SiteUrl,
) -> String {
    og_tags(
        "website",
        title,
        url,
        site_name,
        description,
        cover,
        locale,
        site_url,
        None,
    )
}

/// Shared Open Graph builder. Empty title, description and locale are skipped
/// rather than emitted as `content=""`: an absent tag lets scrapers fall back
/// to other sources, an empty one says "deliberately none". `title` is the
/// page label; `site_name` is the real site name, kept separate so per-page
/// Pages emit og:site_name = site, not label. Articles always emit
/// og:site_name; websites only when it is non-empty. `article` carries the
/// article-only `(published_time, tags)`.
fn og_tags(
    og_type: &str,
    title: &str,
    url: &str,
    site_name: &str,
    description: &str,
    cover: Option<&OgCover>,
    locale: &str,
    site_url: &SiteUrl,
    article: Option<(Option<&str>, &[String])>,
) -> String {
    let cover_url = cover.map(|c| c.cover.to_meta_url(site_url));
    let meta = |property: &str, content: &str| {
        format!(
            r#"<meta property="{property}" content="{}">"#,
            escape_html_attr(content)
        )
    };

    let mut tags_html = vec![format!(r#"<meta property="og:type" content="{og_type}">"#)];
    if !title.is_empty() {
        tags_html.push(meta("og:title", title));
    }
    tags_html.push(meta("og:url", url));
    if article.is_some() || !site_name.is_empty() {
        tags_html.push(meta("og:site_name", site_name));
    }
    if !description.is_empty() {
        tags_html.push(meta("og:description", description));
    }

    if let Some((Some(date), _)) = article {
        tags_html.push(meta("article:published_time", date));
    }

    if let Some(image) = cover_url.as_deref() {
        tags_html.push(meta("og:image", image));
        let alt = cover.and_then(|c| c.alt.as_deref()).unwrap_or(title);
        tags_html.push(meta("og:image:alt", alt));
        if let Some((w, h)) = cover.and_then(|c| c.dims) {
            tags_html.push(format!(r#"<meta property="og:image:width" content="{w}">"#));
            tags_html.push(format!(r#"<meta property="og:image:height" content="{h}">"#));
        }
    }

    if !locale.is_empty() {
        tags_html.push(meta("og:locale", locale));
    }

    if let Some((_, tags)) = article {
        for tag in tags {
            tags_html.push(meta("article:tag", tag));
        }
    }

    tags_html.join("\n    ")
}

/// Generate Twitter Card meta tags. Uses `summary_large_image` when an image
/// is available, falls back to `summary` otherwise.
///
/// # Arguments
///
/// * `title` - Card title
/// * `description` - Card description
/// * `cover` - Optional typed CoverRef (Local served path or External URL). When None, emits `summary` card type.
/// * `cover_alt` - Optional alt text for the image. Falls back to `title` when None.
/// * `site_url` - Base URL used to resolve cover paths via
///   [`CoverRef::to_meta_url`]: absolute on deployed sites (Twitter Card spec
///   requires it to ingest the image), relative on preview / unconfigured
///   builds (accepts that no card image renders in preview rather than
///   baking the dev-server URL into static HTML).
pub fn build_twitter_tags(
    title: &str,
    description: &str,
    cover: Option<&CoverRef>,
    cover_alt: Option<&str>,
    site_url: &SiteUrl,
) -> String {
    // Resolve the cover URL: absolute when deployed, relative in preview
    // (see CoverRef::to_meta_url). Twitter requires absolute URLs to ingest
    // images for the card; in preview we accept that no card image renders
    // rather than baking the dev-server URL into static HTML.
    let cover_url: Option<String> = cover.map(|c| c.to_meta_url(site_url));

    let card_type = if cover_url.is_some() { "summary_large_image" } else { "summary" };
    // Skip empty title/description rather than emitting empty `content=""`.
    let mut tags_html = vec![
        format!(r#"<meta name="twitter:card" content="{}">"#, card_type),
    ];
    if !title.is_empty() {
        tags_html.push(format!(
            r#"<meta name="twitter:title" content="{}">"#,
            escape_html_attr(title)
        ));
    }
    if !description.is_empty() {
        tags_html.push(format!(
            r#"<meta name="twitter:description" content="{}">"#,
            escape_html_attr(description)
        ));
    }
    if let Some(image) = cover_url.as_deref() {
        tags_html.push(format!(
            r#"<meta name="twitter:image" content="{}">"#,
            escape_html_attr(image)
        ));
        let alt = cover_alt.unwrap_or(title);
        tags_html.push(format!(
            r#"<meta name="twitter:image:alt" content="{}">"#,
            escape_html_attr(alt)
        ));
    }
    tags_html.join("\n    ")
}

/// Wrap a JSON-LD value in its `<script>` tag. The one place JSON-LD text
/// reaches HTML: `<` is written as `\u003c` so page text such as `</script>`
/// or `<!--` cannot close the block or open a comment.
pub(crate) fn ld_script(value: &serde_json::Value) -> String {
    let body = serde_json::to_string_pretty(value).unwrap_or_default().replace('<', "\\u003c");
    format!("<script type=\"application/ld+json\">\n{body}\n</script>")
}

/// The page's own URL as meta tags and JSON-LD state it: absolute when the
/// site is deployed, else the relative served path, with `index.html` cut
/// so directory pages keep their trailing slash.
pub(crate) fn page_meta_url(
    doc: &crate::build::types::ParsedDocument,
    site_url: &SiteUrl,
) -> Result<String, String> {
    let page_path = crate::build::served_path::ServedPath::from_source(&doc.url_path)
        .map_err(|e| format!("meta URL path for {}: {}", doc.url_path, e))?;
    let relative = page_path.to_relative_url().trim_end_matches("index.html").to_string();
    Ok(if site_url.is_deployed() { site_url.to_absolute(&relative) } else { relative })
}

/// The JSON-LD block for a page: `Event` for an event page of any layout,
/// else `Article` for an article page, else `WebSite` for the homepage, else
/// none. Only public pages get one, like the og/twitter tags.
pub(crate) fn page_json_ld(
    doc: Option<&crate::build::types::ParsedDocument>,
    is_article_page: bool,
    is_homepage: bool,
    description: &str,
    site_name: &str,
    cover: Option<&CoverRef>,
    site_url: &SiteUrl,
    homepage_url: &str,
    lang_tag: &str,
) -> Result<Option<String>, String> {
    let Some(d) = doc.filter(|d| d.is_public_page()) else { return Ok(None) };
    if let Some(event) = super::event_json_ld::for_page(d, description, site_name, cover, site_url)? {
        return Ok(Some(event));
    }
    if is_article_page {
        // Schema.org headline is chrome; use the plain-text label.
        return Ok(Some(build_schema_json_ld(
            &d.label, description, &page_meta_url(d, site_url)?, site_name,
            d.date.as_deref(), cover, d.tags.as_deref().unwrap_or(&[]), site_url, &d.citations,
        )));
    }
    Ok(is_homepage.then(|| build_schema_website(site_name, description, homepage_url, lang_tag)))
}

/// Generate Schema.org JSON-LD for an article.
///
/// `site_url` resolves the cover through [`CoverRef::to_meta_url`]: absolute
/// on deployed sites, relative on preview builds (same policy as `og:image`).
pub fn build_schema_json_ld(
    title: &str,
    description: &str,
    url: &str,
    site_name: &str,
    date: Option<&str>,
    cover: Option<&CoverRef>,
    tags: &[String],
    site_url: &SiteUrl,
    citations: &[moss_core::ast::quote_source::QuoteSource],
) -> String {
    let mut o = serde_json::json!({
        "@context": "https://schema.org",
        "@type": "Article",
        "headline": title,
        "url": url,
        "description": description,
        // Publisher is required by Google.
        "publisher": {"@type": "Organization", "name": site_name},
    });
    if let Some(date) = date {
        o["datePublished"] = date.into();
    }
    if let Some(cover) = cover {
        o["image"] = cover.to_meta_url(site_url).into();
    }
    if !tags.is_empty() {
        o["keywords"] = tags.into();
    }
    if !citations.is_empty() {
        o["citation"] = citations
            .iter()
            .map(|c| serde_json::json!({"@type": "CreativeWork", "name": c.text, "url": c.href}))
            .collect();
    }
    ld_script(&o)
}

/// Generate Schema.org `WebSite` JSON-LD for the homepage: the site's entity
/// identity for crawlers. `description` and `lang_code` are omitted when empty.
pub fn build_schema_website(
    site_name: &str,
    description: &str,
    url: &str,
    lang_code: &str,
) -> String {
    let mut o = serde_json::json!({
        "@context": "https://schema.org",
        "@type": "WebSite",
        "name": site_name,
        "url": url,
        "publisher": {"@type": "Organization", "name": site_name},
    });
    if !description.is_empty() {
        o["description"] = description.into();
    }
    if !lang_code.is_empty() {
        o["inLanguage"] = lang_code.into();
    }
    ld_script(&o)
}

pub use moss_core::page::excerpt::{
    extract_description, extract_description_markdown, extract_preview, render_description_html, render_description_html_in_link,
    resolve_page_description, resolve_page_description_with_fallbacks, strip_markdown_inline, DescriptionChainInputs,
};
pub(crate) use moss_core::page::excerpt::description_from_content;

/// Build the `<head>` block of `<link rel="alternate" hreflang="...">` tags
/// for a multilingual page.
///
/// Returns `None` when the page has no translations (`translations.is_empty()`)
/// — single-language pages emit no hreflang. Returns `Some(html)` otherwise,
/// containing one `<link>` per language (the current page's own lang plus each
/// alternate) and an `x-default` entry pointing to the site-default-language
/// version when one exists in this translation group.
///
/// `site_host` is a bare host (`"example.com"`) — leading scheme and trailing
/// slash are tolerated and stripped. URL form is `https://{host}/{path}` with
/// any trailing `index.html` removed (matches the canonical_link policy in
/// `render/html.rs`). www-prefix canonicalization is intentionally NOT applied
/// here: hreflang URLs should match the actual served URL of each variant, and
/// the per-variant www decision is already captured in the document's
/// `url_path` if relevant.
///
/// TODO: if `www_canonical` ever gets plumbed into `canonical_url()` (see the
/// comment at the `canonical_link` site in `render/html.rs`), this helper must
/// also accept it and run the same `canonical_url()` transform on each href.
/// Otherwise canonical will emit `https://www.example.com/about/` while
/// hreflang emits `https://example.com/about/` and Google's hreflang validator
/// will flag the canonical/hreflang URL mismatch.
///
/// `current_lang` and `current_url_path` describe the page being rendered;
/// `translations` holds links to OTHER translations (not the current page —
/// see `build_translation_links` line 42). `site_default_lang` controls the
/// `x-default` target: emit `x-default` when the current page IS the site
/// default lang, OR when one of the alternates is. Skip otherwise (no safe
/// fallback target).
///
/// Every value here is a DECLARED BCP-47 tag — the same string the page emits
/// as its own `<html lang>`. Taking them from `Language` instead made a `fr`
/// page advertise `hreflang="en"` against its own `<html lang>: fr`, and
/// collapsed two pages of one unshipped language into a single alternate.
pub fn build_hreflang_link_tags(
    current_lang_tag: &str,
    current_url_path: &str,
    translations: &[TranslationLink],
    site_host: &str,
    site_default_lang_tag: &str,
) -> Option<String> {
    if translations.is_empty() {
        return None;
    }

    let host = site_host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    // Build a properly-escaped href from a relative url_path. Defends against:
    // - leading slash on url_path (would produce https://host//path)
    // - HTML-unsafe characters in the path (matches canonical_link policy)
    // - trailing index.html (matches canonical_link policy in render/html.rs)
    let to_href = |url_path: &str| -> String {
        let trimmed = url_path
            .trim_start_matches('/')
            .trim_end_matches("index.html");
        let raw = format!("https://{}/{}", host, trimmed);
        escape_html_attr(&raw)
    };

    // Dedup by language TAG. `build_translation_links` already dedups by
    // url_path, but two same-lang entries with different url_paths can
    // appear when both stem-grouping and translationKey-grouping produce
    // results for the same lang. Keep the first occurrence; emit each
    // language at most once.
    use std::collections::HashSet;
    let mut seen_langs: HashSet<&str> = HashSet::new();
    let mut links: Vec<String> = Vec::new();

    let alternate = |hreflang: &str, url_path: &str| {
        format!("<link rel=\"alternate\" hreflang=\"{hreflang}\" href=\"{}\">", to_href(url_path))
    };

    // Self entry first — Google docs: each page lists itself + all alternates.
    seen_langs.insert(current_lang_tag);
    links.push(alternate(current_lang_tag, current_url_path));

    // Alternates
    for link in translations {
        if seen_langs.insert(link.lang_tag.as_str()) {
            links.push(alternate(&link.lang_tag, &link.url_path));
        }
    }

    // x-default — points to the site-default-language EDITION of this page,
    // which is not the same question as tag equality: see
    // `i18n::same_language_edition` for why two tests are needed.
    let same_edition = |a: &str| crate::i18n::same_language_edition(a, site_default_lang_tag);
    let x_default = if same_edition(current_lang_tag) {
        Some(current_url_path)
    } else {
        translations.iter().find(|t| same_edition(&t.lang_tag)).map(|t| t.url_path.as_str())
    };
    if let Some(url_path) = x_default {
        links.push(alternate("x-default", url_path));
    }

    Some(links.join("\n    "))
}

/// Build the manifest entry for one page, or `None` when the page has nothing
/// beyond its title. The hover card shows the title and then the excerpt, so a
/// title-only entry would only repeat the words the link already shows.
pub fn preview_entry(
    label: &str,
    description: Option<&str>,
    preview_text: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let description = description
        .map(strip_markdown_inline)
        .filter(|d| !d.is_empty());
    if description.is_none() && preview_text.is_empty() {
        return None;
    }
    let mut entry = serde_json::Map::new();
    entry.insert("title".into(), serde_json::Value::String(label.to_string()));
    if let Some(desc) = description {
        entry.insert("description".into(), serde_json::Value::String(desc));
    }
    if !preview_text.is_empty() {
        entry.insert("preview".into(), serde_json::Value::String(preview_text.to_string()));
    }
    Some(entry)
}

#[cfg(test)]
#[path = "meta_tests.rs"]
mod tests;
