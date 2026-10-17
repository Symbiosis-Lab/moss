//! A `translationKey` that only restates what the page's path already says.
//!
//! A page and its translation pair by their path inside the language tree
//! (`en/about.md` and `de/about.md`); a shared `translationKey` pairs pages
//! whatever they are called. When the pages sharing a key are exactly the
//! pages sharing one path, one per language, the key adds nothing, and moss
//! offers to remove it. Whether it really adds nothing is settled by building
//! the site with and without it (see `migrate.rs`), not by this module.

use std::collections::BTreeMap;

use moss_core::frontmatter::{edit_fields, frontmatter_map, value_as_string, FieldEdit};

use super::{FileChange, Skipped};

const KEYS: [&str; 2] = ["translationKey", "translation_key"];

/// The pages whose `translationKey` the path already implies, each with the
/// key removed, and the groups left whole because one page could not be
/// edited. `pages` holds site-relative paths and file texts.
///
/// This is a cheaper approximation of the build's pairing; the build
/// comparison in `migrate.rs` is what decides.
pub fn redundant(pages: &[(String, String)]) -> (Vec<FileChange>, Vec<Skipped>) {
    let within = |rel: &str| crate::build::footer::language_tree_split(rel).1.to_string();
    let mut groups: BTreeMap<String, Vec<&(String, String)>> = BTreeMap::new();
    let mut per_path: BTreeMap<String, usize> = BTreeMap::new();
    for page in pages {
        *per_path.entry(within(&page.0)).or_insert(0) += 1;
        let fields = frontmatter_map(&page.1);
        if let Some(key) = KEYS.iter().find_map(|k| fields.get(*k).and_then(value_as_string)) {
            groups.entry(key).or_default().push(page);
        }
    }

    let (mut out, mut skipped) = (Vec::new(), Vec::new());
    for (key, members) in groups {
        let path = within(&members[0].0);
        let languages: std::collections::BTreeSet<_> = members.iter().map(|(rel, _)| crate::build::footer::language_tree_split(rel).0).collect();
        let pairs_by_path = members.len() >= 2
            && key != "home"
            && languages.len() == members.len()
            && members.iter().all(|(rel, _)| within(rel) == path)
            && per_path.get(&path) == Some(&members.len());
        if !pairs_by_path {
            continue;
        }
        let edits: Vec<FieldEdit> = KEYS.iter().map(|k| FieldEdit::Remove { key: k.to_string() }).collect();
        let edited: Result<Vec<FileChange>, Skipped> = members
            .iter()
            .map(|(rel, content)| {
                edit_fields(content, &edits)
                    .map(|after| FileChange { path: rel.clone(), before: content.clone(), after })
                    .map_err(|reason| Skipped { path: rel.clone(), line: None, reason: format!("translationKey: {key} kept on every page that has it: {reason}") })
            })
            .collect();
        match edited {
            Ok(changes) => out.extend(changes),
            Err(why) => skipped.push(why),
        }
    }
    (out, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(rel: &str, key: Option<&str>) -> (String, String) {
        let fm = key.map_or(String::new(), |k| format!("translationKey: {k}\n"));
        (rel.to_string(), format!("---\ntitle: t\n{fm}---\nbody\n"))
    }

    #[test]
    fn a_key_on_the_same_path_in_each_language_is_redundant() {
        let pages = [page("en/about.md", Some("about")), page("de/about.md", Some("about")), page("en/news.md", None)];
        let (changes, _) = redundant(&pages);
        assert_eq!(changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["en/about.md", "de/about.md"]);
        assert_eq!(changes[0].after, "---\ntitle: t\n---\nbody\n");
    }

    #[test]
    fn a_key_that_pairs_different_paths_is_kept() {
        assert!(redundant(&[page("en/about.md", Some("about")), page("de/ueber-uns.md", Some("about"))]).0.is_empty());
    }

    #[test]
    fn a_key_missing_from_a_page_that_would_pair_by_path_is_kept() {
        let pages = [page("en/about.md", Some("about")), page("de/about.md", Some("about")), page("fr/about.md", None)];
        assert!(redundant(&pages).0.is_empty());
    }

    #[test]
    fn the_home_key_and_a_lone_page_are_kept() {
        assert!(redundant(&[page("en/index.md", Some("home")), page("de/index.md", Some("home"))]).0.is_empty());
        assert!(redundant(&[page("en/about.md", Some("about"))]).0.is_empty());
    }

    #[test]
    fn a_group_with_a_page_that_cannot_be_edited_is_left_whole() {
        let simplified = ("de/about.md".to_string(), "title: t\ntranslationKey: about\n---\nbody\n".to_string());
        let (changes, skipped) = redundant(&[page("en/about.md", Some("about")), simplified]);
        assert!(changes.is_empty(), "{changes:?}");
        assert_eq!(skipped.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), vec!["de/about.md"]);
    }
}
