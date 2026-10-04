//! Waiting for a cached blob that the cloud has not downloaded here.

use super::ObjectStore;
use crate::build::cloud_readiness::{breaker_for, request_download, retry_reporting_timeout, Breakers};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One breaker per objects directory, so a cache on an unreachable drive does
/// not stop another site's blobs from being waited for.
static BLOB_BREAKERS: Breakers = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// How long to wait for a blob the cloud holds: ten seconds plus one per MiB,
/// capped at ten minutes. A video-sized blob's download wins by construction
/// against a minutes-long re-encode; a small blob resolves either way in
/// seconds.
fn download_deadline(size: u64) -> Duration {
    #[cfg(test)]
    if let Some(short) = crate::build::cloud_readiness::TEST_DEADLINE.with(|d| d.get()) {
        return short;
    }
    (Duration::from_secs(10) + Duration::from_secs(size / (1024 * 1024))).min(Duration::from_secs(600))
}

/// Run `attempt` on the blob `path`, waiting for its download behind the
/// breaker of the objects directory `root`: three waits in a row that ran to
/// their deadline pause waiting, the same pause the record reads have. While
/// paused the blob is only asked for and the read fails at once; an arrival
/// clears the count.
fn retry_after_materialize<T>(
    root: &Path,
    path: &Path,
    deadline: Duration,
    attempt: impl Fn() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let breaker = breaker_for(&BLOB_BREAKERS, root, "cached files");
    if breaker.is_paused(Instant::now()) {
        request_download(path);
        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "waiting for cached files is paused"));
    }
    let (out, timed_out) = retry_reporting_timeout(path, deadline, attempt);
    if timed_out {
        breaker.timed_out(Instant::now());
    } else if out.is_ok() {
        breaker.succeeded();
    }
    out
}

impl ObjectStore {
    /// Whether the store keeps `oid`: its blob is here, or in the cloud and
    /// not yet downloaded. Never downloads it.
    pub fn holds(&self, oid: &str) -> bool {
        let p = self.blob_path(oid);
        crate::build::io_utils::output_present(&p) || crate::build::icloud::is_still_in_the_cloud(&p)
    }

    /// A cached output the read path may use now: the blob's path for a hit,
    /// `None` for a miss the caller fills by regenerating and storing.
    ///
    /// Present ⇒ hit. In the cloud ⇒ its download is requested and waited
    /// for, bounded by the blob's size and by the objects directory's breaker
    /// (see [`retry_after_materialize`]), and the arrival is hashed once ⇒ hit,
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
        let arrived = retry_after_materialize(&self.base, &p, download_deadline(size), || {
            #[cfg(test)]
            if crate::build::icloud::pretend::marked(&p) {
                return Err(std::io::Error::other("pretend: still in the cloud"));
            }
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

}
