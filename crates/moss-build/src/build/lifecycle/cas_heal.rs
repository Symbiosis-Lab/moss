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
    /// Only a `(size, mtime)` hit in the hash index. The video dispatcher runs
    /// on the render thread and must never hash a multi-GB source there.
    StatOnly,
    /// Hash the source on an index miss. Images take this behind their
    /// fingerprint gate, where the file is small and already known unchanged.
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
            match crate::build::video::resolve_source_hash(source_file, rel_source, hash_index) {
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
/// current size and mtime; `None` on a miss or an unstat-able source.
fn indexed_hash(
    hash_index: &crate::build::cache::HashIndex,
    source_file: &Path,
    rel_source: &str,
) -> Option<String> {
    let meta = std::fs::metadata(source_file).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    hash_index.lookup(rel_source, meta.len(), mtime).map(str::to_string)
}
