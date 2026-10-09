use super::*;

#[test]
fn an_unreadable_root_keeps_the_last_complete_watch_set() {
    use crate::build::cloud_readiness::storage::{StorageOperation, TestFault};

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("posts")).unwrap();
    let mut debouncer = notify_debouncer_full::new_debouncer(
        std::time::Duration::from_millis(100),
        None,
        |_| {},
    )
    .unwrap();
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
    let mut debouncer = notify_debouncer_full::new_debouncer(
        std::time::Duration::from_millis(100), None, |_| {},
    ).unwrap();
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
