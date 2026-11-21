//! The keystore: keys as a platform primitive.
//!
//! moss holds key bytes; callers use keys. A caller asks for a key by name,
//! signs with it, and lists its keys — it never receives private bytes. Every
//! name resolves inside the *caller's own* scope, so one caller cannot name or
//! reach another's key. The scope is structural: it comes from who is calling
//! (the plugin dispatch seam), never a parameter, so there is nothing to forge.
//!
//! Every entry belongs to a plugin. The folder's own identity key is not here:
//! it lives in `.moss/identity/` (`identity::keypair`), and moss signs with it
//! directly.
//!
//! Custody, not restriction: moss holds the bytes because
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

/// The plugin a key or secret belongs to, by its manifest name (= install-dir
/// name). Resolved host-side from the dispatch seam, never supplied by the
/// caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope(String);

impl Scope {
    /// The id names the scope's directory, by the same rule as an entry name.
    /// Install names are already safe file names, so no older directory moves.
    pub fn plugin(id: &str) -> Result<Self, KeystoreError> {
        EntryName::new(id).map(|e| Self(e.file)).map_err(|_| KeystoreError::InvalidScope)
    }

    pub(crate) fn dir_segment(&self) -> &str {
        &self.0
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

/// A name a plugin chose (scope, key, secret, channel) and the one file name
/// it is stored under, in every store. A name is any string up to 128 bytes
/// except one holding `/`, `\`, NUL or `..` (the trust boundary) or a Windows
/// device name, whose stored form fits in 200 bytes. The file name percent-encodes every byte outside
/// `[A-Za-z0-9._-]` and a trailing `.`, so it is legal on NTFS and the same
/// on every OS: `.moss/keys` syncs between machines.
pub(crate) struct EntryName {
    pub(crate) file: String,
    /// The name as macOS and Linux stored it before encoding, when that differs.
    legacy: Option<String>,
}

const MAX_NAME_BYTES: usize = 128;
/// Leaves `.<file>.pending.<pid>.<seq>` (the atomic write's temp name) under
/// the 255-byte limit.
const MAX_FILE_BYTES: usize = 200;

impl EntryName {
    pub(crate) fn new(name: &str) -> Result<Self, KeystoreError> {
        if name.is_empty()
            || name.contains(['/', '\\', '\0'])
            || name.contains("..")
            || name.len() > MAX_NAME_BYTES
            || is_device_name(name)
        {
            return Err(KeystoreError::InvalidName);
        }
        let mut file = String::with_capacity(name.len());
        for &b in name.as_bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
                file.push(b as char);
            } else {
                file.push_str(&format!("%{b:02X}"));
            }
        }
        if file.ends_with('.') {
            file.pop();
            file.push_str("%2E");
        }
        if file.len() > MAX_FILE_BYTES {
            return Err(KeystoreError::InvalidName);
        }
        // A raw name holding `%` could be another name's encoding, so it is
        // never read as legacy; no stored name has one.
        let legacy = (file != name && !name.contains('%')).then(|| name.to_string());
        Ok(Self { file, legacy })
    }

    /// The entry's path in `dir`, renaming a file stored under the raw name
    /// (macOS and Linux) to the encoded one first; `present` is the caller's
    /// existence check, so a store that counts a cloud placeholder as present
    /// does here too. A rename that fails (an evicted placeholder) reads the
    /// old file in place. Windows never opens a raw name: `a:b` there names a
    /// stream of the file `a`.
    pub(crate) fn path_in(&self, dir: &Path, present: impl Fn(&Path) -> bool) -> PathBuf {
        let path = dir.join(&self.file);
        if let Some(old) = self.legacy_in(dir) {
            if !present(&path) && present(&old) {
                return if std::fs::rename(&old, &path).is_ok() { path } else { old };
            }
        }
        path
    }

    /// The raw-named file a store must also clear when it forgets the entry.
    pub(crate) fn legacy_in(&self, dir: &Path) -> Option<PathBuf> {
        if cfg!(unix) {
            self.legacy.as_ref().map(|raw| dir.join(raw))
        } else {
            None
        }
    }

    /// The name a stored file name stands for; a legacy raw name is itself.
    pub(crate) fn decode(file: &str) -> String {
        let bytes = file.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let escaped = (bytes[i] == b'%')
                .then(|| bytes.get(i + 1..i + 3))
                .flatten()
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            match escaped {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        String::from_utf8(out).unwrap_or_else(|_| file.to_string())
    }
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, in any case and
/// with any extension: Windows opens the device instead of a file.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s => {
            s.len() == 4
                && (s.starts_with("COM") || s.starts_with("LPT"))
                && s.as_bytes()[3].is_ascii_digit()
        }
    }
}

fn key_present(path: &Path) -> bool {
    path.exists() || crate::build::icloud::is_still_in_the_cloud(path)
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

    fn key_dir(&self, scope: &Scope) -> PathBuf {
        self.root.join(scope.dir_segment())
    }

    fn key_path(&self, scope: &Scope, name: &str) -> Result<PathBuf, KeystoreError> {
        Ok(EntryName::new(name)?.path_in(&self.key_dir(scope), key_present))
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
        // guards against (the identity-file bug).
        if key_present(&path) {
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
        let dir = self.key_dir(scope);
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
            let name = EntryName::decode(&entry.file_name().to_string_lossy());
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
        if !key_present(&path) {
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
                let sk = <k256::schnorr::SigningKey as k256::elliptic_curve::Generate>::generate();
                secret.copy_from_slice(&sk.to_bytes());
            }
        }
        Self { algorithm, secret }
    }

    /// Reads through the cloud-materialize discipline: `.moss/keys/` is
    /// gitignored but deliberately *synced* (it is how a key follows the user
    /// to a second machine), so the provider is free to evict it, and a plain
    /// read of an evicted key fails `EDEADLK` under the process-wide fail-fast
    /// policy. Asking for it back is the only thing that ends that.
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
        let body = format!("{}\n{}\n", self.algorithm.as_wire(), hex::encode(self.secret));
        crate::infra::atomic_write::write_private_atomic(path, body).map_err(|_| KeystoreError::Io)
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
                let sk = k256::schnorr::SigningKey::from_bytes(&self.secret.into())
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
                let sk = k256::schnorr::SigningKey::from_bytes(&self.secret.into())
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
        let a = ks.get_or_create(&Scope::plugin("ipfs").unwrap(), "ipns", Algorithm::Ed25519).unwrap();
        let b = ks.get_or_create(&Scope::plugin("ipfs").unwrap(), "ipns", Algorithm::Ed25519).unwrap();
        assert_eq!(a.public_key, b.public_key, "same name must return the same key");
        assert_eq!(a.algorithm, Algorithm::Ed25519);
        assert_eq!(a.public_key.len(), 32, "ed25519 public key is 32 bytes");
    }

    #[test]
    fn scopes_are_isolated() {
        let (_t, ks) = store();
        let a = ks.get_or_create(&Scope::plugin("a").unwrap(), "k", Algorithm::Ed25519).unwrap();
        let b = ks.get_or_create(&Scope::plugin("b").unwrap(), "k", Algorithm::Ed25519).unwrap();
        assert_ne!(a.public_key, b.public_key, "same name in different scopes = different keys");
        assert_eq!(ks.list(&Scope::plugin("a").unwrap()).unwrap().len(), 1);
        assert_eq!(ks.list(&Scope::plugin("a").unwrap()).unwrap()[0].name, "k");
    }

    #[test]
    fn a_scope_is_one_path_segment() {
        for id in ["", "../ipfs", "a/b", "a\\b", "a\0b"] {
            assert_eq!(Scope::plugin(id), Err(KeystoreError::InvalidScope), "{id:?}");
        }
        assert!(Scope::plugin("ipfs").is_ok());
    }

    #[test]
    fn ed25519_sign_verifies() {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let (_t, ks) = store();
        let scope = Scope::plugin("ipfs").unwrap();
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
        let scope = Scope::plugin("nostr").unwrap();
        let key = ks.get_or_create(&scope, "identity", Algorithm::Secp256k1Schnorr).unwrap();
        let msg = b"nostr event id";
        let sig = ks.sign(&scope, "identity", msg).unwrap();
        let pk: [u8; 32] = key.public_key.as_slice().try_into().unwrap();
        let vk = VerifyingKey::from_bytes(&pk.into()).unwrap();
        let sig = Signature::try_from(sig.as_slice()).unwrap();
        assert!(vk.verify(msg, &sig).is_ok(), "schnorr signature must verify");
    }

    #[test]
    fn algorithm_mismatch_is_refused() {
        let (_t, ks) = store();
        let scope = Scope::plugin("ipfs").unwrap();
        ks.get_or_create(&scope, "k", Algorithm::Ed25519).unwrap();
        assert_eq!(
            ks.get_or_create(&scope, "k", Algorithm::Secp256k1Schnorr),
            Err(KeystoreError::AlgorithmMismatch),
        );
    }

    #[test]
    fn list_names_a_key_as_it_was_created() {
        let (_t, ks) = store();
        let scope = Scope::plugin("p").unwrap();
        let made = ks.get_or_create(&scope, "a:b", Algorithm::Ed25519).unwrap();
        let listed = ks.list(&scope).unwrap();
        assert_eq!(listed, [made]);
    }

    /// Minting a fresh key over one stored under its raw name would lose the
    /// user's key.
    #[cfg(unix)]
    #[test]
    fn a_key_stored_under_its_raw_name_is_the_same_key() {
        let (tmp, ks) = store();
        let scope = Scope::plugin("p").unwrap();
        let made = ks.get_or_create(&scope, "id:alice", Algorithm::Ed25519).unwrap();
        let dir = tmp.path().join(".moss").join("keys").join("p");
        std::fs::rename(dir.join("id%3Aalice"), dir.join("id:alice")).unwrap();
        let again = ks.get_or_create(&scope, "id:alice", Algorithm::Ed25519).unwrap();
        assert_eq!(again.public_key, made.public_key);
    }

    #[test]
    fn keys_are_stored_privately_under_moss_and_names_are_bounded() {
        let (tmp, ks) = store();
        let scope = Scope::plugin("ipfs").unwrap();
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
