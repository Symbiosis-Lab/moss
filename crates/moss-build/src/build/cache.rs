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
//! .moss/build/cache/
//! ├── objects/          # ObjectStore — raw blobs keyed by SHA-256
//! │   └── ab/cd/<full_hash>
//! └── transforms/       # TransformCache — JSON records keyed by source hash
//!     └── ab/cd/<source_hash>.json
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

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
    /// Root directory — typically `.moss/build/cache/objects/`.
    base: PathBuf,
}

impl ObjectStore {
    /// Create a new `ObjectStore` rooted at `base`.
    ///
    /// `base` is typically `.moss/build/cache/objects/`. The directory is created
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
            fs::create_dir_all(parent)
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
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "fs::copy produced 0-byte temp for {} (source: {} bytes)",
                source.display(),
                source_size
            ));
        }

        match fs::rename(&tmp, &dest) {
            Ok(()) => {}
            Err(rename_err) => {
                // Another thread may have won the race and placed the blob.
                // If dest now exists, that's success — return Ok(oid).
                if dest.exists() {
                    let _ = fs::remove_file(&tmp);
                    return Ok(oid);
                }
                // Cross-device fallback: copy then remove temp.
                fs::copy(&tmp, &dest).map_err(|e| {  // allow:raw_write CAS blob under .moss/cache — cloud-excluded, dest is content-addressed
                    format!(
                        "rename failed ({}), copy fallback also failed: {}",
                        rename_err, e
                    )
                })?;
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
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        // Use `.pending.<uuid>` — see store_file() comment for iCloud Drive rationale.
        let tmp = dest.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
        fs::write(&tmp, data)  // allow:raw_write the temp blob this call just minted, under .moss/cache
            .map_err(|e| format!("Failed to write pending {}: {}", tmp.display(), e))?;

        match fs::rename(&tmp, &dest) {
            Ok(()) => {}
            Err(rename_err) => {
                if dest.exists() {
                    let _ = fs::remove_file(&tmp);
                    return Ok(oid);
                }
                fs::copy(&tmp, &dest).map_err(|e| {  // allow:raw_write CAS blob under .moss/cache — cloud-excluded, dest is content-addressed
                    format!(
                        "rename failed ({}), copy fallback also failed: {}",
                        rename_err, e
                    )
                })?;
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

    /// Validate a stored blob's size against the expected source size.
    ///
    /// If the source was non-empty but the blob is not a usable output (a
    /// 0-byte stub, or dataless after an eviction raced `fs::copy`), it is
    /// removed so that [`store_file`](Self::store_file)
    /// can write fresh content, and an `Err` is returned.
    ///
    /// This is the single place where blob integrity is checked and
    /// self-healing happens. Called by `store_file` (idempotency check)
    /// and `link_to` (pre-link check).
    pub fn validate_blob(&self, oid: &str, source_size: u64) -> Result<(), String> {
        let path = self.blob_path(oid);
        if source_size > 0 && !crate::build::io_utils::output_present(&path) {
            log::warn!("[CAS] unusable blob detected, removing: {}", oid);
            let _ = fs::remove_file(&path);
            return Err(format!("Unusable blob: {}", oid));
        }
        Ok(())
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
    /// ALL copies (cache blob, site/, site-stage/) become 0 bytes
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
    /// `<target>`. The next `link_to` to the same `target` sweeps any stale
    /// `.tmp.*` siblings on entry, bounding the leak to one cycle per kill.
    pub fn link_to(&self, oid: &str, target: &Path) -> Result<(), String> {
        let blob = self.blob_path(oid);
        if !blob.exists() {
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
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        // Sweep stale `.tmp.*` siblings from prior killed link_to calls so
        // iCloud Drive doesn't accumulate visible orphan files. Best-effort;
        // failures here don't fail the link_to itself.
        //
        // We use the suffix `.tmp.<uuid>` rather than `.pending.<uuid>` (which
        // store_file uses inside the CAS shard dir) because link_to writes to
        // a user-visible destination (the output tree, or a vault path during
        // a publish-history restore), not the cache dir. The two patterns are
        // intentionally different so iCloud-allowlist tooling can tell them apart.
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
        if let Some(parent) = target.parent() {
            let tmp_prefix = format!("{}.tmp.", target_basename);
            if let Ok(rd) = fs::read_dir(parent) {
                for entry in rd.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with(&tmp_prefix) {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
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
                let _ = fs::remove_file(&tmp);
                return Err(format!("Failed to stat {} after copy: {}", tmp.display(), e));
            }
        };
        if copied != blob_size {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "Blob {} copy truncated: expected {} bytes, got {} (iCloud fault-in race?)",
                oid, blob_size, copied
            ));
        }

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
    /// Root directory — typically `.moss/build/cache/transforms/`.
    base: PathBuf,
    /// Reference to the object store, used to verify that output blobs
    /// still exist on disk.
    objects: ObjectStore,
}

impl TransformCache {
    /// Create a new `TransformCache`.
    ///
    /// - `base` — root directory for transform JSON files (e.g.,
    ///   `.moss/build/cache/transforms/`).
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
            fs::create_dir_all(parent)
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

        if let Err(first_err) = fs::rename(&tmp, &path) {
            if first_err.kind() == std::io::ErrorKind::NotFound {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                // Re-write temp file — it may have been removed too
                let _ = fs::write(&tmp, json.as_bytes());  // allow:raw_write temp for the index's own atomic save, under .moss/cache
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
    /// 4. The output blob (identified by the entry's OID) still exists
    ///    in the [`ObjectStore`].
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

        // 4. Output blob still exists on disk?
        if self.objects.get_path(&entry.oid).is_none() {
            return None;
        }

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

/// A single entry in the hash index, mapping file stat fields to a
/// content hash.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HashIndexEntry {
    /// File size in bytes (corresponds to `st_size`).
    pub size: u64,
    /// Modification time as Unix epoch seconds (corresponds to `st_mtime`).
    pub mtime: u64,
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
/// Our simplified version:
///   - Key: (relative_path) → { size: u64, mtime: u64, content_hash: String }
///   - If (size, mtime) match: trust content_hash, skip ObjectStore::hash_file()
///   - If either differs: re-hash via ObjectStore::hash_file(), update entry
///   - Racy-clean edge case is acceptable for media metadata caching —
///     worst case is one extra ffprobe call, not data corruption
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
            fs::create_dir_all(parent)
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
        // because tmp and target share the same parent directory (.moss/build/cache/).
        if let Err(first_err) = fs::rename(&tmp, path) {
            if first_err.kind() == std::io::ErrorKind::NotFound {
                if let Some(parent) = path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                // Re-write temp file — it may have been removed too
                let _ = fs::write(&tmp, json.as_bytes());  // allow:raw_write temp for the index's own atomic save, under .moss/cache
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

    /// Look up the cached content hash for a file.
    ///
    /// Returns `Some(content_hash)` if (size, mtime) match the cached entry.
    /// Returns `None` if the entry is missing or stat fields differ.
    pub fn lookup(&self, relative_path: &str, size: u64, mtime: u64) -> Option<&str> {
        let entry = self.entries.get(relative_path)?;
        if entry.size == size && entry.mtime == mtime {
            Some(&entry.content_hash)
        } else {
            None
        }
    }

    /// Insert or update an entry in the hash index.
    pub fn update(&mut self, relative_path: String, size: u64, mtime: u64, content_hash: String) {
        self.entries.insert(
            relative_path,
            HashIndexEntry {
                size,
                mtime,
                content_hash,
            },
        );
    }
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

/// Run mark-and-sweep garbage collection on the cache.
///
/// This function identifies and removes orphaned data in two phases:
///
/// 1. **Sweep TransformCache**: any transform record whose source OID is NOT
///    in the HashIndex (i.e., the source file no longer exists or was modified)
///    is deleted.
///
/// 2. **Sweep ObjectStore**: any blob whose OID is NOT referenced by a live
///    transform entry, the HashIndex content hashes, or the SiteHashes file map
///    is deleted.
///
/// ## Arguments
///
/// - `build_dir` — the `.moss/build/` directory containing `cache/`, `hashes.json`,
///   and `cache/hash-index.json`.
///
/// ## Safety
///
/// This must NOT be called during build — it assumes the cache is quiescent.
/// Running GC concurrently with a build could delete blobs that are about to be
/// referenced.
pub fn gc(build_dir: &Path) -> GcResult {
    let cache_dir = build_dir.join("cache");
    let objects_dir = cache_dir.join("objects");
    let transforms_dir = cache_dir.join("transforms");
    let hash_index_path = cache_dir.join("hash-index.json");
    let hashes_path = build_dir.join("hashes.json");

    // Phase 0: Load the HashIndex to determine live source OIDs.
    let hash_index = HashIndex::load(&hash_index_path);
    let live_source_oids: std::collections::HashSet<&str> = hash_index
        .entries
        .values()
        .map(|e| e.content_hash.as_str())
        .collect();

    // Phase 1: Sweep TransformCache — remove records for deleted/changed sources.
    let mut transforms_removed: usize = 0;
    let mut live_output_oids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Also collect source OIDs themselves as live (they may be stored in objects/)
    let mut live_oids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for oid in &live_source_oids {
        live_oids.insert((*oid).to_string());
    }

    if transforms_dir.is_dir() {
        // Walk the sharded directory: transforms/ab/cd/<source_oid>.json
        for prefix1 in read_dir_entries(&transforms_dir) {
            let p1 = transforms_dir.join(&prefix1);
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
                    // Extract source OID from filename: <source_oid>.json
                    let source_oid = match filename.strip_suffix(".json") {
                        Some(oid) => oid.to_string(),
                        None => continue, // Not a transform record
                    };

                    if live_source_oids.contains(source_oid.as_str()) {
                        // This transform record is live — collect its output OIDs
                        if let Ok(data) = fs::read_to_string(&file_path) {
                            if let Ok(record) = serde_json::from_str::<TransformRecord>(&data) {
                                for entry in record.transforms.values() {
                                    live_output_oids.insert(entry.oid.clone());
                                }
                            }
                        }
                    } else {
                        // Orphaned transform record — delete it
                        if fs::remove_file(&file_path).is_ok() {
                            transforms_removed += 1;
                        }
                    }
                }
                // Clean up empty prefix2 directories
                let _ = fs::remove_dir(&p2); // Only succeeds if empty
            }
            // Clean up empty prefix1 directories
            let _ = fs::remove_dir(&p1);
        }
    }

    // Phase 2: Collect all referenced OIDs.
    // Sources: live transform output OIDs + HashIndex content hashes + SiteHashes file values
    let mut referenced_oids = live_oids;
    referenced_oids.extend(live_output_oids);

    // Add SiteHashes file map values (output file hashes that may be stored as blobs)
    if let Ok(content) = fs::read_to_string(&hashes_path) {
        if let Ok(site_hashes) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(files) = site_hashes.get("files").and_then(|f| f.as_object()) {
                for hash in files.values() {
                    if let Some(h) = hash.as_str() {
                        referenced_oids.insert(h.to_string());
                    }
                }
            }
        }
    }

    // Phase 3: Sweep ObjectStore — remove unreferenced blobs.
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
                        if fs::remove_file(&file_path).is_ok() {
                            objects_removed += 1;
                            bytes_freed += file_size;
                        }
                    }
                }
                // Clean up empty prefix2 directories
                let _ = fs::remove_dir(&p2);
            }
            // Clean up empty prefix1 directories
            let _ = fs::remove_dir(&p1);
        }
    }

    GcResult {
        transforms_removed,
        objects_removed,
        bytes_freed,
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
