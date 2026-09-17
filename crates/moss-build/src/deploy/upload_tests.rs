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

// ── Self-heal on a drifted hash ──────────────────────────────────────────────

/// The bug this module was fixed for: a raw/background asset on a
/// Google-Drive-synced vault can legitimately change on disk in the window
/// between the deploy manifest being sealed and this upload running (a
/// second build racing the first, not corruption). Before this fix, a
/// mismatch here made `compare()` return `Err`, which `upload_regular_file`
/// propagated straight up and `UploadWindow` turned into a whole-deploy abort
/// over one file. The file's current bytes are always available by the time
/// this runs, so they are what gets shipped; the drift is only logged.
///
/// Exercises the single-PUT (buffered) branch of `upload_regular_file`
/// against a real HTTP mock, so the assertion is the actual return value of
/// the function under test, not just `compare()`'s.
#[tokio::test]
async fn a_drifted_hash_self_heals_on_the_single_put_path() {
    let mut server = mockito::Server::new_async().await;
    let bytes = b"the bytes actually on disk right now";
    let mock = server
        .mock("PUT", mockito::Matcher::Any)
        .match_body(mockito::Matcher::Exact(
            String::from_utf8_lossy(bytes).into_owned(),
        ))
        .with_status(200)
        .create_async()
        .await;

    let path = tmp_file("drift-single-put.bin", bytes);
    let identity = crate::identity::Identity::generate().expect("generate identity");
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(&identity, &server.url());
    let throughput = upload_policy::Throughput::new();

    // A manifest hash that cannot possibly match `bytes` — standing in for a
    // sealed hash the file has since drifted away from.
    let stale_hash = "0000000000000000";

    let result = upload_regular_file(
        &client,
        "her-blog",
        "assets/raw/index.html",
        &path,
        bytes.len() as u64,
        "abc123def456abcd",
        stale_hash,
        HashAlgo::Xxh3,
        &throughput,
        100, // self_heal_cap: generous, not what this test is about
        None,
    )
    .await;

    let healed_hash = result
        .expect("a hash drift stable across the settle pause must self-heal, not fail the deploy");
    assert_eq!(
        healed_hash,
        Some(HashAlgo::Xxh3.hash_bytes(bytes)),
        "the caller must get back the hash actually shipped, to correct commit_sync's manifest"
    );
    mock.assert_async().await;
}

/// Same self-heal, exercised on the streaming/chunked branch — the other call
/// site inside `upload_regular_file`, which has its own `compare()` call
/// (`hash_file` rather than `verify_bytes`) and its own upload call
/// (`upload_file_chunked` rather than `upload_file`).
///
/// The file is sized one byte over a fresh `Throughput`'s `plan_request_size()`
/// (seeded at [`upload_policy::INITIAL_REQUEST_SIZE`] — see that constant's
/// doc comment) so `needs_chunking` is true with no throughput manipulation,
/// on a real, unmodified deploy-time estimate. The mock server accepts any
/// number of chunk PATCHes before the completing POST, so the test does not
/// depend on exactly how the chunk loop divides the file.
#[tokio::test]
async fn a_drifted_hash_self_heals_on_the_chunked_path() {
    use tokio::net::TcpListener;

    // `crate::test_mock_http_conn` (shared with `seta::chunked_upload_tests`
    // and `deploy::push::tests::mock_seta_sequence`) does the drain +
    // respond + shutdown; only the "which request was this" decision is
    // specific to this test.
    const OK_EMPTY: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

    let listener = TcpListener::bind::<std::net::SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // conn 0: GET /uploads -> 200 [] (nothing staged; upload from zero).
        {
            let (stream, _) = listener.accept().await.unwrap();
            crate::test_mock_http_conn(
                stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]",
            )
            .await;
        }
        // conn 1: POST /upload -> {"uploadId":"t"}.
        {
            let (stream, _) = listener.accept().await.unwrap();
            crate::test_mock_http_conn(
                stream,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"uploadId\":\"t\"}",
            )
            .await;
        }
        // conn 2..N: one or more PATCH chunks, then the completing POST —
        // whichever request line names ".../complete" ends the loop.
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let raw = crate::test_mock_http_conn(stream, OK_EMPTY).await;
            let is_complete = String::from_utf8_lossy(&raw)
                .lines()
                .next()
                .is_some_and(|line| line.contains("/complete"));
            if is_complete {
                break;
            }
        }
    });

    // One byte over a fresh Throughput's plan_request_size() so needs_chunking
    // is true without touching the throughput estimate at all.
    let throughput = upload_policy::Throughput::new();
    let file_size = throughput.plan_request_size() + 4096;
    let bytes = vec![0xABu8; file_size];
    let path = tmp_file("drift-chunked.bin", &bytes);

    let identity = crate::identity::Identity::generate().expect("generate identity");
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(
        &identity,
        &format!("http://{}", addr),
    );

    let stale_hash = "0000000000000000";

    let result = upload_regular_file(
        &client,
        "her-blog",
        "assets/raw/video-poster.bin",
        &path,
        file_size as u64,
        "abc123def456abcd",
        stale_hash,
        HashAlgo::Xxh3,
        &throughput,
        100, // self_heal_cap: generous, not what this test is about
        None,
    )
    .await;

    let healed_hash = result.expect(
        "a hash drift stable across the settle pause must self-heal on the chunked path too, \
         not fail the deploy",
    );
    assert_eq!(
        healed_hash,
        Some(HashAlgo::Xxh3.hash_bytes(&bytes)),
        "the caller must get back the hash actually shipped, to correct commit_sync's manifest"
    );
}

// ── Self-heal is guarded, not automatic ──────────────────────────────────────

/// A single fresh read is not proof of anything — the whole point of the
/// settle-then-recheck. A file that is STILL CHANGING when the settle pause
/// elapses must be refused loudly, the same as before self-heal existed.
/// Simulated by overwriting the file partway through `DRIFT_SETTLE_DELAY`,
/// well before `upload_regular_file`'s second read fires.
#[tokio::test]
async fn a_still_changing_file_does_not_self_heal() {
    let bytes_v1 = b"version one, on disk when the first read happens";
    let bytes_v2 = b"version two, landed mid-settle pause, not the same content";
    let path = tmp_file("still-changing.bin", bytes_v1);

    let path_for_writer = path.clone();
    let bytes_v2_owned = bytes_v2.to_vec();
    tokio::spawn(async move {
        tokio::time::sleep(DRIFT_SETTLE_DELAY / 4).await;
        tokio::fs::write(&path_for_writer, &bytes_v2_owned)
            .await
            .expect("overwrite mid-settle");
    });

    let identity = crate::identity::Identity::generate().expect("generate identity");
    // Never actually dialed: a still-changing file is rejected before any
    // network call is made, so this URL only needs to parse.
    let client =
        crate::seta::client::MossSetaClient::with_identity_and_url(&identity, "http://127.0.0.1:1");
    let throughput = upload_policy::Throughput::new();

    let result = upload_regular_file(
        &client,
        "her-blog",
        "assets/raw/still-changing.bin",
        &path,
        bytes_v1.len() as u64,
        "abc123def456abcd",
        "0000000000000000",
        HashAlgo::Xxh3,
        &throughput,
        100,
        None,
    )
    .await;

    let err =
        result.expect_err("a file still changing across the settle pause must not self-heal");
    assert!(err.contains("still changing"), "{err}");
}

/// Stability alone is not sufficient either: the concrete case the module was
/// hardened for. An iCloud "optimize storage" eviction zeroes a file in
/// place, and that zeroed state is perfectly STABLE across two reads — it is
/// content, not a torn write, that makes it wrong to ship. Same fixture as
/// `a_zeroed_icloud_stub_is_rejected_before_it_is_uploaded`, but exercised at
/// `upload_regular_file`'s level so the settle-then-recheck path is what is
/// actually pinned, not just the lower-level `verify_bytes` primitive it
/// wraps.
#[tokio::test]
async fn a_zeroed_stub_does_not_self_heal_even_though_it_is_stable() {
    let real_size = 4096;
    let evicted = vec![0u8; real_size];
    let path = tmp_file("zeroed-stub.bin", &evicted);

    let identity = crate::identity::Identity::generate().expect("generate identity");
    let client =
        crate::seta::client::MossSetaClient::with_identity_and_url(&identity, "http://127.0.0.1:1");
    let throughput = upload_policy::Throughput::new();

    // A hash for the REAL (non-zero) content this manifest entry was sealed
    // against — any value that isn't xxh3(all-zero bytes) demonstrates the
    // point, since the file never changes and both reads agree on "zero".
    let sealed_hash_for_real_content = "0000000000000001";

    let result = upload_regular_file(
        &client,
        "her-blog",
        "assets/photo.jpg",
        &path,
        real_size as u64,
        "abc123def456abcd",
        sealed_hash_for_real_content,
        HashAlgo::Xxh3,
        &throughput,
        100,
        None,
    )
    .await;

    let err = result.expect_err(
        "a zeroed iCloud stub must not self-heal even though it is stable across two reads",
    );
    assert!(err.contains("zeroed stub"), "{err}");
}

/// A drifted hash or two in one deploy is the race this module exists to
/// tolerate. More than [`self_heal_cap`]'s budget, in the SAME deploy, must
/// fail loudly instead of quietly healing every remaining file — the
/// signature of something systemic (a stale stage, a wrong directory), not
/// an isolated race. Driven directly at `upload_regular_file`'s level
/// (rather than through a whole `push_site_inner` deploy) with an explicit
/// low cap, so the assertion is deterministic and does not depend on how
/// many files a real deploy happens to need.
#[tokio::test]
async fn a_self_heal_cap_stops_healing_the_rest_of_the_deploy() {
    let identity = crate::identity::Identity::generate().expect("generate identity");
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("PUT", mockito::Matcher::Any)
        .with_status(200)
        .create_async()
        .await;
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(&identity, &server.url());
    let throughput = upload_policy::Throughput::new();
    let cap = 2;

    for n in 0..cap {
        let bytes = format!("real content #{n}").into_bytes();
        let path = tmp_file(&format!("cap-{n}.bin"), &bytes);
        let result = upload_regular_file(
            &client,
            "her-blog",
            &format!("assets/raw/cap-{n}.html"),
            &path,
            bytes.len() as u64,
            "abc123def456abcd",
            "0000000000000000",
            HashAlgo::Xxh3,
            &throughput,
            cap,
            None,
        )
        .await;
        assert!(result.is_ok(), "file #{n} is within the cap: {result:?}");
    }

    // One more drifted file, still within the same deploy's shared
    // Throughput — this is the one that crosses the cap.
    let bytes = b"real content #over-cap".to_vec();
    let path = tmp_file("cap-over.bin", &bytes);
    let result = upload_regular_file(
        &client,
        "her-blog",
        "assets/raw/cap-over.html",
        &path,
        bytes.len() as u64,
        "abc123def456abcd",
        "0000000000000000",
        HashAlgo::Xxh3,
        &throughput,
        cap,
        None,
    )
    .await;

    let err = result.expect_err("crossing the self-heal cap must fail loudly, not heal silently");
    assert!(err.contains("drifted from the sealed manifest"), "{err}");
    assert!(err.contains(&format!("cap {cap}")), "{err}");
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
