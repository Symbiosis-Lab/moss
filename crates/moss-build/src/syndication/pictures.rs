//! Pictures shared by article HTML and plain-text copies. Formula addresses
//! are advertised only when the normal build emitted their PNGs.

use serde::{Deserialize, Serialize};
use super::channel::ChannelSpec;
use super::shape::{absolute, page_url, ShapeOptions};
use crate::build::scan::article_map::ArticleInfo;
use crate::build::emit::math_png::swap_math_svgs_for_email_imgs;

/// A picture for the channel to upload before publishing the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    /// Absolute source address; the channel uploads these bytes to its network.
    pub url: String,
    pub alt: String,
}

fn available(opts: &ShapeOptions, path: &str) -> bool {
    opts.math_enabled && opts.output.as_ref().is_some_and(|dir| dir.join(path.trim_start_matches('/')).is_file())
}

/// Reuse the newsletter projection, retaining TeX when its PNG is unavailable.
pub(super) fn article_html(post: &ArticleInfo, site: &str, opts: &ShapeOptions, max: usize) -> (String, usize, usize) {
    let html = swap_math_svgs_for_email_imgs(post.html_content.as_deref().unwrap_or_default(), site);
    let page = page_url(site, &post.url_path);
    let mut count = 0;
    let html = lol_html::rewrite_str(&html, lol_html::RewriteStrSettings {
        element_content_handlers: vec![lol_html::element!("img", |el| {
            if let Some(src) = el.get_attribute("src") {
                let alt = el.get_attribute("alt").unwrap_or_default();
                let math_path = src.split_once("/_moss/math/").map(|(_, rest)| format!("_moss/math/{rest}"));
                if math_path.is_some_and(|p| !available(opts, &p)) {
                    el.replace(&crate::build::features::html_escape(&alt), lol_html::html_content::ContentType::Html);
                } else {
                    count += 1;
                    if count > max {
                        el.replace(&crate::build::features::html_escape(&alt), lol_html::html_content::ContentType::Html);
                    } else { el.set_attribute("src", &absolute(&src, &page, site))?; }
                }
            }
            Ok(())
        })],
        ..Default::default()
    }).unwrap_or(html);
    (html, count.min(max), count.saturating_sub(max))
}

pub(super) fn collect(post: &ArticleInfo, spec: &ChannelSpec, site: &str, opts: &ShapeOptions) -> (Vec<Picture>, usize) {
    let mut pictures = Vec::new();
    // Rendered addresses have already resolved wiki embeds and source-relative paths.
    let (html, _, _) = article_html(post, site, opts, usize::MAX);
    let document = scraper::Html::parse_fragment(&html);
    let selector = scraper::Selector::parse("img[src]").expect("fixed selector");
    for img in document.select(&selector) {
        pictures.push(Picture { url: img.value().attr("src").unwrap_or_default().into(), alt: img.value().attr("alt").unwrap_or_default().into() });
    }
    let omitted = pictures.len().saturating_sub(spec.max_images());
    pictures.truncate(spec.max_images());
    (pictures, omitted)
}
