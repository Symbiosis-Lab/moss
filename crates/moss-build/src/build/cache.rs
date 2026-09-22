//! Content-addressed blob store and transform cache.
//!
//! This module implements a two-layer caching system inspired by several
//! well-known content-addressable storage designs:
//!
//! - **Git's object store** (`.git/objects/ab/cdef...`): files are stored by
//!   their SHA-256 hash in a sharded directory tree. The sharding prevents any
//!   single directory from accumulating millions of entries.
//!
//! - **Bazel's action cache**: a transform record maps (source hash + transform
//!   config) to output hashes, so an expensive transformation (e.g., image
//!   resize, video transcode) is never repeated if the inputs haven't changed.
//!
//! - **DVC's content-addressable cache**: output files are linked (hardlink or
//!   copy) from the cache into the working tree, avoiding redundant copies on
//!   the same filesystem.
//!
//! ## Layout on disk
//!
//! ```text
//! .moss/build.nosync/cache/
//! ├── objects/          # ObjectStore — raw blobs keyed by SHA-256
//! │   └── ab/cd/<full_hash>
//! └── transforms/       # TransformCache — JSON records keyed by source hash
//!     └── ab/cd/<source_hash>.json
//! ```

use crate::build::types::identity_disagrees;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Size of the read buffer used by [`ObjectStore::hash_file`].
///
/// 64 KB is a good trade-off between syscall overhead and memory use.
/// Large media files (videos, high-res images) are streamed through this
/// buffer without ever being loaded entirely into memory.
const HASH_BUF_SIZE: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// ObjectStore
// ---------------------------------------------------------------------------

/// Content-addressable blob storage, modeled after git's `.git/objects/`
/// directory.
///
/// Every blob is stored under a sharded path derived from its SHA-256 hash:
///
/// ```text
/// <base>/ab/cd/<full_64-char_hex_hash>
/// ```
///
/// where `ab` is the first two hex characters and `cd` is the next two.
/// This two-level fan-out keeps directory listings manageable even for
/// repositories with hundreds of thousands of objects (the same strategy
/// git uses).
pub struct ObjectStore {
    /// Root directory — typically `.moss/cache/objects/`.
    base: PathBuf,
}

impl ObjectStore {
    /// Create a new `ObjectStore` rooted at `base`.
    ///
    /// `base` is typically `.moss/cache/objects/`. The directory is created
    /// lazily (on first write), not here.
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }

    /// Returns the root directory of this object store.
    pub fn root(&self) -> &Path {
        &self.base
    }

    /// Compute the SHA-256 hash of a file, streaming in 64 KB chunks.
    ///
    /// Returns the lowercase hex digest (64 characters). The file is never
    /// loaded entirely into memory, so this is safe to call on multi-GB
    /// video files.
    ///
    /// Every non-markdown vault file walks through here on its way into the
    /// build, so this is where a cloud-evicted source gets its one chance to be
    /// downloaded back.
    pub fn hash_file(path: &Path) -> Result<String, String> {
        crate::build::cloud_readiness::retry_after_materialize(
            path,
            crate::build::cloud_readiness::MATERIALIZE_DEADLINE,
            || Self::hash_file_once(path),
        )
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
    }

    fn hash_file_once(path: &Path) -> std::io::Result<String> {
        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; HASH_BUF_SIZE];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// The state around a failed store or link, as ONE log line at the failure
    /// site. Kept out of the returned `Err(String)` on purpose: that string
    /// reaches advisory text, which dedups on its exact wording, so a varying
    /// field in it would warn once per rebuild instead of once.
    fn failure_context(&self, dest: &Path, tmp: Option<&Path>) -> String {
        let shard = dest.parent().unwrap_or(dest);
        let mut line = format!(
            "shard dir exists={} dataless={}",
            shard.is_dir(),
            crate::build::icloud::is_dataless_dir(shard)
        );
        if let Some(tmp) = tmp {
            line.push_str(&format!("; pending tmp exists={}", tmp.symlink_metadata().is_ok()));
        }
        line.push_str(&format!("; {}", last_gc_summary(&self.base)));
        line
    }

    /// Store a file in the object store, returning its SHA-256 OID.
    ///
    /// The write is atomic: the blob is first written to a temporary file
    /// in the same directory, then renamed into place. This follows the
    /// same pattern as git-lfs's "clean filter" — a crash can never leave
    /// a half-written blob at the final path.
    ///
    /// If the blob already exists (same hash), this is a no-op — the
    /// existing blob is kept and the OID is returned. This makes the
    /// operation idempotent.
    pub fn store_file(&self, source: &Path) -> Result<String, String> {
        let oid = Self::hash_file(source)?;
        let dest = self.blob_path(&oid);
        let source_size = fs::metadata(source).map(|m| m.len()).unwrap_or(0);

        // Idempotent: if the blob already exists AND passes validation,
        // skip the write. If the blob is corrupt (0-byte but source is
        // non-empty), validate_blob removes it and we fall through to
        // re-store.
        if dest.exists() {
            if self.validate_blob(&oid, source_size).is_ok() {
                return Ok(oid);
            }
            // validate_blob already removed the corrupt blob; fall through.
        }

        // Ensure the parent directory (e.g., `base/ab/cd/`) exists.
        if let Some(parent) = dest.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        // Write to a temp file in the same directory, then rename.
        // `fs::rename` is atomic on POSIX when source and dest are on the
        // same filesystem. If they're on different filesystems (cross-device),
        // rename fails with EXDEV — we fall back to copy + remove.
        //
        // UUID suffix prevents collisions when multiple concurrent tasks store
        // the same blob (e.g., background asset copy racing with rebuild).
        // Use `.pending.<uuid>` instead of `.tmp.<uuid>` — iCloud Drive excludes
        // files ending in `.tmp` from sync, and fileproviderd may remove or
        // interfere with them before the rename completes.
        // Refs: https://www.idownloadblog.com/2019/08/06/icloud-drive-file-folder-name-exclusion-list/
        //       https://eclecticlight.co/2024/07/09/excluding-folders-and-files-from-time-machine-spotlight-and-icloud-drive/
        let tmp = dest.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
        fs::copy(source, &tmp)  // allow:raw_write the temp blob this call just minted, under .moss/cache
            .map_err(|e| format!("Failed to copy to pending {}: {}", tmp.display(), e))?;

        // Post-copy validation: reject 0-byte temp files when source is
        // non-empty (e.g., fs::copy raced with iCloud materialization).
        let tmp_size = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        if source_size > 0 && tmp_size == 0 {
            // allow:unlink a temp inside the CAS shard dir, not staging
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "fs::copy produced 0-byte temp for {} (source: {} bytes)",
                source.display(),
                source_size
            ));
        }

        // allow:unlink a temp inside the CAS shard dir, not staging
        match fs::rename(&tmp, &dest) {
            Ok(()) => {}
            Err(rename_err) => {
                // Another thread may have won the race and placed the blob.
                // If dest now exists, that's success — return Ok(oid).
                if dest.exists() {
                    // allow:unlink a temp inside the CAS shard dir, not staging
                    let _ = fs::remove_file(&tmp);
                    return Ok(oid);
                }
                // Cross-device fallback: copy then remove temp.
                fs::copy(&tmp, &dest).map_err(|e| {  // allow:raw_write CAS blob under .moss/cache — cloud-excluded, dest is content-addressed
                    log::warn!("CAS store of {} failed: {}", oid, self.failure_context(&dest, Some(&tmp)));
                    format!(
                        "rename failed ({}), copy fallback also failed: {}",
                        rename_err, e
                    )
                })?;
                // allow:unlink a temp inside the CAS shard dir, not staging
                let _ = fs::remove_file(&tmp);
            }
        }

        Ok(oid)
    }

    /// Store raw bytes in the object store, returning the SHA-256 OID.
    ///
    /// Like [`store_file`](Self::store_file), the write is atomic (temp +
    /// rename) and idempotent (existing blob is kept). This variant avoids
    /// an intermediate file when the caller already has bytes in memory —
    /// e.g., a small JSON metadata blob.
    pub fn store_bytes(&self, data: &[u8]) -> Result<String, String> {
        let oid = {
            let mut hasher = Sha256::new();
            hasher.update(data);
            format!("{:x}", hasher.finalize())
        };
        let dest = self.blob_path(&oid);

        if dest.exists() {
            return Ok(oid);
        }

        if let Some(parent) = dest.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        // Use `.pending.<uuid>` — see store_file() comment for iCloud Drive rationale.
        let tmp = dest.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
        fs::write(&tmp, data)  // allow:raw_write the temp blob this call just minted, under .moss/cache
            .map_err(|e| format!("Failed to write pending {}: {}", tmp.display(), e))?;

        // allow:unlink a temp inside the CAS shard dir, not staging
        match fs::rename(&tmp, &dest) {
            Ok(()) => {}
            Err(rename_err) => {
                if dest.exists() {
                    // allow:unlink a temp inside the CAS shard dir, not staging
                    let _ = fs::remove_file(&tmp);
                    return Ok(oid);
                }
                fs::copy(&tmp, &dest).map_err(|e| {  // allow:raw_write CAS blob under .moss/cache — cloud-excluded, dest is content-addressed
                    log::warn!("CAS store of {} failed: {}", oid, self.failure_context(&dest, Some(&tmp)));
                    format!(
                        "rename failed ({}), copy fallback also failed: {}",
                        rename_err, e
                    )
                })?;
                // allow:unlink a temp inside the CAS shard dir, not staging
                let _ = fs::remove_file(&tmp);
            }
        }

        Ok(oid)
    }

    /// Return the on-disk path for a blob only if it is really an output.
    /// Providers evict file data, leaving a 0-byte stub or a dataless
    /// placeholder; `find_cached_output()` would serve those as cache hits.
    pub fn get_path(&self, oid: &str) -> Option<PathBuf> {
        let p = self.blob_path(oid);
        if crate::build::io_utils::output_present(&p) {
            return Some(p);
        }
        // A plain miss is ordinary and silent; one line per object would be a
        // cold cache's worth of noise. Only a blob that is there is logged.
        if p.exists() {
            log::warn!("[CAS] unusable blob at {}, treating as missing", oid);
        }
        None
    }

    /// A cached output the read path may use now: the blob's path for a hit,
    /// `None` for a miss the caller fills by regenerating and storing.
    ///
    /// Present ⇒ hit. In the cloud ⇒ its download is requested and waited
    /// for, bounded by the blob's size, and the arrival is hashed once ⇒ hit,
    /// or removed as corrupt ⇒ miss. Absent, or not arrived in time ⇒ miss.
    /// This is the one read that inverts "dataless is absent": a blob is the
    /// same bytes on every machine that shares the cache and carries its own
    /// checksum, so waiting for it is correct in a way waiting for `staging/`
    /// never is. The hash runs only on an arrival, never on an ordinary hit.
    pub fn ready_blob(&self, oid: &str) -> Option<PathBuf> {
        let p = self.blob_path(oid);
        if crate::build::io_utils::output_present(&p) {
            return Some(p);
        }
        if !crate::build::icloud::is_still_in_the_cloud(&p) {
            return self.get_path(oid);
        }
        let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let arrived = crate::build::cloud_readiness::retry_after_materialize(&p, download_deadline(size), || {
            Self::hash_file_once(&p)
        });
        match arrived {
            Ok(hash) if hash == oid => Some(p),
            Ok(hash) => {
                log::warn!("[CAS] blob {} arrived from the cloud hashing to {} — removed, regenerating", oid, hash);
                // allow:unlink a blob that fails its own checksum under cache/objects, not staging
                let _ = fs::remove_file(&p);
                None
            }
            Err(e) => {
                log::info!("[CAS] blob {} is in the cloud and did not arrive in time ({}) — regenerating", oid, e);
                None
            }
        }
    }

    /// Validate a stored blob's size against the expected source size.
    ///
    /// If the source was non-empty but the blob is not a usable output (a
    /// 0-byte stub, or dataless after an eviction raced `fs::copy`), it is
    /// removed so that [`store_file`](Self::store_file)
    /// can write fresh content, and an `Err` is returned.
    ///
    /// This is the single place where blob integrity is checked and
    /// self-healing happens. Called by `store_file` (idempotency check).
    ///
    /// A blob that cannot be CHECKED is not removed: an I/O error other than a
    /// positive `NotFound` says nothing about the bytes, and deleting on it
    /// takes a healthy blob every other build still links.
    pub fn validate_blob(&self, oid: &str, source_size: u64) -> Result<(), String> {
        use crate::build::io_utils::Presence;
        if source_size == 0 {
            return Ok(());
        }
        let path = self.blob_path(oid);
        match crate::build::io_utils::probe_path(&path) {
            Presence::Present => Ok(()),
            Presence::Unverified(e) => Err(format!("Unverifiable blob {}: {}", oid, e)),
            Presence::Absent | Presence::Evicted => {
                log::warn!("[CAS] unusable blob detected, removing: {}", oid);
                // allow:unlink an unusable blob under cache/objects, not staging
                let _ = fs::remove_file(&path);
                Err(format!("Unusable blob: {}", oid))
            }
        }
    }

    /// Copy a cached blob to a target path.
    ///
    /// Uses `fs::copy` instead of hardlinks. On macOS APFS, `fs::copy`
    /// calls `fclonefileat(2)` first — a COW (copy-on-write) clone that
    /// uses zero extra disk space but creates an independent inode. This
    /// follows DVC's default `reflink,copy` strategy.
    ///
    /// ## Why not hardlinks?
    ///
    /// Hardlinks share the same inode. When a cloud sync provider (iCloud,
    /// Dropbox, OneDrive, Google Drive) evicts a file to reclaim disk
    /// space, it zeroes out the data at the inode level. With hardlinks,
    /// ALL copies (cache blob, staging/, the generation) become 0 bytes
    /// simultaneously — there is no independent copy to fall back on.
    ///
    /// COW clones (via `fclonefileat`) create an independent inode that
    /// shares disk blocks with the source. If the source is evicted, the
    /// clone retains its blocks. This is the same approach OneDrive uses
    /// internally between its cache and sync root.
    ///
    /// ## Cross-platform behavior of `fs::copy`
    ///
    /// - **macOS APFS**: `fclonefileat` (COW, zero extra space) → `fcopyfile` fallback
    /// - **Linux**: `copy_file_range(2)` — COW on Btrfs/XFS, kernel copy on ext4
    /// - **Windows**: `CopyFileEx` (full copy, no COW except ReFS server-only)
    ///
    /// If `target` already exists, the new blob is written to a sibling
    /// `.tmp.<uuid>` and atomically renamed over `target`. Parent directories
    /// are created as needed.
    ///
    /// ## Atomicity (the killed-mid-link bug)
    ///
    /// The pre-atomic implementation called `fs::remove_file(target)` then
    /// `fs::copy(blob, target)`. `fs::copy` opens the destination for writing
    /// (creating it as a 0-byte file) before streaming the bytes. A process
    /// killed between the create and the final write left a 0-byte (or
    /// partial) target. The next build's per-video fast-path treated the
    /// 0-byte file as "in canonical" and never repaired it; the dispatch-level
    /// fingerprint skip never re-ran conversion. Result: a permanent loader
    /// on the affected video card until app restart.
    ///
    /// Now: copy to `<target>.tmp.<uuid>`, verify byte count, then `fs::rename`
    /// over the target. `rename` on POSIX same-filesystem is atomic and
    /// replaces an existing target. On macOS APFS same-volume `rename` is
    /// `rename(2)` and preserves the `fclonefileat` COW that `fs::copy` uses
    /// (see `fs::copy` docs).
    ///
    /// ### Tmp-sibling sweep
    ///
    /// A killed process leaks `<target>.tmp.<uuid>` instead of corrupting
    /// `<target>`. The build's permitted staging sweep removes it; sweeping
    /// here could take a concurrent `link_to`'s temp for the same target
    /// mid-rename.
    pub fn link_to(&self, oid: &str, target: &Path) -> Result<(), String> {
        let blob = self.blob_path(oid);
        if !blob.exists() {
            log::warn!("CAS link of {} failed, blob absent: {}", oid, self.failure_context(&blob, None));
            return Err(format!("Blob {} does not exist in object store", oid));
        }

        // Defense-in-depth: reject 0-byte blobs even though get_path() already
        // filters them on the cache-hit path. This protects the cache-miss path
        // (store_file → link_to) and any future callers that bypass get_path.
        let blob_size = fs::metadata(&blob).map(|m| m.len()).unwrap_or(0);
        if blob_size == 0 {
            return Err(format!("Blob {} is 0 bytes (corrupt)", oid));
        }

        // Create parent directories for the target.
        if let Some(parent) = target.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        let target_basename = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if target_basename.is_empty() {
            // `target` has no file_name (e.g. ends in `..` or is `/`). All
            // current callers pass real file paths; treat this as a programmer
            // error rather than silently skipping the sweep + writing a
            // mis-named tmp.
            return Err(format!(
                "link_to target has no file name component: {}",
                target.display()
            ));
        }

        // Atomic write: copy to a sibling tmp, then rename over the target.
        // The post-copy size check still guards iCloud short writes (the
        // original 1-MiB-of-99-MiB incident); it now removes the .tmp instead
        // of the live target on truncation.
        //
        // If `target` is a symlink, fs::rename replaces the symlink itself —
        // same effective behavior as the previous fs::remove_file(target).
        let tmp = target.with_file_name(format!(
            "{}.tmp.{}",
            target_basename,
            uuid::Uuid::new_v4()
        ));

        if let Err(e) = fs::copy(&blob, &tmp) {  // allow:raw_write the temp this call just minted, under .moss/cache
            // allow:unlink the temp this call minted beside the target; the rename replaces the entry in place
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "Failed to copy {} -> {}: {}",
                blob.display(),
                tmp.display(),
                e
            ));
        }
        // Stat the temp instead of trusting fs::copy's return value — on
        // Windows that's CopyFileEx's progress count, which Wine reports as 0.
        let copied = match fs::metadata(&tmp) {
            Ok(m) => m.len(),
            Err(e) => {
                // allow:unlink the temp this call minted beside the target; the rename replaces the entry in place
                let _ = fs::remove_file(&tmp);
                return Err(format!("Failed to stat {} after copy: {}", tmp.display(), e));
            }
        };
        if copied != blob_size {
            // allow:unlink the temp this call minted beside the target; the rename replaces the entry in place
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "Blob {} copy truncated: expected {} bytes, got {} (iCloud fault-in race?)",
                oid, blob_size, copied
            ));
        }

        // allow:unlink the temp this call minted beside the target; the rename replaces the entry in place
        if let Err(e) = fs::rename(&tmp, target) {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "Failed to rename {} -> {}: {}",
                tmp.display(),
                target.display(),
                e
            ));
        }
        Ok(())
    }

    /// Compute the sharded blob path for a given OID, using git's two-level
    /// fan-out: `base/ab/cd/<full_hash>`. OIDs are ASCII hex, so `get` always
    /// hits — it is used so a malformed OID cannot abort the build.
    pub fn blob_path(&self, oid: &str) -> PathBuf {
        let (p1, p2) = (oid.get(..2).unwrap_or(oid), oid.get(2..4).unwrap_or(oid));
        self.base.join(p1).join(p2).join(oid)
    }
}

// ---------------------------------------------------------------------------
// TransformCache
// ---------------------------------------------------------------------------

/// A record that maps a source file (by OID) to one or more transformed
/// outputs.
///
/// This is conceptually similar to **Bazel's action cache**: the key is
/// the hash of the inputs (here, the source file) and the value records
/// what each transform produced. By also storing the `params` used for
/// each transform, we can detect when the transform configuration has
/// changed and invalidate the cached output.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransformRecord {
    /// SHA-256 OID of the source file.
    pub source_oid: String,
    /// Size of the source file in bytes.
    pub source_size: u64,
    /// Map of transform name (e.g., "thumbnail", "webp") to its output
    /// entry.
    pub transforms: HashMap<String, TransformEntry>,
}

/// A single transform output within a [`TransformRecord`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransformEntry {
    /// SHA-256 OID of the output blob.
    pub oid: String,
    /// Size of the output blob in bytes.
    pub size: u64,
    /// The parameters used for this transform (e.g., target width, quality).
    /// Stored as opaque JSON so any transform can define its own config
    /// shape.
    pub params: serde_json::Value,
}

/// Transform cache — records what transforms produced what outputs.
///
/// Modeled after **Bazel's action cache**: given a source OID and a
/// transform name, the cache can tell you whether we've already
/// performed that transform with the same parameters, and if so, where
/// the output blob lives.
///
/// Records are stored as JSON files at:
///
/// ```text
/// <base>/<oid[0..2]>/<oid[2..4]>/<source_oid>.json
/// ```
pub struct TransformCache {
    /// Root directory — typically `.moss/cache/transforms/`.
    base: PathBuf,
    /// Reference to the object store, used to verify that output blobs
    /// still exist on disk.
    objects: ObjectStore,
}

impl TransformCache {
    /// Create a new `TransformCache`.
    ///
    /// - `base` — root directory for transform JSON files (e.g.,
    ///   `.moss/cache/transforms/`).
    /// - `objects` — the [`ObjectStore`] that holds the actual blobs;
    ///   needed by [`find_cached_output`](Self::find_cached_output) to
    ///   verify blob existence.
    pub fn new(base: PathBuf, objects: ObjectStore) -> Self {
        Self { base, objects }
    }

    /// The object store backing this cache's transform outputs.
    ///
    /// Exposed so callers can store a non-image blob (e.g. a small cached
    /// classification verdict, not just a converted asset) under the same
    /// content-addressed store and register it via [`put`](Self::put) —
    /// see `should_skip`'s `format-probe` cache entry.
    pub fn objects(&self) -> &ObjectStore {
        &self.objects
    }

    /// Read and deserialize a transform record for the given source OID.
    ///
    /// Returns `None` if the record file doesn't exist or can't be parsed.
    pub fn get(&self, source_oid: &str) -> Option<TransformRecord> {
        let path = self.record_path(source_oid);
        let data = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Write a transform record atomically.
    ///
    /// Like [`ObjectStore::store_file`], we write to a temp file first
    /// and then rename, so a crash can never leave a corrupt JSON file.
    ///
    /// Uses `.pending` instead of `.tmp` for the temp file — iCloud Drive
    /// excludes `.tmp` files from sync and fileproviderd may remove them.
    /// On ENOENT, retries after re-creating parent AND re-writing the temp
    /// file (the source may have been removed, not just the parent).
    pub fn put(&self, record: &TransformRecord) -> Result<(), String> {
        let path = self.record_path(&record.source_oid);

        if let Some(parent) = path.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        let json = serde_json::to_string_pretty(record)
            .map_err(|e| format!("Failed to serialize TransformRecord: {}", e))?;

        // UUID-suffixed temp so concurrent writers of the SAME key (e.g. the
        // parallel preview scan, or the background image+video workers both
        // calling save_merging) never share a temp path — otherwise two writers
        // race on one temp + a rename of a vanished file. Mirrors store_file/
        // store_bytes. `.pending` (not `.tmp`) so iCloud doesn't exclude it.
        let tmp = path.with_extension(format!("json.pending.{}", uuid::Uuid::new_v4()));
        fs::write(&tmp, json.as_bytes())  // allow:raw_write temp for the index's own atomic save, under .moss/cache
            .map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;

        // allow:unlink rename into place under cache/transforms, not staging
        if let Err(first_err) = fs::rename(&tmp, &path) {
            if first_err.kind() == std::io::ErrorKind::NotFound {
                if let Some(parent) = path.parent() {
                    let _ = crate::build::io_utils::create_output_dir_all(parent);
                }
                // Re-write temp file — it may have been removed too
                let _ = fs::write(&tmp, json.as_bytes());  // allow:raw_write temp for the index's own atomic save, under .moss/cache
                // allow:unlink rename into place under cache/transforms, not staging
                fs::rename(&tmp, &path).map_err(|e| {
                    format!(
                        "Failed to rename {} -> {} (retry after ENOENT): {}",
                        tmp.display(),
                        path.display(),
                        e
                    )
                })?;
            } else {
                return Err(format!(
                    "Failed to rename {} -> {}: {}",
                    tmp.display(),
                    path.display(),
                    first_err
                ));
            }
        }

        Ok(())
    }

    /// Remove a transform record for the given source OID.
    ///
    /// Used to evict corrupt or invalid cached outputs so the next build
    /// re-runs the transform.
    pub fn remove(&self, source_oid: &str) -> Result<(), String> {
        let path = self.record_path(source_oid);
        if path.exists() {
            // allow:unlink a transform record under cache/transforms, not staging
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
        }
        Ok(())
    }

    /// Look up a cached transform output.
    ///
    /// This is the main query entry point. It performs a **4-way check**
    /// before returning a cache hit:
    ///
    /// 1. A [`TransformRecord`] exists for `source_oid`.
    /// 2. That record contains an entry for the given `transform` name.
    /// 3. The entry's `params` match `current_params` exactly (deep
    ///    equality on `serde_json::Value`).
    /// 4. The output blob is usable now, or is in the cloud and arrives within
    ///    its deadline hashing to its OID ([`ObjectStore::ready_blob`]).
    ///
    /// If any check fails, `None` is returned and the caller should
    /// re-run the transform.
    pub fn find_cached_output(
        &self,
        source_oid: &str,
        transform: &str,
        current_params: &serde_json::Value,
    ) -> Option<String> {
        // 1. Record exists?
        let record = self.get(source_oid)?;

        // 2. Transform entry exists?
        let entry = record.transforms.get(transform)?;

        // 3. Params match?
        if entry.params != *current_params {
            return None;
        }

        // 4. Output blob usable, or arriving?
        self.objects.ready_blob(&entry.oid)?;

        Some(entry.oid.clone())
    }

    /// Compute the path where a transform record is stored on disk, under the
    /// same ASCII-hex fan-out as [`ObjectStore::blob_path`].
    fn record_path(&self, source_oid: &str) -> PathBuf {
        let oid = source_oid.replace(':', "-"); // NTFS forbids ':' in a path component (it's the ADS separator)
        self.base
            .join(oid.get(..2).unwrap_or(oid.as_str()))
            .join(oid.get(2..4).unwrap_or(oid.as_str()))
            .join(format!("{}.json", oid))
    }
}

// ---------------------------------------------------------------------------
// HashIndex
// ---------------------------------------------------------------------------

/// Cached metadata for a media file (dimensions + dominant color).
///
/// Stored as a JSON blob in the ObjectStore, keyed via TransformCache
/// with transform name `"media/meta"`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CachedMediaMeta {
    /// Video/image dimensions as `[width, height]`.
    pub dimensions: Option<(u32, u32)>,
    /// Dominant color as hex string, e.g. `"#FF5733"`.
    pub dominant_color: Option<String>,
    /// LQIP (Low Quality Image Placeholder) as a base64 data URI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lqip_data_uri: Option<String>,
    /// Whether the source is an animated gif/webp. `#[serde(default)]` so
    /// cache entries written before this field existed deserialize as
    /// `false` — a bounded, self-healing gap (the next content change
    /// re-sniffs and writes the real value); see moss#919.
    #[serde(default)]
    pub is_animated: bool,
}

/// What `stat(2)` reports about a file that can tell one version of its bytes
/// from another — the record the [`HashIndex`] keeps beside a content hash, and
/// the record a file must still show for the hash to be trusted.
///
/// The same fields, for the same reasons, as `SourceMetadata` (`build/types.rs`):
/// a whole-second mtime cannot tell the hashed file from a same-size rewrite
/// landing in the same second, ctime cannot be forged from userland where mtime
/// can, and replace-via-rename changes the inode even when size and mtime survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    /// Modification time, whole Unix seconds.
    pub mtime: u64,
    /// Sub-second part of the modification time. `None` when the platform reports
    /// no mtime, or for a stat built by [`FileStat::whole_second`].
    pub mtime_nanos: Option<u32>,
    /// Inode change time, Unix seconds, where the platform reports one.
    pub ctime: Option<i64>,
    /// Inode number, where the platform reports one.
    pub inode: Option<u64>,
}

impl FileStat {
    /// Everything the platform reports about `md`.
    pub fn of(md: &fs::Metadata) -> Self {
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
        let (ctime, inode) = crate::build::types::stat_identity(md);
        Self {
            size: md.len(),
            mtime: mtime.map_or(0, |d| d.as_secs()),
            mtime_nanos: mtime.map(|d| d.subsec_nanos()),
            ctime,
            inode,
        }
    }

    /// Size and whole-second mtime only, for a caller that has nothing finer —
    /// see [`HashIndex::lookup_whole_second`].
    pub fn whole_second(size: u64, mtime: u64) -> Self {
        Self { size, mtime, mtime_nanos: None, ctime: None, inode: None }
    }
}

#[cfg(test)]
impl FileStat {
    /// Stamp `path` with the wall-clock second `previous` carries, at a different
    /// instant inside it — which is what two writes in one second are on a
    /// filesystem with sub-second timestamps, forced instead of hoped for. Nothing
    /// sleeps. Returns the new mtime.
    pub(crate) fn stamp_in_the_second_of(path: &Path, previous: std::time::SystemTime) -> std::time::SystemTime {
        let d = previous.duration_since(std::time::UNIX_EPOCH).unwrap();
        let nanos = d.subsec_nanos();
        let other = if nanos >= 500_000_000 { nanos - 250_000_000 } else { nanos + 250_000_000 };
        let stamped = std::time::UNIX_EPOCH + std::time::Duration::new(d.as_secs(), other);
        fs::File::options().write(true).open(path).unwrap().set_modified(stamped).unwrap();
        stamped
    }

    /// Replace `path` the way an atomic save does — write the new bytes beside it and
    /// rename over — with the old mtime carried across. Size and mtime are what a
    /// record keyed by them cannot tell apart; the inode (and, a second later, the
    /// ctime) is all that does.
    pub(crate) fn replace_by_rename_keeping_mtime(path: &Path, bytes: &[u8]) {
        let before = fs::metadata(path).unwrap();
        assert_eq!(bytes.len() as u64, before.len(), "precondition: a same-size replacement");
        let beside = path.with_file_name(format!("{}.replacement", path.file_name().unwrap().to_string_lossy()));
        fs::write(&beside, bytes).unwrap();
        fs::File::options().write(true).open(&beside).unwrap().set_modified(before.modified().unwrap()).unwrap();
        fs::rename(&beside, path).unwrap();
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), before.modified().unwrap());
    }

    /// This record with each field changed in turn: what a record taken at another
    /// instant of the file can differ in. A consumer that leaves one out of the stat
    /// it looks up (or records) trusts the hash of bytes the file no longer has.
    /// ctime and inode are left out where the platform has none, since an absent
    /// field agrees with anything.
    pub(crate) fn each_field_changed(&self) -> Vec<(&'static str, FileStat)> {
        let mut rows = vec![
            ("size", FileStat { size: self.size + 1, ..*self }),
            ("mtime", FileStat { mtime: self.mtime + 1, ..*self }),
            ("sub-second mtime", FileStat { mtime_nanos: self.mtime_nanos.map(|n| (n + 250_000_000) % 1_000_000_000), ..*self }),
        ];
        if let Some(c) = self.ctime {
            rows.push(("ctime", FileStat { ctime: Some(c - 7), ..*self }));
        }
        if let Some(i) = self.inode {
            rows.push(("inode", FileStat { inode: Some(i + 1), ..*self }));
        }
        rows
    }
}

/// A single entry in the hash index: the stat record a file had when its bytes
/// were hashed, and the hash.
///
/// Only `size`, `mtime` and `content_hash` existed in the first format. The
/// rest are `#[serde(default)]` so an index written by that version still loads
/// — its entries simply never match a full-stat lookup, and are rewritten the
/// next time the file is hashed (see [`HashIndex::lookup`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HashIndexEntry {
    /// File size in bytes (corresponds to `st_size`).
    pub size: u64,
    /// Modification time as Unix epoch seconds (corresponds to `st_mtime`).
    pub mtime: u64,
    /// Sub-second part of the modification time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_nanos: Option<u32>,
    /// Inode change time as Unix seconds (`st_ctime`), where the platform has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctime: Option<i64>,
    /// Inode number (`st_ino`), where the platform has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<u64>,
    /// SHA-256 content hash (hex, 64 chars).
    pub content_hash: String,
}

/// Hash index: avoids re-hashing unchanged files.
///
/// This is the same optimization Git uses in `ce_match_stat()` to avoid
/// re-computing SHA-1 hashes for unchanged files during `git status`:
///
///   Git's approach (documented in Documentation/technical/racy-git.txt):
///   1. The index stores (path, stat fields, content hash) for each file
///   2. On `git status`, Git calls lstat(2) and compares stat fields
///      (st_size, st_mtime, st_ctime, st_ino, st_uid, st_gid, st_dev)
///   3. If stat fields match: trust the cached hash (skip reading file)
///   4. If stat fields differ: re-read file, compute new hash, update index
///
///   The "racy-git" edge case: if a file is modified within the same
///   second as the index write, st_mtime matches but content may differ.
///   Git handles this by comparing actual content for entries whose mtime
///   equals the index mtime (see racy-git.txt for details).
///
/// Our version:
///   - Key: (relative_path) → the file's [`FileStat`] + `content_hash`
///   - [`lookup`](Self::lookup) trusts `content_hash` only while size, mtime to the
///     nanosecond, ctime and inode all still match, and fails OPEN — a field
///     either side lacks is a miss, and a miss costs one hash — never to a hit
///   - [`lookup_whole_second`](Self::lookup_whole_second) is the older, weaker
///     rule (size + whole-second mtime), kept for the one caller that cannot hash
///     on a miss: the video path
///
/// The racy-clean edge case is NOT acceptable for an image: its content hash
/// names the encode, so a false hit ships the previous picture's variant. A
/// same-second rewrite is told apart by the sub-second mtime (the filesystems
/// moss runs on report one); on a filesystem whose timestamps are whole seconds
/// the comparison is no weaker than it used to be, and no stronger.
///
/// Reference: <https://git-scm.com/docs/racy-git>
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HashIndex {
    /// Map from relative file path to its cached stat+hash entry.
    pub entries: HashMap<String, HashIndexEntry>,
}

impl HashIndex {
    /// Create an empty hash index.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Load a hash index for a reader that must not act on a blind read:
    /// `Ok(None)` when the file does not exist, `Err` when it exists but cannot
    /// be read or parsed (`InvalidData`). [`load`](Self::load)'s empty-on-error
    /// is right for a worker — it costs a re-hash — and wrong for the GC mark
    /// phase, where an empty index marks nothing live.
    pub fn load_strict(path: &Path) -> std::io::Result<Option<Self>> {
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        serde_json::from_str(&data)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Load a hash index from a JSON file on disk.
    ///
    /// Returns an empty index if the file doesn't exist or can't be parsed.
    pub fn load(path: &Path) -> Self {
        let data = match fs::read_to_string(path) {
            Ok(d) => d,
            Err(_) => return Self::new(),
        };
        serde_json::from_str(&data).unwrap_or_else(|_| Self::new())
    }

    /// Save the hash index to a JSON file on disk (atomic write).
    ///
    /// Uses `.pending` instead of `.tmp` for the temp file — iCloud Drive
    /// excludes `.tmp` files from sync and fileproviderd may remove them.
    /// Refs: https://www.idownloadblog.com/2019/08/06/icloud-drive-file-folder-name-exclusion-list/
    ///       https://eclecticlight.co/2024/07/09/excluding-folders-and-files-from-time-machine-spotlight-and-icloud-drive/
    ///
    /// On ENOENT, retries after re-creating parent AND re-writing the temp
    /// file (the source may have been removed, not just the parent).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            crate::build::io_utils::create_output_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize HashIndex: {}", e))?;

        // UUID-suffixed temp so concurrent writers of the SAME key (e.g. the
        // parallel preview scan, or the background image+video workers both
        // calling save_merging) never share a temp path — otherwise two writers
        // race on one temp + a rename of a vanished file. Mirrors store_file/
        // store_bytes. `.pending` (not `.tmp`) so iCloud doesn't exclude it.
        let tmp = path.with_extension(format!("json.pending.{}", uuid::Uuid::new_v4()));
        fs::write(&tmp, json.as_bytes())  // allow:raw_write temp for the index's own atomic save, under .moss/cache
            .map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;

        // Rename is atomic on the same filesystem. No cross-device fallback needed
        // because tmp and target share the same parent directory (.moss/build.nosync/cache/).
        // allow:unlink rename into place for cache/hash-index.json, not staging
        if let Err(first_err) = fs::rename(&tmp, path) {
            if first_err.kind() == std::io::ErrorKind::NotFound {
                if let Some(parent) = path.parent() {
                    let _ = crate::build::io_utils::create_output_dir_all(parent);
                }
                // Re-write temp file — it may have been removed too
                let _ = fs::write(&tmp, json.as_bytes());  // allow:raw_write temp for the index's own atomic save, under .moss/cache
                // allow:unlink rename into place for cache/hash-index.json, not staging
                fs::rename(&tmp, path).map_err(|e| {
                    format!(
                        "Failed to rename {} -> {} (retry after ENOENT): {}",
                        tmp.display(),
                        path.display(),
                        e
                    )
                })?;
            } else {
                return Err(format!(
                    "Failed to rename {} -> {}: {}",
                    tmp.display(),
                    path.display(),
                    first_err
                ));
            }
        }

        Ok(())
    }

    /// Save, MERGING with the current on-disk index rather than overwriting it.
    ///
    /// The background image and video workers run concurrently and each hold
    /// only their own category of entries (images vs videos, on disjoint keys).
    /// A plain `save` (full-file overwrite) would let whichever worker writes
    /// last clobber the other's freshly-added entries, forcing a full re-hash of
    /// that category on the next build. `save_merging` re-reads the current
    /// on-disk entries and layers `self`'s on top (self wins on key collision),
    /// so neither worker loses the other's contribution. Scan output keeps using
    /// plain `save` (it is authoritative and prunes stale entries).
    pub fn save_merging(&self, path: &Path) -> Result<(), String> {
        let mut merged = Self::load(path);
        for (key, entry) in &self.entries {
            merged.entries.insert(key.clone(), entry.clone());
        }
        merged.save(path)
    }

    /// The content hash recorded for `relative_path`, if the file still shows the
    /// stat record it was hashed at.
    ///
    /// Fails open. A field that is `Some` on both sides and differs is a miss (the
    /// rule `SourceMetadata` uses), and so is a missing sub-second mtime on either
    /// side — an entry from before the field existed, one recorded by
    /// [`update_whole_second`](Self::update_whole_second), a file whose mtime the
    /// platform will not report: a whole-second match cannot rule out a same-size
    /// rewrite in the same second. A miss costs one hash; a hit that should have
    /// missed is a stale image.
    pub fn lookup(&self, relative_path: &str, stat: &FileStat) -> Option<&str> {
        let entry = self.entries.get(relative_path)?;
        let same_instant = stat.mtime_nanos.is_some() && entry.mtime_nanos == stat.mtime_nanos;
        count_hit(
            (entry.size == stat.size
                && entry.mtime == stat.mtime
                && same_instant
                && !identity_disagrees(entry.ctime, stat.ctime)
                && !identity_disagrees(entry.inode, stat.inode))
            .then_some(entry.content_hash.as_str()),
        )
    }

    /// [`lookup`](Self::lookup)'s older rule: size and whole-second mtime only,
    /// blind to a same-size rewrite in the same second.
    ///
    /// For the video path, which cannot afford the full-stat rule: the render thread
    /// hashes no multi-GB source (`HashPolicy::StatOnly`), and a stricter worker would
    /// rehash every video on any ctime or inode change (a cloud provider
    /// re-materializing it) and on the first build after this rule changed. Callers
    /// record with [`update_whole_second`](Self::update_whole_second), so what they
    /// leave in the index is never mistaken for a full stat record.
    pub fn lookup_whole_second(&self, relative_path: &str, size: u64, mtime: u64) -> Option<&str> {
        let entry = self.entries.get(relative_path)?;
        count_hit((entry.size == size && entry.mtime == mtime).then_some(entry.content_hash.as_str()))
    }

    /// Record `content_hash` for a file as it stood at `stat`.
    ///
    /// `stat` must be taken BEFORE the bytes are read: a write landing during the
    /// hash then leaves an entry the file no longer matches, where the other order
    /// would pair the new stat with the old bytes' hash and vouch for it.
    pub fn update(&mut self, relative_path: String, stat: &FileStat, content_hash: String) {
        REHASHED.fetch_add(1, Ordering::Relaxed);
        self.entries.insert(
            relative_path,
            HashIndexEntry {
                size: stat.size,
                mtime: stat.mtime,
                mtime_nanos: stat.mtime_nanos,
                ctime: stat.ctime,
                inode: stat.inode,
                content_hash,
            },
        );
    }

    /// [`update`](Self::update) for a caller that only has size and whole-second
    /// mtime: the entry carries no sub-second field, so [`lookup`](Self::lookup)
    /// will never trust it.
    pub fn update_whole_second(&mut self, relative_path: String, size: u64, mtime: u64, content_hash: String) {
        self.update(relative_path, &FileStat::whole_second(size, mtime), content_hash);
    }

    /// Copy `relative_path`'s entry from `previous` exactly as it was recorded.
    ///
    /// For carrying a hit forward into a fresh index. Copying, rather than
    /// re-recording the hit under the file's current stat, is what keeps the record
    /// honest: a hit that came from [`lookup_whole_second`](Self::lookup_whole_second)
    /// could be a stale one, and stamping it with today's stat would turn it into an
    /// entry [`lookup`](Self::lookup) trusts. That is not only a video's concern: the
    /// scan answers an evicted image by the whole-second rule too, and its hit may be
    /// a strict entry that only ctime and inode disagree with.
    pub fn carry_forward(&mut self, previous: &HashIndex, relative_path: &str) {
        if let Some(entry) = previous.entries.get(relative_path) {
            self.entries.insert(relative_path.to_string(), entry.clone());
        }
    }

    /// The content hash of `file`: the recorded one while [`lookup`](Self::lookup)
    /// trusts it, otherwise hashed now and recorded.
    ///
    /// Never reads a file that is still in the cloud — that is an `Err`. The read
    /// blocks for the provider's whole download or fails, and a re-materialised file
    /// has a new ctime and inode, so the strict lookup misses on exactly the files
    /// the old size-and-second key answered without one. A hit still answers.
    pub fn resolve(&mut self, file: &Path, relative_path: &str) -> Result<String, String> {
        self.resolve_with(file, relative_path, ObjectStore::hash_file, crate::build::icloud::is_still_in_the_cloud)
    }

    /// [`resolve`](Self::resolve) with its two effects passed in: a test can hash
    /// without a real file and put one in the cloud, and a caller that reads the bytes
    /// itself (the parse cache) still gets the lookup, the guard and the record.
    pub(crate) fn resolve_with(
        &mut self,
        file: &Path,
        relative_path: &str,
        hash_file: impl FnOnce(&Path) -> Result<String, String>,
        in_the_cloud: impl FnOnce(&Path) -> bool,
    ) -> Result<String, String> {
        // Stat before anything is read: see `update`.
        let stat = FileStat::of(
            &fs::metadata(file).map_err(|e| format!("Failed to stat {}: {}", file.display(), e))?,
        );
        if let Some(hash) = self.lookup(relative_path, &stat) {
            return Ok(hash.to_string());
        }
        self.hash_and_record(&stat, file, relative_path, hash_file, in_the_cloud)
    }

    /// [`resolve`](Self::resolve) for a scan that builds a new index beside the last
    /// one, at the stat its walk took: what `previous` still vouches for is carried
    /// across as recorded (see [`carry_forward`](Self::carry_forward)), the rest is
    /// hashed and recorded here.
    pub(crate) fn resolve_from(
        &mut self,
        previous: &HashIndex,
        stat: &FileStat,
        file: &Path,
        relative_path: &str,
    ) -> Result<String, String> {
        if let Some(hash) = previous.lookup(relative_path, stat) {
            let hash = hash.to_string();
            self.carry_forward(previous, relative_path);
            return Ok(hash);
        }
        self.hash_and_record(stat, file, relative_path, ObjectStore::hash_file, crate::build::icloud::is_still_in_the_cloud)
    }

    /// What a miss comes to: no read of a file in the cloud, else its hash, recorded at
    /// the stat taken before the read.
    fn hash_and_record(
        &mut self,
        stat: &FileStat,
        file: &Path,
        relative_path: &str,
        hash_file: impl FnOnce(&Path) -> Result<String, String>,
        in_the_cloud: impl FnOnce(&Path) -> bool,
    ) -> Result<String, String> {
        if in_the_cloud(file) {
            return Err(format!("{} is still in the cloud; not reading it to hash it", file.display()));
        }
        let hash = hash_file(file)?;
        self.update(relative_path.to_string(), stat, hash.clone());
        Ok(hash)
    }
}

// The index's answers, process-wide, for the one line a build prints about it: a build
// is served by a dozen short-lived indexes and what they saved spans all of them. Every
// path that hashes for want of a hit records the result through `update`, so that is
// what a rehash is.
static HITS: AtomicU64 = AtomicU64::new(0);
static REHASHED: AtomicU64 = AtomicU64::new(0);

fn count_hit<T>(hit: Option<T>) -> Option<T> {
    if hit.is_some() {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
    hit
}

/// Print, at `info`, what the index did since the last call, and start over.
pub fn report_hash_index_activity() {
    let (hits, rehashed) = (HITS.swap(0, Ordering::Relaxed), REHASHED.swap(0, Ordering::Relaxed));
    let line = format!("[cache] hash-index: {hits} hits, {rehashed} rehashed");
    #[cfg(test)]
    LAST_LINE.with(|last| *last.borrow_mut() = Some(line.clone()));
    log::info!("{line}");
}

// The line the last `report_hash_index_activity` on this thread printed.
#[cfg(test)]
thread_local! {
    pub(crate) static LAST_LINE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Singleflight — Go's x/sync/singleflight for Rust (ADR-010, Phase 4)
// ---------------------------------------------------------------------------

/// Dedup concurrent work on the same key (Go's x/sync/singleflight).
///
/// When multiple callers request work for the same key concurrently, only the
/// first executes the work. All others wait and receive a clone of the result.
///
/// # Why tokio::sync::watch over Condvar alone
///
/// One channel type for both sync and async callers. `watch::Receiver::changed()`
/// can be `.await`'d in async code. For sync callers (`do_work`), a thin
/// `Condvar` bridge in `FlightSlot` provides blocking notification. This lets
/// plugin hooks (async) and video conversion (sync) share the same dedup
/// primitive without requiring two separate implementations.
///
/// # API overview
///
/// Two usage patterns:
///
/// 1. **`try_start` / `complete` / `abort`** — for callers that need to own
///    the work execution (sync or async). The caller checks `is_first`, does
///    the work, then calls `complete` or `abort`.
///
/// 2. **`do_work`** — convenience wrapper for sync callers. Takes a closure,
///    handles `complete`/`abort` automatically (including on panic).
///
/// # Why try_start/complete over do_work with async closure
///
/// Rust stable doesn't have `async FnOnce`. Separating the guard (`try_start`)
/// from the work execution lets the caller own the work (sync or async).
/// Trade-off: caller must remember to call `complete`/`abort` — the `do_work`
/// wrapper eliminates this risk for sync callers.
///
/// # Panic safety
///
/// `do_work` calls `abort` if the closure panics, so all waiters receive `None`.
/// When using `try_start` directly, the caller is responsible for calling
/// `abort` on failure paths.
pub struct Singleflight<V: Clone + Send + Sync + 'static> {
    in_flight: std::sync::Mutex<HashMap<String, std::sync::Arc<FlightSlot<V>>>>,
}

/// Internal slot combining a `watch` channel (for async waiters) with a
/// `Condvar` (for sync waiters in `do_work`). Both are signaled on
/// `complete`/`abort`.
struct FlightSlot<V> {
    /// Watch channel sender — async callers subscribe via `tx.subscribe()`.
    tx: tokio::sync::watch::Sender<Option<V>>,
    /// Condvar for sync waiters. Paired with `done` mutex.
    condvar: std::sync::Condvar,
    /// `(done, result)` — set by `complete`/`abort`, read by sync waiters.
    state: std::sync::Mutex<(bool, Option<V>)>,
}

impl<V: Clone + Send + Sync + 'static> Singleflight<V> {
    pub fn new() -> Self {
        Self {
            in_flight: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Start work for `key`, or join an existing in-flight execution.
    ///
    /// Returns `(is_first, rx)`:
    /// - `is_first = true`: this caller is responsible for doing the work
    ///   and must call [`complete`](Self::complete) or [`abort`](Self::abort).
    /// - `is_first = false`: another caller is already doing the work.
    ///   Wait on `rx` via `.changed().await` in async code.
    pub fn try_start(&self, key: &str) -> (bool, tokio::sync::watch::Receiver<Option<V>>) {
        let mut map = self.in_flight.lock().unwrap();
        if let Some(slot) = map.get(key) {
            // Already in-flight — subscribe and let the caller wait.
            (false, slot.tx.subscribe())
        } else {
            // First caller — create a new watch channel + condvar.
            let (tx, rx) = tokio::sync::watch::channel(None);
            let slot = std::sync::Arc::new(FlightSlot {
                tx,
                condvar: std::sync::Condvar::new(),
                state: std::sync::Mutex::new((false, None)),
            });
            map.insert(key.to_string(), slot);
            (true, rx)
        }
    }

    /// Signal successful completion, delivering `value` to all waiters.
    ///
    /// Sends `Some(value)` through the watch channel (async waiters) and
    /// notifies the condvar (sync waiters), then removes the key from the
    /// in-flight map.
    pub fn complete(&self, key: &str, value: V) {
        let slot = {
            let mut map = self.in_flight.lock().unwrap();
            map.remove(key)
        };
        if let Some(slot) = slot {
            // Signal async waiters via watch channel.
            let _ = slot.tx.send(Some(value.clone()));
            // Signal sync waiters via condvar.
            {
                let mut guard = slot.state.lock().unwrap();
                *guard = (true, Some(value));
            }
            slot.condvar.notify_all();
        }
    }

    /// Signal failure/cancellation, waking all waiters with `None`.
    ///
    /// Sends `None` through the watch channel and notifies the condvar,
    /// then removes the key from the in-flight map.
    pub fn abort(&self, key: &str) {
        let slot = {
            let mut map = self.in_flight.lock().unwrap();
            map.remove(key)
        };
        if let Some(slot) = slot {
            // Re-send None to trigger changed() for async waiters.
            let _ = slot.tx.send(None);
            // Signal sync waiters via condvar.
            {
                let mut guard = slot.state.lock().unwrap();
                *guard = (true, None);
            }
            slot.condvar.notify_all();
        }
    }

    /// Execute `work_fn` for `key`, or wait for an existing execution.
    ///
    /// Convenience wrapper around `try_start`/`complete`/`abort` for sync
    /// callers. Handles panic safety: if `work_fn` panics, `abort` is called
    /// so waiters receive `None` instead of hanging.
    ///
    /// Returns `(result, shared)` where:
    /// - `result` is `Some(value)` on success, `None` if the work fn panicked
    /// - `shared` is `true` if this caller waited for another's result
    pub fn do_work<F>(&self, key: &str, work_fn: F) -> (Option<V>, bool)
    where
        F: FnOnce() -> V,
    {
        // Check if already in-flight. For sync waiters we use the Condvar
        // inside FlightSlot rather than the watch channel (watch::Receiver
        // only has async changed()).
        let slot = {
            let mut map = self.in_flight.lock().unwrap();
            if let Some(existing) = map.get(key) {
                let existing = existing.clone();
                drop(map);
                // Block on the condvar until the first caller finishes.
                let mut guard = existing.state.lock().unwrap();
                while !guard.0 {
                    guard = existing.condvar.wait(guard).unwrap();
                }
                return (guard.1.clone(), true);
            }
            // Register as in-flight.
            let (tx, _rx) = tokio::sync::watch::channel(None);
            let slot = std::sync::Arc::new(FlightSlot {
                tx,
                condvar: std::sync::Condvar::new(),
                state: std::sync::Mutex::new((false, None)),
            });
            map.insert(key.to_string(), slot.clone());
            slot
        };

        // Execute work function outside any lock.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work_fn));

        match result {
            Ok(value) => {
                // Signal both async and sync waiters, then remove key.
                let _ = slot.tx.send(Some(value.clone()));
                {
                    let mut guard = slot.state.lock().unwrap();
                    *guard = (true, Some(value.clone()));
                }
                slot.condvar.notify_all();
                if let Ok(mut map) = self.in_flight.lock() {
                    map.remove(key);
                }
                (Some(value), false)
            }
            Err(_panic) => {
                // Panic safety: abort so waiters get None instead of hanging.
                let _ = slot.tx.send(None);
                {
                    let mut guard = slot.state.lock().unwrap();
                    *guard = (true, None);
                }
                slot.condvar.notify_all();
                if let Ok(mut map) = self.in_flight.lock() {
                    map.remove(key);
                }
                (None, false)
            }
        }
    }

    /// Clear all in-flight entries, waking all waiters with `None`.
    ///
    /// Used during rebuild cancellation to allow re-dispatch of previously
    /// in-flight work items. Notifies both async waiters (via watch channel)
    /// and sync waiters (via condvar) before removing entries.
    pub fn clear(&self) {
        let entries: Vec<std::sync::Arc<FlightSlot<V>>> = if let Ok(mut map) = self.in_flight.lock() {
            map.drain().map(|(_, v)| v).collect()
        } else {
            return;
        };
        // Wake all waiters so they don't hang.
        for slot in entries {
            let _ = slot.tx.send(None);
            {
                let mut guard = slot.state.lock().unwrap();
                *guard = (true, None);
            }
            slot.condvar.notify_all();
        }
    }

}

impl<V: Clone + Send + Sync + 'static> Default for Singleflight<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that calls [`Singleflight::abort`] when dropped while armed.
///
/// Prevents singleflight key leaks when a future is cancelled (e.g., by
/// `tokio::time::timeout`) before the caller can call `complete()` or
/// `abort()`. Create after `try_start` returns `is_first=true`, then
/// disarm before calling `complete()`.
///
/// # Usage
/// ```rust,ignore
/// let (is_first, rx) = sf.try_start("key");
/// if is_first {
///     let mut guard = SingleflightGuard::new(&sf, "key".to_string());
///     // ... do work ...
///     guard.disarm();
///     sf.complete("key", result);
/// }
/// ```
pub struct SingleflightGuard<'a, V: Clone + Send + Sync + 'static> {
    singleflight: &'a Singleflight<V>,
    key: String,
    armed: bool,
}

impl<'a, V: Clone + Send + Sync + 'static> SingleflightGuard<'a, V> {
    pub fn new(singleflight: &'a Singleflight<V>, key: String) -> Self {
        Self {
            singleflight,
            key,
            armed: true,
        }
    }

    /// Disarm the guard so it does NOT abort on drop.
    /// Call this right before `complete()`.
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<V: Clone + Send + Sync + 'static> Drop for SingleflightGuard<'_, V> {
    fn drop(&mut self) {
        if self.armed {
            self.singleflight.abort(&self.key);
        }
    }
}

// ---------------------------------------------------------------------------
// Garbage Collection — mark-and-sweep for orphaned cache entries
// ---------------------------------------------------------------------------

/// Result of a garbage collection run.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GcResult {
    /// Number of TransformCache records removed (orphaned source OIDs).
    pub transforms_removed: usize,
    /// Number of ObjectStore blobs removed (unreferenced OIDs).
    pub objects_removed: usize,
    /// Total bytes freed from the object store.
    pub bytes_freed: u64,
}

/// When each object store in this process was last swept, and how much it
/// removed — read back only by failure forensics, so a missing blob's log line
/// can say whether a sweep ran just before it went missing.
static LAST_GC: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, (std::time::Instant, usize)>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// How long to wait for a blob the cloud holds: ten seconds plus one per MiB,
/// capped at ten minutes. A video-sized blob's download wins by construction
/// against a minutes-long re-encode; a small blob resolves either way in
/// seconds.
fn download_deadline(size: u64) -> std::time::Duration {
    let per_mib = std::time::Duration::from_secs(size / (1024 * 1024));
    (std::time::Duration::from_secs(10) + per_mib).min(std::time::Duration::from_secs(600))
}

fn record_gc(objects_dir: &Path, objects_removed: usize) {
    if let Ok(mut map) = LAST_GC.lock() {
        map.insert(objects_dir.to_path_buf(), (std::time::Instant::now(), objects_removed));
    }
}

fn last_gc_summary(objects_dir: &Path) -> String {
    match LAST_GC.lock().ok().and_then(|m| m.get(objects_dir).copied()) {
        Some((at, removed)) => format!("last GC {} s ago removed {}", at.elapsed().as_secs(), removed),
        None => "no GC in this process".to_string(),
    }
}

/// Run mark-and-sweep garbage collection on the cache.
///
/// The whole mark phase runs before anything is deleted:
///
/// 0. **Load the HashIndex** to determine the source OIDs live on this machine.
/// 1. **Mark transform records**: every record contributes its output OIDs. A
///    record whose source is not live here is condemned only once it is older
///    than [`RECORD_TTL`]: the cache is shared by every machine that opens the
///    folder, and a source this machine never scanned is live on the one that
///    stored the record. This machine cannot tell, so until then it keeps it.
/// 2. **Mark `hashes.json`**'s file hashes.
///
/// Then the sweep deletes the condemned records and every blob no mark reached.
///
/// Every mark input must be READABLE. A missing index or `hashes.json` is an
/// answer (nothing live from it); an index, record, shard directory or
/// `hashes.json` that exists and cannot be read or parsed is not, because it
/// marks less and so deletes more — one unreadable `hash-index.json` makes
/// every blob garbage. Any such error aborts with nothing deleted.
///
/// ## Arguments
///
/// - `mp` — names the store (`cache_objects`, `cache_transforms`) and the two
///   per-machine mark inputs (`cache_hash_index`, `hashes`).
///
/// `_token` proves no build or detached encode of this folder holds a cache
/// lease (`lifecycle::try_begin_cache_gc`): a writer stores its blobs and
/// transform records before the hash index that marks them live is saved, so a
/// sweep beside one deletes what it just wrote.
pub(crate) fn gc(mp: &crate::moss_paths::MossPaths, _token: &crate::build::lifecycle::CacheGcToken) -> Result<GcResult, String> {
    let objects_dir = mp.cache_objects();
    let transforms_dir = mp.cache_transforms();
    let hash_index_path = mp.cache_hash_index();
    let hashes_path = mp.hashes();
    let unreadable = |input: &Path, e: std::io::Error| format!("{} unreadable ({})", input.display(), e);

    // Phase 0: the HashIndex names the live source OIDs.
    let hash_index = HashIndex::load_strict(&hash_index_path)
        .map_err(|e| unreadable(&hash_index_path, e))?
        .unwrap_or_else(HashIndex::new);
    let live_source_oids: std::collections::HashSet<&str> = hash_index
        .entries
        .values()
        .map(|e| e.content_hash.as_str())
        .collect();

    // Phase 1: mark transform records. Source OIDs are live themselves (they
    // may be stored in objects/); a live record's outputs are live.
    let mut referenced_oids: std::collections::HashSet<String> =
        live_source_oids.iter().map(|oid| (*oid).to_string()).collect();
    let mut condemned_records: Vec<PathBuf> = Vec::new();
    let mut shard_dirs: Vec<PathBuf> = Vec::new();
    // transforms/ab/cd/<source_oid>.json
    for prefix1 in read_dir_strict(&transforms_dir).map_err(|e| unreadable(&transforms_dir, e))? {
        let p1 = transforms_dir.join(&prefix1);
        if !p1.is_dir() {
            continue;
        }
        for prefix2 in read_dir_strict(&p1).map_err(|e| unreadable(&p1, e))? {
            let p2 = p1.join(&prefix2);
            if !p2.is_dir() {
                continue;
            }
            for filename in read_dir_strict(&p2).map_err(|e| unreadable(&p2, e))? {
                let file_path = p2.join(&filename);
                if !file_path.is_file() {
                    continue;
                }
                let Some(source_oid) = filename.strip_suffix(".json") else {
                    continue; // not a transform record
                };
                if !live_source_oids.contains(source_oid) && older_than(&file_path, RECORD_TTL) {
                    condemned_records.push(file_path);
                    continue;
                }
                let data = fs::read_to_string(&file_path).map_err(|e| unreadable(&file_path, e))?;
                let record = serde_json::from_str::<TransformRecord>(&data).map_err(|e| {
                    unreadable(&file_path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })?;
                // A surviving record keeps everything it names, its source
                // included: the machine that stored it may hold that source
                // only here.
                referenced_oids.insert(record.source_oid);
                referenced_oids.extend(record.transforms.into_values().map(|entry| entry.oid));
            }
            shard_dirs.push(p2);
        }
        shard_dirs.push(p1);
    }

    // Phase 2: mark the output hashes `hashes.json` names (they may be blobs).
    match fs::read_to_string(&hashes_path) {
        Ok(content) => {
            let site_hashes = serde_json::from_str::<serde_json::Value>(&content).map_err(|e| {
                unreadable(&hashes_path, std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
            if let Some(files) = site_hashes.get("files").and_then(|f| f.as_object()) {
                referenced_oids.extend(files.values().filter_map(|h| h.as_str()).map(str::to_string));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(unreadable(&hashes_path, e)),
    }

    // Sweep: the condemned records, their emptied shard directories (a
    // `remove_dir` succeeds only on an empty one), then unreferenced blobs.
    let mut transforms_removed: usize = 0;
    for record in &condemned_records {
        // allow:unlink cache/objects and cache/transforms, not staging
        if fs::remove_file(record).is_ok() {
            transforms_removed += 1;
        }
    }
    for dir in &shard_dirs {
        // allow:unlink cache/objects and cache/transforms, not staging
        let _ = fs::remove_dir(dir);
    }

    let mut objects_removed: usize = 0;
    let mut bytes_freed: u64 = 0;

    if objects_dir.is_dir() {
        // Walk the sharded directory: objects/ab/cd/<full_hash>
        for prefix1 in read_dir_entries(&objects_dir) {
            let p1 = objects_dir.join(&prefix1);
            if !p1.is_dir() {
                continue;
            }
            for prefix2 in read_dir_entries(&p1) {
                let p2 = p1.join(&prefix2);
                if !p2.is_dir() {
                    continue;
                }
                for filename in read_dir_entries(&p2) {
                    let file_path = p2.join(&filename);
                    if !file_path.is_file() {
                        continue;
                    }
                    // Skip temp files (.pending.*)
                    if filename.contains(".pending.") {
                        continue;
                    }
                    // The filename IS the OID (no extension)
                    if !referenced_oids.contains(filename.as_str()) {
                        // Unreferenced blob — delete it
                        let file_size = fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                        // allow:unlink cache/objects and cache/transforms, not staging
                        if fs::remove_file(&file_path).is_ok() {
                            objects_removed += 1;
                            bytes_freed += file_size;
                        }
                    }
                }
                // Clean up empty prefix2 directories
                // allow:unlink cache/objects and cache/transforms, not staging
                let _ = fs::remove_dir(&p2);
            }
            // Clean up empty prefix1 directories
            // allow:unlink cache/objects and cache/transforms, not staging
            let _ = fs::remove_dir(&p1);
        }
    }

    record_gc(&objects_dir, objects_removed);
    Ok(GcResult {
        transforms_removed,
        objects_removed,
        bytes_freed,
    })
}

/// How long a transform record nothing on this machine references is kept
/// before GC condemns it. Ninety days bounds the cost of another machine's
/// record being condemned here — one regeneration per record per quarter,
/// paid on that machine — without letting records for deleted sources live
/// forever.
pub(crate) const RECORD_TTL: std::time::Duration = std::time::Duration::from_secs(90 * 24 * 60 * 60);

/// Whether `path` was last written more than `ttl` ago. An unreadable or
/// future mtime answers no: condemning on a guess is what the TTL exists to
/// prevent.
fn older_than(path: &Path, ttl: std::time::Duration) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|written| written.elapsed().ok())
        .is_some_and(|age| age > ttl)
}

/// Directory entries for a GC mark input: an absent directory has none, an
/// unreadable one is an error.
fn read_dir_strict(dir: &Path) -> std::io::Result<Vec<String>> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Read directory entries as a Vec of filenames (strings).
/// Returns an empty vec if the directory can't be read.
fn read_dir_entries(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
