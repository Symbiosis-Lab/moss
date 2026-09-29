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

            // Cloudflare's own request-time endpoints (its email-obfuscation
            // decoder under `/cdn-cgi/l/email-protection`, and siblings under
            // the same prefix) are same-host, same-path-prefix URLs that look
            // like an ordinary in-scope link but serve no page of the site's
            // own — never queue one for import.
            if absolute_url.path().to_ascii_lowercase().starts_with("/cdn-cgi/") {
                continue;
            }

            // Normalize the URL (remove fragment)
            if let Some(normalized) = normalize_url(absolute_url.as_str()) {
                links.insert(normalized);
            }
        }
    }

    links
}

/// Content types that always qualify a fetched response as an HTML page.
const HTML_CONTENT_TYPES: [&str; 2] = ["text/html", "application/xhtml+xml"];

/// Content types the server left generic — a placeholder some servers send
/// for anything, or what ureq itself substitutes when the response carried no
/// `Content-Type` header at all (`Response::content_type()`'s documented
/// default is `text/plain`) — rather than declaring what the response
/// actually is. These fall through to a body sniff instead of being refused
/// outright.
const GENERIC_CONTENT_TYPES: [&str; 3] =
    ["text/plain", "application/octet-stream", "binary/octet-stream"];

/// Whether a fetched response should be treated as an HTML page to import,
/// rather than skipped. This is the fix for a recursive crawl writing a PDF,
/// a TIFF, an `.ics` calendar file, or an RSS feed's own XML as a garbled
/// `.md` page — every one of those was previously written to disk and built
/// into a real, published page.
///
/// A declared HTML/XHTML content type always qualifies. A missing or generic
/// type falls through to a sniff of the body's own opening bytes. Every other
/// declared type (`application/pdf`, `image/tiff`, `text/calendar`,
/// `application/rss+xml`, an image type, …) is refused outright and never
/// sniffed — a PDF that happens to start with a stray `<` must not be misread
/// as HTML because its declared type was ignored.
pub fn looks_like_html_page(content_type: &str, body: &str) -> bool {
    // A bare essence, `; charset=…` and the like dropped — ureq's own
    // `Response::content_type()` already strips this, but the check does it
    // itself too so it does not depend on a caller having done so first.
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if HTML_CONTENT_TYPES.contains(&essence.as_str()) {
        return true;
    }
    if essence.is_empty() || GENERIC_CONTENT_TYPES.contains(&essence.as_str()) {
        return sniffs_like_html(body);
    }
    false
}

/// Body sniff used only when the Content-Type header didn't say. Real markup
/// opens within its first few dozen characters, so a short prefix is enough;
/// this is never reached for a response whose Content-Type already named a
/// specific non-HTML type.
fn sniffs_like_html(body: &str) -> bool {
    // A leading UTF-8 BOM (U+FEFF) is not in Unicode's White_Space property,
    // so `trim_start()` alone leaves it in place — a real HTML page saved
    // or served with one would otherwise sniff as non-HTML and get skipped.
    let head: String = body
        .trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
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

    #[test]
    fn extract_links_never_queues_cdn_cgi_paths() {
        let html = r#"
            <a href="/cdn-cgi/l/email-protection#a1b2c3d4">Email</a>
            <a href="/about">About</a>
        "#;
        let links = extract_links(html, "https://example.com/");
        assert_eq!(links.len(), 1, "got: {links:?}");
        assert!(links.contains(&"https://example.com/about".to_string()));
    }

    // ── looks_like_html_page (the fix for a recursive crawl writing a PDF,
    // a TIFF, an .ics calendar file, or an RSS feed as a garbled .md page) ──

    #[test]
    fn looks_like_html_page_accepts_declared_html_and_xhtml() {
        assert!(looks_like_html_page("text/html", "<html></html>"));
        assert!(looks_like_html_page("text/html; charset=utf-8", "<html></html>"));
        assert!(looks_like_html_page("application/xhtml+xml", "<html></html>"));
    }

    #[test]
    fn looks_like_html_page_refuses_declared_non_html_without_sniffing() {
        // Each body opens with bytes a naive sniff could mistake for markup;
        // the declared type must still win without ever inspecting the body.
        assert!(
            !looks_like_html_page("application/pdf", "%PDF-1.4\n<html>fake</html>"),
            "a PDF must never become a page"
        );
        assert!(
            !looks_like_html_page("image/tiff", "<html>II*\u{0}binary tiff bytes</html>"),
            "a TIFF must never become a page"
        );
        assert!(
            !looks_like_html_page("text/calendar", "<html>BEGIN:VCALENDAR\nEND:VCALENDAR</html>"),
            "an .ics calendar file must never become a page"
        );
        assert!(
            !looks_like_html_page(
                "application/rss+xml",
                "<html><?xml version=\"1.0\"?><rss></rss></html>"
            ),
            "an RSS feed must never become a page"
        );
    }

    #[test]
    fn looks_like_html_page_sniffs_only_a_missing_or_generic_type() {
        assert!(looks_like_html_page("", "<!doctype html><html><body>ok</body></html>"));
        assert!(looks_like_html_page("text/plain", "  <html><body>ok</body></html>"));
        assert!(looks_like_html_page(
            "application/octet-stream",
            "<HTML><BODY>ok</BODY></HTML>"
        ));
        assert!(
            !looks_like_html_page("text/plain", "just plain text, not markup at all"),
            "generic type with a non-HTML body must still be refused"
        );
        assert!(
            !looks_like_html_page("application/octet-stream", "%PDF-1.4 binary bytes here"),
            "generic type with a non-HTML body must still be refused"
        );
    }

    #[test]
    fn looks_like_html_page_sniffs_past_a_leading_bom() {
        // A saved or served page can open with a UTF-8 BOM before its
        // doctype; the BOM is not whitespace and must not defeat the sniff.
        assert!(looks_like_html_page(
            "",
            "\u{feff}<!doctype html><html><body>ok</body></html>"
        ));
        assert!(looks_like_html_page(
            "text/plain",
            "\u{feff}  <html><body>ok</body></html>"
        ));
    }
}
