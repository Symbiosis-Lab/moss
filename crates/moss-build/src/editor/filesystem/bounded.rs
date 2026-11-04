//! The tree of a loose document's folder, which can be any folder on the
//! machine (a home directory, `~/Downloads`): listed breadth-first and only
//! until a budget of entries is spent, so it never walks the whole disk.

use std::path::Path;

use super::{list_directory_inner, DirEntry, TreeNode};

/// Entries a loose document's tree may list before deeper folders are left
/// unlisted (`children: None`, which the tree shows as an unavailable level).
pub const DOCUMENT_TREE_BUDGET: usize = 2000;

/// No front-matter reads, so no publish dates, home election or hidden counts;
/// order is the directory listing's own.
pub fn bounded_tree(path: &str, show_internal: bool, mut budget: usize) -> Result<TreeNode, String> {
    struct Slot {
        entry: DirEntry,
        kids: Option<Vec<usize>>,
    }
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut slots = vec![Slot {
        entry: DirEntry { name, path: path.to_string(), is_dir: true, modified: None, file_id: None },
        kids: None,
    }];
    let mut queue = std::collections::VecDeque::from([0usize]);
    while let Some(i) = queue.pop_front() {
        if !slots[i].entry.is_dir || (budget == 0 && i != 0) {
            continue;
        }
        let dir = slots[i].entry.path.clone();
        let entries = match list_directory_inner(&dir, path, show_internal) {
            Ok(entries) => entries,
            Err(e) if i == 0 => return Err(e),
            Err(_) => continue,
        };
        budget = budget.saturating_sub(entries.len());
        let mut kids = Vec::with_capacity(entries.len());
        for entry in entries {
            kids.push(slots.len());
            queue.push_back(slots.len());
            slots.push(Slot { entry, kids: None });
        }
        slots[i].kids = Some(kids);
    }
    fn build(slots: &mut [Slot], i: usize) -> TreeNode {
        let kids = slots[i].kids.take();
        let children = kids.map(|ks| ks.into_iter().map(|k| build(slots, k)).collect());
        let e = &slots[i].entry;
        TreeNode {
            name: e.name.clone(),
            path: e.path.clone(),
            is_dir: e.is_dir,
            modified: e.modified,
            publish_date: None,
            date_source: moss_core::date::DateSource::None,
            is_home: false,
            children,
            hidden: 0,
            file_id: e.file_id.clone(),
            special: None,
        }
    }
    Ok(build(&mut slots, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        for page in ["note.md", "sub/inner.md", "sub/deeper/leaf.md"] {
            // allow:raw_write test fixture pages in a temp folder
            std::fs::write(dir.path().join(page), "hi").unwrap();
        }
        dir
    }

    fn find<'a>(n: &'a TreeNode, name: &str) -> &'a TreeNode {
        n.children.as_ref().unwrap().iter().find(|k| k.name == name).unwrap()
    }

    #[test]
    fn a_small_folder_is_listed_all_the_way_down() {
        let dir = fixture();
        let root = bounded_tree(&dir.path().to_string_lossy(), false, DOCUMENT_TREE_BUDGET).unwrap();
        assert_eq!(find(find(find(&root, "sub"), "deeper"), "leaf.md").name, "leaf.md");
    }

    #[test]
    fn folders_past_the_budget_are_left_unlisted() {
        let dir = fixture();
        let root = bounded_tree(&dir.path().to_string_lossy(), false, 3).unwrap();
        assert!(find(&root, "sub").children.is_some(), "the root and first level are listed");
        assert!(find(find(&root, "sub"), "deeper").children.is_none(), "past the budget: unlisted");
    }
}
