//! minisign verification, for the registry's documents and for the desktop
//! app `moss desktop install` downloads. No I/O: bytes, a `.minisig` file's
//! text and public keys in, a verdict out.
//!
//! The registry signs `index.json` and `revoked.json` with one Ed25519 key and
//! publishes each signature beside its document as `<name>.minisig`. A build
//! that pins keys accepts a document only with a valid signature by one of
//! them; there is no unsigned path once a key is pinned. The list holds the
//! signing key and a backup made with it and kept offline, so a lost or
//! stolen key is replaced the same day without waiting for a release.

use minisign_verify::{Error as MinisignError, PublicKey, Signature};

/// The registry's public keys, as the base64 line of a minisign public key
/// file. Empty until the registry's keys exist, and a build that reads the
/// registry's own hostname refuses to compile while it is (see `fetch`).
pub const REGISTRY_KEYS: &[&str] = &[];

/// Why a signature was not accepted. Every variant refuses the document; they
/// differ only in what the log says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// The `.minisig` text is not a minisign signature.
    Malformed,
    /// Signed, but by a key this build does not pin.
    UnknownKey,
    /// A pinned key's id, but the bytes do not verify, or the signature is the
    /// legacy non-prehashed kind minisign itself no longer makes by default.
    Invalid,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SignatureError::Malformed => "the signature file is not a minisign signature",
            SignatureError::UnknownKey => "it is signed by a key this build does not trust",
            SignatureError::Invalid => "its signature does not match its bytes",
        })
    }
}

/// Whether `bytes` carry a valid prehashed minisign signature by one of
/// `keys`. `signature` is the text of a `.minisig` file; a key is either the
/// text of a minisign public key file or its bare base64 line. A key that does
/// not parse is skipped, so it can never be the one that matched.
pub fn verify(bytes: &[u8], signature: &str, keys: &[&str]) -> Result<(), SignatureError> {
    let signature = Signature::decode(signature).map_err(|_| SignatureError::Malformed)?;
    let mut verdict = SignatureError::UnknownKey;
    for key in keys.iter().filter_map(|k| parse_key(k)) {
        match key.verify(bytes, &signature, false) {
            Ok(()) => return Ok(()),
            Err(MinisignError::UnexpectedKeyId) => {}
            Err(_) => verdict = SignatureError::Invalid,
        }
    }
    Err(verdict)
}

fn parse_key(key: &str) -> Option<PublicKey> {
    let key = key.trim();
    if key.contains('\n') {
        PublicKey::decode(key).ok()
    } else {
        PublicKey::from_base64(key).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test keys only: made for these fixtures with `minisign -G -W` (Ubuntu's
    // minisign 0.11, the package the registry's signing job installs) and
    // used to sign nothing else. Key A signed every fixture; key B signed only
    // `index-28.json.b.minisig`.
    const TEST_KEY_A: &str = "RWSwHWgPw9OoEjHpFFM3ug+PbwYqHf4y1Be5bRmZJ/5v3lDabkeBQWdu";
    const TEST_KEY_B_FILE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/registry-signing/b.pub"));
    const INDEX_28: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/registry-signing/index-28.json"));
    const INDEX_28_SIG_A: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/registry-signing/index-28.json.minisig"));
    const INDEX_28_SIG_B: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/registry-signing/index-28.json.b.minisig"));
    const INDEX_28_SIG_LEGACY: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/registry-signing/index-28.json.legacy.minisig"));

    #[test]
    fn a_document_signed_by_the_minisign_cli_verifies() {
        assert_eq!(verify(INDEX_28.as_bytes(), INDEX_28_SIG_A, &[TEST_KEY_A]), Ok(()));
    }

    #[test]
    fn a_key_given_as_a_whole_public_key_file_verifies_too() {
        assert_eq!(verify(INDEX_28.as_bytes(), INDEX_28_SIG_B, &[TEST_KEY_B_FILE]), Ok(()));
    }

    #[test]
    fn the_backup_key_is_accepted_beside_the_signing_key() {
        assert_eq!(
            verify(INDEX_28.as_bytes(), INDEX_28_SIG_B, &[TEST_KEY_A, TEST_KEY_B_FILE]),
            Ok(())
        );
    }

    #[test]
    fn a_signature_by_a_key_not_pinned_is_refused() {
        assert_eq!(
            verify(INDEX_28.as_bytes(), INDEX_28_SIG_B, &[TEST_KEY_A]),
            Err(SignatureError::UnknownKey)
        );
    }

    #[test]
    fn one_changed_byte_is_refused() {
        let tampered = INDEX_28.replace("\"serial\":28", "\"serial\":99");
        assert_eq!(
            verify(tampered.as_bytes(), INDEX_28_SIG_A, &[TEST_KEY_A]),
            Err(SignatureError::Invalid)
        );
    }

    #[test]
    fn a_legacy_signature_by_a_pinned_key_is_refused() {
        // `minisign -l`: signs the bytes themselves instead of their hash.
        assert_eq!(
            verify(INDEX_28.as_bytes(), INDEX_28_SIG_LEGACY, &[TEST_KEY_A]),
            Err(SignatureError::Invalid)
        );
    }

    #[test]
    fn something_that_is_not_a_signature_is_refused() {
        assert_eq!(
            verify(INDEX_28.as_bytes(), "<!doctype html>", &[TEST_KEY_A]),
            Err(SignatureError::Malformed)
        );
    }

    #[test]
    fn no_pinned_key_accepts_nothing() {
        assert_eq!(
            verify(INDEX_28.as_bytes(), INDEX_28_SIG_A, &[]),
            Err(SignatureError::UnknownKey)
        );
    }

    #[test]
    fn every_pinned_registry_key_parses() {
        // A pinned key that does not parse is skipped by `verify`, so a typo
        // in the list would silently leave the registry with one key fewer.
        assert!(REGISTRY_KEYS.iter().all(|k| parse_key(k).is_some()));
    }
}
