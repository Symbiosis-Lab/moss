//! Import engine entry: dialect dispatch for pages the generic scorer
//! mishandles.
//!
//! Content-detected JSON dialects run first (builders live on custom
//! domains — hostname is never evidence), then host-gated selector
//! dialects. `None` → the caller falls back to the generic
//! readability-style extractor, which is itself the oldest convention on
//! the internet.

use super::dialects::{self, SelectorDialect};
use crate::vault::import::scrape::converter::{AdapterBody, MetadataOverrides, SiteContent};

/// The single entry the scrape pipeline calls
/// (`scrape::converter::extract_article_with_snapshot`); `snapshot` is the
/// page's pre-rendered mirror when the crawl loop fetched one (see
/// [`snapshot_request`]).
pub(crate) fn extract_with_evidence(
    html: &str,
    snapshot: Option<&str>,
    url: &str,
) -> Option<SiteContent> {
    if let Some(content) = super::strikingly::extract(html, url) {
        return Some(content);
    }
    if let Some(content) = super::readymag::extract(html, snapshot, url) {
        return Some(content);
    }
    apply_selector_dialect(&dialects::DOUBAN, html, url)
}

/// Pages the state-JSON manifest names that the DOM never links (client-
/// rendered viewers) — the crawl loop enqueues them like anchor links.
pub(crate) fn discover_pages(html: &str, url: &str) -> Vec<String> {
    super::readymag::manifest_urls(html, url)
}

/// A pre-rendered snapshot the crawl loop should fetch as extra evidence
/// for this page (an extra fetch, scoped to the site's own account).
pub(crate) fn snapshot_request(html: &str, url: &str) -> Option<String> {
    super::readymag::snapshot_request(html, url)
}

/// Generic selector-hint mechanism: on a matching host, return the first
/// non-empty content subtree as HTML for htmd conversion. Selecting the
/// container directly is the same approach Readability-style extractors use
/// for hostile layouts.
fn apply_selector_dialect(d: &SelectorDialect, html: &str, url: &str) -> Option<SiteContent> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    if host != d.host_suffix && !host.ends_with(&format!(".{}", d.host_suffix)) {
        return None;
    }
    let doc = scraper::Html::parse_document(html);
    for sel in d.selectors {
        let selector = match scraper::Selector::parse(sel) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Some(el) = doc.select(&selector).next() {
            let inner = el.inner_html();
            if !inner.trim().is_empty() {
                return Some(SiteContent {
                    body: AdapterBody::Html(inner),
                    overrides: MetadataOverrides::default(),
                });
            }
        }
    }
    None
}
