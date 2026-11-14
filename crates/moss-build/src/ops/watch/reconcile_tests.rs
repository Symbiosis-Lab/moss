use super::*;

#[test]
fn an_unreadable_root_keeps_the_last_complete_watch_set() {
    use crate::build::cloud_readiness::storage::{StorageOperation, TestFault};

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("posts")).unwrap();
    let mut debouncer = new_debouncer(std::time::Duration::from_millis(100), |_| {}).unwrap();
    let mut watched = Vec::new();
    targets(&mut debouncer, root.path(), &mut watched).unwrap();
    assert!(watched.iter().any(|(path, _)| path == &root.path().join("posts")));
    let before = watched.clone();

    // Keep the managed refusal active for the full assertion. A one-shot
    // EDEADLK can be recovered by the pool if the first attempt runs inline.
    let _fault = TestFault::install(
        root.path(),
        root.path(),
        StorageOperation::ReadDirOpen,
        usize::MAX,
        std::time::Duration::from_millis(50),
    );
    assert!(targets(&mut debouncer, root.path(), &mut watched).is_err());

    assert_eq!(watched, before, "an unknown listing cannot revoke valid subscriptions");
}

#[test]
fn a_read_dir_error_after_a_prefix_keeps_the_last_complete_watch_set() {
    use crate::build::cloud_readiness::storage::{StorageOperation, TestFault};

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("posts")).unwrap();
    std::fs::create_dir(root.path().join("pages")).unwrap();
    let mut debouncer = new_debouncer(std::time::Duration::from_millis(100), |_| {}).unwrap();
    let mut watched = Vec::new();
    targets(&mut debouncer, root.path(), &mut watched).unwrap();
    let before = watched.clone();

    // Keep the managed refusal active for the full assertion. A one-shot
    // EDEADLK can be recovered by the pool if the first attempt runs inline.
    let _fault = TestFault::install(
        root.path(), root.path(), StorageOperation::ReadDirNext, usize::MAX,
        std::time::Duration::from_millis(50),
    );
    assert!(targets(&mut debouncer, root.path(), &mut watched).is_err());

    assert_eq!(watched, before, "a prefix is not a complete replacement watch set");
}

/// On macOS every top-level file is its own target, and a restart of the event
/// stream for each one takes about a quarter of a second.
#[cfg(target_os = "macos")]
#[test]
fn a_folder_of_many_top_level_files_is_watched_in_one_step() {
    let root = tempfile::tempdir().unwrap();
    for n in 0..200 {
        std::fs::write(root.path().join(format!("page-{n}.md")), "A page.\n").unwrap();
    }
    let mut debouncer = new_debouncer(std::time::Duration::from_millis(100), |_| {}).unwrap();
    let mut watched = Vec::new();

    let started = std::time::Instant::now();
    targets(&mut debouncer, root.path(), &mut watched).unwrap();
    let took = started.elapsed();

    assert!(watched.len() >= 200);
    assert!(took < std::time::Duration::from_secs(15), "watching {} target(s) took {took:?}", watched.len());
}
