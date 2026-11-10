//! Review feature: renders book/media colophon cards for review articles.
//!
//! Two phases:
//! 1. **Process** (`process_reviews`): for each `review_of` URL with no stored
//!    item, detects the source (NeoDB, Douban, Goodreads, TMDB), fetches its
//!    catalog entry through NeoDB (direct or catalog search), and keeps it
//!    under `.moss/data/social/reviews/`, one file per URL (`link_snapshot`'s
//!    store). An item is fetched once per URL and kept, so editing a review
//!    never refetches it.
//! 2. **Enhance** (via `render_colophon`): reads the stored item for each
//!    review article and renders its colophon, with the writer's own rating
//!    from the article.

use super::html_escape;
use crate::build::page::link_snapshot::{self, Entry, Kind};
use crate::moss_paths::MossPaths;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use url::Url;

// ============================================================================
// Data types
// ============================================================================

/// One reviewed item (book, movie, etc.) as its catalog gave it. Nothing a
/// machine or a page adds (no fetch time, no writer's rating), so two machines
/// fetching one item write the same bytes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReviewItem {
    pub source_url: String,
    pub source: String,
    pub category: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub creator: Option<Vec<String>>,
    pub year: Option<i32>,
    pub publisher: Option<String>,
    pub pages: Option<i32>,
    pub isbn: Option<String>,
    pub community_rating: Option<f64>,
    pub community_rating_count: Option<i32>,
    pub external_urls: Option<BTreeMap<String, String>>,
}

// ============================================================================
// Source detection
// ============================================================================

/// Recognized review sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSource {
    NeoDB,
    Douban,
    TMDB,
    Goodreads,
}

/// Detect review source from a URL's hostname.
/// Returns `None` for unrecognized URLs.
pub fn detect_source(url_str: &str) -> Option<ReviewSource> {
    let parsed = Url::parse(url_str).ok()?;
    let host = parsed.host_str()?;
    if host.contains("neodb.social") || host.contains("neodb.") {
        Some(ReviewSource::NeoDB)
    } else if host.contains("douban.com") {
        Some(ReviewSource::Douban)
    } else if host.contains("themoviedb.org") || host.contains("tmdb.org") {
        Some(ReviewSource::TMDB)
    } else if host.contains("goodreads.com") {
        Some(ReviewSource::Goodreads)
    } else {
        None
    }
}

// ============================================================================
// NeoDB API fetch
// ============================================================================

/// Parse a NeoDB item URL into (base_url, api_path).
/// e.g. "https://neodb.social/book/abc" -> ("https://neodb.social", "/api/book/abc")
fn parse_neodb_url(url_str: &str) -> Option<(String, String)> {
    let parsed = Url::parse(url_str).ok()?;
    let path = parsed.path();
    if path.is_empty() || path == "/" {
        return None;
    }
    let base = format!("{}://{}", parsed.scheme(), parsed.host_str()?);
    let api_path = format!("/api{}", path); // allow:served-path-url-construct (NeoDB external API path, not a moss framework asset URL)
    Some((base, api_path))
}

/// Normalize a raw NeoDB JSON response into a ReviewItem.
fn normalize_neodb_response(
    raw: &serde_json::Value,
    base_url: &str,
    original_url: &str,
    source_label: &str,
) -> ReviewItem {
    // Creator: author (books), director (movies/tv), artist (music)
    let creator = raw
        .get("author")
        .or_else(|| raw.get("director"))
        .or_else(|| raw.get("artist"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });

    // Year: pub_year (books), year (movies/tv), release_year (games)
    let year = raw
        .get("pub_year")
        .or_else(|| raw.get("year"))
        .or_else(|| raw.get("release_year"))
        .and_then(|v| v.as_i64())
        .map(|y| y as i32);

    // Cover image URL (resolve relative paths)
    let cover_url = raw
        .get("cover_image_url")
        .and_then(|v| v.as_str())
        .map(|s| {
            if s.starts_with("http") {
                s.to_string()
            } else if s.starts_with("//") {
                format!("https:{}", s)
            } else {
                format!("{}{}", base_url, s)
            }
        });
    // We don't store cover_url in ReviewItem (cover is handled by frontmatter),
    // but the cover download logic may use it. For now, we note it in external_urls
    // or handle it separately.
    let _ = cover_url;

    // Rating
    let community_rating = raw.get("rating").and_then(|v| v.as_f64());
    let community_rating_count = raw
        .get("rating_count")
        .and_then(|v| v.as_i64())
        .map(|c| c as i32);

    // External URLs from external_resources
    let mut external_urls = BTreeMap::new();
    external_urls.insert("neodb".to_string(), original_url.to_string());
    if let Some(resources) = raw.get("external_resources").and_then(|v| v.as_array()) {
        for r in resources {
            if let Some(url) = r.get("url").and_then(|v| v.as_str()) {
                if let Ok(parsed) = Url::parse(url) {
                    if let Some(host) = parsed.host_str() {
                        if host.contains("douban.com") {
                            external_urls.insert("douban".to_string(), url.to_string());
                        } else if host.contains("goodreads.com") {
                            external_urls.insert("goodreads".to_string(), url.to_string());
                        } else if host.contains("openlibrary.org") {
                            external_urls.insert("openlibrary".to_string(), url.to_string());
                        } else if host.contains("imdb.com") {
                            external_urls.insert("imdb".to_string(), url.to_string());
                        } else if host.contains("themoviedb.org") {
                            external_urls.insert("tmdb".to_string(), url.to_string());
                        }
                    }
                }
            }
        }
    }

    let title = raw
        .get("display_title")
        .or_else(|| raw.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let subtitle = raw.get("subtitle").and_then(|v| v.as_str()).map(String::from);

    let category = raw.get("category").and_then(|v| v.as_str()).map(String::from);

    let publisher = raw
        .get("pub_house")
        .or_else(|| raw.get("label"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let pages = raw.get("pages").and_then(|v| v.as_i64()).map(|p| p as i32);
    let isbn = raw.get("isbn").and_then(|v| v.as_str()).map(String::from);

    ReviewItem {
        source_url: original_url.to_string(),
        source: source_label.to_string(),
        category,
        title,
        subtitle,
        creator,
        year,
        publisher,
        pages,
        isbn,
        community_rating,
        community_rating_count,
        external_urls: Some(external_urls),
    }
}

/// Fetch item metadata directly from a NeoDB API endpoint.
/// Returns `None` on any error (404, network, parse).
fn fetch_from_neodb(url: &str) -> Option<ReviewItem> {
    let (base, api_path) = parse_neodb_url(url)?;
    let api_url = format!("{}{}", base, api_path);

    let response = ureq::get(&api_url)
        .set("User-Agent", "moss")
        .set("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;

    let json: serde_json::Value = response.into_json().ok()?;
    Some(normalize_neodb_response(&json, &base, url, "neodb"))
}

/// Fetch item metadata by searching NeoDB's catalog (for Douban, Goodreads, TMDB URLs).
/// NeoDB indexes items from these sources and stores the original URL in external_resources.
fn fetch_via_catalog_search(url: &str, source_label: &str) -> Option<ReviewItem> {
    let search_url = format!(
        "https://neodb.social/api/catalog/search?query={}",
        urlencoding::encode(url)
    );

    let response = ureq::get(&search_url)
        .set("User-Agent", "moss")
        .set("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;

    let json: serde_json::Value = response.into_json().ok()?;

    // NeoDB catalog search returns { "data": [...] } or { "results": [...] }
    let items = json
        .get("data")
        .or_else(|| json.get("results"))
        .and_then(|v| v.as_array())?;

    let first = items.first()?;

    let mut item = normalize_neodb_response(first, "https://neodb.social", url, source_label);
    item.source_url = url.to_string();

    // Ensure the original URL is in external_urls under the right key
    if let Some(ref mut urls) = item.external_urls {
        urls.insert(source_label.to_string(), url.to_string());
    }

    Some(item)
}

/// Fetch review metadata from any supported source.
/// Dispatches to the appropriate fetcher based on URL detection.
pub fn fetch_review_item(url: &str) -> Option<ReviewItem> {
    let source = detect_source(url)?;
    match source {
        ReviewSource::NeoDB => fetch_from_neodb(url),
        ReviewSource::Douban => fetch_via_catalog_search(url, "douban"),
        ReviewSource::Goodreads => fetch_via_catalog_search(url, "goodreads"),
        ReviewSource::TMDB => fetch_via_catalog_search(url, "tmdb"),
    }
}

// ============================================================================
// The stored items
// ============================================================================

/// The item stored for `review_of`, if this machine holds it. One in the cloud
/// reads as none for now, has been requested, and is marked pending in
/// `evidence`, so a publish waits for a build that has read it.
pub fn stored(project_path: &str, review_of: &str, evidence: Option<&crate::build::cloud_ledger::InputEvidence>) -> Option<ReviewItem> {
    match link_snapshot::read_kind(&moss_dir(project_path), Kind::Reviews, review_of, evidence) {
        Entry::Fetched(item) => Some(item),
        Entry::InCloud | Entry::Absent => None,
    }
}

fn moss_dir(project_path: &str) -> std::path::PathBuf {
    MossPaths::new(Path::new(project_path)).root().to_path_buf()
}

/// Fetch and keep the item for each URL that has none stored. A stored item is
/// never refetched, whatever its age: it is an input every build reads, so one
/// that refreshed itself would differ between machines that synced at
/// different times. One still in the cloud is waited for, not refetched,
/// since fetching it here would overwrite another machine's. One item that
/// cannot be written is retried next time and does not stop the others.
///
/// Returns the number of items fetched and kept.
///
/// # Visibility
///
/// **`pub(in crate::build::features)` — do not widen.** Blocking network I/O.
/// Always go through
/// [`crate::build::features::sync::start_background_sync`] or
/// `refresh_comments_for_exiting_build`.
pub(in crate::build::features) fn process_reviews(project_path: &str, review_ofs: &[String]) -> usize {
    keep_fetched(project_path, review_ofs, fetch_review_item)
}

fn keep_fetched(project_path: &str, review_ofs: &[String], fetch: impl Fn(&str) -> Option<ReviewItem>) -> usize {
    let moss_dir = moss_dir(project_path);
    let mut kept = 0;
    for url in review_ofs {
        if !matches!(link_snapshot::read_kind::<ReviewItem>(&moss_dir, Kind::Reviews, url, None), Entry::Absent) {
            continue;
        }
        match fetch(url) {
            Some(item) => match link_snapshot::write_kind(&moss_dir, Kind::Reviews, url, &item) {
                Ok(()) => kept += 1,
                Err(e) => log::info!("Review: could not keep {url}: {e}"),
            },
            None => log::info!("Review: could not fetch {url}"),
        }
    }
    kept
}

/// The single file moss kept every review item in, keyed by the page's uid.
#[derive(Deserialize)]
struct Legacy {
    articles: HashMap<String, ReviewItem>,
}

/// Move the items of the old single file into the per-URL store once, then
/// remove it, only after every item it held is stored. An item already stored
/// for its URL is kept. Never writes when the old file is unreadable, so one
/// still in the cloud is moved when it arrives.
pub fn migrate_legacy(project_path: &str) {
    use crate::build::cloud_readiness::{probe_input, Found};
    let moss_dir = moss_dir(project_path);
    let path = MossPaths::new(Path::new(project_path)).social_dir().join("review.json");
    let Ok(Found::Bytes(bytes)) = probe_input(&path) else { return };
    let Ok(legacy) = serde_json::from_slice::<Legacy>(&bytes) else {
        log::warn!("Review: {} does not parse; left in place", path.display());
        return;
    };
    let mut moved = true;
    for item in legacy.articles.values() {
        if matches!(link_snapshot::read_kind::<ReviewItem>(&moss_dir, Kind::Reviews, &item.source_url, None), Entry::Absent) {
            moved &= link_snapshot::write_kind(&moss_dir, Kind::Reviews, &item.source_url, item).is_ok();
        }
    }
    if moved {
        // allow:unlink .moss/data/social/review.json, user data outside staging, removed only once every item it held is stored
        if let Err(e) = std::fs::remove_file(&path) {
            log::info!("Review: could not remove {} after moving its items: {e}", path.display());
        }
    }
}

// ============================================================================
// Colophon rendering (enhance phase)
// ============================================================================

/// Render star rating (1-5) as Unicode characters.
/// Full star: ★, Empty star: ☆
pub fn render_stars(rating: u8) -> String {
    let full = rating.min(5) as usize;
    let empty = 5 - full;
    let mut s = String::with_capacity(15);
    for _ in 0..full {
        s.push('★');
    }
    for _ in 0..empty {
        s.push('☆');
    }
    s
}

/// Render a review colophon HTML footer for a single article.
///
/// When `media_lookup` is `Some`, the cover image routes through
/// `image_render::synthesize_image_html` (the single-emission seam shared
/// by every `<img>` in moss output). This gates `<source
/// srcset="X.webp">` emission on manifest presence — eliminating the
/// WebP-404 failure mode for the colophon — and adds dims/LQIP/dominant-color
/// attribute injection at the typed-data layer. Uses `ImageContext::FolderCardCover`
/// since the colophon cover shares container-bounded thumbnail semantics
/// with folder cards (fixed `width: 150px` in `review.css`).
///
/// When `media_lookup` is `None`, falls back to the legacy bare-`<img>` emission;
/// preserves the test/fragment-rendering API surface used by the unit tests
/// in this module.
pub fn render_colophon(
    review: &ReviewItem,
    writer_rating: Option<u8>,
    cover_path: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
) -> String {
    let mut html = String::from("<footer class=\"review-colophon\">\n");

    // Cover image — ensure absolute path (article-map stores relative paths)
    if let Some(cover) = cover_path {
        let abs_cover = if cover.starts_with('/') || cover.starts_with("http") {
            cover.to_string()
        } else {
            format!("/{}", cover) // allow:served-path-url-construct (user-content cover image path from article frontmatter)
        };
        // Adapt the Option<&MediaDimensionLookup> to the AssetSnapshot
        // contract. An empty snapshot when the caller has no manifest
        // means no dimension lookup.
        // BUG 6: the snapshot indexes under the lookup's real `dir_overrides`
        // (uniform with the other FolderCardCover paths), so a colophon cover
        // under an overridden dir (e.g. CJK `图片/` → `image/`) resolves to its
        // output-URL key and gets real dims instead of the 800x600 fallback.
        let cover_assets = crate::build::media::dimensions::snapshot_or_empty(media_lookup);
        let cover_html = moss_core::render::image::synthesize_image_html(
            &abs_cover,
            &review.title,
            cover_assets,
            moss_core::render::image::ImageContext::FolderCardCover,
            &moss_core::render::image::ImageRenderOptions {
                class: Some("review-colophon-cover"),
                ..Default::default()
            },
        );
        // Indent and append (the synthesizer emits a single line; preserve the
        // legacy 2-space indent that the surrounding footer uses).
        html.push_str("  ");
        html.push_str(&cover_html);
        html.push('\n');
    }

    html.push_str("  <div class=\"review-colophon-details\">\n");

    // Title
    html.push_str(&format!(
        "    <div class=\"review-colophon-title\">{}</div>\n",
        html_escape(&review.title)
    ));

    // Subtitle (optional)
    if let Some(ref subtitle) = review.subtitle {
        html.push_str(&format!(
            "    <div class=\"review-colophon-subtitle\">{}</div>\n",
            html_escape(subtitle)
        ));
    }

    // Creator · Year
    let mut identity_parts = Vec::new();
    if let Some(ref creators) = review.creator {
        if !creators.is_empty() {
            identity_parts.push(creators.join(", "));
        }
    }
    if let Some(year) = review.year {
        identity_parts.push(year.to_string());
    }
    if !identity_parts.is_empty() {
        html.push_str(&format!(
            "    <div class=\"review-colophon-identity\">{}</div>\n",
            html_escape(&identity_parts.join(" · "))
        ));
    }

    // Publisher · Pages · ISBN
    let mut biblio_parts = Vec::new();
    if let Some(ref publisher) = review.publisher {
        biblio_parts.push(publisher.clone());
    }
    if let Some(pages) = review.pages {
        biblio_parts.push(format!("{} pages", pages));
    }
    if let Some(ref isbn) = review.isbn {
        biblio_parts.push(format!("ISBN {}", isbn));
    }
    if !biblio_parts.is_empty() {
        html.push_str(&format!(
            "    <div class=\"review-biblio\">{}</div>\n",
            html_escape(&biblio_parts.join(" · "))
        ));
    }

    // Writer rating (stars)
    if let Some(rating) = writer_rating {
        if rating > 0 {
            html.push_str(&format!(
                "    <div class=\"review-rating\">{}</div>\n",
                render_stars(rating)
            ));
        }
    }

    // Community rating
    if let Some(community_rating) = review.community_rating {
        let count = review.community_rating_count.unwrap_or(0);
        let source_label = match review.source.as_str() {
            "neodb" => "NeoDB",
            "douban" => "Douban",
            "tmdb" => "TMDB",
            "goodreads" => "Goodreads",
            other => other,
        };
        html.push_str(&format!(
            "    <div class=\"review-community-rating\">{} {}/10 · {} ratings</div>\n",
            source_label, community_rating, count
        ));
    }

    // External links
    if let Some(ref urls) = review.external_urls {
        if !urls.is_empty() {
            html.push_str(&format!("    {}\n", render_links(urls)));
        }
    }

    html.push_str("  </div>\n");
    html.push_str("</footer>");
    html
}

/// Render the external links navigation for a review.
fn render_links(external_urls: &BTreeMap<String, String>) -> String {
    // Sort links for deterministic output: Douban first, then NeoDB, then others
    let order = ["douban", "neodb", "goodreads", "openlibrary", "imdb", "tmdb"];
    let mut links: Vec<(&str, &str)> = Vec::new();

    for key in &order {
        if let Some(url) = external_urls.get(*key) {
            let label = match *key {
                "douban" => "Douban",
                "neodb" => "NeoDB",
                "goodreads" => "Goodreads",
                "openlibrary" => "Open Library",
                "imdb" => "IMDB",
                "tmdb" => "TMDB",
                other => other,
            };
            links.push((label, url.as_str()));
        }
    }

    let link_html: Vec<String> = links
        .iter()
        .map(|(label, url)| {
            format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a>",
                html_escape(url),
                label
            )
        })
        .collect();

    format!(
        "<nav class=\"review-links\">{}</nav>",
        link_html.join("<span class=\"review-sep\"> · </span>")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_review() -> ReviewItem {
        ReviewItem {
            source_url: "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO".into(),
            source: "neodb".into(),
            category: Some("book".into()),
            title: "Tools for Thought".into(),
            subtitle: Some("The History and Future of Mind-Expanding Technology".into()),
            creator: Some(vec!["Howard Rheingold".into()]),
            year: Some(2000),
            publisher: Some("MIT University Press".into()),
            pages: Some(360),
            isbn: Some("9780262681155".into()),
            community_rating: None,
            community_rating_count: Some(0),
            external_urls: Some(BTreeMap::from([
                (
                    "neodb".into(),
                    "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO".into(),
                ),
                (
                    "douban".into(),
                    "https://book.douban.com/subject/2847996/".into(),
                ),
            ])),
        }
    }

    // =====================================================================
    // Source detection tests
    // =====================================================================

    #[test]
    fn test_detect_source_neodb() {
        assert_eq!(
            detect_source("https://neodb.social/book/abc123"),
            Some(ReviewSource::NeoDB)
        );
        assert_eq!(
            detect_source("https://neodb.social/movie/xyz"),
            Some(ReviewSource::NeoDB)
        );
    }

    #[test]
    fn test_detect_source_douban() {
        assert_eq!(
            detect_source("https://book.douban.com/subject/12345/"),
            Some(ReviewSource::Douban)
        );
        assert_eq!(
            detect_source("https://movie.douban.com/subject/67890/"),
            Some(ReviewSource::Douban)
        );
    }

    #[test]
    fn test_detect_source_tmdb() {
        assert_eq!(
            detect_source("https://www.themoviedb.org/movie/12345"),
            Some(ReviewSource::TMDB)
        );
    }

    #[test]
    fn test_detect_source_goodreads() {
        assert_eq!(
            detect_source("https://www.goodreads.com/book/show/12345"),
            Some(ReviewSource::Goodreads)
        );
    }

    #[test]
    fn test_detect_source_unknown() {
        assert_eq!(detect_source("https://example.com/some/page"), None);
        assert_eq!(detect_source("https://amazon.com/book/12345"), None);
    }

    #[test]
    fn test_detect_source_invalid_url() {
        assert_eq!(detect_source("not a url"), None);
        assert_eq!(detect_source(""), None);
    }

    // =====================================================================
    // NeoDB URL parsing tests
    // =====================================================================

    #[test]
    fn test_parse_neodb_url_book() {
        let result = parse_neodb_url("https://neodb.social/book/abc123");
        assert_eq!(
            result,
            Some((
                "https://neodb.social".to_string(),
                "/api/book/abc123".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_neodb_url_movie() {
        let result = parse_neodb_url("https://neodb.social/movie/xyz789");
        assert_eq!(
            result,
            Some((
                "https://neodb.social".to_string(),
                "/api/movie/xyz789".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_neodb_url_root() {
        // Root path should return None
        assert_eq!(parse_neodb_url("https://neodb.social/"), None);
    }

    #[test]
    fn test_parse_neodb_url_invalid() {
        assert_eq!(parse_neodb_url("not a url"), None);
    }

    // =====================================================================
    // Normalization tests
    // =====================================================================

    #[test]
    fn test_normalize_neodb_book() {
        let raw = serde_json::json!({
            "id": "https://neodb.social/book/abc",
            "uuid": "abc",
            "category": "book",
            "display_title": "Seeing Like a State",
            "subtitle": "How Certain Schemes to Improve the Human Condition Have Failed",
            "author": ["James C. Scott"],
            "pub_year": 1998,
            "pub_house": "Yale University Press",
            "pages": 445,
            "isbn": "9780300078152",
            "rating": 8.2,
            "rating_count": 45,
            "cover_image_url": "https://neodb.social/m/item/cover.jpg",
            "external_resources": [
                {"url": "https://book.douban.com/subject/3062Mo/"},
                {"url": "https://www.goodreads.com/book/show/20186"}
            ]
        });

        let item = normalize_neodb_response(
            &raw,
            "https://neodb.social",
            "https://neodb.social/book/abc",
            "neodb",
        );

        assert_eq!(item.title, "Seeing Like a State");
        assert_eq!(
            item.subtitle.as_deref(),
            Some("How Certain Schemes to Improve the Human Condition Have Failed")
        );
        assert_eq!(item.creator, Some(vec!["James C. Scott".to_string()]));
        assert_eq!(item.year, Some(1998));
        assert_eq!(item.publisher, Some("Yale University Press".to_string()));
        assert_eq!(item.pages, Some(445));
        assert_eq!(item.isbn, Some("9780300078152".to_string()));
        assert_eq!(item.community_rating, Some(8.2));
        assert_eq!(item.community_rating_count, Some(45));
        assert_eq!(item.source, "neodb");
        assert_eq!(item.category, Some("book".to_string()));

        let urls = item.external_urls.unwrap();
        assert!(urls.contains_key("neodb"));
        assert!(urls.contains_key("douban"));
        assert!(urls.contains_key("goodreads"));
    }

    #[test]
    fn test_normalize_neodb_movie() {
        let raw = serde_json::json!({
            "id": "https://neodb.social/movie/xyz",
            "uuid": "xyz",
            "category": "movie",
            "display_title": "Blade Runner 2049",
            "director": ["Denis Villeneuve"],
            "year": 2017,
            "rating": 8.0,
            "rating_count": 200,
            "external_resources": [
                {"url": "https://www.imdb.com/title/tt1856101/"}
            ]
        });

        let item = normalize_neodb_response(
            &raw,
            "https://neodb.social",
            "https://neodb.social/movie/xyz",
            "neodb",
        );

        assert_eq!(item.title, "Blade Runner 2049");
        assert_eq!(
            item.creator,
            Some(vec!["Denis Villeneuve".to_string()])
        );
        assert_eq!(item.year, Some(2017));
        assert_eq!(item.category, Some("movie".to_string()));
        let urls = item.external_urls.unwrap();
        assert!(urls.contains_key("imdb"));
    }

    #[test]
    fn test_normalize_neodb_relative_cover() {
        let raw = serde_json::json!({
            "display_title": "Test",
            "cover_image_url": "/m/item/cover.jpg",
        });

        let item = normalize_neodb_response(
            &raw,
            "https://neodb.social",
            "https://neodb.social/book/test",
            "neodb",
        );

        // cover_image_url is resolved but not stored in ReviewItem directly
        // (covers are handled via frontmatter). Verify the item still works.
        assert_eq!(item.title, "Test");
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        dir
    }

    fn store(project: &Path, item: &ReviewItem) {
        link_snapshot::write_kind(&project.join(".moss"), Kind::Reviews, &item.source_url, item).unwrap();
    }

    /// Nothing a machine adds is stored, and the links are in a fixed order,
    /// so two machines fetching one item write the same bytes.
    #[test]
    fn two_identical_fetches_store_identical_bytes() {
        let (one, two) = (project(), project());
        store(one.path(), &sample_review());
        store(two.path(), &sample_review());
        let file = |p: &Path| {
            let dir = p.join(".moss/data/social/reviews");
            std::fs::read(std::fs::read_dir(dir).unwrap().next().unwrap().unwrap().path()).unwrap()
        };
        assert_eq!(file(one.path()), file(two.path()));
        let text = String::from_utf8(file(one.path())).unwrap();
        assert!(!text.contains("fetched_at") && !text.contains("writer_rating"), "{text}");
        assert!(text.find("douban").unwrap() < text.find("\"neodb\":").unwrap(), "{text}");
        assert_eq!(stored(one.path().to_str().unwrap(), &sample_review().source_url, None), Some(sample_review()));
    }

    /// The single uid-keyed file older moss kept moves into the per-URL store
    /// and is removed; an item already stored for a URL is kept.
    #[test]
    fn the_old_single_file_moves_into_the_per_url_store_once() {
        let dir = project();
        let path = dir.path().to_str().unwrap();
        let mut kept = sample_review();
        kept.title = "Fetched on another machine".into();
        store(dir.path(), &kept);
        let mut other = sample_review();
        other.source_url = "https://neodb.social/movie/other".into();
        let legacy = serde_json::json!({
            "schemaVersion": "2.0.0",
            "updatedAt": "2026-03-27T18:25:10.855Z",
            "articles": {
                "41109bd2": serde_json::to_value(sample_review()).unwrap(),
                "a0a0a0a0": {"writer_rating": 4, "fetched_at": "2026-03-27T12:44:20.608Z", "source_url": other.source_url, "source": "neodb",
                    "category": null, "title": other.title, "subtitle": other.subtitle, "creator": other.creator, "year": other.year,
                    "publisher": other.publisher, "pages": other.pages, "isbn": other.isbn, "community_rating": null,
                    "community_rating_count": 0, "external_urls": other.external_urls},
            }
        });
        let old = dir.path().join(".moss/data/social/review.json");
        std::fs::write(&old, serde_json::to_vec(&legacy).unwrap()).unwrap();
        other.category = None;

        migrate_legacy(path);
        assert_eq!(stored(path, &kept.source_url, None), Some(kept));
        assert_eq!(stored(path, &other.source_url, None), Some(other));
        assert!(!old.exists());
    }

    /// An item the sync client has not downloaded is requested and held from
    /// the publish, as a link card is: the build that draws no colophon for it
    /// cannot ship.
    #[test]
    fn an_item_in_the_cloud_is_requested_and_holds_the_publish() {
        let dir = project();
        let item = sample_review();
        store(dir.path(), &item);
        let entry = dir.path().join(".moss/data/social/reviews").join(format!("{}.json", link_snapshot::url_key(&item.source_url)));
        let _cloud = crate::build::icloud::pretend::evicted(&entry);
        let evidence = crate::build::cloud_ledger::InputEvidence::new(dir.path());
        assert_eq!(stored(dir.path().to_str().unwrap(), &item.source_url, Some(&evidence)), None);
        assert_eq!(crate::build::icloud::pretend::requests_for(&entry), 1);
        assert_eq!(evidence.unresolved_structural(), [format!(".moss/data/social/reviews/{}.json", link_snapshot::url_key(&item.source_url))]);
    }

    #[test]
    fn an_old_file_that_does_not_parse_is_left_in_place() {
        let dir = project();
        let old = dir.path().join(".moss/data/social/review.json");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "not json").unwrap();
        migrate_legacy(dir.path().to_str().unwrap());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "not json");
    }

    /// An item that cannot be written leaves the next one to be fetched and kept.
    #[test]
    fn one_item_that_cannot_be_kept_does_not_stop_the_rest() {
        let dir = project();
        let path = dir.path().to_str().unwrap();
        let (blocked, open) = ("https://neodb.social/book/blocked".to_string(), "https://neodb.social/book/open".to_string());
        let moss_dir = moss_dir(path);
        let blocked_file = moss_dir.join(format!("data/social/reviews/{}.json", link_snapshot::url_key(&blocked)));
        std::fs::create_dir_all(blocked_file.join("in-the-way")).unwrap();

        let kept = keep_fetched(path, &[blocked.clone(), open.clone()], |_| Some(sample_review()));

        assert_eq!(kept, 1);
        assert!(matches!(link_snapshot::read_kind::<ReviewItem>(&moss_dir, Kind::Reviews, &open, None), Entry::Fetched(_)));
    }

    /// The writer's rating is the article's own, never stored with the item.
    #[test]
    fn the_writer_rating_comes_from_the_article() {
        let review = sample_review();
        assert!(render_colophon(&review, Some(4), None, None).contains("★★★★☆"));
        assert!(!render_colophon(&review, None, None, None).contains("review-rating"));
    }

    // =====================================================================
    // Star rendering tests
    // =====================================================================

    #[test]
    fn test_render_stars_5() {
        assert_eq!(render_stars(5), "★★★★★");
    }

    #[test]
    fn test_render_stars_3() {
        assert_eq!(render_stars(3), "★★★☆☆");
    }

    #[test]
    fn test_render_stars_1() {
        assert_eq!(render_stars(1), "★☆☆☆☆");
    }

    #[test]
    fn test_render_stars_0() {
        assert_eq!(render_stars(0), "☆☆☆☆☆");
    }

    // =====================================================================
    // Colophon HTML tests
    // =====================================================================

    #[test]
    fn test_colophon_contains_title() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.contains("review-colophon-title"));
        assert!(html.contains("Tools for Thought"));
    }

    #[test]
    fn test_colophon_contains_subtitle() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.contains("review-colophon-subtitle"));
        assert!(html.contains("The History and Future of Mind-Expanding Technology"));
    }

    #[test]
    fn test_colophon_contains_creator_and_year() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.contains("review-colophon-identity"));
        assert!(html.contains("Howard Rheingold"));
        assert!(html.contains("2000"));
    }

    #[test]
    fn test_colophon_contains_biblio() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.contains("review-biblio"));
        assert!(html.contains("MIT University Press"));
        assert!(html.contains("360 pages"));
        assert!(html.contains("ISBN 9780262681155"));
    }

    #[test]
    fn test_colophon_contains_links() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.contains("review-links"));
        assert!(html.contains("Douban"));
        assert!(html.contains("NeoDB"));
        assert!(html.contains("https://book.douban.com/subject/2847996/"));
    }

    #[test]
    fn test_colophon_with_cover() {
        // Test the no-manifest-entry path: cover present but lookup has no
        // matching metadata. Synthesizer falls through to the bare-img form
        // and the class identifier still survives (it's structural identity,
        // not manifest data).
        let review = sample_review();
        let html = render_colophon(&review, None, Some("/image/cover.jpg"), None);
        assert!(html.contains("review-colophon-cover"));
        assert!(html.contains("src=\"/image/cover.jpg\""));
    }

    #[test]
    fn test_colophon_with_cover_routed_through_synthesizer() {
        // When the manifest carries dims for the cover, the synthesizer
        // produces the full production shape: dims + loading="lazy" +
        // data-placeholder-src. With a WebP variant present, it also wraps
        // in <picture><source srcset="X.webp">.
        use crate::build::media::dimensions::MediaDimensionLookup;
        use crate::types::content::MediaMetadata;
        let review = sample_review();
        let images = vec![MediaMetadata {
            is_animated: false,
            path: "image/cover.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 50_000,
            modified: None,
            dimensions: Some((1200, 800)),
            dominant_color: None,
            lqip_data_uri: None,
        }];
        let lookup = MediaDimensionLookup::new(&images, &[], &HashMap::new(), None);
        let html = render_colophon(&review, None, Some("/image/cover.jpg"), Some(&lookup));
        assert!(
            html.contains("review-colophon-cover"),
            "class preserved through synthesizer, got: {}", html
        );
        assert!(
            html.contains("<picture>"),
            "manifest carries WebP variant → <picture> wrap expected, got: {}", html
        );
        // 1200px-wide fixture → srcset ladder (responsive-image-variants
        // Task 3): w800 rung + base descriptor, card grid-cell sizes.
        assert!(
            html.contains(
                r#"<source srcset="/image/cover.w800.webp 800w, /image/cover.webp 1200w" type="image/webp" sizes="auto, (min-width: 48rem) 24rem, 100vw">"#
            ),
            "synthesizer must emit <source> ladder for the WebP variant, got: {}", html
        );
        assert!(
            html.contains(r#"loading="lazy""#),
            "lazy loading still applied for below-the-fold colophon, got: {}", html
        );
        assert!(
            html.contains(r#"width="1200""#) && html.contains(r#"height="800""#),
            "manifest dims must reach <img>, got: {}", html
        );
    }

    #[test]
    fn test_colophon_with_cjk_dir_overrides_routes_correctly() {
        // CJK regression guard (BUG 6). On sites with a `图片/` directory
        // remapped to `image/` via dir_overrides, the article-map persists the
        // resolved (ASCII) cover path and `dir_overrides` itself. The lookup
        // carries those overrides so `build_asset_snapshot` additively indexes
        // the cover dims under the ASCII output-URL key (`image/cover.jpg`) that
        // the cover URL probes with. Without the overrides the base-slug of the
        // CJK dir equals the raw source key (slugify preserves CJK), so the
        // ASCII-key probe would MISS and re-fire the 800x600 dims fallback.
        use crate::build::media::dimensions::MediaDimensionLookup;
        use crate::types::content::MediaMetadata;
        let review = sample_review();
        let images = vec![MediaMetadata {
            is_animated: false,
            path: "图片/cover.jpg".to_string(), // scan inventory keeps raw CJK
            file_type: "jpg".to_string(),
            size: 50_000,
            modified: None,
            dimensions: Some((1200, 800)),
            dominant_color: None,
            lqip_data_uri: None,
        }];
        let mut dir_overrides = HashMap::new();
        dir_overrides.insert("图片".to_string(), "image".to_string());
        let lookup = MediaDimensionLookup::new(&images, &[], &dir_overrides, None);
        // info.cover is the resolved (ASCII) form — what article-map stores.
        let html = render_colophon(&review, None, Some("/image/cover.jpg"), Some(&lookup));
        assert!(
            html.contains("<picture>"),
            "CJK-remapped manifest must still produce <picture> wrap, got: {}", html
        );
        // Ladder URLs derive from the resolved src too (Task 3): every rung
        // must stay in the ASCII output-URL space.
        assert!(
            html.contains(
                r#"<source srcset="/image/cover.w800.webp 800w, /image/cover.webp 1200w" type="image/webp" sizes="auto, (min-width: 48rem) 24rem, 100vw">"#
            ),
            "<source> must use the ASCII-resolved URL, not the raw CJK form, got: {}", html
        );
        assert!(
            !html.contains("图片"),
            "no raw CJK directory in the emitted HTML (would 404), got: {}", html
        );
        // BUG 6: the override-slug cover URL must resolve to the REAL dims, not
        // the 800x600 fallback. This is the exact symptom the reviewer flagged.
        assert!(
            html.contains(r#"width="1200""#) && html.contains(r#"height="800""#),
            "override-slug cover must carry real dims, not the 800x600 fallback, got: {}", html
        );
        assert!(
            !(html.contains(r#"width="800""#) && html.contains(r#"height="600""#)),
            "800x600 fallback must NOT fire for a dir-override cover, got: {}", html
        );
    }

    #[test]
    fn test_colophon_no_subtitle() {
        let mut review = sample_review();
        review.subtitle = None;
        let html = render_colophon(&review, None, None, None);
        assert!(!html.contains("review-colophon-subtitle"));
    }

    #[test]
    fn test_colophon_structure_is_footer() {
        let review = sample_review();
        let html = render_colophon(&review, None, None, None);
        assert!(html.starts_with("<footer class=\"review-colophon\">"));
        assert!(html.ends_with("</footer>"));
    }
}
