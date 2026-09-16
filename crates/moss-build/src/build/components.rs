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
pub mod nav;

pub mod series_nav;
pub mod sidebar_nav;
pub mod year_group;

/// The container every children listing wears: a `.moss-cards-container`
/// wrapping one `.moss-cards` of the given layout.
///
/// One source of truth, because the hand-picked `:::grid {.summary}` variant
/// emits the SAME container a folder listing does — that identity is what lets
/// the two share `site.css`'s `.moss-cards[data-layout="list"]` rules and the
/// render gates already written against them.
///
/// `is_embed` stamps `data-embed`, which marks a body-embedded listing (a
/// `![[folder/]]` embed or a grid fence) apart from the trailing automatic one
/// so CSS can give it its own block rhythm.
pub(crate) fn cards_container(layout: &str, is_embed: bool, inner: &str) -> String {
    format!(
        "<div class=\"moss-cards-container\"{}><div class=\"moss-cards\" data-layout=\"{}\">{}</div></div>",
        if is_embed { " data-embed" } else { "" },
        layout,
        inner,
    )
}

// Re-export commonly used items
pub use child_list::ArticleListItemProps;

pub use date::{extract_date_from_doc, format_article_date, format_compact_date, format_date_string, format_display_date, format_vertical_cjk_date, format_vertical_cjk_date_compact};
pub use year_group::render as render_year_grouped_list;
