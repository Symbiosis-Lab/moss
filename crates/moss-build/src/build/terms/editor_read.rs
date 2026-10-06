//! Every term the last build derived, grouped by kind, for the editor's name
//! chip completion. Split out of `terms.rs` (sibling-file pattern) so the
//! editor-facing readout of the finished [`super::TermKind`]/`ArticleMap`
//! tables stays apart from the derivation pass that produces them — this file
//! only reads what a build already wrote, never derives anything itself.
//!
//! Two carriers reach this: the desktop Tauri command and the HTTP editor
//! carrier both call [`list_vault_terms_in`] with the article map they load
//! for their own session, so the two answers cannot drift.

use std::collections::BTreeMap;

use crate::build::scan::article_map::ArticleMap;

/// Every term in the vault, by kind, for chip completion. Display forms as
/// the build recorded them, in key order. `fields` is the kind's own field
/// list, so the caller can map the field a chip speaks (`editor`) to the
/// names it should complete against — every field of one kind shares one
/// name list.
#[derive(Debug, Default, serde::Serialize, specta::Type)]
pub struct VaultTerms {
    pub kinds: BTreeMap<String, KindTerms>,
}

/// One kind's fields and the display names the last build derived for it.
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct KindTerms {
    pub fields: Vec<String>,
    pub names: Vec<String>,
}

/// Every term the last build derived, grouped by kind. A kind nothing feeds
/// lists nothing; a vault with no build lists no kinds at all.
pub fn list_vault_terms_in(map: &ArticleMap) -> VaultTerms {
    let mut kinds = BTreeMap::new();
    for kind in &map.kinds {
        let prefix = format!("{}/", kind.key);
        let names = map
            .terms
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(_, rec)| rec.display.clone())
            .collect();
        kinds.insert(
            kind.key.clone(),
            KindTerms { fields: kind.fields.clone(), names },
        );
    }
    VaultTerms { kinds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::scan::article_map::ArticleInfo;
    use crate::build::terms::{TermKind, TermSite};

    fn article(source_path: &str, url_path: &str) -> ArticleInfo {
        ArticleInfo {
            source_path: source_path.into(),
            title: String::new(),
            content: String::new(),
            html_content: None,
            frontmatter: Default::default(),
            url_path: url_path.into(),
            date: None,
            tags: vec![],
            uid: None,
        }
    }

    fn kind(key: &str, fields: &[&str]) -> TermKind {
        TermKind {
            key: key.to_string(),
            fields: fields.iter().map(|f| f.to_string()).collect(),
            title: key.to_string(),
            is_place: false,
            parents: Default::default(), explorer: None, line: None,
        }
    }

    fn map_with(kinds: Vec<TermKind>, terms: &[(&str, &str, Option<&str>)]) -> ArticleMap {
        let mut map = ArticleMap::new();
        map.kinds = kinds;
        map.articles.insert("about/ma/".into(), article("about/ma.md", "about/ma/"));
        for (key, display, claimed_by) in terms {
            map.terms.insert(
                key.to_string(),
                TermSite {
                    display: display.to_string(),
                    claimed_by: claimed_by.map(String::from),
                    parent: None,
                },
            );
        }
        map
    }

    #[test]
    fn vault_terms_group_by_kind_in_key_order() {
        let map = map_with(
            vec![kind("people", &["author", "editor"]), kind("tags", &["tags"])],
            &[
                ("tags/city", "City", None),
                ("people/scarly", "Scarly", None),
                ("tags/prose", "Prose", Some("x/")),
            ],
        );
        let t = list_vault_terms_in(&map);
        assert_eq!(t.kinds["people"].fields, vec!["author", "editor"]);
        assert_eq!(t.kinds["people"].names, vec!["Scarly"]);
        assert_eq!(t.kinds["tags"].names, vec!["City", "Prose"]);
    }

    #[test]
    fn a_kind_nothing_feeds_lists_no_names() {
        let map = map_with(vec![kind("people", &["author"])], &[]);
        let t = list_vault_terms_in(&map);
        assert_eq!(t.kinds["people"].names, Vec::<String>::new());
    }

    #[test]
    fn no_build_lists_no_kinds() {
        let map = ArticleMap::new();
        assert!(list_vault_terms_in(&map).kinds.is_empty());
    }
}
