//! HTML synthesizers for moss-emitted markdown content.
//!
//! Each submodule owns one media kind's `<X>` emission given typed
//! inputs (`TitleParams`, `&AssetSnapshot`, source URL). Pure functions:
//! no I/O, no async, no Tauri primitives. Consumed by src-tauri's
//! markdown pipeline (Stage 2 event dispatcher) and by moss-core's own
//! shortcode renderers (Phase 2E v5 PR3).
pub mod audio;
pub mod iframe;
pub mod image;
pub mod model;
pub mod pdf;
pub mod url_embed;
pub mod video;
