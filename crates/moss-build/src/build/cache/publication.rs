//! Owned atomic shared-cache publication on the existing background readers.
use super::ObjectStore;
use crate::build::cloud_readiness::storage::{OperationKey, OperationPolicy, StorageValue};
use crate::build::cloud_readiness::{StorageFailure, StorageOperation};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The required local blob is immutable and protected from cache GC for the
/// entire native attempt, including after its caller has detached. Large
/// sources stay on disk and are streamed by `fs::copy` and hash verification.
pub(super) struct BlobPublication {
    root: PathBuf,
    oid: String,
    source: PathBuf,
    _lease: crate::build::lifecycle::DetachedCacheLease,
}

impl BlobPublication {
    pub fn new(root: PathBuf, oid: String, source: PathBuf, lease: crate::build::lifecycle::DetachedCacheLease) -> Self {
        Self { root, oid, source, _lease: lease }
    }

    pub fn publish(self) -> crate::build::cloud_readiness::storage::PublicationOutcome {
        let dest = ObjectStore::blob_path_in(&self.root, &self.oid);
        let key = OperationKey { path: dest, policy: OperationPolicy::CachePublication { root: self.root.clone() } };
        let request = Arc::new(self);
        let attempt: crate::build::cloud_readiness::storage::Operation = Arc::new(move || request.run());
        crate::build::cloud_readiness::storage::await_publication(key, attempt)
    }

    fn run(&self) -> Result<StorageValue, StorageFailure> {
        let dest = ObjectStore::blob_path_in(&self.root, &self.oid);
        // A winner is successful only when it supplies the exact bytes. This
        // probe runs on a native reader, so a placeholder never means Ready.
        if crate::build::io_utils::output_present(&dest)
            && ObjectStore::hash_file_once(&dest).is_ok_and(|hash| hash == self.oid)
        {
            return Ok(StorageValue::Published(0));
        }
        let parent = dest.parent().expect("blob path has a shard");
        attempt(&dest, StorageOperation::CacheMkdir, || std::fs::create_dir_all(parent))?; // allow:raw_write shared CAS shard, never staging
        let tmp = dest.with_extension(format!("pending.{}", uuid::Uuid::new_v4()));
        let write = attempt(&dest, StorageOperation::CacheWrite, || {
            std::fs::copy(&self.source, &tmp)?; // allow:raw_write owned CAS candidate, never an existing shared blob
            let hash = ObjectStore::hash_file_once(&tmp)?;
            if hash != self.oid { return Err(std::io::Error::other("owned replica bytes do not match their content hash")); }
            Ok(())
        });
        if let Err(failure) = write {
            let _ = std::fs::remove_file(&tmp); // allow:unlink this attempt's own pending CAS candidate
            return Err(failure);
        }
        let publish = attempt(&dest, StorageOperation::CachePublish, || {
            std::fs::rename(&tmp, &dest) // allow:unlink atomic publication of the verified sibling candidate
        });
        if let Err(failure) = publish {
            let won = crate::build::io_utils::output_present(&dest)
                && ObjectStore::hash_file_once(&dest).is_ok_and(|hash| hash == self.oid);
            let _ = std::fs::remove_file(&tmp); // allow:unlink this attempt's own pending CAS candidate
            if !won { return Err(failure); }
        }
        Ok(StorageValue::Published(0))
    }
}

/// One native transaction stage. It has no retry or request policy; the pool
/// re-runs the owned whole transaction after a returned provider refusal.
pub(super) fn attempt<T>(path: &Path, operation: StorageOperation, op: impl FnOnce() -> std::io::Result<T>) -> Result<T, StorageFailure> {
    #[cfg(test)]
    {
        crate::build::cloud_readiness::storage::test_probe(path, operation)?;
        if let Some(error) = crate::build::icloud::pretend::refusal_below(path) {
            return Err(StorageFailure::new(Some(path.to_path_buf()), operation, error));
        }
    }
    op().map_err(|error| StorageFailure::new(Some(path.to_path_buf()), operation, error))
}

#[cfg(test)]
#[test]
fn availability_rejected_replica_admission_is_not_pending_and_local_blob_stays_usable() {
    crate::infra::home::with_moss_home(|_| {
        use crate::build::cloud_readiness::storage::{await_publication_in, Operation, PublicationOutcome};
        let pool = crate::build::cloud_prefetch::Prefetcher::with_materializer(8, Arc::new(|_| Ok(())), false);
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let native_gate = gate.clone();
        let held: Operation = Arc::new(move || {
            let mut released = native_gate.0.lock().unwrap();
            while !*released { released = native_gate.1.wait(released).unwrap(); }
            Ok(StorageValue::Published(0))
        });
        for i in 0..6 {
            pool.cache_publication(OperationKey { path: format!("/held/{i}").into(), policy: OperationPolicy::CachePublication { root: "/held".into() } }, held.clone()).unwrap();
        }
        let started = std::time::Instant::now();
        while pool.snapshot().in_flight != 6 {
            assert!(started.elapsed() < std::time::Duration::from_secs(2));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for i in 6..18 {
            pool.cache_publication(OperationKey { path: format!("/held/{i}").into(), policy: OperationPolicy::CachePublication { root: "/held".into() } }, held.clone()).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::moss_paths::MossPaths::new(dir.path());
        let local = ObjectStore::new(paths.cache_local_objects());
        let oid = local.store_bytes(b"complete local asset", crate::build::cache::RecordMode::Request).unwrap();
        let source = local.blob_path(&oid);
        let job = Arc::new(BlobPublication::new(paths.cache_objects(), oid.clone(), source.clone(), crate::build::lifecycle::detached_cache_lease(&paths)));
        let dest = ObjectStore::blob_path_in(&job.root, &oid);
        let op: Operation = Arc::new(move || job.run());
        let result = await_publication_in(&pool, OperationKey { path: dest.clone(), policy: OperationPolicy::CachePublication { root: paths.cache_objects() } }, op, None, 0);
        assert_eq!(pool.snapshot().waiting, 12, "rejection retains no extra publication");
        assert_eq!(std::fs::read(&source).unwrap(), b"complete local asset");
        assert!(!dest.exists());
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(matches!(result, PublicationOutcome::Fatal(ref failure) if failure.io_error().is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)), "no admitted job means no future completion and must not be Pending");
    });
}
