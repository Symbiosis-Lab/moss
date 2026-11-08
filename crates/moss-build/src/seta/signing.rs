//! Request signing for moss-seta authentication: BIP-340 Schnorr signatures
//! (Nostr-compatible) over each request.
//!
//! REFACTOR-LATER (keystore): this signs with the identity key directly. The
//! identity key is one entry in the keystore at the `System` scope
//! (`identity::keystore`), so this should become a keystore caller, unifying
//! every signing path, as a change with no behavior difference.

use std::time::{SystemTime, UNIX_EPOCH};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::identity::keypair::{Identity, IdentityError};

/// The v2 message: every part of the request on its own line, so no part
/// can be shifted into another. The server rebuilds it from the request it
/// receives, so a header signed for one request fails on any other, and the
/// server refuses a nonce it has seen, so the header works once.
pub fn message_v2(timestamp: u64, nonce: &str, method: &str, path_and_query: &str, body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let body_sha256 = hex::encode(Sha256::digest(body));
    format!("moss-seta-v2\n{timestamp}\n{nonce}\n{}\n{path_and_query}\n{body_sha256}", method.to_ascii_uppercase())
}

/// The `Authorization` header for one request, signed over its method, its
/// path and query exactly as sent, and its body bytes.
pub fn sign_request_v2(identity: &Identity, method: &str, path_and_query: &str, body: &[u8]) -> Result<String, IdentityError> {
    use k256::schnorr::signature::Signer;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_secs();
    let nonce = hex::encode(rand::random::<[u8; 16]>());
    let message = message_v2(timestamp, &nonce, method, path_and_query, body);
    let signature = identity.signing_key_loaded()?.sign(message.as_bytes());
    let header = serde_json::json!({
        "v": 2,
        "pubkey": identity.pubkey,
        "signature": hex::encode(signature.to_bytes()),
        "timestamp": timestamp,
        "nonce": nonce,
    });
    Ok(format!("Nostr {}", BASE64.encode(header.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_message_has_each_part_on_its_own_line() {
        let m = message_v2(1700000000, "00112233445566778899aabbccddeeff", "post", "/api/sites/a?x=1", b"{\"id\":\"a\"}");
        assert_eq!(
            m,
            "moss-seta-v2\n1700000000\n00112233445566778899aabbccddeeff\nPOST\n/api/sites/a?x=1\n\
             8489a5deb454a360345c7868bca8672de92b446caf3d3b014af6a56e3d549d30"
        );
    }

    /// The server's own tests verify this header, made by this signer for
    /// fixed inputs. It must still verify against `message_v2` here, so a
    /// change of message format on either side fails a test.
    #[test]
    fn the_servers_fixture_header_verifies_against_this_message() {
        use sha2::{Digest, Sha256};
        const HEADER: &str = "eyJub25jZSI6IjAwMTEyMjMzNDQ1NTY2Nzc4ODk5YWFiYmNjZGRlZWZmIiwicHVia2V5IjoiOTRlYmZmY2NlNDk4MmIyNjQyMWY0Mjc1NzBjMThhZDZiNDIyMDFhODQ4YmQ4ZWY0YmE4ODY0NDExOTcyMjY5ZiIsInNpZ25hdHVyZSI6ImQxZTg0N2QxOWU2NTNhNjdmNTdlYmE1MGY2YmVmMmM0ZmQ1NjhiNDZhNWMwMDViY2E4ZWU1MjM2ZTYwNGJiZjM2MmU5MjE4Y2NmY2RiMTIxZGZhMTgwNzA0NmEyODc2MDg4YTkyM2M1MWYzNTg5NDQyOTJiODMzYTViZWQ5NTVkIiwidGltZXN0YW1wIjoxNzAwMDAwMDAwLCJ2IjoyfQ==";
        let h: serde_json::Value = serde_json::from_slice(&BASE64.decode(HEADER).unwrap()).unwrap();
        assert_eq!((h["v"].as_u64(), h["timestamp"].as_u64()), (Some(2), Some(1700000000)));
        let message = message_v2(1700000000, h["nonce"].as_str().unwrap(), "POST", "/api/sites/a?x=1", b"{\"id\":\"a\"}");
        let key = k256::schnorr::VerifyingKey::from_slice(&hex::decode(h["pubkey"].as_str().unwrap()).unwrap()).unwrap();
        let signature = k256::schnorr::Signature::try_from(hex::decode(h["signature"].as_str().unwrap()).unwrap().as_slice()).unwrap();
        key.verify_raw(&Sha256::digest(message.as_bytes()), &signature).expect("the fixture signs this message");
    }
}
