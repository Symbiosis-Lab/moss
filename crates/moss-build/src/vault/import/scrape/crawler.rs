//! Site crawler for discovering pages within scope
//!
//! Extracts links from HTML pages and normalizes URLs for crawling.

use scraper::{Html, Selector};
use std::collections::HashSet;
use url::Url;

/// Extract all links from an HTML document
///
/// Returns a set of normalized, absolute URLs found in anchor tags.
/// Filters out non-HTTP schemes (mailto:, javascript:, tel:, etc.)
pub fn extract_links(html: &str, base_url: &str) -> HashSet<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a[href]").unwrap();
    let base = match Url::parse(base_url) {
        Ok(u) => u,
        Err(_) => return HashSet::new(),
    };

    let mut links = HashSet::new();

    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            // Skip empty hrefs
            if href.is_empty() {
                continue;
            }

            // Resolve relative URLs against base
            let absolute_url = match base.join(href) {
                Ok(u) => u,
                Err(_) => continue,
            };

            // Filter out non-HTTP schemes
            match absolute_url.scheme() {
                "http" | "https" => {}
                _ => continue,
            }

            // Normalize the URL (remove fragment)
            if let Some(normalized) = normalize_url(absolute_url.as_str()) {
                links.insert(normalized);
            }
        }
    }

    links
}

/// Normalize a URL for comparison and storage
///
/// - Removes fragment identifiers (#section)
/// - Preserves query strings
/// - Returns None for invalid URLs
pub fn normalize_url(url_str: &str) -> Option<String> {
    let mut url = Url::parse(url_str).ok()?;

    // Remove fragment
    url.set_fragment(None);

    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_links_basic() {
        let html = r#"<a href="/page">Link</a>"#;
        let links = extract_links(html, "https://example.com/");
        assert!(links.contains(&"https://example.com/page".to_string()));
    }

    #[test]
    fn test_extract_links_from_html() {
        let html = r#"
            <html>
            <body>
                <a href="/blog/post-1">Post 1</a>
                <a href="/about">About</a>
                <a href="https://external.com/page">External</a>
            </body>
            </html>
        "#;
    
        let base_url = "https://example.com/blog/";
        let links = extract_links(html, base_url);
    
        assert!(links.contains(&"https://example.com/blog/post-1".to_string()));
        assert!(links.contains(&"https://example.com/about".to_string()));
        assert!(links.contains(&"https://external.com/page".to_string()));
    }
    
    #[test]
    fn test_extract_links_resolves_relative_urls() {
        let html = r#"
            <html>
            <body>
                <a href="post-1">Post 1</a>
                <a href="./post-2">Post 2</a>
                <a href="../about">About</a>
            </body>
            </html>
        "#;
    
        let base_url = "https://example.com/blog/";
        let links = extract_links(html, base_url);
    
        assert!(links.contains(&"https://example.com/blog/post-1".to_string()));
        assert!(links.contains(&"https://example.com/blog/post-2".to_string()));
        assert!(links.contains(&"https://example.com/about".to_string()));
    }
    
    #[test]
    fn test_extract_links_ignores_non_http_schemes() {
        let html = r#"
            <html>
            <body>
                <a href="mailto:test@example.com">Email</a>
                <a href="javascript:void(0)">Click</a>
                <a href="tel:+1234567890">Call</a>
                <a href="/valid-page">Valid</a>
            </body>
            </html>
        "#;
    
        let base_url = "https://example.com/";
        let links = extract_links(html, base_url);
    
        assert_eq!(links.len(), 1);
        assert!(links.contains(&"https://example.com/valid-page".to_string()));
    }
    
    #[test]
    fn test_extract_links_removes_fragments() {
        let html = r#"
            <html>
            <body>
                <a href="/page#section1">Section 1</a>
                <a href="/page#section2">Section 2</a>
            </body>
            </html>
        "#;
    
        let base_url = "https://example.com/";
        let links = extract_links(html, base_url);
    
        // Should deduplicate after removing fragments
        assert_eq!(links.len(), 1);
        assert!(links.contains(&"https://example.com/page".to_string()));
    }
    
    #[test]
    fn test_normalize_url_removes_fragment() {
        assert_eq!(
            normalize_url("https://example.com/page#section"),
            Some("https://example.com/page".to_string())
        );
    }
    
    #[test]
    fn test_normalize_url_preserves_query_string() {
        assert_eq!(
            normalize_url("https://example.com/page?foo=bar"),
            Some("https://example.com/page?foo=bar".to_string())
        );
    }
    
    #[test]
    fn test_normalize_url_handles_trailing_slash() {
        // Both forms should normalize consistently
        let with_slash = normalize_url("https://example.com/page/");
        let without_slash = normalize_url("https://example.com/page");
    
        // For page URLs, we keep them as-is (the scope checker handles normalization)
        assert!(with_slash.is_some());
        assert!(without_slash.is_some());
    }
}
