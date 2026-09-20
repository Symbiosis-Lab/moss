//! How `ship_phase` reads one manifest entry's staged bytes: from an immutable
//! CAS object, or from the mutable stage file with a stat fingerprint taken to
//! make a seal-to-ship rewrite audible. [`ShipSource`] is the single owner of
//! that choice, and this module holds the accessors and the one stamping
//! transition that read and write it on a [`SealedManifest`].
//!
//! Split out of `manifest.rs`, which is about registration and sealing: none of
//! this code touches a hash, a bucket or a mark set, only the `ship_sources`
//! map those two structs carry.

use std::path::Path;

use super::SealedManifest;

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

/// Which of the two ways `ship_phase` can resolve an entry's staged bytes is
/// on record for it — `Cas` before anything runs a post-seal rewrite,
/// `Fingerprint` once one does. An entry with neither is read straight from
/// the mutable `stage_dir` path, exactly as before either existed.
///
/// Replaces what used to be two same-keyed maps (`staged_oids`,
/// `ship_fingerprints`) whose "an entry has one or the other, never both"
/// invariant was maintained only by disciplined pairing at the two call
/// sites that transitioned between them: `stamp_all_ship_fingerprints`'s
/// `!staged_oids.contains_key(...)` filter, and `clear_staged_oids` +
/// `stamp_ship_fingerprints` always being called together in
/// `degrade::apply_to_staging`. A single map keyed to one of two variants
/// makes the exclusivity structural — there is no second map left for an
/// entry to also be in, so `stamp_ship_fingerprints` transitioning `Cas` to
/// `Fingerprint` is one `insert` overwriting one slot, not a clear-then-stamp
/// pair that could drift apart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShipSource {
    /// The CAS object id backing this entry's staged bytes — `ship_phase`
    /// reads the immutable blob instead of the mutable stage path.
    Cas(String),
    /// The stat-identity fingerprint taken at (or re-taken after) seal, for
    /// an entry `ship_phase` will read from the mutable stage path.
    Fingerprint(ShipFingerprint),
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

    /// Snapshot `stage_dir.join(key)`'s stat identity for each of `keys`,
    /// replacing whatever [`ShipSource`] was already recorded for that key —
    /// a live `Cas` source included. That makes this the ONE transition point
    /// from `Cas` to `Fingerprint`: a post-seal rewrite that bypasses the CAS
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

    /// Stamp every `100644:` entry that has no live `Cas` ship source — the
    /// whole set `ship_phase` will read from the mutable `stage_dir` rather
    /// than an immutable CAS blob. Called once, right after this manifest
    /// seals and before the post-seal repair passes run, so the window it
    /// protects is exactly "seal to ship": a concurrent build's rewrite
    /// anywhere in that window is what the fingerprint is meant to catch.
    pub(crate) fn stamp_all_ship_fingerprints(&mut self, stage_dir: &Path) {
        let keys: Vec<String> = self
            .inner
            .files
            .iter()
            .filter(|(k, v)| {
                !matches!(self.ship_sources.get(k.as_str()), Some(ShipSource::Cas(_)))
                    && crate::types::content::parse_entry(v).0 == crate::types::content::MODE_FILE
            })
            .map(|(k, _)| k.clone())
            .collect();
        self.stamp_ship_fingerprints(stage_dir, keys.iter().map(|s| s.as_str()));
    }
}
