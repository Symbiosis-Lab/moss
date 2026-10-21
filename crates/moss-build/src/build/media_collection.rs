//! Media collection support for photography, videos, and interactive experiments.
//!
//! This module handles:
//! - Collecting the media a page marks with HTML comment markers
//! - Aggregating media items into collections
//! - Generating collection pages with grid layouts

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;
use crate::build::media::cover::html_escape;

// Compiled once per process: `extract_media_items` runs for every page, and
// compiling these per call cost more than matching them. They read only HTML
// the author wrote by hand; an image written in markdown is read from the AST.
// Each element is matched on its own and then the text right after it is
// tested for a marker, so a match can never run from one element to a later
// element's marker.
static FIGURE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<figure\b[^>]*>.*?</figure>|<p>\s*<img\b[^>]*>\s*</p>"#).unwrap());
static VIDEO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<video\b[^>]+src="([^"]+\.(?:mov|mp4|webm|MOV|MP4|WEBM))"[^>]*>.*?</video>"#).unwrap());
static IFRAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<iframe\b[^>]+src="([^"]+)"[^>]*>.*?</iframe>"#).unwrap());
static IMG_SRC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<img\b[^>]+src="([^"]+)""#).unwrap());
static INTERACTIVE_MARK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*<!--\s*interactive-meta:?\s*(\w*)?(?:\s*\|\s*title:\s*([^-]+))?\s*-->"#).unwrap());
static FIGCAPTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<figcaption>([^<]+)</figcaption>"#).unwrap());
static ALT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"alt="([^"]+)""#).unwrap());

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
}

impl MediaItem {
    fn marked(media_type: MediaType, source_path: &str, title: String, collection: Option<String>, article_url: &str) -> Self {
        Self { media_type, source_path: source_path.to_string(), title, collection, article_url: article_url.to_string() }
    }
}

/// Collects the media a page marks for the **gallery grids**, in page order,
/// including inside callouts, lists, grid cells and the like.
///
/// A marker is an HTML comment on the line after the media:
///
/// ```markdown
/// ![Sunset](sunset.jpg)
/// <!-- photography -->
///
/// ![Morning mist](mist.mp4)
/// <!-- video-meta: collection_name -->
/// ```
///
/// An image written in markdown is read from the page's blocks
/// ([`moss_core::ast::marked_media_at`]), so how the renderer wraps it (a
/// figure, a paragraph, a `<picture>`, source-line attributes) does not
/// matter, and a photography marker collects only an image file and a video
/// marker only a video file. Media written as HTML by hand (`<figure>`,
/// `<video>`, `<iframe>` followed by `<!-- interactive-meta: collection |
/// title: Demo -->`) is read from that HTML.
///
/// **Note:** Video conversion is handled separately by `collect_videos_for_conversion()`,
/// which automatically converts all MOV files without requiring markers.
pub fn extract_media_items(blocks: &[moss_core::ast::Block], article_url: &str, lang: crate::i18n::Language) -> Vec<MediaItem> {
    let mut items = Vec::new();
    collect_media(blocks, article_url, lang, &mut items);
    items
}

/// [`extract_media_items`] for one block sequence, descending into a
/// container where it stands so nested media keeps its place in the page.
fn collect_media(sequence: &[moss_core::ast::Block], article_url: &str, lang: crate::i18n::Language, items: &mut Vec<MediaItem>) {
    use moss_core::ast::{Block, MediaMarker};
    // Consecutive raw HTML blocks, read together: a hand-written `<video>`
    // and the marker on the line after it can arrive as two blocks.
    let mut raw = String::new();
    for (i, block) in sequence.iter().enumerate() {
        if let Some(marked) = moss_core::ast::marked_media_at(sequence, i) {
            items.extend(media_in_html(&std::mem::take(&mut raw), article_url, lang));
            let href = marked.href.as_str();
            items.push(match marked.marker {
                MediaMarker::Photography => {
                    let title = if marked.title.is_empty() { crate::i18n::t(lang, "untitled").to_string() } else { marked.title };
                    MediaItem::marked(MediaType::Photography, href, title, None, article_url)
                }
                MediaMarker::Video { collection } => {
                    MediaItem::marked(MediaType::Video, href, extract_filename(href), collection, article_url)
                }
                // A marker this build does not collect for yet.
                _ => continue,
            });
        } else if let Block::Other(html) = block {
            raw.push_str(html);
            raw.push('\n');
        } else {
            items.extend(media_in_html(&std::mem::take(&mut raw), article_url, lang));
            moss_core::ast::for_each_child_sequence(block, |nested| {
                collect_media(nested, article_url, lang, items);
                true
            });
        }
    }
    items.extend(media_in_html(&raw, article_url, lang));
}

/// The marker comment directly after `html[..end]`, if there is one.
fn marker_after(html: &str, end: usize) -> Option<moss_core::ast::MediaMarker> {
    let rest = html[end..].trim_start();
    let len = rest.strip_prefix("<!--")?.find("-->")? + "<!--".len() + "-->".len();
    moss_core::ast::MediaMarker::parse(&rest[..len])
}

/// Media written as HTML by hand and followed by its marker, in page order.
fn media_in_html(html: &str, article_url: &str, lang: crate::i18n::Language) -> Vec<MediaItem> {
    if !html.contains("<!--") {
        return Vec::new();
    }
    let item = |media_type, source_path: &str, title, collection| MediaItem::marked(media_type, source_path, title, collection, article_url);
    let word = |m: Option<regex::Match>| m.map(|m| m.as_str().trim()).filter(|s| !s.is_empty()).map(str::to_string);
    let mut found: Vec<(usize, MediaItem)> = Vec::new();
    for m in FIGURE_RE.find_iter(html) {
        if let (Some(moss_core::ast::MediaMarker::Photography), Some(src)) = (marker_after(html, m.end()), IMG_SRC_RE.captures(m.as_str())) {
            found.push((m.start(), item(MediaType::Photography, &src[1], extract_title_from_html(m.as_str(), lang), None)));
        }
    }
    for caps in VIDEO_RE.captures_iter(html) {
        let element = caps.get(0).expect("whole match");
        if let Some(moss_core::ast::MediaMarker::Video { collection }) = marker_after(html, element.end()) {
            found.push((element.start(), item(MediaType::Video, &caps[1], extract_filename(&caps[1]), collection)));
        }
    }
    for caps in IFRAME_RE.captures_iter(html) {
        let element = caps.get(0).expect("whole match");
        if let Some(mark) = INTERACTIVE_MARK_RE.captures(&html[element.end()..]) {
            let title = word(mark.get(2)).unwrap_or_else(|| extract_filename(&caps[1]));
            found.push((element.start(), item(MediaType::Interactive, &caps[1], title, word(mark.get(1)))));
        }
    }
    found.sort_by_key(|(start, _)| *start);
    found.into_iter().map(|(_, item)| item).collect()
}

/// Extracts a title from HTML, trying figcaption first, then alt text.
fn extract_title_from_html(html: &str, lang: crate::i18n::Language) -> String {
    // Try figcaption first
    if let Some(caps) = FIGCAPTION_RE.captures(html) {
        return caps[1].to_string();
    }
    // Fall back to alt text
    if let Some(caps) = ALT_RE.captures(html) {
        return caps[1].to_string();
    }
    crate::i18n::t(lang, "untitled").to_string()
}

/// Extracts filename without extension from a path.
fn extract_filename(path: &str) -> String {
    Path::new(path)
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
    // construct a manifest.
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

/// The full-screen viewer's markup, which `fullscreen.js` drives. Shared by the
/// media collection pages and by any page with a picture that opens in it.
pub(crate) fn lightbox_html(lang: crate::i18n::Language) -> String {
    format!(
        r##"<!-- Lightbox -->
  <div id="lightbox" class="lightbox" role="dialog" aria-modal="true" aria-label="{viewer_label}" hidden tabindex="-1">
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
  </div>"##,
        viewer_label = crate::i18n::t(lang, "image_viewer"),
        close_label = crate::i18n::t(lang, "close"),
        view_in_article = crate::i18n::t(lang, "view_in_article"),
        previous_label = crate::i18n::t(lang, "previous"),
        next_label = crate::i18n::t(lang, "next"),
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
    // `zh-Hant`.
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

  {lightbox}

  <script src="{js_theme_path}"{lazy_chunk_attrs}></script>
{js_fullscreen_script}{search_js_tag}
</body>
</html>"##,
        html_lang = site_lang_tag,
        lightbox = lightbox_html(lang),
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
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("Photography"));
        assert!(json.contains("Sunset"));
    }

    // ===========================================
    // extract_media_items tests
    // ===========================================

    /// Parse `markdown` the way a page is parsed, implicit figures on or off.
    fn media(markdown: &str, implicit_figure: bool, lang: crate::i18n::Language) -> Vec<MediaItem> {
        let config = moss_core::ast::ParseConfig { implicit_figure, ..Default::default() };
        extract_media_items(&moss_core::ast::parse_with_config(markdown, &config).blocks, "posts/travel.html", lang)
    }

    #[test]
    fn a_marked_markdown_photo_is_collected_as_a_figure_or_a_paragraph() {
        for implicit_figure in [true, false] {
            let items = media("![Sunset](assets/sunset.jpg)\n<!-- photography -->\n", implicit_figure, crate::i18n::Language::En);
            assert_eq!(items.len(), 1, "implicit_figure = {implicit_figure}");
            assert_eq!(items[0].media_type, MediaType::Photography);
            assert_eq!(items[0].source_path, "assets/sunset.jpg");
            assert_eq!(items[0].title, "Sunset");
            assert_eq!(items[0].article_url, "posts/travel.html");
        }
    }

    #[test]
    fn an_italic_caption_titles_the_photo() {
        let items = media("![Sunset](sunset.jpg)\n*Over the hills*\n<!-- photography -->\n", true, crate::i18n::Language::En);
        assert_eq!(items[0].title, "Over the hills");
    }

    #[test]
    fn untitled_media_fallback_localizes_by_lang() {
        // An image without alt text is never promoted to a figure, and falls
        // back to the localized "untitled" title of the page's own language.
        let zh = media("![](x.jpg)\n<!-- photography -->\n", true, crate::i18n::Language::ZhHans);
        assert_eq!(zh.len(), 1);
        assert_eq!(zh[0].title, "无标题", "zh-hans page → Chinese untitled title");
        let en = media("![](x.jpg)\n<!-- photography -->\n", true, crate::i18n::Language::En);
        assert_eq!(en[0].title, "Untitled", "En page → English untitled title");
    }

    #[test]
    fn a_marked_markdown_video_is_collected_with_its_collection() {
        for implicit_figure in [true, false] {
            let items = media("![Morning Mist](videos/mist.mov)\n<!-- video-meta: nature -->\n", implicit_figure, crate::i18n::Language::En);
            assert_eq!(items.len(), 1, "implicit_figure = {implicit_figure}");
            assert_eq!(items[0].media_type, MediaType::Video);
            assert_eq!(items[0].source_path, "videos/mist.mov");
            assert_eq!(items[0].title, "mist");
            assert_eq!(items[0].collection, Some("nature".to_string()));
            let bare = media("![](clip.mp4)\n<!-- video-meta -->\n", implicit_figure, crate::i18n::Language::En);
            assert_eq!(bare[0].collection, None);
        }
    }

    #[test]
    fn a_marked_photo_in_a_grid_cell_is_collected() {
        let items = media(":::grid\n![A](a.jpg)\n<!-- photography -->\n+++\n![B](b.jpg)\n:::\n", true, crate::i18n::Language::En);
        let srcs: Vec<&str> = items.iter().map(|i| i.source_path.as_str()).collect();
        assert_eq!(srcs, ["a.jpg"]);
    }

    /// Media inside a container keeps its place in the page: before a marked
    /// photo that comes after the container, not after every top-level one.
    #[test]
    fn nested_media_is_collected_in_page_order() {
        let callout = "> [!note]\n> ![A](a.jpg)\n> <!-- photography -->\n\n![B](b.jpg)\n<!-- photography -->\n";
        let grid = ":::grid\n![A](a.jpg)\n<!-- photography -->\n+++\nText.\n:::\n\n![B](b.jpg)\n<!-- photography -->\n";
        for source in [callout, grid] {
            let srcs: Vec<String> = media(source, true, crate::i18n::Language::En).into_iter().map(|i| i.source_path).collect();
            assert_eq!(srcs, ["a.jpg", "b.jpg"], "{source}");
        }
    }

    /// An unmarked image before a marked one is not listed in its place.
    #[test]
    fn only_the_marked_one_of_two_images_is_collected() {
        let markdown = "![First](one.jpg)\n\nText between.\n\n![Second](two.jpg)\n<!-- photography -->\n";
        let html = "<figure><img src=\"one.jpg\" alt=\"First\"></figure>\n<figure><img src=\"two.jpg\" alt=\"Second\"></figure>\n<!-- photography -->\n";
        for source in [markdown, html] {
            let srcs: Vec<String> = media(source, true, crate::i18n::Language::En).into_iter().map(|i| i.source_path).collect();
            assert_eq!(srcs, ["two.jpg"], "{source}");
        }
    }

    #[test]
    fn a_marked_photo_inside_a_callout_or_list_is_collected() {
        let items = media("> [!note]\n> ![A](a.jpg)\n> <!-- photography -->\n\n- ![B](b.jpg)\n  <!-- photography -->\n", true, crate::i18n::Language::En);
        let srcs: Vec<&str> = items.iter().map(|i| i.source_path.as_str()).collect();
        assert_eq!(srcs, ["a.jpg", "b.jpg"]);
    }

    #[test]
    fn hand_written_html_media_is_collected() {
        let html = "<figure>\n<img src=\"sunset.jpg\" alt=\"Sunset\" />\n<figcaption>Beautiful sunset</figcaption>\n</figure>\n<!-- photography -->\n\n\
                    <video src=\"./Morning Mist.mov\" controls>\n</video>\n<!-- video-meta: films -->\n\n\
                    <iframe src=\"particles.html\"></iframe>\n<!-- interactive-meta: experiments | title: Particle Universe -->\n\n\
                    <iframe src=\"demo.html\"></iframe>\n<!-- interactive-meta -->\n";
        let items = media(html, true, crate::i18n::Language::En);
        let got: Vec<(&MediaType, &str, &str, Option<&str>)> =
            items.iter().map(|i| (&i.media_type, i.source_path.as_str(), i.title.as_str(), i.collection.as_deref())).collect();
        assert_eq!(
            got,
            [
                (&MediaType::Photography, "sunset.jpg", "Beautiful sunset", None),
                (&MediaType::Video, "./Morning Mist.mov", "Morning Mist", Some("films")),
                (&MediaType::Interactive, "particles.html", "Particle Universe", Some("experiments")),
                (&MediaType::Interactive, "demo.html", "demo", None),
            ]
        );
    }

    #[test]
    fn test_no_markers_returns_empty() {
        assert!(media("Regular content\n\n![Photo](photo.jpg)\n", true, crate::i18n::Language::En).is_empty());
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
        };
        let images = vec![MediaMetadata {
            is_animated: false,
            path: "assets/photo.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 50_000,
            modified: None,
            dimensions: Some((1600, 1067)),
            dominant_color: None,
            tone_bands: None,
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
            },
            MediaItem {
                media_type: MediaType::Photography,
                source_path: "assets/photo2.jpg".to_string(),
                title: "Second Photo".to_string(),
                collection: None,
                article_url: "posts/nature.html".to_string(),
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
        // contradiction found elsewhere, on the one page the audit first missed.
        // Every other test here passes `En` for BOTH arguments and so cannot
        // tell the two apart.
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mp4".to_string(),
            title: "Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
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
                    },
                    MediaItem {
                        media_type: MediaType::Video,
                        source_path: "video1.mov".to_string(),
                        title: "Video 1".to_string(),
                        collection: None,
                        article_url: "posts/post2.html".to_string(),
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

    
    
    
    
    
    
    #[test]
    fn test_generate_media_page_uses_root_relative_paths() {
        let items = vec![MediaItem {
            media_type: MediaType::Video,
            source_path: "videos/clip.mov".to_string(),
            title: "My Clip".to_string(),
            collection: None,
            article_url: "posts/video.html".to_string(),
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
