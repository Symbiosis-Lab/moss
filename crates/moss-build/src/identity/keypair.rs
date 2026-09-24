//! Keypair management for Nostr-style identity.
//!
//! Generates and stores secp256k1 keypairs compatible with Nostr (BIP-340).
//! The public key and metadata are stored in `.moss/identity/public.json`.
//! The private key is stored in `.moss/identity/secret-key` as a hex-encoded file
//! with restrictive permissions (0600 on Unix).
//!
//! # Key Storage (v3 — file-based)
//! - **All platforms**: Private key in `.moss/identity/secret-key` (hex-encoded, 64 chars)
//! - **public.json**: Contains only pubkey, version, and metadata (never the private key)
//!
//! # Migration
//! - **v1 → v3**: Private key extracted from `public.json`, written to `secret-key` file,
//!   stripped from JSON, version set to 3.
//! - **v2 → v3**: Key was in OS keychain (now removed). If `secret-key` file doesn't exist,
//!   the key is unrecoverable. `IdentityService::ensure_signing_key()` handles regeneration.

use fs2::FileExt;
use k256::schnorr::SigningKey;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// Current identity file version.
const IDENTITY_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// Key file helpers
// ---------------------------------------------------------------------------

/// Returns the path to the identity key file: `<project_path>/.moss/identity/secret-key`
pub(crate) fn key_file_path(project_path: &Path) -> PathBuf {
    project_path.join(".moss").join("identity").join("secret-key")
}

/// Write hex-encoded private key to file with restrictive permissions.
///
/// On Unix, the file is created with mode 0o600 (owner read/write only) atomically
/// to avoid a window where the key is world-readable.
/// TODO: On Windows, restrict ACLs to current user only.
pub(crate) fn save_key_to_file(path: &Path, key_bytes: &[u8]) -> Result<(), IdentityError> {
    let hex_key = hex::encode(key_bytes);

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|_| IdentityError::DirectoryCreation)?;
        }
    }

    // On Unix, create file with restrictive permissions atomically (no TOCTOU race)
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // allow:raw_write .moss/identity/ is user-owned key material, not regenerable output
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(IdentityError::FileWrite)?;
        file.write_all(hex_key.as_bytes()).map_err(IdentityError::FileWrite)?;
    }

    #[cfg(not(unix))]
    {
        // allow:raw_write .moss/identity/ is user-owned key material, not regenerable output
        fs::write(path, hex_key.as_bytes()).map_err(IdentityError::FileWrite)?;
    }

    Ok(())
}

/// Load hex-encoded private key bytes from file.
///
/// Reads directly rather than pre-checking `path.exists()`: `exists()` is
/// stat-based and returns `true` for a cloud-dataless file (macOS Sonoma+),
/// so a stat-then-read split would just move the TOCTOU race, not close it.
/// [`IdentityError::PrivateKeyNotFound`] is reserved for a read failure
/// [`crate::build::icloud::is_definitely_absent`] confirms is a real ENOENT —
/// on macOS 12–13, an evicted file is a hidden `.name.icloud` stub and the
/// real path returns ENOENT despite the key existing, which is exactly the
/// case `ensure_signing_key()`'s caller must NOT treat as license to
/// regenerate and overwrite the site's identity. A dataless-fail-fast
/// `EDEADLK` (Sonoma+) is a *different* error kind and always maps to
/// [`IdentityError::Read`], never to `PrivateKeyNotFound`.
///
/// Reads through [`cloud_readiness::read_to_string_with_materialize_wait`], not
/// `fs::read_to_string`. The key lives at `.moss/identity/secret-key` — inside
/// the synced vault, so Google Drive and iCloud both evict it — and under the
/// process-wide fail-fast policy a plain read of an evicted key returns
/// `EDEADLK` immediately. That is what killed publish outright in the
/// identity-file bug:
/// "Identity error: Failed to read identity file: Resource deadlock avoided
/// (os error 11)", with nothing in the system that would ever fix the state on
/// its own. Asking for the file back is the missing half of failing fast.
pub(crate) fn load_key_from_file(path: &Path) -> Result<Vec<u8>, IdentityError> {
    let read = crate::build::cloud_readiness::read_to_string_with_materialize_wait(
        path,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    );
    let hex_str = match read {
        Ok(s) => s,
        Err(err) if crate::build::icloud::is_definitely_absent(path, &err) => {
            return Err(IdentityError::PrivateKeyNotFound);
        }
        Err(err) => return Err(IdentityError::Read(err)),
    };
    let hex_str = hex_str.trim();
    hex::decode(hex_str).map_err(|_| IdentityError::InvalidPrivateKey)
}

// ---------------------------------------------------------------------------
// Identity types
// ---------------------------------------------------------------------------

/// User identity containing Nostr-compatible keypair (BIP-340).
///
/// The private key is stored in `.moss/identity/secret-key`, not in this struct or
/// the JSON file. Only `version`, `pubkey`, and `email` are serialized to JSON.
/// The signing key is lazily loaded from the key file on first access.
#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    /// Version number for format migrations
    pub version: u32,
    /// BIP-340 x-only public key in hex format (64 chars, 32 bytes)
    pub pubkey: String,
    /// Optional email linked via magic link
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The signing key, loaded from file at runtime. Never serialized.
    #[serde(skip)]
    signing_key: Option<SigningKey>,
    /// Path to the project directory, used for lazy key file loading. Never serialized.
    #[serde(skip)]
    project_path: Option<PathBuf>,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("version", &self.version)
            .field("pubkey", &self.pubkey)
            .field("email", &self.email)
            .field("signing_key", &self.signing_key.as_ref().map(|_| "[loaded]"))
            .finish()
    }
}

/// Legacy v1 identity structure (plaintext private key in JSON).
#[derive(Deserialize)]
struct IdentityV1 {
    #[allow(dead_code)]
    version: u32,
    pubkey: String,
    #[serde(rename = "privkey")]
    private_key: String,
    email: Option<String>,
}

/// Errors that can occur during identity operations.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("Failed to read identity file: {0}")]
    Read(std::io::Error),

    #[error("Failed to write identity file: {0}")]
    FileWrite(std::io::Error),

    #[error("Failed to parse identity file: {0}")]
    Parse(serde_json::Error),

    #[error("Invalid private key format")]
    InvalidPrivateKey,

    #[error("Identity not found at {0}")]
    NotFound(PathBuf),

    #[error("Failed to create identity directory")]
    DirectoryCreation,

    #[error("Failed to lock identity file: {0}")]
    Lock(std::io::Error),

    #[error("Private key not found. It may need to be regenerated.")]
    PrivateKeyNotFound,

    #[error("This identity was created with a newer version of moss.")]
    IncompatibleVersion,
}

impl From<serde_json::Error> for IdentityError {
    fn from(e: serde_json::Error) -> Self {
        IdentityError::Parse(e)
    }
}

// ---------------------------------------------------------------------------
// Identity implementation
// ---------------------------------------------------------------------------

impl Identity {
    /// Generate a new random identity.
    ///
    /// Creates a v3 identity with the private key held in memory.
    /// The key is written to `.moss/identity/secret-key` when [`save()`] is called.
    pub fn generate() -> Result<Self, IdentityError> {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = hex::encode(verifying_key.to_bytes());

        Ok(Self {
            version: IDENTITY_VERSION,
            pubkey: pubkey_hex,
            email: None,
            signing_key: Some(signing_key),
            project_path: None,
        })
    }

    /// Load identity from `.moss/identity/public.json`, migrating v1/v2 → v3 if needed.
    ///
    /// - **v1**: Private key extracted from JSON, written to `secret-key` file,
    ///   stripped from JSON, version set to 3.
    /// - **v2**: Key was in OS keychain (removed). If `secret-key` file exists,
    ///   use it. Otherwise, key is unrecoverable — `signing_key` will be `None`
    ///   and `IdentityService::ensure_signing_key()` will regenerate.
    /// - **v3**: Normal load, signing key lazily loaded from `identity-key` file.
    ///
    /// Uses a file lock for atomic migration to prevent concurrent corruption.
    pub fn load(project_path: &Path) -> Result<Self, IdentityError> {
        let identity_path = Self::identity_path(project_path);
        if !identity_path.exists() && !crate::build::icloud::is_still_in_the_cloud(&identity_path) {
            return Err(IdentityError::NotFound(identity_path));
        }

        // Ask for the bytes back before opening. This read cannot go through
        // `read_to_string_with_materialize_wait` the way the key file does: the
        // handle is held open, locked, and — for a v1/v2 identity — migrated in
        // place, so the file must be opened once and kept. `materialize_input`
        // is the same bounded wait expressed as a pre-flight (the identity-file bug). A
        // timeout falls through deliberately: the open below then produces the
        // real `EDEADLK`, which `IdentityError::Read` carries, and NOTHING on
        // this path may look like `PrivateKeyNotFound` — that is what would let
        // `ensure_signing_key` mint a new keypair over a site's live identity.
        if let Err(e) = crate::build::cloud_readiness::materialize_input(
            &identity_path,
            crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
        ) {
            log::warn!("identity: {}", e);
        }

        // allow:raw_write .moss/identity/ is user-owned key material, not regenerable output
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&identity_path)
            .map_err(IdentityError::Read)?;

        file.lock_exclusive().map_err(IdentityError::Lock)?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(IdentityError::Read)?;

        // Detect format: v1 has "privkey" field
        let raw: serde_json::Value = serde_json::from_str(&contents)?;
        let has_privkey = raw.get("privkey").is_some();
        let file_version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

        let identity: Identity = if has_privkey {
            // V1 format: private key is in the JSON — migrate to v3
            let v1: IdentityV1 = serde_json::from_value(raw)?;

            let privkey_bytes =
                hex::decode(&v1.private_key).map_err(|_| IdentityError::InvalidPrivateKey)?;
            let key_array: [u8; 32] = privkey_bytes
                .clone()
                .try_into()
                .map_err(|_| IdentityError::InvalidPrivateKey)?;
            let signing_key =
                SigningKey::from_bytes(&key_array).map_err(|_| IdentityError::InvalidPrivateKey)?;

            // Write key to identity-key file
            let kf_path = key_file_path(project_path);
            save_key_to_file(&kf_path, &privkey_bytes)?;

            log::info!("Migrated v1 identity to file-based key storage (v3).");

            // Rewrite the JSON file without privkey, version=3
            let v3 = Identity {
                version: IDENTITY_VERSION,
                pubkey: v1.pubkey.clone(),
                email: v1.email.clone(),
                signing_key: None,
                project_path: None,
            };
            let new_contents = serde_json::to_string_pretty(&v3)?;
            file.set_len(0).map_err(IdentityError::FileWrite)?;
            file.rewind().map_err(IdentityError::FileWrite)?;
            file.write_all(new_contents.as_bytes())
                .map_err(IdentityError::FileWrite)?;
            file.sync_all().map_err(IdentityError::FileWrite)?;

            Identity {
                version: IDENTITY_VERSION,
                pubkey: v1.pubkey,
                email: v1.email,
                signing_key: Some(signing_key),
                project_path: Some(project_path.to_path_buf()),
            }
        } else if file_version == 2 {
            // V2 format: key was in OS keychain (now removed) — migrate to v3
            let mut identity: Identity = serde_json::from_str(&contents)?;

            let kf_path = key_file_path(project_path);
            if kf_path.exists() {
                // Key file already exists (perhaps from a previous partial migration)
                log::info!("v2 identity: key file already exists, upgrading version to 3.");
            } else {
                // Key was in OS keychain, which is gone. Can't recover.
                log::warn!(
                    "v2 identity: private key was in OS keychain (now removed). \
                     Key file not found. Identity will need regeneration."
                );
            }

            // Update version to 3 in the JSON file
            identity.version = IDENTITY_VERSION;
            let new_contents = serde_json::to_string_pretty(&identity)?;
            file.set_len(0).map_err(IdentityError::FileWrite)?;
            file.rewind().map_err(IdentityError::FileWrite)?;
            file.write_all(new_contents.as_bytes())
                .map_err(IdentityError::FileWrite)?;
            file.sync_all().map_err(IdentityError::FileWrite)?;

            identity.project_path = Some(project_path.to_path_buf());
            identity
        } else {
            // V3 format: normal load, key lazily loaded from file
            let mut identity: Identity = serde_json::from_str(&contents)?;
            identity.project_path = Some(project_path.to_path_buf());
            identity
        };

        // File lock released on drop (fs2 advisory locks are fd-scoped)
        drop(file);

        if identity.version > IDENTITY_VERSION {
            return Err(IdentityError::IncompatibleVersion);
        }

        Ok(identity)
    }

    /// Load the project's identity AND force its signing key into memory.
    ///
    /// [`load`](Self::load) is lazy: it leaves `signing_key = None` and defers
    /// the `secret-key` file read. The seta auth path is non-lazy — it signs via
    /// [`signing_key_loaded`](Self::signing_key_loaded), which returns
    /// [`IdentityError::PrivateKeyNotFound`] if the key was never loaded. So
    /// every caller that builds an *authenticated* `MossSetaClient` from a
    /// project path MUST use this, not bare `load()`, or signed requests fail
    /// with "Private key not found" even though the key exists on disk.
    ///
    /// Unlike [`IdentityService::ensure_signing_key`](super::IdentityService::ensure_signing_key),
    /// this does NOT regenerate on a missing key: minting a new pubkey would make
    /// seta stop recognizing this account as the site's owner.
    pub fn load_for_signing(project_path: &Path) -> Result<Self, IdentityError> {
        let mut identity = Self::load(project_path)?;
        // Force the lazy read of `.moss/identity/secret-key` now, so the later
        // immutable `signing_key_loaded()` in the auth path succeeds.
        identity.signing_key()?;
        Ok(identity)
    }

    /// Save identity to `.moss/identity/public.json` and `.moss/identity/secret-key`.
    ///
    /// Always writes v3 format:
    /// - `public.json`: metadata only (version, pubkey, email)
    /// - `secret-key`: hex-encoded private key (64 chars, 0600 permissions)
    pub fn save(&self, project_path: &Path) -> Result<(), IdentityError> {
        let identity_dir = project_path.join(".moss").join("identity");
        if !identity_dir.exists() {
            fs::create_dir_all(&identity_dir).map_err(|_| IdentityError::DirectoryCreation)?;
        }

        let identity_path = Self::identity_path(project_path);

        // Always save v3 format (metadata only in JSON)
        let v3 = Identity {
            version: IDENTITY_VERSION,
            pubkey: self.pubkey.clone(),
            email: self.email.clone(),
            signing_key: None,
            project_path: None,
        };
        let contents = serde_json::to_string_pretty(&v3)?;
        // allow:raw_write .moss/identity/ is user-owned key material, not regenerable output
        fs::write(&identity_path, contents).map_err(IdentityError::FileWrite)?;

        // Write the key file if we have the signing key in memory
        if let Some(ref sk) = self.signing_key {
            let kf_path = key_file_path(project_path);
            save_key_to_file(&kf_path, &sk.to_bytes())?;
        }

        Ok(())
    }

    /// Check if identity exists for a project.
    ///
    /// A cloud placeholder counts as existing. `IdentityService::get_or_create`
    /// reads this as "load it" versus "generate a new one and save it," and
    /// saving writes over `.moss/identity/`. On macOS 12–13 an evicted file is
    /// *replaced* by a hidden `.public.json.icloud` sibling, so a plain
    /// `exists()` reports the user's identity as absent and the else-branch
    /// mints a replacement — losing the key that ties them to their published
    /// site, for no reason but that iCloud had it that minute. Refusing is the
    /// only safe answer: `load()` then fails loudly and nothing is overwritten.
    pub fn exists(project_path: &Path) -> bool {
        let path = Self::identity_path(project_path);
        path.exists() || crate::build::icloud::is_still_in_the_cloud(&path)
    }

    /// Get the path to the identity file.
    pub fn identity_path(project_path: &Path) -> PathBuf {
        project_path.join(".moss").join("identity").join("public.json")
    }

    /// Check if the signing key is already loaded in memory.
    ///
    /// Returns `true` if the signing key has been loaded from the key file
    /// (or was generated in-memory). Useful for diagnostics and testing.
    pub fn has_signing_key_loaded(&self) -> bool {
        self.signing_key.is_some()
    }

    /// Get the signing key if already loaded. Returns error if not yet loaded.
    /// Use `signing_key()` or `ensure_signing_key()` first to load from disk.
    pub fn signing_key_loaded(&self) -> Result<&SigningKey, IdentityError> {
        self.signing_key.as_ref().ok_or(IdentityError::PrivateKeyNotFound)
    }

    /// Get the signing key for BIP-340 Schnorr signatures.
    ///
    /// Lazily loads from `.moss/identity/secret-key` on first access for v3 identities.
    pub fn signing_key(&mut self) -> Result<&SigningKey, IdentityError> {
        if self.signing_key.is_none() {
            let path = self
                .project_path
                .as_ref()
                .ok_or(IdentityError::PrivateKeyNotFound)?;
            let kf_path = key_file_path(path);
            let secret_bytes = load_key_from_file(&kf_path)?;
            let key_array: [u8; 32] = secret_bytes
                .try_into()
                .map_err(|_| IdentityError::InvalidPrivateKey)?;
            let signing_key =
                SigningKey::from_bytes(&key_array).map_err(|_| IdentityError::InvalidPrivateKey)?;
            self.signing_key = Some(signing_key);
        }
        Ok(self.signing_key.as_ref().unwrap())
    }

    /// Link an email address to this identity.
    pub fn link_email(&mut self, email: String) {
        self.email = Some(email);
    }

    /// Export the private key for backup (nsec format).
    ///
    /// Uses the in-memory signing key if loaded, falls back to reading
    /// from `.moss/identity/secret-key`.
    pub fn export_private_key(&self) -> Result<String, IdentityError> {
        if let Some(ref sk) = self.signing_key {
            return Ok(format!("nsec:{}", hex::encode(sk.to_bytes())));
        }
        let path = self
            .project_path
            .as_ref()
            .ok_or(IdentityError::PrivateKeyNotFound)?;
        let kf_path = key_file_path(path);
        let secret_bytes = load_key_from_file(&kf_path)?;
        Ok(format!("nsec:{}", hex::encode(secret_bytes)))
    }

    /// Import identity from a private key string.
    ///
    /// Creates a v3 identity with the key held in memory until [`save()`].
    pub fn import(private_key_str: &str) -> Result<Self, IdentityError> {
        let hex_key = private_key_str
            .strip_prefix("nsec:")
            .unwrap_or(private_key_str);

        let bytes = hex::decode(hex_key).map_err(|_| IdentityError::InvalidPrivateKey)?;
        let bytes_array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidPrivateKey)?;
        let signing_key =
            SigningKey::from_bytes(&bytes_array).map_err(|_| IdentityError::InvalidPrivateKey)?;

        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = hex::encode(verifying_key.to_bytes());

        Ok(Self {
            version: IDENTITY_VERSION,
            pubkey: pubkey_hex,
            email: None,
            signing_key: Some(signing_key),
            project_path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // New v3 tests (TDD — written first)
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_v3_identity() {
        let identity = Identity::generate().unwrap();

        // Should be v3
        assert_eq!(identity.version, 3);
        // Valid BIP-340 x-only pubkey (32 bytes = 64 hex chars)
        assert_eq!(identity.pubkey.len(), 64);
        assert!(identity.pubkey.chars().all(|c| c.is_ascii_hexdigit()));
        // No email initially
        assert!(identity.email.is_none());
        // Signing key in memory (not yet saved to file)
        assert!(identity.signing_key.is_some());
    }

    #[test]
    fn test_generate_v3_key_not_in_json() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate().unwrap();
        identity.save(dir.path()).unwrap();

        // Read the JSON file and verify no private key
        let contents = fs::read_to_string(Identity::identity_path(dir.path())).unwrap();
        assert!(!contents.contains("privkey"));
        assert!(!contents.contains("signing_key"));
        assert!(!contents.contains("private_key"));

        let raw: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(raw["version"], 3);
        assert_eq!(raw["pubkey"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn test_generate_v3_key_written_to_file() {
        let dir = tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        let key_bytes = identity.signing_key().unwrap().to_bytes();

        identity.save(dir.path()).unwrap();

        // identity-key file should exist
        let kf_path = key_file_path(dir.path());
        assert!(kf_path.exists(), "identity-key file should exist after save");

        // Should contain the hex-encoded key
        let file_contents = fs::read_to_string(&kf_path).unwrap();
        assert_eq!(file_contents, hex::encode(key_bytes));
        assert_eq!(file_contents.len(), 64, "hex-encoded 32-byte key = 64 chars");
    }

    #[test]
    fn test_v3_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        let original_pubkey = identity.pubkey.clone();
        let original_key_bytes = identity.signing_key().unwrap().to_bytes();

        identity.save(dir.path()).unwrap();
        let mut loaded = Identity::load(dir.path()).unwrap();

        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.pubkey, original_pubkey);
        assert_eq!(loaded.signing_key().unwrap().to_bytes(), original_key_bytes);
    }

    #[test]
    fn test_v1_to_v3_migration() {
        let dir = tempdir().unwrap();
        let project_path = dir.path();

        // Create a v1 identity (privkey in JSON)
        let v1_key = SigningKey::random(&mut OsRng);
        let v1_pubkey = hex::encode(v1_key.verifying_key().to_bytes());
        let v1_privkey = hex::encode(v1_key.to_bytes());

        let v1_json = format!(
            r#"{{"version":1,"pubkey":"{}","privkey":"{}","email":"test@example.com"}}"#,
            v1_pubkey, v1_privkey
        );

        let moss_dir = project_path.join(".moss");
        fs::create_dir_all(moss_dir.join("identity")).unwrap();
        fs::write(Identity::identity_path(project_path), &v1_json).unwrap();

        // Load should trigger migration
        let mut loaded = Identity::load(project_path).unwrap();

        // Should be v3 now
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.pubkey, v1_pubkey);
        assert_eq!(loaded.email, Some("test@example.com".to_string()));
        assert_eq!(loaded.signing_key().unwrap().to_bytes(), v1_key.to_bytes());

        // JSON should no longer contain privkey
        let new_contents = fs::read_to_string(Identity::identity_path(project_path)).unwrap();
        assert!(!new_contents.contains("privkey"));
        let raw: serde_json::Value = serde_json::from_str(&new_contents).unwrap();
        assert_eq!(raw["version"], 3);

        // identity-key file should exist with the key
        let kf_path = key_file_path(project_path);
        assert!(kf_path.exists(), "identity-key file should be created during v1 migration");
        let key_from_file = load_key_from_file(&kf_path).unwrap();
        assert_eq!(key_from_file, v1_key.to_bytes().to_vec());
    }

    #[test]
    fn test_v2_to_v3_migration_with_key_file() {
        let dir = tempdir().unwrap();
        let project_path = dir.path();

        // Create a v2 identity JSON (no privkey, key was in keychain)
        let key = SigningKey::random(&mut OsRng);
        let pubkey = hex::encode(key.verifying_key().to_bytes());

        let v2_json = format!(
            r#"{{"version":2,"pubkey":"{}","email":"test@example.com"}}"#,
            pubkey
        );

        let moss_dir = project_path.join(".moss");
        fs::create_dir_all(moss_dir.join("identity")).unwrap();
        fs::write(Identity::identity_path(project_path), &v2_json).unwrap();

        // Simulate: the key file was already created (e.g., by a prior partial migration)
        let kf_path = key_file_path(project_path);
        save_key_to_file(&kf_path, &key.to_bytes()).unwrap();

        // Load should upgrade version to 3
        let mut loaded = Identity::load(project_path).unwrap();
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.pubkey, pubkey);
        assert_eq!(loaded.email, Some("test@example.com".to_string()));

        // Should be able to load key from file
        assert_eq!(loaded.signing_key().unwrap().to_bytes(), key.to_bytes());

        // JSON should now say version 3
        let new_contents = fs::read_to_string(Identity::identity_path(project_path)).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&new_contents).unwrap();
        assert_eq!(raw["version"], 3);
    }

    #[test]
    fn test_v2_to_v3_migration_without_key_file() {
        let dir = tempdir().unwrap();
        let project_path = dir.path();

        // Create a v2 identity JSON (no privkey, key was in keychain)
        let pubkey = "a".repeat(64);
        let v2_json = format!(
            r#"{{"version":2,"pubkey":"{}"}}"#,
            pubkey
        );

        let moss_dir = project_path.join(".moss");
        fs::create_dir_all(moss_dir.join("identity")).unwrap();
        fs::write(Identity::identity_path(project_path), &v2_json).unwrap();

        // No key file exists — key was in keychain, which is gone

        // Load should succeed but signing_key will be None
        let loaded = Identity::load(project_path).unwrap();
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.pubkey, pubkey);
        assert!(!loaded.has_signing_key_loaded());

        // Attempting to get signing key should fail with PrivateKeyNotFound
        let mut loaded = loaded;
        let result = loaded.signing_key();
        assert!(
            matches!(result, Err(IdentityError::PrivateKeyNotFound)),
            "signing_key() should return PrivateKeyNotFound when key file is missing"
        );
    }

    /// The same invariant one level up: `exists()` is what decides between
    /// loading the user's identity and minting a new one over the top of it.
    /// A pre-Sonoma placeholder must read as "there," so the caller loads
    /// (and fails loudly) instead of generating and saving.
    #[test]
    #[cfg(target_os = "macos")]
    fn identity_evicted_to_an_icloud_stub_still_counts_as_existing() {
        let dir = tempdir().unwrap();
        let id_dir = dir.path().join(".moss").join("identity");
        fs::create_dir_all(&id_dir).unwrap();

        assert!(
            !Identity::exists(dir.path()),
            "a project with no identity at all must not claim to have one"
        );

        fs::write(id_dir.join(".public.json.icloud"), b"stub").unwrap();
        assert!(
            Identity::exists(dir.path()),
            "an identity evicted to a placeholder must NOT read as absent — the \
             else-branch of get_or_create would overwrite it with a fresh keypair"
        );
    }

    /// Regression test for the "unreadable is not absent" invariant (design
    /// doc §4): on macOS 12–13, an evicted iCloud file is a hidden
    /// `.name.icloud` stub and the real path returns ENOENT — a truly-present
    /// key must NOT be classified as `PrivateKeyNotFound` (which
    /// `ensure_signing_key` treats as license to regenerate and overwrite
    /// the site's permanent identity) just because the real path is
    /// momentarily absent while its stub sibling is present.
    #[test]
    #[cfg(target_os = "macos")]
    fn load_key_from_file_with_icloud_stub_sibling_is_not_private_key_not_found() {
        let dir = tempdir().unwrap();
        let key_path = dir.path().join("secret-key");
        // The real key file does NOT exist — only its `.icloud` stub does,
        // exactly the pre-Sonoma eviction shape.
        fs::write(dir.path().join(".secret-key.icloud"), b"stub").unwrap();

        let result = load_key_from_file(&key_path);
        assert!(
            matches!(result, Err(IdentityError::Read(_))),
            "a present .icloud stub must propagate a Read error, not PrivateKeyNotFound \
             (got {result:?}) — treating this as PrivateKeyNotFound would regenerate and \
             overwrite the site's identity while the real key still exists in the cloud"
        );
    }

    /// Companion case: a genuinely absent key (no stub sibling either) must
    /// still classify as `PrivateKeyNotFound` so first-run auto-creation
    /// keeps working.
    #[test]
    fn load_key_from_file_truly_missing_is_private_key_not_found() {
        let dir = tempdir().unwrap();
        let key_path = dir.path().join("secret-key");

        let result = load_key_from_file(&key_path);
        assert!(
            matches!(result, Err(IdentityError::PrivateKeyNotFound)),
            "a genuinely missing key file (no stub) must classify as PrivateKeyNotFound, got {result:?}"
        );
    }

    #[test]
    fn test_signing_key_lazy_load_from_file() {
        let dir = tempdir().unwrap();

        // Generate and save
        let mut original = Identity::generate().unwrap();
        let original_key_bytes = original.signing_key().unwrap().to_bytes();
        original.save(dir.path()).unwrap();

        // Load — signing_key should be None (lazy)
        let mut loaded = Identity::load(dir.path()).unwrap();
        assert!(
            loaded.signing_key.is_none(),
            "signing_key should be None after load (lazy)"
        );

        // First call to signing_key() should load from file
        let key = loaded.signing_key().unwrap();
        assert_eq!(key.to_bytes(), original_key_bytes);

        // After the call, the field should be cached
        assert!(
            loaded.signing_key.is_some(),
            "signing_key should be cached after first access"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_identity_key_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let identity = Identity::generate().unwrap();
        identity.save(dir.path()).unwrap();

        let kf_path = key_file_path(dir.path());
        let metadata = fs::metadata(&kf_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;

        assert_eq!(
            mode, 0o600,
            "identity-key file should have 0600 permissions, got {:o}",
            mode
        );
    }

    // -----------------------------------------------------------------------
    // Updated existing tests (adapted for v3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_identity() {
        let identity = Identity::generate().unwrap();
        assert_eq!(identity.version, 3);
        assert_eq!(identity.pubkey.len(), 64);
        assert!(identity.email.is_none());
        assert!(identity.signing_key.is_some());
    }

    #[test]
    fn test_save_no_privkey_in_json() {
        let dir = tempdir().unwrap();
        let identity = Identity::generate().unwrap();
        identity.save(dir.path()).unwrap();

        let contents = fs::read_to_string(Identity::identity_path(dir.path())).unwrap();
        assert!(!contents.contains("privkey"));
        assert!(!contents.contains("signing_key"));
        assert!(!contents.contains("private_key"));

        let raw: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(raw["version"], 3);
        assert_eq!(raw["pubkey"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        let original_pubkey = identity.pubkey.clone();
        let original_key_bytes = identity.signing_key().unwrap().to_bytes();

        identity.save(dir.path()).unwrap();
        let mut loaded = Identity::load(dir.path()).unwrap();

        assert_eq!(loaded.pubkey, original_pubkey);
        assert_eq!(loaded.signing_key().unwrap().to_bytes(), original_key_bytes);
    }

    #[test]
    fn test_import_and_export() {
        let original_key = SigningKey::random(&mut OsRng);
        let nsec = format!("nsec:{}", hex::encode(original_key.to_bytes()));

        let mut imported = Identity::import(&nsec).unwrap();
        assert_eq!(imported.version, 3);
        assert_eq!(imported.signing_key().unwrap().to_bytes(), original_key.to_bytes());

        let exported = imported.export_private_key().unwrap();
        assert_eq!(exported, nsec);
    }

    #[test]
    fn test_import_without_nsec_prefix() {
        let original_key = SigningKey::random(&mut OsRng);
        let hex_key = hex::encode(original_key.to_bytes());

        let mut imported = Identity::import(&hex_key).unwrap();
        assert_eq!(imported.signing_key().unwrap().to_bytes(), original_key.to_bytes());
    }

    #[test]
    fn test_link_email() {
        let mut identity = Identity::generate().unwrap();
        assert!(identity.email.is_none());

        identity.link_email("test@example.com".to_string());
        assert_eq!(identity.email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_incompatible_version() {
        let dir = tempdir().unwrap();
        let project_path = dir.path();

        let moss_dir = project_path.join(".moss");
        fs::create_dir_all(moss_dir.join("identity")).unwrap();
        let fake_pubkey = "a".repeat(64);
        let future_json = format!(r#"{{"version":99,"pubkey":"{}"}}"#, fake_pubkey);
        fs::write(Identity::identity_path(project_path), &future_json).unwrap();

        let result = Identity::load(project_path);
        assert!(matches!(result, Err(IdentityError::IncompatibleVersion)));
    }

    #[test]
    fn test_load_nonexistent() {
        let dir = tempdir().unwrap();
        let result = Identity::load(dir.path());
        assert!(matches!(result, Err(IdentityError::NotFound(_))));
    }

    #[test]
    fn test_export_from_file() {
        // Test that export_private_key() can read from file when key is not in memory
        let dir = tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        let original_key_bytes = identity.signing_key().unwrap().to_bytes();
        identity.save(dir.path()).unwrap();

        // Load identity (signing_key is None, lazy)
        let loaded = Identity::load(dir.path()).unwrap();
        assert!(!loaded.has_signing_key_loaded());

        // export_private_key should read from file
        let exported = loaded.export_private_key().unwrap();
        let expected = format!("nsec:{}", hex::encode(original_key_bytes));
        assert_eq!(exported, expected);
    }

    #[test]
    fn test_save_with_email_roundtrip() {
        let dir = tempdir().unwrap();
        let mut identity = Identity::generate().unwrap();
        identity.link_email("user@example.com".to_string());
        identity.save(dir.path()).unwrap();

        let loaded = Identity::load(dir.path()).unwrap();
        assert_eq!(loaded.email, Some("user@example.com".to_string()));
        assert_eq!(loaded.version, 3);
    }
}
