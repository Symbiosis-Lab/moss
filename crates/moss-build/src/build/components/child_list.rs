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


use moss_core::i18n::dates as date_formatters;
use crate::build::assets::paths::PathResolver;
use crate::build::media::cover::html_escape;
use crate::build::types::ParsedDocument;

pub use moss_core::page::card::{root_attrs, source_file_attr, ChildItemProps, RowLang, EVENT_SEP};

/// Set when `doc` is written in another language than the page listing it.
pub(crate) fn row_lang(doc: &ParsedDocument, listing_tag: &str, ui: crate::i18n::Language) -> Option<RowLang> {
    let tag = doc.lang_tag.as_deref().unwrap_or_else(|| doc.lang.as_bcp47_attr());
    (!tag.eq_ignore_ascii_case(listing_tag)).then(|| RowLang {
        tag: tag.to_string(),
        name: crate::i18n::language_name(ui, tag),
        listing_tag: listing_tag.to_string(),
    })
}

/// ` lang="de"` for an element holding a page's words in another language, or nothing.
pub(crate) fn lang_attr(lang: Option<&RowLang>) -> String {
    lang.map(|l| format!(r#" lang="{}""#, html_escape(&l.tag))).unwrap_or_default()
}

/// The small mark naming a row's language, or nothing for a row in the listing's own.
pub(crate) fn lang_mark(lang: Option<&RowLang>) -> String {
    match lang {
        Some(l) if !l.name.is_empty() => {
            format!(r#"<span class="moss-lang-mark" lang="{}">{}</span>"#, html_escape(&l.listing_tag), l.name)
        }
        _ => String::new(),
    }
}

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
    /// (`moss_core::event::when::render_when`); shown in place of `date_display`.
    pub when_html: Option<String>,
    /// Where an event stands, published on the row as `data-when` (and
    /// `data-ends`); `None` for any other page.
    pub when: Option<moss_core::event::when::EventWhen>,
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
    pub(crate) fn date_sort_key(&self) -> moss_core::sort::DateSortKey<'_> {
        moss_core::sort::DateSortKey {
            date: self.date_raw.as_deref(),
            is_folder: false,
            label: &self.title,
            url_path: &self.url_path,
            weight: self.weight,
        }
    }
}

/// A row's or card's own element, opening and closing: an anchor to `url`,
/// or, for an entry with no page, the same box as a `box_tag` (`span` where it
/// sits inside a `<p>`). `attrs` starts with a space (` class="moss-card"`).
pub(crate) fn link_or_box(url: Option<&str>, box_tag: &'static str, attrs: &str) -> (String, String) {
    match url {
        Some(url) => (format!(r#"<a href="{}"{attrs}>"#, html_escape(url)), "</a>".to_string()),
        None => (format!("<{box_tag}{attrs}>"), format!("</{box_tag}>")),
    }
}

/// The page's `subtitle:` frontmatter, trimmed; `None` when absent or blank.
pub(crate) fn card_subtitle(doc: &ParsedDocument) -> Option<String> {
    moss_core::page::card::trimmed(doc.raw_frontmatter.get("subtitle").and_then(|v| v.as_str()))
}

/// The line under a card's or row's title; nothing when there is no subtitle.
pub(crate) fn subtitle_html(subtitle: Option<&str>) -> String {
    subtitle
        .map(|s| format!(r#"<span class="moss-card-subtitle">{}</span>"#, html_escape(s)))
        .unwrap_or_default()
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

/// The compact meta text for a leaf's date, with its resolved place appended
/// when the page declares one — the single owner of this composition.
/// `grid_card::render_item`, `child_summary::render_with_sort` and this
/// module's own `render`/`render_child` each decided independently whether a
/// date-or-count string needed anything else beside it; this fold is that
/// one decision, made once, so a place name (always author-supplied text)
/// gets escaped exactly once rather than at three call sites with three
/// different escaping habits — `grid_card`'s `count_text` was interpolated
/// unescaped before this fold, safe only because it never carried anything
/// but machine-generated text.
///
/// `text` is escaped even with no place to append, so every caller can stop
/// escaping it separately.
pub(crate) fn with_place(text: &str, place: Option<&str>) -> String {
    with_place_html(&html_escape(text), place)
}

/// [`with_place`] for a date that is already HTML.
fn with_place_html(date_html: &str, place: Option<&str>) -> String {
    match place.map(str::trim).filter(|p| !p.is_empty()) {
        None => date_html.to_string(),
        Some(p) if date_html.is_empty() => html_escape(p),
        Some(p) => format!("{} · {}", date_html, html_escape(p)),
    }
}

/// A leaf's date-and-place text: the event's `<time>` when it has one, else
/// the date markup (`date_html`, already escaped and `<time>`-wrapped by
/// whoever produced it), then the page's listed names when a term kind shows
/// them on cards. The one choice between the two, for every listing form.
pub(crate) fn leaf_meta(date_html: &str, when_html: Option<&str>, place: Option<&str>, credits: Option<&str>) -> String {
    let base = match when_html {
        Some(h) => with_event_place(h, place),
        None => with_place_html(date_html, place),
    };
    match credits {
        None => base,
        Some(c) if base.is_empty() => c.to_string(),
        Some(c) => format!("{base}{EVENT_SEP}{c}"),
    }
}

/// [`with_place`] for an event's `<time>`. The place is its own span after a
/// separator span, so a theme can position it; the text is the same as
/// `with_place` gives.
fn with_event_place(time_html: &str, place: Option<&str>) -> String {
    match place.map(str::trim).filter(|p| !p.is_empty()) {
        None => time_html.to_string(),
        Some(p) => format!(
            r#"{time_html}{EVENT_SEP}<span class="moss-event-place">{}</span>"#,
            html_escape(p),
        ),
    }
}

/// The compact meta text every listing form shows: a leaf's date (with its
/// place via [`with_place`]), a folder's article count, or — when a child
/// folder's own home page declares a date — that date (with its place) and
/// the count after it, the same `date · place` a page's own meta gets. The
/// one owner of this composition; `grid_card`, `child_summary` and this
/// module's own `render_child` all call it instead of deciding count-vs-date
/// on their own.
///
/// `count_label` is `Some` when the card shows a count (with or without a
/// date of its own) — a folder's place only ever shows beside ITS OWN date,
/// never beside a bare count.
/// `credits` is a leaf's listed names; a folder never shows them, so the
/// count arm ignores it.
pub(crate) fn meta_text(
    date_display: Option<&str>,
    when_html: Option<&str>,
    place: Option<&str>,
    count_label: Option<&str>,
    credits: Option<&str>,
) -> String {
    match count_label {
        Some(count) => match date_display {
            Some(date) => format!("{} · {}", with_place_html(date, place), html_escape(count)),
            None => html_escape(count),
        },
        None => leaf_meta(date_display.unwrap_or(""), when_html, place, credits),
    }
}

/// The text a card shows when the author wrote no excerpt: the frontmatter
/// `description:`, else the page's first `byline:` row. The folder listing's
/// excerpt fallback and the incremental listing hash read it too, so a card's
/// text is chosen here and nowhere else.
pub(crate) fn explicit_card_text(doc: &ParsedDocument) -> Option<String> {
    moss_core::page::card::card_text(doc.description.as_deref(), doc.byline.first().map(String::as_str))
}

/// What a card reads from the build beyond the page's own facts: the path
/// resolver with the site's directory overrides, and the other passes' results
/// on the document (a term's card, places, credits, section, body pictures).
pub(crate) struct BuildCards<'a> {
    resolver: PathResolver,
    typesetting: Option<&'a str>,
    now: String,
}

impl<'a> BuildCards<'a> {
    pub(crate) fn new(dir_overrides: &std::collections::HashMap<String, String>, typesetting: Option<&'a str>, now: chrono::NaiveDateTime) -> Self {
        Self { resolver: PathResolver::new().with_dir_overrides(dir_overrides.clone()), typesetting, now: crate::build::folder_embed::upcoming::minute(now) }
    }
}

impl moss_core::page::card::CardHost<ParsedDocument> for BuildCards<'_> {
    fn resolve_url(&self, path: &str) -> String { self.resolver.resolve_url(path) }
    fn typesetting(&self) -> Option<&str> { self.typesetting }
    fn now(&self) -> &str { &self.now }
    fn term_card(&self, doc: &ParsedDocument) -> Option<moss_core::page::card::TermCardFacts> {
        doc.term_card.clone()
    }
    fn body_cover(&self, doc: &ParsedDocument) -> Option<String> { doc.card_cover.clone() }
    fn place_names(&self, doc: &ParsedDocument) -> Option<String> { doc.place_names.clone() }
    fn credits(&self, doc: &ParsedDocument) -> Option<String> { doc.card_extras.clone() }
    fn section(&self, doc: &ParsedDocument) -> Option<crate::build::types::SectionAccent> { doc.section.clone() }
    fn pictures_after_cover(&self, doc: &ParsedDocument, n: usize) -> Vec<String> {
        crate::build::page::cover::card_pictures_after_cover(doc.listing_cover(), doc.hero_image_url.as_deref(), &doc.body_cover_candidates, n)
    }
}

/// The links of an event card whose page has a web `tickets:` or `online:`
/// address. Such a card is not one anchor: its title links to the page, and
/// the action links sit beside it. Every card kind (summary, grid, list row)
/// builds its markup from these two strings, so the shape is decided here once.
pub(crate) struct EventCardLinks {
    /// `<a class="moss-card-link">` around the escaped title.
    pub title: String,
    /// The Tickets and Online links, in that order, each naming the event for
    /// assistive technology.
    pub actions: String,
}

/// `None` for every card without a web `tickets:` or `online:` address; those
/// keep their whole-card anchor and render byte-identically to before.
pub(crate) fn event_card_links(props: &ChildItemProps, lang: crate::i18n::Language) -> Option<EventCardLinks> {
    let tickets = props
        .tickets
        .as_deref()
        .and_then(|u| moss_core::event::when::tickets_link(u, lang, Some(&props.title)));
    let online = props
        .online
        .as_deref()
        .and_then(|u| moss_core::event::when::online_link(u, lang, Some(&props.title)));
    if tickets.is_none() && online.is_none() {
        return None;
    }
    let title = html_escape(&props.title);
    Some(EventCardLinks {
        title: match props.url.as_deref() {
            Some(url) => format!(r#"<a class="moss-card-link" href="{}">{title}</a>"#, html_escape(url)),
            None => title,
        },
        actions: format!("{}{}", tickets.unwrap_or_default(), online.unwrap_or_default()),
    })
}

/// The deepest `<a>` nesting in `html`, for tests that assert a card holds no
/// anchor inside another.
#[cfg(test)]
pub(crate) fn max_anchor_depth(html: &str) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        if rest.starts_with("<a ") {
            depth += 1;
            deepest = deepest.max(depth);
        } else if rest.starts_with("</a>") {
            depth = depth.saturating_sub(1);
        }
        rest = &rest[1..];
    }
    deepest
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
                crate::build::page::meta::render_description_html(desc)
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
#[path = "child_list_tests.rs"]
mod tests;
