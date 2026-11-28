//! JSON Feed 1.1 (<https://jsonfeed.org/version/1.1>): the same articles as
//! the RSS feed, with the fields RSS 2.0 has no element for (summary, image,
//! tags, language) and a stable item id, so a reader or a service following
//! the site can tell a new article from an edited one.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::rss::{article_url, feed_items};
use crate::build::types::ParsedDocument;

/// `feed.json` for a site at `site_url`, from the bodies [`feed_items`] made. Each item's `id` is the page's note id
/// once the page carries it or a publish recorded it, so it follows the page
/// through renames; a derived note id changes with every edit, so until then
/// the id is the page's address. `dir_overrides` maps source folders to the
/// served ones, so a cover's address is the one the page itself shares.
pub(crate) fn json_feed(items: &[(&ParsedDocument, String)], site_title: &str, site_url: &str, site_lang: &str, dir_overrides: &HashMap<String, String>) -> String {
    let items: Vec<Value> = items
        .iter()
        .map(|(article, body)| item(article, &article_url(site_url, &article.url_path), body, site_url, dir_overrides))
        .collect();
    let feed_url = article_url(site_url, crate::build::served_path::ServedPath::for_json_feed().as_str());
    let feed = json!({
        "version": "https://jsonfeed.org/version/1.1",
        "title": site_title,
        "home_page_url": format!("{}/", site_url.trim_end_matches('/')),
        "feed_url": feed_url,
        "language": site_lang,
        "items": items,
    });
    serde_json::to_string(&feed).expect("a JSON value serializes")
}

fn item(article: &ParsedDocument, url: &str, content_html: &str, site_url: &str, dir_overrides: &HashMap<String, String>) -> Value {
    let mut item = Map::new();
    let id = article.uid.as_deref().filter(|_| !article.uid_derived).unwrap_or(url);
    item.insert("id".into(), json!(id));
    item.insert("url".into(), json!(url));
    item.insert("title".into(), json!(article.label));
    item.insert("content_html".into(), json!(content_html));
    if let Some(summary) = article.description.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        item.insert("summary".into(), json!(summary));
    }
    if let Some(cover) = article.cover.as_deref().map(|c| moss_core::media::split_pipe(c).0).filter(|c| !c.is_empty()) {
        item.insert("image".into(), json!(cover_url(cover, site_url, dir_overrides)));
    }
    if let Some(date) = article.date.as_deref().and_then(rfc3339) {
        item.insert("date_published".into(), json!(date));
    }
    if let Some(tags) = article.tags.as_ref().filter(|t| !t.is_empty()) {
        item.insert("tags".into(), json!(tags));
    }
    let lang = article.lang_tag.as_deref().unwrap_or(article.lang.as_bcp47_attr());
    item.insert("language".into(), json!(lang));
    Value::Object(item)
}

/// A page's cover is a path from the site's root (or a full address), never
/// from the page, so it is not resolved against the item's URL.
fn cover_url(cover: &str, site_url: &str, dir_overrides: &HashMap<String, String>) -> String {
    if cover.starts_with("http://") || cover.starts_with("https://") {
        return cover.to_string();
    }
    let path = moss_core::resolve::output_url::pinned_url(cover, dir_overrides);
    format!("{}{path}", site_url.trim_end_matches('/'))
}

/// A feed date carries the day only, as the RSS feed's does.
fn rfc3339(date: &str) -> Option<String> {
    let d = moss_core::date::parse_partial_date(date)?;
    Some(format!("{:04}-{:02}-{:02}T00:00:00Z", d.year, d.month.unwrap_or(1), d.day.unwrap_or(1)))
}

#[cfg(test)]
#[path = "json_feed_tests.rs"]
mod tests;
