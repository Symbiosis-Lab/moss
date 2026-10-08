use super::{FolderSession, PreviewSource};
use std::path::PathBuf;

#[test]
fn focus_revision_changes_only_when_the_requirement_changes() {
    let root = tempfile::tempdir().unwrap();
    let session = FolderSession::new(root.path().to_path_buf());
    assert!(session.preview_requirement().is_none());
    assert!(session.set_preview_requirement("/posts/one/".into(), PreviewSource::Unresolved).unwrap());
    let first = session.preview_requirement().unwrap();
    assert_eq!(first.revision, 1);
    assert!(!session.set_preview_requirement("/posts/one/".into(), PreviewSource::Unresolved).unwrap());
    assert_eq!(session.preview_requirement().unwrap().revision, first.revision);
    assert!(session.set_preview_requirement("/posts/one/".into(), PreviewSource::File(PathBuf::from("posts/one.md"))).unwrap());
    assert_eq!(session.preview_requirement().unwrap().revision, first.revision + 1);
    assert!(session.set_preview_requirement("/posts/two/".into(), PreviewSource::Unresolved).unwrap());
    assert_eq!(session.preview_requirement().unwrap().revision, first.revision + 2);
}

#[test]
fn source_paths_cannot_escape_the_folder() {
    let root = tempfile::tempdir().unwrap();
    let session = FolderSession::new(root.path().to_path_buf());
    for source in [PathBuf::from("../outside.md"), PathBuf::from("/outside.md")] {
        assert!(session.set_preview_requirement("/posts/one/".into(), PreviewSource::File(source)).is_err());
        assert!(session.preview_requirement().is_none());
    }
}

#[test]
fn a_generated_page_is_distinct_from_a_pending_lookup() {
    let root = tempfile::tempdir().unwrap();
    let session = FolderSession::new(root.path().to_path_buf());
    session.set_preview_requirement("/places/".into(), PreviewSource::Unresolved).unwrap();
    let pending = session.preview_requirement().unwrap();
    session.set_preview_requirement("/places/".into(), PreviewSource::Generated).unwrap();
    let generated = session.preview_requirement().unwrap();
    assert_eq!(generated.source, PreviewSource::Generated);
    assert!(generated.revision > pending.revision);
}

#[test]
fn preview_url_cannot_traverse_the_output() {
    let root = tempfile::tempdir().unwrap();
    let session = FolderSession::new(root.path().to_path_buf());
    assert!(session.set_preview_requirement("/../outside".into(), PreviewSource::Generated).is_err());
    assert!(session.preview_requirement().is_none());
}
