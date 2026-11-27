//! Tests for the integrity checks and self-heal rules a deploy's file reads go through.

use super::*;

fn tmp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/test-tmp/upload-tests");
    std::fs::create_dir_all(&base).expect("create target/test-tmp/upload-tests");
    let path = base.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

// ── Streaming and buffered digests ───────────────────────────────────────────

/// Streaming and buffered digests must agree, or a file would verify on the
/// single-PUT path and fail on the chunked path purely because of its size.
#[test]
fn streaming_and_buffered_digests_agree() {
    // Larger than the 64 KB streaming buffer so it spans multiple reads.
    let bytes: Vec<u8> = (0u32..40_000).flat_map(|i| i.to_le_bytes()).collect();
    let path = tmp_file("streaming-agreement.bin", &bytes);
    assert_eq!(
        hash_bytes(&bytes),
        hash_file(&path).expect("hash file"),
        "streaming digest must equal the buffered one"
    );
    assert_eq!(hash_bytes(&bytes).len(), 16, "xxh3_64 is 16 hex chars");
}

/// An empty manifest hash means a legacy or regressed entry. Failing the deploy
/// would strand the user behind a stale build cache with no way forward.
#[test]
fn an_empty_manifest_hash_skips_verification_rather_than_failing() {
    assert!(verify_bytes("f.txt", b"anything", "").is_ok());
}

#[test]
fn a_wrong_hash_names_the_file_and_both_hashes_so_the_error_is_actionable() {
    let err = verify_bytes("assets/photo.jpg", b"bytes", "deadbeefdeadbeef")
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
    let expected = hash_bytes(real);
    let evicted = vec![0u8; real.len()];
    assert!(verify_bytes("assets/photo.jpg", &evicted, &expected).is_err());
}

// ── Self-heal on a drifted hash ──────────────────────────────────────────────

/// A file a background rebuild rewrote between the seal and the upload is
/// shipped as it is on disk once it has stopped changing: its settled hash
/// comes back so the caller corrects the manifest entry.
#[tokio::test]
async fn a_drifted_hash_that_holds_still_self_heals_to_the_bytes_on_disk() {
    let bytes = b"the bytes actually on disk right now";
    let path = tmp_file("drift.bin", bytes);
    let throughput = upload_policy::Throughput::new(1);

    let healed = settled_hash("assets/raw/index.html", &path, "0000000000000000", &throughput, 100).await;

    assert_eq!(healed, Ok(Some(hash_bytes(bytes))));
}

/// A file still changing when the settle pause elapses is refused, never healed.
#[tokio::test]
async fn a_still_changing_file_does_not_self_heal() {
    let path = tmp_file("still-changing.bin", b"version one, on disk when the first read happens");
    let writer = path.clone();
    tokio::spawn(async move {
        tokio::time::sleep(DRIFT_SETTLE_DELAY / 4).await;
        tokio::fs::write(&writer, b"version two, landed mid-settle pause").await.expect("overwrite mid-settle");
    });
    let throughput = upload_policy::Throughput::new(1);

    let err = settled_hash("assets/raw/still-changing.bin", &path, "0000000000000000", &throughput, 100).await.unwrap_err();

    assert!(err.contains("still changing"), "{err}");
}

/// An iCloud eviction zeroes a file in place, and that state is stable across
/// both reads: content, not a torn write, is what makes it wrong to ship.
#[tokio::test]
async fn a_zeroed_stub_does_not_self_heal_even_though_it_is_stable() {
    let path = tmp_file("zeroed-stub.bin", &[0u8; 4096]);
    let throughput = upload_policy::Throughput::new(1);

    let err = settled_hash("assets/photo.jpg", &path, "0000000000000001", &throughput, 100).await.unwrap_err();

    assert!(err.contains("zeroed stub"), "{err}");
}

/// More drifted files than [`self_heal_cap`] in one deploy is something
/// systemic, so the one that crosses the cap fails the publish.
#[tokio::test]
async fn a_self_heal_cap_stops_healing_the_rest_of_the_deploy() {
    let throughput = upload_policy::Throughput::new(1);
    let cap = 2;
    for n in 0..cap {
        let path = tmp_file(&format!("cap-{n}.bin"), format!("real content #{n}").as_bytes());
        let healed = settled_hash(&format!("assets/raw/cap-{n}.html"), &path, "0000000000000000", &throughput, cap).await;
        assert!(matches!(healed, Ok(Some(_))), "file #{n} is within the cap: {healed:?}");
    }
    let path = tmp_file("cap-over.bin", b"real content #over-cap");

    let err = settled_hash("assets/raw/cap-over.html", &path, "0000000000000000", &throughput, cap).await.unwrap_err();

    assert!(err.contains("drifted from the sealed manifest") && err.contains(&format!("cap {cap}")), "{err}");
}

/// Sizes cover regular-file entries only, at their exact length; a symlink
/// entry and a file missing on disk get no key.
#[cfg(unix)]
#[tokio::test]
async fn regular_file_sizes_cover_regular_files_only() {
    use crate::types::content::{file_entry, symlink_entry};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/a.html"), b"12345").unwrap();
    std::fs::write(dir.path().join("old.html"), b"123").unwrap();
    std::os::unix::fs::symlink("docs/a.html", dir.path().join("latest")).unwrap();
    let manifest: std::collections::HashMap<String, String> = [
        ("docs/a.html".to_string(), file_entry("h1")),
        ("old.html".to_string(), "h2".to_string()),
        ("latest".to_string(), symlink_entry("docs/a.html")),
        ("gone.html".to_string(), file_entry("h3")),
    ]
    .into();
    let sizes = regular_file_sizes(dir.path(), &manifest).await.unwrap();
    assert_eq!(sizes, [("docs/a.html".to_string(), 5u64), ("old.html".to_string(), 3)].into());
}
