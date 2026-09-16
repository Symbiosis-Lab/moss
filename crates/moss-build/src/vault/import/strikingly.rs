//! Strikingly dialect: detection + `window.$S` page/blog selection.
//!
//! Strikingly pages embed their full content model as a series of
//! `$S.<name>=<json>;` assignments inside a bootstrap `<script>`. This
//! module rebuilds the article body from that structured data instead of
//! scraping the rendered DOM, which kills chrome pollution and preserves
//! videos as moss `![[url]]` embeds.
//!
//! The component-tree walk is the generic [`super::infer::walk_tree`]
//! driven by the [`super::dialects::STRIKINGLY`] row; markdown emission is
//! the shared [`super::emit`]. What remains here is irreducibly Strikingly:
//! content detection, the `$S` store names, page/blog selection, and the
//! CDN res-id marker.
//!
//! Detection is content-based only — Strikingly sites live on custom
//! domains, so hostname is never evidence.

use crate::vault::import::scrape::converter::{AdapterBody, MetadataOverrides, SiteContent};
use serde_json::Value;

/// Strikingly entry point for the import engine (`super::engine::extract`).
///
/// `url` is the request URL — regular pages are matched against it (only the
/// fetched page is fully hydrated; siblings collapse to `SlideSettings`
/// stubs, so the adapter must select the current page and use only it).
/// Any parse failure or non-match returns `None` so the generic extractor
/// takes over (portfolio items intentionally land there: `portfolioMeta` has
/// no section tree).
pub(crate) fn extract(html: &str, url: &str) -> Option<SiteContent> {
    if !is_strikingly(html) {
        return None;
    }
    let res_id = extract_res_id(html).unwrap_or_default();
    if let Some(blog) = extract_s_assignment(html, "blogPostData").filter(|v| !v.is_null()) {
        return extract_blog(&blog, &res_id);
    }
    let stores = extract_s_assignment(html, "stores").filter(|v| !v.is_null())?;
    let nav = extract_s_assignment(html, "nav");
    extract_page(&stores, nav.as_ref(), url, &res_id)
}

/// Blog post: body from `blogPostData.content.sections`; title from
/// `socialMediaConfig` (clean, no site suffix); date from `publishedAt`.
fn extract_blog(blog: &Value, res_id: &str) -> Option<SiteContent> {
    let sections = blog.get("content")?.get("sections")?;
    let rendered = render_sections(sections, res_id);
    log_skipped(&rendered.skipped);
    if rendered.markdown.trim().is_empty() {
        return None;
    }
    let meta = blog.get("blogPostMeta");
    let title = meta
        .and_then(|m| m.get("socialMediaConfig"))
        .and_then(|c| c.get("title"))
        .and_then(Value::as_str)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    let date = meta
        .and_then(|m| m.get("publishedAt"))
        .and_then(Value::as_str)
        .and_then(iso_date_prefix);
    Some(SiteContent {
        body: AdapterBody::Markdown(rendered.markdown),
        overrides: MetadataOverrides {
            title,
            date,
            // Left None on purpose: frontmatter assembly fills author, and
            // the publisher==title guard already drops Strikingly's noise.
            publisher: None,
            author: None,
        },
    })
}

/// Regular page: find the current page in `pageData.pages[]`, render its
/// sections. Non-home pages match the request URL's last path segment
/// against `path`; the homepage (request path `/`) is found via the
/// `$S.nav[]` entry with `isHomePage == true` → `uid` → `pages[].uid`
/// (no page has `path == "/"`). No match → `None`, never guess.
fn extract_page(
    stores: &Value,
    nav: Option<&Value>,
    url: &str,
    res_id: &str,
) -> Option<SiteContent> {
    let pages = stores.get("pageData")?.get("pages")?.as_array()?;
    let parsed = url::Url::parse(url).ok()?;
    let request_slug = parsed
        .path_segments()
        .and_then(|mut segs| segs.rfind(|s| !s.is_empty()))
        .map(str::to_string);
    let page = match request_slug {
        Some(slug) => pages.iter().find(|p| page_slug(p) == Some(slug.as_str()))?,
        None => {
            let home_uid = nav?
                .as_array()?
                .iter()
                .find(|n| n.get("isHomePage").and_then(Value::as_bool) == Some(true))?
                .get("uid")?
                .as_str()?;
            pages
                .iter()
                .find(|p| p.get("uid").and_then(Value::as_str) == Some(home_uid))?
        }
    };
    let rendered = render_sections(page.get("sections")?, res_id);
    log_skipped(&rendered.skipped);
    if rendered.markdown.trim().is_empty() {
        return None;
    }
    let title = page
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    Some(SiteContent {
        body: AdapterBody::Markdown(rendered.markdown),
        overrides: MetadataOverrides {
            title,
            date: None,
            publisher: None,
            author: None,
        },
    })
}

/// Trailing segment of a page's `path` field ("/2" → "2", "/supportus" →
/// "supportus"). `autoPath` is a boolean flag, not a second path.
fn page_slug(page: &Value) -> Option<&str> {
    page.get("path")?
        .as_str()?
        .trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}

/// `"2024-10-13T20:08:41.387-07:00"` → `"2024-10-13"`, with a shape check so
/// garbage never lands in frontmatter.
fn iso_date_prefix(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    let b = d.as_bytes();
    (b[4] == b'-' && b[7] == b'-' && d.chars().filter(char::is_ascii_digit).count() == 8)
        .then(|| d.to_string())
}

/// Unknown component types are skipped but never silently: aggregate counts
/// into one warning line (raw eprintln is forbidden under panic=abort).
fn log_skipped(skipped: &[String]) {
    if skipped.is_empty() {
        return;
    }
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for t in skipped {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    log::warn!("strikingly import: skipped unknown component types: {counts:?}");
}

/// Content-based Strikingly detection. Never keyed on hostname — Strikingly
/// sites live on custom domains. Requires the `$S` bootstrap AND a platform
/// marker (CDN asset or a `$S` store assignment) to avoid firing on pages
/// that merely mention the platform.
pub(crate) fn is_strikingly(html: &str) -> bool {
    html.contains("window.$S={}")
        && (html.contains("strikinglycdn.com")
            || html.contains("$S.stores=")
            || html.contains("$S.blogPostData="))
}

/// Parse one `$S.<name>=<json>` assignment out of the bootstrap script via
/// the generic state-JSON scanner.
pub(crate) fn extract_s_assignment(html: &str, name: &str) -> Option<serde_json::Value> {
    super::state_json::extract_json_assignment(html, &format!("$S.{}=", name))
}

/// Read the per-page Cloudinary res id from the first
/// `strikinglycdn.com/res/<id>/` occurrence in the raw HTML. Empirically a
/// platform-wide constant (`hrscywv4p`), but read per-page — never hardcode.
pub(crate) fn extract_res_id(html: &str) -> Option<String> {
    const MARKER: &str = "strikinglycdn.com/res/";
    let (_, after_marker) = html.split_once(MARKER)?;
    let (id, _) = after_marker.split_once('/')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())).then(|| id.to_string())
}

/// Output of [`render_sections`]: the markdown body plus the component types
/// that had no match arm (skipped, but never silently — the caller logs them).
pub(crate) struct RenderedSections {
    pub markdown: String,
    /// One entry per skipped unknown component occurrence.
    pub skipped: Vec<String>,
}

/// Render a `$S` section tree (blog `content.sections` or a page's
/// `sections`) to markdown: the generic walker with the Strikingly dialect
/// row, then the shared emitter. `res_id` builds CDN URLs for images whose
/// `url` field is the `"!"` sentinel; when empty, such images are dropped
/// rather than emitted as broken links.
pub(crate) fn render_sections(sections: &serde_json::Value, res_id: &str) -> RenderedSections {
    let out = super::infer::walk_tree(sections, &super::dialects::STRIKINGLY, res_id);
    RenderedSections {
        markdown: super::emit::emit_markdown(&out.blocks),
        skipped: out.skipped,
    }
}

#[cfg(test)]
#[path = "strikingly_tests.rs"]
mod tests;
