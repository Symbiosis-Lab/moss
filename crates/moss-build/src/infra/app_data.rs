//! The app-data directory, resolved without an `AppHandle`.

/// `dirs::data_dir()/host.moss.publisher` — the directory Tauri's
/// `app_data_dir()` resolves to on every desktop platform, spelled without an
/// `AppHandle` for the call sites that have none (startup before the app
/// exists, the plugin registry cache, the CLI). Keep the bundle identifier in
/// sync with the app's own Tauri config.
pub fn app_data_dir_early() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("host.moss.publisher"))
}

/// The app's settings file, which its Settings window writes.
pub fn app_config_path_early() -> Option<std::path::PathBuf> {
    app_data_dir_early().map(|d| d.join("app-config.json"))
}

/// Whether the user turned on Preview features (`experimental.preview_features`
/// in [`app_config_path_early`]). The CLI reads the switch the app writes, so
/// there is one switch for both. No file, or one that does not parse, is off.
pub fn preview_features_on() -> bool {
    // allow:raw_read app-support dir, outside any vault — never cloud-synced
    let Some(text) = app_config_path_early().and_then(|p| std::fs::read_to_string(p).ok()) else { return false };
    serde_json::from_str::<serde_json::Value>(&text)
        .is_ok_and(|config| config["experimental"]["preview_features"] == serde_json::Value::Bool(true))
}
