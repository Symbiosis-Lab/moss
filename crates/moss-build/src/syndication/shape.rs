//! A post turned into the copy one network takes: its form, and the text of
//! each part already fitted to the network's limits.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::channel::{ChannelSpec, Form};
use crate::build::scan::article_map::ArticleInfo;

/// One post of a copy. A thread has several, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    /// Ready to post: plain text, links as their absolute addresses.
    pub text: String,
    /// Every address in `text`, in order, for networks that mark links up.
    #[serde(default)]
    pub links: Vec<String>,
}

/// What a link card to the original needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Teaser {
    pub title: String,
    pub description: String,
    pub url: String,
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub title: String,
    pub html: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Absolute, like a teaser's.
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shaped {
    pub form: Form,
    /// Upload attachments for plain copies; long articles own images in HTML.
    #[serde(default)]
    pub pictures: Vec<super::pictures::Picture>,
    #[serde(default)]
    pub omitted_pictures: usize,
    #[serde(default)]
    pub included_pictures: usize,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub teaser: Option<Teaser>,
    #[serde(default)]
    pub article: Option<Article>,
}

impl Shaped {
    /// Over the shaped copy, not the file, so writing `syndicated:` back or an
    /// edit the copy does not show changes nothing.
    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        hex(&Sha256::digest(&bytes)[..16])
    }
}

/// The same for every send of one post to one channel, on every computer, so a
/// network that honours it can never make two copies.
pub fn idempotency_key(uid: &str, channel: &str) -> String {
    hex(&Sha256::digest(format!("moss-copy:{uid}:{channel}").as_bytes())[..16])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A page's absolute address on the site at `site_url`.
pub fn page_url(site_url: &str, url_path: &str) -> String {
    format!("{}/{}", site_url.trim_end_matches('/'), moss_core::resolve::fuzzy_path::percent_encode_url(url_path.trim_start_matches('/')))
}

/// Settings shared by previews and sends. Without a built output, math stays text.
#[derive(Debug, Clone)]
pub struct ShapeOptions {
    pub link_back: bool,
    pub site_name: Option<String>,
    pub lang: Option<String>,
    pub output: Option<std::path::PathBuf>,
    pub math_enabled: bool,
}

impl Default for ShapeOptions {
    fn default() -> Self {
        Self { link_back: true, site_name: None, lang: None, output: None, math_enabled: true }
    }
}

impl ShapeOptions {
    /// The build's rendered identity and assets, shared by planning, previews and sends.
    /// A saved publication's language takes precedence over the latest map.
    pub fn new(config: &crate::config::ConfigFile, folder: &std::path::Path, map: &crate::build::scan::article_map::ArticleMap, post: &ArticleInfo, channel: &str, published_lang: Option<&str>) -> Self {
        let lang = published_lang.map(str::to_string)
            .or_else(|| super::record::effective_language(post, Some(map), config.site_lang()).map(|l| l.into_owned()));
        let site_name = map.page_site_names.get(&post.url_path).filter(|name| !name.trim().is_empty()).cloned();
        Self {
            link_back: config.syndication_link_back(channel),
            site_name,
            lang,
            output: Some(crate::moss_paths::MossPaths::new(folder).initial_serve_dir()),
            math_enabled: config.site_bool("math").unwrap_or(true),
        }
    }
}

/// Choose the first declared form that fits, independently of the page's title.
pub fn shape_with(post: &ArticleInfo, spec: &ChannelSpec, site_url: &str, opts: &ShapeOptions) -> Result<Shaped, String> {
    let page = page_url(site_url, &post.url_path);
    let mut whole = lower(&post.content, &page, site_url);
    own_title(&mut whole, post);
    if opts.link_back && !site_url.is_empty() { link_back(&mut whole, &page); }
    let mut included_pictures = 0;
    let mut omitted_pictures = 0;
    let (form, parts, teaser, article) = if spec.takes(Form::Short) && spec.fits(&whole.text) {
        (Form::Short, vec![whole], None, None)
    } else {
        let mut parts: Vec<Part> = split_parts(&post.content).iter().map(|md| lower(md, &page, site_url)).collect();
        if let Some(first) = parts.first_mut() { own_title(first, post); }
        if opts.link_back && !site_url.is_empty() {
            if let Some(last) = parts.last_mut() { link_back(last, &page); }
        }
        if spec.takes(Form::Thread) && parts.len() > 1 && parts.iter().all(|p| spec.fits(&p.text)) {
            (Form::Thread, parts, None, None)
        } else if spec.takes(Form::Long) {
            let (mut html, included, omitted) = super::pictures::article_html(post, site_url, opts, spec.max_article_images());
            included_pictures = included;
            omitted_pictures = omitted;
            if opts.link_back {
                let escape = crate::build::features::html_escape;
                let host = url::Url::parse(site_url).ok().and_then(|url| url.host_str().map(str::to_string));
                let name = escape(opts.site_name.as_deref().or(host.as_deref()).unwrap_or(&page));
                let url = escape(&page);
                let lang = super::record::effective_language(post, None, opts.lang.as_deref());
                let (prefix, stop) = footer(lang.as_deref().unwrap_or(""));
                html.push_str(&format!("\n<p>{prefix} <a href=\"{url}\">{name}</a>{stop}</p>"));
            }
            (Form::Long, Vec::new(), None, Some(Article { title: post.title.clone(), html, description: described(post), cover: cover(post, site_url) }))
        } else if spec.takes(Form::Teaser) {
            let (teaser, part) = teaser(post, spec, &page, site_url)?;
            (Form::Teaser, vec![part], Some(teaser), None)
        } else {
            return Err("the whole page, including its title and link back, fits none of this channel's declared forms".into());
        }
    };
    let pictures = if form == Form::Long { Vec::new() } else {
        let (pictures, omitted) = super::pictures::collect(post, spec, site_url, opts);
        included_pictures = pictures.len();
        omitted_pictures = omitted;
        pictures
    };
    Ok(Shaped { form, parts, teaser, article, pictures, included_pictures, omitted_pictures })
}

/// Assess with the same selector as sends. An unknown publish address cannot
/// prove a link fits: omit its footer for now and mark previews provisional.
pub(super) fn assess(post: &ArticleInfo, spec: &ChannelSpec, site: Option<&str>, opts: &ShapeOptions, record: &super::record::CopyRecord) -> Result<Shaped, String> {
    if let Some(site) = site.filter(|s| !s.is_empty()) { return shape_record(post, spec, site, opts, record); }
    shape_record(post, spec, "", opts, record)
}

/// Shape an existing copy only in its original form; uncertainty keeps it intact.
pub fn shape_record(post: &ArticleInfo, spec: &ChannelSpec, site: &str, opts: &ShapeOptions, record: &super::record::CopyRecord) -> Result<Shaped, String> {
    let existing = record.remote.is_some() || record.form.is_some();
    if !existing { return shape_with(post, spec, site, opts); }
    let form = record.form.ok_or_else(|| "retaining the prior version: its original form was not recorded".to_string())?;
    let count = if record.remote.is_some() {
        (!record.parts.is_empty()).then_some(record.parts.len() + 1)
    } else { record.part_count };
    if !spec.takes(form) {
        return Err("retaining the prior version: the channel no longer accepts its original form".into());
    }
    let mut fixed = spec.clone();
    fixed.forms = vec![form];
    let copy = shape_with(post, &fixed, site, opts)
        .map_err(|e| format!("retaining the prior version in {form:?} form: {e}"))?;
    if form == Form::Thread && count != Some(copy.parts.len()) {
        return Err("retaining the prior version: the thread's number of parts changed or is unknown".into());
    }
    Ok(copy)
}

fn own_title(part: &mut Part, post: &ArticleInfo) {
    if !post.untitled && !post.title.trim().is_empty() {
        part.text = format!("{}\n{}", post.title.trim(), part.text).trim_end().to_string();
        part.links = addresses(&part.text);
    }
}

/// Include the original in the posted text, before checking network limits.
fn link_back(part: &mut Part, page_url: &str) {
    if !part.text.trim_end().ends_with(page_url) {
        end_paragraph(&mut part.text);
        part.text.push_str(page_url);
        if !part.links.iter().any(|link| link == page_url) { part.links.push(page_url.to_string()); }
    }
}

fn footer(lang: &str) -> (&'static str, &'static str) {
    if crate::i18n::Language::from_code(lang) == Some(crate::i18n::Language::ZhHans) {
        ("本文首发于", "。")
    } else { ("Originally published on", ".") }
}

/// The parts of a post written as a thread: split at each line holding only
/// `---` with a blank line on both sides. Without the blank line above it
/// markdown reads `---` as underlining a heading, so that is not a split.
fn split_parts(markdown: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser};
    let mut start = 0;
    let mut parts = Vec::new();
    for (event, range) in Parser::new(markdown).into_offset_iter() {
        if matches!(event, Event::Rule) {
            let rule = markdown.get(range.clone()).unwrap_or_default();
            let before = markdown.get(..range.start).unwrap_or_default();
            let after = markdown.get(range.end..).unwrap_or_default();
            let blank = |line: Option<&str>| line.is_none_or(|s| s.trim().is_empty());
            if rule.trim() == "---" && blank(before.lines().next_back()) && blank(after.lines().next()) {
                parts.push(markdown.get(start..range.start).unwrap_or_default().to_string());
                start = range.end;
            }
        }
    }
    parts.push(markdown.get(start..).unwrap_or_default().to_string());
    parts.retain(|p| !p.trim().is_empty());
    if parts.is_empty() { parts.push(String::new()); }
    parts
}

/// Markdown as the plain text a network posts: emphasis dropped, a link as
/// `words (address)` or the bare address, pictures left out (a copy carries
/// pictures separately), paragraphs separated by a blank line.
fn lower(markdown: &str, page_url: &str, site_url: &str) -> Part {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut text = String::new();
    let mut link: Option<(String, usize)> = None;
    let mut in_image = 0usize;
    let mut list_depth = 0usize;
    let options = pulldown_cmark::Options::ENABLE_MATH | pulldown_cmark::Options::ENABLE_WIKILINKS;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
            _ if in_image > 0 => {}
            // A wiki link names a page by title, not address: its words stand alone.
            Event::Start(Tag::Link { link_type: pulldown_cmark::LinkType::WikiLink { .. }, .. }) => {}
            Event::Start(Tag::Link { dest_url, .. }) => link = Some((absolute(&dest_url, page_url, site_url), text.len())),
            Event::End(TagEnd::Link) => {
                if let Some((url, start)) = link.take() {
                    let words = text[start..].trim().to_string();
                    text.truncate(start);
                    if words.is_empty() || words == url || url.trim_end_matches('/') == words.trim_end_matches('/') {
                        text.push_str(&url);
                    } else {
                        text.push_str(&format!("{words} ({url})"));
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => text.push_str(&t),
            // A network shows an equation as the source the author wrote.
            Event::InlineMath(t) => text.push_str(&format!("${t}$")),
            Event::DisplayMath(t) => text.push_str(&format!("$${t}$$")),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => {
                end_line(&mut text);
                text.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                text.push_str("- ");
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::CodeBlock) if list_depth == 0 => {
                end_paragraph(&mut text)
            }
            Event::End(TagEnd::Item) => end_line(&mut text),
            _ => {}
        }
    }
    let text = text.trim().to_string();
    let links = addresses(&text);
    Part { text, links }
}

/// Every web address in the text, in order: the ones a link spelled out and
/// the ones written bare.
fn addresses(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'', '。', '，']).to_string())
        .collect()
}

fn end_line(text: &mut String) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
}

fn end_paragraph(text: &mut String) {
    let trimmed = text.trim_end_matches([' ', '\n']).len();
    text.truncate(trimmed);
    if !text.is_empty() {
        text.push_str("\n\n");
    }
}

/// A link's address as a reader elsewhere can follow it.
pub(super) fn absolute(dest: &str, page_url: &str, site_url: &str) -> String {
    if dest.contains("://") || dest.starts_with("mailto:") {
        return dest.to_string();
    }
    if let Some(rest) = dest.strip_prefix('/') {
        return format!("{}/{rest}", site_url.trim_end_matches('/'));
    }
    if dest.starts_with('#') {
        return format!("{page_url}{dest}");
    }
    format!("{}/{}", page_url.trim_end_matches('/'), dest)
}

/// The teaser and link: the title, the description (or the opening, cut at the
/// last sentence end that fits), and the address, together within the limit.
fn teaser(post: &ArticleInfo, spec: &ChannelSpec, page_url: &str, site_url: &str) -> Result<(Teaser, Part), String> {
    let title = if post.untitled || post.title.trim().is_empty() { String::new() } else { post.title.trim().to_string() };
    let description = described(post).unwrap_or_else(|| {
        let first = split_parts(&post.content).into_iter().next().unwrap_or_default();
        lower(&first, page_url, site_url).text.split("\n\n").next().unwrap_or_default().to_string()
    });
    let compose = |desc: &str| {
        [title.as_str(), desc, page_url].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n\n")
    };
    let fitted = fit(&description, |d| {
        // The authored title and opening still obey the limit before a publish
        // address exists. Only the mandatory final address is deferred.
        let known = if site_url.is_empty() {
            [title.as_str(), d].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n")
        } else { compose(d) };
        spec.fits(&known)
    })
        .ok_or_else(|| "the post's title and address alone are longer than this network allows".to_string())?;
    let cover = cover(post, site_url);
    let text = compose(&fitted);
    let part = Part { links: addresses(&text), text };
    Ok((Teaser { title, description: fitted, url: page_url.to_string(), cover }, part))
}

fn described(post: &ArticleInfo) -> Option<String> {
    post.frontmatter.get("description").and_then(|d| d.as_str()).map(str::to_string)
}

/// The scan has already resolved a cover to a path from the site's root.
fn cover(post: &ArticleInfo, site_url: &str) -> Option<String> {
    let c = post.frontmatter.get("cover")?.as_str()?;
    Some(if c.contains("://") { c.to_string() } else { page_url(site_url, c) })
}

/// The longest prefix of `text` that `fits` and ends a sentence; else the
/// longest that ends a word, with an ellipsis; else nothing at all.
fn fit(text: &str, fits: impl Fn(&str) -> bool) -> Option<String> {
    let text = text.trim();
    if fits(text) {
        return Some(text.to_string());
    }
    let ends = |pred: &dyn Fn(char) -> bool| -> Vec<usize> {
        text.char_indices().filter(|(_, c)| pred(*c)).map(|(i, c)| i + c.len_utf8()).collect()
    };
    let sentence_ends = ends(&|c| matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '…'));
    for end in sentence_ends.into_iter().rev() {
        if fits(&text[..end]) {
            return Some(text[..end].trim().to_string());
        }
    }
    let mut cut: Vec<usize> = ends(&|c| c.is_whitespace() || is_wide(c));
    cut.reverse();
    for end in cut {
        let candidate = format!("{}…", text[..end].trim_end());
        if fits(&candidate) {
            return Some(candidate);
        }
    }
    fits("").then(String::new)
}

/// A character after which a line may break without a space (CJK).
fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
