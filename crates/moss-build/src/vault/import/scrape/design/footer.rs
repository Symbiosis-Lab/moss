//! The page's footer as markdown: its paragraphs and links, then one list of
//! its icon-only and social links. Links come out absolute; the writer rewrites the ones
//! that point at pages the crawl wrote.

use std::collections::HashSet;

use scraper::{ElementRef, Html, Node};
use url::Url;

use super::facts::{collapse, norm_url, sel};
use crate::vault::import::widgets::html_to_markdown;

/// Elements that carry no footer text: behaviour, media and form controls.
const DROPPED: &[&str] = &[
    "script", "style", "noscript", "template", "form", "button", "input", "select", "textarea",
    "iframe", "svg", "img", "picture", "video", "audio", "canvas",
];

/// Block elements kept as they are for the converter; `address` is a paragraph.
const BLOCKS: &[&str] = &["p", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6", "div", "address", "section"];

/// Inline emphasis, kept only around some text.
const INLINE: &[&str] = &["strong", "b", "em", "i"];

/// Hosts whose links are a site's social presence. Rows are matched as the
/// whole host or a parent domain of it.
const SOCIAL_HOSTS: &[&str] = &[
    "facebook.com", "instagram.com", "twitter.com", "x.com", "linkedin.com", "youtube.com",
    "vimeo.com", "tiktok.com", "pinterest.com", "github.com", "threads.net", "bsky.app",
    "behance.net", "dribbble.com", "soundcloud.com", "spotify.com", "medium.com", "tumblr.com",
    "flickr.com", "mastodon.social", "weibo.com", "t.me",
];

pub(super) fn read(doc: &Html, base: &Url, primary_nav: &HashSet<String>) -> Option<String> {
    let footer = doc.select(&sel("footer, [role=contentinfo]")).last()?;
    let mut walk = Walk { base, primary_nav, socials: Vec::new() };
    let html = walk.children(footer, false);
    let flow = html_to_markdown(&html).ok()?;
    let mut out = drop_invisible_lines(&flow);
    if !walk.socials.is_empty() {
        let list: Vec<String> =
            walk.socials.iter().map(|(label, url)| format!("- [{label}]({url})")).collect();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&list.join("\n"));
    }
    (!out.is_empty()).then_some(out + "\n")
}

struct Walk<'a> {
    base: &'a Url,
    primary_nav: &'a HashSet<String>,
    /// `(label, url)` in page order, one per URL.
    socials: Vec<(String, String)>,
}

impl Walk<'_> {
    fn children(&mut self, el: ElementRef, in_nav: bool) -> String {
        let mut out = String::new();
        for child in el.children() {
            match child.value() {
                Node::Text(t) => out.push_str(&escape(t)),
                Node::Element(_) => {
                    if let Some(c) = ElementRef::wrap(child) {
                        out.push_str(&self.element(c, in_nav));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn element(&mut self, el: ElementRef, in_nav: bool) -> String {
        let name = el.value().name();
        if DROPPED.contains(&name) {
            return String::new();
        }
        match name {
            "a" => self.anchor(el, in_nav),
            "br" => "<br>".to_string(),
            "nav" => self.children(el, true),
            n if BLOCKS.contains(&n) || INLINE.contains(&n) => {
                let inner = self.children(el, in_nav);
                if inner.trim().is_empty() {
                    return String::new();
                }
                let tag = if n == "address" { "p" } else { n };
                format!("<{tag}>{inner}</{tag}>")
            }
            _ => self.children(el, in_nav),
        }
    }

    fn anchor(&mut self, el: ElementRef, in_nav: bool) -> String {
        let Some(url) = el
            .value()
            .attr("href")
            .map(str::trim)
            .filter(|h| !h.starts_with('#'))
            .and_then(|h| self.base.join(h).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https" | "mailto" | "tel"))
        else {
            return String::new();
        };
        if in_nav && norm_url(url.as_str()).is_some_and(|k| self.primary_nav.contains(&k)) {
            return String::new();
        }
        let text = collapse(&el.text().collect::<String>());
        let host = url.host_str().map(|h| h.trim_start_matches("www.").to_lowercase());
        let is_social = host.as_deref().is_some_and(|h| {
            SOCIAL_HOSTS.iter().any(|s| h == *s || h.ends_with(&format!(".{s}")))
        });
        if text.is_empty() || is_social {
            let label = [text, attr(el, "aria-label"), attr(el, "title")]
                .into_iter()
                .find(|l| !l.is_empty())
                .or(host)
                .unwrap_or_else(|| url.path().to_string());
            let label = label.replace(['[', ']'], " ");
            if !self.socials.iter().any(|(_, u)| u == url.as_str()) {
                self.socials.push((collapse(&label), url.to_string()));
            }
            return String::new();
        }
        format!("<a href=\"{}\">{}</a>", escape(url.as_str()), escape(&text))
    }
}

/// `text` without lines that show nothing (whitespace and zero-width characters
/// only), runs of blank lines folded to one, trimmed.
fn drop_invisible_lines(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let blank = line.chars().all(|c| c.is_whitespace() || matches!(c, '\u{200B}' | '\u{FEFF}'));
        if blank && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(if blank { "" } else { line });
    }
    out.join("\n").trim().to_string()
}

fn attr(el: ElementRef, name: &str) -> String {
    el.value().attr(name).map(collapse).unwrap_or_default()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
