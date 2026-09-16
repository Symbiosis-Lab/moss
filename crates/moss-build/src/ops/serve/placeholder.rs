//! Placeholder SVG generation and asset request handling for the preview server.
//!
//! When assets are still being processed (e.g. video transcoding, image optimization),
//! the preview server can serve SVG placeholders instead of 404s. This module provides
//! the placeholder generation logic and the request handler that decides whether to
//! serve a placeholder or fall through to `ServeDir`.

use axum::{body::Body, response::Response};

// ADR-002: Step 4 - Preview server with placeholder support

/// Generate an SVG placeholder for a pending asset.
pub fn generate_placeholder_svg(dims: Option<(u32, u32)>, color: Option<String>) -> String {
    let (w, h) = dims.unwrap_or((800, 600));
    let fill = color.unwrap_or_else(|| "#E8E8E8".to_string());
    format!(
        r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg"><rect width="100%" height="100%" fill="{}"/></svg>"#,
        w, h, fill
    )
}

/// Tiny 1×1 transparent webp embedded as a constant. The last-resort,
/// never-404 fallback body for a Pending image variant whose source
/// passthrough could not be served (unregistered, or the source read failed —
/// e.g. an iCloud-dataless original). 50 bytes; the smallest valid webp the
/// encoder can produce. A chosen `<source>` must never 404 (ADR-013).
const TRANSPARENT_WEBP_1X1: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x2a, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
    0x56, 0x50, 0x38, 0x4c, 0x1d, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x80,
    0x10, 0x00,
];

/// Build the 1×1 transparent-webp stub response. The router calls this as the
/// graceful fallback when a registered source-passthrough hand-off fails
/// (missing/dataless/unreadable source) so a chosen `<source>` never 404s.
pub fn transparent_stub_response() -> Response<Body> {
    use axum::http::{header, StatusCode};
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .body(Body::from(TRANSPARENT_WEBP_1X1.to_vec()))
        .unwrap()
}

/// Returns `true` when the request path looks like an image variant URL
/// (`.webp` / `.avif`) — these fall back to the transparent-webp stub, while
/// a Pending non-image (video) URL uses the SVG-with-rect placeholder.
fn is_image_variant_url(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".webp") || lower.ends_with(".avif")
}

/// Build the warning-SVG response body for a Failed asset. The user only
/// sees this in the preview when encoding has terminally failed (e.g.,
/// corrupt source); the inline error message helps them debug.
///
/// Truncates the error to 80 chars on a character (codepoint) boundary,
/// not a byte boundary. Plain `&error[..80]` panics if byte 80 falls inside
/// a multi-byte UTF-8 codepoint (e.g., a CJK path in the source file name).
/// `chars().take(80)` walks codepoints and never splits one.
///
/// Visual: a static blueprint-grid pattern (the same motif as
/// `frontend/app/components/blueprint-grid.ts`'s animated canvas background
/// and the `.moss-img-fallback` CSS class in `site.css`, both grid lines in
/// blueprint blue `#143c82`) rather than a flat pink rect — this is moss's
/// own "asset failed" state, not a generic missing-image placeholder, so it
/// keeps a legible warning message in a distinguishing red, layered over the
/// grid. The pattern is a single small `<path>` tiled by the SVG renderer's
/// `<pattern>` element — no per-request cost beyond the flat-rect it replaces.
fn generate_failed_svg(error: &str) -> String {
    let short_chars: String = error.chars().take(80).collect();
    let short = if short_chars.chars().count() < error.chars().count() {
        format!("{}…", short_chars)
    } else {
        short_chars
    };
    let escaped = short
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600"><defs><pattern id="moss-grid" width="18" height="18" patternUnits="userSpaceOnUse"><path d="M18 0H0V18" fill="none" stroke="#143c82" stroke-opacity="0.16"/></pattern></defs><rect width="100%" height="100%" fill="#f4f1ec"/><rect width="100%" height="100%" fill="url(#moss-grid)"/><g transform="translate(400 280)"><text text-anchor="middle" font-family="system-ui,sans-serif" font-size="28" fill="#a33">⚠ asset failed</text><text y="42" text-anchor="middle" font-family="system-ui,sans-serif" font-size="15" fill="#7a2b2b">{}</text></g></svg>"##,
        escaped
    )
}

/// Handle an asset request with placeholder support.
///
/// Returns `Some(Response)` only when the server should respond from the
/// AssetRegistry's promised state instead of falling through to `ServeDir`.
/// Returns `None` for all other cases — `ServeDir` then handles the request
/// with proper Range support (critical for video playback) and correct
/// Content-Length headers.
///
/// Two techniques, two audiences (the rule the precedence order encodes):
///
/// - **Preview / build → the source passthrough.** The audience is the AUTHOR,
///   who is looking at the picture to judge it. They get the real original file,
///   full size, sharp. A blurred stand-in here is worse than useless: it looks
///   like the photo, so a bad encode and a placeholder are indistinguishable.
/// - **Published site → the LQIP.** The audience is a VISITOR on a network, and
///   a blurred first paint is a genuine improvement over empty space. The LQIP
///   is baked into the emitted HTML by `moss_core::render::image` and is never
///   served by this server.
///
/// So the SOURCE PASSTHROUGH lives in the router (`router.rs`) and runs BEFORE
/// this handler. For a Pending variant URL whose encoded output is not yet on
/// disk, the router serves the FULL ORIGINAL source bytes (Range-aware via
/// `ServeFile`). It reads SOURCE bytes only for registered/promised originals,
/// which `copy_deferred_assets` copies into staging → generation → deploy, so
/// preview never surfaces bytes the deploy won't reproduce (honest-mirror
/// preserved; ADR-022 `moss-source://` is the editor analog). This handler is
/// therefore the RESIDUAL fallback — reached when the original itself cannot be
/// read (iCloud-dataless, deleted mid-build) or was never registered.
///
/// Response shape per state:
/// - File exists on disk → `None` (let ServeDir handle it).
/// - `Pending` + image variant URL (`.webp`/`.avif`) → 200 with the 1×1
///   transparent-webp stub. Deliberately something the author can NEVER mistake
///   for their own image; it exists only so a chosen `<source>` never 404s
///   (ADR-013). An LQIP is never served here — see the two-audiences rule above.
/// - `Pending` + other URL → 200 with SVG placeholder (video path — the
///   derived poster `to_thumb` has no source equivalent, so it stays a brief
///   gray poster until playback loads the passthrough original).
/// - `Failed` → 200 with warning SVG (annotated with the error message). The
///   router does NOT passthrough a Failed variant, so the encode failure stays
///   visible.
/// - Unknown (not registered) → `None` (ServeDir → 404).
///
/// All placeholder/warning responses carry `Cache-Control: no-cache,
/// no-store, must-revalidate` (RFC 9111 § 5.2.2.5) so the browser refetches
/// when the iframe-bridge sideband signals the asset is ready and mutates
/// the matching `<source srcset>` to trigger fresh source-set selection
/// (HTML spec § reacting-to-dom-mutations).
///
/// Pattern: explicit promise model. See
/// `docs/archive/2026-05-20-image-variant-honest-mirror.md` (Layer 1).
pub fn handle_asset_request(
    request_path: &str,
    asset_registry: &crate::types::assets::AssetRegistry,
    site_dir: &std::path::Path,
) -> Option<Response<Body>> {
    use crate::types::assets::AssetState;
    use axum::http::{header, StatusCode};

    // URL-decode the request path so a CJK/Unicode variant URL
    // (`%E5%86%AC.webp`) matches the on-disk file AND the AssetRegistry key —
    // both are decoded raw UTF-8 (registry keys come from source file paths;
    // ServeDir percent-decodes before hitting disk). Mirrors the decode in
    // `preview::asset_rewriter`. Without it, non-ASCII image-variant
    // placeholders MISS the registry here and fall through to a ServeDir 404
    // during the Pending/Failed window — the exact non-recoverable
    // `<picture><source>` 404 the promise model (ADR-013) exists to prevent.
    let encoded = request_path.trim_start_matches('/');
    let decoded = urlencoding::decode(encoded)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| encoded.to_string());
    let normalized_path = decoded.as_str();

    // Honest-mirror invariant: the preview server only reads from the served
    // root (site/ or its stage swap). Never from CAS. Production has no CAS,
    // so a preview that surfaced CAS-only bytes would mask deploys that
    // would 404 in production. See design § "Honest-mirror invariant."
    if site_dir.join(normalized_path).exists() {
        return None;
    }

    match asset_registry.get(normalized_path) {
        Some(AssetState::Pending(placeholder)) => {
            if is_image_variant_url(normalized_path) {
                // Image variant last resort: the router's source passthrough
                // serves the real original for a Pending variant before this
                // runs. We only reach here when the original could not be read
                // or was never registered; return the 1×1 transparent-webp stub
                // so a chosen <source> never 404s (ADR-013). Nothing that
                // resembles the author's image is ever served in its place.
                // `placeholder`'s dimensions/color are the VIDEO branch's
                // material; an image variant needs none of it.
                Some(transparent_stub_response())
            } else {
                // Non-image variant (currently videos): existing SVG path.
                let svg = generate_placeholder_svg(
                    placeholder.dimensions,
                    placeholder.dominant_color,
                );
                Some(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "image/svg+xml")
                        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
                        .body(Body::from(svg))
                        .unwrap(),
                )
            }
        }
        Some(AssetState::Failed(error)) => {
            // Terminal failure: serve a visible warning so the user sees
            // the encoding failure rather than a perpetual LQIP. The
            // previous "warn-and-return-Ok" pattern silently swallowed
            // these (verified at ~/Library/Logs/host.moss.publisher/
            // moss.log L2006/L2446/L2450 on 2026-05-19); this surfaces them.
            let svg = generate_failed_svg(&error);
            Some(
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/svg+xml")
                    .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
                    .body(Body::from(svg))
                    .unwrap(),
            )
        }
        // Ready or unknown: let ServeDir handle it with proper Range support.
        // Unknown → 404 (the URL was never promised — likely a typo or stale link).
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_placeholder_svg_with_dims_and_color() {
        let svg = generate_placeholder_svg(Some((800, 600)), Some("#3498DB".to_string()));
        assert!(svg.contains(r#"width="800""#));
        assert!(svg.contains(r#"height="600""#));
        assert!(svg.contains("fill="));
    }

    #[test]
    fn test_generate_placeholder_svg_defaults() {
        let svg = generate_placeholder_svg(None, None);
        assert!(svg.contains(r#"width="800""#));
        assert!(svg.contains(r#"height="600""#));
        assert!(svg.contains("fill="));
    }

    // ---- generate_failed_svg: blueprint-grid visual + message handling ----

    #[test]
    fn generate_failed_svg_uses_blueprint_grid_pattern() {
        // Reskinned to the same blueprint-blue grid motif as
        // blueprint-grid.ts / .moss-img-fallback, not a flat pink rect.
        let svg = generate_failed_svg("decode error");
        assert!(
            svg.contains("<pattern") && svg.contains("#143c82"),
            "expected a blueprint-grid pattern def, got: {}",
            svg
        );
        assert!(
            !svg.contains("#fbeaea"),
            "old flat pink background should be gone, got: {}",
            svg
        );
    }

    #[test]
    fn generate_failed_svg_includes_escaped_message() {
        let svg = generate_failed_svg(r#"bad <tag> & "quote""#);
        assert!(
            svg.contains("bad &lt;tag&gt; &amp; \"quote\""),
            "error text must be HTML-escaped (&, < and >), got: {}",
            svg
        );
        assert!(svg.contains("asset failed"));
    }

    #[test]
    fn generate_failed_svg_truncates_long_error_on_char_boundary() {
        // Regression guard for the documented-but-previously-untested
        // truncation behavior: `chars().take(80)` must walk codepoints, not
        // bytes, so an 80-char cut through a multi-byte CJK run never panics
        // and never splits a codepoint.
        let long_cjk = "冬".repeat(100); // 100 codepoints, 3 bytes each in UTF-8
        let svg = generate_failed_svg(&long_cjk);
        let expected: String = "冬".repeat(80);
        assert!(
            svg.contains(&expected),
            "expected 80 truncated CJK chars, got: {}",
            svg
        );
        assert!(svg.contains('…'), "truncated message should end with an ellipsis");
    }

    #[test]
    fn generate_failed_svg_short_message_has_no_ellipsis() {
        let svg = generate_failed_svg("short error");
        assert!(svg.contains("short error"));
        assert!(
            !svg.contains('…'),
            "message under 80 chars must not be truncated"
        );
    }

    #[test]
    fn test_handle_asset_request_pending_returns_svg() {
        use crate::types::assets::AssetRegistry;
        use axum::http::{header, StatusCode};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_pending("img/photo.jpg".to_string(), Some((1920, 1080)), Some("#3498DB".to_string()));

        let response = handle_asset_request("/img/photo.jpg", &registry, temp_dir.path());
        assert!(response.is_some());
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), "image/svg+xml");
    }

    #[test]
    fn test_handle_asset_request_unknown_returns_none() {
        use crate::types::assets::AssetRegistry;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        let response = handle_asset_request("/unknown.jpg", &registry, temp_dir.path());
        assert!(response.is_none());
    }

    #[test]
    fn test_handle_asset_request_file_on_disk_bypasses_pending() {
        // When a video file exists on disk, handle_asset_request should return
        // None even if the AssetRegistry has it as Pending. This lets ServeDir
        // serve the real file with Range request support (critical for video).
        //
        // This catches the race where registry.clear() + set_pending() during a
        // rebuild marks already-converted videos as Pending.
        use crate::types::assets::AssetRegistry;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        // Create the file on disk
        std::fs::create_dir_all(temp_dir.path().join("videos")).unwrap();
        std::fs::write(temp_dir.path().join("videos/clip.mp4"), b"fake mp4 data").unwrap();

        let registry = AssetRegistry::new();
        registry.set_pending("videos/clip.mp4".to_string(), Some((1920, 1080)), None);

        let response = handle_asset_request("/videos/clip.mp4", &registry, temp_dir.path());
        assert!(
            response.is_none(),
            "Should return None when file exists on disk, letting ServeDir handle it"
        );
    }

    #[test]
    fn test_handle_asset_request_pending_served_when_file_missing() {
        // When the file does NOT exist on disk and registry has it as Pending,
        // serve the SVG placeholder. This is the normal case during initial
        // video conversion.
        use crate::types::assets::AssetRegistry;
        use axum::http::{header, StatusCode};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        // Do NOT create the file on disk

        let registry = AssetRegistry::new();
        registry.set_pending("videos/clip.mp4".to_string(), Some((1920, 1080)), None);

        let response = handle_asset_request("/videos/clip.mp4", &registry, temp_dir.path());
        assert!(response.is_some(), "Should serve placeholder when file missing from disk");
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/svg+xml"
        );
    }

    #[test]
    fn test_handle_asset_request_pending_cjk_path_decodes() {
        // Regression: a CJK/Unicode image-variant URL arrives percent-encoded
        // (冬.webp → %E5%86%AC.webp), but the AssetRegistry is keyed by decoded
        // raw UTF-8. Without decoding the request path, the registry lookup
        // misses and the placeholder falls through to a 404 during the pending
        // window. The request path must be decoded before the registry lookup.
        use crate::types::assets::AssetRegistry;
        use axum::http::{header, StatusCode};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        // Registry key is the DECODED path, as set from the source file path.
        registry.set_pending("images/冬.webp".to_string(), Some((1920, 1080)), Some("#3498DB".to_string()));

        // Browser requests the percent-ENCODED variant URL.
        let response = handle_asset_request("/images/%E5%86%AC.webp", &registry, temp_dir.path());
        assert!(
            response.is_some(),
            "CJK image-variant placeholder must be served (request path decoded before registry lookup)"
        );
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // .webp is an image variant → image bytes (1×1 webp stub when no LQIP).
        assert_eq!(response.headers().get(header::CONTENT_TYPE).unwrap(), "image/webp");
    }

    #[test]
    fn test_handle_asset_request_ready_returns_none() {
        // When registry has Ready state, return None to let ServeDir handle it.
        // ServeDir supports Range requests and Content-Length which are critical
        // for video playback. The old code used fs::read() to load the entire
        // file into memory without Range support.
        use crate::types::assets::AssetRegistry;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_ready("videos/clip.mp4".to_string());

        let response = handle_asset_request("/videos/clip.mp4", &registry, temp_dir.path());
        assert!(
            response.is_none(),
            "Ready state should return None, letting ServeDir handle the file"
        );
    }

    // ---- Image variant tests (Bug 1: source-passthrough is the sharp path;
    // this handler is the last-resort never-404 stub) ----

    #[test]
    fn pending_variant_without_passthrough_source_falls_back_to_transparent_stub_not_404() {
        // LQIP-at-URL serving is retired: `AssetPlaceholder` no longer carries
        // an LQIP at all, so this handler CANNOT serve one. A Pending image
        // variant that reaches it (the router's source passthrough did not
        // serve the original) returns the 1×1 transparent-webp stub — never
        // None/404 — so a chosen <source> never 404s (ADR-013).
        use crate::types::assets::AssetRegistry;
        use axum::http::{header, StatusCode};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_pending(
            "assets/header.webp".to_string(),
            Some((2554, 1048)),
            Some("#3498DB".to_string()),
        );

        let response = handle_asset_request("/assets/header.webp", &registry, temp_dir.path());
        assert!(response.is_some(), "webp + Pending must never 404");
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/webp",
            "Pending image variant falls back to the transparent-webp stub (no LQIP JPEG)"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache, no-store, must-revalidate",
            "Stub must not be cached (browser refetches on asset-ready)"
        );
    }

    #[test]
    fn test_handle_asset_request_avif_pending_treated_as_image_variant() {
        // The image-variant matcher must catch .avif too, not just .webp →
        // transparent-webp stub (image path), not the SVG (video path).
        use crate::types::assets::AssetRegistry;
        use axum::http::header;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_pending("assets/header.avif".to_string(), None, None);

        let response = handle_asset_request("/assets/header.avif", &registry, temp_dir.path()).unwrap();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/webp",
            "AVIF variant must take the image-variant stub path, not the SVG path"
        );
    }

    #[test]
    fn test_handle_asset_request_failed_returns_warning_svg() {
        // Failed state → 200 with a visible warning SVG annotated with the
        // error message. Surfaces terminal encoding failures rather than
        // perpetually serving LQIP (the silent-warn trap fixed in this work).
        use crate::types::assets::AssetRegistry;
        use axum::http::{header, StatusCode};
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_failed(
            "assets/corrupt.webp".to_string(),
            "decode error: invalid PNG signature".to_string(),
        );

        let response = handle_asset_request("/assets/corrupt.webp", &registry, temp_dir.path()).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/svg+xml",
            "Failed responses are SVG so users see a visible warning"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache, no-store, must-revalidate",
        );
    }

    #[test]
    fn test_handle_asset_request_path_normalization_strips_leading_slash() {
        // Registry key is `assets/header.webp` (no leading slash; this is
        // what `to_webp(mapped_source)` produces in blocking.rs). The
        // request path comes in as `/assets/header.webp` from the HTTP layer.
        // `normalized_path = trim_start_matches('/')` must align them.
        use crate::types::assets::AssetRegistry;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let registry = AssetRegistry::new();
        registry.set_pending(
            "assets/header.webp".to_string(), // no leading slash, as emitted
            None,
            None,
        );

        let response =
            handle_asset_request("/assets/header.webp", &registry, temp_dir.path()); // leading slash
        assert!(
            response.is_some(),
            "path normalization must match registry key to request URL"
        );
    }
}
