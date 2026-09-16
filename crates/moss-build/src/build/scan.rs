//! Site scanning, content structure analysis, and URL mapping.
//!
//! This bucket handles the first phase of a moss build: recursively scanning
//! the source directory, categorising files, building the page map, and
//! assigning URL slugs.

pub mod article_map;
pub(crate) mod cascade;
// `classify` stays crate-private; the one symbol an integration test needs
// (`is_excluded_dir_name`) is re-exported at the crate root in
// `src-tauri/src/lib.rs` so external callers don't see the module path.
// Per architecture-review feedback on T3
// (`docs/archive/2026-05-28-onboarding-dispatch-plan.md`).
pub mod classify;
pub mod page_map;
pub mod scan;
pub mod slug;
pub(crate) mod sort_inference;
