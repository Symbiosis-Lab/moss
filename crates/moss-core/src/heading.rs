//! Everything moss knows about a heading, in one place — not four separate
//! subjects, but four steps of one pipeline:
//!
//! ```text
//!   inline nodes / parser events ──[text]──> plain text
//!                                              │
//!                                              ├─[anchor]──> the <hN id="…">
//!                                              └─[extract]─> autocomplete rows
//!   file path + frontmatter ───────[state]───> the auto-injected <h1>
//! ```
//!
//! The keystone invariant of the whole cluster is **byte-identity**: the
//! slug `extract` reports, the `id` the renderer emits, and the raw-line
//! slug the wikilink scanner computes in `build/scan/scan.rs` must agree
//! character for character, or a `[[Page#Heading]]` link resolves to a
//! fragment the page does not have. Co-locating the four steps in one module
//! is what keeps that identity from drifting when one of them is edited in
//! isolation.

pub mod anchor;
pub mod extract;
pub mod state;
pub mod text;

pub use anchor::obsidian_heading_anchor;
pub use extract::{extract_headings, extract_headings_with_config, HeadingInfo};
pub use state::{
    hero_at_top_owns_title, compute, filename_text, filename_text_with_root, HeadingInputs,
    HeadingSource, HeadingState,
};
