//! The two halves every hosted-picture family shares: writing a picture
//! atomically, and keeping every picture a family ever wrote in each build.
//!
//! A hosted picture's URL is content-addressed and ends up in sent email,
//! where mail proxies cache it forever, so a family is append-only: a picture
//! whose source was edited or deleted keeps serving its original bytes.

use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::{ServedPath, ServedPathError};
use std::path::Path;

/// Atomic write: temp + rename (`.pending.<uuid>` because iCloud Drive
/// excludes `.tmp` from sync). A reader sees no file or a complete one, and
/// identical inputs render identical bytes, so whichever writer wins is
/// immaterial.
pub(crate) fn write_atomic(disk_path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = disk_path.parent() {
        crate::build::io_utils::create_output_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = disk_path.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?; // allow:raw_write the temp this fn just minted; the rename below is the output write
    // allow:unlink rename into place: the picture is replaced, never absent
    std::fs::rename(&tmp, disk_path).map_err(|e| format!("rename {}: {e}", disk_path.display()))
}

/// Registers every picture under `dir` (relative to `output_dir`) — this
/// build's and every earlier build's — so each ships in this generation.
pub(crate) fn retain(
    output_dir: &Path,
    dir: &str,
    served: fn(&str) -> Result<ServedPath, ServedPathError>,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let disk_dir = output_dir.join(dir);
    if !disk_dir.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&disk_dir)
        .map_err(|e| format!("read {}: {e}", disk_dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // A crash between temp-write and rename leaves `*.pending.<uuid>`
        // behind. It is never registered; the permitted staging sweep removes
        // it. Removing it here could take a concurrent write's temp mid-rename.
        if name.contains(".pending.") {
            continue;
        }
        let Some(stem) = name.strip_suffix(".png") else { continue };
        let Ok(sp) = served(stem) else { continue };
        match std::fs::read(entry.path()) {
            Ok(bytes) if !bytes.is_empty() => {
                pending.register(&sp, &bytes, HashBucket::Files);
            }
            // Unreadable or a 0-byte eviction stub. Dropping it would let the
            // next publish remove a URL already in somebody's inbox, so carry
            // the previous build's entry instead: the picture is
            // content-addressed, so that entry describes these exact bytes.
            _ => {
                if let Some(previous) = pending.files().get(sp.as_str()).cloned() {
                    pending.register_hashed(&sp, &previous, HashBucket::Files);
                }
            }
        }
    }
    Ok(())
}
