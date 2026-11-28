//! `series: true` on each folder home whose pages lost their previous and next
//! links in moss 0.15.30, when an order alone stopped drawing them.
//!
//! Which folders those are is the build's decision
//! ([`unlinked_folders`](crate::build::scan::sort_inference::unlinked_folders)),
//! read from a build of the site, so a `sort:` or `series:` set by a
//! `cascade:` counts here as it does there.

use moss_core::frontmatter::{edit_fields, FieldEdit};

use super::{FileChange, Skipped};
use crate::build::scan::sort_inference::UnlinkedFolder;

/// The edits that give each of `unlinked`'s homes `series: true`, and the
/// folders that cannot take one. `pages` holds site-relative paths and texts.
pub fn plan(pages: &[(String, String)], unlinked: &[UnlinkedFolder]) -> (Vec<FileChange>, Vec<Skipped>) {
    let (mut changes, mut skipped) = (Vec::new(), Vec::new());
    for folder in unlinked {
        let home = folder.home.as_deref().and_then(|home| pages.iter().find(|(rel, _)| rel == home));
        let Some((path, text)) = home else {
            let reason = "series: true not added: the folder has no home page; add one that sets it to keep the links".to_string();
            skipped.push(Skipped { path: folder.folder.clone(), line: None, reason });
            continue;
        };
        match edit_fields(text, &[FieldEdit::Set { key: "series".to_string(), value: serde_yaml::Value::Bool(true) }]) {
            Ok(after) => changes.push(FileChange { path: path.clone(), before: text.clone(), after }),
            Err(reason) => skipped.push(Skipped { path: path.clone(), line: None, reason: format!("series: true not added: {reason}") }),
        }
    }
    (changes, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlinked(folder: &str, home: Option<&str>) -> UnlinkedFolder {
        UnlinkedFolder { folder: folder.to_string(), home: home.map(str::to_string) }
    }

    #[test]
    fn each_named_home_gets_series_true_and_a_folder_without_one_is_skipped() {
        let pages = [("Journal/Journal.md".to_string(), "---\ntitle: t\n---\nbody\n".to_string())];
        let (changes, skipped) = plan(&pages, &[unlinked("Journal", Some("Journal/Journal.md")), unlinked("Notes", None)]);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].after, "---\ntitle: t\nseries: true\n---\nbody\n");
        assert_eq!(skipped.len(), 1, "{skipped:?}");
        assert_eq!(skipped[0].path, "Notes");
    }
}
