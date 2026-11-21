//! The app-data directories, resolved without an `AppHandle`.
//!
//! Two roots per app identifier: `roaming` (`dirs::data_dir()`) for state that
//! belongs to the person, such as settings, which a Windows roaming profile may
//! carry to another computer; and `local` (`dirs::data_local_dir()`) for facts
//! about this computer, which must never follow. On macOS and Linux they are
//! the same directory.
//!
//! Which app the roots belong to is the identifier the running binary was built
//! with: the desktop app seeds its own once at startup (a local build of it has
//! an identifier of its own), and the command line, which seeds nothing, is the
//! shipped app's. Only entry points read [`AppRoots::running`]; the code under
//! them takes the root as an argument.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The shipped app's bundle identifier; the desktop's `tauri.conf.json` names
/// the same one.
pub const SHIPPED_IDENTIFIER: &str = "host.moss.publisher";

static APP_IDENTIFIER: OnceLock<String> = OnceLock::new();

/// Name the app whose data this process reads and writes. The desktop binary
/// calls it with its own bundle identifier before anything reads app data.
pub fn seed_app_identifier(identifier: &str) {
    let seeded = APP_IDENTIFIER.get_or_init(|| identifier.to_owned());
    debug_assert_eq!(seeded, identifier, "one process belongs to one app");
}

/// One app's two data roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRoots {
    /// The person's settings: may roam.
    pub roaming: PathBuf,
    /// Facts about this computer: never roams.
    pub local: PathBuf,
}

impl AppRoots {
    /// The roots Tauri's `app_data_dir()` and `app_local_data_dir()` give the
    /// app built as `identifier`.
    pub fn for_identifier(identifier: &str) -> Option<Self> {
        Some(Self { roaming: dirs::data_dir()?.join(identifier), local: dirs::data_local_dir()?.join(identifier) })
    }

    /// The running app's roots: the seeded identifier's, or the shipped app's.
    pub fn running() -> Option<Self> {
        Self::for_identifier(APP_IDENTIFIER.get().map_or(SHIPPED_IDENTIFIER, String::as_str))
    }

    /// The app's settings file, which its Settings window writes.
    pub fn config_path(&self) -> PathBuf {
        self.roaming.join("app-config.json")
    }

    /// Every root, for the check that none is inside a synced folder.
    fn all(&self) -> [&Path; 2] {
        [&self.roaming, &self.local]
    }
}

/// `dirs::data_dir()/host.moss.publisher`: the shipped app's roaming root,
/// which every build shares for the plugin registry cache, plugin downloads,
/// secrets and sign-ins. A local build of the app reads it and refreshes
/// nothing there.
pub fn app_data_dir_early() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("host.moss.publisher"))
}

/// `dirs::data_local_dir()/host.moss.publisher`: the non-roaming root for
/// facts about this computer, which must never follow the user to another one
/// (Windows roams `data_dir()`; elsewhere the two are the same directory).
pub fn app_data_local_dir_early() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|d| d.join("host.moss.publisher"))
}

/// The running app's settings file.
pub fn app_config_path_early() -> Option<std::path::PathBuf> {
    AppRoots::running().map(|roots| roots.config_path())
}

/// Whether the user turned on Preview features (`experimental.preview_features`
/// in the settings file `config`). The CLI reads the switch the app writes, so
/// there is one switch for both. No file, or one that does not parse, is off.
pub fn preview_features_in(config: &Path) -> bool {
    // allow:raw_read app-support dir, outside any vault — never cloud-synced
    let Ok(text) = std::fs::read_to_string(config) else { return false };
    serde_json::from_str::<serde_json::Value>(&text)
        .is_ok_and(|config| config["experimental"]["preview_features"] == serde_json::Value::Bool(true))
}

/// Refuses a moss home or app-data root inside a synced folder: what is kept
/// there is about this computer, and a sync would carry it to another.
pub fn refuse_synced_roots(roots: &AppRoots, moss_home: Option<&Path>) -> Result<(), String> {
    let machine = app_data_local_dir_early();
    let paths = roots.all().into_iter().chain(machine.as_deref()).chain(moss_home);
    for path in paths {
        if crate::build::cloud_provider::detect_from_path(path).is_some() {
            return Err(format!(
                "moss keeps this computer's own state in {}, which is inside a synced folder. Move it out of the synced folder.",
                path.display()
            ));
        }
    }
    Ok(())
}

/// [`refuse_synced_roots`] for the running app and the moss home: what a
/// binary checks at start.
pub fn refuse_synced_running_roots() -> Result<(), String> {
    let Some(roots) = AppRoots::running() else { return Ok(()) };
    refuse_synced_roots(&roots, crate::infra::home::moss_home().ok().as_deref())
}

/// The root the credential stores (plugin secrets, sign-ins) live under: the
/// shipped app's roaming root. Any test binary, this crate's or a dependent's,
/// gets a scratch root per process instead, so no test can read, write or
/// delete a person's credentials.
pub fn credential_root() -> Option<PathBuf> {
    if cfg!(test) || in_test_binary() {
        return Some(std::env::temp_dir().join(format!("moss-test-app-data-{}", std::process::id())));
    }
    app_data_dir_early()
}

/// Whether this process is a test binary, which must never reach a real app
/// data root.
pub(crate) fn in_test_binary() -> bool {
    std::env::current_exe().is_ok_and(|exe| is_test_binary_path(&exe))
}

/// Cargo runs every test binary, unit and integration, from
/// `<profile>/deps/`, or under its newer build-dir layout from
/// `<profile>/build/<package>/<hash>/out/`. A binary a person runs is
/// copied up to `<profile>/` or installed elsewhere, so it is neither.
fn is_test_binary_path(exe: &Path) -> bool {
    let mut dirs = exe.ancestors().skip(1).map(|dir| dir.file_name().unwrap_or_default());
    match dirs.next() {
        Some(parent) if parent == "deps" => true,
        Some(parent) if parent == "out" => dirs.nth(2).is_some_and(|dir| dir == "build"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_binary_is_known_in_both_cargo_layouts_and_a_shipped_one_is_not() {
        assert!(is_test_binary_path(Path::new("/w/target/debug/deps/moss_build-0123abcd")));
        assert!(is_test_binary_path(Path::new("/w/target/debug/build/moss-build/0123abcd/out/moss_build-0123abcd")));
        assert!(!is_test_binary_path(Path::new("/w/target/debug/moss")));
        assert!(!is_test_binary_path(Path::new("/Applications/moss.app/Contents/MacOS/moss")));
        assert!(!is_test_binary_path(Path::new("/w/out/moss")));
    }

    #[test]
    fn preview_features_follow_the_settings_file_given() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("app-config.json");
        assert!(!preview_features_in(&config), "no file is off");
        std::fs::write(&config, r#"{"experimental":{"preview_features":true}}"#).unwrap();
        assert!(preview_features_in(&config));
    }

    #[test]
    fn a_root_inside_a_synced_folder_is_refused() {
        let home = dirs::home_dir().unwrap();
        let local = AppRoots { roaming: PathBuf::from("/tmp/a"), local: PathBuf::from("/tmp/a") };
        assert!(refuse_synced_roots(&local, Some(Path::new("/tmp/moss-home"))).is_ok());
        let synced = home.join("Library/CloudStorage/GoogleDrive-someone@example.com/My Drive/moss");
        assert!(refuse_synced_roots(&local, Some(&synced)).is_err(), "a synced moss home is refused");
        let roots = AppRoots { roaming: synced.clone(), local: PathBuf::from("/tmp/a") };
        assert!(refuse_synced_roots(&roots, None).is_err(), "a synced app-data root is refused");
    }

    #[test]
    fn an_identifier_names_both_roots() {
        let roots = AppRoots::for_identifier("host.moss.publisher.wt-x").unwrap();
        assert!(roots.roaming.ends_with("host.moss.publisher.wt-x"));
        assert!(roots.local.ends_with("host.moss.publisher.wt-x"));
        assert!(roots.config_path().ends_with("host.moss.publisher.wt-x/app-config.json"));
    }
}
