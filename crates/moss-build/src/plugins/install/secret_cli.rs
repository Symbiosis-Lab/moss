//! `moss plugin secret set <plugin> <key> [<folder>]`: store a credential a
//! plugin's manifest asks the user for, read from standard input, in moss's
//! secret store outside every site folder. The headless counterpart of the
//! app's credential prompt.

use std::io::Read;
use std::path::PathBuf;

use crate::identity::keystore::Scope;
use crate::identity::secrets::SecretStore;

pub const USAGE: &str = "Usage: moss plugin secret set <plugin> <key> [<folder>]   (the value is read from standard input; empty input removes it)";

pub fn run(args: &[String]) -> i32 {
    let mut words = args.iter().skip(1);
    let (Some(verb), Some(plugin), Some(key), folder, None) = (words.next(), words.next(), words.next(), words.next(), words.next()) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if verb != "set" || [plugin, key].iter().any(|w| w.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let folder = folder.map(PathBuf::from).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
    let mut value = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut value) {
        eprintln!("moss plugin secret: could not read the value from standard input: {e}");
        return 1;
    }
    match set(plugin, key, value.trim_end_matches(['\r', '\n']), &folder) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(message) => {
            eprintln!("moss plugin secret: {message}");
            1
        }
    }
}

/// Store `value` for `plugin` under `key`, which its manifest must declare as
/// a credential the user supplies: a typo would otherwise store a secret the
/// plugin never reads, and say nothing.
pub fn set(plugin: &str, key: &str, value: &str, folder: &std::path::Path) -> Result<String, String> {
    let root = crate::vault_root::VaultRoot::resolve(folder);
    let installed = crate::plugins::discovery::scan_installed_plugins(root.as_str())?;
    let Some(found) = installed.loaded.iter().find(|p| p.manifest.name == plugin) else {
        let why = installed.refused.iter().find(|(path, _)| path.ends_with(plugin)).map(|(_, why)| format!(": it does not load ({why})"));
        return Err(format!("no plugin named {plugin} is installed in {}{}", root.as_str(), why.unwrap_or_default()));
    };
    let keys = found.manifest.user_supplied_secret_keys();
    if !keys.iter().any(|k| k == key) {
        return Err(match keys.is_empty() {
            true => format!("{plugin} asks for no credentials"),
            false => format!("{plugin} asks for {}, not {key}", keys.join(", ")),
        });
    }
    SecretStore::require("moss plugin secret")?
        .set(&Scope::Plugin(plugin.to_string()), key, value)
        .map_err(|e| format!("{e:?}"))?;
    Ok(if value.is_empty() { format!("removed {plugin}'s {key}") } else { format!("stored {plugin}'s {key}, outside the site folder") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_key_the_manifest_asks_for_is_stored() {
        let site = tempfile::tempdir().unwrap();
        let dir = site.path().join(".moss/plugins/bluesky");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::json!({"name": "bluesky", "version": "0.0.1", "display_name": "Bluesky", "description": "", "author": "",
                "entry": "main.bundle.js", "global_name": "B",
                "contributes": {"channel": {"setup": {"settings": [{"key": "app_password", "type": "secret"}]}}}})
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("main.bundle.js"), "window.B = {};").unwrap();

        let err = set("bluesky", "app_pasword", "x", site.path()).unwrap_err();
        assert!(err.contains("asks for app_password"), "{err}");
        assert!(set("mastodon", "token", "x", site.path()).unwrap_err().contains("no plugin named mastodon"));
    }
}
