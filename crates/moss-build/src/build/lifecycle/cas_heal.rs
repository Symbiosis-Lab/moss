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
    /// The staged output is there; nothing was touched. `oid` is the blob a
    /// staged-link record vouches the file holds, `None` when only its
    /// presence was checked.
    AlreadyPresent { oid: Option<String> },
    /// The cached blob `oid` was linked into staging.
    Healed { oid: String },
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
        Presence::Present => return HealOutcome::AlreadyPresent { oid: None },
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
/// Deliberately does NOT take [`rematerialize`]'s "present → already correct"
/// shortcut: that shortcut is only sound when the caller reached this file by
/// first confirming its own bytes are unchanged. A stat-index hit proves only
/// what the SOURCE's current content is, never that the file already sitting
/// at `staging_path` was encoded from it — staging carries content forward
/// between builds, so a changed source can find its PREVIOUS build's output
/// still present and, if presence alone were trusted, ship it unchanged. This
/// always re-verifies the exact oid against the transform cache, then leaves
/// the placement to [`StagedLinks::link`], which links it unless its record
/// of the last link still vouches for the file, and never overwrites a path
/// it cannot verify.
///
/// [`HashIndex::lookup`]: crate::build::cache::HashIndex::lookup
pub(crate) fn rematerialize_with_oid(
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    staged: &mut StagedLinks,
    staging_path: &Path,
    transform: &str,
    source_oid: &str,
) -> HealOutcome {
    let Some(oid) = transforms.find_cached_output(source_oid, transform, params, crate::build::cache::RecordMode::Wait) else {
        return HealOutcome::NotCached;
    };
    match staged.link(objects, &oid, staging_path) {
        Placement::Held => HealOutcome::AlreadyPresent { oid: Some(oid) },
        Placement::Linked => {
            log::debug!("[cas-heal] re-materialized {} from CAS ({})", staging_path.display(), oid);
            HealOutcome::Healed { oid }
        }
        Placement::Unverified(e) => HealOutcome::Unverified(e),
        Placement::Failed(e) => {
            log::warn!("[cas-heal] re-link failed for {}: {}", staging_path.display(), e);
            HealOutcome::NotCached
        }
    }
}

/// [`rematerialize`]'s second half: `source_oid` is already known, so this
/// only has to find its cached transform output and link it in.
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
    let Some(oid) = transforms.find_cached_output(source_oid, transform, params, crate::build::cache::RecordMode::Wait) else {
        return HealOutcome::NotCached;
    };
    match objects.link_to(&oid, staging_path) {
        Ok(()) => {
            log::debug!("[cas-heal] re-materialized {} from CAS ({})", staging_path.display(), oid);
            HealOutcome::Healed { oid }
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
    let stat = crate::build::stat::FileStat::of(&std::fs::metadata(source_file).ok()?);
    hash_index.lookup_whole_second(rel_source, stat.size, stat.mtime).map(str::to_string)
}

/// Which CAS blob each staged output was last linked from, with the stat
/// record the staged file had right after the link — a [`HashIndex`] keyed by
/// the path relative to staging, whose content hash is the linked blob's oid,
/// judged by the same rule as every other stat-keyed fast path
/// ([`FileStat::vouches_for`]).
///
/// A staged file's presence never proves which bytes it holds; this record
/// does, for as long as the file's stat still vouches for it. Whatever
/// replaces or edits the file moves that stat: another link (temp-and-rename,
/// so a new inode), an encode, an editor, a same-size rewrite in the same
/// second (sub-second mtime, ctime). An evicted or 0-byte file is never
/// asked: [`link`](Self::link) probes presence first. Where the platform
/// reports no inode or ctime (Windows), size and the full-resolution mtime
/// still have to agree — a copy keeps the blob's own mtime there, so two
/// different blobs would have to share size and mtime to the tick.
///
/// Per machine, beside staging: the record is of this machine's inodes. One
/// file per producer, rewritten whole from what this run linked or found
/// still held, so a path that left the site leaves the record with it.
///
/// [`HashIndex`]: crate::build::cache::HashIndex
/// [`FileStat::vouches_for`]: crate::build::stat::FileStat::vouches_for
pub(crate) struct StagedLinks {
    file: std::path::PathBuf,
    staging: std::path::PathBuf,
    previous: crate::build::cache::HashIndex,
    current: crate::build::cache::HashIndex,
}

#[cfg(test)]
thread_local! {
    /// Test-only: called by [`StagedLinks::link`] between its link and the
    /// stat it records, where another process's write would land.
    pub(crate) static AFTER_LINK: std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>> =
        const { std::cell::RefCell::new(None) };
}

/// What [`StagedLinks::link`] did with a staged path.
#[derive(Debug)]
pub(crate) enum Placement {
    /// The file there is still the one linked from this blob; left alone.
    Held,
    /// The blob was linked in.
    Linked,
    /// The path could not be checked, so nothing was linked over it.
    Unverified(std::io::Error),
    /// The link itself failed.
    Failed(String),
}

impl StagedLinks {
    /// The record `producer` left in `paths`' per-machine cache for the
    /// outputs it links under `staging`, or an empty one (everything relinks
    /// once) when there is none or it cannot be read.
    pub(crate) fn load(paths: &crate::moss_paths::MossPaths, producer: &str, staging: &Path) -> Self {
        let file = paths.cache_dir().join(format!("staged-links-{producer}.json"));
        let previous = crate::build::cache::HashIndex::load(&file);
        Self { file, staging: staging.to_path_buf(), previous, current: crate::build::cache::HashIndex::new() }
    }

    /// Link `oid` into `staging_path` unless the file there is still the one
    /// this record says was linked from `oid`. The one owner of that decision
    /// for every staged output a CAS blob backs.
    pub(crate) fn link(
        &mut self,
        objects: &crate::build::cache::ObjectStore,
        oid: &str,
        staging_path: &Path,
    ) -> Placement {
        use crate::build::io_utils::Presence;
        let key = staging_path.strip_prefix(&self.staging).ok().map(|rel| rel.to_string_lossy().into_owned());
        match crate::build::io_utils::probe_path(staging_path) {
            Presence::Unverified(e) => return Placement::Unverified(e),
            Presence::Present if key.as_deref().is_some_and(|key| self.holds(key, staging_path, oid)) => {
                return Placement::Held;
            }
            _ => {}
        }
        // Sampled before the bytes land, like every other record's clock.
        let recorded_at = crate::build::stat::recording_clock();
        let written_inode = match objects.link_to_inode(oid, staging_path) {
            Ok(inode) => inode,
            Err(e) => return Placement::Failed(e),
        };
        #[cfg(test)]
        AFTER_LINK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(staging_path);
            }
        });
        // Recorded only when the file stat'd is the one this link wrote: a
        // concurrent build could rename its own link in between.
        let stat = std::fs::symlink_metadata(staging_path).map(|md| crate::build::stat::FileStat::of(&md));
        if let (Some(key), Ok(stat)) = (key, stat) {
            if !crate::build::stat::identity_disagrees(written_inode, stat.inode) {
                let entry = crate::build::cache::HashIndexEntry { stat, recorded_at, content_hash: oid.to_string() };
                self.current.entries.insert(key, entry);
            }
        }
        Placement::Linked
    }

    /// Whether the present file at `staging_path` is still the one linked
    /// from `oid`; carries its record into this run's when it is.
    fn holds(&mut self, key: &str, staging_path: &Path, oid: &str) -> bool {
        let Some(entry) = self.previous.entries.get(key) else { return false };
        let Ok(md) = std::fs::symlink_metadata(staging_path) else { return false };
        let current = crate::build::stat::FileStat::of(&md);
        let held = entry.content_hash == oid && entry.stat.vouches_for(&current, entry.recorded_at);
        if held {
            self.current.carry_forward(&self.previous, key);
        }
        held
    }

    /// Persist what this run linked or found held. Writes nothing when that
    /// is exactly what was loaded — the steady state of a warm rebuild.
    pub(crate) fn save(&self) {
        if self.current.entries == self.previous.entries {
            return;
        }
        if let Err(e) = self.current.save(&self.file) {
            log::warn!("[cas-heal] could not save {}: {}", self.file.display(), e);
        }
    }
}
