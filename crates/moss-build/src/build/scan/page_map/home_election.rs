//! The home election as the build and the editor's address planner run it.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::types::content::FileInfo;

fn parent_dir(path: &str) -> &str {
    Path::new(path).parent().and_then(|p| p.to_str()).unwrap_or("")
}

/// Each folder's home election: the `home: true` overrides and the winning
/// file per folder. `reserved` folders have a verbatim `index.html`, so no
/// markdown file there can be the home: they are left out of the election and
/// each file that would have won is warned about, since it publishes at its
/// own address instead. [`crate::build::scan::classify::reserved_home_dirs`]
/// decides which folders are reserved.
pub(crate) fn elect_homes_with_evicted(
    markdown_files: &[FileInfo],
    root: &crate::vault::paths::VaultRoot,
    reserved: &HashSet<String>,
    locate: &dyn Fn(&str) -> std::path::PathBuf,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> (HashMap<String, String>, HashSet<String>) {
    let (closed, open): (Vec<FileInfo>, Vec<FileInfo>) =
        markdown_files.iter().cloned().partition(|f| reserved.contains(parent_dir(&f.path)));
    let overrides = super::compute_home_overrides_with_evicted(&open, root, locate, is_evicted);
    let winners = super::compute_home_file_winners(&open, root.name(), &overrides);

    let mut displaced: Vec<String> = super::compute_home_file_winners(&closed, root.name(), &HashMap::new())
        .into_iter()
        .chain(closed.iter().filter(|f| {
            let abs = locate(&f.path);
            !is_evicted(&abs) && crate::build::scan::scan::file_has_home_marker(&abs)
        }).map(|f| f.path.clone()))
        .collect();
    displaced.sort();
    displaced.dedup();
    for path in displaced {
        let dir = parent_dir(&path);
        log::warn!(
            "`{path}` would be the home page of `{dir}/`, but `{dir}/index.html` is copied as-is, so `{path}` is published at its own address instead"
        );
    }
    (overrides, winners)
}

/// [`elect_homes_with_evicted`] reading each file from under the site folder.
pub(crate) fn elect_homes(
    markdown_files: &[FileInfo],
    root: &crate::vault::paths::VaultRoot,
    reserved: &HashSet<String>,
) -> (HashMap<String, String>, HashSet<String>) {
    let source_path = root.path();
    elect_homes_with_evicted(
        markdown_files,
        root,
        reserved,
        &|p| source_path.join(p),
        &crate::build::icloud::is_evicted_and_requested,
    )
}

/// Whether a file that lost its folder's home election is that home's
/// language variant (`index.fr.md`), which shares the winner's address so slug
/// deduplication can place it. A folder without a winner has no address to
/// share.
pub(crate) fn is_home_translation(file_path: &str, winners: &HashSet<String>) -> bool {
    let stem = Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let dir = parent_dir(file_path);
    moss_core::home::strip_lang_suffix(&stem).is_some_and(moss_core::home::is_index_stem)
        && winners.iter().any(|w| parent_dir(w) == dir)
}

/// The default edition's language, [`moss_core::home::root_language`] over
/// every page but the root's home, its translations and the layout slots.
pub(crate) fn root_language<'a>(site_lang: &'a str, files: &crate::types::content::ProjectStructure, winners: &HashSet<String>) -> &'a str {
    let is_root_home = |p: &str| !p.contains('/') && (winners.contains(p) || is_home_translation(p, winners) || p == "index.html");
    moss_core::home::root_language(
        site_lang,
        files
            .markdown_files
            .iter()
            .chain(&files.html_files)
            .map(|f| f.path.as_str())
            .filter(|p| !is_root_home(p) && !crate::build::footer::is_excluded_from_pages(p)),
    )
}
