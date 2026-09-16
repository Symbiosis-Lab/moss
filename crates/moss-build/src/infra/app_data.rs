//! The app-data directory, resolved without an `AppHandle`.

/// `dirs::data_dir()/host.moss.publisher` — the directory Tauri's
/// `app_data_dir()` resolves to on every desktop platform, spelled without an
/// `AppHandle` for the call sites that have none (startup before the app
/// exists, the plugin registry cache, the CLI). Keep the bundle identifier in
/// sync with `src-tauri/tauri.conf.json`.
pub fn app_data_dir_early() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("host.moss.publisher"))
}
