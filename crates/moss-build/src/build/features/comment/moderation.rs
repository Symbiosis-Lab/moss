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
/// Verification lives here, with the reducer that needs it, while the key that
/// *signs* is loaded by [`append_owner_event`]: the bake needs to check a
/// signature on every build with nothing but the public key that is already
/// written into each event, and never touches the private key. If a second
/// consumer of this ever appears, hoist it rather than copying it.
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
/// Takes the signing key, NOT an `Identity`: producing a loaded key is the
/// caller's job (it means reading `.moss/identity/secret-key`, which the build
/// must not do) and this function's own job is only to lay out the canonical
/// message, derive the public key from the private one, and sign. That is also
/// what makes it infallible; the one failure the old signature carried was
/// `signing_key_loaded()`, which now happens (and is reported) where the key
/// is obtained — [`append_owner_event`] for the owner's key.
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

/// The `site` value a folder's moderation events carry: its site id, or
/// `"localhost"` before the first publish. The build reads the same value for
/// the comment section's site name, and the command-line moderation commands
/// sign with it, so the two cannot disagree.
pub fn site_value(site_id: Option<&str>) -> &str {
    site_id.unwrap_or("localhost")
}

/// The moderation actions the build's reducer acts on. The wire format still
/// carries `kind` as a string (and reserves `pin`/`unpin`, which the reducer
/// ignores), so this enum is the only way to ask for an event nothing would
/// ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Hide,
    Unhide,
}

impl ModKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ModKind::Hide => "hide",
            ModKind::Unhide => "unhide",
        }
    }
}

/// Sign and append a moderation event as the site's owner; returns its
/// sequence number.
///
/// One exclusive lock (a machine-local file under the moss home's `locks/`, named for this folder and `moderation`, never inside the synced site) is held while the
/// sequence number is worked out, the event signed and appended and the counter
/// written, so two writers on one machine (this command and the app, or two
/// commands) cannot interleave and sign the same number. It cannot cover two
/// machines writing through a synced folder: there each machine sees only the
/// files that have arrived, a tie is possible, and the reducer breaks it by
/// timestamp.
///
/// Uses the key already on disk in `.moss/identity` and never creates one: a
/// new key would not be the one the site's earlier events were signed with, so
/// every earlier hide would stop applying. With no usable key it fails with a
/// plain message and writes nothing.
pub fn append_owner_event(
    project_path: &str,
    kind: ModKind,
    source: &str,
    target: &str,
    page: &str,
    site: &str,
) -> Result<u64, String> {
    let root = Path::new(project_path);
    let identity = crate::identity::keypair::Identity::load_for_signing(root).map_err(|e| {
        format!(
            "this site has no usable signing key ({e}); moderation needs the site's own key in .moss/identity and moss will not create a new one"
        )
    })?;
    let signing_key = identity.signing_key_loaded().map_err(|e| e.to_string())?;
    let derived = hex::encode(signing_key.verifying_key().to_bytes());
    if derived != identity.pubkey {
        return Err("the signing key in .moss/identity does not match the site's public key; refusing to sign an event the build would ignore".into());
    }
    let _lock = crate::infra::folder_lock::acquire_named(root, "moderation")?;
    let seq = next_seq(project_path).map_err(|e| format!("could not advance the moderation counter: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let event = sign_mod_event(signing_key, kind.as_str(), source, target, site, page, seq, ts);
    event_log::append_jsonl(&moderation_log_path(project_path), &event)
        .map_err(|e| format!("could not write the moderation log: {e}"))?;
    Ok(seq)
}

/// The next sequence number: one more than the larger of the counter file
/// (`mod-seq`, beside the log) and the highest `seq` already in the log. The
/// counter alone is not enough — it and the log are separate files and a
/// cloud-synced folder can deliver the log first, leaving the counter behind
/// it, so a number an older event already used would be handed out again and
/// the new event could lose to the old one. Atomic tmp+rename write.
fn next_seq(project_path: &str) -> std::io::Result<u64> {
    let path = MossPaths::new(Path::new(project_path))
        .social_dir()
        .join("mod-seq");
    let counter = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let logged = load_mod_events(project_path).iter().map(|e| e.seq).max().unwrap_or(0);
    let next = counter.max(logged) + 1;
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

    /// A throwaway owner key from k256's own generator, for the tests that
    /// only exercise signing and verification and need no key file on disk.
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

    fn project() -> tempfile::TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("target").join("test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let d = tempfile::TempDir::new_in(&base).unwrap();
        std::fs::create_dir_all(d.path().join(".moss")).unwrap();
        d
    }

    fn comment(id: &str) -> super::super::NormalizedComment {
        super::super::NormalizedComment {
            id: id.into(),
            source: "artalk".into(),
            content: "<p>hello</p>".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            author: super::super::CommentAuthor { display_name: Some("Ana".into()), name: None, url: None },
            reply_to_id: None,
            state: None,
        }
    }

    #[test]
    fn owner_event_verifies_against_the_site_key_and_hides() {
        crate::infra::home::with_moss_home(|_home| {
        let d = project();
        let p = d.path();
        let id = crate::identity::keypair::Identity::generate().unwrap();
        id.save(p).unwrap();
        let ps = p.to_str().unwrap();

        let seq = append_owner_event(ps, ModKind::Hide, "artalk", "7", "page1", "localhost").unwrap();
        assert_eq!(seq, 1);
        let mut names: Vec<String> = std::fs::read_dir(MossPaths::new(p).social_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["mod-seq", "moderation.jsonl"], "no lock file inside the site folder");
        let events = load_mod_events(ps);
        assert_eq!(events.len(), 1);
        assert!(events[0].verify(&id.pubkey), "event verifies against the site's public key");
        let comments = vec![comment("7"), comment("8")];
        let visible = super::super::reduce::resolve(&comments, &events, &id.pubkey);
        assert_eq!(visible.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["8"]);
        });
    }

    #[test]
    fn owner_event_without_a_signing_key_fails_and_writes_nothing() {
        crate::infra::home::with_moss_home(|_home| {
        let d = project();
        let p = d.path();
        let ps = p.to_str().unwrap();
        let err = append_owner_event(ps, ModKind::Hide, "artalk", "7", "page1", "localhost").unwrap_err();
        assert!(err.contains("signing key"), "plain message: {err}");
        assert!(!moderation_log_path(ps).exists(), "no event written");
        assert!(!p.join(".moss/identity").exists(), "no key created");
        assert!(!MossPaths::new(p).social_dir().join("mod-seq").exists(), "no counter burned");
        });
    }

    #[test]
    fn next_seq_is_above_the_log_when_the_counter_lags() {
        let d = project();
        let ps = d.path().to_str().unwrap();
        let (sk, _) = owner_key();
        let log = moderation_log_path(ps);
        event_log::append_jsonl(&log, &sign_mod_event(&sk, "hide", "artalk", "1", "s", "p", 9, 1)).unwrap();
        event_log::append_jsonl(&log, &sign_mod_event(&sk, "unhide", "artalk", "1", "s", "p", 4, 2)).unwrap();
        std::fs::write(log.parent().unwrap().join("mod-seq"), "3").unwrap();
        assert_eq!(next_seq(ps).unwrap(), 10);
        assert_eq!(next_seq(ps).unwrap(), 11);
    }

    #[test]
    fn concurrent_writers_never_share_a_sequence_number() {
        crate::infra::home::with_moss_home(|_home| {
        let d = project();
        crate::identity::keypair::Identity::generate().unwrap().save(d.path()).unwrap();
        let ps = d.path().to_str().unwrap().to_string();
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let ps = ps.clone();
                std::thread::spawn(move || {
                    for i in 0..30 {
                        append_owner_event(&ps, ModKind::Hide, "artalk", &format!("{t}-{i}"), "pg", "localhost").unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let mut seqs: Vec<u64> = load_mod_events(&ps).iter().map(|e| e.seq).collect();
        assert_eq!(seqs.len(), 120);
        seqs.sort();
        seqs.dedup();
        assert_eq!(seqs.len(), 120, "every event has its own sequence number");
        });
    }
}
