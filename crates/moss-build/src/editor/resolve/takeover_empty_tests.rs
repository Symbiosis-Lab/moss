use super::*;

#[test]
fn folder_empty_tracks_author_entries_and_preserves_unknown() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(folder_empty(dir.path(), dir.path()), Some(true));
    std::fs::create_dir(dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join(".DS_Store"), "").unwrap();
    assert_eq!(folder_empty(dir.path(), dir.path()), Some(true));
    std::fs::create_dir(dir.path().join("posts")).unwrap();
    assert_eq!(folder_empty(dir.path(), dir.path()), Some(false));
    assert_eq!(folder_empty(&dir.path().join("posts"), dir.path()), Some(true));
    std::fs::write(dir.path().join("posts/story.md"), "story").unwrap();
    assert_eq!(folder_empty(&dir.path().join("posts"), dir.path()), Some(false));
    std::fs::remove_file(dir.path().join("posts/story.md")).unwrap();
    assert_eq!(folder_empty(&dir.path().join("posts"), dir.path()), Some(true));
    assert_eq!(folder_empty(&dir.path().join("missing"), dir.path()), None);
}

#[test]
fn a_stale_home_map_after_deletion_resolves_the_empty_folder_recipe() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".moss/build.nosync")).unwrap();
    let map = ArticleMap { pages: [("".into(), "index.md".into())].into(), ..Default::default() };
    map.save(&dir.path().join(".moss")).unwrap();
    let source = crate::editor::page_source::resolve_page_source("/", dir.path()).unwrap();
    assert_eq!(source.takeover.unwrap().folder_empty, Some(true));
}

#[test]
fn an_empty_nested_folder_has_its_own_canonical_recipe() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("posts")).unwrap();
    let source = crate::editor::page_source::resolve_page_source("/posts/", dir.path()).unwrap();
    let recipe = source.takeover.unwrap();
    assert_eq!(recipe.folder_empty, Some(true));
    assert_eq!(recipe.files[0].dir, dir.path().join("posts").to_string_lossy());
}
