//! The headless halves of the plugin runtime (open-CLI slice 2, #1019).
//!
//! Hand-written parent so `download.rs` and `portable.rs` crossed verbatim from
//! `src-tauri/src/plugins/runtime/` — their `super::download` / `super::portable`
//! spellings resolve here exactly as they did under the app's `runtime.rs`. The
//! Tauri command wrappers, the streaming binary path, and the action-panel
//! members stay app-side in `src-tauri/src/plugins/runtime.rs`.

pub mod download;
pub mod portable;
