//! The plugin *vocabulary* half of the app's plugin family — the members the
//! build tree reaches: hook/context types, contribution declarations, the
//! builder/plugin fingerprints, and the `.moss/config.toml` plugin-config
//! readers in `discovery`; the install half: the bundled-plugin installer,
//! the registry catalog and the download/verify client; and the runtime: the
//! manager, its engine adapter and the signal route. The Tauri
//! commands and the desktop host stay app-side.
//!
//! The desktop app's `plugins.rs` re-exports most of these at their old paths, so
//! app-side `crate::plugins::…` spellings are unchanged. `admission` is the
//! exception: only the app installs its verdict, and only the loader asks it.
pub mod adapter_host;
pub mod admission;
pub mod bundled;
pub mod contributions;
pub mod discovery;
pub mod fingerprint;
pub mod hook_state;
pub mod install;
pub mod manager;
// Headless plugin-file I/O + the portable runtime halves the QuickJS engine
// dispatches into (open-CLI slice 2).
pub mod project_files;
pub mod registry;
pub mod runtime;
pub mod setup;
pub mod types;

pub use setup::*;
pub use types::*;

/// Where a plugin's files live in a project: `.moss/plugins/<id>`. The one
/// place that spells it — ten copies of this join were once scattered across
/// the manager, the cookie store, the config writers and the installers.
pub fn plugin_dir(project_path: &str, plugin_id: &str) -> std::path::PathBuf {
    crate::moss_paths::MossPaths::new(std::path::Path::new(project_path)).plugins_dir().join(plugin_id)
}
