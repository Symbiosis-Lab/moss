//! Media collection support for photography, videos, and interactive experiments.
//!
//! This module handles:
//! - Extracting media metadata from HTML comment markers
//! - Aggregating media items into collections
//! - Generating collection pages with grid layouts

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::build::media::cover::html_escape;

/// Media types supported by moss collection pages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaType {
    Photography,
    Video,
    Interactive,
}

/// Metadata extracted from HTML comment markers.
///
/// Represents a single media item that should appear in a collection page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    /// Type of media (photography, video, or interactive)
    pub media_type: MediaType,
    /// Path to the media file (relative to source)
    pub source_path: String,
    /// Display title (from alt text, figcaption, or marker)
    pub title: String,
    /// Optional collection/category name for grouping
    pub collection: Option<String>,
    /// URL path of the containing article
    pub article_url: String,
    /// Anchor ID for deep linking to media in article
    pub anchor_id: String,
}

/// Extracts media metadata from HTML content with comment markers.
///
/// # Purpose
///
/// This function extracts media items that should appear in **gallery grids**.
/// It requires HTML comment markers to identify which media should be included.
///
/// **Note:** Video conversion is handled separately by `collect_videos_for_conversion()`,
/// which automatically converts all MOV files without requiring markers.
///
/// # Supported Patterns
///
/// Photography:
/// ```html
/// <figure>...<img src="path">...</figure>
/// <!-- photography -->
/// ```
///
/// Video (for gallery display):
/// ```html
/// <p><img src="...video.mov" alt="Title"></p>
/// <!-- video-meta: collection_name -->
/// ```
///
/// Interactive:
/// ```html
/// <iframe src="..."></iframe>
/// <!-- interactive-meta: collection | title: Demo Name -->
/// ```
pub fn extract_media_items(html: &str, article_url: &str, lang: crate::i18n::Language) -> Vec<MediaItem> {
    let mut items = Vec::new();
    let mut anchor_counter = 0;

    // Pattern 1: Photography - figure or img followed by <!-- photography -->
    let photo_pattern = Regex::new(
        r#"(?s)(<figure[^>]*>.*?<img[^>]+src="([^"]+)"[^>]*>.*?</figure>|<p>\s*<img[^>]+src="([^"]+)"[^>]*>\s*</p>)\s*<!--\s*photography\s*-->"#
    ).unwrap();

    for caps in photo_pattern.captures_iter(html) {
        let src = caps.get(2).or(caps.get(3)).map(|m| m.as_str()).unwrap_or("");
        anchor_counter += 1;
        items.push(MediaItem {
            media_type: MediaType::Photography,
            source_path: src.to_string(),
            title: extract_title_from_html(&caps[0], lang),
            collection: None,
            article_url: article_url.to_string(),
            anchor_id: format!("media-{}", anchor_counter),
        });
    }

    // Pattern 2: Video - img with video extension followed by <!-- video-meta: name -->
    let video_pattern = Regex::new(
        r#"(?s)<p>\s*<img[^>]+src="([^"]+\.(mov|mp4|webm|MOV|MP4|WEBM))"[^>]*alt="([^"]*)"[^>]*/?\s*>\s*</p>\s*<!--\s*video-meta:?\s*(\w*)?\s*-->"#
    ).unwrap();

    for caps in video_pattern.captures_iter(html) {
        anchor_counter += 1;
        let collection = caps.get(4).and_then(|m| {
            let s = m.as_str().trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        });
        items.push(MediaItem {
            media_type: MediaType::Video,
            source_path: caps[1].to_string(),
            title: caps[3].to_string(),
            collection,
            article_url: article_url.to_string(),
            anchor_id: format!("media-{}", anchor_counter),
        });
    }

    // Pattern 2b: Video - <video> tag followed by <!-- video-meta: name -->
    let video_tag_pattern = Regex::new(
        r#"(?s)<video[^>]+src="([^"]+\.(mov|mp4|webm|MOV|MP4|WEBM))"[^>]*>.*?</video>\s*<!--\s*video-meta:?\s*(\w*)?\s*-->"#
    ).unwrap();

    for caps in video_tag_pattern.captures_iter(html) {
        anchor_counter += 1;
        let collection = caps.get(3).and_then(|m| {
            let s = m.as_str().trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        });
        // Extract title from filename (stem without extension)
        let src = &caps[1];
        let title = Path::new(src)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "video".to_string());
        items.push(MediaItem {
            media_type: MediaType::Video,
            source_path: src.to_string(),
            title,
            collection,
            article_url: article_url.to_string(),
            anchor_id: format!("media-{}", anchor_counter),
        });
    }

    // Pattern 3: Interactive - iframe followed by <!-- interactive-meta: ... -->
    let interactive_pattern = Regex::new(
        r#"(?s)<iframe[^>]+src="([^"]+)"[^>]*>.*?</iframe>\s*<!--\s*interactive-meta:?\s*(\w*)?(?:\s*\|\s*title:\s*([^-]+))?\s*-->"#
    ).unwrap();

    for caps in interactive_pattern.captures_iter(html) {
        anchor_counter += 1;
        let title = caps.get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| extract_filename(&caps[1]));
        let collection = caps.get(2).and_then(|m| {
            let s = m.as_str().trim();
            if s.is_empty() { None } else { Some(s.to_string()) }
        });
        items.push(MediaItem {
            media_type: MediaType::Interactive,
            source_path: caps[1].to_string(),
            title,
            collection,
            article_url: article_url.to_string(),
            anchor_id: format!("media-{}", anchor_counter),
        });
    }

    items
}


/// Extracts all video file references from HTML content without requiring markers.
///
/// Finds video sources from:
/// - `<video src="...">` attributes
/// - `<source src="...">` elements inside video tags
/// - `<img src="...">` with video file extensions (.mov, .mp4, .webm, etc.)
///
/// # Arguments
/// * `html` - HTML content to scan
///
/// # Returns
/// * `Vec<String>` - List of video source paths found
#[allow(dead_code)] // Part of video pipeline, for detecting which videos are referenced in HTML
pub fn extract_video_references(html: &str) -> Vec<String> {
    let mut refs = Vec::new();

    // Pattern 1: <video src="...">
    let video_src_re = Regex::new(r#"<video[^>]+src="([^"]+)"[^>]*>"#).unwrap();
    for caps in video_src_re.captures_iter(html) {
        refs.push(caps[1].to_string());
    }

    // Pattern 2: <source src="..."> (inside video tags)
    let source_re = Regex::new(r#"<source[^>]+src="([^"]+)"[^>]*>"#).unwrap();
    for caps in source_re.captures_iter(html) {
        refs.push(caps[1].to_string());
    }

    // Pattern 3: <img src="..."> with video extension
    let video_extensions = ["mov", "mp4", "webm", "avi", "mkv"];
    let img_re = Regex::new(r#"<img[^>]+src="([^"]+)"[^>]*>"#).unwrap();
    for caps in img_re.captures_iter(html) {
        let src = &caps[1];
        let ext = std::path::Path::new(src)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if video_extensions.contains(&ext.as_str()) {
            refs.push(caps[1].to_string());
        }
    }

    refs
}

/// Extracts a title from HTML, trying figcaption first, then alt text.
fn extract_title_from_html(html: &str, lang: crate::i18n::Language) -> String {
    // Try figcaption first
    let figcaption_re = Regex::new(r#"<figcaption>([^<]+)</figcaption>"#).unwrap();
    if let Some(caps) = figcaption_re.captures(html) {
        return caps[1].to_string();
    }
    // Fall back to alt text
    let alt_re = Regex::new(r#"alt="([^"]+)""#).unwrap();
    if let Some(caps) = alt_re.captures(html) {
        return caps[1].to_string();
    }
    crate::i18n::t(lang, "untitled").to_string()
}

/// Extracts filename without extension from a path.
fn extract_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

/// Aggregates media items from all documents into collections by type.
pub fn aggregate_media_collections(
    documents: &[crate::build::types::ParsedDocument],
) -> std::collections::HashMap<MediaType, Vec<MediaItem>> {
    use std::collections::HashMap;

    let mut collections: HashMap<MediaType, Vec<MediaItem>> = HashMap::new();

    for doc in documents {
        for item in &doc.media_items {
            collections
                .entry(item.media_type.clone())
                .or_default()
                .push(item.clone());
        }
    }

    collections
}

/// Generates HTML for a single media grid item.
fn generate_grid_item(
    item: &MediaItem,
    // Optional variant manifest. When `Some`, the thumbnail `<img>` is
    // routed through `image_render::synthesize_image_html` and gains
    // `data-placeholder-src`, dims, LQIP, and (for image items)
    // `<picture><source srcset>` WebP wrap. When `None`, falls back to
    // bare `<img loading="lazy">` and the legacy regex pass retrofits
    // attrs — needed for the unit tests at lines 587+ that don't
    // construct a manifest. See
    // `docs/reference/structural-html-emission.md`.
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
) -> String {
    let data_type = match item.media_type {
        MediaType::Photography => "image",
        MediaType::Video => "video",
        MediaType::Interactive => "interactive",
    };

    let thumbnail = match item.media_type {
        MediaType::Video => {
            // Use poster frame (generated by FFmpeg) or first frame
            item.source_path
                .replace(".mov", ".thumb.jpg")
                .replace(".MOV", ".thumb.jpg")
                .replace(".mp4", ".thumb.jpg")
                .replace(".MP4", ".thumb.jpg")
        }
        _ => item.source_path.clone(),
    };

    // For videos, use the converted mp4 path
    let full_src = match item.media_type {
        MediaType::Video => item
            .source_path
            .replace(".mov", ".mp4")
            .replace(".MOV", ".mp4"),
        _ => item.source_path.clone(),
    };

    // Route the thumbnail `<img>` through the synthesizer when a manifest is
    // in scope. Gallery thumbnails are below the fold (lazy-loaded), so
    // `eager: false`. The wrapping `<figure class="media-item">` and the
    // overlay `<div>` are emitted as raw HTML — they're container chrome,
    // not content images.
    //
    // Phase 1 B1 (2026-05-25): the synthesizer now consumes `&AssetSnapshot`
    // instead of `Option<&MediaDimensionLookup>`. We build a snapshot inline
    // from whatever lookup is in scope (or an empty snapshot when callers
    // don't have one — preserving the prior `None` path's no-data semantics).
    // The variant data is unused at this stage (the synthesizer still picks
    // `<picture>` by file extension); Phase 1 B2 wires the variant probe.
    // BUG 6: the snapshot indexes under the lookup's real `dir_overrides`, so a
    // thumbnail under an overridden dir resolves to its output-URL key (not
    // just source-key + base-slug) and gets real dims instead of the fallback.
    let assets = crate::build::media::dimensions::snapshot_or_empty(media_lookup);
    let img_html = moss_core::render::image::synthesize_image_html(
        &thumbnail,
        &item.title,
        assets,
        moss_core::render::image::ImageContext::FolderCardCover,
        &moss_core::render::image::ImageRenderOptions::default(),
    );

    format!(
        r#"<figure class="media-item" tabindex="0" data-type="{}" data-src="{}" data-title="{}" data-article="{}">
  {}
  <div class="media-overlay">
    <p class="media-title">{}</p>
  </div>
</figure>"#,
        data_type,
        html_escape(&full_src),
        html_escape(&item.title),
        html_escape(&item.article_url),
        img_html,
        html_escape(&item.title),
    )
}

/// Generates the full media collection page HTML.
pub fn generate_media_page(
    media_type: &MediaType,
    items: &[MediaItem],
    site_name: &str,
    nav_html: &str,
    css_path: &str,
    lang: crate::i18n::Language,
    // The site's declared BCP-47 tag, for `<html lang>`. Separate from `lang`,
    // which draws the interface: `Language::code()` emitted the lowercase
    // internal form its own doc reserves for routing, so this page said
    // `lang="zh-hans"` while every content page on the same site said
    // `zh-Hant` (moss#1177).
    site_lang_tag: &str,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    js_theme_path: &str,
    lazy_chunk_attrs: &str,
    js_fullscreen_path: Option<&str>,
    search_js_tag: &str,
) -> String {
    let (page_title, _page_slug) = match media_type {
        MediaType::Photography => (crate::i18n::t(lang, "photography"), "photography"),
        MediaType::Video => (crate::i18n::t(lang, "videos"), "videos"),
        MediaType::Interactive => (crate::i18n::t(lang, "experiments"), "experiments"),
    };

    let grid_items: String = items
        .iter()
        .map(|item| generate_grid_item(item, media_lookup))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r##"<!DOCTYPE html>
<html data-moss-html-version="1" lang="{html_lang}">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{page_title} - {site_name}</title>
  <link rel="stylesheet" href="{css_path}">
</head>
<body>
  <header>
    <nav class="main-nav container">
      <div class="nav-content">
        {nav_html}
      </div>
    </nav>
  </header>

  <main>
    <div class="page-wrapper">
      <div class="container">
        <div class="main-content">
          <h1>{page_title}</h1>
          <div class="media-grid">
            {grid_items}
          </div>
        </div>
      </div>
    </div>
  </main>

  <!-- Lightbox -->
  <div id="lightbox" class="lightbox" hidden tabindex="-1">
    <button class="lightbox-close" aria-label="{close_label}">&times;</button>
    <div class="lightbox-content">
      <img class="lightbox-image" src="" alt="" hidden />
      <video class="lightbox-video" controls playsinline hidden></video>
      <iframe class="lightbox-iframe" hidden></iframe>
    </div>
    <div class="lightbox-caption">
      <p class="lightbox-title"></p>
      <a class="lightbox-article-link" href="">{view_in_article} &rarr;</a>
    </div>
    <button class="lightbox-nav lightbox-prev" aria-label="{previous_label}">&lsaquo;</button>
    <button class="lightbox-nav lightbox-next" aria-label="{next_label}">&rsaquo;</button>
  </div>

  <script src="{js_theme_path}"{lazy_chunk_attrs}></script>
{js_fullscreen_script}{search_js_tag}
</body>
</html>"##,
        html_lang = site_lang_tag,
        close_label = crate::i18n::t(lang, "close"),
        view_in_article = crate::i18n::t(lang, "view_in_article"),
        previous_label = crate::i18n::t(lang, "previous"),
        next_label = crate::i18n::t(lang, "next"),
        js_theme_path = js_theme_path,
        lazy_chunk_attrs = lazy_chunk_attrs,
        js_fullscreen_script = js_fullscreen_path
            .map(|p| format!("  <script src=\"{}\"></script>\n", p))
            .unwrap_or_default(),
        search_js_tag = search_js_tag,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::PageKind;

    // ===========================================
    // MediaType and MediaItem tests
    // ===========================================

    #[test]
    fn test_media_type_equality() {
        assert_eq!(MediaType::Photography, MediaType::Photography);
        assert_ne!(MediaType::Photography, MediaType::Video);
    }

    #[test]
    fn test_media_item_serialization() {
        let item = MediaItem {
            media_type: MediaType::Photography,
            source_path: "assets/photo.jpg".to_string(),
            title: "Sunset".to_string(),
            collection: None,
            article_url: "posts/travel.html".to_string(),
            anchor_id: "media-1".to_string(),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("Photography"));
        assert!(json.contains("Sunset"));
    }

    // ===========================================
    // extract_media_items tests
    // ===========================================

    #[test]
    fn test_extract_photography_from_figure() {
        let html = r#"<figure>
            <img src="../../assets/sunset.jpg" alt="Sunset" />
            <figcaption>Beautiful sunset</figcaption>
        </figure>
        <!-- photography -->"#;

        let items = extract_media_items(html, "posts/travel.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Photography);
        assert_eq!(items[0].source_path, "../../assets/sunset.jpg");
        assert_eq!(items[0].title, "Beautiful sunset");
        assert_eq!(items[0].article_url, "posts/travel.html");
    }

    #[test]
    fn untitled_media_fallback_localizes_by_lang() {
        // An uncaptioned photography item (no <figcaption>, no alt) falls back
        // to the localized "untitled" title. pipeline.rs passes the page's own
        // doc_lang here, so an item on a zh-hans page renders "无标题", not the
        // site default. Guards the downstream of the pipeline.rs doc_lang fix.
        let html = r#"<p><img src="../../assets/x.jpg" /></p>
        <!-- photography -->"#;

        let zh = extract_media_items(html, "zh-hans/gallery.html", crate::i18n::Language::ZhHans);
        assert_eq!(zh.len(), 1);
        assert_eq!(zh[0].title, "无标题", "zh-hans page → Chinese untitled title");

        let en = extract_media_items(html, "gallery.html", crate::i18n::Language::En);
        assert_eq!(en[0].title, "Untitled", "En page → English untitled title");
    }

    #[test]
    fn test_extract_photography_from_img() {
        let html = r#"<p><img src="../../assets/mountain.jpg" alt="Mountain peak" /></p>
        <!-- photography -->"#;

        let items = extract_media_items(html, "posts/hiking.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Photography);
        assert_eq!(items[0].title, "Mountain peak");
    }

    #[test]
    fn test_extract_video_with_collection() {
        let html = r#"<p><img src="../../videos/mist.mov" alt="Morning Mist" /></p>
        <!-- video-meta: nature -->"#;

        let items = extract_media_items(html, "posts/journal.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Video);
        assert_eq!(items[0].source_path, "../../videos/mist.mov");
        assert_eq!(items[0].title, "Morning Mist");
        assert_eq!(items[0].collection, Some("nature".to_string()));
    }

    #[test]
    fn test_extract_video_without_collection() {
        let html = r#"<p><img src="../../videos/clip.mp4" alt="My Clip" /></p>
        <!-- video-meta -->"#;

        let items = extract_media_items(html, "posts/video.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Video);
        assert_eq!(items[0].collection, None);
    }

    #[test]
    fn test_extract_interactive_with_title() {
        let html = r#"<iframe src="../../interactive/particles.html" width="100%" height="400"></iframe>
        <!-- interactive-meta: experiments | title: Particle Universe -->"#;

        let items = extract_media_items(html, "posts/code.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Interactive);
        assert_eq!(items[0].source_path, "../../interactive/particles.html");
        assert_eq!(items[0].title, "Particle Universe");
        assert_eq!(items[0].collection, Some("experiments".to_string()));
    }

    #[test]
    fn test_extract_interactive_without_title() {
        let html = r#"<iframe src="../../interactive/demo.html"></iframe>
        <!-- interactive-meta -->"#;

        let items = extract_media_items(html, "posts/demo.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Interactive);
        assert_eq!(items[0].title, "demo"); // Extracted from filename
    }

    #[test]
    fn test_extract_video_from_video_tag_with_marker() {
        let html = r#"<video src="./Morning Mist.mov" controls width="100%"></video>
        <!-- video-meta: films -->"#;

        let items = extract_media_items(html, "videos/morning-mist.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Video);
        assert_eq!(items[0].source_path, "./Morning Mist.mov");
        assert_eq!(items[0].title, "Morning Mist");
        assert_eq!(items[0].collection, Some("films".to_string()));
    }

    #[test]
    fn test_extract_video_from_video_tag_without_collection() {
        let html = r#"<video src="./clip.MOV" controls></video>
        <!-- video-meta -->"#;

        let items = extract_media_items(html, "videos/clip.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].media_type, MediaType::Video);
        assert_eq!(items[0].source_path, "./clip.MOV");
        assert_eq!(items[0].collection, None);
    }

    #[test]
    fn test_no_markers_returns_empty() {
        let html = r#"<p>Regular content</p>
        <figure><img src="photo.jpg" /></figure>"#;

        let items = extract_media_items(html, "posts/article.html", crate::i18n::Language::En);
        assert!(items.is_empty());
    }

    #[test]
    fn test_multiple_media_items() {
        let html = r#"
        <figure><img src="photo1.jpg" alt="First" /></figure>
        <!-- photography -->
        <p>Some text</p>
        <figure><img src="photo2.jpg" alt="Second" /></figure>
        <!-- photography -->
        "#;

        let items = extract_media_items(html, "posts/gallery.html", crate::i18n::Language::En);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].anchor_id, "media-1");
        assert_eq!(items[1].anchor_id, "media-2");
    }

    // ===========================================
    // Grid item generation tests
    // ===========================================

    #[test]
    fn test_generate_grid_item_photography() {
        let item = MediaItem {
            media_type: MediaType::Photography,
            source_path: "assets/photo.jpg".to_string(),
            title: "Sunset".to_string(),
            collection: None,
            article_url: "posts/travel.html".to_string(),
            anchor_id: "media-1".to_string(),
        };
        let html = generate_grid_item(&item, None);
        assert!(html.contains(r#"data-type="image""#));
        assert!(html.contains(r#"data-title="Sunset""#));
        assert!(html.contains(r#"data-article="posts/travel.html""#));
    }

    #[test]
    fn test_generate_grid_item_video() {
        let item = MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mov".to_string(),
            title: "My Video".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
            anchor_id: "media-1".to_string(),
        };
        let html = generate_grid_item(&item, None);
        assert!(html.contains(r#"data-type="video""#));
        // Should use mp4 for full src
        assert!(html.contains(r#"data-src="videos/clip.mp4""#));
        // Should use thumb for thumbnail
        assert!(html.contains(r#"src="videos/clip.thumb.jpg""#));
    }

    #[test]
    fn test_generate_grid_item_photography_synthesizer_routed() {
        // Verifies the gallery-thumbnail synthesizer-routing path that the
        // arch review of Step 6 flagged as untested. When a manifest carries
        // a WebP variant for the thumbnail's source, the inner `<img>` MUST
        // be wrapped in `<picture><source srcset>` with the correct URL,
        // dims, and `loading="lazy"` (galleries are below-the-fold so the
        // synthesizer's `eager: false` default is correct here).
        use crate::build::media::dimensions::MediaDimensionLookup;
        use crate::types::content::MediaMetadata;
        use std::collections::HashMap;
        let item = MediaItem {
            media_type: MediaType::Photography,
            source_path: "assets/photo.jpg".to_string(),
            title: "Sunset".to_string(),
            collection: None,
            article_url: "posts/travel.html".to_string(),
            anchor_id: "media-1".to_string(),
        };
        let images = vec![MediaMetadata {
            is_animated: false,
            path: "assets/photo.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 50_000,
            modified: None,
            dimensions: Some((1600, 1067)),
            dominant_color: None,
            lqip_data_uri: None,
        }];
        let lookup = MediaDimensionLookup::new(&images, &[], &std::collections::HashMap::new(), None);
        let html = generate_grid_item(&item, Some(&lookup));
        assert!(
            html.contains("<picture>"),
            "manifested WebP variant must produce <picture> wrap, got: {}", html
        );
        // 1600px-wide fixture → srcset ladder (responsive-image-variants
        // Task 3): the w800 rung plus the base descriptor at the deployed
        // width, with the card grid-cell sizes.
        assert!(
            html.contains(
                r#"<source srcset="assets/photo.w800.webp 800w, assets/photo.webp 1600w" type="image/webp" sizes="auto, (min-width: 48rem) 24rem, 100vw">"#
            ),
            "<source> must carry the WebP ladder derived from the probe, got: {}", html
        );
        assert!(
            html.contains(r#"width="1600""#) && html.contains(r#"height="1067""#),
            "manifest dims must reach the inner <img>, got: {}", html
        );
        assert!(
            html.contains(r#"loading="lazy""#),
            "below-the-fold gallery thumbs stay lazy, got: {}", html
        );
        // Container chrome stays — `<figure class="media-item">` wraps the
        // synthesizer's `<picture>` and the overlay is appended.
        assert!(html.contains(r#"<figure class="media-item""#));
        assert!(html.contains(r#"data-type="image""#));
        assert!(html.contains(r#"data-article="posts/travel.html""#));
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("Tom & Jerry"), "Tom &amp; Jerry");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape(r#"Say "hello""#), "Say &quot;hello&quot;");
    }

    // ===========================================
    // Page generation tests
    // ===========================================

    #[test]
    fn test_generate_media_page_structure() {
        let items = vec![
            MediaItem {
                media_type: MediaType::Photography,
                source_path: "assets/photo1.jpg".to_string(),
                title: "First Photo".to_string(),
                collection: None,
                article_url: "posts/travel.html".to_string(),
                anchor_id: "media-1".to_string(),
            },
            MediaItem {
                media_type: MediaType::Photography,
                source_path: "assets/photo2.jpg".to_string(),
                title: "Second Photo".to_string(),
                collection: None,
                article_url: "posts/nature.html".to_string(),
                anchor_id: "media-2".to_string(),
            },
        ];

        let html = generate_media_page(
            &MediaType::Photography,
            &items,
            "My Site",
            "<a href=\"/\">Home</a>",
            "/_moss/style.testabcd.css",
            crate::i18n::Language::En,
            crate::i18n::Language::En.as_bcp47_attr(),
            None,
            "/_moss/js/theme.testabcd.js",
            "",
            None,
            "",
        );

        // Check page structure
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Photography - My Site</title>"));
        assert!(html.contains("<h1>Photography</h1>"));
        assert!(html.contains("media-grid"));

        // Check grid items
        assert!(html.contains("First Photo"));
        assert!(html.contains("Second Photo"));

        // Check lightbox structure
        assert!(html.contains("id=\"lightbox\""));
        assert!(html.contains("lightbox-close"));
        assert!(html.contains("lightbox-prev"));
        assert!(html.contains("lightbox-next"));

        // Check nav included
        assert!(html.contains(">Home</a>"));

        // Check script reference
        assert!(!html.contains("fullscreen"), "fullscreen script must not appear when js_fullscreen_path is None");
    }

    #[test]
    fn test_generate_media_page_video_type() {
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mov".to_string(),
            title: "My Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];

        let html = generate_media_page(&MediaType::Video, &items, "My Site", "", "/_moss/style.testabcd.css", crate::i18n::Language::En, crate::i18n::Language::En.as_bcp47_attr(), None, "/_moss/js/theme.testabcd.js", "", None, "");

        assert!(html.contains("<title>Videos - My Site</title>"));
        assert!(html.contains("<h1>Videos</h1>"));
    }

    #[test]
    fn the_media_page_emits_the_sites_declared_tag_not_its_ui_variant() {
        // `fr` has no `Language` variant, so it resolves to `En` for the
        // interface while declaring `fr`. This page used to emit the enum's
        // `code()` and so said `lang="en"` on an `fr` site — the same
        // contradiction as moss#1177, on the one page the audit first missed.
        // Every other test here passes `En` for BOTH arguments and so cannot
        // tell the two apart.
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mp4".to_string(),
            title: "Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];

        let html = generate_media_page(&MediaType::Video, &items, "My Site", "", "/_moss/style.testabcd.css", crate::i18n::Language::En, "fr", None, "/_moss/js/theme.testabcd.js", "", None, "");

        assert!(html.contains(r#"lang="fr""#), "{html}");
        assert!(!html.contains(r#"lang="en""#), "{html}");
    }

    #[test]
    fn test_generate_media_page_interactive_type() {
        let items = vec![MediaItem {
            media_type: MediaType::Interactive,
            source_path: "interactive/demo.html".to_string(),
            title: "Demo".to_string(),
            collection: Some("experiments".to_string()),
            article_url: "posts/code.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];

        let html = generate_media_page(&MediaType::Interactive, &items, "My Site", "", "/_moss/style.testabcd.css", crate::i18n::Language::En, crate::i18n::Language::En.as_bcp47_attr(), None, "/_moss/js/theme.testabcd.js", "", None, "");

        assert!(html.contains("<title>Experiments - My Site</title>"));
        assert!(html.contains("<h1>Experiments</h1>"));
    }

    #[test]
    fn test_aggregate_media_collections() {
        use crate::build::types::ParsedDocument;

        let docs = vec![
            ParsedDocument {
                title: "Post 1".to_string(),
                label: "Post 1".to_string(),
                url_path: "posts/post1.html".to_string(),
                reading_time: 1,
                slug: "post1".to_string(),
                permalink: "/posts/post1".to_string(),
                media_items: vec![
                    MediaItem {
                        media_type: MediaType::Photography,
                        source_path: "photo1.jpg".to_string(),
                        title: "Photo 1".to_string(),
                        collection: None,
                        article_url: "posts/post1.html".to_string(),
                        anchor_id: "media-1".to_string(),
                    },
                ],
                kind: PageKind::Article,
                ..Default::default()
            },
            ParsedDocument {
                title: "Post 2".to_string(),
                label: "Post 2".to_string(),
                url_path: "posts/post2.html".to_string(),
                reading_time: 1,
                slug: "post2".to_string(),
                permalink: "/posts/post2".to_string(),
                media_items: vec![
                    MediaItem {
                        media_type: MediaType::Photography,
                        source_path: "photo2.jpg".to_string(),
                        title: "Photo 2".to_string(),
                        collection: None,
                        article_url: "posts/post2.html".to_string(),
                        anchor_id: "media-1".to_string(),
                    },
                    MediaItem {
                        media_type: MediaType::Video,
                        source_path: "video1.mov".to_string(),
                        title: "Video 1".to_string(),
                        collection: None,
                        article_url: "posts/post2.html".to_string(),
                        anchor_id: "media-2".to_string(),
                    },
                ],
                kind: PageKind::Article,
                ..Default::default()
            },
        ];

        let collections = aggregate_media_collections(&docs);

        assert_eq!(collections.get(&MediaType::Photography).unwrap().len(), 2);
        assert_eq!(collections.get(&MediaType::Video).unwrap().len(), 1);
        assert!(collections.get(&MediaType::Interactive).is_none());
    }

    // ===========================================
    // Folder-Based Collection Derivation Tests (TDD)
    // ===========================================

    
    
    
    
    
    
    // ===========================================
    // Video Reference Extraction Tests (TDD - no markers)
    // ===========================================

    #[test]
    fn test_extract_video_references_from_video_tag() {
        let html = r#"<video src="./clip.MOV" controls width="100%"></video>"#;
        let refs = extract_video_references(html);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], "./clip.MOV");
    }

    #[test]
    fn test_extract_video_references_multiple() {
        let html = r#"
            <p>First video:</p>
            <video src="./intro.mp4" controls></video>
            <p>Second video:</p>
            <video src="../videos/nature.MOV" controls></video>
        "#;
        let refs = extract_video_references(html);

        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"./intro.mp4".to_string()));
        assert!(refs.contains(&"../videos/nature.MOV".to_string()));
    }

    #[test]
    fn test_extract_video_references_no_videos() {
        let html = r#"<p>Just text and <img src="photo.jpg"> images</p>"#;
        let refs = extract_video_references(html);

        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_video_references_img_with_video_extension() {
        // Some users embed videos using img tags (converted by browser/JS)
        let html = r#"<p><img src="./clip.mov" alt="Video"></p>"#;
        let refs = extract_video_references(html);

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], "./clip.mov");
    }

    #[test]
    fn test_extract_video_references_no_marker_required() {
        // Key test: video should be extracted WITHOUT any marker
        let html = r#"<video src="./aimeili.MOV" controls width="100%"></video>"#;
        let refs = extract_video_references(html);

        // Should find the video - no marker needed!
        assert_eq!(refs.len(), 1, "Should extract video without marker");
        assert_eq!(refs[0], "./aimeili.MOV");
    }

    #[test]
    fn test_extract_video_references_source_element() {
        // Video with <source> child elements
        let html = r#"
            <video controls>
                <source src="./clip.mp4" type="video/mp4">
                <source src="./clip.webm" type="video/webm">
            </video>
        "#;
        let refs = extract_video_references(html);

        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"./clip.mp4".to_string()));
        assert!(refs.contains(&"./clip.webm".to_string()));
    }

    #[test]
    fn test_generate_media_page_uses_root_relative_paths() {
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mov".to_string(),
            title: "My Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];
        let html = generate_media_page(
            &MediaType::Video,
            &items,
            "My Site",
            "",
            "/_moss/style.abc123.css",
            crate::i18n::Language::En,
            crate::i18n::Language::En.as_bcp47_attr(),
            None,
            "/_moss/js/theme.def456.js",
            "",
            None,
            "",
        );
        assert!(html.contains(r#"src="/_moss/js/theme.def456.js""#), "theme.js src must be root-relative hashed");
        assert!(!html.contains("../"), "media page must not use ../ relative paths");
    }

    #[test]
    fn test_generate_media_page_includes_search_js_tag() {
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mov".to_string(),
            title: "My Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];
        let html = generate_media_page(
            &MediaType::Video,
            &items,
            "My Site",
            "",
            "/_moss/style.abc123.css",
            crate::i18n::Language::En,
            crate::i18n::Language::En.as_bcp47_attr(),
            None,
            "/_moss/js/theme.def456.js",
            "",
            None,
            "\n    <script src=\"/_moss/js/search.abc123.js\" defer></script>",
        );
        assert!(
            html.contains(r#"src="/_moss/js/search.abc123.js""#),
            "media pages must load search.js when search is enabled, or the nav search button is dead"
        );
    }

    #[test]
    fn test_generate_media_page_fullscreen_js_optional() {
        let items = vec![MediaItem {
            media_type: MediaType::Photography,
            source_path: "photo.jpg".to_string(),
            title: "Photo".to_string(),
            collection: None,
            article_url: "posts/photo.html".to_string(),
            anchor_id: "media-1".to_string(),
        }];
        let html_with = generate_media_page(
            &MediaType::Photography,
            &items,
            "My Site",
            "",
            "/_moss/style.abc.css",
            crate::i18n::Language::En,
            crate::i18n::Language::En.as_bcp47_attr(),
            None,
            "/_moss/js/theme.def.js",
            "",
            Some("/_moss/js/fullscreen.ghi.js"),
            "",
        );
        assert!(html_with.contains(r#"src="/_moss/js/fullscreen.ghi.js""#));

        let html_without = generate_media_page(
            &MediaType::Photography,
            &items,
            "My Site",
            "",
            "/_moss/style.abc.css",
            crate::i18n::Language::En,
            crate::i18n::Language::En.as_bcp47_attr(),
            None,
            "/_moss/js/theme.def.js",
            "",
            None,
            "",
        );
        assert!(!html_without.contains("fullscreen"), "fullscreen script must not appear when js_fullscreen_path is None");
    }
}
