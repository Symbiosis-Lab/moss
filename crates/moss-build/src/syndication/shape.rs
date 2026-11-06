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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shaped {
    pub form: Form,
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
    format!("{}/{}", site_url.trim_end_matches('/'), url_path.trim_start_matches('/'))
}

/// Shape `post` for a channel, linking to it on the site at `site_url`.
pub fn shape(post: &ArticleInfo, spec: &ChannelSpec, site_url: &str) -> Result<Shaped, String> {
    let page_url = &page_url(site_url, &post.url_path);
    let untitled = post.title.trim().is_empty();
    let handoff = spec.takes(Form::Handoff);

    if untitled {
        let parts: Vec<Part> = split_parts(&post.content).into_iter().map(|md| lower(&md, page_url, site_url)).collect();
        if parts.len() == 1 && spec.fits(&parts[0].text) && (spec.takes(Form::Short) || handoff) {
            return Ok(Shaped { form: if handoff { Form::Handoff } else { Form::Short }, parts, teaser: None, article: None });
        }
        if parts.len() > 1 && spec.takes(Form::Thread) && parts.iter().all(|p| spec.fits(&p.text)) {
            return Ok(Shaped { form: Form::Thread, parts, teaser: None, article: None });
        }
        let joined = join(&parts);
        if parts.len() > 1 && spec.fits(&joined.text) && (spec.takes(Form::Short) || handoff) {
            return Ok(Shaped { form: if handoff { Form::Handoff } else { Form::Short }, parts: vec![joined], teaser: None, article: None });
        }
    } else if spec.takes(Form::Long) {
        let article = Article { title: post.title.clone(), html: post.html_content.clone().unwrap_or_default() };
        return Ok(Shaped { form: Form::Long, parts: Vec::new(), teaser: None, article: Some(article) });
    }

    if !(spec.takes(Form::Teaser) || handoff) {
        return Err("too long for this network, which takes no teaser and link".to_string());
    }
    let (teaser, part) = teaser(post, spec, page_url, site_url)?;
    Ok(Shaped { form: if handoff { Form::Handoff } else { Form::Teaser }, parts: vec![part], teaser: Some(teaser), article: None })
}

/// The parts of a post written as a thread: split at each line holding only
/// `---` with a blank line on both sides. Without the blank line above it
/// markdown reads `---` as underlining a heading, so that is not a split.
fn split_parts(markdown: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let lines: Vec<&str> = markdown.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let blank = |j: Option<&&str>| j.is_none_or(|l| l.trim().is_empty());
        let is_break = line.trim() == "---" && blank(i.checked_sub(1).and_then(|j| lines.get(j))) && blank(lines.get(i + 1));
        if is_break {
            parts.push(String::new());
        } else {
            let current = parts.last_mut().expect("parts starts non-empty");
            current.push_str(line);
            current.push('\n');
        }
    }
    parts.retain(|p| !p.trim().is_empty());
    if parts.is_empty() {
        parts.push(String::new());
    }
    parts
}

fn join(parts: &[Part]) -> Part {
    Part {
        text: parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n\n"),
        links: parts.iter().flat_map(|p| p.links.iter().cloned()).collect(),
    }
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
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Image { .. }) => in_image += 1,
            Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
            _ if in_image > 0 => {}
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
fn absolute(dest: &str, page_url: &str, site_url: &str) -> String {
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
    let title = if post.title.trim().is_empty() { String::new() } else { post.title.trim().to_string() };
    let description = post
        .frontmatter
        .get("description")
        .and_then(|d| d.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let first = split_parts(&post.content).into_iter().next().unwrap_or_default();
            lower(&first, page_url, site_url).text.split("\n\n").next().unwrap_or_default().to_string()
        });
    let compose = |desc: &str| {
        [title.as_str(), desc, page_url].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n\n")
    };
    let fitted = fit(&description, |d| spec.fits(&compose(d)))
        .ok_or_else(|| "the post's title and address alone are longer than this network allows".to_string())?;
    let cover = post.frontmatter.get("cover").and_then(|c| c.as_str()).map(|c| absolute(c, page_url, site_url));
    let text = compose(&fitted);
    let part = Part { links: addresses(&text), text };
    Ok((Teaser { title, description: fitted, url: page_url.to_string(), cover }, part))
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
