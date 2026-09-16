//! Readymag dialect: detection + `window.ServerData` navigation + the
//! canvas-widget walk over pre-rendered snapshots.
//!
//! Readymag viewer pages are client-rendered shells: no content and no
//! `<a>` links server-side. Everything flows from the bootstrap JSON —
//! the page manifest (`mags[].pages[]`), each page's `htmlUrl` pointing at
//! a pre-rendered snapshot on the CDN, and titles. The snapshot carries
//! absolutely-positioned widgets, so reading order is recovered with
//! `super::geometry` (canvas DOM order clusters media after text; visual
//! order interleaves them beside their passages).
//!
//! Detection is content-based only (bootstrap global + CDN marker), never
//! hostname. What is irreducibly Readymag here: the ServerData paths, the
//! widget class vocabulary, and the internal-nav link shape. The walking,
//! ordering, media-URL and emission machinery is shared.

use super::blocks::Block;
use super::emit;
use super::geometry::{self, Positioned};
use super::media;
use super::state_json;
use crate::vault::import::scrape::converter::{AdapterBody, MetadataOverrides, SiteContent};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

/// Content-based Readymag detection: the ServerData bootstrap AND a
/// platform CDN marker, so pages merely mentioning Readymag never match.
pub(crate) fn is_readymag(html: &str) -> bool {
    html.contains("window.ServerData") && html.contains("rmcdn.net")
}

/// The mag out of `ServerData.mags` (keyed by internal id): prefer the one
/// whose `uri` appears in the request URL path; fall back to the first
/// (single-mag pages, the normal case).
fn mag_from(html: &str, url: &str) -> Option<Value> {
    let data = state_json::extract_json_assignment(html, "window.ServerData = ")
        .or_else(|| state_json::extract_json_assignment(html, "window.ServerData="))?;
    let mags = data.get("mags")?.as_object()?;
    let segs: Vec<String> = url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .map(|s| s.filter(|x| !x.is_empty()).map(str::to_string).collect())
        })
        .unwrap_or_default();
    mags.values()
        .find(|m| {
            m.get("uri")
                .and_then(Value::as_str)
                .is_some_and(|u| segs.iter().any(|s| s == u))
        })
        .or_else(|| mags.values().next())
        .cloned()
}

/// The mag's URL identity segment (`uri`, e.g. "4612303") and where it
/// appears in the request URL. Everything hangs off this: page URLs are
/// siblings under it, and snapshot URLs must contain it (same-account
/// scope, design ruling #3).
fn mag_base(mag: &Value, url: &str) -> Option<(String, String)> {
    let mag_uri = mag.get("uri")?.as_str()?.to_string();
    let parsed = url::Url::parse(url).ok()?;
    let segs: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    let idx = segs.iter().position(|s| *s == mag_uri)?;
    let base = format!(
        "{}://{}/{}",
        parsed.scheme(),
        parsed.host_str()?,
        segs[..=idx].join("/")
    );
    Some((mag_uri, base))
}

/// Every page URL from the manifest — the ONLY discovery channel for a
/// client-rendered viewer (no server-side anchors exist). Empty when the
/// page is not a Readymag viewer.
pub(crate) fn manifest_urls(html: &str, url: &str) -> Vec<String> {
    if !is_readymag(html) {
        return Vec::new();
    }
    let Some(mag) = mag_from(html, url) else {
        return Vec::new();
    };
    let Some((mag_uri, base)) = mag_base(&mag, url) else {
        return Vec::new();
    };
    let Some(pages) = mag.get("pages").and_then(Value::as_array) else {
        return Vec::new();
    };
    // The requested page is excluded, and the home page (num == 1) is
    // emitted in its CANONICAL mag-root form: `{base}/` and `{base}/home/`
    // are different strings for the same page, and the crawler's visited
    // set can only dedupe canonical URLs. (A start URL without the
    // trailing slash still slips this — trailing-slash normalization
    // belongs to the crawler, not this dialect.)
    let current = current_page(&mag, &mag_uri, url)
        .and_then(|p| p.get("uri").and_then(Value::as_str))
        .map(str::to_string);
    pages
        .iter()
        .filter(|p| {
            p.get("uri")
                .and_then(Value::as_str)
                .is_some_and(|u| !u.is_empty() && Some(u) != current.as_deref())
        })
        .map(|p| {
            if p.get("num").and_then(Value::as_i64) == Some(1) {
                format!("{base}/")
            } else {
                let uri = p.get("uri").and_then(Value::as_str).unwrap_or_default();
                format!("{base}/{uri}/")
            }
        })
        .collect()
}

/// Select the manifest entry for the requested URL: last non-empty path
/// segment == `pages[].uri`; a request ending at the mag segment itself is
/// the home page (`num == 1`).
fn current_page<'a>(mag: &'a Value, mag_uri: &str, url: &str) -> Option<&'a Value> {
    let pages = mag.get("pages")?.as_array()?;
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed
        .path_segments()?
        .filter(|s| !s.is_empty())
        .next_back()?
        .to_string();
    if last == mag_uri {
        return pages
            .iter()
            .find(|p| p.get("num").and_then(Value::as_i64) == Some(1));
    }
    pages
        .iter()
        .find(|p| p.get("uri").and_then(Value::as_str) == Some(last.as_str()))
}

/// The pre-rendered snapshot URL for the requested page, if the manifest
/// carries one within the mag's own scope (URL must contain the mag's
/// identity segment — never follow a pointer out of the account).
pub(crate) fn snapshot_request(html: &str, url: &str) -> Option<String> {
    if !is_readymag(html) {
        return None;
    }
    let mag = mag_from(html, url)?;
    let (mag_uri, _) = mag_base(&mag, url)?;
    let page = current_page(&mag, &mag_uri, url)?;
    let snapshot = page.get("htmlUrl")?.as_str()?;
    let snap = url::Url::parse(snapshot).ok()?;
    let snap_host = snap.host_str()?.to_ascii_lowercase();
    let page_host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    // Same-account scope (design ruling #3): the pointer must stay on the
    // page's own host or the platform CDN, AND carry the mag's identity as
    // a real path segment. A hostile page controls the JSON, so a bare
    // substring test would follow it anywhere.
    let host_ok =
        snap_host == page_host || snap_host == "rmcdn.net" || snap_host.ends_with(".rmcdn.net");
    let seg_ok = snap
        .path_segments()
        .is_some_and(|mut segs| segs.any(|s| s == mag_uri));
    (host_ok && seg_ok).then(|| snapshot.to_string())
}

/// Readymag entry point for the import engine. Body comes from the page's
/// pre-rendered snapshot (fetched by the crawl loop); without one there is
/// nothing to extract — return `None` so the generic scorer takes over.
pub(crate) fn extract(html: &str, snapshot: Option<&str>, url: &str) -> Option<SiteContent> {
    if !is_readymag(html) {
        return None;
    }
    let mag = mag_from(html, url)?;
    let (mag_uri, _) = mag_base(&mag, url)?;
    let page = current_page(&mag, &mag_uri, url)?;
    let blocks = walk_snapshot(snapshot?, &mag_uri);
    let markdown = emit::emit_markdown(&blocks);
    if markdown.trim().is_empty() {
        return None;
    }
    // The home page's own title is just "Home"; the mag title names the
    // site's subject and is the right frontmatter title there.
    let is_home = page.get("num").and_then(Value::as_i64) == Some(1);
    let title = if is_home {
        mag.get("title").and_then(Value::as_str)
    } else {
        page.get("title").and_then(Value::as_str)
    }
    .map(|t| t.trim().to_string())
    .filter(|t| !t.is_empty());
    Some(SiteContent {
        body: AdapterBody::Markdown(markdown),
        overrides: MetadataOverrides {
            title,
            date: None,
            publisher: None,
            author: None,
        },
    })
}

/// One walked widget: a text column carries its blocks with char weights
/// (for proportional interleave); everything else is a single block.
enum Widget {
    TextColumn(Vec<(Block, usize)>),
    Media(Block),
}

/// Walk a snapshot's widgets into reading-ordered blocks: parse geometry,
/// sort, then interleave each media widget into the text column whose
/// vertical span contains it (side media sits beside its passage); media
/// outside any column stays standalone at its ordered position.
fn walk_snapshot(snapshot: &str, mag_uri: &str) -> Vec<Block> {
    let doc = Html::parse_document(snapshot);
    let widget_sel = Selector::parse(".rmwidget").expect("static selector");
    let mut widgets: Vec<Positioned<Widget>> = Vec::new();
    for el in doc.select(&widget_sel) {
        if let Some(widget) = walk_widget(el, mag_uri) {
            let (top, left, height) = px_geometry(el);
            widgets.push(Positioned {
                top,
                left,
                height,
                item: widget,
            });
        }
    }
    let ordered = geometry::order(widgets);

    // Text-column vertical spans, keyed by ordered position. Ownership
    // below is vertical-only (media.top inside the span; first span wins)
    // — right for the single-column pages seen so far; side-by-side
    // columns would need left-overlap too. Known residual.
    let spans: Vec<(usize, f64, f64)> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, w)| match &w.item {
            Widget::TextColumn(_) if w.height > 0.0 && w.top.is_finite() => {
                Some((i, w.top, w.height))
            }
            _ => None,
        })
        .collect();

    enum Entry {
        Column {
            blocks: Vec<(Block, usize)>,
            top: f64,
            height: f64,
        },
        Single(Block),
        Consumed,
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(ordered.len());
    let mut column_media: std::collections::BTreeMap<usize, Vec<Positioned<Block>>> =
        std::collections::BTreeMap::new();
    for w in ordered {
        match w.item {
            Widget::TextColumn(blocks) => entries.push(Entry::Column {
                blocks,
                top: w.top,
                height: w.height,
            }),
            Widget::Media(block) => {
                let owner = spans
                    .iter()
                    .find(|(_, top, height)| w.top >= *top && w.top < top + height);
                match owner {
                    Some((i, _, _)) => {
                        column_media.entry(*i).or_default().push(Positioned {
                            top: w.top,
                            left: w.left,
                            height: w.height,
                            item: block,
                        });
                        entries.push(Entry::Consumed);
                    }
                    None => entries.push(Entry::Single(block)),
                }
            }
        }
    }

    let mut out = Vec::new();
    for (i, entry) in entries.into_iter().enumerate() {
        match entry {
            Entry::Column {
                blocks,
                top,
                height,
            } => {
                let media = column_media.remove(&i).unwrap_or_default();
                out.extend(geometry::interleave_into_text(blocks, top, height, media));
            }
            Entry::Single(block) => out.push(block),
            Entry::Consumed => {}
        }
    }
    out
}

/// A px style property (`top: 85px`) from an inline style string; `None`
/// for percent values or absence. Property-name matches require a boundary
/// so `top` never reads `margin-top`, nor `height` `line-height`.
fn style_px(style: &str, prop: &str) -> Option<f64> {
    let bytes = style.as_bytes();
    for (at, matched) in style.match_indices(prop) {
        let boundary = at == 0 || {
            let c = bytes[at - 1] as char;
            c == ';' || c.is_whitespace()
        };
        if !boundary {
            continue;
        }
        let Some(after) = style.get(at + matched.len()..).map(str::trim_start) else {
            continue;
        };
        let Some(v) = after.strip_prefix(':') else {
            continue;
        };
        let v = v.trim_start();
        let number_end = v
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(v.len());
        if let (Some(number), Some(unit)) = (v.get(..number_end), v.get(number_end..)) {
            if unit.starts_with("px") {
                if let Ok(n) = number.parse::<f64>() {
                    return Some(n);
                }
            }
        }
        // The real property with a non-px value (percent) — no fallback.
        return None;
    }
    None
}

/// The widget's canvas position: its own `left/top/height` px composed
/// with every positioned wrapper above it — animated widgets carry
/// `left:0; top:0` themselves while the real offset sits on their
/// `animation-container` parent. Percent-positioned or unpositioned
/// widgets (fixed header chrome) sort into the leading header zone,
/// keeping DOM order among themselves.
fn px_geometry(el: ElementRef) -> (f64, f64, f64) {
    let style = el.value().attr("style").unwrap_or("");
    let height = style_px(style, "height").unwrap_or(0.0);
    let (Some(mut top), Some(mut left)) = (style_px(style, "top"), {
        Some(style_px(style, "left").unwrap_or(0.0))
    }) else {
        return (f64::NEG_INFINITY, 0.0, height);
    };
    for node in el.ancestors() {
        let Some(anc) = ElementRef::wrap(node) else {
            continue;
        };
        if !anc.value().classes().any(|c| c == "animation-container") {
            continue;
        }
        let anc_style = anc.value().attr("style").unwrap_or("");
        top += style_px(anc_style, "top").unwrap_or(0.0);
        left += style_px(anc_style, "left").unwrap_or(0.0);
    }
    (top, left, height)
}

fn walk_widget(el: ElementRef, mag_uri: &str) -> Option<Widget> {
    let classes: Vec<&str> = el.value().classes().collect();
    if classes.contains(&"widget-text-v3") {
        return walk_text_widget(el, mag_uri);
    }
    if classes.contains(&"widget-picture") {
        return picture_block(el).map(Widget::Media);
    }
    // widget-iframe before the generic `video` class: an iframe-carrying
    // widget must never be starved by the posterless video path.
    if classes.contains(&"widget-iframe") {
        return iframe_block(el).map(Widget::Media);
    }
    // The video widget renders as a provider lightbox: only the poster
    // image is in the static DOM, and it names the video.
    if classes.contains(&"video") {
        return video_block(el).map(Widget::Media);
    }
    if classes.contains(&"widget-background") {
        return background_block(el).map(Widget::Media);
    }
    // widget-shape and unknown widget chrome: decorative, skip.
    None
}

/// Video widget → the video its poster thumbnail names; an unrecognized
/// poster still imports as an image rather than vanishing.
fn video_block(el: ElementRef) -> Option<Block> {
    let img_sel = Selector::parse("img.poster, img").expect("static selector");
    let Some(img) = el.select(&img_sel).next() else {
        log::warn!("readymag import: video widget without a poster image; nothing to import");
        return None;
    };
    let src = img.value().attr("src")?.to_string();
    if let Some(watch) = media::youtube_watch_from_thumb(&src) {
        return Some(Block::Video { url: watch });
    }
    log::warn!("readymag import: video poster not recognized as a provider, kept as image: {src}");
    Some(Block::Image {
        src,
        alt: String::new(),
    })
}

/// A text widget → its block children with char weights. Widgets whose
/// entire text lives inside internal-nav anchors (`/{mag_uri}/…`) are the
/// shared page menu — chrome, skipped.
fn walk_text_widget(el: ElementRef, mag_uri: &str) -> Option<Widget> {
    let total: String = el.text().collect::<String>().split_whitespace().collect();
    if total.is_empty() {
        return None;
    }
    let anchor_sel = Selector::parse("a").expect("static selector");
    let nav_marker = format!("/{mag_uri}/");
    let anchor_text: String = el
        .select(&anchor_sel)
        .filter(|a| a.value().attr("href").is_some_and(|h| h.contains(&nav_marker)))
        .flat_map(|a| a.text())
        .collect::<String>()
        .split_whitespace()
        .collect();
    if anchor_text == total {
        return None;
    }
    let viewer_sel = Selector::parse(".text-viewer").expect("static selector");
    let container = el.select(&viewer_sel).next().unwrap_or(el);
    let mut blocks: Vec<(Block, usize)> = Vec::new();
    for child in container.children().filter_map(ElementRef::wrap) {
        let text_len = child.text().map(|t| t.chars().count()).sum::<usize>();
        let html = child.html();
        if html.trim().is_empty() {
            continue;
        }
        blocks.push((Block::RichText { html }, text_len.max(1)));
    }
    if blocks.is_empty() {
        blocks.push((
            Block::RichText {
                html: container.inner_html(),
            },
            1,
        ));
    }
    Some(Widget::TextColumn(blocks))
}

/// Picture widget → Video when the image is a provider lightbox thumbnail,
/// else Image with any CDN resize transform stripped for the original.
fn picture_block(el: ElementRef) -> Option<Block> {
    let img_sel = Selector::parse("img").expect("static selector");
    let (src, alt) = match el.select(&img_sel).next() {
        Some(img) => (
            img.value().attr("src")?.to_string(),
            img.value().attr("alt").unwrap_or("").to_string(),
        ),
        None => (background_image_url(el)?, String::new()),
    };
    if let Some(watch) = media::youtube_watch_from_thumb(&src) {
        return Some(Block::Video { url: watch });
    }
    let src = media::strip_query_transform(&src).unwrap_or(src);
    Some(Block::Image { src, alt })
}

/// Iframe widget → the file or provider it carries. Never silently
/// dropped: unrecognized frames become a visible link plus a warning.
fn iframe_block(el: ElementRef) -> Option<Block> {
    let iframe_sel = Selector::parse("iframe").expect("static selector");
    let src = el.select(&iframe_sel).next()?.value().attr("src")?;
    let src = if src.starts_with("//") {
        format!("https:{src}")
    } else {
        src.to_string()
    };
    if let Some(download) = media::drive_download_from_preview(&src) {
        return Some(Block::Audio { src: download });
    }
    if src.contains("youtube.com/embed/") || src.contains("player.vimeo.com/video/") {
        return Some(Block::Video { url: src });
    }
    log::warn!("readymag import: unrecognized iframe kept as link: {src}");
    Some(Block::RichText {
        html: format!(r#"<p><a href="{src}">{src}</a></p>"#),
    })
}

/// Background widget → its image, when it has one. A slideshow background
/// only carries its first slide in the static snapshot — captured, with a
/// warning for the rest (viewer-JS loads them from an endpoint static
/// import cannot reach).
fn background_block(el: ElementRef) -> Option<Block> {
    let slide_sel = Selector::parse(".slideshow-image").expect("static selector");
    if let Some(slide) = el.select(&slide_sel).next() {
        let src = background_image_url(slide)?;
        log::warn!(
            "readymag import: slideshow background — only the first slide exists in the static \
             snapshot; remaining slides are not importable"
        );
        return Some(Block::Image {
            src,
            alt: String::new(),
        });
    }
    background_image_url(el).map(|src| Block::Image {
        src,
        alt: String::new(),
    })
}

/// `background-image: url("…")` out of an element's inline style.
fn background_image_url(el: ElementRef) -> Option<String> {
    let style = el.value().attr("style")?;
    let (_, after_prop) = style.split_once("background-image")?;
    let (_, after_open) = after_prop.split_once("url(")?;
    let (inner, _) = after_open.split_once(')')?;
    let url = inner
        .trim_matches(|c| c == '"' || c == '\'' || c == ' ')
        .replace("&quot;", "");
    (!url.is_empty()).then_some(url)
}

#[cfg(test)]
#[path = "readymag_tests.rs"]
mod tests;
