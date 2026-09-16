//! The infra members the build tree and the plugin runtime reach: the
//! app-advisory display vocabulary, the atomic file writers, the surgical
//! `config.toml` editor the plugin uninstaller uses, the app-data directory
//! resolved without an `AppHandle`, plus a path alias so the moved tree's
//! `crate::infra::moss_paths::…` spellings keep resolving to the layout owner
//! at [`crate::moss_paths`]. The rest of the app's `infra` family (state, the
//! config migrations runner, app_config) stays app-side.
pub mod app_advisory;
pub mod app_data;
pub mod atomic_write;
pub mod liveness;
pub mod toml_rewrite;

pub use crate::moss_paths;
