//! Directory-shaped resolution capability (injected; pure core).
//! Build impl is backed by the content graph + html_files; the editor impl by
//! the last build's folder-index URL set plus a vault walk. Separate from
//! AssetIndex because the backing data and the query shape (is_dir /
//! markdown-index / static-index) differ.

pub trait FolderIndex {
    /// Does a directory exist at this root-relative path?
    fn is_dir(&self, root_rel: &str) -> bool;
    /// Does this folder resolve a markdown index (→ FolderListing)?
    /// Build: a doc whose url_path is this folder's index URL (covers
    /// content-folder promotion). Editor: the last build's folder-index URL
    /// set, reconstructed from `ArticleMap.pages` ∪ the scanned directory set
    /// (see ADR-040) — NOT an `index.md` stat.
    fn dir_has_markdown_index(&self, root_rel: &str) -> bool;
    /// Does this folder have a static index.html/.htm (and no markdown index)?
    /// Returns the index filename (→ FolderIndexIframe).
    fn dir_has_static_index(&self, root_rel: &str) -> Option<String>;
}

#[cfg(test)]
pub(crate) struct FakeFolderIndex {
    pub dirs: std::collections::HashSet<String>,
    pub md_index: std::collections::HashSet<String>,
    pub static_index: std::collections::HashMap<String, String>,
}

#[cfg(test)]
impl FakeFolderIndex {
    pub fn new() -> Self {
        FakeFolderIndex {
            dirs: Default::default(),
            md_index: Default::default(),
            static_index: Default::default(),
        }
    }
}

#[cfg(test)]
impl FolderIndex for FakeFolderIndex {
    fn is_dir(&self, p: &str) -> bool {
        self.dirs.contains(p)
    }
    fn dir_has_markdown_index(&self, p: &str) -> bool {
        self.md_index.contains(p)
    }
    fn dir_has_static_index(&self, p: &str) -> Option<String> {
        self.static_index.get(p).cloned()
    }
}
