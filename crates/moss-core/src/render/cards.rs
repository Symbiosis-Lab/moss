//! A listing's cards and rows as HTML, for the build and for a host's
//! preview alike. What only a host can read (picture sizes, variants, a
//! cover's colour) comes in through [`CardPictures`].

pub mod grid;
pub mod listing;
pub mod row;
pub mod summary;
pub mod year_group;

use crate::media::html_escape;
use crate::page::card::{ChildItemProps, CoverType, RowLang, EVENT_SEP};
use crate::render::image::Loading;

/// What a card reads about its pictures beyond their addresses. The defaults
/// draw the originals with no colour band, which is what a host that has not
/// scanned the site's media can show.
pub trait CardPictures {
    /// The site's media index: sizes, variants and placeholders.
    fn assets(&self) -> Option<&crate::asset_snapshot::AssetSnapshot> { None }
    /// A cover's `--item-cover-color`. `cover` is pipe-encoded, as on
    /// [`ChildItemProps::cover`].
    fn card_color(&self, _cover: Option<&str>, _cover_type: Option<CoverType>) -> Option<String> { None }
}

impl CardPictures for () {}

/// Render the appropriate HTML for a cover element.
///
/// When `attrs` has non-default values, an inline `style` attribute is added to
/// `<img>` and `<video>` tags with `object-fit` / `object-position`.
/// Iframe covers ignore media attrs (iframes have their own sizing).
///
/// All parameters are HTML-escaped in the output.
pub fn cover_html(
    url: &str,
    cover_type: CoverType,
    alt: &str,
    css_class: &str,
    attrs: &crate::media::MediaAttrs,
    block_interaction: bool,
    // `Some` draws an image through the synthesizer (dimensions, LQIP and
    // the WebP `<picture>`); `None` draws the bare `<img>`.
    assets: Option<&crate::asset_snapshot::AssetSnapshot>,
    loading: Loading,
    // The `sizes=` of the grid cell the cover is drawn in; `None` in a listing.
    grid_cell_sizes: Option<&str>,
) -> String {
    let alt_escaped = html_escape(alt);
    let css_class = html_escape(css_class);
    let style_attr = attrs
        .to_inline_style()
        .map(|s| format!(r#" style="{}""#, html_escape(&s)))
        .unwrap_or_default();

    match cover_type {
        CoverType::Image => {
            if let Some(assets) = assets {
                let style = attrs.to_inline_style();
                let opts = crate::render::image::ImageRenderOptions {
                    loading,
                    style: style.as_deref(),
                    fit: attrs.fit,
                    grid_cell_sizes,
                    ..Default::default()
                };
                let img_html = crate::render::image::synthesize_image_html(
                    url,
                    alt,
                    assets,
                    crate::render::image::ImageContext::FolderCardCover,
                    &opts,
                );
                return format!(r#"<div class="{css_class}">{img_html}</div>"#);
            }
            let url = html_escape(url);
            let loading_attr = if loading == Loading::Lazy { "" } else { loading.attrs() };
            format!(r#"<div class="{css_class}"><img src="{url}" alt="{alt_escaped}"{loading_attr}{style_attr} /></div>"#)
        }
        CoverType::Video => {
            let src = html_escape(&crate::asset_paths::to_mp4(url));
            let poster = html_escape(&crate::asset_paths::to_thumb(url));
            // keep in sync with crate::render::video::AMBIENT_PLAYBACK_ATTRS
            // (covers intentionally omit `autoplay` — they are hover-played, not auto)
            format!(
                r#"<div class="{css_class}"><video src="{src}"{style_attr} muted loop playsinline preload="metadata"></video><img src="{poster}" alt="{alt_escaped}" class="cover-thumb"{style_attr} /></div>"#
            )
        }
        CoverType::Iframe => {
            let url = html_escape(url);
            // `position:relative` inline anchors the overlay even where a
            // caller's class sets no positioning context.
            let (container_style, overlay) = if block_interaction {
                (
                    r#" style="position:relative""#,
                    r#"<div style="position:absolute;inset:0"></div>"#,
                )
            } else {
                ("", "")
            };
            format!(
                r#"<div class="{css_class}"{container_style}><iframe src="{url}" title="{alt_escaped}" loading="lazy" sandbox="allow-scripts"></iframe>{overlay}</div>"#
            )
        }
    }
}

/// The container every children listing wears: a `.moss-cards-container`
/// wrapping one `.moss-cards` of the given layout.
///
/// One source of truth, because the hand-picked `:::grid {.summary}` variant
/// emits the SAME container a folder listing does — that identity is what lets
/// the two share `site.css`'s `.moss-cards[data-layout="list"]` rules and the
/// render gates already written against them.
///
/// `lead` stamps `data-lead`, which `site.css` reads to make the first card
/// lead and the rest a register (`children_style: lead`).
///
/// `is_embed` stamps `data-embed`, which marks a body-embedded listing (a
/// `![[folder/]]` embed or a grid fence) apart from the trailing automatic one
/// so CSS can give it its own block rhythm.
///
/// `placement` is the listing's own width / float / size. It emits NOTHING
/// when empty, which is load-bearing: `render::html::annotate_children_listing`
/// stamps the editor-preview chip by a literal `replacen` on
/// `<div class="moss-cards-container">`, and a stray attribute would make
/// that match silently stop matching with no test going red.
pub fn cards_container(
    layout: &str,
    lead: bool,
    is_embed: bool,
    placement: &crate::media::Placement,
    axis: crate::sort::SortAxis,
    holds: ListHolds,
    inner: &str,
) -> String {
    let place = crate::render::placement::placement_attrs(placement);
    let align = place.align_suffix();
    format!(
        "<div class=\"moss-cards-container{}\"{}{}{}><div class=\"moss-cards\" data-layout=\"{}\"{}{}>{}</div></div>",
        align,
        place.data_width_attr,
        place.size_style_attr,
        if is_embed { " data-embed" } else { "" },
        layout,
        if lead { " data-lead" } else { "" },
        list_attrs(axis, holds),
        inner,
    )
}

/// What a listing's cards stand for: pages, collections (a folder or a
/// term, drawn with a count of what it holds), or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListHolds {
    Entries,
    Collections,
    Mixed,
}

impl ListHolds {
    /// From each card in turn: whether it stands for a collection.
    pub fn of(collections: impl IntoIterator<Item = bool>) -> Self {
        let (mut any_collection, mut any_entry) = (false, false);
        for is_collection in collections {
            if is_collection { any_collection = true } else { any_entry = true }
        }
        match (any_collection, any_entry) {
            (true, true) => ListHolds::Mixed,
            (true, false) => ListHolds::Collections,
            (false, _) => ListHolds::Entries,
        }
    }
}

/// The attributes every listing wrapper carries, so a theme can tell its
/// listings apart by fact: the order (`data-list-axis`) and what the cards
/// stand for (`data-list-holds`).
pub fn list_attrs(axis: crate::sort::SortAxis, holds: ListHolds) -> String {
    use crate::sort::SortAxis;
    let axis = match axis {
        // DateAsc presents as Date: only the comparator's direction differs.
        SortAxis::Date | SortAxis::DateAsc => "date",
        SortAxis::Weight => "weight",
        SortAxis::Title => "title",
    };
    let holds = match holds {
        ListHolds::Entries => "entries",
        ListHolds::Collections => "collections",
        ListHolds::Mixed => "mixed",
    };
    format!(r#" data-list-axis="{axis}" data-list-holds="{holds}""#)
}


/// ` lang="de"` for an element holding a page's words in another language, or nothing.
pub fn lang_attr(lang: Option<&RowLang>) -> String {
    lang.map(|l| format!(r#" lang="{}""#, html_escape(&l.tag))).unwrap_or_default()
}

/// The small mark naming a row's language, or nothing for a row in the listing's own.
pub fn lang_mark(lang: Option<&RowLang>) -> String {
    match lang {
        Some(l) if !l.name.is_empty() => {
            format!(r#"<span class="moss-lang-mark" lang="{}">{}</span>"#, html_escape(&l.listing_tag), l.name)
        }
        _ => String::new(),
    }
}

/// A row's or card's own element, opening and closing: an anchor to `url`,
/// or, for an entry with no page, the same box as a `box_tag` (`span` where it
/// sits inside a `<p>`). `attrs` starts with a space (` class="moss-card"`).
pub fn link_or_box(url: Option<&str>, box_tag: &'static str, attrs: &str) -> (String, String) {
    match url {
        Some(url) => (format!(r#"<a href="{}"{attrs}>"#, html_escape(url)), "</a>".to_string()),
        None => (format!("<{box_tag}{attrs}>"), format!("</{box_tag}>")),
    }
}

/// The line under a card's or row's title; nothing when there is no subtitle.
pub fn subtitle_html(subtitle: Option<&str>) -> String {
    subtitle
        .map(|s| format!(r#"<span class="moss-card-subtitle">{}</span>"#, html_escape(s)))
        .unwrap_or_default()
}

/// A leaf's date markup (already HTML) with its resolved place after it. The
/// place is author text, escaped here, and a blank place counts as none.
fn with_place(date_html: &str, place: Option<&str>) -> String {
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
pub fn leaf_meta(date_html: &str, when_html: Option<&str>, place: Option<&str>, credits: Option<&str>) -> String {
    let base = match when_html {
        Some(h) => with_event_place(h, place),
        None => with_place(date_html, place),
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
/// one owner of this composition for every listing form.
///
/// `count_label` is `Some` when the card shows a count (with or without a
/// date of its own) — a folder's place only ever shows beside ITS OWN date,
/// never beside a bare count.
/// `credits` is a leaf's listed names; a folder never shows them, so the
/// count arm ignores it.
pub fn meta_text(
    date_display: Option<&str>,
    when_html: Option<&str>,
    place: Option<&str>,
    count_label: Option<&str>,
    credits: Option<&str>,
) -> String {
    match count_label {
        Some(count) => match date_display {
            Some(date) => format!("{} · {}", with_place(date, place), html_escape(count)),
            None => html_escape(count),
        },
        None => leaf_meta(date_display.unwrap_or(""), when_html, place, credits),
    }
}

/// The links of an event card whose page has a web `tickets:` or `online:`
/// address. Such a card is not one anchor: its title links to the page, and
/// the action links sit beside it. Every card kind (summary, grid, list row)
/// builds its markup from these two strings, so the shape is decided here once.
pub struct EventCardLinks {
    /// `<a class="moss-card-link">` around the escaped title.
    pub title: String,
    /// The Tickets and Online links, in that order, each naming the event for
    /// assistive technology.
    pub actions: String,
}

/// `None` for every card without a web `tickets:` or `online:` address; those
/// keep their whole-card anchor.
pub fn event_card_links(props: &ChildItemProps, lang: crate::i18n::Language) -> Option<EventCardLinks> {
    let tickets = props
        .tickets
        .as_deref()
        .and_then(|u| crate::event::when::tickets_link(u, lang, Some(&props.title)));
    let online = props
        .online
        .as_deref()
        .and_then(|u| crate::event::when::online_link(u, lang, Some(&props.title)));
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
pub fn max_anchor_depth(html: &str) -> usize {
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

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
