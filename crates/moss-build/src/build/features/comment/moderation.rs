//! Owner-signed comment moderation events.
//!
//! The owner's BIP-340 Schnorr identity signs hide/unhide/pin/unpin events
//! appended to `moderation.jsonl`; the reducer (`reduce.rs`) folds them into the
//! visible comment view at build time (and, later, on the client). Authority is
//! the signature — not a replica tombstone — so a hide never resurrects on sync.
//!
//! Lean by construction: signing reuses the existing `IdentityService`/`signing.rs`
//! BIP-340 primitives; the log uses the generic `event_log` JSONL substrate.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::event_log;
use crate::moss_paths::MossPaths;

/// A single owner-signed moderation action. The schema is LOCKED (events are
/// signed + append-only): `source` qualifies `target` across comment sources;
/// `seq` is the per-(site,pubkey) monotonic PRIMARY ordering key (clock-skew
/// proof); `ts` (ms-epoch) is only a tiebreak. v1 acts on hide/unhide;
/// pin/unpin are reserved in the schema but a no-op in the reducer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModEvent {
    pub kind: String,
    pub target: String,
    pub source: String,
    pub site: String,
    pub page: String,
    pub seq: u64,
    pub ts: u64,
    pub pubkey: String,
    pub sig: String,
}

impl ModEvent {
    /// Canonical signed message — a fixed-format string so Rust (build) and TS
    /// (client) reproduce identical signed bytes WITHOUT JSON canonicalization.
    pub fn signing_message(
        kind: &str,
        source: &str,
        target: &str,
        site: &str,
        page: &str,
        seq: u64,
        ts: u64,
    ) -> String {
        format!("mod:v1:{kind}:{source}:{target}:{site}:{page}:{seq}:{ts}")
    }

    fn message(&self) -> String {
        Self::signing_message(
            &self.kind, &self.source, &self.target, &self.site, &self.page, self.seq, self.ts,
        )
    }

    /// Verify against the expected owner pubkey (must match AND the Schnorr
    /// signature must check out over the canonical message).
    pub fn verify(&self, owner_pubkey: &str) -> bool {
        self.pubkey == owner_pubkey
            && verify_schnorr(&self.pubkey, self.message().as_bytes(), &self.sig)
    }
}

/// Verify a BIP-340 Schnorr signature over `message` against an x-only public
/// key (64 hex chars) and a signature (128 hex chars). False on any decode or
/// verification failure; never panics.
///
/// Verification lives here, with the reducer that needs it, while *signing*
/// lives app-side with the key — see [`sign_mod_event`]. That is not a
/// symmetry we broke for tidiness: signing needs the owner's private key,
/// which only the app's keyring can produce, whereas the bake needs to check
/// a signature on every build with nothing but the public key that is already
/// written into each event. If a second consumer of this ever appears, hoist
/// it rather than copying it.
pub fn verify_schnorr(pubkey_hex: &str, message: &[u8], sig_hex: &str) -> bool {
    use k256::schnorr::{signature::Verifier, Signature, VerifyingKey};

    let Ok(pk_bytes) = hex::decode(pubkey_hex) else { return false };
    let Ok(vk) = VerifyingKey::from_bytes(&pk_bytes) else { return false };
    let Ok(sig_bytes) = hex::decode(sig_hex) else { return false };
    let Ok(sig) = Signature::try_from(sig_bytes.as_slice()) else { return false };
    vk.verify(message, &sig).is_ok()
}

/// Build + sign a moderation event.
///
/// Takes the signing key and the public key it belongs to, NOT the app's
/// `Identity`: producing a loaded key is the caller's job — it means touching
/// the keyring, which the build must not do — and this function's own job is
/// only to lay out the canonical message, derive the public key from the
/// private one, and sign. That is also what makes
/// it infallible; the one failure the old signature carried was
/// `signing_key_loaded()`, which now happens (and is reported) where the key
/// is obtained.
///
/// REFACTOR-LATER (keystore): like seta auth, this signs with the
/// identity key directly. It should become a keystore caller at the `System`
/// scope (`identity::keystore`) so all signing goes through one path. Deferred
/// with the same rationale as `seta::signing::sign_request` — a separate,
/// behavior-preserving change for when this code is next touched.
#[allow(clippy::too_many_arguments)]
pub fn sign_mod_event(
    signing_key: &k256::schnorr::SigningKey,
    kind: &str,
    source: &str,
    target: &str,
    site: &str,
    page: &str,
    seq: u64,
    ts: u64,
) -> ModEvent {
    use k256::schnorr::signature::Signer;
    // The pubkey is DERIVED from the key that signs, not passed alongside it.
    // Taking both would make the pairing a convention: a caller that handed
    // over a mismatched pubkey would produce an event `ModEvent::verify`
    // rejects (it compares `self.pubkey` to the site owner's), so moderation
    // would simply stop applying — no error, nowhere. Same bytes as before;
    // `Identity::generate` builds its `pubkey` field this exact way.
    let pubkey = hex::encode(signing_key.verifying_key().to_bytes());
    let msg = ModEvent::signing_message(kind, source, target, site, page, seq, ts);
    let sig = signing_key.sign(msg.as_bytes());
    ModEvent {
        kind: kind.into(),
        target: target.into(),
        source: source.into(),
        site: site.into(),
        page: page.into(),
        seq,
        ts,
        pubkey,
        sig: hex::encode(sig.to_bytes()),
    }
}

/// Path to the signed moderation log for a project.
pub fn moderation_log_path(project_path: &str) -> std::path::PathBuf {
    MossPaths::new(Path::new(project_path))
        .social_dir()
        .join("moderation.jsonl")
}

/// Load all moderation events for a project (skips corrupt lines).
pub fn load_mod_events(project_path: &str) -> Vec<ModEvent> {
    event_log::read_jsonl(&moderation_log_path(project_path))
}

/// Per-(site,pubkey) monotonic counter persisted next to the log, so a new event
/// always sorts after every prior one on this device — the reducer's primary
/// order key, immune to wall-clock skew. Atomic tmp+rename write.
pub fn next_seq(project_path: &str) -> std::io::Result<u64> {
    let path = MossPaths::new(Path::new(project_path))
        .social_dir()
        .join("mod-seq");
    let cur = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let next = cur + 1;
    if let Some(parent) = path.parent() {
        // allow:raw_write .moss/data, not the build tree
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("seq-tmp");
    std::fs::write(&tmp, next.to_string())?;  // allow:raw_write the temp for this file's own atomic save under .moss/data
    // allow:unlink rename into place in the moderation store, not staging
    std::fs::rename(&tmp, &path)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::schnorr::SigningKey;

    /// A throwaway owner key. Deliberately k256's own generator rather than
    /// `identity::keypair::Identity`: this module no longer knows the app's
    /// keyring exists, and a test that reached for it would quietly restore
    /// the dependency the module was freed of.
    fn owner_key() -> (SigningKey, String) {
        let sk = SigningKey::random(&mut rand::rngs::OsRng);
        let pubkey = hex::encode(sk.verifying_key().to_bytes());
        (sk, pubkey)
    }

    #[test]
    fn signing_message_is_exact() {
        assert_eq!(
            ModEvent::signing_message("hide", "artalk", "23", "guo", "cc4f602b", 7, 1718280000000),
            "mod:v1:hide:artalk:23:guo:cc4f602b:7:1718280000000"
        );
    }

    #[test]
    fn sign_then_verify_roundtrip() {
        let (sk, pubkey) = owner_key();
        let ev = sign_mod_event(
            &sk, "hide", "artalk", "23", "guo", "cc4f602b", 7, 1718280000000,
        );
        assert!(ev.verify(&pubkey), "valid event verifies");

        let mut tampered = ev.clone();
        tampered.target = "24".into();
        assert!(!tampered.verify(&pubkey), "tampered field fails verification");

        let (_, other) = owner_key();
        assert!(!ev.verify(&other), "foreign pubkey fails");
    }

    /// `verify_schnorr`'s own failure modes, which `ModEvent::verify` masks
    /// behind its pubkey equality check. It moved here from what was then
    /// `identity::signing` (now `seta::signing`) with its production caller.
    #[test]
    fn a_malformed_signature_fails_rather_than_panicking() {
        let (sk, pubkey) = owner_key();
        use k256::schnorr::signature::Signer;
        let msg = b"mod:v1:hide:artalk:23:guo:cc4f602b:7:1718280000000";
        let sig_hex = hex::encode(sk.sign(msg).to_bytes());
        assert!(verify_schnorr(&pubkey, msg, &sig_hex), "valid sig must verify");
        assert!(!verify_schnorr(&pubkey, b"tampered", &sig_hex), "wrong msg fails");
        assert!(!verify_schnorr(&pubkey, msg, "zz"), "garbage sig fails");
        assert!(!verify_schnorr("not-hex", msg, &sig_hex), "garbage pubkey fails");
    }
}
