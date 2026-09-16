//! Request signing for moss-seta authentication.
//!
//! Creates signed requests that moss-seta can verify using
//! BIP-340 Schnorr signatures (Nostr-compatible).

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::identity::keypair::{Identity, IdentityError};

/// Signed request format for moss-seta authentication.
///
/// Sent in Authorization header as: `Nostr <base64(SignedRequest)>`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRequest {
    /// Signer's BIP-340 x-only public key (64 hex chars)
    pub pubkey: String,
    /// BIP-340 Schnorr signature (64 bytes = 128 hex chars)
    pub signature: String,
    /// Unix timestamp in seconds
    pub timestamp: u64,
    /// Request identifier (typically endpoint path)
    pub payload: String,
}

impl SignedRequest {
    /// Create the Authorization header value.
    pub fn to_auth_header(&self) -> String {
        let json = serde_json::to_string(self).unwrap();
        let encoded = BASE64.encode(&json);
        format!("Nostr {}", encoded)
    }
}

/// Sign a request payload with the user's identity using BIP-340 Schnorr.
///
/// # Arguments
/// * `identity` - User's identity with private key
/// * `payload` - Request identifier (usually the API endpoint path)
///
/// # Returns
/// A `SignedRequest` that can be used in the Authorization header.
///
/// # Protocol
/// The message format is "{timestamp}:{payload}". Both the Rust k256 library
/// and TypeScript @noble/curves library implement BIP-340 with internal tagged
/// hashing, so we sign the raw message bytes and the server verifies the same.
///
/// REFACTOR-LATER (ADR-031 keystore): this signs with the identity key directly.
/// The identity key is now one entry in the keystore at the `System` scope
/// (`identity::keystore`), so this should become a keystore caller —
/// `keystore.sign(&System, "identity", ...)` — unifying every signing path. Left
/// as-is deliberately: the keystore shipped with plugin keys as its first new
/// caller; rewiring this working seta-auth path is a separate change with no
/// behavior difference, made when there is a reason to touch it.
pub fn sign_request(identity: &Identity, payload: &str) -> Result<SignedRequest, IdentityError> {
    use k256::schnorr::signature::Signer;

    let signing_key = identity.signing_key_loaded()?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    // Message format: "{timestamp}:{payload}"
    let message_str = format!("{}:{}", timestamp, payload);

    // Sign the raw message bytes. k256's Schnorr implementation handles
    // BIP-340 tagged hashing internally (taggedHash('BIP0340/challenge', ...))
    let signature = signing_key.sign(message_str.as_bytes());

    Ok(SignedRequest {
        pubkey: identity.pubkey.clone(),
        signature: hex::encode(signature.to_bytes()),
        timestamp,
        payload: payload.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_request() {
        let identity = Identity::generate().unwrap();
        let signed = sign_request(&identity, "/api/test").unwrap();

        assert_eq!(signed.pubkey, identity.pubkey);
        assert!(!signed.signature.is_empty());
        // BIP-340 signature is 64 bytes = 128 hex chars
        assert_eq!(signed.signature.len(), 128);
        assert_eq!(signed.payload, "/api/test");
        assert!(signed.timestamp > 0);
    }

    #[test]
    fn test_auth_header_format() {
        let identity = Identity::generate().unwrap();
        let signed = sign_request(&identity, "/api/test").unwrap();
        let header = signed.to_auth_header();

        assert!(header.starts_with("Nostr "));
    }
}
