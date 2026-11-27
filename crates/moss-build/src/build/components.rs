//! UI Components for HTML generation
//!
//! This module provides reusable HTML generation components for the static site generator.
//! Components are designed to produce consistent HTML patterns across different contexts
//! (homepage, folder indexes, topic pages, sidebars).
//!
//! ## Components
//! - `date` - The calendar dates a partial date spans
//! - `child_list` - What a card reads from the build; the rows are drawn in `moss_core::render::cards`
//! - `nav` - Navigation components (header, sidebar)
//! - `color_extract` - Dominant color extraction from images
pub mod child_list;
pub mod folder_cover;
pub mod color_extract;
pub mod date;
pub mod event_meta;
pub mod field_places;
pub mod field_text;
pub mod nav;

pub mod sidebar_nav;
pub mod place_hierarchy;

pub(crate) use moss_core::render::cards::{cards_container, ListHolds};

// Re-export commonly used items
pub use child_list::ArticleListItemProps;

pub use moss_core::render::cards::year_group::render as render_year_grouped_list;
