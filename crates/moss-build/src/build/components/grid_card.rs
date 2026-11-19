//! Folder card component for homepage and grid shortcodes
//!
//! Renders a grid of folder cards with cover images and titles.
//! Cards display cover image on top, with title and article count on the same row below.
//!
//! The input is [`ChildItemProps`], the one reading of a page every card
//! surface shares (`child_list::props_for_document`). This module used to own
//! a second struct, `FolderCardProps`, describing the same page in its own
//! vocabulary; the two producers that filled it — the `children_style: grid`
//! listing and the hand-picked `:::grid` card — each re-derived every field
//! and drifted on the date rung. Deleted 2026-09-12. The one thing that struct
//! carried which `ChildItemProps` does not, the cover's band colour, is
//! resolved here, where it is consumed.

use std::path::Path;

use super::child_list::{self, event_card_links, ChildItemProps};
use crate::i18n::{self, Language};
use crate::build::media::cover::{self, html_escape, CoverType};
use moss_core::render::image::Loading;
use crate::build::page::meta::render_description_html_in_link;

/// Renders a single folder card as HTML with typesetting context.
///
/// When `typesetting == Some("vertical")` and `lang.is_cjk()`, the article
/// count is formatted as CJK numerals (e.g. "12" → "十二篇") to match the
/// behavior of `child_summary::render_with_sort`.
///
/// `media_lookup`: optional variant manifest. When `Some`, the card's cover
/// image is routed through `image_render::synthesize_image_html`; `None`
/// emits a bare `<img>`. Pass `Some` from any caller that has a
/// `MediaDimensionLookup` in scope (e.g., `build/render/html.rs`).
///
/// `root`: the project root, which the cover's band colour is read under
/// (`color_extract::resolve_card_color`); `None` disables file reads, so an
/// image cover then colours only through its `|color=` override.
///
/// `loading`: when this card's cover is fetched; a listing hands it out
/// through [`CoverLoading`].
///
/// `list_has_covers`: whether ANY card in this caller's row/list has a cover
/// of its own. A hand-picked `:::grid` cell computes it across its own
/// siblings the same way [`render_list_with_typesetting`] does
/// (`build::render::grid_cells::apply_collection_cards`); a lone card passes
/// `false`. When true and THIS card has no cover of its own, the
/// cover slot becomes a quote card (`data-cover="quote"`) carrying the
/// page's description instead of the empty `.moss-card-no-cover`
/// placeholder. With no description the slot is an empty `aria-hidden` tile:
/// the title is already printed below, so repeating it would show it twice.
/// A uniformly coverless list (e.g. a term index) keeps the plain
/// placeholder, because there every card is the same shape and the
/// placeholder is invisible CSS (`moss-card-cover { display: none }`);
/// the quote slot exists only to fill the gap a MIXED list would
/// otherwise leave.
pub fn render_item_with_typesetting(
    props: &ChildItemProps,
    root: Option<&Path>,
    lang: Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    loading: Loading,
    list_has_covers: bool,
    // The `sizes=` of the `:::grid` cell the card fills; `None` in a listing.
    grid_cell_sizes: Option<&str>,
) -> String {
    // A folder's meta is its count (preceded by its own date and place when
    // its home page declares one); a leaf's is its date and place, or
    // nothing. The slot is emitted either way so the two shapes keep one
    // layout. One owner across every listing form — see
    // `child_list::meta_text`, which also HTML-escapes the result (this slot
    // used to print the count unescaped, safe only because it was always
    // machine-generated text).
    let count_label = props.count_label(lang, typesetting);
    let count_text = super::child_list::meta_text(
        props.date_display.as_deref(),
        props.when_html.as_deref(),
        props.place.as_deref(),
        count_label.as_deref(),
        props.credits.as_deref(),
    );

    let cover_type = CoverType::resolve(props.cover.as_deref(), props.cover_type);

    let cover_html = match (&props.cover, cover_type) {
        (Some(url), Some(ct)) => {
            let (cover_path, attrs_str) = moss_core::media::split_pipe(url);
            let cover_attrs = moss_core::media::parse_media_attrs(attrs_str);
            // The link is named by its title (aria-label below), so a cover
            // with a title is decorative. Without a title the link has no name,
            // and the cover supplies one.
            let cover_alt = if props.title.trim().is_empty() { "cover" } else { "" };
            cover::render_cover_html(cover_path, ct, cover_alt, "moss-card-cover", &cover_attrs, true, media_lookup, loading, grid_cell_sizes)
        }
        _ if list_has_covers => {
            // The description is markdown (`ChildItemProps::description`'s
            // contract), rendered safely inline.
            let quote = props.description.as_deref().map(str::trim).filter(|s| !s.is_empty());
            format!(
                r#"<div class="moss-card-cover" data-cover="quote"{}>{}</div>"#,
                if quote.is_none() { r#" aria-hidden="true""# } else { "" },
                quote.map(|d| format!("<p>{}</p>", render_description_html_in_link(d))).unwrap_or_default(),
            )
        }
        _ => moss_core::ast::link_card::NO_COVER.to_string(),
    };

    // One shared ladder for every cover type: `|color=` override →
    // cache/extract by type → iframe dark default.
    let cover_color = super::color_extract::resolve_card_color(
        props.cover.as_deref(),
        cover_type,
        root,
        media_lookup,
    );
    // An image cover's own shape, where its size was read: a theme lays out
    // by it without waiting for the picture or running script.
    let cover_ratio = match (props.cover.as_deref(), cover_type, media_lookup) {
        (Some(cover), Some(CoverType::Image), Some(lookup)) => {
            moss_core::render::image::lookup_dims(lookup.asset_snapshot(), moss_core::media::split_pipe(cover).0)
        }
        _ => None,
    };
    // One `style` attribute carries the cover's band colour, its shape and
    // the page's section colour.
    let style_decls: Vec<String> = cover_color
        .iter()
        .map(|c| format!("--item-cover-color: {}", html_escape(c)))
        .chain(cover_ratio.iter().filter(|(w, h)| *w > 0 && *h > 0).map(|(w, h)| format!("--item-cover-ratio: {w} / {h}")))
        .chain(props.section_color.iter().map(|a| html_escape(&crate::build::scan::section_text::section_style(a))))
        .collect();
    let cover_color_attrs = format!(
        "{}{}",
        if cover_color.is_some() { " data-cover-color" } else { "" },
        if style_decls.is_empty() { String::new() } else { format!(r#" style="{}""#, style_decls.join("; ")) },
    );

    let kicker_html = props.kicker.as_deref()
        .filter(|s| !s.is_empty())
        .map(|k| format!(r#"<span class="moss-card-kicker">{}</span>"#, html_escape(k)))
        .unwrap_or_default();

    // Slot order: kicker, meta, title (horizontal-mode layout).
    //
    // Meta IS the visual kicker — a small kicker above (horizontal) or to the
    // right of (vertical) the title, since production callers populate meta
    // and leave kicker empty: same uppercase overline treatment, same
    // position above the title. Emitting `kicker, meta, title` matches
    // the parallel emitter at child_summary::render_with_sort so
    // the two card surfaces agree on slot order.
    //
    // Description slot — a frontmatter-only paragraph below the title, placed
    // as in child_summary, on every card that shows no count: a folder's "N articles"
    // says what is behind it, and with the count hidden the description is all
    // it has left. Left out when the quote slot above already carries it.
    let used_in_quote_slot = list_has_covers && props.cover.is_none();
    let description_html = props.description.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|_| count_label.is_none())
        .filter(|_| !used_in_quote_slot)
        .map(|d| format!(r#"<p class="moss-card-description"{}>{}</p>"#, child_list::lang_attr(props.lang.as_ref()), render_description_html_in_link(d)))
        .unwrap_or_default();

    // An event with `tickets:` or `online:` is a `<div>` whose title is the
    // page link, with its own action links after the meta (see
    // `event_card_links`). Otherwise the card is one link, whose accessible
    // name is the title alone: meta, description and the cover's quote are not
    // part of it. An empty title leaves the link unnamed.
    let event = event_card_links(props, lang);
    let (open, close, title_html, actions_html) = match &event {
        Some(links) => (
            format!(r#"<div class="moss-card"{}{}>"#, props.root_attrs(), cover_color_attrs),
            "</div>".to_string(),
            format!(r#"<span class="moss-card-title"{}>{}{}</span>"#, child_list::lang_attr(props.lang.as_ref()), links.title, child_list::lang_mark(props.lang.as_ref())),
            links.actions.as_str(),
        ),
        None => {
            // A page-less card is a plain box, which may not carry a name of its own.
            let label_attr = if props.title.trim().is_empty() || props.url.is_none() {
                String::new()
            } else {
                // An aria-label replaces the content's name, mark included.
                let named = props.lang.as_ref().filter(|l| !l.name.is_empty()).map(|l| format!("{} ({})", props.title, l.name));
                format!(r#" aria-label="{}""#, html_escape(named.as_deref().unwrap_or(&props.title)))
            };
            let (open, close) = child_list::link_or_box(
                props.url.as_deref(),
                "div",
                &format!(r#" class="moss-card"{}{}{}"#, label_attr, props.root_attrs(), cover_color_attrs),
            );
            (
                open,
                close,
                format!(r#"<span class="moss-card-title"{}>{}{}</span>"#, child_list::lang_attr(props.lang.as_ref()), html_escape(&props.title), child_list::lang_mark(props.lang.as_ref())),
                "",
            )
        }
    };

    // A folder card that hides its count and has nothing else to say gets no
    // meta element; every other card keeps its (possibly empty) slot.
    let meta_html = if props.child_count.is_some() && count_text.is_empty() {
        String::new()
    } else {
        format!(r#"<span class="moss-card-meta">{}</span>"#, count_text)
    };

    format!(
        r#"{}{}<div class="moss-card-content">{}{}{}{}{}{}</div>{}"#,
        open,
        cover_html,
        kicker_html,
        meta_html,
        title_html,
        child_list::subtitle_html(props.subtitle.as_deref()),
        description_html,
        actions_html,
        close,
    )
}

/// How many cards of a grid count as its first row on a desktop screen: a
/// cover track is 280px or wider in a column of at most about 1200px. Lazy
/// loading their covers would delay the page's largest paint, which is often
/// a card in that row and not the first.
const FIRST_ROW: usize = 4;

/// Hands out the loading of a card grid's covers in reading order: a cover
/// among the first [`FIRST_ROW`] cards loads with the page, the first such
/// cover at high priority. A page whose opening picture already holds the one
/// high-priority fetch passes `lazy` and every cover loads lazily.
pub(crate) struct CoverLoading {
    first_row_left: usize,
    priority_left: bool,
}

impl CoverLoading {
    pub(crate) fn new(lazy: bool) -> Self {
        Self { first_row_left: if lazy { 0 } else { FIRST_ROW }, priority_left: !lazy }
    }

    /// The loading of the next card's cover; a card without one still takes its place in the row.
    pub(crate) fn next(&mut self, has_cover: bool) -> Loading {
        let in_first_row = self.first_row_left > 0;
        self.first_row_left = self.first_row_left.saturating_sub(1);
        match (has_cover && in_first_row, self.priority_left) {
            (false, _) => Loading::Lazy,
            (true, true) => {
                self.priority_left = false;
                Loading::Priority
            }
            (true, false) => Loading::Eager,
        }
    }
}

/// Renders a grid of folder cards as HTML with typesetting context.
///
/// Passes `typesetting` through to `render_item_with_typesetting` so that
/// CJK numeral formatting is applied when `typesetting == Some("vertical")`
/// and `lang.is_cjk()`.
///
/// `media_lookup`: optional variant manifest. When `Some`, each card's
/// cover image is routed through the synthesizer.
/// `is_embed` — see `folder_embed::generate_children`'s doc comment — stamps
/// a bare `data-embed` attribute on the `.moss-cards-container` this
/// function builds, distinguishing a body embed from the frontmatter-
/// synthesized listing for CSS block-rhythm purposes.
pub fn render_list_with_typesetting<C: std::borrow::Borrow<ChildItemProps>>(
    cards: &[C],
    root: Option<&Path>,
    lang: Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    sort_axis: moss_core::sort::SortAxis,
    is_embed: bool,
    placement: &moss_core::media::Placement,
    // The page already has its one eager image elsewhere: no card here is
    // marked eager, not even the first covered one.
    lazy_cards: bool,
) -> String {
    if cards.is_empty() {
        return String::new();
    }

    // Computed up front (not just for `data-list-has-covers` below) so each
    // item render can decide whether ITS OWN missing cover should become a
    // quote slot — see `render_item`'s `list_has_covers` doc.
    let has_covers = cards.iter().any(|c| c.borrow().cover.is_some());

    let mut loading = CoverLoading::new(lazy_cards);
    let items: String = cards
        .iter()
        .map(|c| {
            let c = c.borrow();
            let loading = loading.next(c.cover.is_some());
            render_item_with_typesetting(c, root, lang, typesetting, media_lookup, loading, has_covers, None)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // `.moss-cards-container` carries `container-type: inline-size`, which
    // cannot sit on the grid itself. The CSS picks `--moss-card-min` from the
    // axis and `data-list-has-covers`.
    let list_attrs = super::list_attrs(sort_axis, super::ListHolds::of(cards.iter().map(|c| c.borrow().is_collection())));
    let cover_attr = if has_covers { " data-list-has-covers" } else { "" };
    let container_attr = if is_embed { " data-embed" } else { "" };
    // Same emit-nothing-when-empty rule as `components::cards_container`.
    let place = moss_core::render::placement::placement_attrs(placement);
    let align = place.align_suffix();

    format!(
        r#"<div class="moss-cards-container{}"{}{}{}><div class="moss-cards" data-layout="grid"{}{}>{}</div></div>"#,
        align, place.data_width_attr, place.size_style_attr, container_attr, list_attrs, cover_attr, items
    )
}

/// Renders a bare external grid-cell link through the SAME `.moss-card`
/// shell [`render_item_with_typesetting`] builds for an internal page card
/// — the owner's "one card kind" decision: a link out of the site is a
/// card too, not a different-looking preview.
///
/// `title`, `domain` and `favicon` are the caller's business
/// (`build::render::grid_cells::external_card_markup` works out author-text-
/// vs-fetched precedence and reads the link-metadata cache); `cover_html`
/// is fully-formed HTML — either the author's own image, rendered through
/// the ordinary cover pipeline, or the plain no-cover placeholder when
/// there's nothing to show yet. This function only assembles the shell.
pub fn render_external_card(
    href: &str,
    title: &str,
    domain: &str,
    favicon: Option<&str>,
    cover_html: &str,
) -> String {
    // Mirrors `render_link_preview`'s favicon handling (deleted alongside
    // the `.link-preview` shell this replaces): route through the
    // synthesizer with `ImageContext::Favicon`, which short-circuits to a
    // bare 16×16 `<img>` — no manifest, no `<picture>`, no LQIP.
    let favicon_assets = moss_core::asset_snapshot::AssetSnapshot::new();
    let favicon_html = favicon
        .filter(|f| !f.is_empty())
        .map(|f| {
            moss_core::render::image::synthesize_image_html(
                f,
                "",
                &favicon_assets,
                moss_core::render::image::ImageContext::Favicon,
                &moss_core::render::image::ImageRenderOptions {
                    class: Some("moss-card-kicker-favicon"),
                    ..Default::default()
                },
            )
        })
        .unwrap_or_default();
    let kicker_html = format!(
        r#"<span class="moss-card-kicker">{}{}</span>"#,
        favicon_html,
        html_escape(domain)
    );
    format!(
        r#"<a href="{}" class="moss-card" data-external target="_blank" rel="noopener">{}<div class="moss-card-content">{}<span class="moss-card-meta"></span><span class="moss-card-title">{}</span></div></a>"#,
        html_escape(href),
        cover_html,
        kicker_html,
        html_escape(title),
    )
}

#[cfg(test)]
#[path = "grid_card_tests.rs"]
mod tests;
