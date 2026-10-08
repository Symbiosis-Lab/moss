//! Source files that differ only by letter case.
//!
//! `Photo.JPG` beside `photo.jpg` is two files on a case-sensitive disk and
//! one on the default macOS and Windows volumes, where saving the second
//! silently replaces the first. A site checked out on Linux, or synced from a
//! machine with a case-sensitive volume, can carry both, and the moment it is
//! built or published somewhere case-insensitive one of them is gone with no
//! error. Detecting the pair is a comparison of the paths themselves, so it
//! does not depend on the disk the scan happens to run on.
//!
//! Two markdown pages are left to the passes that already name them: a clash
//! of page addresses is reported by `slug::resolve_duplicate_slugs_with_lang`
//! and a clash for the home page by the home-slot election. Reporting them
//! here too would print one problem twice.

use std::collections::HashMap;

use crate::types::content::ProjectStructure;

fn is_markdown(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// Every pair of paths equal when compared case-insensitively but not
/// exactly, as `(first, second)` in input order, except pairs where both are
/// markdown. A group of three yields two pairs, each against the first path.
pub(crate) fn case_colliding_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut first_seen: HashMap<String, &str> = HashMap::new();
    let mut pairs = Vec::new();
    for path in paths {
        match first_seen.get(&path.to_lowercase()) {
            Some(first) if *first != path && !(is_markdown(first) && is_markdown(path)) => {
                pairs.push((first.to_string(), path.to_string()))
            }
            Some(_) => {}
            None => {
                first_seen.insert(path.to_lowercase(), path);
            }
        }
    }
    pairs
}

/// Warn, as a build problem so `--strict` fails, once per colliding pair among every file the scan found.
pub(crate) fn warn_case_colliding_files(found: &ProjectStructure) {
    let others = found.image_files.iter().chain(&found.video_files).map(|f| f.path.as_str());
    let files = [&found.markdown_files, &found.html_files, &found.notebook_files, &found.other_files];
    let paths = files.into_iter().flatten().map(|f| f.path.as_str()).chain(others);
    for (first, second) in case_colliding_paths(paths) {
        crate::build::cli_output::log_warn_problem!(
            "`{first}` and `{second}` differ only by letter case: on a case-insensitive disk (the macOS and Windows default) one overwrites the other. Rename one of them."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_both_files_when_only_the_case_differs() {
        let pairs = case_colliding_paths(["photo.jpg", "about.txt", "photo.JPG"]);
        assert_eq!(pairs, vec![("photo.jpg".to_string(), "photo.JPG".to_string())]);
    }

    #[test]
    fn a_static_asset_pair_collides() {
        assert_eq!(case_colliding_paths(["data/Foo.csv", "data/foo.csv"]).len(), 1);
    }

    #[test]
    fn compares_whole_paths_so_a_folder_name_counts() {
        assert_eq!(case_colliding_paths(["Docs/a.png", "docs/A.png"]).len(), 1);
    }

    #[test]
    fn two_markdown_pages_are_left_to_the_url_and_home_passes() {
        assert!(case_colliding_paths(["index.md", "Index.md"]).is_empty());
    }

    #[test]
    fn distinct_names_and_a_repeated_identical_path_are_not_collisions() {
        assert!(case_colliding_paths(["a.png", "b.png", "a.png"]).is_empty());
    }
}
