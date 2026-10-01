//! Child summary component for children lists
//!
//! Renders a full-width, text-dominant summary for articles and folders.
//! The meta slot is sort-driven: it surfaces a date on Date-axis listings
//! and is omitted entirely on Weight/Title axes (no empty div).

use super::child_list::{self, ChildItemProps};
use super::date::extract_year;
use crate::build::media::cover::{self, html_escape, CoverType};
use crate::build::page::meta::render_description_html;

/// Renders a child summary as HTML, with the meta slot resolved against the
/// parent listing's resolved sort axis.
///
/// `media_lookup`: optional variant manifest. When `Some`, the cover image
/// is routed through `image_render::synthesize_image_html`; when `None`,
/// the legacy regex pass picks up the bare `<img>`.
///
/// `sort_axis`: parent's resolved sort axis. Date axis surfaces
/// `date_display`; Weight and Title omit the meta slot entirely. Task 10
/// adds a separate count subtitle for folder cards on non-date axes.
///
/// The name says "sort", but every use of it below is about THE META SLOT —
/// what the card says, not how the list was ordered. A caller with no sort at
/// all (the hand-picked `:::grid {.summary}` fence) passes `Date` because a
/// reader picks a piece by its date, and gets the author's cell order.
pub fn render_with_sort(
    props: &ChildItemProps,
    lang: crate::i18n::Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    sort_axis: moss_core::sort::SortAxis,
) -> String {
    let mut body = String::new();

    // Kicker + date composition. The kicker carries publisher (or any
    // explicit overline) and, on Date-axis listings, absorbs the date as
    // `{kicker} · {YYYY}` so the overline is one line. When both kicker
    // and date are present the separate meta slot is suppressed to avoid
    // duplicating the date. Single-source cases fall back to the previous
    // behavior: meta carries the date if there's no kicker.
    let kicker_base = props.kicker.as_deref().filter(|s| !s.is_empty());
    let date_year = if sort_axis.shows_date() {
        props.date_raw.as_deref().and_then(extract_year)
            .or_else(|| props.date_display.as_deref().and_then(extract_year))
            .map(|y| y.to_string())
    } else {
        None
    };
    let kicker_text = match (kicker_base, date_year.as_deref()) {
        (Some(k), Some(y)) => Some(format!("{} · {}", k, y)),
        (Some(k), None) => Some(k.to_string()),
        (None, _) => None,
    };

    // Linkblog cards (with `external_url:`) get a `★` inside the kicker
    // pointing to the local archive — the card title links to the
    // outlet, the `★` to the author's copy. The kicker stays inside the card
    // head (matching the design for ordinary cards) so the visual
    // layout doesn't shift. Putting `<a>★</a>` inside the kicker is
    // valid IFF the card's outer element is NOT an anchor; the card
    // structure switches to `<div class="moss-card">` for linkblog
    // cards, with title + cover as individual inner anchors (see
    // `is_linkblog` branch in the emit below).
    let permalink = props.permalink.as_deref().filter(|s| !s.is_empty());
    let is_linkblog = permalink.is_some() && kicker_text.is_some();
    let perma_label = crate::i18n::t(lang, "permalink_to").replace("{}", &html_escape(&props.title));
    let kicker_html = match (&kicker_text, permalink, is_linkblog) {
        (Some(k), Some(perma), true) => format!(
            r#"<div class="moss-card-kicker">{} <a class="moss-card-permalink" href="{}" title="{}" aria-label="{}">★</a></div>"#,
            html_escape(k),
            html_escape(perma),
            perma_label,
            perma_label,
        ),
        (Some(k), _, _) => format!(r#"<div class="moss-card-kicker">{}</div>"#, html_escape(k)),
        (None, _, _) => String::new(),
    };

    // Meta slot — sort-driven; suppressed when the kicker absorbed the
    // date (kicker + extractable year). Falls back to the separate
    // meta when the kicker exists but the date could not be
    // year-formatted (e.g. CJK-numeral dates). A folder's own date (its
    // home page's `date:`, if any) and place lead the count the same way a
    // page's date and place would read alone — one owner for every listing
    // form's composition, `child_list::meta_text`.
    let date_merged_into_kicker = kicker_base.is_some() && date_year.is_some();
    let meta_html = match (props.child_count, sort_axis) {
        // Folder: the count is always shown, on every sort axis — a bare
        // folder's own `location:` is not a date to sit beside, but its own
        // `date:`, if any, is exactly that.
        (Some(count), _) => {
            let count_label = crate::i18n::article_count_label(lang, count, typesetting);
            let text = child_list::meta_text(props.date_display.as_deref(), props.place.as_deref(), Some(&count_label));
            format!(r#"<div class="moss-card-meta">{}</div>"#, text)
        }
        (None, axis) if axis.shows_date() && date_merged_into_kicker => {
            // The kicker already carries the date; the meta slot's only
            // remaining job is the place, if there is one — never an empty
            // `.moss-card-meta`.
            match props.place.as_deref() {
                Some(place) => format!(r#"<div class="moss-card-meta">{}</div>"#, child_list::with_place("", Some(place))),
                None => String::new(),
            }
        }
        (None, axis) if axis.shows_date() => props
            .date_display
            .as_deref()
            .map(|date| format!(r#"<div class="moss-card-meta">{}</div>"#, child_list::with_place(date, props.place.as_deref())))
            .unwrap_or_default(),
        _ => String::new(),
    };

    // Title: wrapped in an anchor for linkblog cards (so it stays
    // clickable now that the outer card is a `<div>`, not an `<a>`);
    // bare `<h3>` for ordinary cards (outer card-anchor handles the
    // click target).
    let title_html = if is_linkblog {
        format!(
            r#"<a class="moss-card-title-link" href="{}"><h3 class="moss-card-title">{}</h3></a>"#,
            html_escape(&props.url),
            html_escape(&props.title),
        )
    } else {
        format!(r#"<h3 class="moss-card-title">{}</h3>"#, html_escape(&props.title))
    };

    // Kicker, meta, title, in block flow directly inside `.moss-card-body`
    // (a flex item, not a container, so nothing lays them side by side). The
    // same reading order in both writing modes: the brow leads — top to
    // bottom across the page, right to left under vertical-rl. A vertical
    // reorder (title first) once lived here and is why the date did not lead.
    body.push_str(&kicker_html);
    body.push_str(&meta_html);
    body.push_str(&title_html);

    // Description: wrapped in an anchor for linkblog cards so it
    // remains a click target now that the outer is `<div>`.
    if let Some(ref desc) = props.description {
        let desc_inner = format!(r#"<p class="moss-card-description">{}</p>"#, render_description_html(desc));
        if is_linkblog {
            body.push_str(&format!(
                r#"<a class="moss-card-description-link" href="{}">{}</a>"#,
                html_escape(&props.url),
                desc_inner,
            ));
        } else {
            body.push_str(&desc_inner);
        }
    }

    let cover_type = CoverType::resolve(props.cover.as_deref(), props.cover_type);
    let cover_inner = match (&props.cover, cover_type) {
        (Some(url), Some(ct)) => {
            let (cover_path, attrs_str) = moss_core::media::split_pipe(url);
            let cover_attrs = moss_core::media::parse_media_attrs(attrs_str);
            cover::render_cover_html(cover_path, ct, &props.title, "moss-card-cover", &cover_attrs, true, media_lookup, false)
        }
        _ => String::new(),
    };
    // Cover wrapped in an anchor for linkblog cards.
    let cover_html = if is_linkblog && !cover_inner.is_empty() {
        format!(
            r#"<a class="moss-card-cover-link" href="{}">{}</a>"#,
            html_escape(&props.url),
            cover_inner,
        )
    } else {
        cover_inner
    };

    // Card outer element:
    //
    // - Ordinary card: `<a class="moss-card" href="...">` wraps the
    //   entire card so any click navigates to the URL. The kicker, title,
    //   cover, description are all descendants of this single anchor.
    //
    // - Linkblog card: `<div class="moss-card">`, individual children
    //   are anchors. This is the only way to put `<a>★</a>` inside the
    //   kicker (HTML forbids nested anchors). Title + cover + description
    //   each get their own `<a>` to the external outlet; the `★` inside
    //   the kicker goes to the local archive.
    //
    // The `title=`/`aria-label=` on the `★` are intentional published-
    // site HTML (no `data-tooltip=` portal on the rendered page —
    // CLAUDE.md "Tooltips" rule applies to the editor UI only).
    if is_linkblog {
        format!(
            r#"<div class="moss-card" data-linkblog><div class="moss-card-row"><div class="moss-card-body">{}</div>{}</div></div>"#,
            body,
            cover_html,
        )
    } else {
        format!(
            r#"<a href="{}" class="moss-card"><div class="moss-card-row"><div class="moss-card-body">{}</div>{}</div></a>"#,
            html_escape(&props.url),
            body,
            cover_html,
        )
    }
}

#[cfg(test)]
#[path = "child_summary_tests.rs"]
mod tests;
