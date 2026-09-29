//! Put a cached output back into staging from the object store.
//!
//! A staged output can go missing while its bytes are safe in the CAS: a
//! staging sweep, an iCloud eviction of `.moss/build.nosync`, or a batch that
//! registers long after it linked. The encode is what the skip decision
//! saves; the file's presence in staging is not, so every skip path relinks
//! the blob it already owns instead of sending the source back to the encoder.
//! Images and videos share these two entry points — [`rematerialize`] when the
//! caller only has a stat-index hint and must not hash on a miss, and
//! [`rematerialize_with_oid`] when it already holds the source's content hash
//! (e.g. from a strict, no-hash-on-miss lookup the caller ran itself).

use std::path::Path;

/// What a heal attempt did.
#[derive(Debug)]
pub(crate) enum HealOutcome {
    /// The staged output is there; nothing was touched.
    AlreadyPresent,
    /// The cached blob was linked into staging.
    Healed,
    /// No cached output could be linked: the source hash is unknown to the
    /// stat index, the store has no record or blob for it, or the link failed.
    NotCached,
    /// The staged path could not be checked. An unreadable output is not a
    /// missing one, so nothing was linked over it.
    Unverified(std::io::Error),
}

/// Relink `transform`'s cached output for `source_file` to `staging_path` when
/// the staged copy is absent or evicted.
///
/// A present output is left byte-for-byte alone, so the steady-state rebuild
/// pays one `lstat`. A source the store never encoded is `NotCached`, never a
/// fabricated file. The link is [`ObjectStore::link_to`]'s COW copy with
/// temp-and-rename, never a hard link.
///
/// [`ObjectStore::link_to`]: crate::build::cache::ObjectStore::link_to
///
/// The source hash comes only from a `(size, whole-second mtime)` hit in the
/// hash index ([`indexed_hash`]), never a hash-on-miss: the video dispatcher
/// runs on the render thread and must never hash a multi-GB source there. An
/// index miss is `NotCached`, not a fallback to hashing.
pub(crate) fn rematerialize(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    hash_index: &crate::build::cache::HashIndex,
    source_file: &Path,
    rel_source: &str,
    staging_path: &Path,
    transform: &str,
) -> HealOutcome {
    use crate::build::io_utils::Presence;
    match crate::build::io_utils::probe_path(staging_path) {
        Presence::Present => return HealOutcome::AlreadyPresent,
        Presence::Unverified(e) => return HealOutcome::Unverified(e),
        Presence::Absent | Presence::Evicted => {}
    }
    let source_oid = match indexed_hash(hash_index, source_file, rel_source) {
        Some(oid) => oid,
        None => return HealOutcome::NotCached,
    };
    relink_from_cache(objects, transforms, params, staging_path, transform, &source_oid)
}

/// [`rematerialize`], for a caller that already holds the source's content hash —
/// e.g. a strict, no-hash-on-miss [`HashIndex::lookup`] the caller ran itself,
/// against a `FileStat` it already had in hand.
///
/// Deliberately does NOT take [`rematerialize`]'s "present → already correct,
/// skip the lstat's-worth of work" shortcut: that shortcut is only sound when
/// the caller reached this file by first confirming its own bytes are
/// unchanged (the old fingerprint-ledger contract every other caller of
/// [`rematerialize`] still honors). A stat-index hit proves only what the
/// SOURCE's current content is, never that the file already sitting at
/// `staging_path` was encoded from it — staging carries content forward
/// between builds, so a changed source can find its PREVIOUS build's output
/// still present and, if presence alone were trusted, ship it unchanged. This
/// always re-verifies the exact oid against the transform cache and relinks
/// (a COW copy-and-rename, cheap — see [`ObjectStore::link_to`]), so a stale
/// file is overwritten with the correct bytes and a genuinely-unchanged one
/// is relinked to itself. Bails early only on `Unverified`: a path this
/// process cannot even confirm is not safe to overwrite.
///
/// [`HashIndex::lookup`]: crate::build::cache::HashIndex::lookup
/// [`ObjectStore::link_to`]: crate::build::cache::ObjectStore::link_to
pub(crate) fn rematerialize_with_oid(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    staging_path: &Path,
    transform: &str,
    source_oid: &str,
) -> HealOutcome {
    use crate::build::io_utils::Presence;
    if let Presence::Unverified(e) = crate::build::io_utils::probe_path(staging_path) {
        return HealOutcome::Unverified(e);
    }
    relink_from_cache(objects, transforms, params, staging_path, transform, source_oid)
}

/// The shared second half of both entry points above: `source_oid` is already
/// known, so this only has to find its cached transform output and link it in.
fn relink_from_cache(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    staging_path: &Path,
    transform: &str,
    source_oid: &str,
) -> HealOutcome {
    // `find_cached_output` also checks the blob is still in the store, so a
    // hit means the bytes are recoverable.
    let Some(oid) = transforms.find_cached_output(source_oid, transform, params) else {
        return HealOutcome::NotCached;
    };
    match objects.link_to(&oid, staging_path) {
        Ok(()) => {
            log::debug!("[cas-heal] re-materialized {} from CAS ({})", staging_path.display(), oid);
            HealOutcome::Healed
        }
        Err(e) => {
            log::warn!("[cas-heal] re-link failed for {}: {}", staging_path.display(), e);
            HealOutcome::NotCached
        }
    }
}

/// The source's content hash when the index already holds it for the file's
/// current size and whole-second mtime; `None` on a miss or an unstat-able source.
///
/// Whole-second because the caller cannot hash: a same-size rewrite in the same
/// second still hits here and relinks the previous source's cached output. It is
/// only ever asked for a video, whose own dispatch (and worker) decide what is
/// encoded — see `VideoStore::stage`.
///
/// `pub(crate)`, not private: `VideoStore::stage`'s own HLS-ladder heal leg
/// (video.rs) needs the same source oid `rematerialize` derives internally for
/// mp4/thumb, to look a cached ladder up directly — `hls::heal_cached_ladder`
/// takes an oid, not a file, because a ladder is several files, not one
/// `rematerialize` call.
pub(crate) fn indexed_hash(
    hash_index: &crate::build::cache::HashIndex,
    source_file: &Path,
    rel_source: &str,
) -> Option<String> {
    let stat = crate::build::cache::FileStat::of(&std::fs::metadata(source_file).ok()?);
    hash_index.lookup_whole_second(rel_source, stat.size, stat.mtime).map(str::to_string)
}
