//! This computer's key for a folder.
//!
//! A folder's signing key lives in the folder and travels with it, so every
//! synced copy signs as the same folder. This key does the opposite: it is
//! derived from one secret kept in this computer's local app data, which no
//! folder syncs and no profile roams, so it tells the hosting service "this computer again" from
//! "another computer holding the same folder". Each folder key gets its own,
//! so the service cannot use it to tell that two folders share a computer,
//! and it is random, so it says nothing about the hardware.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::path::Path;

const SECRET_FILE: &str = "machine-secret";
const DOMAIN: &str = "moss-machine-v1";

pub struct MachineKey(SigningKey);

impl MachineKey {
    /// This computer's key for the folder whose public key is `folder_pubkey`.
    /// `None` where there is no lasting computer to name: a CI run starts
    /// from an empty machine every time, and a system with no home directory
    /// has nowhere to keep the secret. Callers then say nothing about the
    /// computer.
    pub fn for_folder(folder_pubkey: &str) -> Option<Self> {
        // Unit tests run in CI too, against their own scratch directory.
        if !cfg!(test) && std::env::var_os("CI").is_some() {
            return None;
        }
        // Only a secret that was read is kept: a failure is tried again.
        static SECRET: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
        let secret = match SECRET.get() {
            Some(secret) => *secret,
            None => {
                let read = secret_in(&secret_root()?)?;
                *SECRET.get_or_init(|| read)
            }
        };
        Some(Self::derive(&secret, folder_pubkey))
    }

    fn derive(secret: &[u8; 32], folder_pubkey: &str) -> Self {
        let seed = Sha256::new().chain_update(DOMAIN).chain_update(secret).chain_update(folder_pubkey).finalize();
        Self(SigningKey::from_bytes(&seed.into()))
    }

    fn sign(&self, message: &str) -> String {
        hex::encode(self.0.sign(message.as_bytes()).to_bytes())
    }

    /// What a request says about this computer: its public key, the kind of
    /// computer it is, and its signature over `message`, which proves the
    /// request came from the holder of the key.
    pub fn claim(&self, message: &str) -> serde_json::Value {
        let mut claim = serde_json::json!({
            "key": hex::encode(self.0.verifying_key().to_bytes()),
            "sig": self.sign(message),
        });
        if let Some(kind) = crate::system::device_kind() {
            claim["kind"] = kind.into();
        }
        claim
    }
}

/// What the computer signs when a folder asks to sign in: the request's own
/// session, the folder's key and the address, each on its own line.
pub fn sign_in_message(session_id: &str, folder_pubkey: &str, email: &str) -> String {
    format!("{DOMAIN}\n{session_id}\n{folder_pubkey}\n{email}")
}

#[cfg(not(test))]
fn secret_root() -> Option<std::path::PathBuf> {
    crate::infra::app_data::app_data_local_dir_early()
}

/// A test never reads or makes the developer's own computer secret.
#[cfg(test)]
fn secret_root() -> Option<std::path::PathBuf> {
    Some(std::env::temp_dir().join(format!("moss-test-machine-{}", std::process::id())))
}

/// The secret in `dir`, made on first use. It is written whole to a file of
/// its own and then linked into place, which never replaces a file already
/// there: of two moss processes that both find none, one's secret becomes the
/// computer's and both read it, and no reader ever sees half a secret.
fn secret_in(dir: &Path) -> Option<[u8; 32]> {
    let path = dir.join(SECRET_FILE);
    if !path.exists() {
        std::fs::create_dir_all(dir).ok()?;
        let draft = dir.join(format!("{SECRET_FILE}.{}", hex::encode(rand::random::<[u8; 8]>())));
        let secret: [u8; 32] = rand::random();
        crate::infra::atomic_write::write_private_atomic(&draft, hex::encode(secret)).ok()?;
        // allow:hard_link no-clobber publish in local app data; the draft is removed next, so no two paths share the inode
        let _ = std::fs::hard_link(&draft, &path);
        let _ = std::fs::remove_file(&draft);
    }
    // allow:raw_read local app data, never cloud-synced
    hex::decode(std::fs::read_to_string(&path).ok()?.trim()).ok()?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    fn public(key: &MachineKey) -> String {
        key.claim("")["key"].as_str().unwrap().to_string()
    }

    #[test]
    fn the_same_computer_and_folder_always_give_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let first = secret_in(dir.path()).unwrap();
        let again = secret_in(dir.path()).unwrap();
        assert_eq!(first, again, "the secret is made once and read after");
        assert_eq!(public(&MachineKey::derive(&first, "aa")), public(&MachineKey::derive(&again, "aa")));
    }

    #[test]
    fn two_folders_on_one_computer_and_one_folder_on_two_computers_differ() {
        let (here, there) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (here, there) = (secret_in(here.path()).unwrap(), secret_in(there.path()).unwrap());
        assert_ne!(public(&MachineKey::derive(&here, "aa")), public(&MachineKey::derive(&here, "bb")));
        assert_ne!(public(&MachineKey::derive(&here, "aa")), public(&MachineKey::derive(&there, "aa")));
    }

    #[test]
    fn a_claim_verifies_against_its_own_key_and_message() {
        let claim = MachineKey::derive(&[7; 32], "aa").claim(&sign_in_message("sid", "aa", "a@example.com"));
        let bytes = |field: &str| hex::decode(claim[field].as_str().unwrap()).unwrap();
        let key = VerifyingKey::from_bytes(&bytes("key").try_into().unwrap()).unwrap();
        let sig = Signature::from_slice(&bytes("sig")).unwrap();
        key.verify(b"moss-machine-v1\nsid\naa\na@example.com", &sig).expect("signed over the sign-in message");
        assert!(key.verify(b"moss-machine-v1\nother\naa\na@example.com", &sig).is_err());
        assert_eq!(claim.get("kind").and_then(|k| k.as_str()), crate::system::device_kind());
    }

    /// The hosting service's tests verify this claim, so the derivation and
    /// the signed bytes cannot change on one side alone.
    #[test]
    fn the_fixture_the_service_verifies_is_what_this_code_makes() {
        let key = MachineKey::derive(&[7; 32], "94ebffcce4982b26421f427570c18ad6b42201a848bd8ef4ba8864411972269f");
        assert_eq!(public(&key), FIXTURE_KEY);
        assert_eq!(key.sign("moss-machine-v1\nsid\npk\na@example.com"), FIXTURE_SIG);
    }
    const FIXTURE_KEY: &str = "541e2b1af4e6ad6b4392de65c8b89d84864300efba18f4761deec2cf4fe4c24c";
    const FIXTURE_SIG: &str = "b1538db7f34790a5521d4dc884ae5ae704602e84055cb8bedc65634ae99a5e3a82875731f2faec0bbc7816b6b9d0790c66ca8592db1553923432e0df397d3e0d";

    #[test]
    fn a_test_run_keeps_off_the_real_secret() {
        let real = crate::infra::app_data::app_data_local_dir_early().unwrap();
        assert!(!secret_root().unwrap().starts_with(&real));
        assert!(MachineKey::for_folder("aa").is_some());
        assert!(secret_root().unwrap().join(SECRET_FILE).exists());
    }

    #[test]
    fn processes_that_start_together_end_with_one_secret() {
        let dir = tempfile::tempdir().unwrap();
        let found: Vec<_> = std::thread::scope(|scope| {
            let starts: Vec<_> = (0..16).map(|_| scope.spawn(|| secret_in(dir.path()))).collect();
            starts.into_iter().map(|start| start.join().unwrap().expect("each reads a whole secret")).collect()
        });
        assert!(found.iter().all(|secret| *secret == found[0]));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no draft is left behind");
    }

    #[cfg(unix)]
    #[test]
    fn the_secret_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        secret_in(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join(SECRET_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
