//! Resident cached blobs. A cloud placeholder is requested and treated as a
//! cache miss; preview work can regenerate the bytes without waiting for it.

use super::ObjectStore;
use crate::build::stat::{FileStat, recording_clock};
use std::fs;
use std::path::PathBuf;

const MAX_VERIFIED_BLOBS: usize = 4096;

#[cfg(test)]
thread_local! {
    pub(super) static HASH_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl ObjectStore {
    /// Whether the store already holds an OID, locally or in the shared cloud
    /// cache. This is a presence query and never requests a download.
    pub fn holds(&self, oid: &str) -> bool {
        if !valid_oid(oid) { return false; }
        self.resident_local_blob(oid).is_some() || {
            let p = self.blob_path(oid);
            crate::build::io_utils::output_present(&p) || crate::build::icloud::is_still_in_the_cloud(&p)
        }
    }

    /// A cached blob usable now, after checking that its bytes match its name.
    /// A cloud-only shared blob is requested in the background and is a miss
    /// for this build. The local CAS holds outputs regenerated this run even
    /// when the shared cache refuses a write.
    pub fn ready_blob(&self, oid: &str) -> Option<PathBuf> {
        if !valid_oid(oid) { return None; }
        for path in [self.resident_local_blob(oid), Some(self.blob_path(oid))].into_iter().flatten() {
            if crate::build::icloud::is_still_in_the_cloud(&path) {
                crate::build::cloud_readiness::request_download(&path);
                continue;
            }
            if !crate::build::io_utils::output_present(&path) {
                if path.exists() {
                    log::warn!("[CAS] unusable blob {} at {}, treating as missing", oid, path.display());
                }
                continue;
            }
            let Ok(metadata) = fs::metadata(&path) else { continue };
            let before = FileStat::of(&metadata);
            let key = path.to_string_lossy().into_owned();
            if self.verified.lock().ok().is_some_and(|index| index.lookup(&key, &before) == Some(oid)) {
                return Some(path);
            }
            let recorded_at = recording_clock();
            match Self::hash_file_once(&path) {
                Ok(hash) if hash == oid => {
                    let unchanged = fs::metadata(&path).ok().is_some_and(|after| FileStat::of(&after) == before);
                    if !unchanged { continue; }
                    if let Ok(mut index) = self.verified.lock() {
                        if index.entries.len() >= MAX_VERIFIED_BLOBS && !index.entries.contains_key(&key) {
                            index.entries.clear();
                        }
                        index.update_read_at(key, &before, hash, recorded_at);
                    }
                    return Some(path);
                },
                Ok(hash) => {
                    log::warn!("[CAS] blob {} hashes to {} — removed, regenerating", oid, hash);
                    // allow:unlink a corrupt blob in an owned CAS directory
                    let _ = fs::remove_file(&path);
                }
                Err(e) => log::debug!("[CAS] cannot read blob {}: {}", oid, e),
            }
        }
        None
    }
}

fn valid_oid(oid: &str) -> bool {
    oid.len() == 64 && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}
