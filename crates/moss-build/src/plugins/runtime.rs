//! The headless halves of the plugin runtime (open-CLI slice 2).
//!
//! Hand-written parent so `download.rs` and `portable.rs` crossed verbatim from
//! the desktop app's `plugins/runtime/` — their `super::download` / `super::portable`
//! spellings resolve here exactly as they did under the app's `runtime.rs`. The
//! Tauri command wrappers, the streaming binary path, and the action-panel
//! members stay app-side.

pub mod download;
pub mod portable;
