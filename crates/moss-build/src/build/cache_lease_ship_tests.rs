//! Regression test for the cache-lease-covers-ship fix: `CacheWriteLease`
//! must stay open through the seal tail's `materialize_and_promote`
//! (`ship_phase`), not just through the build's background workers, so a
//! concurrent `collect_build_store` can never GC a CAS blob `ship_phase` is
//! still reading. And this build's own `collect_build_store` must never find
//! its own lease still open either — see the `drop(cache_lease)` in
//! `advertise_sealed`.
//!
//! Drives the REAL `advertise_sealed` with a REAL `cache_write_lease`, rather
//! than hand-rolling the lease/drop sequence: that would only exercise
//! `CacheWriteLease`'s pre-existing (and already well-tested) counter
//! mechanics, not the wiring this fix changed. The synchronization below
//! (`FolderSession::try_lock_stage_write`) is production machinery too — the
//! rebuild worker's own try-admission probe — reused here only as a
//! deterministic "has `collect_build_store` already run?" signal: the real
//! `_stage_write_guard` `advertise_sealed` holds is acquired before
//! `materialize_and_promote` and dropped only AFTER its `collect_build_store`
//! call (see `build.rs`), so observing the guard go held-then-free is proof
//! of that ordering, not a timing guess.

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::system::folder_session::FolderSession;
use crate::types::content::SiteHashes;

/// A portable tempdir under `target/test-tmp`, matching
/// `epoch_ordering_tests.rs`'s and `promise_gate_tests.rs`'s own fixtures.
fn fixture() -> (tempfile::TempDir, crate::moss_paths::MossPaths, crate::build::manifest::SealedManifest) {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("moss-cache-lease-ship")
        .tempdir_in(&base)
        .unwrap();
    let mp = crate::moss_paths::MossPaths::new(tmp.path());
    std::fs::create_dir_all(mp.staging_dir()).unwrap();

    let page: &[u8] = b"<html><body>hi</body></html>";
    std::fs::write(mp.staging_dir().join("index.html"), page).unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(&ServedPath::from_source("index.html").unwrap(), page, HashBucket::Files);
    let sealed = pending.seal();

    (tmp, mp, sealed)
}

fn seal_ports() -> SealPorts {
    SealPorts {
        events: crate::build::null_sink(),
        announcer: std::sync::Arc::new(crate::build::ports::announcer::LogAnnouncer),
        server_diff: None,
    }
}

/// The fix's core invariant: `advertise_sealed` must drop the build's
/// `CacheWriteLease` before its own step-3 `collect_build_store` call runs —
/// held any longer and this build's own cache GC would always find its own
/// lease open and skip, deferring cleanup for no reason. A regression that
/// drops the explicit `drop(cache_lease)` in `advertise_sealed` (falling
/// back to Rust's implicit end-of-function drop, which runs AFTER
/// `collect_build_store`, `backfill::for_seal`, and the announcer calls)
/// must make this fail.
///
/// It also incidentally depends on `await_completion`-style plumbing: the
/// lease has to actually reach `advertise_sealed` still open for this to be
/// worth anything, which `background.rs`'s own
/// `await_completion_returns_the_cache_lease_instead_of_dropping_it` pins
/// directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advertise_sealed_drops_the_cache_lease_before_collect_build_store() {
    let (_tmp, mp, sealed) = fixture();
    let session = FolderSession::new(mp.project_root().to_path_buf());
    let lease = crate::build::lifecycle::cache_write_lease(&mp);
    assert_eq!(crate::build::lifecycle::snapshot(&mp).2, 1, "sanity: the lease is open before the seal tail runs");

    let ports = seal_ports();
    let hashes_path = mp.hashes();
    let stage = mp.staging_dir();
    let epoch = crate::build::ship::next_promotion_epoch();
    let folder_path = format!("/cache-lease-ship-test-{}", uuid::Uuid::new_v4());

    let seal_fut = advertise_sealed(
        &ports,
        &mp,
        &hashes_path,
        &stage,
        sealed,
        None,
        |_| false,
        Some(&session),
        epoch,
        Some(1),
        true,
        crate::build::feeds::search_lane::Freshness::Now,
        &folder_path,
        None,
        Some(lease),
    );

    // Two-phase probe of the seal tail's OWN stage-write guard, which is
    // acquired before `materialize_and_promote` and released only after
    // `collect_build_store` has returned (`build.rs`'s step-3 comment).
    // Phase 1 waits for the guard to actually become held — proof that
    // `advertise_sealed` has started and is genuinely mid-tail, not just
    // "the lock happens to be free because nothing touched it yet". Phase 2
    // then waits for it to free up again, which by that same ordering can
    // only happen once `collect_build_store` has already run.
    let poll_fut = async {
        let mut observed_held = false;
        for _ in 0..200_000 {
            if session.try_lock_stage_write().is_none() {
                observed_held = true;
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            observed_held,
            "advertise_sealed never appeared to acquire its own stage-write guard"
        );
        loop {
            if session.try_lock_stage_write().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        crate::build::lifecycle::snapshot(&mp).2
    };

    let (_, writers_when_guard_freed) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        futures::future::join(seal_fut, poll_fut),
    )
    .await
    .expect("advertise_sealed (and the probe) must finish well within 10s");

    assert_eq!(
        writers_when_guard_freed, 0,
        "the cache lease must already be dropped by the time collect_build_store has run \
         (proven by the seal tail's own stage-write guard being free again) — held past it \
         means this build's own cache GC would always see its own lease as still open"
    );
    assert_eq!(
        crate::build::lifecycle::snapshot(&mp).2, 0,
        "and the lease must not leak past advertise_sealed's return either"
    );
}
