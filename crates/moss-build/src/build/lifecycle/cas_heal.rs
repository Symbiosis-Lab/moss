//! Put a cached output back into staging from the object store.
//!
//! A staged output can go missing while its bytes are safe in the CAS: a
//! staging sweep, an iCloud eviction of `.moss/build`, or a batch that
//! registers long after it linked. The encode is what the fingerprint skip
//! saves; the file's presence in staging is not, so every skip path relinks
//! the blob it already owns instead of sending the source back to the encoder.
//! Images and videos share this one function.

use std::path::Path;

/// How far the heal may go to learn the source's content hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashPolicy {
    /// Only a `(size, whole-second mtime)` hit in the hash index
    /// ([`HashIndex::lookup_whole_second`]). The video dispatcher runs on the render
    /// thread and must never hash a multi-GB source there, so unlike `HashOnMiss` it
    /// cannot fail open to a hash and cannot afford the full-stat comparison.
    ///
    /// [`HashIndex::lookup_whole_second`]: crate::build::cache::HashIndex::lookup_whole_second
    StatOnly,
    /// Hash the source on an index miss, trusting a hit only by the full stat
    /// record ([`HashIndex::resolve`]). Images take this: the file is small, and the
    /// fingerprint gate has usually already said it is unchanged. A source still in
    /// the cloud is not read: `resolve` refuses, and the heal is `NotCached` — this
    /// runs over every item of a batch, including the ones the worker deferred to
    /// the cloud.
    ///
    /// [`HashIndex::resolve`]: crate::build::cache::HashIndex::resolve
    HashOnMiss,
}

/// What a heal attempt did.
#[derive(Debug)]
pub(crate) enum HealOutcome {
    /// The staged output is there; nothing was touched.
    AlreadyPresent,
    /// The cached blob was linked into staging.
    Healed,
    /// No cached output could be linked: the source hash is unknown under the
    /// policy, the store has no record or blob for it, or the link failed.
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn rematerialize(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    hash_index: &mut crate::build::cache::HashIndex,
    source_file: &Path,
    rel_source: &str,
    staging_path: &Path,
    transform: &str,
    policy: HashPolicy,
) -> HealOutcome {
    use crate::build::io_utils::Presence;
    match crate::build::io_utils::probe_path(staging_path) {
        Presence::Present => return HealOutcome::AlreadyPresent,
        Presence::Unverified(e) => return HealOutcome::Unverified(e),
        Presence::Absent | Presence::Evicted => {}
    }
    let source_oid = match policy {
        HashPolicy::StatOnly => match indexed_hash(hash_index, source_file, rel_source) {
            Some(oid) => oid,
            None => return HealOutcome::NotCached,
        },
        HashPolicy::HashOnMiss => {
            match hash_index.resolve(source_file, rel_source) {
                Ok(oid) => oid,
                Err(_) => return HealOutcome::NotCached,
            }
        }
    };
    // `find_cached_output` also checks the blob is still in the store, so a
    // hit means the bytes are recoverable.
    let Some(oid) = transforms.find_cached_output(&source_oid, transform, params) else {
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
fn indexed_hash(
    hash_index: &crate::build::cache::HashIndex,
    source_file: &Path,
    rel_source: &str,
) -> Option<String> {
    let stat = crate::build::cache::FileStat::of(&std::fs::metadata(source_file).ok()?);
    hash_index.lookup_whole_second(rel_source, stat.size, stat.mtime).map(str::to_string)
}
