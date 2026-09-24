//! Asset Rewriter — removes video placeholder attributes for completed assets
//!
//! ## Problem
//!
//! Video AssetReady events are fire-and-forget. They fire while the user is on
//! the homepage (no `<video>` elements), so they're silently lost. When the user
//! navigates to a video page later, the video shows a placeholder forever.
//!
//! ## Solution
//!
//! This middleware rewrites HTML on-the-fly: for videos whose conversion is
//! already complete (tracked in `AssetRegistry`), it removes the
//! `data-placeholder-src` attribute. This tells iframe-bridge "no swap needed"
//! — the video is ready.
//!
//! ## Design Decisions
//!
//! - **Why server-side rewrite instead of event replay**: Events are
//!   fire-and-forget; caching them adds complexity and state management.
//!   Server-side rewrite is stateless — each HTML response is correct for the
//!   current asset state.
//!
//! - **Why remove `data-placeholder-src` instead of other changes**:
//!   iframe-bridge already skips videos without this attribute. Minimal change,
//!   maximum compatibility.
//!
//! - **Why regex instead of HTML parser**: Following the project's existing
//!   pattern (`inject_iframe_bridge` uses string manipulation). Video tags have
//!   predictable structure from the build pipeline.
//!
//! ## Second job: the inline LQIP never reaches a preview
//!
//! moss emits a blur-up placeholder two ways, and removing one is not removing
//! both. The registry channel is gone (`AssetPlaceholder` carries no LQIP), but
//! `moss_core::render::image` also bakes one **inline into the HTML** as
//! `style="background-image:url(data:image/jpeg;base64,…)"` on the inner
//! `<img>`. That one is emitted by the same renderer for preview and for
//! publish, because there is one pipeline and the generation the preview serves
//! is the generation that gets deployed — so it cannot be switched off at render
//! time without also taking it off the published site, which is the one place it
//! belongs.
//!
//! So it comes off HERE, at serve time, which is precisely the layer this
//! design assigns the job: "HTML is structural and stable. Servers handle availability."
//! The bytes on disk keep their LQIP for the deploy; the preview response does
//! not carry one.
//!
//! Why it must come off at all: an author looking at their own photo to judge it
//! cannot tell a blur that resembles it from a badly-encoded result. With the
//! source passthrough serving the real original the background is invisible
//! anyway — until the moment it is not. If the passthrough cannot read the
//! source (cloud-evicted, deleted mid-session), the fallback is a 1×1
//! **transparent** WebP, and a transparent image over an LQIP background paints
//! the LQIP, scaled to the `width`/`height` attributes. That is the failure a
//! production incident recorded: "Users saw the LQIP placeholder permanently
//! in place of the photo."

use axum::{
    body::Body,
    extract::Request,
    http::{header, Response, StatusCode},
    middleware::Next,
};
use http_body_util::BodyExt;
use regex::Regex;
use std::sync::{Arc, LazyLock};

use crate::types::assets::AssetRegistry;

// Pre-built regexes — built once at first use, reused on every HTML response.
// These are static because the preview server processes HTML on every navigation.
static VIDEO_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<video\b[^>]*data-placeholder-src="[^"]*"[^>]*>"#).unwrap()
});
static SRC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[\s]src="([^"]*)""#).unwrap()
});
static PLACEHOLDER_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s*data-placeholder-src="[^"]*""#).unwrap()
});

/// Resolve a document-relative path to a root-relative path.
///
/// The middleware sees document-relative paths in HTML (e.g., `src="../clip.mp4"`)
/// but AssetRegistry uses root-relative keys (e.g., `videos/clip.mp4`).
///
/// Example: request_path="/videos/冬日之歌/index.html", relative="../clip.mp4"
/// -> "videos/clip.mp4"
fn resolve_to_root_relative(request_path: &str, relative: &str) -> String {
    // If the path is already absolute (starts with /), just strip the leading slash
    if relative.starts_with('/') {
        return relative.trim_start_matches('/').to_string();
    }

    // Get directory of the request (strip filename)
    let dir = request_path
        .trim_start_matches('/')
        .rsplit_once('/')
        .map(|(d, _)| d)
        .unwrap_or("");

    // Split into path segments
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();

    // Apply relative path segments
    for part in relative.split('/') {
        match part {
            ".." => {
                segments.pop();
            }
            "." | "" => {}
            other => segments.push(other),
        }
    }

    segments.join("/")
}

/// Rewrite HTML to remove `data-placeholder-src` from video tags whose assets
/// are already ready in the AssetRegistry.
///
/// For each `<video` tag with `data-placeholder-src`:
/// 1. Extract the `src` attribute value
/// 2. Resolve to root-relative path using the request URI
/// 3. URL-decode (for Unicode filenames like `%E5%86%AC%E6%97%A5%E4%B9%8B%E6%AD%8C`)
/// 4. Check AssetRegistry
/// 5. If Ready: remove `data-placeholder-src="..."` from the tag
fn rewrite_video_placeholders(html: &str, request_path: &str, registry: &AssetRegistry) -> String {
    use crate::types::assets::AssetState;

    // Fast path: skip allocation if no video placeholders exist in the HTML
    if !html.contains("data-placeholder-src") {
        return html.to_string();
    }

    let mut result = html.to_string();

    // Collect all matches first to avoid borrow issues during replacement
    let matches: Vec<String> = VIDEO_TAG_RE
        .find_iter(html)
        .map(|m| m.as_str().to_string())
        .collect();

    for video_tag in &matches {
        // Extract the src attribute value
        let src_value = match SRC_RE.captures(video_tag) {
            Some(caps) => caps.get(1).unwrap().as_str().to_string(),
            None => continue,
        };

        // Resolve document-relative path to root-relative
        let root_relative = resolve_to_root_relative(request_path, &src_value);

        // URL-decode for Unicode filenames (e.g., %E5%86%AC -> 冬)
        let decoded = urlencoding::decode(&root_relative)
            .map(|s| s.into_owned())
            .unwrap_or(root_relative);

        // Check if the asset is ready in the registry
        match registry.get(&decoded) {
            Some(AssetState::Ready) => {
                // Asset is ready — remove data-placeholder-src attribute.
                // Use replacen(…, 1) to replace only the first occurrence,
                // preventing accidental double-replacement if identical tags exist.
                let rewritten_tag = PLACEHOLDER_ATTR_RE.replace(video_tag, "").to_string();
                result = result.replacen(video_tag.as_str(), &rewritten_tag, 1);
            }
            _ => {
                // Asset is pending, unknown, or not in registry — leave unchanged
            }
        }
    }

    result
}

/// Axum middleware that rewrites HTML responses to remove video placeholder
/// attributes for assets that have completed processing.
///
/// This middleware sits between `wrap_non_html_content` and `inject_iframe_bridge`
/// in the middleware stack. It only modifies HTML responses; all other content
/// types pass through unchanged.
/// Matches one `background-image:url(data:…)` declaration and an immediately
/// adjacent `background-size:…` declaration, inside a `style=` attribute value.
///
/// Anchored on the `data:` URL specifically: a user-authored
/// `background-image:url(/photo.jpg)` in `extra_attrs` is theirs and is left
/// alone. `[^)]*` cannot cross the closing paren, so it cannot run past the end
/// of the URL, and the optional trailing `background-size` clause is what the
/// renderer emits beside it (`render/image.rs`).
static INLINE_LQIP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"background-image:url\(data:[^)]*\)\s*;?\s*(?:background-size:[^;"']*;?)?"#)
        .expect("static regex")
});

/// Strip the inline base64 LQIP from every `style=` attribute in a served page.
///
/// Preview-only by construction: this runs in the preview server's response
/// path, so the generation on disk — the bytes a publish uploads — is untouched
/// and the published site keeps its blur-up first paint. See the module doc for
/// why the two audiences want different things here.
///
/// Leaves any other declaration in the same `style=` intact (a `background-color`
/// fallback, an `object-fit` from `:::hero {attrs=…}`), and removes the
/// attribute entirely only if nothing else was in it.
pub fn strip_inline_lqip(html: &str) -> String {
    if !html.contains("background-image:url(data:") {
        return html.to_string();
    }
    let stripped = INLINE_LQIP_RE.replace_all(html, "");
    // An LQIP that was the attribute's only declaration leaves ` style=""`
    // behind; drop the empty shell rather than shipping a no-op attribute.
    stripped.replace(" style=\"\"", "")
}

pub async fn rewrite_ready_assets(
    request: Request,
    next: Next,
    registry: Arc<AssetRegistry>,
) -> Result<Response<Body>, StatusCode> {
    // Capture the request path before passing to next middleware
    let request_path = request.uri().path().to_string();

    // Get the response from the next middleware/handler
    let response = next.run(request).await;

    // Check if this is an HTML response
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("text/html"))
        .unwrap_or(false);

    // Non-HTML responses pass through unchanged
    if !is_html {
        return Ok(response);
    }

    // Extract response parts
    let (parts, body) = response.into_parts();

    // Read the body
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Convert to string
    let html = match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(_) => {
            // If not valid UTF-8, return the original bytes
            return Ok(Response::from_parts(parts, Body::from(bytes)));
        }
    };

    // Rewrite video placeholders for ready assets
    let rewritten = rewrite_video_placeholders(&html, &request_path, &registry);
    // ...and take the inline blur-up off, so the preview shows the author their
    // real photo or nothing — never a stand-in resembling it. See module doc.
    let rewritten = strip_inline_lqip(&rewritten);

    // Remove Content-Length header since we may have modified the body
    let mut parts = parts;
    parts.headers.remove(header::CONTENT_LENGTH);

    Ok(Response::from_parts(parts, Body::from(rewritten)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::assets::AssetRegistry;

    // =========================================================================
    // Inline-LQIP stripping (the SECOND LQIP channel — see module doc)
    // =========================================================================

    /// The exact shape `moss_core::render::image` emits for a source whose
    /// LQIP is known. This is the channel removing the registry field did NOT
    /// close, and it is the one an author actually sees behind a transparent
    /// stub.
    const LQIP_IMG: &str = concat!(
        r#"<picture><source srcset="/hero.webp" type="image/webp">"#,
        r#"<img src="/hero.png" width="800" height="600" "#,
        r#"style="background-image:url(data:image/jpeg;base64,/9j/4AAQSkZJRg==);background-size:cover" "#,
        r#"alt="" /></picture>"#
    );

    #[test]
    fn inline_lqip_is_stripped_from_a_served_page() {
        let out = strip_inline_lqip(LQIP_IMG);
        assert!(
            !out.contains("background-image"),
            "the blur-up must not reach the author's preview: {out}"
        );
        assert!(
            !out.contains("base64"),
            "no data-URI payload may survive: {out}"
        );
        assert!(
            !out.contains("background-size"),
            "background-size:cover exists only to scale the LQIP: {out}"
        );
        // Everything that is not the placeholder survives untouched — the
        // dimensions are what keep the layout from shifting.
        assert!(out.contains(r#"src="/hero.png""#));
        assert!(out.contains(r#"width="800""#));
        assert!(out.contains(r#"height="600""#));
        assert!(out.contains(r#"srcset="/hero.webp""#));
        assert!(
            !out.contains(r#"style="""#),
            "an emptied style= attribute should be dropped, not shipped: {out}"
        );
    }

    #[test]
    fn stripping_keeps_other_declarations_in_the_same_style_attribute() {
        // `:::hero {attrs="cover-fit=contain"}` puts object-fit in the same
        // attribute. Removing the LQIP must not remove the author's own CSS.
        let html = r#"<img src="/a.png" style="background-image:url(data:image/jpeg;base64,AAA);background-size:cover;object-fit:contain" />"#;
        let out = strip_inline_lqip(html);
        assert!(out.contains("object-fit:contain"), "got: {out}");
        assert!(!out.contains("background-image"), "got: {out}");
    }

    #[test]
    fn stripping_leaves_a_dominant_color_fallback_alone() {
        // The renderer emits EITHER an LQIP background-image OR a flat
        // background-color. A flat colour is not a stand-in an author can
        // mistake for their photograph, so it stays.
        let html = r#"<img src="/a.png" style="background-color:#3498db" />"#;
        assert_eq!(strip_inline_lqip(html), html);
    }

    #[test]
    fn stripping_leaves_a_user_authored_background_image_alone() {
        // Anchored on `data:` precisely so a real URL the author wrote is theirs.
        let html = r#"<div style="background-image:url(/paper.png)"></div>"#;
        assert_eq!(strip_inline_lqip(html), html);
    }

    #[test]
    fn a_page_with_no_lqip_is_returned_byte_for_byte() {
        let html = r#"<html><body><img src="/a.png" alt="" /></body></html>"#;
        assert_eq!(strip_inline_lqip(html), html);
    }

    // =========================================================================
    // Path resolution tests
    // =========================================================================

    #[test]
    fn test_resolve_to_root_relative_parent_traversal() {
        // request_path="/videos/冬日之歌/index.html", relative="../clip.mp4"
        // -> "videos/clip.mp4"
        let result =
            resolve_to_root_relative("/videos/冬日之歌/index.html", "../clip.mp4");
        assert_eq!(result, "videos/clip.mp4");
    }

    #[test]
    fn test_resolve_to_root_relative_simple() {
        // request_path="/index.html", relative="assets/photo.jpg"
        // -> "assets/photo.jpg"
        let result = resolve_to_root_relative("/index.html", "assets/photo.jpg");
        assert_eq!(result, "assets/photo.jpg");
    }

    #[test]
    fn test_resolve_to_root_relative_deep_traversal() {
        // request_path="/articles/游记/科隆群岛/index.html", relative="../../../assets/img.jpg"
        // -> "assets/img.jpg"
        let result = resolve_to_root_relative(
            "/articles/游记/科隆群岛/index.html",
            "../../../assets/img.jpg",
        );
        assert_eq!(result, "assets/img.jpg");
    }

    #[test]
    fn test_resolve_to_root_relative_absolute_path() {
        // Absolute paths just strip the leading slash
        let result = resolve_to_root_relative("/any/page.html", "/videos/clip.mp4");
        assert_eq!(result, "videos/clip.mp4");
    }

    #[test]
    fn test_resolve_to_root_relative_current_dir() {
        // ./file.mp4 in a subdirectory
        let result =
            resolve_to_root_relative("/videos/page/index.html", "./clip.mp4");
        assert_eq!(result, "videos/page/clip.mp4");
    }

    // =========================================================================
    // HTML rewriting tests
    // =========================================================================

    #[test]
    fn test_html_without_video_unchanged() {
        let registry = AssetRegistry::new();
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body>
<h1>Hello World</h1>
<p>No videos here.</p>
</body>
</html>"#;

        let result = rewrite_video_placeholders(html, "/index.html", &registry);
        assert_eq!(result, html, "HTML without video tags should be unchanged");
    }

    #[test]
    fn test_video_placeholder_removed_when_ready() {
        let registry = AssetRegistry::new();
        // Mark the asset as ready
        registry.set_ready("videos/clip.mp4".to_string());

        let html = r#"<html><body>
<video data-placeholder-src="../placeholder.svg" src="../clip.mp4" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(
            html,
            "/videos/冬日之歌/index.html",
            &registry,
        );

        // data-placeholder-src should be removed
        assert!(
            !result.contains("data-placeholder-src"),
            "data-placeholder-src should be removed for ready assets. Got: {}",
            result
        );
        // The video tag and src should remain
        assert!(result.contains("<video"), "video tag should remain");
        assert!(result.contains(r#"src="../clip.mp4""#), "src attribute should remain");
        assert!(result.contains("controls"), "other attributes should remain");
    }

    #[test]
    fn test_video_placeholder_kept_when_pending() {
        let registry = AssetRegistry::new();
        // Mark the asset as pending (still processing)
        registry.set_pending(
            "videos/clip.mp4".to_string(),
            Some((1920, 1080)),
            Some("#333".to_string()),
        );

        let html = r#"<html><body>
<video data-placeholder-src="../placeholder.svg" src="../clip.mp4" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(
            html,
            "/videos/冬日之歌/index.html",
            &registry,
        );

        assert!(
            result.contains("data-placeholder-src"),
            "data-placeholder-src should be kept for pending assets"
        );
        assert_eq!(result, html, "HTML should be completely unchanged for pending assets");
    }

    #[test]
    fn test_video_placeholder_kept_when_unknown() {
        let registry = AssetRegistry::new();
        // Don't register the asset at all

        let html = r#"<html><body>
<video data-placeholder-src="../placeholder.svg" src="../clip.mp4" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(
            html,
            "/videos/冬日之歌/index.html",
            &registry,
        );

        assert!(
            result.contains("data-placeholder-src"),
            "data-placeholder-src should be kept for unknown assets"
        );
        assert_eq!(result, html, "HTML should be completely unchanged for unknown assets");
    }

    #[test]
    fn test_unicode_path_decoded() {
        let registry = AssetRegistry::new();
        // Register with decoded Unicode path
        registry.set_ready("videos/冬日之歌/clip.mp4".to_string());

        // HTML uses percent-encoded path
        let html = r#"<html><body>
<video data-placeholder-src="placeholder.svg" src="%E5%86%AC%E6%97%A5%E4%B9%8B%E6%AD%8C/clip.mp4" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(html, "/videos/index.html", &registry);

        assert!(
            !result.contains("data-placeholder-src"),
            "data-placeholder-src should be removed when percent-encoded path matches decoded registry key. Got: {}",
            result
        );
    }

    #[test]
    fn test_multiple_videos_mixed_state() {
        let registry = AssetRegistry::new();
        // First video is ready, second is pending
        registry.set_ready("videos/ready.mp4".to_string());
        registry.set_pending(
            "videos/pending.mp4".to_string(),
            Some((1920, 1080)),
            None,
        );

        let html = r#"<html><body>
<video data-placeholder-src="placeholder1.svg" src="ready.mp4" controls></video>
<video data-placeholder-src="placeholder2.svg" src="pending.mp4" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(html, "/videos/index.html", &registry);

        // First video should have placeholder removed
        assert!(
            result.contains(r#"<video src="ready.mp4" controls>"#),
            "Ready video should have data-placeholder-src removed. Got: {}",
            result
        );
        // Second video should keep placeholder
        assert!(
            result.contains(r#"data-placeholder-src="placeholder2.svg""#),
            "Pending video should keep data-placeholder-src. Got: {}",
            result
        );
    }

    #[test]
    fn test_video_without_src_unchanged() {
        let registry = AssetRegistry::new();

        // A video tag with data-placeholder-src but no src attribute
        let html = r#"<html><body>
<video data-placeholder-src="placeholder.svg" controls></video>
</body></html>"#;

        let result = rewrite_video_placeholders(html, "/index.html", &registry);

        assert_eq!(result, html, "Video without src should be unchanged");
    }

    #[test]
    fn test_non_video_html_elements_unchanged() {
        let registry = AssetRegistry::new();
        registry.set_ready("img/photo.jpg".to_string());

        // Ensure other elements with data attributes are not touched
        let html = r#"<html><body>
<img data-placeholder-src="placeholder.svg" src="img/photo.jpg">
<div data-placeholder-src="something"></div>
</body></html>"#;

        let result = rewrite_video_placeholders(html, "/index.html", &registry);

        assert_eq!(result, html, "Non-video elements should be unchanged");
    }
}
