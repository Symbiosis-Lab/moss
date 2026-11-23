//! Syndication and discovery outputs: RSS and JSON feeds, calendar files, sitemaps, llms.txt, redirects, and
//! the full-text search index.

pub mod calendar;
pub mod json_feed;
pub(crate) mod import_addresses;
pub(crate) mod kept_addresses;
pub mod llms_txt;
pub mod previews;
pub mod redirect_table;
pub mod redirects;
pub mod robots;
pub mod rss;
pub mod search;
pub mod search_lane;
pub mod sitemap;

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;

/// The site's feeds: `rss.xml`, and `feed.json` with the same articles plus
/// the fields RSS has no element for. Its former address `feed.xml` is an
/// entry of the redirect table, written with the rest of it.
pub(crate) fn emit(
    documents: &[crate::build::types::ParsedDocument],
    url: &str,
    site_lang: crate::i18n::Language,
    dir_overrides: &std::collections::HashMap<String, String>,
    output_dir: &std::path::Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let home = documents.iter().find(|d| d.url_path == "index.html");
    let rss_site_title = home
        .map(|d| d.title.as_str())
        .unwrap_or(crate::i18n::t(site_lang, "site"));

    let items = crate::build::feeds::rss::feed_items(documents, url);
    let rss_content = crate::build::feeds::rss::rss_feed(
        &items,
        rss_site_title,
        url,
        None,
        home.and_then(|d| d.analytics.as_ref()),
    )
    .into_bytes();

    let rss_path = ServedPath::for_rss("").unwrap();
    BuildContext::for_render(output_dir, pending)
        .emit_held(&rss_path, rss_content, HashBucket::Files)
        .map_err(|e| format!("Failed to emit {}: {}", rss_path.as_str(), e))?;

    let json_feed = crate::build::feeds::json_feed::json_feed(
        &items,
        rss_site_title,
        url,
        site_lang.as_bcp47_attr(),
        dir_overrides,
    )
    .into_bytes();
    let json_path = ServedPath::for_json_feed();
    BuildContext::for_render(output_dir, pending)
        .emit_held(&json_path, json_feed, HashBucket::Files)
        .map_err(|e| format!("Failed to emit {}: {}", json_path.as_str(), e))?;
    Ok(())
}
