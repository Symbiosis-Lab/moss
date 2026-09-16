//! Page rendering. Split into submodules per docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 1.

pub mod preflight;
pub mod config;
pub mod blocking;
pub mod credits;
pub mod empty_home;
pub mod html;
pub mod image_util;
pub mod incremental;
pub mod lang_roots;
pub mod uid_dedup;

// Enumerated re-exports for symbols with external callers.

// From blocking — main generation entry point + site config
pub use blocking::generate_blocking_content;
pub use blocking::SiteConfig;
pub use crate::build::incremental_gates::IncrementalGates;

// From config — the page/site comments-preference ladder, also needed by
// features::generate_native_slots to decide whether to render a comments
// section at all (not just the `data-comments` JS hint this module emits).
pub(crate) use config::resolve_comments_pref;

// From html — called directly by blocking and may be called by plugins
pub use html::generate_html;

// From preflight — called by build.rs to surface compile-time warnings
pub(crate) use preflight::check_misplaced_theme_files;
pub(crate) use preflight::check_mixed_multilingual_structure;
pub use preflight::is_ignored_root_theme_file;

// resolve_path_with_overrides is defined in page_map; render.rs re-exports it for direct callers.
pub(crate) use crate::build::scan::page_map::resolve_path_with_overrides;
pub(crate) mod grid_cells;
