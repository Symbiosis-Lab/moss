//! Tests for build-store retention and GC policy (moss#976 Part A).

use super::*;
use std::collections::HashSet;

fn roots_of(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Create `names` as generation dirs under a temp root, oldest first, with
/// distinct mtimes so the newest-first sort is deterministic.
fn make_generations(names: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let gens = tmp.path().join("generations");
    std::fs::create_dir_all(&gens).unwrap();
    for (i, name) in names.iter().enumerate() {
        let dir = gens.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), b"x").unwrap();
        // Oldest first: index 0 gets the earliest mtime.
        let t = filetime::FileTime::from_unix_time(1_700_000_000 + i as i64 * 60, 0);
        filetime::set_file_mtime(&dir, t).unwrap();
    }
    (tmp, gens)
}

fn surviving(gens: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(gens)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// Retention policy
// ---------------------------------------------------------------------------

#[test]
fn keeps_the_n_newest_generations() {
    let (_tmp, gens) = make_generations(&["a", "b", "c", "d", "e"]);
    gc_old_generations(&gens, &roots_of(&["e"]), 2).unwrap();
    // Newest two are d and e; a/b/c are unreferenced and go.
    assert_eq!(surviving(&gens), vec!["d", "e"]);
}

/// A1 — the regression that made this a correctness fix rather than a cleanup.
///
/// `email/commands.rs`'s publish-before-send gate opens
/// `last_deployed_generation_id` to check math PNGs. Before moss#976 that
/// generation was not a GC root, so once it aged out the gate failed closed
/// with "Publish the site first" on a site that HAD been published.
#[test]
fn last_deployed_generation_survives_even_when_ancient() {
    let (_tmp, gens) = make_generations(&["deployed", "b", "c", "d", "current"]);
    let roots = gc_roots("current", &HashSet::new(), Some("deployed"));
    gc_old_generations(&gens, &roots, 2).unwrap();
    let left = surviving(&gens);
    assert!(
        left.contains(&"deployed".to_string()),
        "the last-deployed generation is a GC root regardless of age; got {left:?}"
    );
    assert!(left.contains(&"current".to_string()));
}

#[test]
fn deploy_pinned_generation_survives() {
    let (_tmp, gens) = make_generations(&["uploading", "b", "c", "d", "current"]);
    let roots = gc_roots("current", &roots_of(&["uploading"]), None);
    gc_old_generations(&gens, &roots, 2).unwrap();
    assert!(surviving(&gens).contains(&"uploading".to_string()));
}

#[test]
fn current_survives_even_if_its_mtime_is_oldest() {
    let (_tmp, gens) = make_generations(&["current", "b", "c", "d", "e"]);
    let roots = gc_roots("current", &HashSet::new(), None);
    gc_old_generations(&gens, &roots, 2).unwrap();
    assert!(surviving(&gens).contains(&"current".to_string()));
}

#[test]
fn retention_never_drops_below_the_floor() {
    // A COW filesystem is required for the >floor branch to be reachable, so
    // assert the clamp on the input side only — 0 and 1 both round up.
    let tmp = tempfile::tempdir().unwrap();
    for requested in [Some(0usize), Some(1), Some(2)] {
        assert_eq!(
            effective_keep_generations(requested, tmp.path()),
            KEEP_GENERATIONS_FLOOR,
            "keep_generations = {requested:?} must clamp up to the floor"
        );
    }
}

#[test]
fn unset_retention_uses_the_default_or_the_floor_without_cow() {
    let tmp = tempfile::tempdir().unwrap();
    let n = effective_keep_generations(None, tmp.path());
    if supports_cow(tmp.path()) {
        assert_eq!(n, KEEP_GENERATIONS_DEFAULT);
    } else {
        // A5: no reflink means every retained generation costs full price.
        assert_eq!(n, KEEP_GENERATIONS_FLOOR);
    }
}

#[test]
fn floor_is_two_because_preview_serves_the_previous_generation() {
    // Pinned as a constant assertion: `build/pipeline.rs` serves the previous
    // generation while `staging/` is rewritten, so 1 and 0 are not options.
    assert_eq!(KEEP_GENERATIONS_FLOOR, 2);
    assert!(KEEP_GENERATIONS_DEFAULT >= KEEP_GENERATIONS_FLOOR);
}

// ---------------------------------------------------------------------------
// Cache GC trigger — the settling property is the thing worth pinning
// ---------------------------------------------------------------------------

#[test]
fn tiny_object_stores_are_never_swept() {
    assert!(!should_gc_cache(0, None));
    assert!(!should_gc_cache(CACHE_GC_MIN_OBJECTS - 1, None));
    assert!(should_gc_cache(CACHE_GC_MIN_OBJECTS, None));
}

#[test]
fn first_sweep_fires_once_the_store_is_big_enough() {
    // No watermark = never swept. The measured real site sat at 10,143.
    assert!(should_gc_cache(10_143, None));
}

#[test]
fn sweep_does_not_refire_immediately_after_collecting() {
    // 10,143 objects collapse to the reachable set; the next build must NOT
    // re-sweep, or every build pays a full object-store walk.
    let after = 923usize;
    assert!(!should_gc_cache(after, Some(after)));
    assert!(!should_gc_cache(after + CACHE_GC_GROWTH_SLACK, Some(after)));
}

#[test]
fn sweep_refires_once_the_store_has_roughly_doubled() {
    let after = 923usize;
    let trigger = after * CACHE_GC_GROWTH_FACTOR + CACHE_GC_GROWTH_SLACK;
    assert!(!should_gc_cache(trigger, Some(after)));
    assert!(should_gc_cache(trigger + 1, Some(after)));
}

#[test]
fn watermark_round_trips_through_disk() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(load_watermark(tmp.path()), None);
    save_watermark(tmp.path(), 4_242);
    assert_eq!(load_watermark(tmp.path()), Some(4_242));
}

#[test]
fn corrupt_watermark_reads_as_never_swept() {
    let tmp = tempfile::tempdir().unwrap();
    let p = watermark_path(tmp.path());
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"{ not json").unwrap();
    assert_eq!(
        load_watermark(tmp.path()),
        None,
        "an unreadable watermark must degrade to one extra sweep, not a panic"
    );
}

/// A writer stores blobs and transform records before the hash index that
/// marks them live is saved, so a cache sweep beside it deletes what it just
/// wrote. While any build of the folder holds its cache lease the sweep does
/// not run — it is skipped, not waited for — and once none does, it runs.
#[test]
fn the_object_store_is_swept_only_when_no_build_holds_a_cache_lease() {
    use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};

    let tmp = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(tmp.path());
    let _record = crate::build::lifecycle::lock_for(&mp);
    let cache = mp.store_dir();
    // Big enough to be worth sweeping.
    for i in 0..CACHE_GC_MIN_OBJECTS {
        std::fs::create_dir_all(cache.join("objects").join("zz").join(format!("{i:04}"))).unwrap();
    }
    let objects = ObjectStore::new(cache.join("objects"));
    let transforms = TransformCache::new(cache.join("transforms"), ObjectStore::new(cache.join("objects")));
    let blob = objects.store_bytes(b"just encoded").unwrap();
    let orphan = objects.store_bytes(b"nothing names this").unwrap();
    // Its source is not in hash-index.json yet: the writer saves the index last.
    let source = "5".repeat(64);
    transforms
        .put(&TransformRecord {
            source_oid: source.clone(),
            source_size: 1,
            transforms: [("video/mp4".to_string(), TransformEntry { oid: blob.clone(), size: 12, params: serde_json::json!({}) })]
                .into_iter()
                .collect(),
        })
        .unwrap();

    let lease = crate::build::lifecycle::cache_write_lease(&mp);
    assert!(maybe_gc_cache(&mp).is_none(), "no sweep while a build holds its lease");
    assert!(objects.blob_path(&blob).exists(), "the blob the writer just stored survives");
    assert!(transforms.get(&source).is_some(), "and so does its transform record");

    drop(lease);
    assert!(maybe_gc_cache(&mp).is_some(), "with no lease open the sweep runs");
    assert!(!objects.blob_path(&orphan).exists(), "and collects what nothing names");
    // The record is days from its TTL, so the shared-cache rule keeps its blob
    // even now: the lease guards the window between storing a blob and
    // writing the record that names it, which no age can cover.
    assert!(objects.blob_path(&blob).exists(), "a just-written record keeps what it names");
}

#[test]
fn counting_an_absent_object_store_is_zero_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(count_objects(tmp.path()), 0);
    assert!(maybe_gc_cache(&crate::moss_paths::MossPaths::new(tmp.path())).is_none());
}

#[test]
fn count_objects_walks_the_fanout() {
    let tmp = tempfile::tempdir().unwrap();
    let objects = tmp.path().join("cache").join("objects");
    for fanout in ["ab", "cd"] {
        let d = objects.join(fanout);
        std::fs::create_dir_all(&d).unwrap();
        for i in 0..3 {
            std::fs::write(d.join(format!("blob{i}")), b"x").unwrap();
        }
    }
    assert_eq!(count_objects(&objects), 6);
}

// ---------------------------------------------------------------------------
// COW probe
// ---------------------------------------------------------------------------

#[test]
fn cow_probe_answers_and_leaves_no_litter() {
    let tmp = tempfile::tempdir().unwrap();
    let _ = supports_cow(tmp.path());
    let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|n| n.starts_with(".moss-cow-probe"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "the probe must clean up after itself; found {leftovers:?}"
    );
}
