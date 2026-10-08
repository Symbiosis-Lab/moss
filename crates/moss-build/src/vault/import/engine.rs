//! Import engine entry: dialect dispatch for pages the generic scorer
//! mishandles.
//!
//! Content-detected JSON dialects run first (builders live on custom
//! domains — hostname is never evidence), then host-gated selector
//! dialects. `None` → the caller falls back to the generic
//! readability-style extractor, which is itself the oldest convention on
//! the internet.

use super::dialects::{self, ChromeGate, SelectorDialect};
use crate::vault::import::scrape::metadata::{same_name, EventMetadata};
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

/// Remove the builder chrome the dialect rows name, before the generic
/// extractor sees the page. A row whose gate does not hold (no event `start`
/// or `location` in the frontmatter, a line that is not the venue name) leaves
/// its markup, so a fact the frontmatter lacks stays in the page. A page no
/// row matches comes back byte for byte.
pub(crate) fn strip_dialect_chrome(html: &str, event: &EventMetadata) -> String {
    if !carries_dialect_chrome(html) {
        return html.to_string();
    }
    let mut doc = scraper::Html::parse_document(html);
    let mut doomed = Vec::new();
    for row in dialects::CHROME_DIALECTS.iter().flat_map(|d| d.rows) {
        let Ok(selector) = scraper::Selector::parse(row.selector) else { continue };
        doomed.extend(doc.select(&selector).filter(|el| gate_holds(row.gate, el, event)).map(|el| el.id()));
    }
    if doomed.is_empty() {
        return html.to_string();
    }
    for id in doomed {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
    doc.html()
}

/// Whether `html` mentions any class a chrome row selects, so a page with none
/// of a dialect's chrome is never parsed or reserialised. The tokens are read
/// from the rows' own selectors; a false positive only costs the parse.
fn carries_dialect_chrome(html: &str) -> bool {
    dialects::CHROME_DIALECTS.iter().flat_map(|d| d.rows).any(|row| {
        row.selector.split('.').skip(1).any(|rest| {
            let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            html.contains(&rest[..end.unwrap_or(rest.len())])
        })
    })
}

fn gate_holds(gate: ChromeGate, el: &scraper::ElementRef<'_>, event: &EventMetadata) -> bool {
    match gate {
        ChromeGate::CardHas(selector) => {
            let card = el.ancestors().filter_map(scraper::ElementRef::wrap).find(|a| {
                a.value().has_class("summary-item", scraper::CaseSensitivity::CaseSensitive)
            });
            scraper::Selector::parse(selector)
                .ok()
                .zip(card)
                .is_some_and(|(sel, card)| card.select(&sel).next().is_some())
        }
        ChromeGate::EventStart => event.start.is_some(),
        ChromeGate::EventLocation => event.location.is_some(),
        ChromeGate::EventVenueLine => event
            .location
            .as_deref()
            .is_some_and(|venue| same_name(&el.text().collect::<String>(), venue)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pre-check is the only thing standing between an unrelated page and
    /// a parse plus reserialise. The output of an unmatched page is the same
    /// string either way, so only the predicate can be pinned.
    #[test]
    fn only_a_page_naming_a_row_class_is_parsed() {
        assert!(!carries_dialect_chrome("<article><p>Plain page, <b>unclosed</p>"));
        assert!(carries_dialect_chrome(r#"<a class="x eventitem-backlink">Back</a>"#));
        assert!(carries_dialect_chrome(r#"<div class="summary-thumbnail-event-date">"#));
    }
}
