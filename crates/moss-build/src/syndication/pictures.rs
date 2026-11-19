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
        element_content_handlers: vec![lol_html::element!("a[href]", |el| {
            // A link to another page, a footnote or a heading leaves the site with the copy.
            if let Some(href) = el.get_attribute("href") { el.set_attribute("href", &absolute(&href, &page, site))?; }
            Ok(())
        }), lol_html::element!("img", |el| {
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

/// One piece of an article in reading order: a run of HTML, or a picture
/// between runs. For an editor that takes pictures only as files, pasted
/// one by one at their places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Block {
    Html { html: String },
    Picture { picture: Picture },
}

/// The article as an editor is given it: what is not the article's own text
/// removed (head elements, a leading h1 that repeats the title, footnote
/// back-links), embeds as links to their addresses, a heading over the notes,
/// and split at each picture. Formula pictures stay inline, inside their text.
pub(super) fn blocks(html: &str, title: &str) -> Vec<Block> {
    use lol_html::html_content::ContentType;
    use crate::build::features::html_escape;
    // An embed's address, in document order: its own `src`, or its first source's.
    let document = scraper::Html::parse_fragment(html);
    let media = scraper::Selector::parse("iframe, video, audio").expect("fixed selector");
    let source = scraper::Selector::parse("source[src]").expect("fixed selector");
    let mut embeds = document.select(&media).map(|el| {
        let src = el.value().attr("src").map(str::to_string).or_else(|| el.select(&source).next().and_then(|s| s.value().attr("src")).map(str::to_string));
        let label = el.value().attr("title").map(str::to_string).filter(|t| !t.trim().is_empty());
        (src, label)
    }).collect::<Vec<_>>().into_iter();
    let cleaned = lol_html::rewrite_str(html, lol_html::RewriteStrSettings {
        element_content_handlers: vec![
            lol_html::element!("meta, base, link, script, style, a.moss-footnote-backref", |el| { el.remove(); Ok(()) }),
            lol_html::element!("section.moss-footnotes", |el| { el.before("<h2>Notes</h2>", ContentType::Html); Ok(()) }),
            lol_html::element!("iframe, video, audio", |el| {
                match embeds.next() {
                    Some((Some(src), label)) => {
                        let label = label.unwrap_or_else(|| src.clone());
                        el.replace(&format!("<a href=\"{}\">{}</a>", html_escape(&src), html_escape(&label)), ContentType::Html);
                    }
                    _ => el.remove(),
                }
                Ok(())
            }),
        ],
        ..Default::default()
    }).unwrap_or_else(|_| html.to_string());

    let document = scraper::Html::parse_fragment(&cleaned);
    let pictures = scraper::Selector::parse("img[src]").expect("fixed selector");
    let is_formula = |src: &str| src.contains("/_moss/math/");
    let mut out: Vec<Block> = Vec::new();
    let mut push_html = |out: &mut Vec<Block>, piece: &str| {
        if piece.trim().is_empty() { return; }
        match out.last_mut() {
            Some(Block::Html { html }) => html.push_str(piece),
            _ => out.push(Block::Html { html: piece.to_string() }),
        }
    };
    let mut first_element = true;
    for node in document.root_element().children() {
        let Some(el) = scraper::ElementRef::wrap(node) else {
            if let Some(text) = node.value().as_text() { push_html(&mut out, &html_escape(text)); }
            continue;
        };
        let leading = std::mem::take(&mut first_element);
        if leading && el.value().name() == "h1" && el.text().collect::<String>().trim() == title.trim() { continue; }
        // An embed that was a block of its own is now a bare link: give it its paragraph.
        let outer = el.html();
        let outer = if el.value().name() == "a" { format!("<p>{outer}</p>") } else { outer };
        let found: Vec<Picture> = (if el.value().name() == "img" { vec![el] } else { el.select(&pictures).collect() })
            .into_iter()
            .filter_map(|img| img.value().attr("src").filter(|s| !is_formula(s)).map(|src| Picture { url: src.into(), alt: img.value().attr("alt").unwrap_or_default().into() }))
            .collect();
        if found.is_empty() { push_html(&mut out, &outer); continue; }
        let rest = lol_html::rewrite_str(&outer, lol_html::RewriteStrSettings {
            element_content_handlers: vec![lol_html::element!("img[src]", |img| {
                if !img.get_attribute("src").is_some_and(|s| is_formula(&s)) { img.remove(); }
                Ok(())
            })],
            ..Default::default()
        }).unwrap_or_default();
        let rest_has_text = scraper::Html::parse_fragment(&rest).root_element().text().any(|t| !t.trim().is_empty());
        // A picture that opens its block (a figure, a paragraph of its own) comes before what is left, its caption.
        let text_before = el.text().next().is_some_and(|t| !t.trim().is_empty()) && el.value().name() != "figure";
        let pictures = found.into_iter().map(|picture| Block::Picture { picture });
        if text_before {
            if rest_has_text { push_html(&mut out, &rest); }
            out.extend(pictures);
        } else {
            out.extend(pictures);
            if rest_has_text { push_html(&mut out, &rest); }
        }
    }
    out
}

#[cfg(test)]
#[path = "pictures_tests.rs"]
mod tests;
