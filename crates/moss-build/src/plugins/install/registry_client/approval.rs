//! What the user has allowed to run, and where moss keeps it.
//!
//! A plugin directory is part of the project, so anything written there
//! travels with a shared folder — the install receipt beside the code
//! ([`super::receipt`]) included. The record of the user's consent therefore
//! lives in app data, and it names the exact code it was given for: a plugin
//! whose executable bytes change is asked about again, whether a registry
//! update changed them, a hand edit did, or the plugin rewrote its own entry.
//!
//! Pure over a path. The process-wide copy that `load_plugin` consults lives
//! in [`super::enforce`], beside the registry snapshot, for the same reason
//! that one does.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "plugin-approvals.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    /// The plugin directory, canonical — project and id in one string, and
    /// the same one `load_plugin` is handed.
    pub dir: PathBuf,
    /// `bundled::code_hash` of that directory when the user allowed it.
    pub code_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default)]
    pub approvals: Vec<Approval>,
}

impl Approvals {
    pub fn allows(&self, dir: &Path, code_sha256: &str) -> bool {
        self.approvals.iter().any(|a| a.dir == dir && a.code_sha256 == code_sha256)
    }

    /// Record one, replacing any older record for the same directory: a
    /// directory whose code changed is exactly the case the old record must
    /// stop answering for.
    pub fn grant(&mut self, dir: PathBuf, code_sha256: String) {
        self.approvals.retain(|a| a.dir != dir);
        self.approvals.push(Approval { dir, code_sha256 });
    }
}

/// The store under `app_data_dir`, or empty. An unreadable file is empty
/// too: the cost is a question the user has answered before, not a plugin
/// running without one.
pub fn load(app_data_dir: &Path) -> Approvals {
    fs::read_to_string(app_data_dir.join(FILE))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn save(app_data_dir: &Path, approvals: &Approvals) -> Result<(), String> {
    fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("create {}: {e}", app_data_dir.display()))?;
    crate::infra::atomic_write::write_json_atomic(&app_data_dir.join(FILE), approvals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_is_for_one_directory_at_one_hash_and_survives_a_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = PathBuf::from("/v/.moss/plugins/x");

        let mut store = load(tmp.path());
        assert!(!store.allows(&dir, "aaa"), "nothing is allowed until granted");

        store.grant(dir.clone(), "aaa".into());
        save(tmp.path(), &store).unwrap();
        let store = load(tmp.path());
        assert!(store.allows(&dir, "aaa"));
        assert!(!store.allows(&dir, "bbb"), "consent names the bytes, not the directory");
        assert!(!store.allows(Path::new("/v/.moss/plugins/y"), "aaa"));

        // Re-granting after the code changed replaces the record rather than
        // keeping both: the old bytes are gone and must not stay allowed.
        let mut store = store;
        store.grant(dir.clone(), "bbb".into());
        assert!(!store.allows(&dir, "aaa"));
        assert_eq!(store.approvals.len(), 1);

        fs::write(tmp.path().join(FILE), "{not json").unwrap();
        assert_eq!(load(tmp.path()), Approvals::default(), "unreadable is empty, not an error");
    }
}
