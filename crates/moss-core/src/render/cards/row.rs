//! A listing's one-line rows: a date or count, then the title.

use super::{event_card_links, lang_attr, lang_mark, leaf_meta, link_or_box, meta_text, subtitle_html, EventCardLinks};
use crate::i18n::dates as date_formatters;
use crate::media::html_escape;
use crate::page::card::{root_attrs, ChildItemProps, RowLang};

/// Properties for rendering an article list item
#[derive(Debug, Clone, Default)]
pub struct ArticleListItemProps {
    /// Formatted date display (e.g., "2025 · 09" or "September 2, 2025")
    pub date_display: String,
    /// Raw ISO date string for sorting (e.g., "2025-09-24T20:49:08.546Z")
    pub date_raw: Option<String>,
    /// URL path to the article (e.g., "posts/my-article.html"); `None` for a
    /// page-less entry ([`ParsedDocument::is_pageless_entry`]), drawn unlinked.
    pub url: Option<String>,
    /// Article title
    pub title: String,
    /// The page's own `url_path`, which breaks a tie between two rows on the
    /// same date — the same key the folder's series links use.
    pub url_path: String,
    /// The page's `weight:`, which orders rows on one date.
    pub weight: Option<i32>,
    /// The page's resolved place, plain and unlinked (`ParsedDocument::place_names`)
    /// — shown next to the date by [`with_place`]. `None` for a page with no
    /// resolved `location:`.
    pub place: Option<String>,
    /// An event's written time as a `<time>` with its parts in spans
    /// (`crate::event::when::render_when`); shown in place of `date_display`.
    pub when_html: Option<String>,
    /// Where an event stands, published on the row as `data-when` (and
    /// `data-ends`); `None` for any other page.
    pub when: Option<crate::event::when::EventWhen>,
    /// The page's listed names, pre-rendered (`ChildItemProps::credits`).
    pub credits: Option<String>,
    /// The page's `subtitle:` ([`card_subtitle`]), drawn under the title.
    pub subtitle: Option<String>,
    /// Source path of the page this row stands for (`ParsedDocument::source_path`).
    pub source_file: Option<String>,
    /// Set when the page is in another language than the listing.
    pub lang: Option<RowLang>,
}

impl From<&ChildItemProps> for ArticleListItemProps {
    fn from(item: &ChildItemProps) -> Self {
        ArticleListItemProps {
            date_display: item.date_display.clone().unwrap_or_default(),
            date_raw: item.date_raw.clone(),
            url: item.url.clone(),
            title: item.title.clone(),
            url_path: item.url_path.clone(),
            weight: item.weight,
            place: item.place.clone(),
            when_html: item.when_html.clone(),
            when: item.when.clone(),
            credits: item.credits.clone(),
            subtitle: item.subtitle.clone(),
            source_file: item.source_file.clone(),
            lang: item.lang.clone(),
        }
    }
}

impl ArticleListItemProps {
    pub fn date_sort_key(&self) -> crate::sort::DateSortKey<'_> {
        crate::sort::DateSortKey {
            date: self.date_raw.as_deref(),
            is_folder: false,
            label: &self.title,
            url_path: &self.url_path,
            weight: self.weight,
        }
    }
}

/// A row's title with its subtitle beneath. A row lays its prefix and title
/// side by side, so the pair gets a wrapper to stack them; without a subtitle
/// the title stays as it was.
fn title_with_subtitle(title_html: String, subtitle: Option<&str>) -> String {
    match subtitle_html(subtitle) {
        sub if sub.is_empty() => title_html,
        sub => format!(r#"<span class="moss-prefix-link-text">{title_html}{sub}</span>"#),
    }
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
        let (open, close) = link_or_box(props.url.as_deref(), "span", r#" class="moss-prefix-link""#);
        return format!(
            r#"<p class="moss-card"{}>{open}{}{close}</p>"#,
            root_attrs(props.source_file.as_deref(), props.when.as_ref()),
            title_with_subtitle(
                format!(r#"<span class="moss-prefix-link-title title"{}>{}{}</span>"#, lang_attr(props.lang.as_ref()), html_escape(&props.title), lang_mark(props.lang.as_ref())),
                props.subtitle.as_deref(),
            )
        );
    }

    let date_display = if minimal {
        // Month only since the year is the section heading above; `十二月`
        // rather than `12` under vertical CJK, and nothing at all when the
        // date carries no month.
        props
            .date_raw
            .as_deref()
            .map(|d| date_formatters::time_html(&date_formatters::format_month_prefix(d, lang, typesetting), d))
            .unwrap_or_else(|| {
                html_escape(&date_formatters::format_month_prefix(&props.date_display, lang, typesetting))
            })
    } else {
        // Year + month (e.g., "2025 · 11")
        props.date_display.clone()
    };
    // The page's resolved place next to its date — see `with_place`. Applied
    // before the empty check below: a bare-year row under a `minimal` month
    // prefix can have no month text at all, and a place is still worth a row
    // on its own rather than vanishing alongside the empty date.
    let date_display = leaf_meta(&date_display, props.when_html.as_deref(), props.place.as_deref(), props.credits.as_deref());

    // An empty prefix means a year-only date under its own year heading, and
    // no place either; emit no span at all rather than one that lays out as
    // a blank column.
    let prefix = if date_display.is_empty() {
        String::new()
    } else {
        format!(
            r#"<span class="moss-prefix-link-prefix date">{}</span>"#,
            date_display
        )
    };
    let (open, close) = link_or_box(props.url.as_deref(), "span", r#" class="moss-prefix-link""#);
    format!(
        r#"<p class="moss-card"{}>{open}{}{}{close}</p>"#,
        root_attrs(props.source_file.as_deref(), props.when.as_ref()),
        prefix,
        title_with_subtitle(
            format!(r#"<span class="moss-prefix-link-title title"{}>{}{}</span>"#, lang_attr(props.lang.as_ref()), html_escape(&props.title), lang_mark(props.lang.as_ref())),
            props.subtitle.as_deref(),
        )
    )
}

/// The opening element, title markup and closing element of a list row. An
/// ordinary row is one anchor around its whole text; an event row keeps the
/// same flex box as a `<div>`, links only its title, and carries its action
/// links after the title.
fn row_parts(props: &ChildItemProps, event: Option<&EventCardLinks>, row_class: &str, title_class: &str) -> (String, String, String) {
    match event {
        Some(links) => (
            format!(r#"<div class="{row_class}">"#),
            format!(
                "{}{}",
                title_with_subtitle(format!(r#"<span class="{title_class}"{}>{}{}</span>"#, lang_attr(props.lang.as_ref()), links.title, lang_mark(props.lang.as_ref())), props.subtitle.as_deref()),
                links.actions
            ),
            "</div>".to_string(),
        ),
        None => {
            let (open, close) = link_or_box(props.url.as_deref(), "div", &format!(r#" class="{row_class}""#));
            (
                open,
                title_with_subtitle(format!(r#"<span class="{title_class}"{}>{}{}</span>"#, lang_attr(props.lang.as_ref()), html_escape(&props.title), lang_mark(props.lang.as_ref())), props.subtitle.as_deref()),
                close,
            )
        }
    }
}

/// Renders a single child list item as HTML.
///
/// - For articles (child_count is None, date_display is Some): renders with the
///   full-precision date prefix ("year · month", or "year" alone) — there is no
///   year heading above this row to carry it, unlike the year-grouped rows.
/// - For folders (child_count is Some): renders with count suffix (e.g., "4 篇");
///   a folder whose own home page declares a date gets that date (and its
///   place) ahead of the count, the same way a page's date reads, via
///   [`meta_text`].
/// - For articles without date: renders title only
pub fn render_child(props: &ChildItemProps, lang: crate::i18n::Language, typesetting: Option<&str>) -> String {
    let event = event_card_links(props, lang);
    let event = event.as_ref();

    if props.child_count.is_some() {
        let count_label = props.count_label(lang, typesetting);
        let count_text = meta_text(props.date_display.as_deref(), None, props.place.as_deref(), count_label.as_deref(), None);
        let meta_span = |class: &str| {
            if count_text.is_empty() { String::new() } else { format!(r#"<span class="{}">{}</span>"#, class, count_text) }
        };

        if let Some(ref desc) = props.description {
            // Folder WITH description: title + count suffix, description below
            let (open, title, close) = row_parts(props, event, "moss-prefix-link moss-folder-link", "moss-prefix-link-title");
            format!(
                r#"<div class="moss-card moss-folder-item"{}>{}{}{}{}<p class="moss-folder-description"{}>{}</p></div>"#,
                props.root_attrs(), open, title, meta_span("moss-prefix-link-suffix"), close, lang_attr(props.lang.as_ref()),
                crate::page::excerpt::render_description_html(desc)
            )
        } else {
            // Folder WITHOUT description: count in prefix slot
            let (open, title, close) = row_parts(props, event, "moss-prefix-link", "moss-prefix-link-title");
            format!(
                r#"<div class="moss-card"{}>{}{}{}{}</div>"#,
                props.root_attrs(), open, meta_span("moss-prefix-link-prefix"), title, close
            )
        }
    } else if let Some(ref date_display) = props.date_display {
        // An article's compact date ("year · month", or the year alone): no
        // year heading above this row carries it.
        let date = leaf_meta(date_display, props.when_html.as_deref(), props.place.as_deref(), props.credits.as_deref());
        let (open, title, close) = row_parts(props, event, "moss-prefix-link", "moss-prefix-link-title");
        format!(
            r#"<div class="moss-card"{}>{}<span class="moss-prefix-link-prefix date">{}</span>{}{}</div>"#,
            props.root_attrs(), open, date, title, close
        )
    } else {
        // Article without any date: its meta (event time, place, credits and card fields), when it has any.
        let meta = leaf_meta("", props.when_html.as_deref(), props.place.as_deref(), props.credits.as_deref());
        let (open, title, close) = row_parts(props, event, "moss-prefix-link", "moss-prefix-link-title");
        let prefix = if meta.is_empty() { String::new() } else { format!(r#"<span class="moss-prefix-link-prefix date">{meta}</span>"#) };
        format!(
            r#"<div class="moss-card"{}>{}{}{}{}</div>"#,
            props.root_attrs(), open, prefix, title, close
        )
    }
}

#[cfg(test)]
#[path = "row_tests.rs"]
mod tests;
