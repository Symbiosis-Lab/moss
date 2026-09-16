//! Typed body AST.
//!
//! Companion to [`crate::frontmatter`] (typed frontmatter) and
//! [`crate::resolve`] (the upstream wikilink/embed resolver). This module
//! owns the parsed body of a markdown document as a closed enum tree and
//! the visitor + render-hooks machinery that walks it.
//!
//! Pipeline order: the upstream `resolve` phase has already rewritten the
//! markdown source so wikilinks `[[foo]]` are now standard markdown links
//! `[foo](moss-resolved:foo.md)`. From here:
//!
//! ```text
//!   markdown source (post-resolve)
//!         │
//!         ▼
//!     parser::parse  →  Document (typed AST with Url::Unresolved)
//!         │
//!         ▼  visit::visit_urls_mut(&mut doc, &resolver)
//!     Document with every Url in the Resolved state
//!         │
//!         ▼  render::render_document(&doc, &hooks)
//!     final HTML
//! ```
//!
//! Design principles (from `docs/reference/typed-body-ast.md`):
//!
//! - The AST is data, not a hierarchy of objects. Pattern matching is the
//!   visitor framework.
//! - URL resolution is a typed state machine. The renderer accepts only
//!   `Url::Resolved`; emitting `Url::Unresolved` is a bug.
//! - moss-core stays pure Rust: zero I/O, zero async.

pub mod attrs;
pub mod cells;
pub mod dispatch_wikilink_embeds;
pub mod document;
pub mod editor_scan;
pub mod extract_hero;
pub mod footnotes;
pub mod grid_parts;
pub mod line_breaks;
pub mod linked_embed;
pub mod hooks;
pub mod math_text;
pub mod node;
pub mod parser;
pub mod plain_text;
pub mod query;
pub mod render;
pub mod resolve_urls;
pub mod shortcode;
pub mod shortcode_extract;
pub mod tags;
pub mod url;
pub mod visit;

pub use dispatch_wikilink_embeds::{dispatch_wikilink_embeds, WikilinkDispatchResult};
pub use document::{BlockMeta, Document};
pub use extract_hero::{extract_hero, HeroExtraction};
pub use grid_parts::{render_grid_parts, GridCellParts, GridParts};
pub use hooks::{DefaultHooks, RenderHooks};
pub use node::{Block, CalloutKind, Fold, Inline};
pub use parser::{parse, parse_with_config, parser_options, unwrap_implicit_figures, ParseConfig};
pub use plain_text::{inlines_to_plain_text, render_plain_text};
pub use query::find_first_block_image;
pub use render::{render_block_with_meta, render_blocks, render_document};
pub use resolve_urls::{classify_remaining_urls, resolve_urls, GraphAssetIndex, UrlResolution};
pub use shortcode::{
    ButtonItem, ButtonsShortcode, GalleryItem, GalleryShortcode, GridShortcode, HeroShortcode,
    RecentShortcode, Shortcode, ShortcodeKind, SubscribeShortcode,
};
pub use tags::extract_inline_tags;
pub use url::{ResolvedUrl, Url, UrlKind};
pub use visit::{has_callout_recursive, has_shortcode_recursive, visit_blocks, visit_urls_mut};
