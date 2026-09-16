//! QR code generation for article pages.
//!
//! Generates SVG QR codes that encode article URLs, written to the site's
//! `qr/` directory during build.
//!
//! ## One page, one answer
//!
//! [`share_qr_for_page`] is the only place that decides whether a page has a QR
//! code and where that code lives. Two callers ask it the same question in the
//! same build: `render/blocking.rs` writes the file, and `render/html.rs`
//! publishes the answer to the reader as `data-share-qr` on
//! `<article class="container">`.
//!
//! It used to be three places and two languages. The build decided *which*
//! pages got a code (an inline `PageKind::Folder` skip in the render loop, which
//! silently excluded every folder note — a full article with a share button),
//! the build named the file, and `share-card.ts` re-derived that same filename
//! at read time from `location.pathname` — a mirror the doc comments had to ask
//! future editors to keep "byte-for-byte" identical. A silent 404 was the
//! failure mode, and a missing QR looks exactly like a site that never had one,
//! so both halves of the split shipped broken to real sites at once: every
//! folder-note article on one site had no code at all, while another served
//! fourteen codes pointing at the wrong page.
//!
//! The reader no longer computes anything. See
//! `docs/archive/2026-08-10-share-card-audit.md`.

use fast_qr::convert::svg::SvgBuilder;
use fast_qr::convert::{Builder, Shape};
use fast_qr::qr::QRBuilder;
use fast_qr::ECL;

use crate::build::site_url::SiteUrl;

/// A page's QR code: where the SVG is served from, and what it encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareQr {
    /// Output key, e.g. `qr/about.svg`. Root-relative served URL is `/{path}`.
    pub served_path: String,
    /// The page's own canonical URL, scheme included.
    ///
    /// This is the same string as its `<link rel="canonical">` on every page
    /// but one: a linkblog entry declares the *external* source as canonical,
    /// while its code still points at moss's copy — the page the reader is
    /// holding, and the only one the card's other half describes.
    pub payload: String,
}

/// The QR code for a page, or `None` when this page has none.
///
/// A page has one when all three hold:
///
/// - **The site has a real URL.** `is_deployed()` is false for the localhost
///   fallback a never-published site builds against, and a code encoding
///   `http://localhost:1420/...` is worse than no code. This is why an
///   unpublished site's preview cards carry no QR — a stated rule now, rather
///   than an emergent consequence of the feed gate the loop used to sit under.
/// - **The page is public.** `is_public_page` — drafts and slot files are out;
///   `listed: false` pages are in, because they are reachable and shareable.
/// - **Nothing else.** In particular *not* the page's `PageKind`. A folder note
///   renders the same article chrome, the same selection popover and the same
///   Share button as any other page, so excluding it produced a card that was
///   missing a piece for no reason the reader could see.
pub fn share_qr_for_page(
    url_path: &str,
    is_public_page: bool,
    site_url: &SiteUrl,
) -> Option<ShareQr> {
    if !is_public_page || !site_url.is_deployed() {
        return None;
    }
    let payload =
        crate::build::page::canonical::canonical_for_url_path(site_url, url_path).ok()?;
    // The served path is resolved here, once, in the form the emitter writes —
    // `ServedPath::from_source` slugifies directory segments, so rendering the
    // attribute from the raw key would name a file that differs from the one on
    // disk the moment a url override carries an uppercase segment.
    let served_path = crate::build::served_path::ServedPath::from_source(&qr_key_for_url_path(
        url_path,
    ))
    .ok()?
    .to_relative_url()
    .trim_start_matches('/')
    .to_string();
    Some(ShareQr {
        served_path,
        payload,
    })
}

/// Output key for a page's QR SVG, derived from the page's **whole** URL path.
///
/// The key must be unique per page, because the QR image encodes that page's
/// own URL. Deriving it from the last path segment alone collides the moment
/// two pages share a leaf name — which every multilingual site does by
/// construction (`about/index.html` and `zh-hans/about/index.html` both end in
/// `about`). Whichever page rendered last won the file, so one language's card
/// carried the other language's QR code, and which one won depended on render
/// order.
///
/// Mirroring the URL path keeps the key unique for arbitrary nesting and
/// language folders without a hash: `url_path` is the page's output file path,
/// so two pages can no more share a key than they can share an `index.html`.
///
/// This is now a *private* naming scheme: the reader is told the finished path
/// on `data-share-qr` and never derives it. `share-card.ts` used to mirror this
/// function character for character (`qrKeyForPathname`), which meant any
/// disagreement — a slug override, a language folder, a stale build — 404'd in
/// silence. That function is gone; this one is free to change shape as long as
/// the writer and [`share_qr_for_page`] agree, which they do by construction.
///
/// The scheme: drop empty segments, drop a trailing `index.html` (the
/// directory-index page is one page however it is spelled), and fall back to
/// `index` when nothing is left (the site root).
///
/// ```text
/// about/index.html          → qr/about.svg
/// zh-hans/about/index.html  → qr/zh-hans/about.svg
/// interactive/particle.html → qr/interactive/particle.html.svg
/// index.html                → qr/index.svg
/// ```
pub fn qr_key_for_url_path(url_path: &str) -> String {
    let mut segments: Vec<&str> =
        url_path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.last() == Some(&"index.html") {
        segments.pop();
    }
    if segments.is_empty() {
        // The site root. A folder actually named `index/` would land on the
        // same name, so it keeps a segment of its own rather than quietly
        // overwriting the homepage's code with its own.
        return "qr/index.svg".to_string();
    }
    if segments == ["index"] {
        return "qr/index/index.svg".to_string();
    }
    format!("qr/{}.svg", segments.join("/"))
}

/// The `data-share-qr` attribute for a page, or an empty string when it has no
/// code. This is what the reader reads; [`emit_share_qr`] writes what it points
/// at, and both derive it from [`share_qr_for_page`], so the attribute cannot
/// name a file the build did not write.
pub fn share_qr_attr(url_path: &str, is_public_page: bool, site_url: &SiteUrl) -> String {
    match share_qr_for_page(url_path, is_public_page, site_url) {
        Some(qr) => format!(
            r#" data-share-qr="/{}""#,
            crate::build::page::meta::escape_html_attr(&qr.served_path)
        ),
        None => String::new(),
    }
}

/// Writes every page's QR code — the whole site's worth, in one call, so that
/// which pages are reached is decided next to which pages qualify.
///
/// The synthetic homepage is the case that needs saying out loud: a site with
/// no `index.md` still has a homepage, built from the article list, and it is
/// rendered without a document. It is not in `documents`, so a loop over them
/// alone leaves the most-shared page on the site with an empty card corner —
/// while `render/html.rs`, which writes the attribute from the same synthetic
/// path, promises a code that was never written.
pub fn emit_share_qrs(
    documents: &[crate::build::types::ParsedDocument],
    site_url: &SiteUrl,
    output_dir: &std::path::Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<(), String> {
    for doc in documents {
        emit_share_qr(
            &doc.url_path,
            doc.is_public_page(),
            site_url,
            output_dir,
            pending,
        )?;
    }
    if !documents.is_empty() && !documents.iter().any(|d| d.url_path == "index.html") {
        emit_share_qr("index.html", true, site_url, output_dir, pending)?;
    }
    Ok(())
}

/// Writes a page's QR code, if it has one. A generation failure is logged and
/// skipped: a card without a corner code is a smaller loss than a failed build.
pub fn emit_share_qr(
    url_path: &str,
    is_public_page: bool,
    site_url: &SiteUrl,
    output_dir: &std::path::Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<(), String> {
    let Some(qr) = share_qr_for_page(url_path, is_public_page, site_url) else {
        return Ok(());
    };
    let svg = match generate_qr_svg(&qr.payload) {
        Ok(svg) => svg,
        Err(e) => {
            log::warn!("QR generation failed for {}: {}", url_path, e);
            return Ok(());
        }
    };
    let sp = crate::build::served_path::ServedPath::from_source(&qr.served_path)
        .map_err(|e| format!("Failed to construct QR path: {}", e))?;
    crate::build::context::BuildContext::for_render(output_dir, pending)
        .emit(&sp, svg.as_bytes(), crate::build::manifest::HashBucket::Files)
        .map_err(|e| format!("Failed to emit QR SVG: {}", e))
}

/// Generates an SVG string containing a QR code for the given URL.
///
/// Uses error correction level M (15% recovery) and rounded-square modules.
/// Logs a warning if the resulting QR version exceeds 10 (matrix > 57 modules),
/// which indicates the URL may be too long for comfortable scanning.
///
/// # Errors
/// Returns `Err` if the input is empty or QR encoding fails.
pub fn generate_qr_svg(url: &str) -> Result<String, String> {
    if url.is_empty() {
        return Err("Cannot generate QR code for empty URL".to_string());
    }

    let qrcode = QRBuilder::new(url)
        .ecl(ECL::M)
        .build()
        .map_err(|e| format!("QR encoding failed: {e}"))?;

    // QR version > 10 means matrix > 57 modules — warn about scan reliability
    if let Some(version) = qrcode.version {
        if version as u8 > 9 {
            // V01=0 .. V10=9, so discriminant > 9 means version > 10
            log::warn!(
                "QR version {:?} (matrix {}×{}) for URL may be hard to scan: {}",
                version,
                qrcode.size,
                qrcode.size,
                url
            );
        }
    }

    // RoundedSquare, as the doc above always promised — Circle modules only
    // cover ~78% of their cell, and at the share card's 56pt (v7 codes for
    // percent-encoded CJK slugs ≈ 1.9px/module) they rasterize into
    // disconnected dots that scanners misread.
    let svg = SvgBuilder::default()
        .shape(Shape::RoundedSquare)
        .to_str(&qrcode);

    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_valid_svg_for_url() {
        let svg = generate_qr_svg("https://example.com/posts/hello").unwrap();
        assert!(svg.contains("<svg"), "SVG output must contain <svg tag");
        assert!(svg.contains("</svg>"), "SVG output must contain closing </svg> tag");
    }

    #[test]
    fn svg_contains_expected_tags() {
        let svg = generate_qr_svg("https://example.com").unwrap();
        assert!(svg.contains("<svg"), "must start with <svg");
        assert!(svg.contains("</svg>"), "must end with </svg>");
        // SVG should contain path or rect elements for the QR modules
        assert!(
            svg.contains("<rect") || svg.contains("<path") || svg.contains("<circle"),
            "SVG should contain drawing elements"
        );
    }

    #[test]
    fn handles_long_urls_without_panicking() {
        // A very long URL that will push the QR version high
        let long_path = "a".repeat(500);
        let long_url = format!("https://example.com/{}", long_path);
        let result = generate_qr_svg(&long_url);
        // Should either succeed or return a clean error, never panic
        match result {
            Ok(svg) => {
                assert!(svg.contains("<svg"));
                assert!(svg.contains("</svg>"));
            }
            Err(e) => {
                assert!(!e.is_empty(), "Error message should not be empty");
            }
        }
    }

    #[test]
    fn svg_carries_the_colors_the_share_card_recolors() {
        // share-card.ts (loadQrImage) recolors the SVG by regex on exactly
        // these attribute strings — module paths fill="#000000", background
        // rect fill="#ffffff". If a fast_qr upgrade changes its output, this
        // must fail here before the card silently ships un-recolored codes.
        let svg = generate_qr_svg("https://example.com/post").unwrap();
        assert!(
            svg.contains(r##"fill="#000000""##),
            "module fill literal changed — update share-card.ts recolor regex"
        );
        assert!(
            svg.contains(r##"fill="#ffffff""##),
            "background fill literal changed — update share-card.ts recolor regex"
        );
    }

    #[test]
    fn qr_key_mirrors_the_whole_url_path() {
        assert_eq!(qr_key_for_url_path("about/index.html"), "qr/about.svg");
        assert_eq!(
            qr_key_for_url_path("zh-hans/about/index.html"),
            "qr/zh-hans/about.svg"
        );
        assert_eq!(
            qr_key_for_url_path("writings/2024/deep/nest/index.html"),
            "qr/writings/2024/deep/nest.svg"
        );
        // A non-index page keeps its file name — the reader sees the same
        // trailing segment in `location.pathname`.
        assert_eq!(
            qr_key_for_url_path("interactive/particle.html"),
            "qr/interactive/particle.html.svg"
        );
        // Site root — never `qr/.svg`.
        assert_eq!(qr_key_for_url_path("index.html"), "qr/index.svg");
        assert_eq!(qr_key_for_url_path(""), "qr/index.svg");
        assert_eq!(qr_key_for_url_path("/"), "qr/index.svg");
    }

    #[test]
    fn a_folder_named_index_does_not_take_the_homepage_s_code() {
        assert_ne!(
            qr_key_for_url_path("index.html"),
            qr_key_for_url_path("index/index.html")
        );
    }

    #[test]
    fn qr_key_does_not_collide_across_language_editions() {
        // The regression this scheme exists for: the two editions of the same
        // page share a leaf name and previously mapped to one file.
        assert_ne!(
            qr_key_for_url_path("about/index.html"),
            qr_key_for_url_path("zh-hans/about/index.html")
        );
    }

    fn deployed() -> SiteUrl {
        SiteUrl::parse("https://example.com").unwrap()
    }

    #[test]
    fn a_public_page_gets_a_code_encoding_its_canonical_url() {
        let qr = share_qr_for_page("posts/a/index.html", true, &deployed()).unwrap();
        assert_eq!(qr.served_path, "qr/posts/a.svg");
        // Canonical — not `{site}/{url_path}`, which appended `/index.html`.
        assert_eq!(qr.payload, "https://example.com/posts/a/");
    }

    #[test]
    fn a_folder_note_gets_a_code_like_any_other_article() {
        // The regression this function exists for. A folder note renders the
        // full article chrome and the Share button; the render loop's
        // `PageKind::Folder` skip left every one of them with a card that had
        // an empty corner. Nothing about the page kind reaches this decision
        // any more — there is no parameter for it to arrive on.
        let qr = share_qr_for_page("awards/s4/ukraine/index.html", true, &deployed()).unwrap();
        assert_eq!(qr.served_path, "qr/awards/s4/ukraine.svg");
        assert_eq!(qr.payload, "https://example.com/awards/s4/ukraine/");
    }

    #[test]
    fn the_homepage_gets_one_too_and_it_points_at_the_root() {
        let qr = share_qr_for_page("index.html", true, &deployed()).unwrap();
        assert_eq!(qr.served_path, "qr/index.svg");
        assert_eq!(qr.payload, "https://example.com/");
    }

    #[test]
    fn an_onion_site_gets_a_code_a_tor_browser_can_open() {
        let onion = SiteUrl::parse("http://abcdefghij.onion").unwrap();
        let qr = share_qr_for_page("posts/a/index.html", true, &onion).unwrap();
        assert_eq!(qr.payload, "http://abcdefghij.onion/posts/a/");
    }

    #[test]
    fn a_draft_or_slot_file_gets_none() {
        assert_eq!(share_qr_for_page("posts/a/index.html", false, &deployed()), None);
    }

    #[test]
    fn an_unpublished_site_gets_none_rather_than_a_localhost_code() {
        let local = SiteUrl::parse("http://localhost:1420").unwrap();
        assert_eq!(share_qr_for_page("posts/a/index.html", true, &local), None);
    }

    #[test]
    fn returns_error_for_empty_input() {
        let result = generate_qr_svg("");
        assert!(result.is_err(), "Empty input should produce an error");
        let err = result.unwrap_err();
        assert!(
            err.contains("empty"),
            "Error message should mention empty input, got: {err}"
        );
    }
}
