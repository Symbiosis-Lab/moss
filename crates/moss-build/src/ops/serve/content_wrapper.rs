//! Content Wrapper — wraps non-HTML responses in browser-like HTML shells
//!
//! When the preview server serves non-HTML files (images, videos, PDFs), the iframe
//! bridge script cannot be injected (it only targets HTML responses). This middleware
//! wraps those responses in minimal HTML pages so the bridge injection continues to work.
//!
//! For media files (image/video/audio), the HTML template references the original file
//! via `?__moss_raw=1` query param, which this middleware recognizes and passes through.

use axum::{
    body::Body,
    extract::Request,
    http::{header, Response, StatusCode, Uri},
    middleware::Next,
    response::IntoResponse,
};
use http_body_util::BodyExt;

// ---------------------------------------------------------------------------
// WrapType enum & categorization
// ---------------------------------------------------------------------------

/// How a response should be wrapped based on its Content-Type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapType {
    /// Already HTML/XML — pass through unchanged (bridge injection handles it).
    PassThrough,
    /// image/* — wrap in an `<img>` tag.
    Image,
    /// video/* — wrap in a `<video>` tag.
    Video,
    /// audio/* — wrap in an `<audio>` tag.
    Audio,
    /// text/plain, text/csv — inline content in a `<pre>` tag.
    PlainText,
    /// Everything else (PDF, binary, unknown) — show download/open card.
    Unsupported,
}

/// Determine how to wrap a response based on its Content-Type header value.
///
/// Handles parameters (e.g. `text/html; charset=utf-8`) by splitting on `;`
/// and examining only the media type portion.
pub fn categorize_content_type(content_type: &str) -> WrapType {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match mime.as_str() {
        // HTML, XML flavors, and web asset types — serve as-is
        "text/html" | "text/xml" | "application/xml" | "application/rss+xml"
        | "application/atom+xml" | "text/css" | "text/javascript"
        | "application/javascript" | "application/json" | "application/wasm" => {
            WrapType::PassThrough
        }

        // Plain text variants — inline in <pre>
        "text/plain" | "text/csv" => WrapType::PlainText,

        // Check broad categories via prefix
        _ if mime.starts_with("image/") => WrapType::Image,
        _ if mime.starts_with("video/") => WrapType::Video,
        _ if mime.starts_with("audio/") => WrapType::Audio,

        // Everything else
        _ => WrapType::Unsupported,
    }
}

use moss_core::media::html_escape;

// ---------------------------------------------------------------------------
// HTML template functions
// ---------------------------------------------------------------------------

/// Chromium-style image viewer: dark background, centered `<img>`.
pub fn wrap_image(request_path: &str) -> String {
    let escaped_path = html_escape(request_path);
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{path}</title></head>
<body style="margin:0;background:rgb(14, 14, 14);display:flex;align-items:center;justify-content:center;min-height:100vh;">
<img src="{path}?__moss_raw=1" style="max-width:100%;max-height:100vh;margin:auto;display:block;" alt="">
</body>
</html>"#,
        path = escaped_path
    )
}

/// Centered video player with controls on dark background.
pub fn wrap_video(request_path: &str) -> String {
    let escaped_path = html_escape(request_path);
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{path}</title></head>
<body style="margin:0;background:rgb(14, 14, 14);display:flex;align-items:center;justify-content:center;min-height:100vh;">
<video controls style="max-width:100%;max-height:100vh;margin:auto;display:block;"><source src="{path}?__moss_raw=1"></video>
</body>
</html>"#,
        path = escaped_path
    )
}

/// Centered audio player with controls on neutral background.
pub fn wrap_audio(request_path: &str) -> String {
    let escaped_path = html_escape(request_path);
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{path}</title></head>
<body style="margin:0;background:#f5f5f5;display:flex;align-items:center;justify-content:center;min-height:100vh;">
<audio controls src="{path}?__moss_raw=1"></audio>
</body>
</html>"#,
        path = escaped_path
    )
}

/// Plain text viewer: monospace `<pre>` with word-wrap. Body is HTML-escaped and inlined.
pub fn wrap_text(body: &str) -> String {
    let escaped_body = html_escape(body);
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Text</title></head>
<body style="margin:0;padding:16px;">
<pre style="word-wrap:break-word;white-space:pre-wrap;margin:0;font-family:monospace;font-size:14px;">{body}</pre>
</body>
</html>"#,
        body = escaped_body
    )
}

/// Card for unsupported file types (PDF, binary, etc.) with "Open in System Viewer"
/// and "Go Back" buttons.
pub fn wrap_unsupported(filename: &str, content_type: &str, request_path: &str) -> String {
    let escaped_filename = html_escape(filename);
    let escaped_content_type = html_escape(content_type);
    let escaped_path = html_escape(request_path);
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{filename}</title></head>
<body style="margin:0;background:#f5f5f5;display:flex;align-items:center;justify-content:center;min-height:100vh;font-family:-apple-system,BlinkMacSystemFont,sans-serif;">
<div style="background:#fff;border-radius:8px;padding:32px;text-align:center;box-shadow:0 2px 8px rgba(0,0,0,0.1);max-width:400px;">
<div style="font-size:48px;margin-bottom:16px;">📄</div>
<div style="font-size:18px;font-weight:600;margin-bottom:8px;">{filename}</div>
<div style="font-size:14px;color:#666;margin-bottom:24px;">{content_type}</div>
<button data-path="{path}" onclick="window.parent.postMessage({{type:'moss-open-in-system',path:this.dataset.path}},'*')" style="background:#007AFF;color:#fff;border:none;border-radius:6px;padding:10px 20px;font-size:14px;cursor:pointer;margin-right:8px;">Open in System Viewer</button>
<button onclick="window.history.back()" style="background:#e0e0e0;color:#333;border:none;border-radius:6px;padding:10px 20px;font-size:14px;cursor:pointer;">Go Back</button>
</div>
</body>
</html>"#,
        filename = escaped_filename,
        content_type = escaped_content_type,
        path = escaped_path,
    )
}

// ---------------------------------------------------------------------------
// Axum middleware
// ---------------------------------------------------------------------------

/// Check whether a URI query string contains `__moss_raw=1`.
fn is_raw_request(uri: &Uri) -> bool {
    uri.query()
        .map(|q: &str| {
            q.split('&')
                .any(|pair| pair == "__moss_raw=1")
        })
        .unwrap_or(false)
}

/// Rebuild a URI with the `__moss_raw=1` parameter stripped from the query string.
fn strip_raw_param(uri: &Uri) -> Uri {
    let query: &str = match uri.query() {
        Some(q) => q,
        None => return uri.clone(),
    };

    let filtered: Vec<&str> = query
        .split('&')
        .filter(|pair| *pair != "__moss_raw=1")
        .collect();

    let new_query = if filtered.is_empty() {
        String::new()
    } else {
        format!("?{}", filtered.join("&"))
    };

    let path_and_query = format!("{}{}", uri.path(), new_query);

    // Only set path_and_query — Axum middleware URIs typically don't have scheme/authority
    Uri::builder()
        .path_and_query(path_and_query.as_str())
        .build()
        .unwrap_or_else(|_| uri.clone())
}

/// Extract the filename portion from a URI path (last segment).
fn filename_from_path(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Check whether this request is a top-level navigation (e.g. user clicking a link
/// or typing in the address bar) as opposed to a sub-resource load (e.g. `<img src>`,
/// `@font-face url()`, `<script src>`).
///
/// We only wrap navigations — sub-resource loads must receive the raw file bytes,
/// otherwise images, fonts, and scripts in the user's HTML pages would break.
///
/// Uses `Sec-Fetch-Dest` (WebKit/Safari 16+) with `Accept` header fallback.
fn is_navigation_request(headers: &axum::http::HeaderMap) -> bool {
    // Primary: Sec-Fetch-Dest header (most reliable when available)
    if let Some(dest) = headers.get("sec-fetch-dest").and_then(|v| v.to_str().ok()) {
        return dest == "document" || dest == "iframe";
    }

    // Fallback: Check Accept header — navigations include text/html
    if let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) {
        return accept.contains("text/html");
    }

    // If neither header is present (very old clients), don't wrap to be safe
    false
}

/// Axum middleware that wraps non-HTML responses in browser-like HTML shells.
///
/// This ensures the iframe bridge script (injected by `inject_iframe_bridge`) can
/// operate on every response, even for media and binary files that would otherwise
/// render blank in the preview iframe.
///
/// **Only top-level navigations are wrapped** — sub-resource loads (images, fonts,
/// scripts referenced by the user's HTML) pass through unchanged to avoid breaking
/// the user's actual website rendering.
///
/// Requests with `?__moss_raw=1` are passed through unchanged so the HTML wrapper
/// templates can reference the original file content.
pub async fn wrap_non_html_content(
    request: Request,
    next: Next,
) -> Result<impl IntoResponse, StatusCode> {
    // 1. If ?__moss_raw=1 is present, strip the param and pass through
    if is_raw_request(request.uri()) {
        let (mut parts, body) = request.into_parts();
        parts.uri = strip_raw_param(&parts.uri);
        let request = Request::from_parts(parts, body);
        return Ok(next.run(request).await);
    }

    // 2. Check if this is a navigation request BEFORE forwarding
    //    Sub-resource loads (images, fonts, scripts) must receive raw bytes
    let is_navigation = is_navigation_request(request.headers());
    let request_path = request.uri().path().to_string();

    // 3. Let the request go through to the next handler
    let response = next.run(request).await;

    // 4. Don't wrap sub-resource loads — they need raw file bytes
    if !is_navigation {
        return Ok(response);
    }

    // 5. Don't wrap error responses (404, 500, etc.) — let them propagate naturally
    if !response.status().is_success() {
        return Ok(response);
    }

    // 6. Check Content-Type
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 7. Categorize
    let wrap_type = categorize_content_type(&content_type);

    // 8. PassThrough — return as-is
    if wrap_type == WrapType::PassThrough {
        return Ok(response);
    }

    // 9. Generate appropriate wrapper
    let html = match wrap_type {
        WrapType::Image => wrap_image(&request_path),
        WrapType::Video => wrap_video(&request_path),
        WrapType::Audio => wrap_audio(&request_path),
        WrapType::PlainText => {
            // Read the body to inline the text content
            let (_parts, body) = response.into_parts();
            let bytes = body
                .collect()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .to_bytes();
            let text = String::from_utf8_lossy(&bytes);
            wrap_text(&text)
        }
        WrapType::Unsupported => {
            let filename = filename_from_path(&request_path);
            wrap_unsupported(filename, &content_type, &request_path)
        }
        WrapType::PassThrough => unreachable!(),
    };

    // Build a new text/html response
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(
            header::CACHE_CONTROL,
            "no-cache, no-store, must-revalidate",
        )
        .body(Body::from(html))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(response)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ===== categorize_content_type tests =====

    #[test]
    fn test_html_is_passthrough() {
        assert_eq!(
            categorize_content_type("text/html"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_html_with_charset_is_passthrough() {
        assert_eq!(
            categorize_content_type("text/html; charset=utf-8"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_image_png() {
        assert_eq!(categorize_content_type("image/png"), WrapType::Image);
    }

    #[test]
    fn test_image_jpeg() {
        assert_eq!(categorize_content_type("image/jpeg"), WrapType::Image);
    }

    #[test]
    fn test_image_svg_xml() {
        assert_eq!(categorize_content_type("image/svg+xml"), WrapType::Image);
    }

    #[test]
    fn test_video_mp4() {
        assert_eq!(categorize_content_type("video/mp4"), WrapType::Video);
    }

    #[test]
    fn test_audio_mpeg() {
        assert_eq!(categorize_content_type("audio/mpeg"), WrapType::Audio);
    }

    #[test]
    fn test_text_plain() {
        assert_eq!(
            categorize_content_type("text/plain"),
            WrapType::PlainText
        );
    }

    #[test]
    fn test_text_csv() {
        assert_eq!(categorize_content_type("text/csv"), WrapType::PlainText);
    }

    #[test]
    fn test_text_xml_is_passthrough() {
        assert_eq!(
            categorize_content_type("text/xml"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_application_xml_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/xml"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_rss_xml_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/rss+xml"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_atom_xml_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/atom+xml"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_application_pdf_is_unsupported() {
        assert_eq!(
            categorize_content_type("application/pdf"),
            WrapType::Unsupported
        );
    }

    #[test]
    fn test_application_octet_stream_is_unsupported() {
        assert_eq!(
            categorize_content_type("application/octet-stream"),
            WrapType::Unsupported
        );
    }

    #[test]
    fn test_application_zip_is_unsupported() {
        assert_eq!(
            categorize_content_type("application/zip"),
            WrapType::Unsupported
        );
    }

    #[test]
    fn test_empty_content_type_is_unsupported() {
        assert_eq!(categorize_content_type(""), WrapType::Unsupported);
    }

    #[test]
    fn test_text_css_is_passthrough() {
        assert_eq!(
            categorize_content_type("text/css"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_text_javascript_is_passthrough() {
        assert_eq!(
            categorize_content_type("text/javascript"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_application_javascript_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/javascript"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_application_json_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/json"),
            WrapType::PassThrough
        );
    }

    #[test]
    fn test_application_wasm_is_passthrough() {
        assert_eq!(
            categorize_content_type("application/wasm"),
            WrapType::PassThrough
        );
    }

    // ===== HTML template tests =====

    #[test]
    fn test_wrap_image_contains_required_elements() {
        let html = wrap_image("/photos/cat.png");
        assert!(html.contains("<img"), "should contain an img tag");
        assert!(
            html.contains("__moss_raw=1"),
            "should reference raw URL"
        );
        assert!(
            html.contains("rgb(14, 14, 14)"),
            "should have dark background"
        );
        assert!(
            html.contains("max-width"),
            "should have max-width constraint"
        );
    }

    #[test]
    fn test_wrap_video_contains_required_elements() {
        let html = wrap_video("/media/clip.mp4");
        assert!(html.contains("<video"), "should contain a video tag");
        assert!(html.contains("controls"), "should have controls attribute");
        assert!(
            html.contains("__moss_raw=1"),
            "should reference raw URL"
        );
    }

    #[test]
    fn test_wrap_audio_contains_required_elements() {
        let html = wrap_audio("/music/track.mp3");
        assert!(html.contains("<audio"), "should contain an audio tag");
        assert!(html.contains("controls"), "should have controls attribute");
        assert!(
            html.contains("__moss_raw=1"),
            "should reference raw URL"
        );
    }

    #[test]
    fn test_wrap_text_contains_pre_and_escapes_html() {
        let html = wrap_text("Hello <script>alert('xss')</script> World");
        assert!(html.contains("<pre"), "should contain a pre tag");
        assert!(
            html.contains("word-wrap:break-word"),
            "should have word-wrap"
        );
        assert!(
            html.contains("white-space:pre-wrap"),
            "should have pre-wrap"
        );
        // Verify HTML escaping
        assert!(
            !html.contains("<script>"),
            "should escape script tags"
        );
        assert!(
            html.contains("&lt;script&gt;"),
            "should contain escaped script tag"
        );
    }

    #[test]
    fn test_wrap_unsupported_contains_required_elements() {
        let html = wrap_unsupported("document.pdf", "application/pdf", "/docs/document.pdf");
        assert!(
            html.contains("moss-open-in-system"),
            "should contain postMessage type"
        );
        assert!(
            html.contains("document.pdf"),
            "should contain filename"
        );
        assert!(
            html.contains("application/pdf"),
            "should contain content type"
        );
        assert!(
            html.contains("history.back()"),
            "should contain go back handler"
        );
        assert!(
            html.contains("data-path="),
            "should use data-path attribute for XSS safety"
        );
        assert!(
            html.contains("this.dataset.path"),
            "should read path from dataset, not inline JS string"
        );
    }

    #[test]
    fn test_wrap_unsupported_escapes_filename() {
        let html = wrap_unsupported(
            "<img onerror=alert(1)>.pdf",
            "application/pdf",
            "/docs/<img onerror=alert(1)>.pdf",
        );
        assert!(
            !html.contains("<img onerror"),
            "should escape filename to prevent XSS"
        );
        assert!(
            html.contains("&lt;img onerror"),
            "should HTML-escape the filename"
        );
    }

    // ===== Raw bypass tests =====

    #[test]
    fn test_is_raw_request_detects_param() {
        let uri: Uri = "/image.png?__moss_raw=1".parse().unwrap();
        assert!(is_raw_request(&uri));
    }

    #[test]
    fn test_is_raw_request_with_other_params() {
        let uri: Uri = "/image.png?foo=bar&__moss_raw=1".parse().unwrap();
        assert!(is_raw_request(&uri));
    }

    #[test]
    fn test_is_raw_request_false_when_absent() {
        let uri: Uri = "/image.png".parse().unwrap();
        assert!(!is_raw_request(&uri));
    }

    #[test]
    fn test_is_raw_request_false_for_empty_query() {
        let uri: Uri = "/image.png?".parse().unwrap();
        assert!(!is_raw_request(&uri));
    }

    #[test]
    fn test_strip_raw_param_only_param() {
        let uri: Uri = "/image.png?__moss_raw=1".parse().unwrap();
        let stripped = strip_raw_param(&uri);
        assert_eq!(stripped.path(), "/image.png");
        assert!(stripped.query().is_none() || stripped.query() == Some(""));
    }

    #[test]
    fn test_strip_raw_param_with_other_params() {
        let uri: Uri = "/image.png?foo=bar&__moss_raw=1".parse().unwrap();
        let stripped = strip_raw_param(&uri);
        assert_eq!(stripped.path(), "/image.png");
        assert_eq!(stripped.query(), Some("foo=bar"));
    }

    #[test]
    fn test_strip_raw_param_no_query() {
        let uri: Uri = "/image.png".parse().unwrap();
        let stripped = strip_raw_param(&uri);
        assert_eq!(stripped.path(), "/image.png");
    }

    // ===== filename_from_path tests =====

    #[test]
    fn test_filename_from_path_simple() {
        assert_eq!(filename_from_path("/docs/report.pdf"), "report.pdf");
    }

    #[test]
    fn test_filename_from_path_nested() {
        assert_eq!(
            filename_from_path("/a/b/c/image.png"),
            "image.png"
        );
    }

    #[test]
    fn test_filename_from_path_root() {
        assert_eq!(filename_from_path("/file.txt"), "file.txt");
    }

    // ===== html_escape tests =====

    #[test]
    fn test_html_escape_special_chars() {
        assert_eq!(
            html_escape("<div class=\"test\">&'foo'</div>"),
            "&lt;div class=&quot;test&quot;&gt;&amp;&#39;foo&#39;&lt;/div&gt;"
        );
    }

    #[test]
    fn test_html_escape_no_special_chars() {
        assert_eq!(html_escape("hello world"), "hello world");
    }

    // ===== is_navigation_request tests =====

    use axum::http::HeaderMap;

    #[test]
    fn test_navigation_with_sec_fetch_dest_document() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "document".parse().unwrap());
        assert!(is_navigation_request(&headers));
    }

    #[test]
    fn test_navigation_with_sec_fetch_dest_iframe() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "iframe".parse().unwrap());
        assert!(is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_with_sec_fetch_dest_image() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "image".parse().unwrap());
        assert!(!is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_with_sec_fetch_dest_style() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "style".parse().unwrap());
        assert!(!is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_with_sec_fetch_dest_font() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "font".parse().unwrap());
        assert!(!is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_with_sec_fetch_dest_script() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", "script".parse().unwrap());
        assert!(!is_navigation_request(&headers));
    }

    #[test]
    fn test_navigation_fallback_accept_header_with_text_html() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
                .parse()
                .unwrap(),
        );
        assert!(is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_accept_header_image_only() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "image/avif,image/webp,image/png,image/svg+xml,image/*;q=0.8,*/*;q=0.5"
                .parse()
                .unwrap(),
        );
        assert!(!is_navigation_request(&headers));
    }

    #[test]
    fn test_not_navigation_no_headers() {
        let headers = HeaderMap::new();
        assert!(!is_navigation_request(&headers));
    }
}
