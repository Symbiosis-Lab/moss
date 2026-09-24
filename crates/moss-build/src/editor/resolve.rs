//! Editor-side resolution: turning what the author typed into what it points at.
//!
//! Every module here answers one question about a reference — is this an asset,
//! a folder, a link, a deployed URL — using the SAME `moss_core::resolve`
//! kernel the build uses, with editor-flavoured (filesystem / article-map)
//! indexes injected. All filesystem and config I/O lives on this side of the
//! boundary; `moss_core` stays pure.
//!
//! Ended up here as `editor/resolve/{asset_resolver,reference_resolver,
//! url_index,links,folder_index}` (image_size, a pure Tauri wrapper, stayed
//! app-side). Caches will move to `editor/session.rs` in M5.

pub mod asset_resolver;
pub mod folder_index;
pub mod links;
pub mod page_source;
pub mod reference_resolver;
pub mod takeover;
pub mod url_index;
