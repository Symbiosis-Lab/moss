//! Host adapter for the portable folder index.
use std::path::Path;
use crate::build::scan::article_map::ArticleMap;
use super::asset_resolver::{reference_input, EditorScope};

use moss_core::resolve::folder_class::FolderIndex;

pub struct EditorFolderIndex {
    root: std::path::PathBuf,
    map: moss_core::resolve::editor::UrlMap,
    index: std::cell::OnceCell<moss_core::resolve::editor::folders::EditorFolderIndex>,
}
impl EditorFolderIndex {
    pub fn new(root: &Path, map: &ArticleMap) -> Self {
        Self { root: root.to_path_buf(), map: super::url_index::reference_map(map), index: std::cell::OnceCell::new() }
    }

    // Link-only consumers never ask a folder predicate and need no disk walk.
    fn index(&self) -> &moss_core::resolve::editor::folders::EditorFolderIndex {
        self.index.get_or_init(|| {
            let mut input = reference_input(&self.root, EditorScope::Site, "", &[]);
            input.url_index = Some(self.map.clone());
            moss_core::resolve::editor::folders::EditorFolderIndex::new(&input)
        })
    }
}

impl FolderIndex for EditorFolderIndex {
    fn is_dir(&self, path: &str) -> bool { self.index().is_dir(path) }
    fn dir_has_markdown_index(&self, path: &str) -> bool { self.index().dir_has_markdown_index(path) }
    fn dir_has_static_index(&self, path: &str) -> Option<String> { self.index().dir_has_static_index(path) }
}
