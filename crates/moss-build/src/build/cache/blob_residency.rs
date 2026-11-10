//! Resident cached blobs. A cloud placeholder is treated as a
//! cache miss; preview work can regenerate the bytes without waiting for it.

use super::{HashIndex, ObjectStore};
use crate::build::stat::{FileStat, recording_clock};
use std::fs;
use std::path::{Path, PathBuf};

// Bounds a store that lives as long as the process. A store built with
// `remembering_verified` serves one pass and is dropped, so it holds every blob
// the pass asks about: clearing it mid-pass would save a fraction of the pass.
pub(crate) const MAX_VERIFIED_BLOBS: usize = 4096;

#[cfg(test)]
thread_local! {
    pub(crate) static HASH_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
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
    /// A cloud-only shared blob is a miss
    /// for this build. The local CAS holds outputs regenerated this run even
    /// when the shared cache refuses a write.
    pub fn ready_blob(&self, oid: &str) -> Option<PathBuf> {
        if !valid_oid(oid) { return None; }
        for path in [self.resident_local_blob(oid), Some(self.blob_path(oid))].into_iter().flatten() {
            if crate::build::icloud::is_still_in_the_cloud(&path) {
                continue;
            }
            if !crate::build::io_utils::output_present(&path) {
                if path.exists() {
                    log::info!("[CAS] unusable blob {} at {}, treating as missing", oid, path.display());
                }
                continue;
            }
            let Ok(metadata) = fs::metadata(&path) else { continue };
            let before = FileStat::of(&metadata);
            let key = path.to_string_lossy().into_owned();
            let mut index = self.verified.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if index.lookup(&key, &before) == Some(oid) {
                return Some(path);
            }
            if let Some(previous) = self.remembered.as_deref().map(|r| &r.index).filter(|previous| previous.lookup(&key, &before) == Some(oid)) {
                index.carry_forward(previous, &key);
                return Some(path);
            }
            drop(index);
            let recorded_at = recording_clock();
            match Self::hash_file_once(&path) {
                Ok(hash) if hash == oid => {
                    let unchanged = fs::metadata(&path).ok().is_some_and(|after| FileStat::of(&after) == before);
                    if !unchanged { continue; }
                    let mut index = self.verified.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if self.remembered.is_none() && index.entries.len() >= MAX_VERIFIED_BLOBS && !index.entries.contains_key(&key) {
                        index.entries.clear();
                    }
                    index.update_read_at(key, &before, hash, recorded_at);
                    return Some(path);
                },
                Ok(hash) => {
                    log::info!("[CAS] blob {} hashes to {} — removed, regenerating", oid, hash);
                    // allow:unlink a corrupt blob in an owned CAS directory
                    let _ = fs::remove_file(&path);
                }
                Err(e) => log::debug!("[CAS] cannot read blob {}: {}", oid, e),
            }
        }
        None
    }

    /// This store for one pass, answering a blob check from what an earlier
    /// pass saved to `path` with [`save_verified`](Self::save_verified) while
    /// the blob's stat still vouches for it. A blob whose stat moved is hashed.
    pub(crate) fn remembering_verified(mut self, path: &Path) -> Self {
        self.remembered = Some(std::sync::Arc::new(Remembered { index: HashIndex::load(path), path: path.to_path_buf() }));
        self
    }

    /// Save what this store verified, and nothing else: an entry for a blob no
    /// check asked about since the last save drops out, so the file holds one
    /// pass's blobs. A concurrent process's save can replace it; that costs the
    /// next build a re-hash, never a wrong answer.
    pub(crate) fn save_verified(&self) -> Result<(), String> {
        self.save_verified_keeping(|_| false)
    }

    /// [`save_verified`](Self::save_verified) for a pass that checks only some
    /// of the blobs, such as a video run over the videos that changed: what an
    /// earlier save holds for a blob this pass did not check stays while the
    /// blob's file exists.
    pub(crate) fn save_verified_keeping_unchecked(&self) -> Result<(), String> {
        self.save_verified_keeping(|key| Path::new(key).exists())
    }

    fn save_verified_keeping(&self, keep: impl Fn(&str) -> bool) -> Result<(), String> {
        let Some(remembered) = self.remembered.as_deref() else {
            return Err("this store remembers no verified-blob index".into());
        };
        let index = self.verified.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        index.save_pruned(&remembered.path, keep)
    }
}

pub(crate) struct Remembered {
    index: HashIndex,
    path: std::path::PathBuf,
}

fn valid_oid(oid: &str) -> bool {
    oid.len() == 64 && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}
