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
//! mechanics, not the wiring this fix changed. Two independent probes cover
//! the two ends of the held span:
//!
//! - `SHIP_PHASE_LEASE_SAMPLE` (a `build.rs` task-local, scoped here via
//!   `.scope()`) is sampled by `advertise_sealed` itself immediately after
//!   `materialize_and_promote` returns and before `drop(cache_lease)` — proof
//!   the lease is still open at the one moment that actually distinguishes
//!   this fix from the bug it closed. Nothing sampled later can see this
//!   moment: `_stage_write_guard` is held uniformly across the whole tail, and
//!   a lease dropped before the ship reads the same, 0, as a correct one at
//!   any point after it.
//! - The `is_pinned` closure handed to `advertise_sealed` is called while the
//!   tail resolves its pin set — inside step 3, after the ship and before
//!   `collect_build_store` is handed to the blocking pool — and the seal tail's
//!   own `_stage_write_guard` is held across all of it. Sampling the cache
//!   lease's writer count there answers "was the lease already dropped before
//!   `collect_build_store` could run?" at a point the tail itself parks the
//!   test on, so it is a program-order fact, not something polled for. An
//!   earlier version polled the guard from a task interleaved with the tail and
//!   failed about 1 run in 2000 under load: the guard was held only across the
//!   `spawn_blocking(collect_build_store).await` suspension, and when the
//!   blocking pool finished that job before the tail's first poll of it the
//!   suspension never happened and the guard was never visible.

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

/// The fix's core invariant, in two halves. First, the lease must still be
/// open right after `materialize_and_promote` (`ship_phase`) — a regression
/// that drops `cache_lease` any earlier, e.g. right after
/// `_stage_write_guard` is acquired and before `repair_staged_html` /
/// `materialize_and_promote` run at all, reopens the exact GC race this fix
/// closed, and `lease_sample` below must catch it even though the later
/// pin-resolution sample cannot (a lease dropped early reads 0 there too). Second,
/// `advertise_sealed` must drop the lease before its own step-3
/// `collect_build_store` call runs — held any longer and this build's own
/// cache GC would always find its own lease open and skip, deferring cleanup
/// for no reason. A regression that drops the explicit `drop(cache_lease)`
/// in `advertise_sealed` entirely (falling back to Rust's implicit
/// end-of-function drop, which runs AFTER `collect_build_store`,
/// `backfill::for_seal`, and the announcer calls) must make this fail too.
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

    // Sampled by `advertise_sealed` itself (`SHIP_PHASE_LEASE_SAMPLE`, see
    // `build.rs`) immediately after `materialize_and_promote` returns and
    // before `drop(cache_lease)` — the window that actually distinguishes
    // this fix from the bug it closed. `usize::MAX` is a sentinel meaning
    // "never sampled", which would itself be a failure below.
    let lease_sample = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));

    // Sampled by `is_pinned`, which `advertise_sealed` calls once per stored
    // generation while it resolves the pin set (step 3) — after the lease's
    // drop point and before `collect_build_store` is spawned. `usize::MAX` is
    // the "never called" sentinel, which is itself a failure below: without
    // this call the tail never reached step 3 and the ordering proves nothing.
    // The stage-write guard must be held at that instant, or this is not the
    // span step 3's comment says it is.
    let writers_at_pin_resolution = std::sync::atomic::AtomicUsize::new(usize::MAX);
    let guard_held_at_pin_resolution = std::sync::atomic::AtomicBool::new(false);
    let is_pinned = |_: &str| {
        writers_at_pin_resolution.store(
            crate::build::lifecycle::snapshot(&mp).2,
            std::sync::atomic::Ordering::SeqCst,
        );
        guard_held_at_pin_resolution.store(
            session.try_lock_stage_write().is_none(),
            std::sync::atomic::Ordering::SeqCst,
        );
        false
    };

    super::SHIP_PHASE_LEASE_SAMPLE
        .scope(
            lease_sample.clone(),
            advertise_sealed(
                &ports,
                &mp,
                &hashes_path,
                &stage,
                sealed,
                None,
                is_pinned,
                Some(&session),
                epoch,
                Some(1),
                true,
                crate::build::feeds::search_lane::Freshness::Now,
                &folder_path,
                SealGuards {
                    final_sweep: None,
                    cache_lease: Some(lease),
                },
            ),
        )
        .await;

    assert_eq!(
        lease_sample.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the cache lease must still be open right after materialize_and_promote \
         (ship_phase) — dropping it any earlier reopens the GC race this fix closed"
    );
    assert_ne!(
        writers_at_pin_resolution.load(std::sync::atomic::Ordering::SeqCst),
        usize::MAX,
        "the tail never resolved its pin set, so it never reached step 3 and \
         `collect_build_store`; the ordering asserted below would prove nothing"
    );
    assert!(
        guard_held_at_pin_resolution.load(std::sync::atomic::Ordering::SeqCst),
        "pin resolution must run inside the seal tail's stage-write guard span — that span is \
         what serializes it against the next rebuild's promotion"
    );
    assert_eq!(
        writers_at_pin_resolution.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the cache lease must already be dropped before collect_build_store is handed to the \
         blocking pool — held past it means this build's own cache GC would always see its own \
         lease as still open and skip"
    );
    assert_eq!(
        crate::build::lifecycle::snapshot(&mp).2, 0,
        "and the lease must not leak past advertise_sealed's return either"
    );
}

/// Held bytes must be dropped once the tail has shipped them, or the manifest
/// deploy keeps for the life of the app carries every derived output's bytes
/// (megabytes on a real vault) per open folder. Drives the REAL
/// `advertise_sealed` and reads the manifest it hands to `adopt_sealed`, which
/// is the only place the leak would be visible: the tail itself drops `sealed`
/// on every other path.
///
/// Also ships from a stage a later build has overwritten, so it proves the
/// bytes were read before they were released.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn advertise_sealed_ships_held_bytes_then_hands_deploy_a_manifest_without_them() {
    let (_tmp, mp, _) = fixture();
    let mut pending = PendingManifest::new(SiteHashes::default());
    let page: &[u8] = b"<html><body>hi</body></html>";
    pending.register(&ServedPath::from_source("index.html").unwrap(), page, HashBucket::Files);
    pending
        .register_held(&ServedPath::from_source("sitemap.xml").unwrap(), b"<urlset>A</urlset>".to_vec(), HashBucket::Files)
        .unwrap();
    let sealed = pending.seal();
    // The later build's rewrite of the derived file this build sealed.
    std::fs::write(mp.staging_dir().join("sitemap.xml"), b"<urlset>B</urlset>").unwrap();

    let mut host = crate::build::ports::host::test_host_ports();
    let adopted = crate::deploy::one_shot::capture_seal(&mut host);
    let ports = SealPorts { events: crate::build::null_sink(), announcer: host.announcer.clone(), server_diff: None };
    let session = FolderSession::new(mp.project_root().to_path_buf());

    advertise_sealed(
        &ports,
        &mp,
        &mp.hashes(),
        &mp.staging_dir(),
        sealed,
        None,
        |_| false,
        Some(&session),
        crate::build::ship::next_promotion_epoch(),
        Some(1),
        true,
        crate::build::feeds::search_lane::Freshness::Now,
        &format!("/held-release-test-{}", uuid::Uuid::new_v4()),
        SealGuards { final_sweep: None, cache_lease: None },
    )
    .await;

    let adopted = adopted.lock().unwrap().take().expect("the tail must have adopted its manifest");
    assert_eq!(
        std::fs::read(mp.generation_dir(adopted.generation_id()).join("sitemap.xml")).unwrap(),
        b"<urlset>A</urlset>",
        "the generation must carry the bytes the manifest held, not the stage's"
    );
    assert_eq!(adopted.held_bytes("sitemap.xml"), None, "the adopted manifest must not keep the bytes it just shipped");
    assert!(adopted.files().contains_key("sitemap.xml"), "releasing bytes must not drop the entry");
}
