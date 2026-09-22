//! The keystore: keys as a platform primitive.
//!
//! moss holds key bytes; callers use keys. A caller asks for a key by name,
//! signs with it, and lists its keys — it never receives private bytes. Every
//! name resolves inside the *caller's own* scope, so one caller cannot name or
//! reach another's key. The scope is structural: it comes from who is calling
//! (the plugin dispatch seam, or moss itself), never a parameter, so there is
//! nothing to forge.
//!
//! The user's own identity key is one entry here too, at the reserved [`SYSTEM`]
//! scope — the keystore is the general mechanism, and identity is its flagship
//! caller (ADR-031). Plugin keys are separate keys at per-plugin scopes.
//!
//! Custody, not restriction (ADR-032): moss holds the bytes because
//! `.moss/plugins/` is not gitignored, so a caller holding its own key bytes
//! would commit a site's permanent, unrotatable key to the user's repo. Holding
//! the bytes preserves the caller's agency (the key keeps working, stays the
//! caller's, stays user-exportable) by preventing that footgun. Using a key is
//! not a gated capability: a caller signs only with its own key, spending
//! nothing of the user's or another caller's.

use std::path::{Path, PathBuf};

/// A signing algorithm a keystore entry can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// Ed25519 (EdDSA over RFC 8032). Signature: raw 64 bytes. Public key: 32
    /// bytes. The right default for a fresh key — IPNS's `MUST` type.
    Ed25519,
    /// secp256k1 with BIP-340 Schnorr. Signature: 64 bytes. Public key: x-only
    /// 32 bytes. What the user's Nostr/seta identity uses.
    Secp256k1Schnorr,
}

impl Algorithm {
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "ed25519" => Some(Self::Ed25519),
            "secp256k1-schnorr" => Some(Self::Secp256k1Schnorr),
            _ => None,
        }
    }

    pub fn as_wire(&self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::Secp256k1Schnorr => "secp256k1-schnorr",
        }
    }
}

/// The caller a key belongs to. Resolved host-side from the dispatch seam, never
/// supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The user's own identity — seta auth, Nostr, moderation.
    System,
    /// A plugin, by its manifest name (= install-dir name).
    Plugin(String),
}

/// The reserved scope for the user's identity key.
pub const SYSTEM: Scope = Scope::System;

impl Scope {
    /// A single, filesystem-safe path segment for the scope's storage dir.
    pub(crate) fn dir_segment(&self) -> Result<String, KeystoreError> {
        match self {
            Scope::System => Ok("system".to_string()),
            Scope::Plugin(id) => {
                if id.is_empty()
                    || id == "system"
                    || id.contains(['/', '\\', '\0'])
                    || id.contains("..")
                {
                    return Err(KeystoreError::InvalidScope);
                }
                Ok(id.clone())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum KeystoreError {
    InvalidScope,
    InvalidName,
    AlgorithmMismatch,
    NotFound,
    Io,
    Corrupt,
}

/// A keystore entry's public face — what a caller may see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    pub name: String,
    pub algorithm: Algorithm,
    pub public_key: Vec<u8>,
}

/// The keystore's directory name under `.moss/`. Must be listed in
/// `infra::moss_paths::moss_gitignore` — that is the whole reason moss holds
/// the bytes (see the module header). A test in `build.rs` pins the coupling.
pub const KEYS_DIRNAME: &str = "keys";

/// An entry name that is about to become one path segment, in either store.
/// Shared with [`super::secrets`] — the same name, the same rule.
pub(crate) fn validate_entry_name(name: &str) -> Result<(), KeystoreError> {
    if name.is_empty()
        || name.contains(['/', '\\', '\0'])
        || name.contains("..")
        || name.len() > 128
    {
        return Err(KeystoreError::InvalidName);
    }
    Ok(())
}

/// The keystore rooted at a project's `.moss/`.
pub struct Keystore {
    /// `<project>/.moss/keys` — under the gitignored `.moss/` tree.
    root: PathBuf,
}

impl Keystore {
    pub fn for_project(project_path: &Path) -> Self {
        Self { root: project_path.join(".moss").join(KEYS_DIRNAME) }
    }

    fn key_dir(&self, scope: &Scope) -> Result<PathBuf, KeystoreError> {
        Ok(self.root.join(scope.dir_segment()?))
    }

    fn key_path(&self, scope: &Scope, name: &str) -> Result<PathBuf, KeystoreError> {
        validate_entry_name(name)?;
        // `<name>.<algo>` — the algorithm is part of the filename so a key's
        // algorithm is fixed at creation and a mismatched request is caught.
        Ok(self.key_dir(scope)?.join(name))
    }
}

impl Keystore {
    /// Get the caller's key by name, creating it with `algorithm` if absent.
    /// Idempotent: a second call with the same name returns the same key. A
    /// call whose `algorithm` disagrees with an existing key is refused, so a
    /// key's algorithm is fixed at creation.
    pub fn get_or_create(
        &self,
        scope: &Scope,
        name: &str,
        algorithm: Algorithm,
    ) -> Result<KeyInfo, KeystoreError> {
        let path = self.key_path(scope, name)?;
        // A cloud placeholder counts as existing. On macOS 12-13 eviction
        // *replaces* the file with a hidden `.name.icloud` sibling, so a bare
        // `exists()` reads the user's key as absent and the branch below mints
        // a replacement over it — the same key-loss shape `Identity::exists`
        // guards against (moss#986).
        if path.exists() || crate::build::icloud::is_still_in_the_cloud(&path) {
            let stored = StoredKey::load(&path)?;
            if stored.algorithm != algorithm {
                return Err(KeystoreError::AlgorithmMismatch);
            }
            return Ok(stored.info(name.to_string()));
        }
        let stored = StoredKey::generate(algorithm);
        stored.save(&path)?;
        Ok(stored.info(name.to_string()))
    }

    /// Every key in the caller's scope.
    pub fn list(&self, scope: &Scope) -> Result<Vec<KeyInfo>, KeystoreError> {
        let dir = self.key_dir(scope)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(KeystoreError::Io),
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let stored = StoredKey::load(&path)?;
            out.push(stored.info(name));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Sign `payload` with the caller's named key. The signature format is the
    /// key's algorithm's own (ed25519 = raw 64 bytes; secp256k1 = BIP-340).
    pub fn sign(
        &self,
        scope: &Scope,
        name: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, KeystoreError> {
        let path = self.key_path(scope, name)?;
        // See `get_or_create`: an evicted key is not a missing key, and
        // reporting `NotFound` for one would tell a plugin its identity is gone.
        if !path.exists() && !crate::build::icloud::is_still_in_the_cloud(&path) {
            return Err(KeystoreError::NotFound);
        }
        StoredKey::load(&path)?.sign(payload)
    }
}

/// A key at rest: `<algo-wire>\n<hex-secret>\n`. The secret is the raw seed
/// (ed25519) or scalar (secp256k1) — the minimal form, never an expanded key.
struct StoredKey {
    algorithm: Algorithm,
    secret: [u8; 32],
}

impl StoredKey {
    fn generate(algorithm: Algorithm) -> Self {
        use rand::RngCore;
        let mut secret = [0u8; 32];
        match algorithm {
            Algorithm::Ed25519 => rand::rngs::OsRng.fill_bytes(&mut secret),
            Algorithm::Secp256k1Schnorr => {
                // Generate through the schnorr key so the stored scalar is
                // already BIP-340 even-Y canonical — the invariant the keyring
                // relies on (see identity/keyring.rs).
                let sk = k256::schnorr::SigningKey::random(&mut rand::rngs::OsRng);
                secret.copy_from_slice(&sk.to_bytes());
            }
        }
        Self { algorithm, secret }
    }

    /// Reads through the cloud-materialize discipline: `.moss/keys/` is
    /// gitignored but deliberately *synced* (it is how a key follows the user
    /// to a second machine), so the provider is free to evict it, and a plain
    /// read of an evicted key fails `EDEADLK` under the process-wide fail-fast
    /// policy (moss#986). Asking for it back is the only thing that ends that.
    fn load(path: &Path) -> Result<Self, KeystoreError> {
        let text = crate::build::cloud_readiness::read_to_string_with_materialize_wait(
            path,
            crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
        )
        .map_err(|_| KeystoreError::Io)?;
        let mut lines = text.lines();
        let algorithm = lines
            .next()
            .and_then(Algorithm::from_wire)
            .ok_or(KeystoreError::Corrupt)?;
        let hex_secret = lines.next().ok_or(KeystoreError::Corrupt)?;
        let bytes = hex::decode(hex_secret.trim()).map_err(|_| KeystoreError::Corrupt)?;
        let secret: [u8; 32] = bytes.try_into().map_err(|_| KeystoreError::Corrupt)?;
        Ok(Self { algorithm, secret })
    }

    fn save(&self, path: &Path) -> Result<(), KeystoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| KeystoreError::Io)?;
        }
        let body = format!("{}\n{}\n", self.algorithm.as_wire(), hex::encode(self.secret));
        write_private(path, body.as_bytes())
    }

    fn public_key(&self) -> Vec<u8> {
        match self.algorithm {
            Algorithm::Ed25519 => {
                let sk = ed25519_dalek::SigningKey::from_bytes(&self.secret);
                sk.verifying_key().to_bytes().to_vec()
            }
            Algorithm::Secp256k1Schnorr => {
                // secret is already canonical (generate) or was written by the
                // identity migration, which stores the canonical scalar.
                let sk = k256::schnorr::SigningKey::from_bytes(&self.secret)
                    .expect("stored secp256k1 scalar is valid");
                sk.verifying_key().to_bytes().to_vec()
            }
        }
    }

    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, KeystoreError> {
        match self.algorithm {
            Algorithm::Ed25519 => {
                use ed25519_dalek::Signer;
                let sk = ed25519_dalek::SigningKey::from_bytes(&self.secret);
                Ok(sk.sign(payload).to_bytes().to_vec())
            }
            Algorithm::Secp256k1Schnorr => {
                use k256::schnorr::signature::Signer;
                let sk = k256::schnorr::SigningKey::from_bytes(&self.secret)
                    .map_err(|_| KeystoreError::Corrupt)?;
                let sig: k256::schnorr::Signature =
                    sk.try_sign(payload).map_err(|_| KeystoreError::Corrupt)?;
                Ok(sig.to_bytes().to_vec())
            }
        }
    }

    fn info(&self, name: String) -> KeyInfo {
        KeyInfo { name, algorithm: self.algorithm, public_key: self.public_key() }
    }
}

/// Write `bytes` to `path` with `0600` on Unix, created atomically so there is
/// no window where the secret is world-readable. Mirrors `keypair.rs`.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // allow:raw_write identity keys live under .moss/identity/, not .moss/build.nosync/ — 0600 perms require this open, and the path is never cloud-evicted output
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| KeystoreError::Io)?;
        f.write_all(bytes).map_err(|_| KeystoreError::Io)?;
    }
    #[cfg(not(unix))]
    {
        // allow:raw_write identity keys live under .moss/identity/, not .moss/build.nosync/
        let mut f = std::fs::File::create(path).map_err(|_| KeystoreError::Io)?;
        f.write_all(bytes).map_err(|_| KeystoreError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, Keystore) {
        let tmp = TempDir::new().unwrap();
        let ks = Keystore::for_project(tmp.path());
        (tmp, ks)
    }

    #[test]
    fn get_or_create_is_idempotent() {
        let (_t, ks) = store();
        let a = ks.get_or_create(&Scope::Plugin("ipfs".into()), "ipns", Algorithm::Ed25519).unwrap();
        let b = ks.get_or_create(&Scope::Plugin("ipfs".into()), "ipns", Algorithm::Ed25519).unwrap();
        assert_eq!(a.public_key, b.public_key, "same name must return the same key");
        assert_eq!(a.algorithm, Algorithm::Ed25519);
        assert_eq!(a.public_key.len(), 32, "ed25519 public key is 32 bytes");
    }

    #[test]
    fn scopes_are_isolated() {
        let (_t, ks) = store();
        let a = ks.get_or_create(&Scope::Plugin("a".into()), "k", Algorithm::Ed25519).unwrap();
        let b = ks.get_or_create(&Scope::Plugin("b".into()), "k", Algorithm::Ed25519).unwrap();
        assert_ne!(a.public_key, b.public_key, "same name in different scopes = different keys");
        assert_eq!(ks.list(&Scope::Plugin("a".into())).unwrap().len(), 1);
        assert_eq!(ks.list(&Scope::Plugin("a".into())).unwrap()[0].name, "k");
    }

    #[test]
    fn system_scope_is_distinct_from_plugins() {
        let (_t, ks) = store();
        let sys = ks.get_or_create(&SYSTEM, "identity", Algorithm::Secp256k1Schnorr).unwrap();
        let plug = ks.get_or_create(&Scope::Plugin("system".into()), "identity", Algorithm::Ed25519);
        assert_eq!(plug, Err(KeystoreError::InvalidScope));
        assert_eq!(sys.algorithm, Algorithm::Secp256k1Schnorr);
    }

    #[test]
    fn ed25519_sign_verifies() {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let (_t, ks) = store();
        let scope = Scope::Plugin("ipfs".into());
        let key = ks.get_or_create(&scope, "ipns", Algorithm::Ed25519).unwrap();
        let msg = b"ipns record bytes";
        let sig = ks.sign(&scope, "ipns", msg).unwrap();
        assert_eq!(sig.len(), 64, "ed25519 signature is raw 64 bytes");
        let vk = VerifyingKey::from_bytes(&key.public_key.clone().try_into().unwrap()).unwrap();
        let sig = Signature::from_slice(&sig).unwrap();
        assert!(vk.verify(msg, &sig).is_ok(), "ed25519 signature must verify");
    }

    #[test]
    fn secp256k1_schnorr_sign_verifies() {
        use k256::schnorr::{signature::Verifier, Signature, VerifyingKey};
        let (_t, ks) = store();
        let key = ks.get_or_create(&SYSTEM, "identity", Algorithm::Secp256k1Schnorr).unwrap();
        let msg = b"nostr event id";
        let sig = ks.sign(&SYSTEM, "identity", msg).unwrap();
        let vk = VerifyingKey::from_bytes(&key.public_key).unwrap();
        let sig = Signature::try_from(sig.as_slice()).unwrap();
        assert!(vk.verify(msg, &sig).is_ok(), "schnorr signature must verify");
    }

    #[test]
    fn algorithm_mismatch_is_refused() {
        let (_t, ks) = store();
        let scope = Scope::Plugin("ipfs".into());
        ks.get_or_create(&scope, "k", Algorithm::Ed25519).unwrap();
        assert_eq!(
            ks.get_or_create(&scope, "k", Algorithm::Secp256k1Schnorr),
            Err(KeystoreError::AlgorithmMismatch),
        );
    }

    #[test]
    fn keys_are_stored_privately_under_moss_and_names_are_bounded() {
        let (tmp, ks) = store();
        let scope = Scope::Plugin("ipfs".into());
        ks.get_or_create(&scope, "ipns", Algorithm::Ed25519).unwrap();
        let key_file = tmp.path().join(".moss").join("keys").join("ipfs").join("ipns");
        assert!(key_file.exists(), "key must be written under .moss/keys/<scope>/<name>");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "key file must be 0600");
        }
        for bad in ["../escape", "a/b", "", &"x".repeat(200)] {
            assert!(ks.sign(&scope, bad, b"x").is_err(), "name {bad:?} must be rejected");
        }
    }
}
