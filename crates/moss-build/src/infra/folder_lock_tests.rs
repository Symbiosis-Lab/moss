use super::*;

/// Points `MOSS_HOME` at a fresh temp dir for the duration of the closure,
/// restoring whatever was there before. Takes `infra::home::MOSS_HOME_TEST_LOCK`
/// for the whole span — env vars are process-global, and `infra::home`'s own
/// `moss_home_honours_the_env_var_override` mutates the same var, so both
/// must serialize on the one lock rather than each taking their own.
fn with_moss_home<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    let _guard = crate::infra::home::MOSS_HOME_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let prior = std::env::var_os("MOSS_HOME");
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("MOSS_HOME", tmp.path());
    let result = f(tmp.path());
    match prior {
        Some(v) => std::env::set_var("MOSS_HOME", v),
        None => std::env::remove_var("MOSS_HOME"),
    }
    result
}

/// Two spellings of the same folder — its canonical absolute form and a
/// relative path through `..` — must name the same lock file. This is the
/// whole reason [`folder_id`] canonicalizes before hashing rather than
/// hashing whatever string a caller happened to pass.
#[test]
fn the_lock_path_is_derived_from_the_canonical_folder_path() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();
        let canonical = std::fs::canonicalize(&folder).unwrap();

        let via_canonical = lock_path_for(&canonical).unwrap();
        let via_dotdot = lock_path_for(&folder.join("..").join("site")).unwrap();

        assert_eq!(
            via_canonical, via_dotdot,
            "two spellings of one folder must hash to the same lock path"
        );
    });
}

/// A different folder must never collide with the first — otherwise the
/// lock would serialize builds of unrelated sites.
#[test]
fn different_folders_get_different_lock_paths() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        assert_ne!(lock_path_for(&a).unwrap(), lock_path_for(&b).unwrap());
    });
}

/// The lock path lands under `$MOSS_HOME/locks/`, honoring the override the
/// same way `stacks_root`/`get_moss_assets_dir`/`get_moss_bin_dir` do.
#[test]
fn the_lock_path_lives_under_moss_home_locks() {
    with_moss_home(|home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        let path = lock_path_for(&folder).unwrap();
        assert_eq!(path.parent().unwrap(), home.join("locks"));
        assert!(path.extension().is_some_and(|e| e == "build"));
    });
}

/// A second, uncontended acquisition of a DIFFERENT folder must never block
/// on the first — the lock is per folder, not global.
#[test]
fn acquiring_two_different_folders_never_blocks() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let held_a = acquire(&a).unwrap();
        let held_b = acquire(&b).unwrap();
        drop(held_a);
        drop(held_b);
    });
}

/// Dropping a held lock releases it — a second acquisition of the SAME
/// folder must succeed once the first is gone, never hang.
#[test]
fn dropping_the_guard_releases_the_lock() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        let held = acquire(&folder).unwrap();
        drop(held);

        // Would hang forever if the drop above had not released the flock.
        let held_again = acquire(&folder).unwrap();
        drop(held_again);
    });
}

/// A folder with no recorded promotion reads as epoch 0 — the same "never
/// promoted" answer `last_promoted_epoch` gives for a genuinely fresh folder
/// as for one whose record file cannot be read.
#[test]
fn last_promoted_epoch_is_zero_before_anything_is_recorded() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        assert_eq!(last_promoted_epoch(&folder), 0);
    });
}

/// A corrupt `.epoch` file reads as 0 too, never as a parse error that
/// panics or poisons the caller's read-compare-write — unreadable is never
/// a promise about what promoted.
#[test]
fn a_corrupt_epoch_file_reads_as_zero() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        let path = epoch_path_for(&folder).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not-a-number").unwrap();

        assert_eq!(last_promoted_epoch(&folder), 0);
    });
}

/// The round trip [`record_promoted_epoch`] exists for: what is written is
/// what the next read (from a DIFFERENT in-memory call, i.e. a different
/// process in practice) sees.
#[test]
fn record_promoted_epoch_round_trips() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();

        record_promoted_epoch(&folder, 42).unwrap();
        assert_eq!(last_promoted_epoch(&folder), 42);

        // A later, larger epoch overwrites — the only direction a real
        // promotion ever moves this value.
        record_promoted_epoch(&folder, 100).unwrap();
        assert_eq!(last_promoted_epoch(&folder), 100);
    });
}
