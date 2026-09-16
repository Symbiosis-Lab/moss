//! Everything moss knows about a heading, in one place.
//!
//! Headings used to be spread over four homes — `heading.rs` (the article
//! title rule), `heading_anchor.rs` (the Obsidian slug algorithm),
//! `extract_headings.rs` (the autocomplete walker), and a private
//! `collect_heading_text` inside `ast/parser.rs`. They are not four
//! subjects; they are four steps of one pipeline:
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
//! fragment the page does not have. Spread across four files that identity
//! was something tests had to keep re-checking; a math bug cluster hit
//! three of the four homes independently in July 2026 precisely because
//! nothing structural tied them together. Co-locating them makes the
//! shared step ([`text`]) a single function instead of a coincidence.
//!
//! Consolidation mandated by
//! `docs/reference/target/05-consolidation-map.md`
//! (row 36 and the F10 tiny-file merge list).

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
