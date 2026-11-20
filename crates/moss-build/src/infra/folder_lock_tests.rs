use super::*;

use crate::infra::home::with_moss_home;

/// Two spellings of the same folder — its canonical absolute form and a
/// relative path through `..` — must name the same lock file. This is the
/// whole reason [`name_for`](crate::infra::folder_store::name_for) canonicalizes before hashing rather than
/// hashing whatever string a caller happened to pass.
#[test]
fn the_lock_path_is_derived_from_the_canonical_folder_path() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("site");
        std::fs::create_dir_all(&folder).unwrap();
        let canonical = std::fs::canonicalize(&folder).unwrap();

        let via_canonical = lock_path_named(&canonical, "build").unwrap();
        let via_dotdot = lock_path_named(&folder.join("..").join("site"), "build").unwrap();

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

        assert_ne!(lock_path_named(&a, "build").unwrap(), lock_path_named(&b, "build").unwrap());
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

        let path = lock_path_named(&folder, "build").unwrap();
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

        let path = epoch_path_for(&folder);
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

/// A released lock leaves no file behind: names are per folder, and test runs
/// alone build thousands of temporary folders a day.
#[cfg(unix)]
#[test]
fn a_released_lock_leaves_no_file() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let path = lock_path_named(tmp.path(), "build").unwrap();

        let held = acquire(tmp.path()).unwrap();
        assert!(path.exists());
        drop(held);
        assert!(!path.exists(), "the released lock file is still at {path:?}");
    });
}

/// A waiter blocked on a file its holder then removes must not come away
/// holding that removed file, where a later acquirer cannot see it.
#[cfg(unix)]
#[test]
fn a_waiter_on_a_removed_lock_file_holds_the_one_others_see() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().to_path_buf();
        let path = lock_path_named(&folder, "build").unwrap();

        let first = acquire(&folder).unwrap();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            acquire_named_noting_wait(&folder, "build", || waiting_tx.send(()).unwrap()).unwrap()
        });
        waiting_rx.recv().unwrap();
        drop(first);
        let second = waiter.join().unwrap();

        let third = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        assert!(
            matches!(third.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "a third acquirer took the lock while the waiter held it"
        );
        drop(second);
    });
}

/// A waiter keeps going however many times the file is replaced under it:
/// each replacement is another holder finishing, so it is never a reason to
/// give up. Here the file is replaced 40 times while one acquirer waits.
#[cfg(unix)]
#[test]
fn a_waiter_outlasts_any_number_of_replaced_lock_files() {
    with_moss_home(|_home| {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().to_path_buf();
        let path = lock_path_named(&folder, "moderation").unwrap();
        let open = |path: &Path| open_lock_file(path).unwrap();

        let mut held = open(&path);
        held.try_lock().unwrap();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || acquire_named_noting_wait(&folder, "moderation", || waiting_tx.send(()).unwrap()));
        waiting_rx.recv().unwrap();
        for _ in 0..40 {
            // Another holder takes a fresh file before the old one is released,
            // so the waiter wakes on a file that is no longer the lock.
            std::thread::sleep(std::time::Duration::from_millis(20));
            std::fs::remove_file(&path).unwrap();
            let next = open(&path);
            next.try_lock().unwrap();
            drop(std::mem::replace(&mut held, next));
        }
        std::fs::remove_file(&path).unwrap();
        drop(held);

        let acquired = waiter.join().unwrap();
        assert!(acquired.is_ok(), "the waiter gave up: {acquired:?}");
    });
}

/// The promotion epoch lives in the folder's build output, so a folder that
/// is gone leaves nothing under `~/.moss`.
#[test]
fn the_promotion_epoch_lives_in_the_folder() {
    with_moss_home(|home| {
        let tmp = tempfile::tempdir().unwrap();
        record_promoted_epoch(tmp.path(), 7).unwrap();

        assert!(epoch_path_for(tmp.path()).starts_with(tmp.path()));
        assert!(!home.join("locks").join(format!("{}.epoch", crate::infra::folder_store::name_for(tmp.path()))).exists());
        assert_eq!(last_promoted_epoch(tmp.path()), 7);
    });
}

/// A whole build of a folder leaves nothing for it in `~/.moss/locks`. It
/// runs under the default home: an override held for a whole build would move
/// the lock path under a parallel test that took its lock before.
#[test]
fn a_promoted_build_leaves_nothing_in_the_locks_folder() {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    crate::infra::home::without_moss_home(|| {
        let home = crate::infra::home::moss_home().unwrap();
        let site = tempfile::tempdir().unwrap();
        std::fs::write(site.path().join("index.md"), "---\ntitle: Orchard\n---\n\nPlum trees.\n").unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(run_pipeline(PipelineConfig {
            root: crate::vault::paths::VaultRoot::resolve(site.path()),
            progress: crate::build::null_sink(), plugins: PluginMode::Skip,
            watch: false, start_server: false,
            host: crate::build::ports::host::test_host_ports(),
            trigger: BuildTrigger::Full, exits_after_build: true,
            site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
        })).expect("fixture builds");
        assert!(last_promoted_epoch(site.path()) > 0, "the build never recorded a promotion");
        let id = crate::infra::folder_store::name_for(site.path());
        let left: Vec<_> = std::fs::read_dir(home.join("locks")).into_iter().flatten().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&id)).map(|e| e.file_name()).collect();
        if cfg!(unix) {
            assert!(left.is_empty(), "the build left {left:?} in the locks folder");
        }
    });
}

const HOLD_ENV: &str = "MOSS_TEST_HOLD_LOCK_FILE";

/// An owner acquirer has opened its file but has not locked it yet. The
/// background build-lock collector must not compete for that file: its
/// temporary lock would make this process refuse its own next build.
#[cfg(unix)]
#[test]
fn the_litter_sweep_never_competes_with_owner_admission() {
    with_moss_home(|home| {
        let folder = tempfile::tempdir().unwrap();
        let path = lock_path_named(folder.path(), "owner").unwrap();
        let opening = open_lock_file(&path).unwrap();
        assert_eq!(sweep_litter(&home.join("locks"), 1, std::time::Duration::ZERO), 0);
        assert!(still_named(&opening, &path), "the sweep must leave the owner's open file alone");

        use crate::ops::serve::ownership::{acquire, HostKind};
        let first = acquire(folder.path(), HostKind::Cli, "test".into(), String::new()).unwrap();
        let second = acquire(folder.path(), HostKind::Cli, "test".into(), String::new()).unwrap();
        drop(first);
        assert!(try_acquire_named(folder.path(), "owner").unwrap().is_none());
        drop(second);
        assert!(try_acquire_named(folder.path(), "owner").unwrap().is_some());
    });
}

const CHILD_TEST: &str = "infra::folder_lock::folder_lock_tests::child_lock_holder";

/// Not a test of its own: [`the_sweep_leaves_a_lock_another_process_holds`]
/// runs this binary again with only this test, as the process that holds the
/// lock. It holds until it is killed.
#[test]
fn child_lock_holder() {
    let Some(path) = std::env::var_os(HOLD_ENV) else { return };
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    file.lock().unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// The sweep removes lock files nobody holds and leaves the one another
/// process holds: deleting that one would let a second build take the
/// folder while the first is still writing it.
#[cfg(unix)]
#[test]
fn the_sweep_leaves_a_lock_another_process_holds() {
    let dir = tempfile::tempdir().unwrap();
    let held = dir.path().join("held.build");
    let litter: Vec<_> = (0..5).map(|i| dir.path().join(format!("gone{i}.build"))).chain([dir.path().join("old.epoch")]).collect();
    for path in &litter {
        std::fs::write(path, "").unwrap();
    }

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--test-threads=1"])
        .env(HOLD_ENV, &held)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("run this test binary again as the holding process");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let locked = std::fs::File::open(&held).is_ok_and(|f| matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        if locked {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the child never took the lock; has {CHILD_TEST} moved?");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let removed = sweep_litter(dir.path(), 2, std::time::Duration::ZERO);
    let held_survived = held.exists();
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(held_survived, "the sweep removed a lock another process holds");
    assert_eq!(removed, litter.len());
    let left: Vec<_> = litter.iter().filter(|p| p.exists()).collect();
    assert!(left.is_empty(), "the sweep left unheld files: {left:?}");
}
