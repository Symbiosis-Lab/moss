use super::*;
use crate::vault::derive_folders::{folder_decision, set_folder_decision};
use notify::event::{CreateKind, ModifyKind, RenameMode};
use notify::EventKind;
use std::time::Instant;

fn created(path: &Path) -> DebouncedEvent {
    DebouncedEvent::new(notify::Event::new(EventKind::Create(CreateKind::File)).add_path(path.to_path_buf()), Instant::now())
}

fn moved_in(path: &Path) -> DebouncedEvent {
    DebouncedEvent::new(
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any))).add_path(path.to_path_buf()),
        Instant::now(),
    )
}

fn site_with_fake_plugin() -> (tempfile::TempDir, MossPaths) {
    let dir = tempfile::Builder::new().prefix("site").tempdir().unwrap();
    let plugin = dir.path().join(".moss/plugins/fakes");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("manifest.json"),
        r##"{"name":"fakes","version":"1.0.0","contributes":{"derivations":{"pages":{
            "accepts":["fake"],"noun":{"one":"doc","other":"docs"},
            "fragments":{"text":"markdown"},"stub":["#text"],
            "run":{"binary":"unpack","artifact":[{"kind":"path","binary":"/bin/true"}]}}}}}"##,
    )
    .unwrap();
    let site = MossPaths::new(&std::fs::canonicalize(dir.path()).unwrap());
    (dir, site)
}

#[test]
fn a_new_file_in_a_shown_folder_gets_its_stub() {
    let (dir, site) = site_with_fake_plugin();
    let folder = dir.path().join("reports");
    std::fs::create_dir_all(&folder).unwrap();
    set_folder_decision(&site, &folder, Decision::Show(vec!["pages".into()])).unwrap();
    std::fs::write(folder.join("new.fake"), "x").unwrap();
    react(dir.path().to_str().unwrap(), &[created(&folder.join("new.fake"))], &[]);
    assert!(folder.join("new.md").is_file());
}

#[test]
fn a_file_moved_into_a_shown_folder_gets_its_stub() {
    let (dir, site) = site_with_fake_plugin();
    let folder = dir.path().join("reports");
    std::fs::create_dir_all(&folder).unwrap();
    set_folder_decision(&site, &folder, Decision::Show(vec!["pages".into()])).unwrap();
    std::fs::write(folder.join("new.fake"), "x").unwrap();
    react(dir.path().to_str().unwrap(), &[moved_in(&folder.join("new.fake"))], &[]);
    assert!(folder.join("new.md").is_file());
}

#[test]
fn a_file_renamed_inside_the_site_gets_no_stub() {
    let (dir, site) = site_with_fake_plugin();
    let folder = dir.path().join("reports");
    std::fs::create_dir_all(&folder).unwrap();
    set_folder_decision(&site, &folder, Decision::Show(vec!["pages".into()])).unwrap();
    std::fs::write(folder.join("new.fake"), "x").unwrap();
    let renamed = DebouncedEvent::new(
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(folder.join("old.fake"))
            .add_path(folder.join("new.fake")),
        Instant::now(),
    );
    react(dir.path().to_str().unwrap(), &[renamed], &[]);
    assert!(!folder.join("new.md").exists());
}

#[test]
fn a_new_file_in_an_undecided_folder_gets_nothing() {
    let (dir, _site) = site_with_fake_plugin();
    std::fs::write(dir.path().join("new.fake"), "x").unwrap();
    react(dir.path().to_str().unwrap(), &[created(&dir.path().join("new.fake"))], &[]);
    assert!(!dir.path().join("new.md").exists());
}

#[test]
fn a_renamed_folder_keeps_its_decision() {
    let (dir, site) = site_with_fake_plugin();
    set_folder_decision(&site, Path::new("reports"), Decision::Keep).unwrap();
    std::fs::create_dir(dir.path().join("papers")).unwrap();
    react(dir.path().to_str().unwrap(), &[], &[("reports".into(), "papers".into())]);
    assert_eq!(folder_decision(&site, Path::new("papers")), Decision::Keep);
}

#[test]
fn renames_chained_in_one_batch_do_not_move_a_decision_onto_the_wrong_folder() {
    let (dir, site) = site_with_fake_plugin();
    // `c` was kept; then `a` became `b` and `c` became `a`, reported in the
    // order that would replay the second rename's result into the first.
    set_folder_decision(&site, Path::new("a"), Decision::Show(vec!["pages".into()])).unwrap();
    set_folder_decision(&site, Path::new("c"), Decision::Keep).unwrap();
    std::fs::create_dir(dir.path().join("a")).unwrap();
    std::fs::create_dir(dir.path().join("b")).unwrap();
    react(
        dir.path().to_str().unwrap(),
        &[],
        &[("c".into(), "a".into()), ("a".into(), "b".into())],
    );
    assert_eq!(folder_decision(&site, Path::new("a")), Decision::Keep);
    assert_ne!(folder_decision(&site, Path::new("b")), Decision::Keep);
}

/// Through the driver's own batch handler, so a missing call site fails here.
#[tokio::test]
async fn the_watch_driver_writes_the_stub_when_a_file_arrives() {
    let (dir, site) = site_with_fake_plugin();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let folder = root.join("reports");
    std::fs::create_dir_all(&folder).unwrap();
    set_folder_decision(&site, &folder, Decision::Show(vec!["pages".into()])).unwrap();
    std::fs::write(folder.join("new.fake"), "x").unwrap();

    let emit: super::super::EventRelay = std::sync::Arc::new(|_| {});
    let dispatch: super::super::RebuildDispatch = std::sync::Arc::new(|_| Box::pin(async {}));
    super::super::handle_debounced_batch(
        vec![created(&folder.join("new.fake"))],
        root.to_str().unwrap(),
        &emit,
        &dispatch,
    )
    .await;
    // The reaction runs on the blocking pool; give it a moment.
    for _ in 0..100 {
        if folder.join("new.md").is_file() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(folder.join("new.md").is_file());
}

#[test]
fn a_file_that_arrives_goes_to_the_front_of_the_derive_queue() {
    let (dir, _site) = site_with_fake_plugin();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("new.fake"), "x").unwrap();
    std::fs::write(root.join("old.fake"), "x").unwrap();
    react(&root.to_string_lossy(), &[created(&root.join("new.fake"))], &[]);
    let arrived = |f: &str| crate::system::derive::priority::arrived_in_session(&root, f, &[]);
    assert!(arrived("new.fake"));
    assert!(!arrived("old.fake"));
}
