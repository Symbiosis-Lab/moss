use super::*;
use std::io::Write;

/// Helper: create a unique temp directory for each test.
fn make_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("moss_cache_tests")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

/// Helper: write arbitrary content to a temp file and return its path.
fn write_temp_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    let mut f = fs::File::create(&path).expect("create temp file");
    f.write_all(content).expect("write temp file");
    path
}

// -----------------------------------------------------------------------
// ObjectStore tests
// -----------------------------------------------------------------------

#[test]
fn test_hash_file_known_content() {
    let dir = make_test_dir("hash_known");
    let path = write_temp_file(&dir, "hello.txt", b"hello world\n");

    let hash = ObjectStore::hash_file(&path).expect("hash_file");

    // SHA-256 of "hello world\n" (with newline).
    // $ printf 'hello world\n' | shasum -a 256
    // a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447
    assert_eq!(
        hash,
        "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
    );
    assert_eq!(hash.len(), 64);
}

#[test]
fn hash_file_does_not_wait_for_the_cloud_on_an_ordinary_error() {
    // Only EDEADLK means "evicted, ask for it back". Every other failure —
    // here a missing file — must surface at once rather than burn the
    // 90-second materialization deadline on a file no download can produce.
    let dir = make_test_dir("hash_missing");
    let missing = dir.join("nope.bin");

    let start = std::time::Instant::now();
    let err = ObjectStore::hash_file(&missing).expect_err("missing file must fail");

    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "an ordinary error must not enter the materialize-and-poll wait"
    );
    assert!(err.contains("nope.bin"), "error names the file: {err}");
}

#[test]
fn test_store_file_creates_sharded_path() {
    let dir = make_test_dir("store_sharded");
    let objects_dir = dir.join("objects");
    let store = ObjectStore::new(objects_dir.clone());

    let src = write_temp_file(&dir, "data.bin", b"some content");
    let oid = store.store_file(&src).expect("store_file");

    let expected = objects_dir.join(&oid[..2]).join(&oid[2..4]).join(&oid);
    assert!(expected.exists(), "blob should exist at sharded path");
}

#[test]
fn test_store_file_idempotent() {
    let dir = make_test_dir("store_idempotent");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"idempotent content");
    let oid1 = store.store_file(&src).expect("first store");
    let oid2 = store.store_file(&src).expect("second store");

    assert_eq!(oid1, oid2, "same content should produce the same OID");
}

#[test]
fn test_get_path_missing() {
    let dir = make_test_dir("get_path_missing");
    let store = ObjectStore::new(dir.join("objects"));

    let fake_oid = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(store.get_path(fake_oid).is_none());
}

#[test]
fn test_get_path_exists() {
    let dir = make_test_dir("get_path_exists");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"get_path test");
    let oid = store.store_file(&src).expect("store_file");

    let path = store.get_path(&oid);
    assert!(path.is_some(), "get_path should return Some after store");
    assert!(path.unwrap().exists());
}

#[test]
fn test_get_path_rejects_zero_byte_blob() {
    let dir = make_test_dir("get_path_zero");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"non-empty content");
    let oid = store.store_file(&src).expect("store_file");

    // Simulate iCloud eviction: zero out the blob's content.
    // iCloud Drive can silently zero out file data to reclaim disk space.
    // When this happens, the file still exists on disk (passes exists())
    // but contains no data. get_path must reject these 0-byte blobs to
    // prevent cache hits on corrupt data.
    let blob_path = store.blob_path(&oid);
    fs::write(&blob_path, b"").expect("truncate to simulate eviction");

    assert!(
        store.get_path(&oid).is_none(),
        "get_path must return None for 0-byte blobs (iCloud eviction)"
    );
}

#[test]
#[cfg(unix)]
fn test_link_to_creates_independent_copy() {
    use std::os::unix::fs::MetadataExt;

    let dir = make_test_dir("link_independent");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"independent copy test content");
    let oid = store.store_file(&src).expect("store_file");

    let target = dir.join("output").join("linked_file");
    store.link_to(&oid, &target).expect("link_to");

    assert!(target.exists(), "target should exist after link_to");

    // Target must be an independent copy (different inode), NOT a hardlink.
    // Hardlinks share fate with iCloud eviction — when iCloud zeroes out a
    // file's data, all hardlinked copies become 0 bytes simultaneously.
    // Using fs::copy (which does COW cloning on APFS) creates an independent
    // inode that survives eviction of the source.
    let blob_ino = fs::metadata(store.blob_path(&oid))
        .expect("blob metadata")
        .ino();
    let target_ino = fs::metadata(&target).expect("target metadata").ino();

    assert_ne!(
        blob_ino, target_ino,
        "blob and target must have different inodes (independent copy, not hardlink)"
    );

    // Content should still match.
    let content = fs::read(&target).expect("read target");
    assert_eq!(content, b"independent copy test content");
}

#[test]
fn test_link_to_replaces_existing_target() {
    let dir = make_test_dir("link_replace");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"replace test content");
    let oid = store.store_file(&src).expect("store_file");

    // Create a pre-existing file at the target location.
    let target = dir.join("existing_file.txt");
    fs::write(&target, b"old content").expect("write old file");

    store.link_to(&oid, &target).expect("link_to");

    let content = fs::read(&target).expect("read target");
    assert_eq!(content, b"replace test content");
}

#[test]
fn test_link_to_creates_parent_dirs() {
    let dir = make_test_dir("link_parents");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"parent dir test");
    let oid = store.store_file(&src).expect("store_file");

    // Target is nested in directories that don't exist yet.
    let target = dir.join("a").join("b").join("c").join("output.bin");
    assert!(!target.parent().unwrap().exists());

    store.link_to(&oid, &target).expect("link_to");
    assert!(target.exists());
}

#[test]
fn test_link_to_writes_full_blob_no_tmp_leak() {
    // After a successful link_to, no `.tmp.` siblings remain in the
    // target's parent directory. The atomic temp+rename pattern must
    // clean up its own tmp file.
    let dir = make_test_dir("link_no_tmp_leak");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"hello world payload");
    let oid = store.store_file(&src).expect("store_file");

    let target = dir.join("out").join("hello.bin");
    store.link_to(&oid, &target).expect("link_to");

    assert_eq!(
        fs::read(&target).expect("read target"),
        b"hello world payload"
    );

    let stray: Vec<_> = fs::read_dir(target.parent().unwrap())
        .expect("read out dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(stray.is_empty(), "leftover tmp files: {:?}", stray);
}

#[test]
fn test_link_to_overwrites_existing_zero_byte_target() {
    // The user's bug state: a 0-byte stub from a previous killed link_to.
    // The new link_to must replace it with the full blob.
    let dir = make_test_dir("link_overwrite_zero");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "data.bin", b"full content");
    let oid = store.store_file(&src).expect("store_file");

    let target = dir.join("out").join("stub.bin");
    fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
    fs::File::create(&target).expect("create stub");
    assert_eq!(fs::metadata(&target).expect("stat stub").len(), 0);

    store.link_to(&oid, &target).expect("link_to");
    assert_eq!(fs::read(&target).expect("read target"), b"full content");
}

#[test]
fn test_blob_path_sharding() {
    let base = PathBuf::from("/cache/objects");
    let store = ObjectStore::new(base.clone());

    let oid = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let path = store.blob_path(oid);

    assert_eq!(path, base.join("ab").join("cd").join(oid));
}

// -----------------------------------------------------------------------
// ObjectStore::validate_blob tests
// -----------------------------------------------------------------------

#[test]
fn test_validate_blob_removes_zero_byte_blob() {
    let dir = make_test_dir("validate_zero");
    let store = ObjectStore::new(dir.join("objects"));
    let src = write_temp_file(&dir, "photo.jpg", b"real image data");
    let oid = store.store_file(&src).expect("store_file");
    let blob_path = store.blob_path(&oid);

    // Corrupt the blob to 0 bytes (simulating iCloud race).
    fs::write(&blob_path, b"").expect("truncate blob");
    assert_eq!(fs::metadata(&blob_path).unwrap().len(), 0);

    let result = store.validate_blob(&oid, 15);
    assert!(result.is_err(), "should reject 0-byte blob");
    assert!(!blob_path.exists(), "corrupt blob should be removed");
}

#[test]
fn test_validate_blob_accepts_healthy_blob() {
    let dir = make_test_dir("validate_healthy");
    let store = ObjectStore::new(dir.join("objects"));
    let src = write_temp_file(&dir, "photo.jpg", b"real image data");
    let oid = store.store_file(&src).expect("store_file");

    let result = store.validate_blob(&oid, 15);
    assert!(result.is_ok(), "healthy blob should validate");
    assert!(store.blob_path(&oid).exists(), "blob should still exist");
}

#[test]
fn test_validate_blob_accepts_zero_source_zero_blob() {
    let dir = make_test_dir("validate_zero_source");
    let store = ObjectStore::new(dir.join("objects"));
    let src = write_temp_file(&dir, "empty.txt", b"");
    let oid = store.store_file(&src).expect("store_file");

    // source_size=0 and blob_size=0 is valid (genuinely empty file).
    let result = store.validate_blob(&oid, 0);
    assert!(result.is_ok(), "0-byte source + 0-byte blob is valid");
}

// -----------------------------------------------------------------------
// ObjectStore::store_file self-healing tests
// -----------------------------------------------------------------------

#[test]
fn test_store_file_self_heals_zero_byte_blob() {
    let dir = make_test_dir("store_self_heal");
    let store = ObjectStore::new(dir.join("objects"));
    let src = write_temp_file(&dir, "photo.jpg", b"real image data");
    let oid = store.store_file(&src).expect("first store");
    let blob_path = store.blob_path(&oid);

    // Corrupt the blob to 0 bytes.
    fs::write(&blob_path, b"").expect("truncate");
    assert_eq!(fs::metadata(&blob_path).unwrap().len(), 0);

    // Re-storing should self-heal: detect corrupt, remove, re-copy.
    let oid2 = store.store_file(&src).expect("re-store should succeed");
    assert_eq!(oid, oid2, "OID should be the same");
    let blob_size = fs::metadata(&blob_path).unwrap().len();
    assert_eq!(blob_size, 15, "blob should be re-stored with correct size");
}

#[test]
fn test_store_file_genuinely_empty_is_fine() {
    let dir = make_test_dir("store_zero_copy");
    let store = ObjectStore::new(dir.join("objects"));
    let empty_src = write_temp_file(&dir, "empty.txt", b"");
    let result = store.store_file(&empty_src);
    assert!(result.is_ok(), "genuinely empty file should store fine");
}

// -----------------------------------------------------------------------
// ObjectStore::link_to 0-byte rejection test
// -----------------------------------------------------------------------

#[test]
fn test_link_to_rejects_zero_byte_blob() {
    let dir = make_test_dir("link_zero");
    let store = ObjectStore::new(dir.join("objects"));
    let src = write_temp_file(&dir, "photo.jpg", b"real image data");
    let oid = store.store_file(&src).expect("store_file");
    let blob_path = store.blob_path(&oid);

    // Corrupt the blob to 0 bytes.
    fs::write(&blob_path, b"").expect("truncate");

    let target = dir.join("output").join("photo.jpg");
    let result = store.link_to(&oid, &target);
    assert!(result.is_err(), "link_to should reject 0-byte blob");
    assert!(!target.exists(), "target should not be created");
}

/// Verify that link_to checks the byte count returned by fs::copy.
///
/// Background: On iCloud Drive, fs::copy can silently produce truncated
/// copies (e.g. 1 MiB instead of 99 MiB) when the source blob is being
/// downloaded. Without verification, the truncated copy persists in the
/// canonical dir and the fast-path perpetuates it across rebuilds.
/// See: fix/video-truncation-fast-path
#[test]
fn test_link_to_verifies_copy_size() {
    let dir = make_test_dir("link_verify_size");
    let store = ObjectStore::new(dir.join("objects"));
    let content = b"content that should be fully copied";
    let src = write_temp_file(&dir, "data.bin", content);
    let oid = store.store_file(&src).expect("store_file");

    let target = dir.join("output").join("data.bin");
    store
        .link_to(&oid, &target)
        .expect("link_to should succeed");

    // Verify the target has the exact same size as the blob.
    let blob_size = fs::metadata(store.blob_path(&oid)).unwrap().len();
    let target_size = fs::metadata(&target).unwrap().len();
    assert_eq!(
        blob_size, target_size,
        "link_to target must match blob size exactly"
    );
}

// -----------------------------------------------------------------------
// TransformCache tests
// -----------------------------------------------------------------------

#[test]
fn test_transform_cache_roundtrip() {
    let dir = make_test_dir("transform_roundtrip");
    let objects = ObjectStore::new(dir.join("objects"));
    let cache = TransformCache::new(dir.join("transforms"), objects);

    let mut transforms = HashMap::new();
    transforms.insert(
        "thumbnail".to_string(),
        TransformEntry {
            oid: "bbbb".repeat(16),
            size: 1024,
            params: serde_json::json!({"width": 200}),
        },
    );

    let record = TransformRecord {
        source_oid: "aaaa".repeat(16),
        source_size: 5000,
        transforms,
    };

    cache.put(&record).expect("put");

    let loaded = cache.get(&record.source_oid);
    assert!(loaded.is_some(), "get should return the record we put");
    assert_eq!(loaded.unwrap(), record);
}

#[test]
fn test_record_path_replaces_colon() {
    // `:` is the NTFS alternate-data-stream separator and can't appear in an
    // ordinary Windows path component. Real transform keys carry it in two
    // shapes — `xxh3:<hash>` and `stat:<relative_path>:<size>:<mtime>` — so
    // exercise both.
    let cache = TransformCache::new(
        PathBuf::from("/cache/transforms"),
        ObjectStore::new(PathBuf::from("/cache/objects")),
    );

    for oid in ["xxh3:5af3d592d3825d67", "stat:隨筆.md:1234:5678"] {
        let path = cache.record_path(oid);
        assert!(
            !path.to_string_lossy().contains(':'),
            "record path for {:?} must not contain ':' (illegal on NTFS): {}",
            oid,
            path.display()
        );
    }
}

#[test]
fn test_find_cached_output_hit() {
    let dir = make_test_dir("find_hit");
    let store = ObjectStore::new(dir.join("objects"));

    // Store a file so there's a real blob for the output OID.
    let src = write_temp_file(&dir, "output.bin", b"transformed output");
    let output_oid = store.store_file(&src).expect("store output");

    // Store the source file too.
    let source_src = write_temp_file(&dir, "source.bin", b"original source");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash source");

    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let params = serde_json::json!({"width": 200});
    let mut transforms = HashMap::new();
    transforms.insert(
        "thumbnail".to_string(),
        TransformEntry {
            oid: output_oid.clone(),
            size: 18,
            params: params.clone(),
        },
    );

    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 15,
        transforms,
    };
    cache.put(&record).expect("put");

    let result = cache.find_cached_output(&source_oid, "thumbnail", &params);
    assert_eq!(result, Some(output_oid));
}

#[test]
fn test_find_cached_output_params_mismatch() {
    let dir = make_test_dir("find_params_mismatch");
    let store = ObjectStore::new(dir.join("objects"));

    let src = write_temp_file(&dir, "output.bin", b"transformed output");
    let output_oid = store.store_file(&src).expect("store output");

    let source_src = write_temp_file(&dir, "source.bin", b"original source");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash source");

    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let stored_params = serde_json::json!({"width": 200});
    let query_params = serde_json::json!({"width": 400});

    let mut transforms = HashMap::new();
    transforms.insert(
        "thumbnail".to_string(),
        TransformEntry {
            oid: output_oid,
            size: 18,
            params: stored_params,
        },
    );

    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 15,
        transforms,
    };
    cache.put(&record).expect("put");

    let result = cache.find_cached_output(&source_oid, "thumbnail", &query_params);
    assert!(result.is_none(), "different params should not match");
}

#[test]
fn test_find_cached_output_blob_missing() {
    let dir = make_test_dir("find_blob_missing");

    // We intentionally do NOT store a blob for the output OID.
    let fake_output_oid = "cccc".repeat(16);

    let source_src = write_temp_file(&dir, "source.bin", b"original source");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash source");

    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let params = serde_json::json!({"width": 200});
    let mut transforms = HashMap::new();
    transforms.insert(
        "thumbnail".to_string(),
        TransformEntry {
            oid: fake_output_oid,
            size: 0,
            params: params.clone(),
        },
    );

    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 15,
        transforms,
    };
    cache.put(&record).expect("put");

    let result = cache.find_cached_output(&source_oid, "thumbnail", &params);
    assert!(
        result.is_none(),
        "should return None when blob is missing from object store"
    );
}

#[test]
fn test_find_cached_output_rejects_zero_byte_blob() {
    let dir = make_test_dir("find_zero_blob");
    let store = ObjectStore::new(dir.join("objects"));

    // Store a real blob, then zero it out to simulate iCloud eviction.
    let output_src = write_temp_file(&dir, "output.mp4", b"video data here");
    let output_oid = store.store_file(&output_src).expect("store output");

    let source_src = write_temp_file(&dir, "source.mov", b"source video");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash source");

    let cache = TransformCache::new(dir.join("transforms"), store);

    let params = serde_json::json!({"crf": 18});
    let mut transforms = HashMap::new();
    transforms.insert(
        "video/mp4".to_string(),
        TransformEntry {
            oid: output_oid.clone(),
            size: 15,
            params: params.clone(),
        },
    );

    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 12,
        transforms,
    };
    cache.put(&record).expect("put");

    // Verify cache hit works before eviction.
    assert!(
        cache
            .find_cached_output(&source_oid, "video/mp4", &params)
            .is_some(),
        "should find cached output before eviction"
    );

    // Simulate iCloud eviction: zero out the blob.
    let blob_path = cache.objects.blob_path(&output_oid);
    fs::write(&blob_path, b"").expect("truncate to simulate eviction");

    // After eviction, find_cached_output must return None (not a stale hit).
    let result = cache.find_cached_output(&source_oid, "video/mp4", &params);
    assert!(
        result.is_none(),
        "find_cached_output must reject 0-byte blobs (iCloud eviction)"
    );
}

#[test]
fn test_find_cached_output_no_record() {
    let dir = make_test_dir("find_no_record");
    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let fake_oid = "dddd".repeat(16);
    let params = serde_json::json!({"width": 200});

    let result = cache.find_cached_output(&fake_oid, "thumbnail", &params);
    assert!(result.is_none(), "should return None when no record exists");
}

#[test]
fn test_transform_cache_remove() {
    let dir = make_test_dir("transform_remove");
    let store = ObjectStore::new(dir.join("objects"));

    // Store a file so there's a real blob for the output OID.
    let src = write_temp_file(&dir, "output.bin", b"remove test output");
    let output_oid = store.store_file(&src).expect("store output");

    // Hash a source file.
    let source_src = write_temp_file(&dir, "source.bin", b"remove test source");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash source");

    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let params = serde_json::json!({"width": 200});
    let mut transforms = HashMap::new();
    transforms.insert(
        "thumbnail".to_string(),
        TransformEntry {
            oid: output_oid.clone(),
            size: 18,
            params: params.clone(),
        },
    );

    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 18,
        transforms,
    };
    cache.put(&record).expect("put");

    // Verify the record exists before removal.
    assert!(
        cache.get(&source_oid).is_some(),
        "record should exist after put"
    );

    // Remove the record.
    cache.remove(&source_oid).expect("remove");

    // Verify get() returns None after removal.
    assert!(
        cache.get(&source_oid).is_none(),
        "record should be gone after remove"
    );

    // Removing a non-existent record should be a no-op (no error).
    let fake_oid = "eeee".repeat(16);
    cache
        .remove(&fake_oid)
        .expect("removing non-existent record should succeed");
}

// -----------------------------------------------------------------------
// ObjectStore::store_bytes tests
// -----------------------------------------------------------------------

#[test]
fn test_store_bytes_roundtrip() {
    let dir = make_test_dir("store_bytes_rt");
    let store = ObjectStore::new(dir.join("objects"));

    let data = b"hello from store_bytes";
    let oid = store.store_bytes(data).expect("store_bytes");
    assert_eq!(oid.len(), 64, "OID should be 64 hex chars");

    // Blob should exist and match content.
    let blob_path = store.get_path(&oid).expect("blob should exist");
    let on_disk = fs::read(blob_path).expect("read blob");
    assert_eq!(on_disk, data);
}

#[test]
fn test_store_bytes_idempotent() {
    let dir = make_test_dir("store_bytes_idemp");
    let store = ObjectStore::new(dir.join("objects"));

    let data = b"same content twice";
    let oid1 = store.store_bytes(data).expect("first");
    let oid2 = store.store_bytes(data).expect("second");
    assert_eq!(oid1, oid2);
}

#[test]
fn test_store_bytes_json_blob() {
    let dir = make_test_dir("store_bytes_json");
    let store = ObjectStore::new(dir.join("objects"));

    let meta = CachedMediaMeta {
        dimensions: Some((1920, 1080)),
        dominant_color: Some("#ff5733".to_string()),
        lqip_data_uri: None,
        is_animated: false,
    };
    let json = serde_json::to_vec(&meta).expect("serialize");
    let oid = store.store_bytes(&json).expect("store");

    // Read back and deserialize.
    let blob = store.get_path(&oid).expect("blob exists");
    let raw = fs::read(blob).expect("read");
    let loaded: CachedMediaMeta = serde_json::from_slice(&raw).expect("deser");
    assert_eq!(loaded, meta);
}

// -----------------------------------------------------------------------
// HashIndex tests
// -----------------------------------------------------------------------

/// A full stat record, as `FileStat::of` builds one on unix: every field present.
fn stat(size: u64, mtime: u64) -> FileStat {
    FileStat { size, mtime, mtime_nanos: Some(250_000_000), ctime: Some(1_700_000_005), inode: Some(77) }
}

#[test]
fn test_hash_index_new_is_empty() {
    let idx = HashIndex::new();
    assert!(idx.entries.is_empty());
}

#[test]
fn test_hash_index_update_and_lookup_hit() {
    let mut idx = HashIndex::new();
    idx.update("photo.jpg".to_string(), &stat(1024, 1700000000), "abcd1234".to_string());

    // Same stat record → cache hit.
    assert_eq!(idx.lookup("photo.jpg", &stat(1024, 1700000000)), Some("abcd1234"));
}

#[test]
fn test_hash_index_lookup_miss_not_present() {
    let idx = HashIndex::new();
    assert!(idx.lookup("nonexistent.jpg", &stat(0, 0)).is_none());
}

/// Every field of the record is part of the key: a file that differs from what was
/// hashed in ANY of them — a same-size rewrite in the same second differs in the
/// sub-second mtime alone — must miss.
#[test]
fn a_lookup_misses_when_any_one_field_of_the_stat_record_differs() {
    let recorded = stat(1024, 1700000000);
    let mut idx = HashIndex::new();
    idx.update("photo.jpg".to_string(), &recorded, "abcd1234".to_string());
    assert_eq!(idx.lookup("photo.jpg", &recorded), Some("abcd1234"), "premise: the record itself hits");

    let variants = [
        ("size", FileStat { size: 1025, ..recorded }),
        ("mtime", FileStat { mtime: 1700000001, ..recorded }),
        ("sub-second mtime", FileStat { mtime_nanos: Some(250_000_001), ..recorded }),
        ("ctime", FileStat { ctime: Some(1_700_000_006), ..recorded }),
        ("inode", FileStat { inode: Some(78), ..recorded }),
    ];
    for (field, changed) in variants {
        assert!(
            idx.lookup("photo.jpg", &changed).is_none(),
            "a file whose {field} differs from the hashed one must not reuse its hash"
        );
    }
}

/// Missing precision fails OPEN: a fact the entry never recorded, or the file no
/// longer reports, is a miss — never a hit on what remains. ctime and inode are the
/// exception the crate already makes (`identity_disagrees`): a platform that has
/// neither, on either side, must not lose the index for good.
#[test]
fn a_missing_subsecond_mtime_never_hits_but_a_missing_ctime_or_inode_does() {
    let full = stat(1024, 1700000000);
    let mut idx = HashIndex::new();
    idx.update("photo.jpg".to_string(), &full, "abcd1234".to_string());

    // The file reports no sub-second mtime: nothing to compare, so nothing to trust.
    assert!(idx.lookup("photo.jpg", &FileStat { mtime_nanos: None, ..full }).is_none());

    // The entry has none (recorded by a caller that had only whole seconds): None on
    // both sides is not agreement.
    idx.update_whole_second("clip.mov".to_string(), 1024, 1700000000, "abcd1234".to_string());
    assert!(idx.lookup("clip.mov", &FileStat::whole_second(1024, 1700000000)).is_none());
    assert!(idx.lookup("clip.mov", &full).is_none());

    // ctime / inode: absence on either side is agreement.
    assert_eq!(idx.lookup("photo.jpg", &FileStat { ctime: None, inode: None, ..full }), Some("abcd1234"));
}

/// The video path's rule is unchanged: size + whole-second mtime, blind to the rest,
/// and what it records carries no sub-second field for `lookup` to trust.
#[test]
fn the_whole_second_pair_keeps_its_old_rule_and_its_old_serialized_form() {
    let mut idx = HashIndex::new();
    idx.update_whole_second("clip.mov".to_string(), 4096, 1700000000, "vvvv".to_string());

    assert_eq!(idx.lookup_whole_second("clip.mov", 4096, 1700000000), Some("vvvv"));
    assert!(idx.lookup_whole_second("clip.mov", 4097, 1700000000).is_none());
    assert!(idx.lookup_whole_second("clip.mov", 4096, 1700000001).is_none());
    // A full-stat entry is still visible to it.
    idx.update("pic.png".to_string(), &stat(10, 20), "pppp".to_string());
    assert_eq!(idx.lookup_whole_second("pic.png", 10, 20), Some("pppp"));

    // Nothing but the three original keys, so an index a video was recorded in reads
    // exactly as it did before the stat record existed.
    let json = serde_json::to_value(&idx.entries["clip.mov"]).unwrap();
    assert_eq!(json, serde_json::json!({ "size": 4096, "mtime": 1700000000, "content_hash": "vvvv" }));
}

#[test]
fn test_hash_index_save_load_roundtrip() {
    let dir = make_test_dir("hash_idx_rt");
    let path = dir.join("hash-index.json");

    let mut idx = HashIndex::new();
    idx.update("a.jpg".to_string(), &stat(100, 1000), "aaaa".to_string());
    idx.update("b.mp4".to_string(), &stat(200, 2000), "bbbb".to_string());
    idx.save(&path).expect("save");

    let loaded = HashIndex::load(&path);
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.lookup("a.jpg", &stat(100, 1000)), Some("aaaa"));
    assert_eq!(loaded.lookup("b.mp4", &stat(200, 2000)), Some("bbbb"));
}

/// An index the previous version wrote — entries of `size`, `mtime` and
/// `content_hash` only — loads without error through both loaders, its entries
/// miss the full-stat lookup, and hashing the file rewrites the entry in the new
/// format. No migration, no schema version.
#[test]
fn an_index_written_before_the_stat_record_existed_loads_misses_and_is_rewritten() {
    let dir = make_test_dir("hash_idx_old_format");
    let file = write_temp_file(&dir, "pic.png", b"the bytes as they are now");
    let now = FileStat::of(&fs::metadata(&file).unwrap());

    // The old format, with an entry that the OLD lookup would have trusted: same size,
    // same whole second — and a hash that is wrong for these bytes.
    let old_index = serde_json::json!({
        "entries": { "pic.png": { "size": now.size, "mtime": now.mtime, "content_hash": "stale" } }
    });
    let path = dir.join("hash-index.json");
    fs::write(&path, old_index.to_string()).unwrap();

    let loaded = HashIndex::load(&path);
    assert_eq!(loaded.entries.len(), 1, "an old-format index must load, not be discarded as corrupt");
    let strict = HashIndex::load_strict(&path).expect("the GC's strict loader must accept it too");
    assert_eq!(strict.map(|i| i.entries.len()), Some(1));
    assert!(loaded.lookup("pic.png", &now).is_none(), "an entry with no sub-second field must miss");

    let mut idx = loaded;
    let hash = idx.resolve(&file, "pic.png").unwrap();
    assert_ne!(hash, "stale", "the stale hash must not survive");
    assert_eq!(hash, ObjectStore::hash_file(&file).unwrap());
    assert_eq!(idx.lookup("pic.png", &now), Some(hash.as_str()), "the entry is rewritten in the new format");
}

/// An index written by THIS version still loads in the previous one: it ignores the
/// fields it does not know (asserted here as "the new fields are additive JSON keys").
#[test]
fn a_full_stat_entry_only_adds_keys_to_the_old_format() {
    let mut idx = HashIndex::new();
    idx.update("pic.png".to_string(), &stat(10, 20), "pppp".to_string());
    let json = serde_json::to_value(&idx.entries["pic.png"]).unwrap();
    let object = json.as_object().unwrap();
    for original in ["size", "mtime", "content_hash"] {
        assert!(object.contains_key(original), "the original key {original} must still be written");
    }
    assert_eq!(object.len(), 6);
}

#[test]
fn test_hash_index_load_missing_file_returns_empty() {
    let idx = HashIndex::load(Path::new("/nonexistent/hash-index.json"));
    assert!(idx.entries.is_empty());
}

#[test]
fn test_hash_index_load_corrupt_file_returns_empty() {
    let dir = make_test_dir("hash_idx_corrupt");
    let path = dir.join("hash-index.json");
    fs::write(&path, "not valid json{{{").expect("write corrupt");

    let idx = HashIndex::load(&path);
    assert!(idx.entries.is_empty());
}

#[test]
fn test_hash_index_stale_entries_pruned() {
    // Simulates the scan flow: only entries seen this scan survive.
    let mut old_idx = HashIndex::new();
    old_idx.update("kept.jpg".to_string(), &stat(100, 1000), "aaaa".to_string());
    old_idx.update("deleted.jpg".to_string(), &stat(200, 2000), "bbbb".to_string());

    // During a scan, we build a NEW index containing only seen files.
    let mut new_idx = HashIndex::new();
    // "kept.jpg" is seen again, carry over its entry.
    if old_idx.lookup("kept.jpg", &stat(100, 1000)).is_some() {
        new_idx.carry_forward(&old_idx, "kept.jpg");
    }
    // "deleted.jpg" is NOT seen, so it doesn't get carried over.

    assert_eq!(new_idx.entries.len(), 1);
    assert!(new_idx.lookup("kept.jpg", &stat(100, 1000)).is_some());
    assert!(new_idx.lookup("deleted.jpg", &stat(200, 2000)).is_none());
}

/// A carried-forward entry is the entry as recorded, not the hit re-stamped with the
/// file's current stat: a whole-second hit is not proof of anything finer, and
/// re-stamping it would launder it into an entry the full-stat lookup trusts.
#[test]
fn carrying_an_entry_forward_never_upgrades_it() {
    let mut old_idx = HashIndex::new();
    old_idx.update_whole_second("clip.mov".to_string(), 4096, 1700000000, "vvvv".to_string());

    let mut new_idx = HashIndex::new();
    new_idx.carry_forward(&old_idx, "clip.mov");
    new_idx.carry_forward(&old_idx, "never-recorded.mov");

    assert_eq!(new_idx.entries, old_idx.entries);
    assert!(new_idx.lookup("clip.mov", &stat(4096, 1700000000)).is_none());
}

/// `resolve` is the one place that turns a file into a hash through the index: a hit
/// is trusted without reading the file, and a rewrite that keeps the size and the
/// whole second — but not the instant inside it — is hashed, not trusted.
#[test]
fn resolve_hashes_a_same_size_rewrite_in_the_same_second_instead_of_trusting_the_index() {
    let dir = make_test_dir("hash_idx_resolve_rewrite");
    let file = write_temp_file(&dir, "pic.png", b"first version!");
    let first = ObjectStore::hash_file(&file).unwrap();

    let mut idx = HashIndex::new();
    assert_eq!(idx.resolve(&file, "pic.png").unwrap(), first);
    // Unchanged: the entry is trusted (a planted hash comes back, proving the file
    // was not read).
    idx.entries.get_mut("pic.png").unwrap().content_hash = "planted".to_string();
    assert_eq!(idx.resolve(&file, "pic.png").unwrap(), "planted");

    // Same size, different bytes, same wall-clock second, another instant in it.
    let before = fs::metadata(&file).unwrap().modified().unwrap();
    fs::write(&file, b"second version").unwrap();
    assert_eq!(fs::metadata(&file).unwrap().len(), 14, "premise: same size");
    FileStat::stamp_in_the_second_of(&file, before);

    let second = idx.resolve(&file, "pic.png").unwrap();
    assert_ne!(second, "planted", "the rewrite must not be answered from the index");
    assert_eq!(second, ObjectStore::hash_file(&file).unwrap());
}

/// Replace-via-rename — the atomic-save pattern — with size and mtime carried over
/// exactly: only the inode says the file is a different one.
#[cfg(unix)]
#[test]
fn resolve_hashes_a_file_replaced_by_rename_even_when_size_and_mtime_survive() {
    let dir = make_test_dir("hash_idx_resolve_inode");
    let file = write_temp_file(&dir, "pic.png", b"first version!");
    let at = fs::metadata(&file).unwrap().modified().unwrap();

    let mut idx = HashIndex::new();
    idx.resolve(&file, "pic.png").unwrap();
    idx.entries.get_mut("pic.png").unwrap().content_hash = "planted".to_string();

    let replacement = write_temp_file(&dir, "pic.png.new", b"second version");
    fs::File::options().write(true).open(&replacement).unwrap().set_modified(at).unwrap();
    fs::rename(&replacement, &file).unwrap();
    let after = fs::metadata(&file).unwrap();
    assert_eq!((after.len(), after.modified().unwrap()), (14, at), "premise: size and mtime survive the replacement");

    assert_ne!(idx.resolve(&file, "pic.png").unwrap(), "planted", "a different inode must not reuse the hash");
}

/// A file still in the cloud is not read to learn its hash: reading a dehydrated
/// file blocks for the provider's download or fails, and a provider re-materialising
/// one changes its ctime and inode, so the strict lookup misses on exactly the files
/// the old size-and-second key answered without a read.
#[test]
fn resolve_with_never_reads_a_cloud_only_file_the_index_does_not_vouch_for() {
    let dir = make_test_dir("hash_idx_resolve_cloud_miss");
    let file = write_temp_file(&dir, "pic.png", b"in the cloud");
    let here = FileStat::of(&fs::metadata(&file).unwrap());

    // Never recorded, and recorded before the provider changed ctime and inode.
    let mut rematerialised = HashIndex::new();
    rematerialised.update("pic.png".to_string(), &FileStat { ctime: Some(1), inode: Some(1), ..here }, "old".to_string());
    for (what, mut idx) in [("never recorded", HashIndex::new()), ("ctime and inode moved", rematerialised)] {
        let mut reads = 0;
        let before = idx.entries.clone();
        let outcome = idx.resolve_with(&file, "pic.png", |_| { reads += 1; Ok("read".to_string()) }, |_| true);
        assert!(outcome.is_err(), "{what}: answered {outcome:?} for a file in the cloud");
        assert_eq!(reads, 0, "{what}: read a file that is in the cloud");
        assert_eq!(idx.entries, before, "{what}: recorded something for a hash it did not get");
    }
}

/// A hit costs no read, so it answers whether or not the file is in the cloud: the
/// guard is for the read, not for the file.
#[test]
fn resolve_with_still_answers_a_cloud_only_file_from_a_hit() {
    let dir = make_test_dir("hash_idx_resolve_cloud_hit");
    let file = write_temp_file(&dir, "pic.png", b"in the cloud");
    let mut idx = HashIndex::new();
    idx.update("pic.png".to_string(), &FileStat::of(&fs::metadata(&file).unwrap()), "recorded".to_string());

    let outcome = idx.resolve_with(&file, "pic.png", |_| panic!("read on a hit"), |_| true);

    assert_eq!(outcome.as_deref(), Ok("recorded"));
}

/// The other side of the guard: a file on disk is hashed and recorded on a miss.
#[test]
fn resolve_with_hashes_and_records_a_local_file_the_index_does_not_vouch_for() {
    let dir = make_test_dir("hash_idx_resolve_local_miss");
    let file = write_temp_file(&dir, "pic.png", b"on disk");
    let mut idx = HashIndex::new();

    let outcome = idx.resolve_with(&file, "pic.png", |_| Ok("hashed".to_string()), |_| false);

    assert_eq!(outcome.as_deref(), Ok("hashed"));
    assert_eq!(idx.lookup("pic.png", &FileStat::of(&fs::metadata(&file).unwrap())), Some("hashed"));
}

/// `resolve` is what every caller holds, and it must be wired to the real cloud
/// check: the seam above proves the guard, this proves nothing swaps it out.
#[test]
fn resolve_asks_whether_the_file_is_in_the_cloud() {
    let dir = make_test_dir("hash_idx_resolve_cloud_wired");
    let file = write_temp_file(&dir, "pic.png", b"in the cloud");
    let mut idx = HashIndex::new();

    let cloud = crate::build::icloud::pretend::evicted(&file);
    assert!(idx.resolve(&file, "pic.png").is_err(), "resolve read a file that is in the cloud");
    assert!(idx.entries.is_empty());

    drop(cloud);
    assert_eq!(idx.resolve(&file, "pic.png").unwrap(), ObjectStore::hash_file(&file).unwrap());
}

/// Simulate the iCloud eviction scenario: parent directory disappears
/// between create_dir_all() and rename(). The save() method should
/// retry after re-creating the parent.
#[test]
fn test_hash_index_save_retries_on_parent_dir_eviction() {
    let dir = make_test_dir("hash_idx_eviction");
    let cache_dir = dir.join("cache");
    let path = cache_dir.join("hash-index.json");

    let mut idx = HashIndex::new();
    idx.update("a.jpg".to_string(), &stat(100, 1000), "aaaa".to_string());

    // First save should succeed and create the parent directory.
    idx.save(&path).expect("first save");
    assert!(path.exists());

    // Simulate iCloud eviction: remove the parent directory entirely.
    fs::remove_dir_all(&cache_dir).expect("remove cache dir");
    assert!(!cache_dir.exists(), "cache dir should be gone");

    // Second save should succeed via the retry path: save() calls
    // create_dir_all, writes the tmp file, rename fails with NotFound
    // (parent gone), retry re-creates parent, rename succeeds.
    idx.update("b.jpg".to_string(), &stat(200, 2000), "bbbb".to_string());
    idx.save(&path).expect("save after eviction should succeed");

    // Verify the data round-trips correctly.
    let loaded = HashIndex::load(&path);
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.lookup("a.jpg", &stat(100, 1000)), Some("aaaa"));
    assert_eq!(loaded.lookup("b.jpg", &stat(200, 2000)), Some("bbbb"));
}

// -----------------------------------------------------------------------
// Metadata cache via TransformCache tests (media/meta transform)
// -----------------------------------------------------------------------

#[test]
fn test_metadata_cache_hit_returns_stored_meta() {
    let dir = make_test_dir("meta_cache_hit");
    let store = ObjectStore::new(dir.join("objects"));

    // Store a metadata JSON blob.
    let meta = CachedMediaMeta {
        dimensions: Some((1280, 720)),
        dominant_color: Some("#00ff00".to_string()),
        lqip_data_uri: None,
        is_animated: false,
    };
    let json_bytes = serde_json::to_vec(&meta).expect("ser");
    let meta_oid = store.store_bytes(&json_bytes).expect("store blob");

    // Create a fake source OID.
    let source_src = write_temp_file(&dir, "source.bin", b"video file content");
    let source_oid = ObjectStore::hash_file(&source_src).expect("hash");

    // Write a TransformRecord with "media/meta" transform.
    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );
    let params = serde_json::json!({});
    let mut transforms = HashMap::new();
    transforms.insert(
        "media/meta".to_string(),
        TransformEntry {
            oid: meta_oid.clone(),
            size: json_bytes.len() as u64,
            params: params.clone(),
        },
    );
    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 18,
        transforms,
    };
    cache.put(&record).expect("put");

    // Look up → should hit.
    let found_oid = cache.find_cached_output(&source_oid, "media/meta", &params);
    assert_eq!(found_oid, Some(meta_oid.clone()));

    // Read the blob and deserialize.
    let blob_path = ObjectStore::new(dir.join("objects"))
        .get_path(&meta_oid)
        .expect("blob exists");
    let raw = fs::read(blob_path).expect("read");
    let loaded: CachedMediaMeta = serde_json::from_slice(&raw).expect("deser");
    assert_eq!(loaded, meta);
}

#[test]
fn test_metadata_cache_miss_triggers_extraction() {
    let dir = make_test_dir("meta_cache_miss");
    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );

    let fake_oid = "ffff".repeat(16);
    let params = serde_json::json!({});

    // No record exists → cache miss.
    let result = cache.find_cached_output(&fake_oid, "media/meta", &params);
    assert!(result.is_none(), "should miss when no record");
}

#[test]
fn test_metadata_cache_full_roundtrip() {
    // Simulate the full flow: hash file → check meta cache → miss →
    // extract → store blob → write record → check again → hit.
    let dir = make_test_dir("meta_full_rt");
    let store = ObjectStore::new(dir.join("objects"));

    // 1. Hash the source file.
    let src = write_temp_file(&dir, "video.mp4", b"fake mp4 content");
    let source_oid = ObjectStore::hash_file(&src).expect("hash");
    let source_size = fs::metadata(&src).expect("stat").len();

    // 2. Check TransformCache → miss.
    let cache = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );
    let params = serde_json::json!({});
    assert!(cache
        .find_cached_output(&source_oid, "media/meta", &params)
        .is_none());

    // 3. "Extract" metadata (simulate ffprobe + ffmpeg result).
    let meta = CachedMediaMeta {
        dimensions: Some((1920, 1080)),
        dominant_color: Some("#336699".to_string()),
        lqip_data_uri: None,
        is_animated: false,
    };

    // 4. Store metadata as blob in ObjectStore.
    let json_bytes = serde_json::to_vec(&meta).expect("ser");
    let meta_oid = store.store_bytes(&json_bytes).expect("store blob");

    // 5. Write TransformRecord.
    let mut transforms = HashMap::new();
    transforms.insert(
        "media/meta".to_string(),
        TransformEntry {
            oid: meta_oid.clone(),
            size: json_bytes.len() as u64,
            params: params.clone(),
        },
    );
    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size,
        transforms,
    };
    cache.put(&record).expect("put");

    // 6. Check TransformCache again → hit!
    let cache2 = TransformCache::new(
        dir.join("transforms"),
        ObjectStore::new(dir.join("objects")),
    );
    let found = cache2.find_cached_output(&source_oid, "media/meta", &params);
    assert_eq!(found, Some(meta_oid));
}

// -----------------------------------------------------------------------
// Singleflight tests
// -----------------------------------------------------------------------

#[test]
fn test_singleflight_basic() {
    let sf = Singleflight::<String>::new();
    let (result, shared) = sf.do_work("key1", || "hello".to_string());
    assert_eq!(result, Some("hello".to_string()));
    assert!(!shared, "First caller should not be shared");
}

#[test]
fn test_singleflight_sequential_same_key() {
    let sf = Singleflight::<i32>::new();

    let (r1, s1) = sf.do_work("key", || 42);
    assert_eq!(r1, Some(42));
    assert!(!s1);

    // Second call with same key should execute again (first is done)
    let (r2, s2) = sf.do_work("key", || 99);
    assert_eq!(r2, Some(99));
    assert!(!s2, "Sequential call should execute independently");
}

#[test]
fn test_singleflight_different_keys_independent() {
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::sync::Arc;
    use std::thread;

    let sf = Arc::new(Singleflight::<String>::new());
    let call_count = Arc::new(AtomicU32::new(0));

    let sf2 = sf.clone();
    let cc2 = call_count.clone();
    let t1 = thread::spawn(move || {
        sf2.do_work("key_a", || {
            cc2.fetch_add(1, AtomicOrdering::SeqCst);
            "a".to_string()
        })
    });

    let sf3 = sf.clone();
    let cc3 = call_count.clone();
    let t2 = thread::spawn(move || {
        sf3.do_work("key_b", || {
            cc3.fetch_add(1, AtomicOrdering::SeqCst);
            "b".to_string()
        })
    });

    let (r1, _) = t1.join().unwrap();
    let (r2, _) = t2.join().unwrap();

    assert_eq!(r1, Some("a".to_string()));
    assert_eq!(r2, Some("b".to_string()));
    assert_eq!(
        call_count.load(AtomicOrdering::SeqCst),
        2,
        "Different keys should both execute"
    );
}

#[test]
fn test_singleflight_concurrent_same_key() {
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    let sf = Arc::new(Singleflight::<String>::new());
    let call_count = Arc::new(AtomicU32::new(0));

    let barrier = Arc::new(std::sync::Barrier::new(2));

    let sf1 = sf.clone();
    let cc1 = call_count.clone();
    let b1 = barrier.clone();
    let t1 = thread::spawn(move || {
        sf1.do_work("same_key", || {
            cc1.fetch_add(1, AtomicOrdering::SeqCst);
            b1.wait(); // Ensure t2 has time to call do_work too
            thread::sleep(Duration::from_millis(50));
            "result".to_string()
        })
    });

    // Give t1 time to register as in-flight
    thread::sleep(Duration::from_millis(10));

    let sf2 = sf.clone();
    let cc2 = call_count.clone();
    let t2 = thread::spawn(move || {
        barrier.wait(); // Wait until t1 is executing
        sf2.do_work("same_key", || {
            cc2.fetch_add(1, AtomicOrdering::SeqCst);
            "should_not_run".to_string()
        })
    });

    let (r1, s1) = t1.join().unwrap();
    let (r2, s2) = t2.join().unwrap();

    assert_eq!(r1, Some("result".to_string()));
    assert_eq!(
        r2,
        Some("result".to_string()),
        "Waiter should get same result"
    );
    assert!(!s1, "First caller should not be shared");
    assert!(s2, "Second caller should be shared");
    assert_eq!(
        call_count.load(AtomicOrdering::SeqCst),
        1,
        "Work should only execute once"
    );
}

#[test]
fn test_singleflight_panic_safety() {
    let sf = Singleflight::<String>::new();

    // Work function panics
    let (result, shared) = sf.do_work("panic_key", || {
        panic!("intentional panic");
    });

    assert!(result.is_none(), "Panic should return None");
    assert!(!shared);

    // Key should be removed, so a new call should work
    let (result2, _) = sf.do_work("panic_key", || "recovered".to_string());
    assert_eq!(result2, Some("recovered".to_string()));
}

#[test]
fn test_singleflight_clear() {
    let sf = Singleflight::<()>::new();
    // No-op on empty
    sf.clear();
}

#[test]
fn test_singleflight_default() {
    let sf: Singleflight<i32> = Singleflight::default();
    let (r, _) = sf.do_work("key", || 42);
    assert_eq!(r, Some(42));
}

// =========================================================================
// Singleflight Design Invariant Tests (ADR-010, Phase 4)
// =========================================================================

/// INVARIANT: After completion, key is cleaned up from in-flight map.
/// A subsequent call with the same key must execute fresh work, not share stale result.
/// Guards: No permanent key occupation in singleflight.
#[test]
fn test_singleflight_key_not_permanently_occupied() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let sf = Singleflight::<i32>::new();
    let call_count = AtomicU32::new(0);

    let (r1, _) = sf.do_work("key", || {
        call_count.fetch_add(1, Ordering::SeqCst);
        42
    });
    assert_eq!(r1, Some(42));

    let (r2, shared) = sf.do_work("key", || {
        call_count.fetch_add(1, Ordering::SeqCst);
        99
    });
    assert_eq!(
        r2,
        Some(99),
        "New call must get new result, not stale cached"
    );
    assert!(!shared, "Sequential call must not be marked as shared");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "INVARIANT: Work must execute twice for sequential calls"
    );
}

/// INVARIANT: Many concurrent waiters all receive the same result.
/// Work function executes exactly once.
/// Guards: Singleflight dedup with N waiters.
#[test]
fn test_singleflight_many_waiters_all_receive_result() {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier,
    };
    use std::thread;
    use std::time::Duration;

    let sf = Arc::new(Singleflight::<String>::new());
    let call_count = Arc::new(AtomicU32::new(0));
    let num_waiters = 10;
    let barrier = Arc::new(Barrier::new(num_waiters));

    let mut handles = vec![];
    for _ in 0..num_waiters {
        let sf = sf.clone();
        let cc = call_count.clone();
        let b = barrier.clone();
        handles.push(thread::spawn(move || {
            b.wait(); // All threads start simultaneously
            sf.do_work("shared_key", || {
                cc.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(50));
                "shared_result".to_string()
            })
        }));
    }

    let mut shared_count = 0;
    for h in handles {
        let (result, shared) = h.join().unwrap();
        assert_eq!(
            result,
            Some("shared_result".to_string()),
            "INVARIANT: All waiters must receive the same result"
        );
        if shared {
            shared_count += 1;
        }
    }

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "INVARIANT: Work function must execute exactly once"
    );
    assert!(
        shared_count >= 1,
        "At least one caller should be marked as shared (got {})",
        shared_count
    );
}

/// INVARIANT: Different keys execute independently and concurrently.
/// Guards: Keys don't interfere with each other.
#[test]
fn test_singleflight_different_keys_run_in_parallel() {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Barrier,
    };
    use std::thread;

    let sf = Arc::new(Singleflight::<String>::new());
    let barrier = Arc::new(Barrier::new(2));
    let concurrent_count = Arc::new(AtomicU32::new(0));

    let sf1 = sf.clone();
    let b1 = barrier.clone();
    let cc1 = concurrent_count.clone();
    let t1 = thread::spawn(move || {
        sf1.do_work("key_x", || {
            cc1.fetch_add(1, Ordering::SeqCst);
            b1.wait(); // Both should reach this point concurrently
            "x".to_string()
        })
    });

    let sf2 = sf.clone();
    let cc2 = concurrent_count.clone();
    let t2 = thread::spawn(move || {
        sf2.do_work("key_y", || {
            cc2.fetch_add(1, Ordering::SeqCst);
            barrier.wait(); // Both should reach this point concurrently
            "y".to_string()
        })
    });

    let (rx, _) = t1.join().unwrap();
    let (ry, _) = t2.join().unwrap();
    assert_eq!(rx, Some("x".to_string()));
    assert_eq!(ry, Some("y".to_string()));
    assert_eq!(
        concurrent_count.load(Ordering::SeqCst),
        2,
        "INVARIANT: Both keys must execute their work functions independently"
    );
}

// -----------------------------------------------------------------------
// Singleflight try_start/complete/abort API tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_try_start_returns_true_for_new_key() {
    let sf = Singleflight::<String>::new();
    let (is_first, _rx) = sf.try_start("new_key");
    assert!(is_first, "First caller should get is_first=true");
    // Clean up: complete the flight so the key is removed
    sf.complete("new_key", "done".to_string());
}

#[tokio::test]
async fn test_try_start_returns_false_for_inflight_key() {
    let sf = Singleflight::<String>::new();
    let (is_first, _rx1) = sf.try_start("key");
    assert!(is_first);

    // Second caller for the same key while first is still in-flight
    let (is_first2, _rx2) = sf.try_start("key");
    assert!(!is_first2, "Second caller should get is_first=false");

    // Clean up
    sf.complete("key", "value".to_string());
}

#[tokio::test]
async fn test_complete_wakes_waiters_with_value() {
    use std::sync::Arc;

    let sf = Arc::new(Singleflight::<String>::new());

    // First caller starts the flight
    let (is_first, _rx1) = sf.try_start("key");
    assert!(is_first);

    // Second caller gets a receiver to wait on
    let (is_first2, mut rx2) = sf.try_start("key");
    assert!(!is_first2);

    // Complete the flight from a separate task
    let sf2 = sf.clone();
    tokio::spawn(async move {
        sf2.complete("key", "hello".to_string());
    });

    // Waiter should receive the value
    rx2.changed()
        .await
        .expect("watch channel should not be closed");
    let value = rx2.borrow().clone();
    assert_eq!(value, Some("hello".to_string()));
}

#[tokio::test]
async fn test_abort_wakes_waiters_with_none() {
    use std::sync::Arc;

    let sf = Arc::new(Singleflight::<String>::new());

    // First caller starts the flight
    let (is_first, _rx1) = sf.try_start("key");
    assert!(is_first);

    // Second caller gets a receiver
    let (is_first2, mut rx2) = sf.try_start("key");
    assert!(!is_first2);

    // Abort the flight
    let sf2 = sf.clone();
    tokio::spawn(async move {
        sf2.abort("key");
    });

    // Waiter should receive None
    rx2.changed()
        .await
        .expect("watch channel should not be closed");
    let value = rx2.borrow().clone();
    assert!(value.is_none(), "Abort should send None to waiters");
}

#[tokio::test]
async fn test_independent_keys() {
    let sf = Singleflight::<String>::new();

    // Start two independent keys
    let (is_first_a, _rx_a) = sf.try_start("key_a");
    let (is_first_b, _rx_b) = sf.try_start("key_b");

    assert!(is_first_a, "key_a should be first");
    assert!(is_first_b, "key_b should be first (independent key)");

    // Complete them independently
    sf.complete("key_a", "value_a".to_string());
    sf.complete("key_b", "value_b".to_string());

    // Both keys should be cleaned up
}

/// SingleflightGuard aborts on drop when armed, cleaning up the key.
/// This prevents key leaks when a future is cancelled by tokio::time::timeout.
#[tokio::test]
async fn test_singleflight_guard_aborts_on_drop() {
    let sf = Singleflight::<String>::new();

    // Simulate: first caller starts a flight, guard aborts on drop
    {
        let (is_first, _rx) = sf.try_start("guarded_key");
        assert!(is_first);

        // Guard will abort on drop since we never call disarm()
        let _guard = SingleflightGuard::new(&sf, "guarded_key".to_string());
    }

    // After the guard drops, the key should be cleaned up.
    // A new caller should be is_first=true.
    let (is_first, _rx) = sf.try_start("guarded_key");
    assert!(
        is_first,
        "Key should be cleaned up after guard drop — \
             new caller should be is_first=true"
    );

    // Clean up
    sf.complete("guarded_key", "recovered".to_string());
}

/// SingleflightGuard does NOT abort when disarmed before drop.
#[tokio::test]
async fn test_singleflight_guard_disarmed_does_not_abort() {
    let sf = Singleflight::<String>::new();

    let (is_first, _rx) = sf.try_start("disarmed_key");
    assert!(is_first);

    {
        let mut guard = SingleflightGuard::new(&sf, "disarmed_key".to_string());
        guard.disarm();
        // Guard drops here but is disarmed — should NOT abort
    }

    // Key should still be in-flight (guard didn't abort it)
    let (is_first, _rx) = sf.try_start("disarmed_key");
    assert!(
        !is_first,
        "Disarmed guard should not abort — key should still be in-flight"
    );

    // Clean up
    sf.complete("disarmed_key", "done".to_string());
}

#[test]
fn test_do_work_still_works() {
    // Existing do_work pattern must continue to function
    let sf = Singleflight::<String>::new();
    let (result, shared) = sf.do_work("key", || "hello".to_string());
    assert_eq!(result, Some("hello".to_string()));
    assert!(!shared);

    // Sequential call should work independently
    let (result2, shared2) = sf.do_work("key", || "world".to_string());
    assert_eq!(result2, Some("world".to_string()));
    assert!(!shared2);
}

#[test]
fn test_do_work_panic_safety() {
    let sf = Singleflight::<String>::new();

    // Closure panics — should return None and clean up key
    let (result, _) = sf.do_work("panic_key", || {
        panic!("intentional test panic");
    });
    assert!(result.is_none(), "Panic should return None");

    // Key should be reusable after panic
    let (result2, _) = sf.do_work("panic_key", || "recovered".to_string());
    assert_eq!(result2, Some("recovered".to_string()));
}

// -----------------------------------------------------------------------
// Singleflight<MediaMetadata> tests (metadata singleflight feature)
// -----------------------------------------------------------------------

/// Verify Singleflight<MediaMetadata> do_work returns correct result.
#[test]
fn test_singleflight_media_metadata_basic() {
    use crate::types::content::MediaMetadata;

    let sf = Singleflight::<MediaMetadata>::new();
    let (result, shared) = sf.do_work("meta:abc123", || MediaMetadata {
        is_animated: false,
        path: "images/photo.jpg".to_string(),
        file_type: "jpg".to_string(),
        size: 1024,
        modified: Some("1700000000".to_string()),
        dimensions: Some((1920, 1080)),
        dominant_color: Some("#ff5733".to_string()),
        lqip_data_uri: None,
    });

    assert!(!shared, "First caller should not be shared");
    let meta = result.expect("should return Some");
    assert_eq!(meta.path, "images/photo.jpg");
    assert_eq!(meta.dimensions, Some((1920, 1080)));
    assert_eq!(meta.dominant_color, Some("#ff5733".to_string()));
}

/// Verify concurrent dedup: 4 threads call do_work with the same key,
/// work executes exactly once, all threads get Some(result).
#[test]
fn test_singleflight_media_metadata_concurrent_dedup() {
    use crate::types::content::MediaMetadata;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    let sf = Arc::new(Singleflight::<MediaMetadata>::new());
    let call_count = Arc::new(AtomicU32::new(0));
    let num_threads = 4;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];
    for _ in 0..num_threads {
        let sf = sf.clone();
        let cc = call_count.clone();
        let b = barrier.clone();
        handles.push(thread::spawn(move || {
            b.wait(); // All threads start simultaneously
            sf.do_work("meta:same_hash", || {
                cc.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(50));
                MediaMetadata {
                    is_animated: false,
                    path: "images/shared.jpg".to_string(),
                    file_type: "jpg".to_string(),
                    size: 2048,
                    modified: None,
                    dimensions: Some((800, 600)),
                    dominant_color: Some("#00ff00".to_string()),
                    lqip_data_uri: None,
                }
            })
        }));
    }

    let mut shared_count = 0;
    for h in handles {
        let (result, shared) = h.join().unwrap();
        assert!(result.is_some(), "All threads must receive Some(result)");
        let meta = result.unwrap();
        assert_eq!(meta.path, "images/shared.jpg");
        assert_eq!(meta.dimensions, Some((800, 600)));
        if shared {
            shared_count += 1;
        }
    }

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Work function must execute exactly once"
    );
    assert!(
        shared_count >= 1,
        "At least some callers should be marked as shared (got {})",
        shared_count
    );
}

// -----------------------------------------------------------------------
// Singleflight error-propagation invariant tests (ADR-010, Phase 4)
//
// These tests lock the invariant fixed by the singleflight error-channel
// refactor: when the primary closure returns an outcome with an error, all
// concurrent waiters receive the SAME outcome with the SAME error — not a
// sentinel with no error.
//
// Singleflight<V> is generic, so these tests use a minimal local struct
// that mirrors the `error: Option<String>` convention used by both
// VideoConversionOutcome and ImageConversionOutcome.
// -----------------------------------------------------------------------

/// Minimal stand-in for media conversion outcomes — mirrors the
/// `error: Option<String>` convention used by both VideoConversionOutcome
/// and ImageConversionOutcome.
#[derive(Clone, Debug)]
struct FakeConversionOutcome {
    output_id: Option<String>,
    error: Option<String>,
}

#[test]
fn singleflight_waiter_observes_error_from_primary() {
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    let sf: Arc<Singleflight<FakeConversionOutcome>> = Arc::new(Singleflight::new());
    // Barrier of 2: primary waits for waiter to enter do_work before returning.
    let barrier = Arc::new(Barrier::new(2));

    // Primary: registers as in-flight, waits for waiter, then returns an
    // outcome with an error.
    let sf_a = sf.clone();
    let barrier_a = barrier.clone();
    let handle_a = thread::spawn(move || {
        sf_a.do_work("test-key-error", || {
            barrier_a.wait(); // Block until waiter is in the in-flight path
            thread::sleep(Duration::from_millis(50)); // Ensure waiter reaches condvar wait
            FakeConversionOutcome {
                output_id: None,
                error: Some("primary_failed_encode".to_string()),
            }
        })
    });

    // Waiter: enters do_work with same key after primary has registered.
    let sf_b = sf.clone();
    let barrier_b = barrier.clone();
    let handle_b = thread::spawn(move || {
        barrier_b.wait(); // Unblock primary; primary now sleeps 50ms
                          // Tiny sleep so we are definitely second (primary is already in the
                          // map by the time our do_work acquires the lock).
        thread::sleep(Duration::from_millis(5));
        sf_b.do_work("test-key-error", || {
            panic!("waiter closure must not execute — should share primary's result");
        })
    });

    let (outcome_a, shared_a) = handle_a.join().unwrap();
    let (outcome_b, shared_b) = handle_b.join().unwrap();

    // Primary did the work.
    assert!(
        !shared_a,
        "primary should have executed the closure (shared=false)"
    );
    let a = outcome_a.expect("primary must receive Some(outcome)");
    assert_eq!(
        a.error.as_deref(),
        Some("primary_failed_encode"),
        "primary outcome must carry the error"
    );

    // Waiter shared the result and received the same error — not a sentinel.
    assert!(
        shared_b,
        "waiter should have shared primary's result (shared=true)"
    );
    let b = outcome_b.expect("waiter must receive Some(outcome), not None sentinel");
    assert_eq!(
        b.error.as_deref(),
        Some("primary_failed_encode"),
        "waiter must receive the same error as primary, not an empty sentinel"
    );
}

#[test]
fn singleflight_waiter_observes_success_from_primary() {
    // Mirror of the error test: a successful outcome is also propagated
    // intact to the waiter. Completes the invariant coverage.
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    let sf: Arc<Singleflight<FakeConversionOutcome>> = Arc::new(Singleflight::new());
    let barrier = Arc::new(Barrier::new(2));

    let sf_a = sf.clone();
    let barrier_a = barrier.clone();
    let handle_a = thread::spawn(move || {
        sf_a.do_work("test-key-success", || {
            barrier_a.wait();
            thread::sleep(Duration::from_millis(50));
            FakeConversionOutcome {
                output_id: Some("oid_abc123".to_string()),
                error: None,
            }
        })
    });

    let sf_b = sf.clone();
    let barrier_b = barrier.clone();
    let handle_b = thread::spawn(move || {
        barrier_b.wait();
        thread::sleep(Duration::from_millis(5));
        sf_b.do_work("test-key-success", || {
            panic!("waiter closure must not execute");
        })
    });

    let (outcome_a, shared_a) = handle_a.join().unwrap();
    let (outcome_b, shared_b) = handle_b.join().unwrap();

    assert!(!shared_a, "primary should not be shared");
    let a = outcome_a.expect("primary must get Some(outcome)");
    assert!(a.error.is_none(), "primary success has no error");
    assert_eq!(a.output_id.as_deref(), Some("oid_abc123"));

    assert!(shared_b, "waiter should be shared");
    let b = outcome_b.expect("waiter must get Some(outcome)");
    assert!(
        b.error.is_none(),
        "waiter receives success outcome with no error"
    );
    assert_eq!(b.output_id.as_deref(), Some("oid_abc123"));
}

// -----------------------------------------------------------------------
// CachedMediaMeta LQIP field tests
// -----------------------------------------------------------------------

#[test]
fn test_cached_media_meta_backward_compat_no_lqip() {
    // JSON from before the lqip_data_uri field existed must still deserialize.
    let json = r##"{"dimensions":[800,600],"dominant_color":"#ff5733"}"##;
    let meta: CachedMediaMeta = serde_json::from_str(json).expect("deser");
    assert_eq!(meta.dimensions, Some((800, 600)));
    assert_eq!(meta.dominant_color.as_deref(), Some("#ff5733"));
    assert_eq!(meta.lqip_data_uri, None);
}

#[test]
fn test_cached_media_meta_backward_compat_no_is_animated() {
    // JSON from before the is_animated field existed must still deserialize,
    // defaulting to false (a bounded, self-healing gap — see moss#919: the
    // next content change re-sniffs and writes the real value).
    let json = r##"{"dimensions":[800,600],"dominant_color":"#ff5733"}"##;
    let meta: CachedMediaMeta = serde_json::from_str(json).expect("deser");
    assert_eq!(meta.is_animated, false);
}

#[test]
fn test_cached_media_meta_roundtrip_with_is_animated() {
    let meta = CachedMediaMeta {
        dimensions: Some((480, 270)),
        dominant_color: None,
        lqip_data_uri: None,
        is_animated: true,
    };
    let json = serde_json::to_string(&meta).expect("ser");
    let loaded: CachedMediaMeta = serde_json::from_str(&json).expect("deser");
    assert_eq!(loaded, meta);
    assert!(loaded.is_animated);
}

#[test]
fn test_cached_media_meta_roundtrip_with_lqip() {
    let meta = CachedMediaMeta {
        dimensions: Some((1920, 1080)),
        dominant_color: Some("#336699".to_string()),
        lqip_data_uri: Some("data:image/jpeg;base64,/9j/4AAQ".to_string()),
        is_animated: false,
    };
    let json = serde_json::to_string(&meta).expect("ser");
    let loaded: CachedMediaMeta = serde_json::from_str(&json).expect("deser");
    assert_eq!(loaded, meta);
    assert_eq!(
        loaded.lqip_data_uri.as_deref(),
        Some("data:image/jpeg;base64,/9j/4AAQ")
    );
}

#[test]
fn test_cached_media_meta_none_lqip_omitted_in_json() {
    // When lqip_data_uri is None, it should not appear in the serialized JSON
    // (skip_serializing_if = "Option::is_none").
    let meta = CachedMediaMeta {
        dimensions: Some((640, 480)),
        dominant_color: None,
        lqip_data_uri: None,
        is_animated: false,
    };
    let json = serde_json::to_string(&meta).expect("ser");
    assert!(
        !json.contains("lqip_data_uri"),
        "None field must be omitted"
    );
}

// -----------------------------------------------------------------------
// Garbage Collection tests
// -----------------------------------------------------------------------

/// Helper: set up a mock build directory with cache structure.
/// Returns (build_dir, objects_dir, transforms_dir).
fn make_gc_test_dir(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = make_test_dir(name);
    let build_dir = dir.join("build");
    let objects_dir = build_dir.join("cache").join("objects");
    let transforms_dir = build_dir.join("cache").join("transforms");
    fs::create_dir_all(&objects_dir).expect("create objects dir");
    fs::create_dir_all(&transforms_dir).expect("create transforms dir");
    (build_dir, objects_dir, transforms_dir)
}

/// Helper: write a blob to the object store at the correct sharded path.
fn put_object(objects_dir: &Path, oid: &str, content: &[u8]) {
    let p1 = &oid[..2];
    let p2 = &oid[2..4];
    let dir = objects_dir.join(p1).join(p2);
    fs::create_dir_all(&dir).expect("create shard dir");
    fs::write(dir.join(oid), content).expect("write blob");
}

/// Helper: write a transform record at the correct sharded path.
fn put_transform(transforms_dir: &Path, record: &TransformRecord) {
    let p1 = &record.source_oid[..2];
    let p2 = &record.source_oid[2..4];
    let dir = transforms_dir.join(p1).join(p2);
    fs::create_dir_all(&dir).expect("create shard dir");
    let path = dir.join(format!("{}.json", &record.source_oid));
    let json = serde_json::to_string_pretty(record).expect("serialize");
    fs::write(path, json).expect("write transform");
}

#[test]
fn test_gc_empty_cache() {
    // GC on an empty cache should succeed with zero removals.
    let (build_dir, _, _) = make_gc_test_dir("gc_empty");
    let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test()).expect("every mark input is readable");
    assert_eq!(result.transforms_removed, 0);
    assert_eq!(result.objects_removed, 0);
    assert_eq!(result.bytes_freed, 0);
}

#[test]
fn test_gc_nonexistent_dirs() {
    // GC on a build dir with no cache subdirs should not panic.
    let dir = make_test_dir("gc_nonexistent");
    let build_dir = dir.join("build");
    fs::create_dir_all(&build_dir).expect("create build dir");
    let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test()).expect("every mark input is readable");
    assert_eq!(result.transforms_removed, 0);
    assert_eq!(result.objects_removed, 0);
    assert_eq!(result.bytes_freed, 0);
}

#[test]
fn test_gc_removes_orphaned_transform() {
    let (build_dir, objects_dir, transforms_dir) = make_gc_test_dir("gc_orphan_transform");

    // Source OID that IS in HashIndex (live)
    let live_oid = "aaaa".repeat(16);
    // Source OID that is NOT in HashIndex (orphaned)
    let orphan_oid = "bbbb".repeat(16);

    // Output OIDs
    let live_output = "cccc".repeat(16);
    let orphan_output = "dddd".repeat(16);

    // Write hash-index.json with only the live source
    let hash_index = HashIndex {
        entries: {
            let mut m = HashMap::new();
            m.insert(
                "source.md".to_string(),
                HashIndexEntry {
                    size: 100,
                    mtime: 1000,
                    mtime_nanos: None,
                    ctime: None,
                    inode: None,
                    content_hash: live_oid.clone(),
                },
            );
            m
        },
    };
    let hash_index_path = build_dir.join("cache").join("hash-index.json");
    hash_index.save(&hash_index_path).expect("save hash index");

    // Create transform records
    let live_record = TransformRecord {
        source_oid: live_oid.clone(),
        source_size: 100,
        transforms: {
            let mut m = HashMap::new();
            m.insert(
                "thumbnail".to_string(),
                TransformEntry {
                    oid: live_output.clone(),
                    size: 50,
                    params: serde_json::json!({}),
                },
            );
            m
        },
    };
    let orphan_record = TransformRecord {
        source_oid: orphan_oid.clone(),
        source_size: 200,
        transforms: {
            let mut m = HashMap::new();
            m.insert(
                "thumbnail".to_string(),
                TransformEntry {
                    oid: orphan_output.clone(),
                    size: 75,
                    params: serde_json::json!({}),
                },
            );
            m
        },
    };

    put_transform(&transforms_dir, &live_record);
    put_transform(&transforms_dir, &orphan_record);

    // Put objects for all OIDs
    put_object(&objects_dir, &live_oid, b"live source");
    put_object(&objects_dir, &live_output, b"live output");
    put_object(&objects_dir, &orphan_oid, b"orphan source");
    put_object(&objects_dir, &orphan_output, b"orphan output");

    let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test()).expect("every mark input is readable");

    // Should have removed 1 transform record (the orphaned one)
    assert_eq!(
        result.transforms_removed, 1,
        "should remove 1 orphaned transform"
    );

    // Should have removed 2 objects: orphan_oid + orphan_output
    assert_eq!(
        result.objects_removed, 2,
        "should remove 2 orphaned objects"
    );
    assert!(result.bytes_freed > 0, "should have freed some bytes");

    // Verify live objects still exist
    let store = ObjectStore::new(objects_dir.clone());
    assert!(
        store.blob_path(&live_oid).exists(),
        "live source should still exist"
    );
    assert!(
        store.blob_path(&live_output).exists(),
        "live output should still exist"
    );

    // Verify orphaned objects are gone
    assert!(
        !store.blob_path(&orphan_oid).exists(),
        "orphan source should be removed"
    );
    assert!(
        !store.blob_path(&orphan_output).exists(),
        "orphan output should be removed"
    );

    // Verify live transform record still exists
    let live_transform_path = transforms_dir
        .join(&live_oid[..2])
        .join(&live_oid[2..4])
        .join(format!("{}.json", &live_oid));
    assert!(
        live_transform_path.exists(),
        "live transform should still exist"
    );

    // Verify orphaned transform record is gone
    let orphan_transform_path = transforms_dir
        .join(&orphan_oid[..2])
        .join(&orphan_oid[2..4])
        .join(format!("{}.json", &orphan_oid));
    assert!(
        !orphan_transform_path.exists(),
        "orphan transform should be removed"
    );
}

#[test]
fn test_gc_preserves_objects_referenced_by_site_hashes() {
    let (build_dir, objects_dir, _) = make_gc_test_dir("gc_site_hashes");

    // Object referenced by hashes.json but NOT by any transform or hash index
    let site_hash_oid = "eeee".repeat(16);
    // Unreferenced object
    let orphan_oid = "ffff".repeat(16);

    put_object(&objects_dir, &site_hash_oid, b"site content hash");
    put_object(&objects_dir, &orphan_oid, b"unreferenced blob");

    // Write hashes.json with the site hash
    let hashes_json = serde_json::json!({
        "files": {
            "index.html": site_hash_oid,
        },
        "sources": {},
        "video_outputs": [],
    });
    fs::write(
        build_dir.join("hashes.json"),
        serde_json::to_string_pretty(&hashes_json).unwrap(),
    )
    .expect("write hashes.json");

    let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test()).expect("every mark input is readable");

    // Should remove only the orphan, not the site-hashes-referenced blob
    assert_eq!(result.objects_removed, 1, "should remove 1 orphaned object");

    let store = ObjectStore::new(objects_dir);
    assert!(
        store.blob_path(&site_hash_oid).exists(),
        "site-hash-referenced object should survive"
    );
    assert!(
        !store.blob_path(&orphan_oid).exists(),
        "orphan should be removed"
    );
}

#[test]
fn test_gc_preserves_objects_referenced_by_transforms() {
    let (build_dir, objects_dir, transforms_dir) = make_gc_test_dir("gc_transform_refs");

    let source_oid = "1111".repeat(16);
    let output_oid = "2222".repeat(16);
    let orphan_oid = "3333".repeat(16);

    // Make source live via hash index
    let hash_index = HashIndex {
        entries: {
            let mut m = HashMap::new();
            m.insert(
                "file.jpg".to_string(),
                HashIndexEntry {
                    size: 500,
                    mtime: 2000,
                    mtime_nanos: None,
                    ctime: None,
                    inode: None,
                    content_hash: source_oid.clone(),
                },
            );
            m
        },
    };
    let hash_index_path = build_dir.join("cache").join("hash-index.json");
    hash_index.save(&hash_index_path).expect("save");

    // Create transform that references output_oid
    let record = TransformRecord {
        source_oid: source_oid.clone(),
        source_size: 500,
        transforms: {
            let mut m = HashMap::new();
            m.insert(
                "webp".to_string(),
                TransformEntry {
                    oid: output_oid.clone(),
                    size: 300,
                    params: serde_json::json!({"quality": 80}),
                },
            );
            m
        },
    };
    put_transform(&transforms_dir, &record);

    // Put all objects
    put_object(&objects_dir, &source_oid, b"source image");
    put_object(&objects_dir, &output_oid, b"converted webp");
    put_object(&objects_dir, &orphan_oid, b"orphaned blob");

    let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test()).expect("every mark input is readable");

    assert_eq!(
        result.transforms_removed, 0,
        "no transforms should be removed"
    );
    assert_eq!(result.objects_removed, 1, "should remove 1 orphaned object");

    let store = ObjectStore::new(objects_dir);
    assert!(
        store.blob_path(&source_oid).exists(),
        "source should survive"
    );
    assert!(
        store.blob_path(&output_oid).exists(),
        "transform output should survive"
    );
    assert!(
        !store.blob_path(&orphan_oid).exists(),
        "orphan should be removed"
    );
}

/// A GC mark input that exists but cannot be read marks less, and marking less
/// deletes more. Three inputs, one rig: the sweep must abort with nothing
/// deleted when any of them is unreadable (moss 404c: an unreadable build tree
/// is an ordinary input on a cloud-managed vault, not a corner case).
#[cfg(unix)]
#[test]
fn gc_deletes_nothing_when_a_mark_input_is_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    let source_oid = "5555".repeat(16);
    let output_oid = "6666".repeat(16);
    let site_oid = "7777".repeat(16);
    let orphan_oid = "8888".repeat(16);

    let rig = |name: &str| {
        let (build_dir, objects_dir, transforms_dir) = make_gc_test_dir(name);
        let mut entries = HashMap::new();
        entries.insert(
            "file.jpg".to_string(),
            HashIndexEntry { size: 500, mtime: 2000, mtime_nanos: None, ctime: None, inode: None, content_hash: source_oid.clone() },
        );
        HashIndex { entries }
            .save(&build_dir.join("cache").join("hash-index.json"))
            .expect("save index");
        let mut transforms = HashMap::new();
        transforms.insert(
            "webp".to_string(),
            TransformEntry { oid: output_oid.clone(), size: 3, params: serde_json::json!({}) },
        );
        put_transform(
            &transforms_dir,
            &TransformRecord { source_oid: source_oid.clone(), source_size: 500, transforms },
        );
        fs::write(
            build_dir.join("hashes.json"),
            format!(r#"{{"files": {{"page/index.html": "{site_oid}"}}}}"#),
        )
        .unwrap();
        for oid in [&source_oid, &output_oid, &site_oid, &orphan_oid] {
            put_object(&objects_dir, oid, oid.as_bytes());
        }
        (build_dir, objects_dir, transforms_dir)
    };
    let record_of = |transforms_dir: &Path| {
        transforms_dir.join(&source_oid[..2]).join(&source_oid[2..4]).join(format!("{source_oid}.json"))
    };

    let cases: [(&str, fn(&Path, &Path) -> PathBuf); 3] = [
        ("gc_unreadable_index", |build, _| build.join("cache").join("hash-index.json")),
        ("gc_unreadable_record", |_, record| record.to_path_buf()),
        ("gc_unreadable_hashes", |build, _| build.join("hashes.json")),
    ];
    for (name, locked_input) in cases {
        let (build_dir, objects_dir, transforms_dir) = rig(name);
        let record = record_of(&transforms_dir);
        let locked = locked_input(&build_dir, &record);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&locked).is_ok() {
            eprintln!("skipped: this process can read a 0o000 file (running as root?)");
            return;
        }

        let result = gc(&build_dir, &crate::build::lifecycle::gc_token_for_test());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

        let store = ObjectStore::new(objects_dir.clone());
        for oid in [&source_oid, &output_oid, &site_oid, &orphan_oid] {
            assert!(
                store.blob_path(oid).exists(),
                "{name}: blob {oid} was deleted by a sweep that could not read {}",
                locked.display()
            );
        }
        assert!(record.exists(), "{name}: the live transform record was deleted");
        assert!(result.is_err(), "{name}: an unreadable mark input must abort the sweep, got {result:?}");
    }
}
