//! `moss plugin install <id> [<folder>]`: put a plugin into a site.
//!
//! The command records no trust. A folder's plugins run when the person trusts
//! the folder in the app, whose answer lives in per-user app data and never in
//! the site folder, or when a build is run with `--allow-plugins`. The line it
//! prints says when the folder is not trusted.
//!
//! The catalog is fetched on every install, as a package manager does, so a
//! version withdrawn since the last look is never installed. A catalog that
//! cannot be reached falls back to the cached one and says how old it is.

use std::path::Path;

use super::registry_client::{enforce, fetch};
use crate::vault_root::VaultRoot;

pub const USAGE: &str = "Usage: moss plugin install <id> [<folder>]";

/// What a successful install tells the user.
pub struct Installed {
    /// Set when the catalog could not be refreshed and the cached copy was used.
    pub notice: Option<String>,
    pub line: String,
}

pub fn run(args: &[String]) -> i32 {
    let mut words = args.iter().skip(1);
    let (Some(id), folder, None) = (words.next(), words.next(), words.next()) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if id.starts_with('-') || folder.is_some_and(|f| f.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let folder = match folder {
        Some(f) => std::path::PathBuf::from(f),
        None => match std::env::current_dir() {
            Ok(dir) => match VaultRoot::find_containing(&dir) {
                Some(site) => site.path().to_path_buf(),
                None => {
                    eprintln!("moss plugin install: {} is not inside a moss site (no .moss folder above it)", dir.display());
                    return 1;
                }
            },
            Err(e) => {
                eprintln!("moss plugin install: could not determine the current directory: {e}");
                return 1;
            }
        },
    };
    match install(id, &folder, fetch::REGISTRY_ORIGIN, fetch::VERIFY_KEYS) {
        Ok(done) => {
            if let Some(notice) = done.notice {
                eprintln!("{notice}");
            }
            println!("{}", done.line);
            0
        }
        Err(message) => {
            eprintln!("moss plugin install: {message}");
            1
        }
    }
}

/// [`run`] without the process: `origin` and `keys` are parameters so a test can
/// point at a local registry. `folder` must be the site root. The app-data directory is the app's own.
pub fn install(id: &str, folder: &Path, origin: &str, keys: &[&str]) -> Result<Installed, String> {
    let folder = crate::vault_root::resolve_input(folder);
    if !crate::nested_roots::owns_moss(&folder) {
        return Err(format!("{} is not a moss site root (no site .moss folder here)", folder.display()));
    }
    let site = folder.to_string_lossy().into_owned();
    let app_data = crate::infra::app_data::app_data_dir_early().ok_or("no app-data directory to keep the plugin catalog in")?;
    let roots = crate::infra::app_data::AppRoots::running();
    let settings = roots.as_ref().map(|roots| roots.config_path());
    let preview_on = settings.as_deref().is_some_and(crate::infra::app_data::preview_features_in);

    // Reads the folder answers and records none: the command installs code,
    // and only the person, in the app, trusts a folder to run it.
    let trust = roots.as_ref().map(super::registry_client::trust::root);
    enforce::install_headless(Some(&app_data), trust.as_deref(), false);
    let refreshed = fetch::refresh_from(&app_data, origin, keys);
    let unreached = match &refreshed.index {
        fetch::RefreshOutcome::Updated => None,
        fetch::RefreshOutcome::Offline(why) | fetch::RefreshOutcome::NotCached(why) => Some(why.clone()),
        fetch::RefreshOutcome::Refused(why) => Some(why.to_string()),
    };
    let cached_at = refreshed.cache.anchor.index_fetched_at.clone();
    let have_catalog = refreshed.cache.index.is_some();
    enforce::publish(refreshed.cache);

    let notice = unreached.as_ref().filter(|_| have_catalog).map(|why| {
        format!("could not refresh the plugin catalog ({why}); using the copy cached {}", age(cached_at.as_deref()))
    });
    crate::plugins::registry::install_plugin_allowing(id, &site, preview_on, settings.as_deref()).map_err(|e| match &unreached {
        Some(why) if !have_catalog => format!("could not fetch the plugin catalog ({why}) and none is cached: {e}"),
        _ => e,
    })?;

    let dir = crate::plugins::plugin_dir(&site, id);
    let version = crate::plugins::bundled::read_installed_manifest(&dir).map(|m| m.version).unwrap_or_default();
    let line = if enforce::is_trusted(&folder) {
        format!("Installed {id} {version} into {site}")
    } else {
        format!("Installed {id} {version} into {site}. This folder is not trusted, so its plugins stay off until you trust it in the moss app or build with --allow-plugins")
    };
    Ok(Installed { notice, line })
}

fn age(fetched_at: Option<&str>) -> String {
    let Some(then) = fetched_at.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) else {
        return "at an unknown time".to_string();
    };
    let hours = (chrono::Utc::now() - then.to_utc()).num_hours().max(0);
    match hours {
        0 => "within the last hour".to_string(),
        1..=47 => format!("{hours} hours ago"),
        _ => format!("{} days ago", hours / 24),
    }
}
