//! Put a cached output back into staging from the object store.
//!
//! A staged output can go missing while its bytes are safe in the CAS: a
//! staging sweep, an iCloud eviction of `.moss/build.nosync`, or a batch that
//! registers long after it linked. The encode is what the skip decision
//! saves; the file's presence in staging is not, so every skip path relinks
//! the blob it already owns instead of sending the source back to the encoder.
//! Images and videos share these two entry points — [`rematerialize`] when the
//! caller only has a stat-index hint and must not hash on a miss, and
//! [`rematerialize_with_record`] when it already loaded the transform record
//! for a source identified by a strict stat-index lookup.

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

/// Restore an image variant from an already loaded source record. The caller
/// reuses that record for the base and every rung. Matching parameters select
/// the desired blob; a staged receipt proves its bytes with one stat. Presence
/// alone cannot distinguish an old generation's output from the desired one.
pub(crate) fn rematerialize_with_record(
    objects: &crate::build::cache::ObjectStore,
    record: Option<&crate::build::cache::TransformRecord>,
    params: &serde_json::Value,
    staged: &mut StagedLinks,
    staging_path: &Path,
    transform: &str,
) -> HealOutcome {
    let Some(entry) = record.and_then(|r| r.transforms.get(transform)).filter(|e| e.params == *params) else {
        return HealOutcome::NotCached;
    };
    let oid = entry.oid.clone();
    if staged.holds_oid(staging_path, &oid) {
        return HealOutcome::AlreadyPresent { oid: Some(oid) };
    }
    crate::build::phase::record_count("image_variants_blob_checks", 1);
    if objects.ready_blob(&oid).is_none() { return HealOutcome::NotCached; }
    match staged.link(objects, &oid, staging_path, None) {
        Placement::Held => HealOutcome::AlreadyPresent { oid: Some(oid) },
        Placement::Linked => {
            log::debug!("[cas-heal] re-materialized {} from CAS ({})", staging_path.display(), oid);
            HealOutcome::Healed { oid }
        }
        Placement::Unverified(e) => HealOutcome::Unverified(e),
        Placement::Failed(e) => {
            log::info!("[cas-heal] re-link failed for {}: {}", staging_path.display(), e);
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
            log::info!("[cas-heal] re-link failed for {}: {}", staging_path.display(), e);
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
        local: Option<&Path>,
    ) -> Placement {
        use crate::build::io_utils::Presence;
        let key = staging_path.strip_prefix(&self.staging).ok().map(|rel| rel.to_string_lossy().into_owned());
        let metadata = std::fs::symlink_metadata(staging_path);
        let stat = metadata.as_ref().ok().filter(|md| md.is_file()).map(crate::build::stat::FileStat::of);
        match crate::build::io_utils::probe_output_stat(staging_path, "100644", metadata) {
            Presence::Unverified(e) => return Placement::Unverified(e),
            Presence::Present if key.as_deref().zip(stat.as_ref()).is_some_and(|(key, stat)| {
                self.holds_stat(key, stat, oid) || self.renews_named(key, stat, oid, staging_path)
            }) => {
                return Placement::Held;
            }
            _ => {}
        }
        // Sampled before the bytes land, like every other record's clock.
        let recorded_at = crate::build::stat::recording_clock();
        let written_inode = match objects.link_to_inode(oid, staging_path, local) {
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
    fn holds_stat(&mut self, key: &str, stat: &crate::build::stat::FileStat, oid: &str) -> bool {
        let Some(entry) = self.current.entries.get(key).or_else(|| self.previous.entries.get(key)).cloned() else { return false };
        let held = entry.content_hash == oid && entry.stat.vouches_for(stat, entry.recorded_at);
        if held { self.current.entries.insert(key.to_string(), entry); }
        held
    }

    /// Whether the present file at `staging_path`, which a record names as
    /// `oid`'s copy but whose stat no longer vouches for it, still holds
    /// `oid`'s bytes; records it anew when it does. Replacing it instead would
    /// move its ctime, which on a coarse-timestamp filesystem never settles.
    fn renews_named(&mut self, key: &str, stat: &crate::build::stat::FileStat, oid: &str, staging_path: &Path) -> bool {
        let named = [&self.current, &self.previous].iter().any(|index| index.entries.get(key).is_some_and(|e| e.content_hash == oid));
        if !named { return false; }
        let recorded_at = crate::build::stat::recording_clock();
        let renewed = crate::build::cache::ObjectStore::hash_file(staging_path).is_ok_and(|hash| hash == oid)
            && std::fs::symlink_metadata(staging_path).is_ok_and(|md| crate::build::stat::FileStat::of(&md) == *stat);
        if renewed { self.current.update_read_at(key.to_string(), stat, oid.to_string(), recorded_at); }
        renewed
    }

    /// One stat proves both residency and the identity of the staged bytes.
    pub(crate) fn holds_oid(&mut self, path: &Path, oid: &str) -> bool {
        let Ok(md) = std::fs::symlink_metadata(path) else { return false };
        if !md.is_file() || md.len() == 0 || crate::build::icloud::is_evicted_stat(path, &md) { return false; }
        let Some(key) = path.strip_prefix(&self.staging).ok().map(|p| p.to_string_lossy().into_owned()) else { return false };
        self.holds_stat(&key, &crate::build::stat::FileStat::of(&md), oid)
    }

    /// The blob the file at `key` (staging-relative) was linked from, when the
    /// file there is still that link; carries its record into this run's.
    pub(crate) fn vouched_oid(&mut self, key: &str) -> Option<String> {
        let oid = self.previous.entries.get(key)?.content_hash.clone();
        self.holds_oid(&self.staging.join(key), &oid).then_some(oid)
    }

    /// Persist what this run linked or found held. Writes nothing when that
    /// is exactly what was loaded — the steady state of a warm rebuild.
    pub(crate) fn save(&self) {
        if self.current.entries == self.previous.entries {
            return;
        }
        if let Err(e) = self.current.save(&self.file) {
            log::info!("[cas-heal] could not save {}: {}", self.file.display(), e);
        }
    }
}
