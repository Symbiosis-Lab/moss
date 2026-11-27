//! `style:stream`: each page drawn whole, one after another.
//!
//! A page that cannot be drawn whole is drawn as a row of its title and
//! opening words: a folder (its listing is not part of its body), a page that
//! lists folders itself (its body is a listing whose content depends on which
//! pages were expanded first), and the page the listing sits on.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use moss_core::page::site_page::SitePage;

use regex::{Captures, Regex};

use super::inline_body::{dir_segments, listed_body, IdScopes};
use super::ListingRowOptions;
use crate::build::components::child_list::ChildItemProps;
use moss_core::i18n::dates as date_formatters;
use crate::build::features::html_escape;
use crate::build::markdown::pipeline::absolute_pretty_url;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use moss_core::fields::{LinkTarget, TypedValue, IN_REPLY_TO};
use moss_core::PageKind;

/// Whether the page's own `children_style` is `stream`.
pub(crate) fn is_stream_host(doc: &ParsedDocument) -> bool {
    doc.children_style.as_ref().is_some_and(|s| s.value == "stream")
}

/// Give each page with `children_style: stream` the features of the pages it
/// draws whole, so the scripts and styles their bodies need load on it.
pub(super) fn absorb_listed_features(documents: &mut [ParsedDocument], project: &crate::types::content::ProjectStructure, now: chrono::NaiveDateTime) {
    for i in 0..documents.len() {
        if !is_stream_host(&documents[i]) {
            continue;
        }
        let host = &documents[i];
        let urls: BTreeSet<String> = super::members::listing_members(host, documents, project, now)
            .into_iter()
            .filter(|d| drawn_whole(d, &host.url_path))
            .map(|d| d.url_path.clone())
            .collect();
        absorb_into(documents, i, &urls);
    }
}

/// Mark `documents[host]` as drawing a stream and give it the features of the
/// pages at `urls`, which it draws whole.
pub(super) fn absorb_into(documents: &mut [ParsedDocument], host: usize, urls: &BTreeSet<String>) {
    let inlined: Vec<_> = documents.iter().filter(|d| urls.contains(&d.url_path)).map(|d| d.features.clone()).collect();
    documents[host].features.stream = true;
    for features in &inlined {
        documents[host].features.absorb(features);
    }
}

/// Whether the listing draws `doc`'s own body.
pub(super) fn drawn_whole(doc: &ParsedDocument, host_url: &str) -> bool {
    doc.kind != PageKind::Folder && !doc.lists_folders && !doc.is_pageless_entry() && doc.url_path != host_url
}

/// Headings one level down: the listing's own page owns the `h1`.
fn demote_headings(html: &str) -> String {
    static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(/?)h([1-5])([\s>])").unwrap());
    HEADING.replace_all(html, |c: &Captures| format!("<{}h{}{}", &c[1], c[2].parse::<u8>().unwrap_or(1) + 1, &c[3])).into_owned()
}

/// Pictures that load lazily, never competing with the page's own opening
/// picture for the one eager slot.
fn lazy_pictures(html: &str) -> String {
    static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<img\b[^>]*>").unwrap());
    static LOADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\sloading="[^"]*""#).unwrap());
    static PRIORITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\sfetchpriority="[^"]*""#).unwrap());
    IMG.replace_all(html, |c: &Captures| {
        let without_loading = LOADING.replace_all(&c[0], "");
        let tag = PRIORITY.replace_all(&without_loading, "");
        match tag.strip_suffix("/>") {
            Some(head) => format!(r#"{} loading="lazy"/>"#, head.trim_end()),
            None => format!(r#"{} loading="lazy">"#, tag.trim_end_matches('>').trim_end()),
        }
    })
    .into_owned()
}

fn date_link(props: &ChildItemProps, doc: &ParsedDocument, typesetting: Option<&str>) -> String {
    let (Some(href), Some(raw), Some(_)) = (props.permalink.as_ref().or(props.url.as_ref()), props.date_raw.as_deref(), props.date_display.as_ref()) else {
        return String::new();
    };
    let Some(day) = date_formatters::datetime_attr(raw) else { return String::new() };
    let day_shown = date_formatters::format_display_date(raw, doc.lang, typesetting);
    // Two posts of one day read apart by their time, when the date has one.
    let (iso, shown) = match date_formatters::clock_time(raw).filter(|_| day.len() == 10) {
        Some(clock) => (format!("{day}T{clock}"), date_formatters::with_clock(&day_shown, clock, doc.lang)),
        None => (day, day_shown),
    };
    let shown = html_escape(&shown);
    format!(r#"<p class="moss-stream-date"><a class="u-url" href="{}"><time class="dt-published" datetime="{iso}">{shown}</time></a></p>"#, html_escape(href))
}

fn heading(props: &ChildItemProps) -> String {
    let title = html_escape(&props.title);
    match props.url.as_deref() {
        Some(href) => format!(r#"<h2 class="moss-stream-title p-name"><a href="{}">{title}</a></h2>"#, html_escape(href)),
        None => format!(r#"<h2 class="moss-stream-title p-name">{title}</h2>"#),
    }
}

/// The listing: one `h-entry` per page, in the order given. The pages of one
/// thread are one unit, placed where the listing first reaches one of them
/// (the newest, in a newest-first listing), its parts in thread order.
pub(super) fn render(
    sorted: &[&ChildItemProps],
    docs: &[&ParsedDocument],
    typesetting: Option<&str>,
    rows: &ListingRowOptions<'_>,
    fold_threads: bool,
) -> String {
    if sorted.is_empty() {
        return String::new();
    }
    let host_url = rows.host_url_path;
    let to_dir = dir_segments(host_url);
    let by_url: HashMap<&str, &ParsedDocument> = docs.iter().map(|d| (d.url_path.as_str(), *d)).collect();
    let listed: HashMap<&str, usize> =
        sorted.iter().enumerate().filter_map(|(at, p)| Some((by_url.get(p.url_path.as_str())?.source_path.as_deref()?, at))).collect();
    let own_scopes = std::cell::RefCell::new(IdScopes::default());
    let mut scopes = rows.id_scopes.unwrap_or(&own_scopes).borrow_mut();
    let mut drawn = 0usize;
    let mut item = |props: &ChildItemProps, doc: &ParsedDocument| {
        let n = drawn;
        drawn += 1;
        let date = date_link(props, doc, typesetting);
        if !drawn_whole(doc, host_url) {
            let summary = props.description.as_deref().filter(|_| !doc.untitled).map(|d| format!(r#"<p class="p-summary">{}</p>"#, crate::build::page::meta::render_description_html(d))).unwrap_or_default();
            return format!(r#"<article class="moss-stream-item moss-stream-row h-entry">{}{summary}{date}</article>"#, heading(props));
        }
        let mut body = scopes.scope(&demote_headings(&listed_body(doc, &to_dir)), &doc.url_path, n);
        // The listing's first picture may take the eager slot the page left open.
        if n > 0 || rows.lazy_cards {
            body = lazy_pictures(&body);
        }
        let title = if doc.untitled { String::new() } else { heading(props) };
        format!(r#"<article class="moss-stream-item h-entry">{title}<div class="moss-stream-body e-content">{body}</div>{date}</article>"#)
    };
    let mut items = String::new();
    let mut threads_drawn = std::collections::HashSet::new();
    for props in sorted {
        let Some(doc) = by_url.get(props.url_path.as_str()) else { continue };
        let thread: Vec<usize> = doc.thread.iter().filter_map(|m| listed.get(m.source_path.as_str()).copied()).collect();
        if !fold_threads || thread.len() < 2 {
            items.push_str(&item(props, doc));
            continue;
        }
        if !threads_drawn.insert(thread[0]) {
            continue;
        }
        items.push_str(&thread_unit(&thread, sorted, &by_url, doc.lang, &mut item));
    }
    let place = moss_core::render::placement::placement_attrs(&rows.placement);
    format!(
        r#"<div class="moss-stream{}"{}{}{}>{items}</div>"#,
        place.align_suffix(),
        place.data_width_attr,
        place.size_style_attr,
        if rows.is_embed { " data-embed" } else { "" },
    )
}

/// A thread's listed pages as one unit: all of them up to three, else the
/// first, a link to the newest saying how many are left out, and the last two.
fn thread_unit(
    thread: &[usize],
    sorted: &[&ChildItemProps],
    by_url: &HashMap<&str, &ParsedDocument>,
    lang: Language,
    item: &mut dyn FnMut(&ChildItemProps, &ParsedDocument) -> String,
) -> String {
    let mut draw = |at: usize| by_url.get(sorted[at].url_path.as_str()).map(|doc| item(sorted[at], doc)).unwrap_or_default();
    let inner = if thread.len() <= 3 {
        thread.iter().map(|at| draw(*at)).collect::<String>()
    } else {
        let newest = sorted[thread[thread.len() - 1]];
        let href = newest.permalink.as_ref().or(newest.url.as_ref()).cloned().unwrap_or_default();
        let more = crate::i18n::t(lang, "thread_more").replace("{n}", &(thread.len() - 3).to_string());
        let fold = format!(r#"<a class="moss-stream-thread-more" href="{}">{}</a>"#, html_escape(&href), html_escape(&more));
        format!("{}{fold}{}{}", draw(thread[0]), draw(thread[thread.len() - 2]), draw(thread[thread.len() - 1]))
    };
    format!(r#"<div class="moss-stream-thread">{inner}</div>"#)
}

// ---- a page's thread ---------------------------------------------------------
//
// The pages it continues above its title, the pages continuing it below, each
// drawn as the stream draws it; and the outside post it answers.

/// What a page draws above its title and after its body.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ThreadHtml {
    pub before: String,
    pub after: String,
}

/// `draw` draws pages as the stream does.
fn thread_parts<'d>(doc: &ParsedDocument, all_docs: &'d [ParsedDocument], draw: &dyn Fn(&[&'d ParsedDocument]) -> String) -> ThreadHtml {
    let lang = doc.lang;
    let mut out = ThreadHtml { before: outside_reply(doc, lang), after: String::new() };
    let page = |source: &str| all_docs.iter().find(|p| p.source_path.as_deref() == Some(source));
    let members: Vec<(&'d ParsedDocument, usize)> = doc.thread.iter().filter_map(|m| Some((page(&m.source_path)?, m.depth))).collect();
    let Some(at) = members.iter().position(|(m, _)| m.source_path == doc.source_path) else {
        return out;
    };
    let label = html_escape(&crate::i18n::t(lang, "thread_label"));
    let section = |part: &str, inner: String| format!("<section class=\"moss-thread\" data-part=\"{part}\" aria-label=\"{label}\">{inner}</section>");
    let earlier: Vec<&'d ParsedDocument> = members[..at].iter().map(|(m, _)| *m).collect();
    let later: Vec<&'d ParsedDocument> = members[at + 1..].iter().map(|(m, _)| *m).collect();
    // The parent is the nearest earlier member one step shallower.
    let depth = members[at].1;
    let parent = members[..at].iter().rev().find(|(_, d)| *d + 1 == depth).map(|(m, _)| *m);
    let reply = parent.map(|p| format!("<data class=\"u-in-reply-to\" value=\"{}\"></data>", html_escape(&absolute_pretty_url(&p.url_path)))).unwrap_or_default();
    let drawn = if earlier.is_empty() { String::new() } else { draw(&earlier) };
    // The root has nothing above it.
    if !(drawn.is_empty() && reply.is_empty()) {
        out.before.push_str(&section("before", format!("{drawn}{reply}")));
    }
    if !later.is_empty() {
        out.after = section("after", draw(&later));
    }
    out
}

/// "In reply to example.social/@someone/1" for a page answering a post elsewhere.
fn outside_reply(doc: &ParsedDocument, lang: Language) -> String {
    let Some(TypedValue::Page(link)) = doc.declared.get(IN_REPLY_TO) else { return String::new() };
    let LinkTarget::Outside(url) = &link.target else { return String::new() };
    let name = crate::build::components::field_places::link_name(link);
    format!(
        "<p class=\"moss-in-reply-to\">{} <a class=\"u-in-reply-to\" href=\"{}\">{}</a></p>",
        html_escape(&crate::i18n::t(lang, "in_reply_to")),
        html_escape(url),
        html_escape(name)
    )
}

/// `doc`'s thread among `all_docs`, its members drawn as the stream draws them.
pub(crate) fn page_thread(
    doc: &ParsedDocument,
    all_docs: &[ParsedDocument],
    dir_overrides: &HashMap<String, String>,
    typesetting: Option<&str>,
    now: chrono::NaiveDateTime,
) -> ThreadHtml {
    if doc.thread.is_empty() && !doc.declared.contains_key(IN_REPLY_TO) {
        return ThreadHtml::default();
    }
    let draw = |pages: &[&ParsedDocument]| {
        let cards = crate::build::components::child_list::BuildCards::new(dir_overrides, typesetting, now);
        let props: Vec<ChildItemProps> = pages
            .iter()
            .map(|p| moss_core::page::card::props_for(*p, all_docs, &cards, None))
            .collect();
        let sorted: Vec<&ChildItemProps> = props.iter().collect();
        let rows = ListingRowOptions {
            is_embed: false,
            placement: Default::default(),
            lazy_cards: true,
            count: None,
            listing_tag: None,
            pictures: None,
            marquee: false,
            host_url_path: &doc.url_path,
            id_scopes: None,
        };
        render(&sorted, pages, typesetting, &rows, false)
    };
    thread_parts(doc, all_docs, &draw)
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
