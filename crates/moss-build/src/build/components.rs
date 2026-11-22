//! UI Components for HTML generation
//!
//! This module provides reusable HTML generation components for the static site generator.
//! Components are designed to produce consistent HTML patterns across different contexts
//! (homepage, folder indexes, topic pages, sidebars).
//!
//! ## Components
//! - `date` - Date formatting utilities (full and compact formats)
//! - `child_list` - Single child list item with date and link
//! - `year_group` - Articles grouped by year with headings
//! - `nav` - Navigation components (header, sidebar)
//! - `grid_card` - Grid card for `:::grid` shortcode
//! - `color_extract` - Dominant color extraction from images
pub mod child_summary;
pub mod child_list;
pub mod grid_card;
pub mod folder_cover;
pub mod folder_title;
pub mod color_extract;
pub mod date;
pub mod event_meta;
pub mod field_places;
pub mod field_text;
pub mod nav;

pub mod sidebar_nav;
pub mod year_group;
pub mod place_hierarchy;

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
pub(crate) fn cards_container(
    layout: &str,
    lead: bool,
    is_embed: bool,
    placement: &moss_core::media::Placement,
    axis: moss_core::sort::SortAxis,
    holds: ListHolds,
    inner: &str,
) -> String {
    let place = moss_core::render::placement::placement_attrs(placement);
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
pub(crate) enum ListHolds {
    Entries,
    Collections,
    Mixed,
}

impl ListHolds {
    /// From each card in turn: whether it stands for a collection.
    pub(crate) fn of(collections: impl IntoIterator<Item = bool>) -> Self {
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
pub(crate) fn list_attrs(axis: moss_core::sort::SortAxis, holds: ListHolds) -> String {
    use moss_core::sort::SortAxis;
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

// Re-export commonly used items
pub use child_list::ArticleListItemProps;

pub use date::{time_html, extract_date_from_doc, format_article_date, format_compact_date, format_date_string, format_display_date, format_vertical_cjk_date, format_vertical_cjk_date_compact};
pub use year_group::render as render_year_grouped_list;
