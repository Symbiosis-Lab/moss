//! `style:stream`: each page drawn whole, one after another.
//!
//! A page that cannot be drawn whole is drawn as a row of its title and
//! opening words: a folder (its listing is not part of its body), a page that
//! lists folders itself (its body is a listing whose content depends on which
//! pages were expanded first), and the page the listing sits on.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::inline_body::{dir_segments, listed_body, IdScopes};
use super::ListingRowOptions;
use crate::build::components::child_list::ChildItemProps;
use crate::build::components::date as date_formatters;
use crate::build::features::html_escape;
use crate::build::types::ParsedDocument;
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

/// The listing: one `h-entry` per page, in the order given.
pub(super) fn render(
    sorted: &[&ChildItemProps],
    docs: &[&ParsedDocument],
    typesetting: Option<&str>,
    rows: &ListingRowOptions<'_>,
) -> String {
    let host_url = rows.host_url_path;
    let to_dir = dir_segments(host_url);
    let by_url: HashMap<&str, &ParsedDocument> = docs.iter().map(|d| (d.url_path.as_str(), *d)).collect();
    let own_scopes = std::cell::RefCell::new(IdScopes::default());
    let mut scopes = rows.id_scopes.unwrap_or(&own_scopes).borrow_mut();
    let mut items = String::new();
    for (n, props) in sorted.iter().enumerate() {
        let Some(doc) = by_url.get(props.url_path.as_str()) else { continue };
        let date = date_link(props, doc, typesetting);
        if !drawn_whole(doc, host_url) {
            let summary = props.description.as_deref().filter(|_| !doc.untitled).map(|d| format!(r#"<p class="p-summary">{}</p>"#, crate::build::page::meta::render_description_html(d))).unwrap_or_default();
            items.push_str(&format!(r#"<article class="moss-stream-item moss-stream-row h-entry">{}{summary}{date}</article>"#, heading(props)));
            continue;
        }
        let mut body = scopes.scope(&demote_headings(&listed_body(doc, &to_dir)), &doc.url_path, n);
        // The listing's first picture may take the eager slot the page left open.
        if n > 0 || rows.lazy_cards {
            body = lazy_pictures(&body);
        }
        let title = if doc.untitled { String::new() } else { heading(props) };
        items.push_str(&format!(r#"<article class="moss-stream-item h-entry">{title}<div class="moss-stream-body e-content">{body}</div>{date}</article>"#));
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
