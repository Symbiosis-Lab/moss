//! Tests for the shared upload routing and the sliding window.
//!
//! The routing test exists because this exact routing was silently deleted by a
//! refactor once (`b1df2298a`) and cost liu-guo.com six 100 MB videos. The
//! algorithm test exists because a shared verifier that hard-coded one hash
//! would break 100% of `moss deploy --prebuilt` while every `deploy.rs` test
//! stayed green.

use super::*;

fn tmp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/test-tmp/upload-tests");
    std::fs::create_dir_all(&base).expect("create target/test-tmp/upload-tests");
    let path = base.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

// ── Hash algorithm is a parameter, not a constant ────────────────────────────

/// The two deploy paths build their manifests with different digests:
/// `deploy.rs` uses xxh3_64 (16 hex chars), `deploy/prebuilt.rs` uses Sha256
/// (64 hex). Verifying one against the other rejects every single file.
#[test]
fn the_two_algorithms_produce_different_digests_and_lengths() {
    let bytes = b"the same bytes";
    let xxh3 = HashAlgo::Xxh3.hash_bytes(bytes);
    let sha = HashAlgo::Sha256.hash_bytes(bytes);
    assert_eq!(xxh3.len(), 16, "xxh3_64 is 16 hex chars");
    assert_eq!(sha.len(), 64, "sha256 is 64 hex chars");
    assert_ne!(xxh3, sha);
}

#[test]
fn each_algorithm_verifies_its_own_manifest_hash() {
    let bytes = b"content";
    for algo in [HashAlgo::Xxh3, HashAlgo::Sha256] {
        let expected = algo.hash_bytes(bytes);
        assert!(verify_bytes("f.txt", bytes, &expected, algo).is_ok());
    }
}

/// The regression this module was restructured to make impossible.
#[test]
fn verifying_a_sha256_manifest_with_xxh3_rejects_a_correct_file() {
    let bytes = b"content";
    let sha_entry = HashAlgo::Sha256.hash_bytes(bytes);
    let err = verify_bytes("f.txt", bytes, &sha_entry, HashAlgo::Xxh3)
        .expect_err("wrong algorithm must not silently pass");
    assert!(err.contains("integrity"), "{err}");
}

/// Streaming and buffered digests must agree, or a file would verify on the
/// single-PUT path and fail on the chunked path purely because of its size.
#[test]
fn streaming_and_buffered_digests_agree_for_both_algorithms() {
    // Larger than the 64 KB streaming buffer so it spans multiple reads.
    let bytes: Vec<u8> = (0u32..40_000).flat_map(|i| i.to_le_bytes()).collect();
    let path = tmp_file("streaming-agreement.bin", &bytes);
    for algo in [HashAlgo::Xxh3, HashAlgo::Sha256] {
        assert_eq!(
            algo.hash_bytes(&bytes),
            algo.hash_file(&path).expect("hash file"),
            "streaming digest must equal the buffered one"
        );
    }
}

/// An empty manifest hash means a legacy or regressed entry. Failing the deploy
/// would strand the user behind a stale build cache with no way forward.
#[test]
fn an_empty_manifest_hash_skips_verification_rather_than_failing() {
    assert!(verify_bytes("f.txt", b"anything", "", HashAlgo::Xxh3).is_ok());
}

#[test]
fn a_wrong_hash_names_the_file_and_both_hashes_so_the_error_is_actionable() {
    let err = verify_bytes("assets/photo.jpg", b"bytes", "deadbeefdeadbeef", HashAlgo::Xxh3)
        .expect_err("must reject");
    assert!(err.contains("assets/photo.jpg"), "{err}");
    assert!(err.contains("deadbeefdeadbeef"), "must quote the expected hash: {err}");
    assert!(err.contains("Deploy integrity error"), "{err}");
}

/// The concrete case this check exists for. iCloud Drive's "optimize storage"
/// eviction zeroes a file in place, leaving a stub of the right length and the
/// wrong content. Uploading it would publish a blank asset that no rebuild
/// notices, because the build cache still holds the original hash.
#[test]
fn a_zeroed_icloud_stub_is_rejected_before_it_is_uploaded() {
    let real = b"actual image bytes";
    let expected = HashAlgo::Xxh3.hash_bytes(real);
    let evicted = vec![0u8; real.len()];
    assert!(verify_bytes("assets/photo.jpg", &evicted, &expected, HashAlgo::Xxh3).is_err());
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Routing is by `upload_policy::needs_chunking` against the window's live
/// throughput estimate, which is what makes the duration bound hold. Pinned
/// here as well as in the policy module because this is the call site a
/// refactor actually deletes.
#[test]
fn routing_matches_the_policy_at_the_boundary() {
    let tp = upload_policy::Throughput::new();
    let t = tp.plan_request_size() as u64;
    assert!(!tp.needs_chunking(t));
    assert!(tp.needs_chunking(t + 1));
    // The file that killed the okagaki deploy.
    assert!(tp.needs_chunking(8_910_888));
}

/// The tasks size their requests by how many share the link, and the count
/// they read is the one this window maintains — spawn enters, harvest exits.
/// If this mirror drifts, every chunk of a solo upload is sized as if the
/// window were full, which is the 262-requests-for-one-video defect of
/// 2026-08-27 in a new coat.
#[tokio::test]
async fn the_window_mirrors_its_occupancy_into_the_shared_throughput() {
    let mut window = UploadWindow::new();
    let tp = window.throughput();
    assert_eq!(tp.in_flight(), 0);
    window.reserve(1).await.unwrap();
    window.spawn(1, async { Ok(()) });
    assert_eq!(tp.in_flight(), 1);
    window.drain().await.unwrap();
    assert_eq!(tp.in_flight(), 0);
}

/// The window's estimate is per-deploy state, not process state: two windows
/// must not see each other's measurements, or a fast publish would leave a
/// slow one mis-sized (and the tests would be order-dependent).
#[test]
fn each_window_carries_its_own_throughput_estimate() {
    use crate::seta::upload_policy::SEED_BANDWIDTH_BYTES_PER_SEC;
    let a = UploadWindow::new();
    let b = UploadWindow::new();
    a.throughput()
        .observe(256 * 1024, std::time::Duration::from_secs(40));
    assert!(a.throughput().bytes_per_sec() < SEED_BANDWIDTH_BYTES_PER_SEC);
    assert_eq!(b.throughput().bytes_per_sec(), SEED_BANDWIDTH_BYTES_PER_SEC);
}

// ── The sliding window ───────────────────────────────────────────────────────

use crate::seta::upload_policy::{BYTE_BUDGET, LIMIT_START};

/// The old `.chunks(20)` shape was a barrier: nineteen finished files waited on
/// one slow straggler before ANY further work started. A window must admit new
/// work while the straggler is still running.
///
/// Modelled with a task that never completes on its own: if the window were a
/// barrier, `reserve` for the second task would hang and the test would time
/// out.
#[tokio::test]
async fn a_slow_task_does_not_block_admission_of_the_next() {
    let mut w = UploadWindow::new();
    let (release, blocked) = tokio::sync::oneshot::channel::<()>();

    w.reserve(1024).await.unwrap();
    w.spawn(1024, async move {
        let _ = blocked.await;
        Ok(())
    });

    // Must not hang: a barrier would.
    w.reserve(1024).await.unwrap();
    w.spawn(1024, async { Ok(()) });

    let _ = release.send(());
    w.drain().await.unwrap();
}

/// Admission is capped by count even when the bytes are trivial — otherwise a
/// directory of small files would run thousands-wide and divide the uplink into
/// slices too thin for any request to finish inside the edge budget.
#[tokio::test]
async fn the_window_never_exceeds_the_concurrency_limit() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let live = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut w = UploadWindow::new();

    for _ in 0..40 {
        w.reserve(1).await.unwrap();
        let live = Arc::clone(&live);
        let peak = Arc::clone(&peak);
        w.spawn(1, async move {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            tokio::task::yield_now().await;
            live.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        });
    }
    w.drain().await.unwrap();

    assert!(
        peak.load(Ordering::SeqCst) <= LIMIT_START,
        "peak concurrency {} exceeded LIMIT_START {}",
        peak.load(Ordering::SeqCst),
        LIMIT_START
    );
}

/// The memory bound. The old shape could hold 20 x 20 MB = 400 MB resident.
#[tokio::test]
async fn the_window_never_exceeds_the_byte_budget() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let live = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicU64::new(0));
    let mut w = UploadWindow::new();
    const SIZE: u64 = 3 * 1024 * 1024;

    for _ in 0..12 {
        w.reserve(SIZE).await.unwrap();
        let live = Arc::clone(&live);
        let peak = Arc::clone(&peak);
        w.spawn(SIZE, async move {
            let now = live.fetch_add(SIZE, Ordering::SeqCst) + SIZE;
            peak.fetch_max(now, Ordering::SeqCst);
            tokio::task::yield_now().await;
            live.fetch_sub(SIZE, Ordering::SeqCst);
            Ok(())
        });
    }
    w.drain().await.unwrap();

    assert!(
        peak.load(Ordering::SeqCst) <= BYTE_BUDGET,
        "peak resident {} exceeded BYTE_BUDGET {}",
        peak.load(Ordering::SeqCst),
        BYTE_BUDGET
    );
}

/// A single file bigger than the whole byte budget must still be admitted, or
/// the deploy deadlocks. (Reachable only below the chunking threshold in
/// practice, but the window must not depend on that.)
#[tokio::test]
async fn an_oversized_lone_file_is_still_admitted() {
    let mut w = UploadWindow::new();
    w.reserve(BYTE_BUDGET * 4).await.unwrap();
    w.spawn(BYTE_BUDGET * 4, async { Ok(()) });
    w.drain().await.unwrap();
}

/// Once the deploy is going to fail, continuing to spend a slow user's uplink
/// is pure cost — and the whole incident was about a user whose uplink was the
/// scarce resource.
#[tokio::test]
async fn the_first_error_surfaces_and_stops_the_deploy() {
    let mut w = UploadWindow::new();
    w.reserve(1).await.unwrap();
    w.spawn(1, async { Err("upload of audio/dreamin.mp3 failed (524)".into()) });

    // The error surfaces from whichever call harvests it.
    let err = match w.reserve(1).await {
        Err(e) => e,
        Ok(()) => match w.drain().await {
            Err(e) => e,
            Ok(()) => panic!("a failed task must surface its error"),
        },
    };
    assert!(err.contains("524"), "{err}");
}

/// A panicking task never returns its size. If the window leaked those bytes it
/// would wedge at a permanently-full budget — but it fails the deploy anyway,
/// so the requirement is simply that the panic is reported, not swallowed.
#[tokio::test]
async fn a_panicking_task_is_reported_not_swallowed() {
    let mut w = UploadWindow::new();
    w.reserve(1).await.unwrap();
    w.spawn(1, async { panic!("task blew up") });

    let err = match w.drain().await {
        Err(e) => e,
        Ok(()) => panic!("a panicking task must surface an error"),
    };
    assert!(err.contains("panicked"), "{err}");
}

/// Draining an untouched window is a no-op, not a hang — `push_site_inner`
/// reaches `drain()` even when `need` is empty.
#[tokio::test]
async fn draining_an_empty_window_returns_immediately() {
    let mut w = UploadWindow::new();
    w.drain().await.unwrap();
}

/// A chunked upload seeks and reads one planned request at a time, so what it
/// actually occupies is that request. Charging it the whole file made every
/// asset over `BYTE_BUDGET` fail admission against any other in-flight work and
/// run alone through the `in_flight_count == 0` hatch: three videos on a
/// GFW-boundary link uploaded strictly end-to-end, and their 71 KB/s aggregate
/// came in *under* the 77-84 KB/s each had reached by itself.
#[tokio::test]
async fn a_chunked_file_is_admitted_at_its_request_size_not_its_own() {
    let w = UploadWindow::new();
    let big = BYTE_BUDGET * 3;
    assert!(w.throughput().needs_chunking(big), "fixture must take the chunked path");
    assert_eq!(w.admission_cost(big), w.throughput().plan_request_size() as u64);

    // A single-PUT file is read into memory whole, so it is charged whole.
    let small = 4 * 1024;
    assert!(!w.throughput().needs_chunking(small));
    assert_eq!(w.admission_cost(small), small);
}

/// The consequence, stated against `admits` so it cannot race: two files that
/// each dwarf the budget are now admissible together, where the old accounting
/// refused the second until the first had finished.
#[tokio::test]
async fn two_files_larger_than_the_budget_can_share_the_window() {
    use crate::seta::upload_policy::admits;
    let w = UploadWindow::new();
    let big = BYTE_BUDGET * 3;
    let cost = w.admission_cost(big);

    assert!(
        admits(1, cost, cost, LIMIT_START),
        "a second chunked file must be admissible alongside the first"
    );
    assert!(
        !admits(1, big, big, LIMIT_START),
        "charging the whole file is what made it inadmissible — the defect this fixes"
    );
}
