//! JSON Feed 1.1 (<https://jsonfeed.org/version/1.1>): the same articles as
//! the RSS feed, with the fields RSS 2.0 has no element for (summary, image,
//! tags, language) and a stable item id, so a reader or a service following
//! the site can tell a new article from an edited one.

use serde_json::{json, Map, Value};

use super::rss::{article_url, feed_articles, item_html};
use crate::build::types::ParsedDocument;
use crate::syndication::shape::absolute;

/// The feed for a site at `site_url`. Each item's `id` is the page's note id
/// once the page carries it or a publish recorded it, so it follows the page
/// through renames; a derived note id changes with every edit, so until then
/// the id is the page's address.
pub fn generate_json_feed(documents: &[ParsedDocument], site_title: &str, site_url: &str, site_lang: &str) -> String {
    let mut scopes = crate::build::folder_embed::inline_body::IdScopes::default();
    let items: Vec<Value> = feed_articles(documents)
        .into_iter()
        .enumerate()
        .map(|(index, article)| {
            let url = article_url(site_url, &article.url_path);
            let html = item_html(article, index, &mut scopes, site_url, str::to_string);
            item(article, &url, absolute_html(&html, &url, site_url), site_url)
        })
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

fn item(article: &ParsedDocument, url: &str, content_html: String, site_url: &str) -> Value {
    let mut item = Map::new();
    let id = article.uid.as_deref().filter(|_| !article.uid_derived).unwrap_or(url);
    item.insert("id".into(), json!(id));
    item.insert("url".into(), json!(url));
    item.insert("title".into(), json!(article.label));
    item.insert("content_html".into(), json!(content_html));
    if let Some(summary) = article.description.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        item.insert("summary".into(), json!(summary));
    }
    if let Some(cover) = article.cover.as_deref().filter(|c| !c.is_empty()) {
        item.insert("image".into(), json!(absolute(cover, url, site_url)));
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

/// A feed date carries the day only, as the RSS feed's does.
fn rfc3339(date: &str) -> Option<String> {
    let d = crate::build::components::date::parse_partial_date(date)?;
    Some(format!("{:04}-{:02}-{:02}T00:00:00Z", d.year, d.month.unwrap_or(1), d.day.unwrap_or(1)))
}

/// Links and pictures as a reader elsewhere follows them. A fragment link
/// stays relative, so it still jumps within the item. `srcset` and `<source>`
/// go, since their candidates are relative and `src` already names the picture.
fn absolute_html(html: &str, page: &str, site: &str) -> String {
    lol_html::rewrite_str(html, lol_html::RewriteStrSettings {
        element_content_handlers: vec![
            lol_html::element!("a[href]", |el| {
                if let Some(href) = el.get_attribute("href").filter(|h| !h.starts_with('#')) {
                    el.set_attribute("href", &absolute(&href, page, site))?;
                }
                Ok(())
            }),
            lol_html::element!("img", |el| {
                if let Some(src) = el.get_attribute("src") {
                    el.set_attribute("src", &absolute(&src, page, site))?;
                }
                el.remove_attribute("srcset");
                el.remove_attribute("sizes");
                Ok(())
            }),
            lol_html::element!("source", |el| {
                el.remove();
                Ok(())
            }),
        ],
        ..Default::default()
    })
    .unwrap_or_else(|_| html.to_string())
}

#[cfg(test)]
#[path = "json_feed_tests.rs"]
mod tests;
