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

/// Extension families [`is_non_page_file_url`] refuses a stub for — every
/// one a real site routinely links to without it ever being a page of its
/// own. Grouped by kind for readability; the check itself just flattens
/// them. PDF and audio are not repeated here — they are
/// [`crate::vault::import::media::PDF_EXTENSIONS`] and
/// [`crate::vault::import::media::AUDIO_EXTENSIONS`], the same two rows
/// [`crate::vault::import::media::is_localizable_file_url`] already uses, so
/// "is this a PDF" has one answer instead of two extension lists that could
/// drift apart.
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "tiff", "tif", "ico", "avif", "heic", "heif",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mov", "avi", "mkv", "m4v", "wmv", "flv"];
const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2"];
const OFFICE_EXTENSIONS: &[&str] = &[
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf",
];
const CALENDAR_EXTENSIONS: &[&str] = &["ics"];
const FEED_DATA_EXTENSIONS: &[&str] = &["xml", "rss", "atom", "json"];
const STYLE_SCRIPT_EXTENSIONS: &[&str] = &["css", "js", "mjs"];

/// Whether `url`'s own path extension already names a file type that is
/// never a page — the pre-fetch counterpart to [`looks_like_html_page`] for
/// the one case that check can never reach: a fetch that failed outright and
/// so never produced a Content-Type or a body to sniff. A successful fetch
/// of a `/report.pdf` link is skipped by `looks_like_html_page` once its
/// response comes back; this answers the same question from the URL alone,
/// so a FAILED fetch of the same link is never written as a `scrape_error`
/// stub page just because it didn't get far enough to prove what
/// `looks_like_html_page` would have found anyway.
///
/// Checked on the extension only, never the query string or fragment — a
/// tracking/cache-busting suffix after `.pdf` is still a PDF. A URL with no
/// extension, or one not in any list here (including an ordinary
/// `.html`/`.htm`, `.php`, `.asp`, or `.aspx` — the extensions a page is
/// actually served at on the CMSes this importer meets), answers `false`:
/// it is still a page candidate, and a failed fetch of it keeps getting its
/// stub exactly as before this function existed.
///
/// `FEED_DATA_EXTENSIONS` is the one family that can be wrong: a `.json` or
/// `.xml` URL is almost always a feed or a data endpoint, but a headless
/// CMS can expose real content at one. Misclassifying such a URL costs
/// nothing on a successful fetch — `looks_like_html_page` would make the
/// real call from the response — and on a failed one it trades a garbled
/// `scrape_error` stub for a silent `unreachable_files` count plus the
/// `log::warn!` below, the same trade-off `VariantFetchFailed` already
/// makes for a query-string variant.
pub fn is_non_page_file_url(url_str: &str) -> bool {
    let Ok(url) = Url::parse(url_str) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    crate::vault::import::media::PDF_EXTENSIONS
        .iter()
        .chain(crate::vault::import::media::AUDIO_EXTENSIONS)
        .chain(IMAGE_EXTENSIONS)
        .chain(VIDEO_EXTENSIONS)
        .chain(ARCHIVE_EXTENSIONS)
        .chain(OFFICE_EXTENSIONS)
        .chain(CALENDAR_EXTENSIONS)
        .chain(FEED_DATA_EXTENSIONS)
        .chain(STYLE_SCRIPT_EXTENSIONS)
        .any(|ext| path.ends_with(&format!(".{ext}")))
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

/// A URL's host, lowercased — the key per-host request pacing
/// ([`super::crawl_state::HostPacer`]) groups by. Read fresh from each URL
/// rather than assumed to match the crawl's own scope host: a page's own
/// images routinely redirect to a separate CDN subdomain (see
/// `fetch_following_redirects`'s doc in `run.rs`), which is its own pacing
/// bucket, not the page host's. Empty string for a URL that fails to parse
/// or carries no host at all — its own (harmless) bucket, never a panic.
pub fn host_of(url_str: &str) -> String {
    Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// A page's identity for the no-canonical duplicate fallback (rule 2):
/// scheme + host + path, with the query string and fragment dropped
/// entirely and a leading `www.` / one trailing `/` normalized away on
/// both sides being compared. This is deliberately narrower than
/// [`extract_canonical_url`]'s identity — it exists only to recognize a
/// query-string variant of the SAME path (`?itemId=…`, `?ref=…`), never to
/// collapse two different paths onto each other just because they render
/// the same short, templated body text (two "coming soon" location pages
/// at different addresses must both stay).
pub fn path_identity(url_str: &str) -> Option<String> {
    let url = Url::parse(url_str).ok()?;
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let mut path = url.path();
    if path.len() > 1 {
        path = path.strip_suffix('/').unwrap_or(path);
    }
    Some(format!("{}://{}{}", url.scheme(), host.to_ascii_lowercase(), path))
}

/// A fetched page's own declared identity: `<link rel="canonical" href>`,
/// resolved against `page_url` and accepted only when it names the SAME host
/// as the page itself.
///
/// This is the primary signal `scrape_to_folder` dedupes a recursive crawl
/// by — a canonical tag is the site's own claim that "this exact document
/// lives here", which is what lets a real distinct page (an old WordPress
/// `?p=123`) keep importing while a lightbox `?itemId=…` or listing-filter
/// `?category=…` variant collapses onto the page it is a variant of. A
/// canonical naming a DIFFERENT host (an AMP mirror, a syndication partner)
/// is not this site's own identity and is treated the same as no canonical
/// at all — the caller falls through to a body-hash comparison instead.
pub fn extract_canonical_url(html: &str, page_url: &str) -> Option<String> {
    let page = Url::parse(page_url).ok()?;
    let document = Html::parse_document(html);
    let selector = Selector::parse(r#"link[rel~="canonical"]"#).unwrap();
    let href = document.select(&selector).find_map(|el| {
        let h = el.value().attr("href")?;
        (!h.trim().is_empty()).then_some(h)
    })?;
    let resolved = page.join(href).ok()?;
    let same_host = resolved
        .host_str()
        .zip(page.host_str())
        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b));
    if !same_host {
        return None;
    }
    normalize_url(resolved.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_lowercases_and_strips_scheme_and_path() {
        assert_eq!(host_of("https://Example.TEST/blog/post?x=1"), "example.test");
    }

    #[test]
    fn host_of_distinguishes_a_different_subdomain() {
        assert_eq!(host_of("https://cdn.example.test/a.png"), "cdn.example.test");
        assert_ne!(host_of("https://cdn.example.test/a.png"), host_of("https://example.test/"));
    }

    #[test]
    fn host_of_is_empty_for_an_unparsable_url() {
        assert_eq!(host_of("not a url"), "");
    }

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

    // ── is_non_page_file_url (the pre-fetch counterpart to
    // looks_like_html_page, for a fetch that failed before any Content-Type
    // came back) ──────────────────────────────────────────────────────────

    #[test]
    fn is_non_page_file_url_matches_one_row_from_each_family() {
        assert!(is_non_page_file_url("https://example.test/s/doc.pdf"), "pdf");
        assert!(is_non_page_file_url("https://example.test/audio/track.mp3"), "audio");
        assert!(is_non_page_file_url("https://example.test/img/photo.jpg"), "image");
        assert!(is_non_page_file_url("https://example.test/video/clip.mp4"), "video");
        assert!(is_non_page_file_url("https://example.test/dl/bundle.zip"), "archive");
        assert!(is_non_page_file_url("https://example.test/files/report.docx"), "office");
        assert!(is_non_page_file_url("https://example.test/cal/event.ics"), "calendar");
        assert!(is_non_page_file_url("https://example.test/feed.xml"), "feed/data");
        assert!(is_non_page_file_url("https://example.test/style.css"), "style/script");
    }

    #[test]
    fn is_non_page_file_url_is_case_insensitive_and_ignores_the_query_string() {
        assert!(is_non_page_file_url("https://example.test/s/DOC.PDF"));
        assert!(is_non_page_file_url("https://example.test/s/doc.pdf?v=2&cache=bust"));
    }

    #[test]
    fn is_non_page_file_url_leaves_a_page_candidate_alone() {
        assert!(
            !is_non_page_file_url("https://example.test/about"),
            "no extension at all is still a page candidate"
        );
        assert!(
            !is_non_page_file_url("https://example.test/page.html"),
            "an HTML-like extension is still a page candidate"
        );
        assert!(!is_non_page_file_url("https://example.test/"));
        assert!(!is_non_page_file_url("not a url at all"));
    }

    // ── extract_canonical_url (the page-identity signal the run loop dedupes
    // a recursive crawl's query-string variants by) ──

    #[test]
    fn extract_canonical_url_reads_a_same_host_absolute_link() {
        let html = r#"<html><head><link rel="canonical" href="https://example.test/gallery"></head></html>"#;
        assert_eq!(
            extract_canonical_url(html, "https://example.test/gallery?itemId=1"),
            Some("https://example.test/gallery".to_string())
        );
    }

    #[test]
    fn extract_canonical_url_resolves_a_relative_href() {
        let html = r#"<html><head><link rel="canonical" href="/gallery"></head></html>"#;
        assert_eq!(
            extract_canonical_url(html, "https://example.test/gallery?itemId=1"),
            Some("https://example.test/gallery".to_string())
        );
    }

    #[test]
    fn extract_canonical_url_absent_without_the_tag() {
        let html = "<html><head></head><body>no canonical here</body></html>";
        assert_eq!(extract_canonical_url(html, "https://example.test/p"), None);
    }

    #[test]
    fn extract_canonical_url_ignores_a_different_host() {
        // A canonical pointing at another host (an AMP mirror, a syndication
        // partner) is not this site's own identity — treated as absent.
        let html = r#"<html><head><link rel="canonical" href="https://amp.example.test/p"></head></html>"#;
        assert_eq!(extract_canonical_url(html, "https://example.test/p"), None);
    }

    // ── path_identity (the rule-2 no-canonical duplicate fallback's own
    // identity — narrower than extract_canonical_url on purpose) ──────────

    #[test]
    fn path_identity_ignores_the_query_string() {
        assert_eq!(
            path_identity("https://example.test/gallery?itemId=1"),
            path_identity("https://example.test/gallery?itemId=2")
        );
    }

    #[test]
    fn path_identity_normalizes_a_leading_www_and_a_trailing_slash() {
        assert_eq!(
            path_identity("https://www.example.test/blog/"),
            path_identity("https://example.test/blog")
        );
    }

    #[test]
    fn path_identity_treats_different_paths_as_different() {
        assert_ne!(
            path_identity("https://example.test/boston"),
            path_identity("https://example.test/chicago")
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
