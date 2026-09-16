//! Plugin credentials: opaque strings moss holds on a plugin's behalf.
//!
//! Same arrangement as the [keystore](super::keystore), one shape down. A
//! keystore entry is a signing key moss uses for you; a secret is a token
//! somebody else issued you — an API key, a bearer token — that you do have to
//! read, because you put it in an HTTP header. So a plugin CAN read its own
//! secrets, and that is the only difference. Everything else is the keystore's
//! model, reused deliberately: the [`Scope`] comes from the dispatch seam and
//! never from a parameter, so one plugin cannot name another's token; moss is
//! the only writer; the bytes never enter the user's repo.
//!
//! **Not the OS keychain, on purpose.** moss linked `keyring` once and removed
//! it (`fc091d7fe`, identity v3): three platform backends to keep alive, and a
//! keychain unlock prompt in the middle of a publish (`5d3d84c57`). The
//! identity key — the one whose loss is unrecoverable — is a 0600 file today.
//! A revocable API token does not earn a stricter home than that.
//!
//! **App-global, not under `.moss/`.** A Pinata token belongs to the account,
//! not to one site, so the same token would otherwise be pasted per folder.
//! And `.moss/keys` is deliberately synced — for a signing key that is the
//! feature (it follows the user to a second machine), but a vault can sit in a
//! shared Google Drive folder, and a shared folder is the wrong place for a
//! credential. This store lives beside `app-config.json` in the app-support
//! directory, which no vault syncs.

use super::keystore::{write_private, KeystoreError, Scope};
use std::path::{Path, PathBuf};

/// The store's directory name under the app-support directory.
pub const SECRETS_DIRNAME: &str = "secrets";

/// The secret store rooted at moss's app-support directory.
pub struct SecretStore {
    /// `<app-data>/secrets`.
    root: PathBuf,
}

impl SecretStore {
    /// `app_data_dir` is Tauri's `app_data_dir()` — see `infra::app_config`.
    pub fn in_app_data(app_data_dir: &Path) -> Self {
        Self { root: app_data_dir.join(SECRETS_DIRNAME) }
    }

    /// The store, resolved without a Tauri path resolver.
    ///
    /// Same reasoning as `infra::app_config::app_config_path_early`: Tauri's
    /// `app_data_dir()` is `data_dir()/<bundle identifier>` on every desktop
    /// platform, so the two resolutions cannot diverge. Doing it this way is
    /// what lets the host-function arms answer a plugin directly, the way the
    /// keystore arms do, instead of adding an app-only capability that would
    /// deny under `moss-cli`. `None` = the platform has no data dir.
    fn app_default() -> Option<Self> {
        dirs::data_dir().map(|d| Self::in_app_data(&d.join("host.moss.publisher")))
    }

    /// The store or a refusal naming what wanted it. Every caller that cannot
    /// proceed without secrets goes through here, so the refusal reads the same
    /// whether it surfaced from a command, a host function, or the setup gate.
    pub fn require(context: &str) -> std::result::Result<Self, String> {
        Self::app_default().ok_or_else(|| {
            format!("'{context}': no application data directory on this platform")
        })
    }

    /// One secret's file. Key naming is the keystore's rule, for the same
    /// reason: the name becomes a path segment.
    fn path(&self, scope: &Scope, key: &str) -> Result<PathBuf, KeystoreError> {
        super::keystore::validate_entry_name(key)?;
        Ok(self.root.join(scope.dir_segment()?).join(key))
    }

    /// The stored value, or `None` if this plugin has no secret under `key`.
    ///
    /// An empty file reads as `None`, not as `Some("")`. An empty string is not
    /// a usable credential, so treating it as one would make "moss has a token"
    /// true for a slot nothing can authenticate with — and the settings page
    /// asks exactly that question through [`Self::get`]. [`Self::set`] no longer
    /// creates such a file, but plugins spent several releases erasing with
    /// `setSecret(key, "")`, and those files are on disk in the wild.
    pub fn get(&self, scope: &Scope, key: &str) -> Result<Option<String>, KeystoreError> {
        let path = self.path(scope, key)?;
        // allow:raw_read app-global keystore, not the vault — see this module's
        // "App-global, not under `.moss/`" note; nothing here can be evicted.
        match std::fs::read_to_string(&path) {
            Ok(v) if v.is_empty() => Ok(None),
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(KeystoreError::Io),
        }
    }

    /// Store `value` under `key`, replacing whatever was there. An empty
    /// `value` erases instead of storing, exactly as [`Self::remove`] would.
    ///
    /// Both moss's credential modal and a plugin may write here: a plugin may
    /// deposit what a login moss supervised already returned to it, but may
    /// never write a key its own manifest declared as user-supplied, because
    /// only moss draws that field (ADR-072 §3, amended 2026-08-30 — the
    /// caller-side gate lives in `plugins::commands::secrets`).
    ///
    /// Erasing on empty is what keeps [`Self::remove`]'s promise that "moss has
    /// no usable token" has exactly one representation. `setSecret(key, "")` is
    /// how a plugin has always spelled "forget this", and storing it literally
    /// created a second representation that read back as a stored credential.
    pub fn set(&self, scope: &Scope, key: &str, value: &str) -> Result<(), KeystoreError> {
        if value.is_empty() {
            return self.remove(scope, key);
        }
        let path = self.path(scope, key)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| KeystoreError::Io)?;
        }
        write_private(&path, value.as_bytes())
    }

    /// Forget `key`. Absent is success — this is what `rejectSecret` does, and
    /// a plugin rejecting a token that was already cleared has got what it
    /// asked for.
    ///
    /// Rejection *erases* rather than flagging, so "moss has no usable token"
    /// has exactly one representation and the setup gate has one question to
    /// ask. Same shape as git-credential's `erase`.
    pub fn remove(&self, scope: &Scope, key: &str) -> Result<(), KeystoreError> {
        let path = self.path(scope, key)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(KeystoreError::Io),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn plugin(id: &str) -> Scope {
        Scope::Plugin(id.to_string())
    }

    #[test]
    fn a_stored_secret_reads_back_and_a_missing_one_is_none() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        assert_eq!(store.get(&plugin("ipfs"), "pinata_jwt").unwrap(), None);
        store.set(&plugin("ipfs"), "pinata_jwt", "tok-1").unwrap();
        assert_eq!(store.get(&plugin("ipfs"), "pinata_jwt").unwrap().as_deref(), Some("tok-1"));
    }

    /// The scope is structural, so a second plugin asking for the same key name
    /// gets its own answer — it cannot read the first one's token.
    #[test]
    fn one_plugin_cannot_read_anothers_secret() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        store.set(&plugin("ipfs"), "token", "ipfs-token").unwrap();
        assert_eq!(store.get(&plugin("github"), "token").unwrap(), None);
    }

    /// Rejection erases: the next publish must find nothing and re-prompt.
    #[test]
    fn rejecting_a_secret_leaves_nothing_behind() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        store.set(&plugin("ipfs"), "pinata_jwt", "revoked").unwrap();
        store.remove(&plugin("ipfs"), "pinata_jwt").unwrap();
        assert_eq!(store.get(&plugin("ipfs"), "pinata_jwt").unwrap(), None);
        // Idempotent — a plugin may reject twice.
        store.remove(&plugin("ipfs"), "pinata_jwt").unwrap();
    }

    /// `setSecret(key, "")` is how a plugin spells "forget this" — matters logs
    /// out that way. Storing it literally left a file behind, and every
    /// stored-ness check in moss is `get(...).is_some()`, so a signed-out
    /// account went on reporting a credential moss could not authenticate with.
    #[test]
    fn erasing_with_an_empty_value_leaves_nothing_stored() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        store.set(&plugin("matters"), "access_token", "tok-1").unwrap();
        store.set(&plugin("matters"), "access_token", "").unwrap();
        assert_eq!(store.get(&plugin("matters"), "access_token").unwrap(), None);
        let path = dir.path().join(SECRETS_DIRNAME).join("matters").join("access_token");
        assert!(!path.exists(), "an erased secret left a file behind at {path:?}");
    }

    /// The releases that stored `""` literally are already installed, so their
    /// files are on disk and only the read side can answer for them.
    #[test]
    fn an_empty_file_written_by_an_older_moss_reads_as_absent() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        let path = dir.path().join(SECRETS_DIRNAME).join("matters").join("access_token");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();
        assert_eq!(store.get(&plugin("matters"), "access_token").unwrap(), None);
    }

    /// A key name is a path segment, so traversal has to be refused here rather
    /// than trusted to the caller.
    #[test]
    fn a_key_name_cannot_escape_its_scope() {
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        for bad in ["../../etc/passwd", "a/b", ""] {
            assert_eq!(store.set(&plugin("ipfs"), bad, "x"), Err(KeystoreError::InvalidName));
            assert_eq!(store.get(&plugin("ipfs"), bad), Err(KeystoreError::InvalidName));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_secret_is_never_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let store = SecretStore::in_app_data(dir.path());
        store.set(&plugin("ipfs"), "pinata_jwt", "tok").unwrap();
        let path = dir.path().join(SECRETS_DIRNAME).join("ipfs").join("pinata_jwt");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "secret file is readable beyond its owner: {mode:o}");
    }
}
