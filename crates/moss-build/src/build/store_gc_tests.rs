//! Tests for build-store retention and GC policy.

use super::*;
use std::collections::HashSet;

fn roots_of(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// Create `names` as generation dirs under a temp root, oldest first, with
/// distinct mtimes so the newest-first sort is deterministic. None has a
/// write-lock file, so each gets ordinary retention.
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

/// Where `current.generation` sits relative to `gens`, as in a real build dir.
fn current_marker(gens: &std::path::Path) -> std::path::PathBuf {
    gens.with_file_name("current.generation")
}

fn gc(gens: &std::path::Path, roots: &HashSet<String>, n: usize) -> std::io::Result<()> {
    gc_old_generations(gens, &current_marker(gens), roots, n)
}

/// The generation directories left under `gens`, skipping lock files.
fn surviving(gens: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(gens)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// Retention policy
// ---------------------------------------------------------------------------

/// A generation directory with its write-lock file, as `ship` leaves it
/// while copying.
fn start_writing(gens: &std::path::Path, name: &str) -> GenerationWriteLock {
    let lock = GenerationWriteLock::acquire(gens, name).unwrap();
    let dir = gens.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), b"partial").unwrap();
    lock
}

/// A copy in progress holds its write lock. GC must not remove that
/// directory, even when retention would (`n = 0`): another process could be
/// copying into it, and removing it would leave that copy to finish into a
/// hollow tree and promote it. The test's handle stands in for that process.
#[test]
fn a_generation_whose_write_lock_is_held_is_kept_even_outside_retention() {
    let (_tmp, gens) = make_generations(&["current"]);
    let _held = start_writing(&gens, "in-flight");

    gc(&gens, &gc_roots("current", &HashSet::new(), None), 0).unwrap();

    assert!(surviving(&gens).contains(&"in-flight".to_string()), "a held write lock must keep its generation");
}

/// Once nobody holds the lock but its file remains, the copy failed or was
/// cut off. GC removes that directory even inside the retention window,
/// while a finished generation (no lock file), `current`, and a pinned one
/// all stay.
#[test]
fn an_abandoned_generation_is_removed_once_its_write_lock_is_free() {
    let (_tmp, gens) = make_generations(&["complete", "current"]);
    drop(start_writing(&gens, "abandoned"));
    drop(start_writing(&gens, "pinned"));

    gc(&gens, &gc_roots("current", &roots_of(&["pinned"]), None), 5).unwrap();

    assert_eq!(
        surviving(&gens),
        vec!["complete".to_string(), "current".to_string(), "pinned".to_string()],
        "only the abandoned, unpinned generation may go"
    );
    assert!(!gens.join(".abandoned.writing").exists(), "its lock file goes with it");
}

/// A lock file GC cannot open or lock (unreadable here; on some network
/// filesystems locking itself fails) must keep the generation: GC cannot
/// tell whether a copy is running.
#[cfg(unix)]
#[test]
fn a_write_lock_that_cannot_be_checked_keeps_its_generation() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, gens) = make_generations(&["current"]);
    drop(start_writing(&gens, "unknown"));
    let lock_file = gens.join(".unknown.writing");
    std::fs::set_permissions(&lock_file, std::fs::Permissions::from_mode(0o000)).unwrap();

    gc(&gens, &gc_roots("current", &HashSet::new(), None), 0).unwrap();

    std::fs::set_permissions(&lock_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(surviving(&gens).contains(&"unknown".to_string()), "an uncheckable lock must keep its generation");
}

/// GC removes a directory only while holding its write lock, so a writer
/// that re-derives the same id (and would copy into the existing directory)
/// waits for the removal instead of copying into a tree being deleted, then
/// retries onto a fresh lock file rather than holding the unlinked one.
#[test]
fn a_writer_waits_for_gc_removal_and_then_locks_a_fresh_lock_file() {
    let (_tmp, gens) = make_generations(&["old", "current"]);
    let writer = std::rc::Rc::new(std::cell::RefCell::new(None));
    let (hook_gens, hook_writer) = (gens.clone(), writer.clone());
    BEFORE_GENERATION_REMOVAL.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |_path: &std::path::Path| {
            let (tx, rx) = std::sync::mpsc::channel();
            let gens = hook_gens.clone();
            *hook_writer.borrow_mut() = Some(std::thread::spawn(move || {
                let lock = GenerationWriteLock::acquire(&gens, "old").unwrap();
                let _ = tx.send(());
                lock
            }));
            assert!(
                rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(),
                "a writer must not get the lock while GC is removing the directory"
            );
        }));
    });

    gc(&gens, &gc_roots("current", &HashSet::new(), None), 1).unwrap();
    BEFORE_GENERATION_REMOVAL.with(|hook| *hook.borrow_mut() = None);

    assert!(!surviving(&gens).contains(&"old".to_string()), "sanity: GC removed the old generation");
    let writer = writer.borrow_mut().take().expect("GC must have reached the removal");
    let _writer_lock = writer.join().unwrap();
    assert!(
        matches!(GenerationWriteLock::take(&gens, "old", false), Ok(Take::Busy)),
        "the writer must hold the lock file now at the path, not the one GC unlinked"
    );
}

/// `roots` holds this process's own view of `current`. Another process may
/// have promoted a generation since, so GC re-reads the on-disk marker under
/// the candidate's lock and keeps what it names.
#[test]
fn a_generation_promoted_by_another_process_is_kept() {
    let (_tmp, gens) = make_generations(&["theirs", "mine"]);
    std::fs::write(current_marker(&gens), b"theirs\n").unwrap();

    gc(&gens, &gc_roots("mine", &HashSet::new(), None), 0).unwrap();

    assert!(surviving(&gens).contains(&"theirs".to_string()), "the on-disk current generation must survive");
}

fn inject_lock_errors(errors: Vec<std::io::Error>) {
    INJECTED_LOCK_ERRORS.with(|q| *q.borrow_mut() = errors.into());
}

/// A signal interrupting the blocking lock must not let a copy run
/// unlocked: the writer retries and really holds the lock afterwards.
#[test]
fn an_interrupted_lock_is_retried_rather_than_skipped() {
    let (_tmp, gens) = make_generations(&[]);
    inject_lock_errors(vec![std::io::Error::from(std::io::ErrorKind::Interrupted)]);

    let _lock = GenerationWriteLock::acquire(&gens, "g").expect("an interrupted lock must be retried");

    inject_lock_errors(vec![]);
    assert!(
        matches!(GenerationWriteLock::take(&gens, "g", false), Ok(Take::Busy)),
        "after the retry the writer must actually hold the lock"
    );
}

/// A lock failure on a filesystem that can otherwise lock fails the copy
/// before it starts, since a GC might still lock and remove the directory.
/// Only a filesystem with no locks at all lets the copy proceed.
#[test]
fn a_lock_error_fails_the_copy_unless_the_filesystem_cannot_lock() {
    let (_tmp, gens) = make_generations(&[]);
    inject_lock_errors(vec![std::io::Error::other("transient")]);
    assert!(GenerationWriteLock::acquire(&gens, "g").is_err(), "an unexpected lock error must fail the copy");

    inject_lock_errors(vec![std::io::Error::from(std::io::ErrorKind::Unsupported)]);
    let proceeded = GenerationWriteLock::acquire(&gens, "g");
    inject_lock_errors(vec![]);
    assert!(proceeded.is_ok(), "a filesystem without locks must still let the copy run");
    drop(proceeded);

    // What a macOS network share returns; std gives it no specific kind.
    #[cfg(unix)]
    {
        inject_lock_errors(vec![std::io::Error::from_raw_os_error(libc::ENOTSUP)]);
        let proceeded = GenerationWriteLock::acquire(&gens, "g");
        inject_lock_errors(vec![]);
        assert!(proceeded.is_ok(), "ENOTSUP must count as a filesystem without locks");
    }
}

#[test]
fn keeps_the_n_newest_generations() {
    let (_tmp, gens) = make_generations(&["a", "b", "c", "d", "e"]);
    gc(&gens, &roots_of(&["e"]), 2).unwrap();
    // Newest two are d and e; a/b/c are unreferenced and go.
    assert_eq!(surviving(&gens), vec!["d", "e"]);
}

/// A1 — the regression that made this a correctness fix rather than a cleanup.
///
/// `email/commands.rs`'s publish-before-send gate opens
/// `last_deployed_generation_id` to check math PNGs. Before this fix that
/// generation was not a GC root, so once it aged out the gate failed closed
/// with "Publish the site first" on a site that HAD been published.
#[test]
fn last_deployed_generation_survives_even_when_ancient() {
    let (_tmp, gens) = make_generations(&["deployed", "b", "c", "d", "current"]);
    let roots = gc_roots("current", &HashSet::new(), Some("deployed"));
    gc(&gens, &roots, 2).unwrap();
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
    gc(&gens, &roots, 2).unwrap();
    assert!(surviving(&gens).contains(&"uploading".to_string()));
}

#[test]
fn current_survives_even_if_its_mtime_is_oldest() {
    let (_tmp, gens) = make_generations(&["current", "b", "c", "d", "e"]);
    let roots = gc_roots("current", &HashSet::new(), None);
    gc(&gens, &roots, 2).unwrap();
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
    assert!(transforms.get_with(&source, crate::build::cache::RecordMode::Request).is_some(), "and so does its transform record");

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

// ---------------------------------------------------------------------------
// Reusing a generation already on disk
// ---------------------------------------------------------------------------

/// Windows ships a symlink entry's target bytes, but the entry hashes only
/// the target path, so an edit inside a linked folder keeps the id. A
/// directory holding such an entry is never reused where targets are copied.
#[test]
fn a_symlink_entry_is_never_reused_where_ship_copies_its_target() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("linked"), b"target bytes").unwrap();
    let files: std::collections::HashMap<String, String> =
        [("linked".to_string(), crate::types::content::symlink_entry("../elsewhere"))].into();

    assert!(dir_holds(tmp.path(), &files, false), "sanity: the entry is present");
    assert!(!dir_holds(tmp.path(), &files, true));
}
