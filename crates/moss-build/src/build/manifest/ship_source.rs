//! How `ship_phase` reads one manifest entry's staged bytes: from an immutable
//! CAS object, from bytes the build kept in memory, or from the mutable stage
//! file with a stat fingerprint taken to make a seal-to-ship rewrite audible.
//! [`ShipSource`] is the single owner of that choice, and this module holds the
//! accessors and the one stamping transition that read and write it on a
//! [`SealedManifest`], plus the registration that pins held bytes.
//!
//! Split out of `manifest.rs`, which is about registration and sealing: none of
//! this code touches a hash, a bucket or a mark set beyond what it delegates to
//! `register`, only the `ship_sources` map those two structs carry.

use std::path::Path;
use std::sync::Arc;

use super::{HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::{transform_for, ServedPath, ShipTransform};

/// The most bytes one manifest holds for [`ShipSource::Held`].
///
/// Held bytes ride the manifest from the render that registered them through
/// the background phase and the queue for the stage-write lock, and are
/// dropped only when the seal tail has shipped ([`SealedManifest::release_held`]).
/// So the bound on memory is one manifest's bytes for every seal tail still in
/// flight, and a tail queued behind the lock keeps its manifest until its turn.
/// How many tails queue depends on how often the folder is rebuilt meanwhile,
/// which nothing here controls, so each manifest is capped instead of trusting
/// the payloads to stay small: N queued tails cost at most N times this.
///
/// 32 MiB is a bit over three times the largest real vault measured (246 pages,
/// 4.2 MB of markdown: `llms.txt` 4.1 MB + `rss.xml` 4.8 MB + a 25 KB sitemap,
/// about 9 MB), so today's sites are never demoted. A site past it (the largest
/// documented corpus, 2,347 pages, extrapolates to 40 to 85 MB) loses the
/// protection for the files that do not fit and ships them from the stage as it
/// did before `Held` existed: [`PendingManifest::register_held`] falls back, it
/// never fails the build.
pub(crate) const HELD_BYTES_BUDGET: usize = 32 * 1024 * 1024;

/// Output bytes shared between the manifest that registered them and the seal
/// tail that ships them. Cloning shares the buffer.
///
/// A newtype over `Arc<Vec<u8>>` rather than the bare type because manifests
/// derive `Debug`, and the derived `Debug` of a byte vector would print
/// megabytes of decimal digits into a failing test or a log line. `Vec`, not
/// `[u8]`, so a producer's own buffer moves in without a copy.
#[derive(Clone, PartialEq)]
pub(crate) struct HeldBytes(Arc<Vec<u8>>);

impl HeldBytes {
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl std::fmt::Debug for HeldBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HeldBytes({} bytes)", self.len())
    }
}

/// The forgery-resistant stat snapshot `ship_phase`'s integrity check compares
/// against, for one manifest entry. Same fields `SourceMetadata`/`stat_identity`
/// already model for the identical reason (`build/types.rs`): `mtime` alone
/// cannot distinguish the hashed bytes from a same-second, same-size rewrite,
/// and a replace-via-rename changes the inode even when size and mtime survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShipFingerprint {
    size: u64,
    mtime_secs: u64,
    mtime_nanos: Option<u32>,
    ctime: Option<i64>,
    inode: Option<u64>,
}

impl ShipFingerprint {
    pub(crate) fn of(meta: &std::fs::Metadata) -> Option<Self> {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let (ctime, inode) = crate::build::types::stat_identity(meta);
        Some(Self {
            size: meta.len(),
            mtime_secs: mtime.map(|d| d.as_secs()).unwrap_or(0),
            mtime_nanos: mtime.map(|d| d.subsec_nanos()),
            ctime,
            inode,
        })
    }
}

/// Which of the ways `ship_phase` can resolve an entry's staged bytes is on
/// record for it — `Cas` or `Held` before anything runs a post-seal rewrite,
/// `Fingerprint` once one does. An entry with none is read straight from the
/// mutable `stage_dir` path, exactly as before any of them existed.
///
/// `Cas` and `Held` are the immutable ones: the bytes were fixed when the
/// entry was registered, so a later build rewriting the stage path cannot reach
/// them. `Fingerprint` is the audit for the rest, and only ever logs.
///
/// Replaces what used to be two same-keyed maps (`staged_oids`,
/// `ship_fingerprints`) whose "an entry has one or the other, never both"
/// invariant was maintained only by disciplined pairing at the two call
/// sites that transitioned between them: `stamp_all_ship_fingerprints`'s
/// `!staged_oids.contains_key(...)` filter, and `clear_staged_oids` +
/// `stamp_ship_fingerprints` always being called together in
/// `degrade::apply_to_staging`. A single map keyed to one of the variants
/// makes the exclusivity structural — there is no second map left for an
/// entry to also be in, so `stamp_ship_fingerprints` transitioning `Cas` to
/// `Fingerprint` is one `insert` overwriting one slot, not a clear-then-stamp
/// pair that could drift apart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShipSource {
    /// The CAS object id backing this entry's staged bytes — `ship_phase`
    /// reads the immutable blob instead of the mutable stage path.
    Cas(String),
    /// The bytes themselves, kept in memory from registration until the seal
    /// tail ships. For a derived output every build rewrites under one name
    /// (`sitemap.xml`, `llms.txt`), where a CAS object would be minted and
    /// abandoned on every edit. See [`HELD_BYTES_BUDGET`] for the bound.
    Held(HeldBytes),
    /// The stat-identity fingerprint taken at (or re-taken after) seal, for
    /// an entry `ship_phase` will read from the mutable stage path.
    Fingerprint(ShipFingerprint),
}

impl ShipSource {
    /// Bytes this source keeps alive: only `Held` keeps any.
    fn held_len(&self) -> usize {
        match self {
            ShipSource::Held(bytes) => bytes.len(),
            _ => 0,
        }
    }
}

impl PendingManifest {
    /// `Err(InvalidInput)` for a path `ship_phase` would transform, `Ok` for one
    /// that can be held. Its own function so `BuildContext::emit_held` can refuse
    /// BEFORE it writes the stage copy: a refusal after the write leaves a stage
    /// file no manifest entry names. [`register_held`][Self::register_held]
    /// asks it again, so registering directly cannot skip the guard.
    pub(crate) fn ensure_holdable(rel_path: &ServedPath) -> std::io::Result<()> {
        if transform_for(rel_path.as_str()) != ShipTransform::CopyAsIs {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{}: only a copied-as-is output can be held; ship rewrites this one", rel_path.as_str()),
            ));
        }
        Ok(())
    }

    /// [`register`][PendingManifest::register] `bytes` and keep them, so the
    /// generation ships THESE bytes rather than whatever the stage path holds
    /// when the seal tail gets to it. For the derived outputs a later build
    /// rewrites under the same name and that no CAS object backs.
    ///
    /// Refuses a path `ship_phase` would transform: an `.html` entry's hash is
    /// of the bytes as registered but its shipped copy is stripped of preview
    /// attributes, so holding the registered bytes would ship `data-source-*`
    /// annotations to production under a hash that agrees with them, and
    /// nothing downstream would notice. A returned error, not a debug assertion:
    /// in release the misuse is silent.
    ///
    /// Past [`HELD_BYTES_BUDGET`] the path is registered without keeping the
    /// bytes, i.e. exactly as `register` alone: it ships from the stage under
    /// the fingerprint audit. That is the same disposition as a CAS store that
    /// failed, and never fails the build.
    ///
    /// `register` runs FIRST and the source is inserted SECOND. Registration is
    /// last-wins and removes any source on record for the path
    /// (`register_with_hash` with no oid); inserted first, the pin would be
    /// cleared by the very call that records its hash. Doing it in this order
    /// also means the byte total below never counts a payload this call is
    /// replacing.
    pub(crate) fn register_held(
        &mut self,
        rel_path: &ServedPath,
        bytes: Vec<u8>,
        bucket: HashBucket,
    ) -> std::io::Result<()> {
        Self::ensure_holdable(rel_path)?;
        self.register(rel_path, &bytes, bucket);
        let held: usize = self.ship_sources.values().map(ShipSource::held_len).sum();
        if held + bytes.len() > HELD_BYTES_BUDGET {
            log::info!(
                "[manifest] {} ({} bytes) not held: {} of {} bytes already are; it ships from the stage",
                rel_path.as_str(),
                bytes.len(),
                held,
                HELD_BYTES_BUDGET
            );
            return Ok(());
        }
        self.ship_sources.insert(rel_path.as_str().to_string(), ShipSource::Held(HeldBytes(Arc::new(bytes))));
        Ok(())
    }
}

impl SealedManifest {
    /// The CAS object id already backing `rel_path`'s staged bytes, if a
    /// producer recorded one (see [`super::PendingManifest::ship_sources`]) and it has
    /// not since transitioned to a [`ShipSource::Fingerprint`] (a post-seal
    /// repair rewrote it directly — see
    /// [`stamp_ship_fingerprints`][Self::stamp_ship_fingerprints]).
    pub(crate) fn staged_oid(&self, rel_path: &str) -> Option<&str> {
        match self.ship_sources.get(rel_path) {
            Some(ShipSource::Cas(oid)) => Some(oid.as_str()),
            _ => None,
        }
    }

    /// The fingerprint recorded for `rel_path`, if any — `None` for a
    /// CAS-backed entry (never stamped), a symlink entry (never stamped), or
    /// one whose stage file could not be stat'd when stamping ran.
    pub(crate) fn ship_fingerprint(&self, rel_path: &str) -> Option<&ShipFingerprint> {
        match self.ship_sources.get(rel_path) {
            Some(ShipSource::Fingerprint(fp)) => Some(fp),
            _ => None,
        }
    }

    /// The bytes held for `rel_path`, if its source is [`ShipSource::Held`].
    pub(crate) fn held_bytes(&self, rel_path: &str) -> Option<&[u8]> {
        match self.ship_sources.get(rel_path) {
            Some(ShipSource::Held(bytes)) => Some(bytes.0.as_slice()),
            _ => None,
        }
    }

    /// Drop every [`ShipSource::Held`] entry, freeing the bytes. The seal tail
    /// calls this once `materialize_and_promote` has returned, whatever it
    /// returned: `ship_phase` is the only reader, and the manifest goes on to
    /// live for the app's lifetime as the one deploy reads, where megabytes of
    /// already-shipped output per open folder would be pure retention. An entry
    /// loses its source, not its place: the hash and the file stay.
    pub(crate) fn release_held(&mut self) {
        self.ship_sources.retain(|_, source| !matches!(source, ShipSource::Held(_)));
    }

    /// Snapshot `stage_dir.join(key)`'s stat identity for each of `keys`,
    /// replacing whatever [`ShipSource`] was already recorded for that key —
    /// a live `Cas` or `Held` source included. That makes this the ONE
    /// transition point from an immutable source to `Fingerprint`: a post-seal rewrite that bypasses the CAS
    /// (`degrade::apply_to_staging`, the one caller outside of seal itself)
    /// calls this and nothing else, so the entry it names can no longer read
    /// as `Cas` afterward — there is no second map left for a stale OID to
    /// survive in. A key whose file cannot be stat'd is left with no ship
    /// source at all (removed if it had one) rather than erroring —
    /// `ship_phase`'s check fails open on a missing fingerprint exactly as it
    /// does on a stat match: nothing to compare against is not evidence of a
    /// race.
    pub(crate) fn stamp_ship_fingerprints<'a>(
        &mut self,
        stage_dir: &Path,
        keys: impl IntoIterator<Item = &'a str>,
    ) {
        for key in keys {
            match std::fs::metadata(stage_dir.join(key)).ok().and_then(|m| ShipFingerprint::of(&m)) {
                Some(fp) => {
                    self.ship_sources.insert(key.to_string(), ShipSource::Fingerprint(fp));
                }
                None => {
                    self.ship_sources.remove(key);
                }
            }
        }
    }

    /// Stamp every `100644:` entry that has no ship source at all — the whole
    /// set `ship_phase` will read from the mutable `stage_dir` rather than an
    /// immutable one. Called once, right after this manifest seals and before
    /// the post-seal repair passes run, so the window it protects is exactly
    /// "seal to ship": a concurrent build's rewrite anywhere in that window is
    /// what the fingerprint is meant to catch.
    ///
    /// Filters on "has a source", not on which one: at this point the map holds
    /// only what registration recorded, every one of it immutable, so a new
    /// immutable variant needs no matching edit here. Matching variants instead
    /// would let a forgotten one be silently overwritten by a fingerprint.
    pub(crate) fn stamp_all_ship_fingerprints(&mut self, stage_dir: &Path) {
        let keys: Vec<String> = self
            .inner
            .files
            .iter()
            .filter(|(k, v)| {
                !self.ship_sources.contains_key(k.as_str())
                    && crate::types::content::parse_entry(v).0 == crate::types::content::MODE_FILE
            })
            .map(|(k, _)| k.clone())
            .collect();
        self.stamp_ship_fingerprints(stage_dir, keys.iter().map(|s| s.as_str()));
    }
}
