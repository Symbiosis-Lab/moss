//! What the author chose to do with a folder of files a plugin can turn into
//! pages: show them as pages, or keep them as plain files.
//!
//! The choice lives in `.moss/config.toml` under `[derive]`, keyed by the
//! folder's path from the site root with a trailing slash:
//!
//! ```toml
//! [derive]
//! "reports/" = ["pdf-pages"]   # show these files as pages, by derivation id
//! "scans/" = false             # keep them as files
//! ```
//!
//! No key means the author has not been asked yet. Writes go through the one
//! managed-TOML primitive, so comments and key order survive and an unchanged
//! save writes nothing.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use super::config::{load_managed_toml, subtable, write_managed_toml};
use crate::moss_paths::MossPaths;

/// The author's answer for one folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Not asked, or not answered yet.
    Undecided,
    /// Show the folder's files as pages, using these derivations (by id).
    Show(Vec<String>),
    /// Leave the files as files.
    Keep,
}

const TABLE: &str = "derive";

/// The `[derive]` key for a folder: its path from the site root, forward
/// slashes, one trailing slash (`/` for the site root itself). `folder` may be
/// absolute (inside the site) or already relative. `None` when it lies outside
/// the site.
pub fn folder_key(site: &MossPaths, folder: &Path) -> Option<String> {
    let absolute;
    let folder = if folder.is_absolute() {
        folder
    } else {
        absolute = site.project_root().join(folder);
        &absolute
    };
    // The watcher reports resolved paths; the site may be spelled through a symlink.
    let key = crate::build::watch::path_to_relative_key(site.project_root(), folder)?;
    Some(if key.is_empty() { "/".to_string() } else { format!("{key}/") })
}

fn decision_of(value: &Value) -> Decision {
    match value {
        Value::Boolean(false) => Decision::Keep,
        Value::Array(ids) => {
            let ids: Vec<String> = ids.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            if ids.is_empty() { Decision::Undecided } else { Decision::Show(ids) }
        }
        // `true`, a number, a table: nothing moss wrote, so nothing to act on.
        _ => Decision::Undecided,
    }
}

/// Every decision in a parsed config, keyed as written.
pub fn decisions_in(root: &Table) -> BTreeMap<String, Decision> {
    root.get(TABLE)
        .and_then(Value::as_table)
        .map(|t| t.iter().map(|(k, v)| (k.clone(), decision_of(v))).collect())
        .unwrap_or_default()
}

/// Every decision recorded for the site. A config that cannot be read answers
/// with none rather than failing a caller that only wants to show a hint.
pub fn decisions(site: &MossPaths) -> BTreeMap<String, Decision> {
    load_managed_toml(&site.config()).map(|m| decisions_in(&m.root)).unwrap_or_default()
}

/// The decision that governs a folder key (as [`folder_key`] spells it): the
/// folder's own entry, else the nearest ancestor's, up to the site root (`/`).
/// A value that is not a decision (`true`, a table) does not shadow an ancestor.
pub fn nearest_decision<'a>(decided: &'a BTreeMap<String, Decision>, key: &str) -> Option<&'a Decision> {
    let mut path = key.trim_end_matches('/');
    loop {
        if let Some(d) = decided.get(&format!("{path}/")).filter(|d| **d != Decision::Undecided) {
            return Some(d);
        }
        if path.is_empty() {
            return None;
        }
        path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    }
}

/// The author's choice for one folder, inherited from its nearest entry.
pub fn folder_decision(site: &MossPaths, folder: &Path) -> Decision {
    folder_key(site, folder)
        .and_then(|key| nearest_decision(&decisions(site), &key).cloned())
        .unwrap_or(Decision::Undecided)
}

/// Record the author's choice. `Undecided` removes the key.
pub fn set_folder_decision(site: &MossPaths, folder: &Path, decision: Decision) -> Result<(), String> {
    let key = folder_key(site, folder)
        .ok_or_else(|| format!("{} is outside the site", folder.display()))?;
    let path = site.config();
    let mut managed = load_managed_toml(&path)?;
    match decision {
        Decision::Undecided => {
            if let Some(t) = managed.root.get_mut(TABLE).and_then(Value::as_table_mut) {
                t.remove(&key);
                if t.is_empty() {
                    managed.root.remove(TABLE);
                }
            }
        }
        Decision::Keep => {
            subtable(&mut managed.root, TABLE)?.insert(key, Value::Boolean(false));
        }
        Decision::Show(ids) => {
            let ids = ids.into_iter().map(Value::String).collect();
            subtable(&mut managed.root, TABLE)?.insert(key, Value::Array(ids));
        }
    }
    write_managed_toml(&path, &managed.original, &managed.root, crate::config::migrations::CURRENT_VERSION)
}

/// Re-key `[derive]` entries after folders were renamed. `renames` are
/// `(from, to)` site-relative paths without a trailing slash. A key equal to a renamed folder, or below one, follows it; the
/// rest stay. Returns whether anything changed.
pub fn heal_renamed_keys(root: &mut Table, renames: &[(String, String)]) -> bool {
    let Some(table) = root.get_mut(TABLE).and_then(Value::as_table_mut) else { return false };
    let mut changed = false;
    for (from, to) in renames {
        let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
        if from.is_empty() || from == to {
            continue;
        }
        let moved: Vec<(String, String)> = table
            .keys()
            .filter_map(|key| {
                let below = key.strip_prefix(from)?.strip_prefix('/')?;
                Some((key.clone(), format!("{to}/{below}")))
            })
            .collect();
        for (old, new) in moved {
            if let Some(value) = table.remove(&old) {
                table.insert(new, value);
                changed = true;
            }
        }
    }
    changed
}

/// [`heal_renamed_keys`] against the site's config file. Idempotent: a key
/// already moved is not found again, so the rename engine and the watcher (for
/// renames made outside moss) can both call it.
pub fn heal_renamed_folders(site: &MossPaths, renames: &[(String, String)]) -> Result<(), String> {
    if renames.is_empty() {
        return Ok(());
    }
    let path = site.config();
    let mut managed = load_managed_toml(&path)?;
    if heal_renamed_keys(&mut managed.root, renames) {
        write_managed_toml(&path, &managed.original, &managed.root, crate::config::migrations::CURRENT_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "derive_folders_tests.rs"]
mod derive_folders_tests;
