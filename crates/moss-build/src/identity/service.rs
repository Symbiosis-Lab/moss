//! Core Identity Service
//!
//! # Architecture Decision
//!
//! Identity management is independent of any specific backend service.
//! This allows the same identity to be used by:
//! - moss-seta (domain search, purchase)
//! - Nostr (content syndication)
//! - Future services requiring user identity
//!
//! # Storage
//!
//! The public key and metadata are stored per-project at `{project_path}/.moss/identity/public.json`.
//! The private key is stored in `.moss/identity/secret-key` (hex-encoded, 0600 permissions on Unix).
//!
//! # Why Per-Project (not Global)?
//!
//! - Privacy: Different projects may need different public identities
//! - Isolation: Domain purchases are project-specific
//! - Flexibility: Users can share projects without sharing identity

use std::path::{Path, PathBuf};
use super::keypair::{Identity, IdentityError};

/// Core identity service - manages user identity for a project.
///
/// This service handles loading, creating, and caching the project's identity,
/// abstracting away the details of key file storage and file management.
///
/// # Usage
///
/// ```rust,ignore
/// let mut service = IdentityService::new(&project_path);
/// let identity = service.get_or_create()?;  // Auto-creates if missing
/// ```
pub struct IdentityService {
    project_path: PathBuf,
    cached: Option<Identity>,
}

#[allow(dead_code)]
impl IdentityService {
    /// Create a new identity service for a project.
    pub fn new(project_path: &Path) -> Self {
        Self {
            project_path: project_path.to_path_buf(),
            cached: None,
        }
    }

    /// Get the identity, creating one if it doesn't exist.
    ///
    /// This is the primary method for obtaining an identity.
    /// It handles auto-generation for seamless onboarding.
    ///
    /// This method does NOT load the signing key from the key file. The signing
    /// key is loaded lazily via [`ensure_signing_key()`] only when signing is
    /// actually needed. Callers that only need the pubkey or email (e.g., DNS
    /// checks) can use this method without any file I/O beyond identity.json.
    pub fn get_or_create(&mut self) -> Result<&Identity, IdentityError> {
        if self.cached.is_none() {
            let identity = if Identity::exists(&self.project_path) {
                Identity::load(&self.project_path)?
            } else {
                let identity = Identity::generate()?;
                identity.save(&self.project_path)?;
                identity
            };

            self.cached = Some(identity);
        }
        Ok(self.cached.as_ref().unwrap())
    }

    /// Get the identity if it exists, without creating one.
    ///
    /// Like [`get_or_create`], this does NOT load the signing key from file.
    /// Call [`ensure_signing_key()`] when signing is actually needed.
    pub fn get(&mut self) -> Result<Option<&Identity>, IdentityError> {
        if self.cached.is_none() && Identity::exists(&self.project_path) {
            let identity = Identity::load(&self.project_path)?;
            self.cached = Some(identity);
        }
        Ok(self.cached.as_ref())
    }

    /// Load the signing key from `.moss/identity/secret-key` into memory, once.
    ///
    /// A key file that is confirmed gone is [`IdentityError::PrivateKeyNotFound`],
    /// never a new key: a folder with a `public.json` was signed in, and a key
    /// minted over it is one the hosting service has never seen, so it could
    /// not publish the folder's site. Only [`key_for_sign_in`](Self::key_for_sign_in)
    /// replaces a lost key. A key that cannot be read yet (still in the cloud)
    /// is a different error and replaces nothing.
    pub fn ensure_signing_key(&mut self) -> Result<(), IdentityError> {
        self.get_or_create()?;
        let identity = self.cached.as_mut().expect("get_or_create fills the cache");
        if !identity.has_signing_key_loaded() {
            identity.signing_key()?;
        }
        Ok(())
    }

    /// The identity a sign-in sends, with its signing key loaded. A folder whose
    /// key file is gone gets a new key; `save` retires the lost one, and the new
    /// key publishes nothing until the sign-in link sent for it is opened.
    pub fn key_for_sign_in(&mut self) -> Result<&Identity, IdentityError> {
        match self.ensure_signing_key() {
            Err(IdentityError::PrivateKeyNotFound) => self.own_key_for_sign_in(),
            other => other.map(|()| self.cached.as_ref().expect("loaded above")),
        }
    }

    /// A new key for a sign-in, the only place one is minted over a folder that
    /// has one: for a lost key, and for a key the server says is signed in to
    /// another site (a copy of another folder's key, which `save` keeps among
    /// the retired keys while the folder it came from keeps using it).
    pub fn own_key_for_sign_in(&mut self) -> Result<&Identity, IdentityError> {
        let fresh = Identity::generate()?;
        fresh.save(&self.project_path)?;
        Ok(self.cached.insert(fresh))
    }

    /// Check if an identity exists for this project.
    pub fn exists(&self) -> bool {
        Identity::exists(&self.project_path)
    }

    /// Get the project path this service is bound to.
    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    /// Clear the cached identity, forcing a reload on next access.
    pub fn clear_cache(&mut self) {
        self.cached = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_new_service_has_no_cached_identity() {
        let dir = tempdir().unwrap();
        let service = IdentityService::new(dir.path());

        assert!(!service.exists());
        assert_eq!(service.project_path(), dir.path());
    }

    #[test]
    fn test_get_or_create_generates_identity_when_none_exists() {

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Should auto-generate identity
        let identity = service.get_or_create().unwrap();
        assert!(!identity.pubkey.is_empty());

        // Identity file should be created
        let identity_path = dir.path().join(".moss").join("identity").join("public.json");
        assert!(identity_path.exists());
    }

    #[test]
    fn test_get_or_create_returns_same_identity_on_subsequent_calls() {

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        let identity1 = service.get_or_create().unwrap();
        let pubkey1 = identity1.pubkey.clone();

        let identity2 = service.get_or_create().unwrap();
        let pubkey2 = identity2.pubkey.clone();

        assert_eq!(pubkey1, pubkey2);
    }

    #[test]
    fn test_get_returns_none_when_no_identity() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        let identity = service.get().unwrap();
        assert!(identity.is_none());
    }

    #[test]
    fn test_get_returns_identity_when_exists() {

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Create identity first
        service.get_or_create().unwrap();

        // Now get should return it
        let identity = service.get().unwrap();
        assert!(identity.is_some());
    }

    #[test]
    fn test_clear_cache_forces_reload() {

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Create identity
        let pubkey1 = service.get_or_create().unwrap().pubkey.clone();

        // Clear cache
        service.clear_cache();

        // Get should reload from disk
        let pubkey2 = service.get_or_create().unwrap().pubkey.clone();

        // Should be same identity (loaded from disk)
        assert_eq!(pubkey1, pubkey2);
    }

    #[test]
    fn test_exists_returns_true_after_creation() {

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        assert!(!service.exists());

        service.get_or_create().unwrap();

        assert!(service.exists());
    }

    #[test]
    fn test_get_or_create_eagerly_loads_signing_key_for_generated_identity() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Generated identity should have signing key loaded (it was just created in memory)
        let identity = service.get_or_create().unwrap();
        assert!(
            identity.has_signing_key_loaded(),
            "Generated identity should have signing key pre-loaded"
        );
    }

    #[test]
    fn test_get_or_create_does_not_load_signing_key_for_loaded_identity() {
        let dir = tempdir().unwrap();

        // First, create an identity and save it to disk
        {
            let mut service = IdentityService::new(dir.path());
            service.get_or_create().unwrap();
        }

        // Now create a fresh service that will load from disk
        let mut service = IdentityService::new(dir.path());

        // The loaded identity should NOT have signing key loaded (lazy, not loaded from file)
        let identity = service.get_or_create().unwrap();
        assert!(
            !identity.has_signing_key_loaded(),
            "Loaded identity should NOT have signing key loaded — file read is deferred"
        );
    }

    #[test]
    fn test_cloned_identity_retains_signing_key_after_ensure() {
        let dir = tempdir().unwrap();

        // Create and save an identity
        {
            let mut service = IdentityService::new(dir.path());
            service.get_or_create().unwrap();
        }

        // Load from disk via a fresh service
        let mut service = IdentityService::new(dir.path());

        // ensure_signing_key() loads it from key file
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();

        // Clone the identity (simulates what get_identity_from_state does)
        let cloned = identity.clone();
        assert!(
            cloned.has_signing_key_loaded(),
            "Cloned identity should retain the signing key after ensure_signing_key()"
        );
    }

    #[test]
    fn test_get_does_not_load_signing_key() {
        let dir = tempdir().unwrap();

        // Create and save an identity
        {
            let mut service = IdentityService::new(dir.path());
            service.get_or_create().unwrap();
        }

        // Load from disk via get() on a fresh service
        let mut service = IdentityService::new(dir.path());
        let identity = service.get().unwrap().unwrap();
        assert!(
            !identity.has_signing_key_loaded(),
            "Identity loaded via get() should NOT have signing key loaded — file read is deferred"
        );
    }

    #[test]
    fn test_clear_cache_and_reload_does_not_load_signing_key() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Create identity
        service.get_or_create().unwrap();

        // Clear cache (forces reload from disk on next access)
        service.clear_cache();

        // Reload — signing key should NOT be loaded (lazy)
        let identity = service.get_or_create().unwrap();
        assert!(
            !identity.has_signing_key_loaded(),
            "Reloaded identity after cache clear should NOT have signing key loaded"
        );
    }

    #[test]
    fn test_clear_cache_and_ensure_signing_key_reloads_key() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Create identity
        service.get_or_create().unwrap();

        // Clear cache (forces reload from disk on next access)
        service.clear_cache();

        // ensure_signing_key() should load from key file
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();
        assert!(
            identity.has_signing_key_loaded(),
            "Identity should have signing key loaded after ensure_signing_key()"
        );
    }

    /// A folder that was signed in and lost its key is refused, never given a
    /// new key the hosting service has not seen.
    #[test]
    fn a_lost_key_is_refused_and_never_replaced() {
        use crate::identity::keypair::key_file_path;

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());
        let original = service.get_or_create().unwrap().pubkey.clone();
        std::fs::remove_file(key_file_path(dir.path())).unwrap();
        service.clear_cache();

        let err = service.ensure_signing_key().expect_err("a lost key must refuse");
        assert!(matches!(err, IdentityError::PrivateKeyNotFound), "{err:?}");
        assert_eq!(crate::identity::Identity::read_pubkey(dir.path()), Some(original));
        assert!(!key_file_path(dir.path()).exists(), "no key may be written");
    }

    /// A sign-in is the one place a lost key is replaced; the lost one is
    /// retired, and the new one carries no email until its link is opened.
    #[test]
    fn only_a_sign_in_replaces_a_lost_key_and_retires_it() {
        use crate::identity::keypair::{key_file_path, Identity};

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());
        let original = service.get_or_create().unwrap().pubkey.clone();
        std::fs::remove_file(key_file_path(dir.path())).unwrap();
        service.clear_cache();

        let fresh = service.key_for_sign_in().unwrap().clone();
        assert_ne!(fresh.pubkey, original);
        assert!(fresh.has_signing_key_loaded() && fresh.email.is_none(), "{fresh:?}");
        assert_eq!(Identity::load(dir.path()).unwrap().pubkey, fresh.pubkey, "saved to the folder");
        let retired = std::fs::read_to_string(dir.path().join(".moss/identity/previous-keys.jsonl")).unwrap();
        assert!(retired.contains(&original), "{retired}");

        assert_eq!(service.key_for_sign_in().unwrap().pubkey, fresh.pubkey, "a key that is there is kept");
    }

    /// A key that exists but cannot be read yet, as one still downloading from
    /// a cloud drive, is an error to wait out; a sign-in must not replace it.
    #[test]
    fn a_key_that_cannot_be_read_is_never_replaced() {
        use crate::identity::keypair::key_file_path;

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());
        let original = service.get_or_create().unwrap().pubkey.clone();
        std::fs::remove_file(key_file_path(dir.path())).unwrap();
        std::fs::create_dir(key_file_path(dir.path())).unwrap();
        service.clear_cache();

        let err = service.key_for_sign_in().expect_err("an unreadable key must refuse");
        assert!(!matches!(err, IdentityError::PrivateKeyNotFound), "{err:?}");
        assert_eq!(crate::identity::Identity::read_pubkey(dir.path()), Some(original));
    }

    #[test]
    fn test_get_does_not_regenerate_on_lost_signing_key() {
        use crate::identity::keypair::key_file_path;

        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // Create an identity normally
        let original_pubkey = service.get_or_create().unwrap().pubkey.clone();

        // Simulate key loss: delete the identity-key file
        let kf_path = key_file_path(dir.path());
        std::fs::remove_file(&kf_path).unwrap();

        // Clear the service cache so it reloads from disk
        service.clear_cache();

        // get() should NOT detect the lost key — it doesn't access the key file
        let identity = service.get().unwrap();
        assert!(identity.is_some(), "get() should return Some (loaded from disk)");

        let identity = identity.unwrap();

        // Pubkey should be the SAME (no regeneration happened)
        assert_eq!(
            identity.pubkey, original_pubkey,
            "get() should NOT have regenerated — it doesn't read the key file"
        );

        // Signing key should NOT be loaded
        assert!(
            !identity.has_signing_key_loaded(),
            "Identity from get() should not have signing key loaded"
        );
    }

    #[test]
    fn test_ensure_signing_key_loads_key_from_file() {
        let dir = tempdir().unwrap();

        // Create and save an identity
        {
            let mut service = IdentityService::new(dir.path());
            service.get_or_create().unwrap();
        }

        // Load from disk via a fresh service — signing key not loaded yet
        let mut service = IdentityService::new(dir.path());
        let identity = service.get_or_create().unwrap();
        assert!(
            !identity.has_signing_key_loaded(),
            "Signing key should not be loaded before ensure_signing_key()"
        );

        // ensure_signing_key() loads it from key file
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();
        assert!(
            identity.has_signing_key_loaded(),
            "Signing key should be loaded after ensure_signing_key()"
        );
    }

    #[test]
    fn test_ensure_signing_key_is_idempotent() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());
        service.get_or_create().unwrap();

        // Call ensure_signing_key() multiple times — should not error
        service.ensure_signing_key().unwrap();
        service.ensure_signing_key().unwrap();
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();
        assert!(identity.has_signing_key_loaded());
    }

    #[test]
    fn test_ensure_signing_key_creates_identity_if_missing() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // No identity exists yet — ensure_signing_key() calls get_or_create() internally
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();
        assert!(!identity.pubkey.is_empty());
        assert!(
            identity.has_signing_key_loaded(),
            "Signing key should be loaded for newly generated identity"
        );
    }

    #[test]
    fn test_ensure_signing_key_skips_file_read_for_generated_identity() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());

        // get_or_create generates the identity — signing key is already in memory
        let identity = service.get_or_create().unwrap();
        assert!(
            identity.has_signing_key_loaded(),
            "Generated identity already has signing key in memory"
        );

        // ensure_signing_key() should be a no-op (key already loaded)
        service.ensure_signing_key().unwrap();

        let identity = service.get_or_create().unwrap();
        assert!(identity.has_signing_key_loaded());
    }

    #[test]
    fn a_key_that_cannot_be_retired_is_not_replaced() {
        let dir = tempdir().unwrap();
        let mut service = IdentityService::new(dir.path());
        let original = service.get_or_create().unwrap().pubkey.clone();
        std::fs::remove_file(crate::identity::keypair::key_file_path(dir.path())).unwrap();
        std::fs::create_dir_all(dir.path().join(".moss/identity/previous-keys.jsonl")).unwrap();
        service.clear_cache();

        assert!(service.key_for_sign_in().is_err(), "no new key without the old one recorded");
        assert_eq!(crate::identity::Identity::read_pubkey(dir.path()), Some(original), "public.json still names the old key");
    }
}

