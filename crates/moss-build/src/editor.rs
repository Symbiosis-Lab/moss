//! The editor backend — resolution, tree/file listing, content parse/persist,
//! and source-asset bytes.
//!
//! Everything here is tauri-free and runs behind BOTH carriers: the desktop's
//! `#[tauri::command]` wrappers (app-side `editor/`, which re-exports these
//! modules as shims) and the preview server's HTTP command carrier. Panel and
//! window choreography stay in the app; this is the host-fn split
//! shape applied to the editor.

pub mod completions;
pub mod content;
pub mod filesystem;
pub mod frontmatter;
// Reference-aware rename and delete: the pure text rewriting (`ref_rewrite`)
// and the walk that applies it (`ref_scan`). Crossed for `moss rename`
// (slice B4); the plain rename door itself is `vault::fs::rename_entry_inner`.
pub mod ref_rewrite;
pub mod ref_scan;
pub mod resolve;
pub mod source_asset;
pub mod validation;
