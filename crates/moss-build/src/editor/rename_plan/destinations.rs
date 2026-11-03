//! Where a batch of entries lands when it is moved into one folder.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The `(from, to)` moves that put each of `paths` into `target_dir`. A name
/// already taken there, on disk or by an earlier entry of the batch, gets the
/// next free suffix (`a-2.md`, `a-3.md`), as Finder's "Keep Both" does. An
/// entry inside another selected folder travels with that folder, and one
/// already in `target_dir` is left out. Nothing is moved.
pub(crate) fn into_folder(project_root: &Path, paths: &[String], target_dir: &Path) -> Result<Vec<(String, String)>, String> {
    crate::vault::fs::rejects_traversal(&target_dir.to_string_lossy())?;
    if !target_dir.is_dir() {
        return Err(format!("Target '{}' is not a directory", target_dir.display()));
    }
    let (root, target) = crate::vault::fs::recheck_canonical(project_root, target_dir)?;
    let mut claimed = HashSet::new();
    let mut moves = Vec::new();
    for path in outermost(paths) {
        if let Some(dest) = destination(path, target_dir, &root, &target, &mut claimed)
            .map_err(|e| format!("Failed to move '{path}': {e}"))?
        {
            moves.push((path.clone(), dest));
        }
    }
    Ok(moves)
}

/// The selected entries, once each, that no other selected entry contains.
fn outermost(paths: &[String]) -> Vec<&String> {
    let mut seen = HashSet::new();
    paths
        .iter()
        .filter(|p| seen.insert(p.as_str()))
        .filter(|p| !paths.iter().any(|other| other != *p && Path::new(p.as_str()).starts_with(other)))
        .collect()
}

fn destination(
    source: &str,
    target_dir: &Path,
    root: &Path,
    target: &Path,
    claimed: &mut HashSet<PathBuf>,
) -> Result<Option<String>, String> {
    crate::vault::fs::rejects_traversal(source)?;
    let src = Path::new(source);
    let (_, src_canonical) = crate::vault::fs::recheck_canonical(root, src)?;
    if src_canonical == root {
        return Err("Cannot move the project root".to_string());
    }
    if src_canonical.parent() == Some(target) {
        return Ok(None);
    }
    let name = src.file_name().ok_or_else(|| format!("Cannot determine filename for '{source}'"))?;
    let taken = |p: &Path| p.exists() || claimed.contains(p);
    let mut dest = target_dir.join(name);
    if taken(&dest) {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext = src.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
        dest = (2..=100)
            .map(|n| target_dir.join(format!("{stem}-{n}{ext}")))
            .find(|d| !taken(d))
            .ok_or_else(|| format!("Too many name collisions for '{}' in '{}'", name.to_string_lossy(), target_dir.display()))?;
    }
    claimed.insert(dest.clone());
    Ok(Some(dest.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["posts", "notes", "dst"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        (dir, root)
    }

    fn file(root: &Path, rel: &str) -> String {
        // allow:raw_write test fixture in a temp folder
        std::fs::write(root.join(rel), rel).unwrap();
        root.join(rel).to_string_lossy().into_owned()
    }

    fn s(p: PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_taken_name_gets_the_next_free_suffix_on_disk_and_within_the_batch() {
        let (_d, root) = site();
        file(&root, "dst/a.md");
        file(&root, "dst/a-2.md");
        let batch = [file(&root, "posts/a.md"), file(&root, "notes/a.md")];
        let moves = into_folder(&root, &batch, &root.join("dst")).unwrap();
        let to: Vec<_> = moves.into_iter().map(|(_, to)| to).collect();
        assert_eq!(to, [s(root.join("dst/a-3.md")), s(root.join("dst/a-4.md"))]);
        assert!(root.join("posts/a.md").exists(), "nothing moves");
    }

    /// The target is named through a symlink, so "already there" must compare
    /// canonical paths.
    #[cfg(unix)]
    #[test]
    fn an_entry_inside_a_selected_folder_and_one_already_there_are_left_out() {
        let (d, root) = site();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let inside = file(&root, "posts/a.md");
        let already = file(&root, "dst/b.md");
        let posts = s(root.join("posts"));
        let moves = into_folder(&root, &[posts.clone(), inside, already], &link.join("dst")).unwrap();
        assert_eq!(moves, [(posts, s(link.join("dst/posts")))]);
    }

    #[test]
    fn a_name_prefix_is_not_an_ancestor_and_a_repeat_counts_once() {
        let paths = ["posts".to_string(), "posts2/a.md".to_string(), "posts".to_string()];
        assert_eq!(outermost(&paths), [&paths[0], &paths[1]]);
    }

    #[test]
    fn traversal_the_root_and_a_target_outside_are_refused() {
        let (d, root) = site();
        let a = file(&root, "posts/a.md");
        let refused = |paths: &[String], target: &Path| into_folder(&root, paths, target).unwrap_err();
        assert!(refused(&[a.clone()], &root.join("dst/../posts")).contains("traversal"));
        assert!(refused(&[s(root.join("posts/../notes"))], &root.join("dst")).contains("traversal"));
        assert!(refused(&[s(root.clone())], &root.join("dst")).contains("project root"));
        let outside = tempfile::tempdir_in(d.path().parent().unwrap()).unwrap();
        assert!(refused(&[a], outside.path()).contains("within the project"));
    }
}
