//! Importing external content into the vault.
//!
//! One conventions engine instead of per-builder adapters (design:
//! `docs/archive/2026-07-27-import-conventions-engine-design.md`). The
//! scrape pipeline enters through [`engine::extract_with_evidence`] via
//! [`scrape::converter::extract_article_with_snapshot`].
//!
//! Layout: `state_json` parses bootstrap-state JSON globals; `infer` walks a
//! builder's component tree into `blocks` (the normalized content IR) driven
//! by a row from `dialects`; `geometry` recovers reading order on canvas
//! layouts; `media` holds media-URL conventions; `emit` renders blocks to
//! markdown. `strikingly` and `readymag` hold those builders' detection +
//! page selection around the shared machinery.

pub mod blocks;
pub mod dialects;
pub mod emit;
pub mod engine;
pub mod geometry;
pub mod infer;
pub mod media;
pub mod readymag;
pub mod scrape;
pub mod state_json;
pub mod strikingly;
