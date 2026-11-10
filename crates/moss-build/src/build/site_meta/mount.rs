//! Where a site sits on its host, and the one pass that puts its URLs there.
//!
//! A site served at `https://university.edu/~alice/` has the mount `/~alice/`.
//! Inside moss every site URL stays root-relative to the site (`/notes/`,
//! `/style.css`) and never carries the mount: [`Mount`] is the only code that
//! knows the prefix. It is applied once, to the bytes a generation ships
//! (`ship::apply_transform`), so the stage the preview serves stays mount-free
//! and a page re-read from the stage can never be prefixed twice.
//!
//! The pass prefixes every root-relative URL in emitted HTML (`href`, `src`,
//! `srcset`, `poster`, `action`, `formaction`, `data`, a meta refresh, inline
//! `style` and `<style>`) and in CSS (`url()`, `image-set()` strings,
//! `@import`), and gives `<head>` a `<meta name="moss:base">` that runtime
//! scripts read the mount from. Absolute URLs (canonical, Open Graph, JSON-LD,
//! feeds, the sitemap) already carry the path through `SiteUrl` and are left
//! alone, as are protocol-relative, fragment-only, relative and non-http URLs.
//! At the root mount nothing is parsed and the bytes come back unchanged.

use std::borrow::Cow;

/// The path a site is served under: always starts and ends with `/`; `/` for a
/// site at its origin's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount(String);

impl Default for Mount {
    fn default() -> Self {
        Self::root()
    }
}

impl Mount {
    pub fn root() -> Self {
        Mount("/".to_string())
    }

    /// The path of an absolute site URL (`https://example.edu/~alice` gives
    /// `/~alice/`). Anything after `?` or `#` is not part of it. Bytes that
    /// would need quoting in an HTML attribute or a CSS `url()` are
    /// percent-encoded, so the prefix can be inserted as-is into either.
    pub fn of_site_url(url: &str) -> Self {
        let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
        let path = after_scheme.find('/').and_then(|i| after_scheme.get(i..)).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or("");
        let mut out = String::with_capacity(path.len() + 2);
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            out.push('/');
            for b in segment.bytes() {
                if b.is_ascii_alphanumeric() || b"-._~!$*+,;=:@%".contains(&b) {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
        out.push('/');
        Mount(out)
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The mount without its trailing slash: what goes in front of a
    /// root-relative URL. Empty at the root.
    fn prefix(&self) -> &str {
        self.0.strip_suffix('/').unwrap_or(&self.0)
    }

    /// `/notes/` → `/~alice/notes/`. `None` for anything that is not a
    /// site-root-relative URL, which is left as it is.
    pub fn apply(&self, url: &str) -> Option<String> {
        is_root_relative(url).then(|| format!("{}{}", self.prefix(), url))
    }

    /// `/~alice/notes/` → `/notes/`: what a reader of shipped output uses to
    /// get back to the site's own URL. `None` for a URL outside the mount.
    pub fn strip<'a>(&self, url: &'a str) -> Option<&'a str> {
        if self.is_root() {
            return is_root_relative(url).then_some(url);
        }
        let rest = url.strip_prefix(self.prefix())?;
        match rest.as_bytes().first() {
            Some(b'/') => Some(rest),
            // `/~alice` names the site's home.
            None => Some("/"),
            _ => None,
        }
    }

    /// Mount every root-relative URL in an HTML document or fragment and give
    /// its `<head>` the `moss:base` meta.
    pub fn rewrite_html<'a>(&self, html: &'a str) -> Cow<'a, str> {
        if self.is_root() {
            return Cow::Borrowed(html);
        }
        match rewrite_html(html, self) {
            Ok(out) => Cow::Owned(out),
            Err(e) => {
                log::warn!("could not mount the URLs of an HTML file ({e}); it ships unchanged");
                Cow::Borrowed(html)
            }
        }
    }

    /// Mount every root-relative URL in a stylesheet, splicing into the
    /// original text so nothing else in it changes.
    pub fn rewrite_css<'a>(&self, css: &'a str) -> Cow<'a, str> {
        if self.is_root() {
            return Cow::Borrowed(css);
        }
        let offsets = css_url_offsets(css);
        if offsets.is_empty() {
            return Cow::Borrowed(css);
        }
        Cow::Owned(insert_at(css, &offsets, self.prefix()))
    }
}

/// `/x`, but not `//host/x` or `/\host` (both name another host to a browser).
fn is_root_relative(url: &str) -> bool {
    let b = url.as_bytes();
    b.first() == Some(&b'/') && !matches!(b.get(1), Some(b'/' | b'\\'))
}

fn insert_at(text: &str, offsets: &[usize], insertion: &str) -> String {
    let mut out = String::with_capacity(text.len() + offsets.len() * insertion.len());
    let mut last = 0;
    // Every offset is the byte of an ASCII `/`, so each piece is on a char boundary.
    for &at in offsets {
        out.push_str(text.get(last..at).unwrap_or_default());
        out.push_str(insertion);
        last = at;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out
}

/// The value with the mount inserted at each offset, or `None` when it had
/// none (so an untouched attribute keeps its original quoting).
fn mounted(value: &str, offsets: Vec<usize>, mount: &Mount) -> Option<String> {
    (!offsets.is_empty()).then(|| insert_at(value, &offsets, mount.prefix()))
}

fn url_attr_offsets(value: &str) -> Vec<usize> {
    let start = value.len() - value.trim_start().len();
    if is_root_relative(value.get(start..).unwrap_or_default()) { vec![start] } else { Vec::new() }
}

/// Each candidate URL in a `srcset`: `url [descriptor], url [descriptor]`.
fn srcset_offsets(value: &str) -> Vec<usize> {
    let b = value.as_bytes();
    let mut offsets = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        if value.get(i..).is_some_and(is_root_relative) {
            offsets.push(i);
        }
        while i < b.len() && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        // The descriptor runs to the next comma.
        while i < b.len() && b[i] != b',' {
            i += 1;
        }
    }
    offsets
}

/// `0; url=/x` (any case, optional quotes) in a meta refresh.
fn refresh_offsets(value: &str) -> Vec<usize> {
    let lower = value.to_ascii_lowercase();
    let Some(at) = lower.find("url") else { return Vec::new() };
    let rest = value.get(at + 3..).unwrap_or_default();
    let trimmed = rest.trim_start();
    let Some(after_eq) = trimmed.strip_prefix('=') else { return Vec::new() };
    let after_eq = after_eq.trim_start();
    let after_quote = after_eq.strip_prefix(['"', '\'']).unwrap_or(after_eq);
    let start = value.len() - after_quote.len();
    if is_root_relative(after_quote) { vec![start] } else { Vec::new() }
}

/// Attributes whose whole value is one URL: the HTML ones, then the `data-*`
/// ones moss emits for its own scripts and placeholders to load. Only a
/// root-relative value is touched. `data-path` and `data-page-key` are a
/// page's identity, not a URL to load, and stay as the site wrote them.
const URL_ATTRS: [&str; 24] = [
    "href", "src", "poster", "action", "formaction", "data", "xlink:href",
    "data-placeholder-src", "data-thumb-src", "data-src", "data-article", "data-hls",
    "data-share-card", "data-share-cover", "data-share-qr",
    "data-world", "data-tiles", "data-places", "data-labels", "data-layers", "data-hydrate-url",
    "data-search", "data-link", "data-next",
];

fn rewrite_html(html: &str, mount: &Mount) -> Result<String, lol_html::errors::RewritingError> {
    use lol_html::html_content::ContentType;
    use lol_html::{element, text, HtmlRewriter, Settings};

    let meta = format!(r#"<meta name="moss:base" content="{}">"#, mount.as_str());
    let style_text = std::cell::RefCell::new(String::new());
    let mut out = Vec::with_capacity(html.len() + 256);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("head", |el| {
                    el.prepend(&meta, ContentType::Html);
                    Ok(())
                }),
                element!("*", |el| {
                    for name in URL_ATTRS {
                        if let Some(value) = el.get_attribute(name) {
                            if let Some(new) = mounted(&value, url_attr_offsets(&value), mount) {
                                el.set_attribute(name, &new)?;
                            }
                        }
                    }
                    if let Some(value) = el.get_attribute("srcset") {
                        if let Some(new) = mounted(&value, srcset_offsets(&value), mount) {
                            el.set_attribute("srcset", &new)?;
                        }
                    }
                    if let Some(value) = el.get_attribute("style") {
                        if let Cow::Owned(new) = mount.rewrite_css(&value) {
                            el.set_attribute("style", &new)?;
                        }
                    }
                    // A moss form's landing page after sending, posted as `_next`.
                    if el.tag_name() == "input" && el.get_attribute("name").as_deref() == Some("_next") {
                        if let Some(value) = el.get_attribute("value") {
                            if let Some(new) = mounted(&value, url_attr_offsets(&value), mount) {
                                el.set_attribute("value", &new)?;
                            }
                        }
                    }
                    let refresh = el.tag_name() == "meta"
                        && el.get_attribute("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh"));
                    if refresh {
                        if let Some(value) = el.get_attribute("content") {
                            if let Some(new) = mounted(&value, refresh_offsets(&value), mount) {
                                el.set_attribute("content", &new)?;
                            }
                        }
                    }
                    Ok(())
                }),
                // A `<style>` body arrives in chunks; hold them until the last
                // so a `url(` split across two is still seen whole.
                text!("style", |chunk| {
                    let mut buf = style_text.borrow_mut();
                    buf.push_str(chunk.as_str());
                    if chunk.last_in_text_node() {
                        let css = std::mem::take(&mut *buf);
                        chunk.replace(&mount.rewrite_css(&css), ContentType::Html);
                    } else {
                        chunk.remove();
                    }
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |c: &[u8]| out.extend_from_slice(c),
    );
    rewriter.write(html.as_bytes())?;
    rewriter.end()?;
    Ok(String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// Byte offsets of the leading `/` of every root-relative URL in `css`, in
/// order, found by tokenizing it (comments and strings that merely contain
/// `url(` are never touched).
fn css_url_offsets(css: &str) -> Vec<usize> {
    let mut input = cssparser::ParserInput::new(css);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut offsets = Vec::new();
    walk_css(&mut parser, css, false, &mut offsets);
    offsets
}

/// `in_url_function` is true inside `url(…)`, `image-set(…)` and
/// `-webkit-image-set(…)`, where a quoted string is a URL.
fn walk_css(parser: &mut cssparser::Parser<'_, '_>, css: &str, in_url_function: bool, offsets: &mut Vec<usize>) {
    use cssparser::Token;
    let mut after_import = false;
    loop {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace_and_comments() else { return };
        let token = token.clone();
        match token {
            Token::UnquotedUrl(ref value) if is_root_relative(value) => {
                // `url(` then optional whitespace, then the URL as written.
                let open = start + 4;
                let tail = css.get(open..).unwrap_or_default();
                let at = open + tail.len() - tail.trim_start().len();
                if css.as_bytes().get(at) == Some(&b'/') {
                    offsets.push(at);
                }
            }
            Token::QuotedString(ref value) if (in_url_function || after_import) && is_root_relative(value) => {
                // Only when the string is written literally: an escaped
                // leading slash is too rare to splice around.
                if css.as_bytes().get(start + 1) == Some(&b'/') {
                    offsets.push(start + 1);
                }
            }
            Token::Function(ref name) => {
                let is_url = name.eq_ignore_ascii_case("url")
                    || name.eq_ignore_ascii_case("image-set")
                    || name.eq_ignore_ascii_case("-webkit-image-set");
                let _ = parser.parse_nested_block(|p| -> Result<(), cssparser::ParseError<'_, ()>> {
                    walk_css(p, css, is_url, offsets);
                    Ok(())
                });
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(|p| -> Result<(), cssparser::ParseError<'_, ()>> {
                    walk_css(p, css, false, offsets);
                    Ok(())
                });
            }
            _ => {}
        }
        after_import = match token {
            Token::AtKeyword(ref name) => name.eq_ignore_ascii_case("import"),
            Token::WhiteSpace(_) | Token::Comment(_) => after_import,
            _ => false,
        };
    }
}

#[cfg(test)]
#[path = "mount_tests.rs"]
mod tests;
