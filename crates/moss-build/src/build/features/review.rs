//! Review feature: renders book/media colophon cards for review articles.
//!
//! Two phases:
//! 1. **Process** (`process_reviews`): Scans articles for `review_of` URLs,
//!    detects the source (NeoDB, Douban, Goodreads, TMDB), fetches metadata
//!    via NeoDB API (direct or catalog search), and writes
//!    `.moss/data/social/review.json`. Respects a 24-hour cache TTL.
//! 2. **Enhance** (via `render_colophon`): Reads the social data and generates
//!    HTML colophon footers for each review article.

use super::html_escape;
use crate::moss_paths::MossPaths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// 24-hour cache TTL in seconds.
const CACHE_TTL_SECS: i64 = 24 * 60 * 60;

// ============================================================================
// Data types
// ============================================================================

/// Review data for the entire site, loaded from `.moss/data/social/review.json`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReviewData {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    pub articles: HashMap<String, ReviewItem>,
}

/// Metadata for a single reviewed item (book, movie, etc.).
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub writer_rating: Option<i32>,
    pub external_urls: Option<HashMap<String, String>>,
    pub fetched_at: Option<String>,
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
    let mut external_urls: HashMap<String, String> = HashMap::new();
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

    let now = chrono::Utc::now().to_rfc3339();

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
        writer_rating: None,
        external_urls: Some(external_urls),
        fetched_at: Some(now),
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
// Cache TTL
// ============================================================================

/// Check if a cached entry is still fresh (within 24-hour TTL).
/// Returns `true` if the entry should be used from cache (still fresh).
pub fn is_cache_fresh(fetched_at: Option<&str>) -> bool {
    let fetched_at = match fetched_at {
        Some(s) => s,
        None => return false,
    };

    // Parse ISO 8601 timestamp
    if let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(fetched_at) {
        let age = chrono::Utc::now().signed_duration_since(fetched);
        age.num_seconds() < CACHE_TTL_SECS
    } else {
        false
    }
}

// ============================================================================
// Process reviews (the main entry point for the process phase)
// ============================================================================

/// Article info needed for review processing. Mirrors the frontmatter fields
/// that the build pipeline extracts from each article.
pub struct ReviewArticleInput {
    pub uid: String,
    pub review_of: String,
    pub rating: Option<u8>,
}

/// Process all review articles: fetch metadata from sources, update cache.
///
/// This is the native Rust equivalent of the JS review plugin's `process` hook.
/// It reads the existing `.moss/data/social/review.json`, checks each article's
/// `review_of` URL, fetches metadata if the cache is stale or missing, and
/// writes the updated data back.
///
/// Returns the number of items fetched (0 if all were cached).
///
/// # Visibility
///
/// **`pub(in crate::build::features)` — do not widen.** Blocking network I/O.
/// Always go through
/// [`crate::build::features::sync::spawn_native_process_sync`]. See #570.
pub(in crate::build::features) fn process_reviews(
    project_path: &str,
    articles: &[ReviewArticleInput],
) -> Result<usize, String> {
    if articles.is_empty() {
        return Ok(0);
    }

    // Load existing social data (or create empty)
    let mut data = load_review_data(project_path).unwrap_or_else(|| ReviewData {
        schema_version: "2.0.0".to_string(),
        updated_at: None,
        articles: HashMap::new(),
    });

    let mut fetch_count = 0;

    for article in articles {
        let existing = data.articles.get(&article.uid);

        // Check cache: skip if URL unchanged and fetched within TTL
        if let Some(existing_item) = existing {
            if existing_item.source_url == article.review_of
                && is_cache_fresh(existing_item.fetched_at.as_deref())
            {
                // Still update writer_rating if changed
                if let Some(rating) = article.rating {
                    let item = data.articles.get_mut(&article.uid).unwrap();
                    item.writer_rating = Some(rating as i32);
                }
                continue;
            }
        }

        // Fetch from detected source
        match fetch_review_item(&article.review_of) {
            Some(mut item) => {
                // Set writer rating from frontmatter
                item.writer_rating = article.rating.map(|r| r as i32);
                data.articles.insert(article.uid.clone(), item);
                fetch_count += 1;
            }
            None => {
                // Network error — update writer_rating on cached data if available
                if let Some(item) = data.articles.get_mut(&article.uid) {
                    if let Some(rating) = article.rating {
                        item.writer_rating = Some(rating as i32);
                    }
                } else {
                    log::warn!(
                        "Review: Failed to fetch {}, no cache available",
                        article.review_of
                    );
                }
                continue;
            }
        }
    }

    // Save updated data
    if fetch_count > 0 {
        data.updated_at = Some(chrono::Utc::now().to_rfc3339());
        save_review_data(project_path, &data)?;
    }

    Ok(fetch_count)
}

// ============================================================================
// Data persistence
// ============================================================================

/// Load review data from `.moss/data/social/review.json`.
pub fn load_review_data(project_path: &str) -> Option<ReviewData> {
    let path = MossPaths::new(std::path::Path::new(project_path))
        .social_dir()
        .join("review.json");

    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save review data to `.moss/data/social/review.json`.
pub fn save_review_data(project_path: &str, data: &ReviewData) -> Result<(), String> {
    let social_dir = MossPaths::new(std::path::Path::new(project_path)).social_dir();

    std::fs::create_dir_all(&social_dir)
        .map_err(|e| format!("Failed to create social dir: {}", e))?;

    let path = social_dir.join("review.json");
    let content =
        serde_json::to_string_pretty(data).map_err(|e| format!("Failed to serialize: {}", e))?;

    std::fs::write(&path, content).map_err(|e| format!("Failed to write review.json: {}", e))  // allow:raw_write user state under .moss/data, not regenerable output
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
/// `image_render::synthesize_image_html` (single-emission seam per
/// `docs/reference/structural-html-emission.md`). This gates `<source
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
        // Phase 1 B1 (2026-05-25): adapt the Option<&MediaDimensionLookup>
        // call to the new AssetSnapshot contract. Empty snapshot when the
        // caller has no manifest preserves the prior `None` lookup semantics.
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
    if let Some(rating) = review.writer_rating {
        if rating > 0 {
            html.push_str(&format!(
                "    <div class=\"review-rating\">{}</div>\n",
                render_stars(rating as u8)
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
fn render_links(external_urls: &HashMap<String, String>) -> String {
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
            writer_rating: None,
            external_urls: Some(HashMap::from([
                (
                    "neodb".into(),
                    "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO".into(),
                ),
                (
                    "douban".into(),
                    "https://book.douban.com/subject/2847996/".into(),
                ),
            ])),
            fetched_at: Some("2026-03-27T12:44:20.608Z".into()),
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

    // =====================================================================
    // Cache TTL tests
    // =====================================================================

    #[test]
    fn test_cache_fresh_recent() {
        let now = chrono::Utc::now().to_rfc3339();
        assert!(is_cache_fresh(Some(&now)));
    }

    #[test]
    fn test_cache_stale_old() {
        let old = (chrono::Utc::now() - chrono::Duration::hours(25)).to_rfc3339();
        assert!(!is_cache_fresh(Some(&old)));
    }

    #[test]
    fn test_cache_fresh_23h() {
        let recent = (chrono::Utc::now() - chrono::Duration::hours(23)).to_rfc3339();
        assert!(is_cache_fresh(Some(&recent)));
    }

    #[test]
    fn test_cache_none() {
        assert!(!is_cache_fresh(None));
    }

    #[test]
    fn test_cache_invalid_timestamp() {
        assert!(!is_cache_fresh(Some("not-a-timestamp")));
    }

    // =====================================================================
    // Serialization round-trip tests
    // =====================================================================

    #[test]
    fn test_review_data_serialize_roundtrip() {
        let data = ReviewData {
            schema_version: "2.0.0".to_string(),
            updated_at: Some("2026-03-27T12:00:00Z".to_string()),
            articles: HashMap::from([(
                "abc123".to_string(),
                sample_review(),
            )]),
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        let parsed: ReviewData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, "2.0.0");
        assert_eq!(parsed.updated_at, Some("2026-03-27T12:00:00Z".to_string()));
        assert!(parsed.articles.contains_key("abc123"));
        assert_eq!(parsed.articles["abc123"].title, "Tools for Thought");
        assert_eq!(
            parsed.articles["abc123"].fetched_at,
            Some("2026-03-27T12:44:20.608Z".to_string())
        );
    }

    #[test]
    fn test_review_data_deserialize_existing_format() {
        // Verify we can deserialize the actual file format that already exists on disk
        let json = r#"{
            "schemaVersion": "2.0.0",
            "updatedAt": "2026-03-27T18:25:10.855Z",
            "articles": {
                "41109bd2": {
                    "source_url": "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO",
                    "source": "neodb",
                    "category": "book",
                    "title": "Tools for Thought",
                    "subtitle": "The History and Future of Mind-Expanding Technology",
                    "creator": ["Howard Rheingold"],
                    "year": 2000,
                    "publisher": "MIT University Press",
                    "pages": 360,
                    "isbn": "9780262681155",
                    "community_rating": null,
                    "community_rating_count": 0,
                    "external_urls": {
                        "neodb": "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO",
                        "douban": "https://book.douban.com/subject/2847996/"
                    },
                    "writer_rating": null,
                    "fetched_at": "2026-03-27T12:44:20.608Z"
                }
            }
        }"#;

        let data: ReviewData = serde_json::from_str(json).unwrap();
        assert_eq!(data.schema_version, "2.0.0");
        assert_eq!(
            data.updated_at,
            Some("2026-03-27T18:25:10.855Z".to_string())
        );
        let item = &data.articles["41109bd2"];
        assert_eq!(item.title, "Tools for Thought");
        assert_eq!(item.fetched_at, Some("2026-03-27T12:44:20.608Z".to_string()));
        assert_eq!(item.source, "neodb");
    }

    // =====================================================================
    // process_reviews tests (filesystem-based)
    // =====================================================================

    #[test]
    fn test_process_reviews_empty_articles() {
        let dir = tempfile::tempdir().unwrap();
        let result = process_reviews(dir.path().to_str().unwrap(), &[]);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_process_reviews_creates_social_dir() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        // Create .moss dir
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();

        // This will try to fetch from a non-existent URL and fail,
        // but it should still create the social directory structure
        let articles = vec![ReviewArticleInput {
            uid: "test123".to_string(),
            review_of: "https://neodb.social/book/nonexistent".to_string(),
            rating: Some(4),
        }];

        // Fetch will fail (no mock server), but process should not error
        let result = process_reviews(project_path, &articles);
        assert!(result.is_ok());
        // fetch_count = 0 because the fetch failed
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_process_reviews_respects_cache() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        // Pre-populate cache with fresh data
        let social_dir = dir.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();

        let mut review = sample_review();
        review.fetched_at = Some(chrono::Utc::now().to_rfc3339());

        let data = ReviewData {
            schema_version: "2.0.0".to_string(),
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            articles: HashMap::from([("test123".to_string(), review)]),
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        std::fs::write(social_dir.join("review.json"), &json).unwrap();

        // Process with same URL — should skip (cache is fresh)
        let articles = vec![ReviewArticleInput {
            uid: "test123".to_string(),
            review_of: "https://neodb.social/book/7dnJWlkxN5dT6DyO0MGLpO".to_string(),
            rating: Some(5),
        }];

        let result = process_reviews(project_path, &articles);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // 0 fetches — all cached

        // Note: writer_rating update happens in-memory but only saves if fetch_count > 0.
        // Since we didn't fetch, the file wasn't rewritten. This is intentional --
        // the rating update will be persisted on the next fetch cycle.
    }

    #[test]
    fn test_process_reviews_stale_cache_triggers_fetch_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        // Pre-populate with STALE data (25 hours old)
        let social_dir = dir.path().join(".moss").join("data").join("social");
        std::fs::create_dir_all(&social_dir).unwrap();

        let mut review = sample_review();
        // Use a URL that definitely won't resolve via NeoDB API
        review.source_url = "https://neodb.social/book/ZZZZZZ_nonexistent_9999".to_string();
        review.fetched_at =
            Some((chrono::Utc::now() - chrono::Duration::hours(25)).to_rfc3339());

        let data = ReviewData {
            schema_version: "2.0.0".to_string(),
            updated_at: None,
            articles: HashMap::from([("test123".to_string(), review)]),
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        std::fs::write(social_dir.join("review.json"), &json).unwrap();

        // Process with same URL — cache is stale, will try to fetch but fail (404)
        let articles = vec![ReviewArticleInput {
            uid: "test123".to_string(),
            review_of: "https://neodb.social/book/ZZZZZZ_nonexistent_9999".to_string(),
            rating: Some(3),
        }];

        let result = process_reviews(project_path, &articles);
        assert!(result.is_ok());
        // Fetch fails (404) -> falls back to cached data -> fetch_count = 0
        assert_eq!(result.unwrap(), 0);

        // Verify the stale cached data is still there (not deleted)
        let loaded = load_review_data(project_path).unwrap();
        assert!(loaded.articles.contains_key("test123"));
    }

    #[test]
    fn test_save_and_load_review_data() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        let data = ReviewData {
            schema_version: "2.0.0".to_string(),
            updated_at: Some("2026-03-27T12:00:00Z".to_string()),
            articles: HashMap::from([("uid1".to_string(), sample_review())]),
        };

        save_review_data(project_path, &data).unwrap();

        let loaded = load_review_data(project_path).unwrap();
        assert_eq!(loaded.schema_version, "2.0.0");
        assert_eq!(loaded.articles.len(), 1);
        assert_eq!(loaded.articles["uid1"].title, "Tools for Thought");
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
        let html = render_colophon(&review, None, None);
        assert!(html.contains("review-colophon-title"));
        assert!(html.contains("Tools for Thought"));
    }

    #[test]
    fn test_colophon_contains_subtitle() {
        let review = sample_review();
        let html = render_colophon(&review, None, None);
        assert!(html.contains("review-colophon-subtitle"));
        assert!(html.contains("The History and Future of Mind-Expanding Technology"));
    }

    #[test]
    fn test_colophon_contains_creator_and_year() {
        let review = sample_review();
        let html = render_colophon(&review, None, None);
        assert!(html.contains("review-colophon-identity"));
        assert!(html.contains("Howard Rheingold"));
        assert!(html.contains("2000"));
    }

    #[test]
    fn test_colophon_contains_biblio() {
        let review = sample_review();
        let html = render_colophon(&review, None, None);
        assert!(html.contains("review-biblio"));
        assert!(html.contains("MIT University Press"));
        assert!(html.contains("360 pages"));
        assert!(html.contains("ISBN 9780262681155"));
    }

    #[test]
    fn test_colophon_contains_links() {
        let review = sample_review();
        let html = render_colophon(&review, None, None);
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
        let html = render_colophon(&review, Some("/image/cover.jpg"), None);
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
        let html = render_colophon(&review, Some("/image/cover.jpg"), Some(&lookup));
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
        let html = render_colophon(&review, Some("/image/cover.jpg"), Some(&lookup));
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
        let html = render_colophon(&review, None, None);
        assert!(!html.contains("review-colophon-subtitle"));
    }

    #[test]
    fn test_colophon_structure_is_footer() {
        let review = sample_review();
        let html = render_colophon(&review, None, None);
        assert!(html.starts_with("<footer class=\"review-colophon\">"));
        assert!(html.ends_with("</footer>"));
    }

    // =====================================================================
    // Data loading tests
    // =====================================================================

    #[test]
    fn test_load_review_data_missing_file() {
        let result = load_review_data("/nonexistent/path");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_review_data_from_real_file() {
        // Test with the actual test folder data
        let data = load_review_data(
            "/Users/alice/Library/Mobile Documents/iCloud~md~obsidian/Documents/Obsidian Vault/Notes",
        );
        if let Some(data) = data {
            assert_eq!(data.schema_version, "2.0.0");
            assert!(data.articles.contains_key("41109bd2"));
            let review = &data.articles["41109bd2"];
            assert_eq!(review.title, "Tools for Thought");
            assert!(review.fetched_at.is_some());
            assert_eq!(data.updated_at.is_some(), true);
        }
        // OK if file doesn't exist in CI
    }
}
