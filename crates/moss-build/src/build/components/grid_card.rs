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

use super::child_list::ChildItemProps;
use crate::i18n::{self, Language};
use crate::build::media::cover::{self, html_escape, CoverType};
use crate::build::page::meta::render_description_html_in_link;

/// Renders a single folder card as HTML with typesetting context.
///
/// When `typesetting == Some("vertical")` and `lang.is_cjk()`, the article
/// count is formatted as CJK numerals (e.g. "12" → "十二篇") to match the
/// behavior of `child_summary::render_with_sort`.
///
/// `media_lookup`: optional variant manifest. When `Some`, the card's cover
/// image is routed through `image_render::synthesize_image_html`. When
/// `None`, falls back to the bare `<img>` shape — the legacy regex pass
/// then retrofits attrs. Pass `Some` from any caller that has a
/// `MediaDimensionLookup` in scope (e.g., `build/render/html.rs`).
///
/// `root`: the project root, which the cover's band colour is read under
/// (`color_extract::resolve_card_color`); `None` disables file reads, so an
/// image cover then colours only through its `|color=` override.
///
/// `eager`: when true, this card's cover gets `loading="eager"
/// fetchpriority="high"`. Used by `render_list_with_typesetting` to mark
/// the first card with a cover as the LCP candidate. Replaces the prior
/// `html.insert_str(pos+5, "loading=\"eager\" ")` mutation pattern with a
/// typed flag so the LCP decision flows through the synthesizer.
///
/// `list_has_covers`: whether ANY card in this caller's row/list has a cover
/// of its own — see the parameter doc on [`render_item`], which this calls
/// straight through to. A hand-picked `:::grid` cell computes it across its
/// own siblings the same way [`render_list_with_typesetting`] does
/// (`build::render::grid_cells::apply_collection_cards`); a caller with no
/// sibling list to compare against (a lone card, or a context that predates
/// this parameter) passes `false`.
pub fn render_item_with_typesetting(
    props: &ChildItemProps,
    root: Option<&Path>,
    lang: Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    eager: bool,
    list_has_covers: bool,
) -> String {
    render_item(props, root, lang, typesetting, media_lookup, eager, list_has_covers)
}

/// Shared by [`render_item_with_typesetting`] (a lone card, or `:::grid`
/// cells whose caller now computes `list_has_covers` across the row) and
/// [`render_list_with_typesetting`] (which already knows whether ANY card
/// in the list has a cover).
///
/// `list_has_covers`: when true and THIS card has no cover of its own, the
/// cover slot becomes a quote card (`data-cover="quote"`) carrying the
/// page's description — or its title, with no description — instead of
/// the empty `.moss-card-no-cover` placeholder. A
/// uniformly coverless list (e.g. a term index) keeps the plain
/// placeholder, because there every card is the same shape and the
/// placeholder is invisible CSS (`moss-card-cover { display: none }`);
/// the quote slot exists only to fill the gap a MIXED list would
/// otherwise leave.
fn render_item(
    props: &ChildItemProps,
    root: Option<&Path>,
    lang: Language,
    typesetting: Option<&str>,
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    eager: bool,
    list_has_covers: bool,
) -> String {
    // A folder's meta is its count (preceded by its own date and place when
    // its home page declares one); a leaf's is its date and place, or
    // nothing. The slot is emitted either way so the two shapes keep one
    // layout. One owner across every listing form — see
    // `child_list::meta_text`, which also HTML-escapes the result (this slot
    // used to print the count unescaped, safe only because it was always
    // machine-generated text).
    let count_label = props
        .child_count
        .map(|count| i18n::article_count_label(lang, count, typesetting));
    let count_text = super::child_list::meta_text(
        props.date_display.as_deref(),
        props.when_html.as_deref(),
        props.place.as_deref(),
        count_label.as_deref(),
    );

    let cover_type = CoverType::resolve(props.cover.as_deref(), props.cover_type);

    let cover_html = match (&props.cover, cover_type) {
        (Some(url), Some(ct)) => {
            let (cover_path, attrs_str) = moss_core::media::split_pipe(url);
            let cover_attrs = moss_core::media::parse_media_attrs(attrs_str);
            cover::render_cover_html(cover_path, ct, &format!("{} cover", props.title), "moss-card-cover", &cover_attrs, true, media_lookup, eager)
        }
        _ if list_has_covers => {
            // The description is markdown (`ChildItemProps::description`'s
            // contract, per `child_list.rs`), rendered safely inline; the
            // title fallback is plain chrome text, so it stays a bare escape.
            let quote_html = match props.description.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(desc) => render_description_html_in_link(desc),
                None => html_escape(&props.title),
            };
            format!(
                r#"<div class="moss-card-cover" data-cover="quote"><p>{}</p></div>"#,
                quote_html
            )
        }
        _ => r#"<div class="moss-card-cover moss-card-no-cover"></div>"#.to_string(),
    };

    // One shared ladder for every cover type: `|color=` override →
    // cache/extract by type → iframe dark default.
    let cover_color_attrs = super::color_extract::resolve_card_color(
        props.cover.as_deref(),
        cover_type,
        root,
        media_lookup,
    )
        .map(|c| format!(r#" data-cover-color style="--item-cover-color: {}""#, html_escape(&c)))
        .unwrap_or_default();

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
    // the parallel emitter at child_summary::render_with_sort (line 64) so
    // the two card surfaces agree on slot order.
    //
    // Pre-fix this emitted `{kicker}{title}{meta}` — placing the meta
    // BELOW the title (where it visually read as a subtitle, not a kicker).
    //
    // Description slot — a paragraph BELOW the title, file/article cards
    // only (`child_count.is_none()`). Folder cards keep their "N articles"
    // meta and render no description. The text is frontmatter-only (the
    // callers leave `props_for_document`'s reading alone, never the
    // auto-extracted excerpt), so the slot stays author-intentional. Mirrors
    // child_summary's below-title `.moss-card-description` so grid and
    // summary cards agree:
    // meta = date overline ABOVE, description = paragraph BELOW the title.
    // Suppressed when the quote slot above already carries this same text
    // (coverless card, covered list) — otherwise it would print twice.
    let used_in_quote_slot = list_has_covers && props.cover.is_none();
    let description_html = props.description.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|_| props.child_count.is_none())
        .filter(|_| !used_in_quote_slot)
        .map(|d| format!(r#"<p class="moss-card-description">{}</p>"#, render_description_html_in_link(d)))
        .unwrap_or_default();

    format!(
        r#"<a href="{}" class="moss-card"{}>{}<div class="moss-card-content">{}<span class="moss-card-meta">{}</span><span class="moss-card-title">{}</span>{}</div></a>"#,
        html_escape(&props.url),
        cover_color_attrs,
        cover_html,
        kicker_html,
        count_text,
        html_escape(&props.title),
        description_html,
    )
}

/// Renders a grid of folder cards as HTML with typesetting context.
///
/// Passes `typesetting` through to `render_item_with_typesetting` so that
/// CJK numeral formatting is applied when `typesetting == Some("vertical")`
/// and `lang.is_cjk()`.
///
/// `media_lookup`: optional variant manifest. When `Some`, each card's
/// cover image is routed through the synthesizer instead of the legacy
/// regex pass. The first card with a cover is marked `eager=true` for
/// LCP optimization — the typed flag replaces the post-render
/// `html.insert_str` hack that prepended `loading="eager"` after the
/// fact.
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
) -> String {
    if cards.is_empty() {
        return String::new();
    }

    // Computed up front (not just for `data-list-has-covers` below) so each
    // item render can decide whether ITS OWN missing cover should become a
    // quote slot — see `render_item`'s `list_has_covers` doc.
    let has_covers = cards.iter().any(|c| c.borrow().cover.is_some());

    let mut found_first_cover = false;
    let items: String = cards
        .iter()
        .map(|c| {
            let c = c.borrow();
            // Mark the first card with a cover as eager (LCP candidate).
            // Pass the flag into the synthesizer via `render_item_with_typesetting`
            // so the eager hint is part of the emitted markup from the
            // start — no post-render mutation needed.
            let eager = !found_first_cover && c.cover.is_some();
            if eager {
                found_first_cover = true;
            }
            render_item(c, root, lang, typesetting, media_lookup, eager, has_covers)
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Wrap in `.moss-cards-container` so the CSS in Task 11 can
    // apply `container-type: inline-size` (CSS containment queries
    // can't sit on the grid itself without breaking layout). Inner
    // `.moss-cards[data-layout="grid"]` carries `data-list-axis`
    // (date/weight/title) and an optional `data-list-has-covers`
    // flag — the CSS selects on these to pick the right
    // `--moss-card-min` token.
    let axis_str = match sort_axis {
        // DateAsc presents identically to Date — same density tuning, same
        // compact-date meta slot; only the comparator's direction differs.
        moss_core::sort::SortAxis::Date | moss_core::sort::SortAxis::DateAsc => "date",
        moss_core::sort::SortAxis::Weight => "weight",
        moss_core::sort::SortAxis::Title => "title",
    };
    let cover_attr = if has_covers { " data-list-has-covers" } else { "" };
    let container_attr = if is_embed { " data-embed" } else { "" };
    // Same emit-nothing-when-empty rule as `components::cards_container`.
    let place = moss_core::render::placement::placement_attrs(placement);
    let align = place.align_suffix();

    format!(
        r#"<div class="moss-cards-container{}"{}{}{}><div class="moss-cards" data-layout="grid" data-list-axis="{}"{}>{}</div></div>"#,
        align, place.data_width_attr, place.size_style_attr, container_attr, axis_str, cover_attr, items
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
