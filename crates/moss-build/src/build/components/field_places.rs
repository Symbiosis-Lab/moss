//! A page's declared fields, drawn where their `place` says.
//!
//! The frontmatter holds the fact once; this module turns the typed values
//! into the eyebrow above the title, the subtitle and key figure below it,
//! and the facts list. Text comes from `field_text`, so a value reads in the
//! page's language and typesetting. Pure: page links arrive as a closure.

use crate::build::components::field_text::{datetime_attr, number_text, value_text};
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use moss_core::content_graph::ContentGraph;
use moss_core::fields::{FieldDecl, PageLink, Place, TypedValue};
use moss_core::media::html_escape;

/// The markup a page's placed fields draw. `eyebrow` goes above the title;
/// the rest follows it, `subtitle` first and `key_figures` then `facts` after
/// the page's own masthead.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Placements {
    pub eyebrow: String,
    pub subtitle: String,
    pub key_figures: String,
    pub facts: String,
}

impl Placements {
    /// What follows the masthead: key figures, then the facts list.
    pub fn after_masthead(&self) -> String {
        format!("{}{}", self.key_figures, self.facts)
    }
}

/// `placed` holds the page's placed declarations with a valid value, in
/// declaration order. `page_href` maps a page-link target to its URL, or `None` when the
/// target is not a page of this site (it is then drawn as plain text).
pub fn draw(
    placed: &[(FieldDecl, TypedValue)],
    lang: Language,
    typesetting: Option<&str>,
    page_href: &dyn Fn(&str) -> Option<String>,
) -> Placements {
    let drawn = |place: Place| {
        placed.iter().filter(move |(d, _)| d.place == Some(place)).filter_map(|(d, v)| {
            let html = inline(v, lang, typesetting, page_href);
            (!html.is_empty()).then_some((d, v, html))
        })
    };
    let field = |d: &FieldDecl| html_escape(&d.name);

    // Spaced, so the fields stay separate words in the page's text (search
    // excerpts, copying, reader views); the space collapses into the
    // separator the stylesheet draws.
    let eyebrow: String = drawn(Place::Eyebrow)
        .map(|(d, _, html)| format!("<span data-field=\"{}\">{html}</span>", field(d)))
        .collect::<Vec<_>>()
        .join(" ");
    let subtitle = drawn(Place::Subtitle)
        .next()
        .map(|(d, _, html)| format!("<p class=\"moss-subtitle\" data-field=\"{}\">{html}</p>\n", field(d)))
        .unwrap_or_default();
    let key_figures: String = drawn(Place::KeyFigure)
        .map(|(d, v, html)| {
            let body = if matches!(v, TypedValue::Count { .. } | TypedValue::Number(_)) { html } else { format!("<span>{html}</span>") };
            format!("<p class=\"moss-key-figure\" data-field=\"{}\">{body}</p>\n", field(d))
        })
        .collect();
    let rows: String = drawn(Place::Facts)
        .map(|(d, _, html)| {
            format!(
                "<div data-field=\"{}\"><dt>{}</dt><dd>{html}</dd></div>\n",
                field(d),
                html_escape(&d.label_for(lang.as_bcp47_attr()))
            )
        })
        .collect();
    Placements {
        eyebrow: if eyebrow.is_empty() { eyebrow } else { format!("<p class=\"moss-eyebrow\">{eyebrow}</p>\n") },
        subtitle,
        key_figures,
        facts: if rows.is_empty() { rows } else { format!("<dl class=\"moss-facts\" data-beside>\n{rows}</dl>\n") },
    }
}

/// [`draw`] for a rendered page: a page link resolves through the content
/// graph to another page of `all_docs`, which supplies its URL.
pub fn draw_for_page(
    doc: &ParsedDocument,
    all_docs: &[ParsedDocument],
    graph: Option<&ContentGraph>,
    typesetting: Option<&str>,
) -> Placements {
    if doc.placed_fields.is_empty() {
        return Placements::default();
    }
    let page_href = |target: &str| {
        let source = graph?.resolve_path(target, doc.source_path.as_deref()?)?;
        let page = all_docs.iter().find(|d| d.source_path.as_deref() == Some(source.as_str()))?;
        Some(crate::build::markdown::pipeline::absolute_pretty_url(&page.url_path))
    };
    draw(&doc.placed_fields, doc.lang, typesetting, &page_href)
}

/// One value as inline markup.
fn inline(v: &TypedValue, lang: Language, typesetting: Option<&str>, page_href: &dyn Fn(&str) -> Option<String>) -> String {
    if let Some(datetime) = datetime_attr(v) {
        return format!("<time datetime=\"{}\">{}</time>", html_escape(&datetime), html_escape(&value_text(v, lang, typesetting)));
    }
    match v {
        TypedValue::Number(n) => format!("<data value=\"{n}\">{}</data>", html_escape(&number_text(n, lang))),
        TypedValue::Count { number: Some(n), noun, .. } => {
            let data = format!("<data value=\"{n}\">{}</data>", html_escape(&number_text(n, lang)));
            match noun {
                Some(noun) => format!("{data} <span>{}</span>", html_escape(noun)),
                None => data,
            }
        }
        TypedValue::Count { number: None, text, .. } if !text.is_empty() => format!("<span>{}</span>", html_escape(text)),
        TypedValue::Url(u) => format!("<a href=\"{}\">{}</a>", html_escape(u), html_escape(u)),
        TypedValue::Page(p) => page_link_html(p, page_href),
        TypedValue::Pages(pages) => pages.iter().map(|p| page_link_html(p, page_href)).collect::<Vec<_>>().join(", "),
        other => html_escape(&value_text(other, lang, typesetting)),
    }
}

/// A page link reads as written, like a body wikilink: its alias, else the last
/// segment of its target. It links to the page when the target resolves.
fn page_link_html(link: &PageLink, page_href: &dyn Fn(&str) -> Option<String>) -> String {
    let name = link.alias.as_deref().unwrap_or_else(|| link.target.rsplit('/').next().unwrap_or(&link.target));
    let name = html_escape(name);
    match page_href(&link.target) {
        Some(href) => format!("<a href=\"{}\">{name}</a>", html_escape(&href)),
        None => name,
    }
}

/// Marks where a page's own body starts, under its title block, on a page
/// whose title block goes after its picture.
pub const BODY_START: &str = "<!-- moss:body-start -->";

/// Whether `doc` puts its title block after its leading picture
/// (`title_block: after-picture`, its own or cascaded). A folder index and a
/// slot such as the footer keep their title first.
pub fn title_block_after_picture(doc: &ParsedDocument) -> bool {
    title_block_is(doc, "after-picture")
}

/// Whether `doc` sets its title block beside its leading picture
/// (`title_block: beside-picture`), under the same rules as
/// [`title_block_after_picture`].
pub fn title_block_beside_picture(doc: &ParsedDocument) -> bool {
    title_block_is(doc, "beside-picture")
}

/// Whether `doc` moves its title block next to its leading picture, either way.
pub fn title_block_follows_picture(doc: &ParsedDocument) -> bool {
    title_block_after_picture(doc) || title_block_beside_picture(doc)
}

fn title_block_is(doc: &ParsedDocument, value: &str) -> bool {
    doc.title_block.as_deref() == Some(value) && doc.kind != moss_core::page_kind::PageKind::Folder && !doc.slot_only
}

/// Put [`BODY_START`] right after the title of a body that opens with moss's
/// own title, where the rest of the title block is spliced in later. A body
/// whose layout hides the title, or that has none, is left unmarked.
pub fn mark_body_start(html: &mut String) {
    let (eyebrow, rest) = crate::build::markdown::html_post::split_leading_eyebrow(html);
    if !rest.starts_with("<h1 class=\"moss-article-title\"") {
        return;
    }
    let Some(close) = rest.find("</h1>") else { return };
    let at = eyebrow.len() + close + "</h1>".len();
    html.insert_str(at, BODY_START);
}

/// `html` with its title block, everything above [`BODY_START`], moved to
/// just after the body's leading picture: its first top-level
/// `figure.moss-image`, when only divs come before it. Without one, the title
/// block stays first.
pub fn after_leading_picture(html: &str) -> String {
    place_title_block(html, false)
}

/// Like [`after_leading_picture`], but the picture and the title block go in
/// one `div.moss-title-beside`, the picture first, so a stylesheet can set
/// them side by side.
pub fn beside_leading_picture(html: &str) -> String {
    place_title_block(html, true)
}

fn place_title_block(html: &str, beside: bool) -> String {
    let Some((masthead, body)) = html.split_once(BODY_START) else {
        return html.to_string();
    };
    let Some((start, end)) = leading_picture_span(body) else {
        return format!("{masthead}{body}");
    };
    let (before, picture, after) = (&body[..start], &body[start..end], &body[end..]);
    if beside {
        format!("{before}<div class=\"moss-title-beside\">{picture}\n<div>{masthead}</div></div>{after}")
    } else {
        format!("{before}{picture}\n{masthead}{after}")
    }
}

/// Where the leading picture of `body` starts and ends, per [`after_leading_picture`].
fn leading_picture_span(body: &str) -> Option<(usize, usize)> {
    let mut at = 0;
    loop {
        at += body[at..].len() - body[at..].trim_start().len();
        let rest = &body[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            at += 4 + comment.find("-->")? + 3;
            continue;
        }
        if rest.starts_with("<figure class=\"moss-image") {
            return Some((at, at + rest.find("</figure>")? + "</figure>".len()));
        }
        if !rest.starts_with("<div") {
            return None;
        }
        at += element_len(rest, "div")?;
    }
}

/// The length of the element `<tag …>…</tag>` that `html` starts with,
/// counting nested elements of the same name.
fn element_len(html: &str, tag: &str) -> Option<usize> {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut depth = 0usize;
    let mut at = 0;
    loop {
        let next_open = html[at..].match_indices(&open).map(|(i, _)| i).find(|&i| {
            html[at + i + open.len()..].starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace())
        });
        let next_close = html[at..].find(&close)?;
        match next_open {
            Some(o) if o < next_close => {
                depth += 1;
                at += o + open.len();
            }
            _ => {
                depth = depth.checked_sub(1)?;
                at += next_close + close.len();
                if depth == 0 {
                    return Some(at);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "field_places_tests.rs"]
mod tests;
