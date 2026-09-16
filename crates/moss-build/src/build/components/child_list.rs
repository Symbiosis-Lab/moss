//! Child list item component
//!
//! Renders a single article entry with date and link, used in:
//! - Collection index pages
//! - Topic pages
//! - Latest sidebar
//! - Auto-generated home pages
//!
//! The `minimal` flag controls date format:
//! - `minimal=false`: Shows "year · month" (e.g., "2025 · 11")
//! - `minimal=true`: Shows month only (e.g., "11") since year is in section header

use moss_core::PageKind;

use super::date as date_formatters;
use crate::build::assets::paths::PathResolver;
use crate::build::media::cover::{detect_cover_type, html_escape, CoverType};
use crate::build::types::ParsedDocument;

/// Properties for rendering an article list item
#[derive(Debug, Clone)]
pub struct ArticleListItemProps {
    /// Formatted date display (e.g., "2025 · 09" or "September 2, 2025")
    pub date_display: String,
    /// Raw ISO date string for sorting (e.g., "2025-09-24T20:49:08.546Z")
    pub date_raw: Option<String>,
    /// URL path to the article (e.g., "posts/my-article.html")
    pub url: String,
    /// Article title
    pub title: String,
}

/// Renders an article list item as HTML.
///
/// # Output
/// ```html
/// <p class="moss-card"><a href="post.html"><span class="date">2025 · 09</span><span class="title">Article Title</span></a></p>
/// ```
/// or with `minimal=true`:
/// ```html
/// <p class="moss-card"><a href="post.html"><span class="date">09</span><span class="title">Article Title</span></a></p>
/// ```
/// or with `no_date=true` (series items):
/// ```html
/// <p class="moss-card"><a href="post.html"><span class="title">Article Title</span></a></p>
/// ```
///
/// The `.moss-card` class enables:
/// - Only title gets underline (date does not)
/// - Hover changes color only (keeps underline)
///
/// # Arguments
/// * `props` - The article properties to render
/// * `minimal` - If true, shows month only; if false, shows year + month
/// * `no_date` - If true, omit the date span entirely (for series lists)
///
/// # Returns
/// HTML string for the article list item
pub fn render(
    props: &ArticleListItemProps,
    minimal: bool,
    no_date: bool,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> String {
    if no_date {
        return format!(
            r#"<p class="moss-card"><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-title title">{}</span></a></p>"#,
            html_escape(&props.url),
            html_escape(&props.title)
        );
    }

    let date_display = if minimal {
        // Month only since the year is the section heading above; `十二月`
        // rather than `12` under vertical CJK, and nothing at all when the
        // date carries no month.
        props
            .date_raw
            .as_deref()
            .map(|d| date_formatters::format_month_prefix(d, lang, typesetting))
            .unwrap_or_else(|| {
                date_formatters::format_month_prefix(&props.date_display, lang, typesetting)
            })
    } else {
        // Year + month (e.g., "2025 · 11")
        props.date_display.clone()
    };

    // An empty prefix means a year-only date under its own year heading; emit
    // no span at all rather than one that lays out as a blank column.
    let prefix = if date_display.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="moss-prefix-link-prefix date">{}</span>"#,
            html_escape(&date_display)
        )
    };
    format!(
        r#"<p class="moss-card"><a href="{}" class="moss-prefix-link">{}<span class="moss-prefix-link-title title">{}</span></a></p>"#,
        html_escape(&props.url),
        prefix,
        html_escape(&props.title)
    )
}

/// Properties for rendering a child list item (article or folder)
#[derive(Debug, Clone)]
pub struct ChildItemProps {
    pub title: String,
    pub url: String,
    /// None for folders
    pub date_display: Option<String>,
    /// Raw ISO date for sorting. For folders: latest article date within
    pub date_raw: Option<String>,
    /// Number of articles inside. None for leaf articles
    pub child_count: Option<usize>,
    /// From frontmatter description or auto-extracted
    pub description: Option<String>,
    /// Optional cover image path
    pub cover: Option<String>,
    /// Type of cover media (image, video, iframe). Defaults to Image when cover is Some.
    pub cover_type: Option<CoverType>,
    /// Optional overline shown above the title (horizontal) or between title
    /// and meta (vertical). Honored by `child_summary::render_with_sort` (cards-list);
    /// silently ignored by `child_list::render_child` (cards-minimal) because
    /// the minimal layout has no slot for it. Empty string and None are
    /// equivalent — no kicker rendered.
    pub kicker: Option<String>,
    /// Local archival URL. Set ONLY when the primary `url` field points OUT
    /// (i.e. `external_url:` overrode it for a linkblog/imported card). The
    /// summary renderer (`child_summary::render_with_sort`) emits a `★`
    /// permalink mark linking here so readers can reach the local copy
    /// independent of the external title link. The minimal-style renderer
    /// (`child_list::render_child`) intentionally ignores this field — its
    /// year-grouped row layout has no slot for a second anchor.
    /// `None` for ordinary cards (where `url` is already the local URL).
    pub permalink: Option<String>,
}

/// Build one card's props from the document it describes.
///
/// The shared reading of a `ParsedDocument` that both listing styles and the
/// hand-picked `:::grid {.summary}` variant need: the folder-count-or-date
/// branch, the pipe-encoded cover split-resolve-rejoin, the `external_url:`
/// linkblog pair, and the publisher kicker.
///
/// `description` comes from frontmatter only. A caller that wants the
/// auto-extracted excerpt fallback (the folder-listing path does) overrides
/// that one field with `page::meta::resolve_page_description` afterwards —
/// which is also what keeps this function out of the `math: bool` the excerpt
/// resolver needs.
pub(crate) fn props_for_document<D: std::borrow::Borrow<ParsedDocument>>(
    doc: &ParsedDocument,
    all_docs: &[D],
    root_path: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
    typesetting: Option<&str>,
) -> ChildItemProps {
    let path_resolver = PathResolver::new().with_dir_overrides(dir_overrides.clone());

    // The card always counts every listable article beneath the folder,
    // subfolders included — independent of `children_depth`, which only
    // controls how many of them the folder's OWN page lists when opened. A
    // `direct` folder can legitimately show fewer items than its card claims;
    // the claim is "this many pages live here", not "this many rows below".
    // No articles → no claim (a date, like grid cards), not "0 articles".
    let article_count = (doc.kind == PageKind::Folder)
        .then(|| {
            crate::build::page::page::count_listable_articles(
                all_docs,
                doc.url_path.trim_end_matches("index.html"),
                &doc.url_path,
            )
        })
        .filter(|n| *n > 0);

    let (child_count, date_raw, date_display) = if let Some(count) = article_count {
        let latest = crate::build::folder_embed::folder_latest_date(doc, all_docs, root_path);
        (Some(count), latest, None)
    } else {
        // `dd` is Arabic-formatted; re-format the raw date so a vertical-CJK
        // page gets CJK numerals.
        let (dd, raw, is_explicit) = date_formatters::extract_date_from_doc(doc, root_path);
        let dd_opt = (is_explicit && dd != "Unknown" && !dd.is_empty()).then(|| {
            date_formatters::format_compact_date(raw.as_deref().unwrap_or(&dd), doc.lang, typesetting)
        });
        (None, raw, dd_opt)
    };

    // Split pipe-encoded cover to resolve only the path part. Rejoin with
    // attrs so downstream renderers can extract them.
    let (cover, cover_path_for_type) = match doc.cover.as_deref() {
        Some(c) => {
            let (path_part, attrs_str) = moss_core::media::split_pipe(c);
            let resolved = path_resolver.resolve_url(path_part);
            let cover_with_attrs = if attrs_str.is_empty() {
                resolved.clone()
            } else {
                format!("{}|{}", resolved, attrs_str)
            };
            (Some(cover_with_attrs), Some(resolved))
        }
        None => (None, None),
    };

    // When the child page declares an `external_url:` (linkblog / imported
    // article), the card href targets the outlet — not the local archive. The
    // page itself is still built at its slug; direct visits keep working.
    // Pattern from JSON Feed 1.1. The local archive URL is preserved as the
    // card's `permalink` so child_summary can emit a `★` mark next to the
    // kicker (the Daring-Fireball linkblog convention — see #680).
    let external_url = crate::build::scan::page_map::external_url(&doc.raw_frontmatter);
    let local_url =
        path_resolver.resolve_url(&crate::build::scan::article_map::to_pretty_url(&doc.url_path));
    let permalink = external_url.as_ref().map(|_| local_url.clone());

    ChildItemProps {
        // Child list cards/rows are chrome; use the plain-text label.
        title: doc.label.clone(),
        url: external_url.unwrap_or(local_url),
        date_display,
        date_raw,
        child_count,
        description: doc.description.clone().filter(|s| !s.trim().is_empty()),
        cover,
        cover_type: cover_path_for_type
            .as_deref()
            .map(|c| detect_cover_type(c, doc.cover_type.as_deref())),
        kicker: crate::build::scan::page_map::publisher(&doc.raw_frontmatter),
        permalink,
    }
}

/// Renders a single child list item as HTML.
///
/// - For articles (child_count is None, date_display is Some): renders with month-only date prefix
/// - For folders (child_count is Some): renders with count suffix (e.g., "4 篇")
/// - For articles without date: renders title only
pub fn render_child(props: &ChildItemProps, lang: crate::i18n::Language, typesetting: Option<&str>) -> String {
    let escaped_url = html_escape(&props.url);
    let escaped_title = html_escape(&props.title);

    if let Some(count) = props.child_count {
        let count_text = html_escape(&crate::i18n::article_count_label(lang, count, typesetting));

        if let Some(ref desc) = props.description {
            // Folder WITH description: title + count suffix, description below
            format!(
                r#"<div class="moss-card moss-folder-item"><a href="{}" class="moss-prefix-link moss-folder-link"><span class="moss-prefix-link-title">{}</span><span class="moss-prefix-link-suffix">{}</span></a><p class="moss-folder-description">{}</p></div>"#,
                escaped_url, escaped_title, count_text, html_escape(desc)
            )
        } else {
            // Folder WITHOUT description: count in prefix slot
            format!(
                r#"<div class="moss-card"><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-prefix">{}</span><span class="moss-prefix-link-title">{}</span></a></div>"#,
                escaped_url, count_text, escaped_title
            )
        }
    } else if let Some(ref date_raw) = props.date_raw {
        // Article with date — the month, or the year when that is all the date
        // says. Unlike the year-grouped rows above there is no heading here to
        // carry the year, so a monthless date must still name itself.
        let month = month_or_year(date_raw, lang, typesetting);
        format!(
            r#"<div class="moss-card"><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-prefix date">{}</span><span class="moss-prefix-link-title">{}</span></a></div>"#,
            escaped_url, html_escape(&month), escaped_title
        )
    } else if let Some(ref date_display) = props.date_display {
        // Article with display date but no raw date
        let month = month_or_year(date_display, lang, typesetting);
        format!(
            r#"<div class="moss-card"><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-prefix date">{}</span><span class="moss-prefix-link-title">{}</span></a></div>"#,
            escaped_url, html_escape(&month), escaped_title
        )
    } else {
        // Article without any date
        format!(
            r#"<div class="moss-card"><a href="{}" class="moss-prefix-link"><span class="moss-prefix-link-title">{}</span></a></div>"#,
            escaped_url, escaped_title
        )
    }
}

/// The month of `date_str`, falling back to its year — both written in the
/// numerals the page's typesetting calls for.
fn month_or_year(
    date_str: &str,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
) -> String {
    let month = date_formatters::format_month_prefix(date_str, lang, typesetting);
    if !month.is_empty() {
        return month;
    }
    date_formatters::extract_year(date_str)
        .map(|y| date_formatters::format_year_heading(y, lang, typesetting))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "child_list_tests.rs"]
mod tests;
