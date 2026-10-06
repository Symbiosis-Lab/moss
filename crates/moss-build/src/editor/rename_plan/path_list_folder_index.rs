//! In-memory `FolderIndex` over a plain path list.
//!
//! A `ContentGraph` built from a path list already serves as the file set
//! (`ReferenceContext::assets`); nothing equivalent existed for `FolderIndex`,
//! so this is the one adapter the pure planner needs — everything else
//! (`classify_reference`, `ContentGraph`) is zero-I/O.

use std::collections::HashSet;

use moss_core::resolve::folder_class::FolderIndex;

/// Every directory referenced by `paths` — the planner's stand-in for
/// `ProjectStructure::dirs`. Shared by `Indexes::build` (rename_plan.rs) and
/// [`PathListFolderIndex::build`] so the two indexes never disagree.
pub(crate) fn collect_dirs(paths: &[String]) -> Vec<String> {
    let mut dirs = HashSet::new();
    for p in paths {
        let mut cur = p.as_str();
        while let Some((parent, _)) = cur.rsplit_once('/') {
            dirs.insert(parent.to_string());
            cur = parent;
        }
    }
    dirs.into_iter().collect()
}

pub(crate) struct PathListFolderIndex {
    dirs: HashSet<String>,
    files: HashSet<String>,
}

impl PathListFolderIndex {
    pub(crate) fn build(paths: &[String]) -> Self {
        let files = paths.iter().cloned().collect();
        PathListFolderIndex { dirs: collect_dirs(paths).into_iter().collect(), files }
    }
}

impl FolderIndex for PathListFolderIndex {
    fn is_dir(&self, root_rel: &str) -> bool {
        root_rel.is_empty() || self.dirs.contains(root_rel)
    }

    fn dir_has_markdown_index(&self, root_rel: &str) -> bool {
        let join = |name: &str| {
            if root_rel.is_empty() {
                name.to_string()
            } else {
                format!("{root_rel}/{name}")
            }
        };
        for stem in moss_core::home::INDEX_STEMS {
            if self.files.contains(join(&format!("{stem}.md")).as_str())
                || self.files.contains(join(&format!("{stem}.markdown")).as_str())
            {
                return true;
            }
        }
        if !root_rel.is_empty() {
            let leaf = root_rel.rsplit('/').next().unwrap_or(root_rel);
            if self.files.contains(join(&format!("{leaf}.md")).as_str()) {
                return true;
            }
        }
        false
    }

    fn dir_has_static_index(&self, root_rel: &str) -> Option<String> {
        for name in ["index.html", "index.htm"] {
            let p = if root_rel.is_empty() {
                name.to_string()
            } else {
                format!("{root_rel}/{name}")
            };
            if self.files.contains(p.as_str()) {
                return Some(name.to_string());
            }
        }
        None
    }
}
