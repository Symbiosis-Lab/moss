//! Translation linking for multilingual documents.
//!
//! Groups documents by shared stem (directory + basename without language suffix)
//! and by `translationKey` frontmatter to create translation links between pages.

/// A link to a translated version of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationLink {
    /// The translated document's DECLARED BCP-47 tag — what its own page
    /// emits as `<html lang>`, and the only language this link carries.
    ///
    /// It used to carry moss's three-variant UI enum as well, which collapsed
    /// every language moss has no interface for onto one of the three: a `fr`
    /// page was advertised as `hreflang="en"` while its own `<html lang>` said
    /// `fr`, and two pages in one unshipped language became a single alternate.
    pub lang_tag: String,
    /// URL path to the translated document
    pub url_path: String,
    /// Display name for the language (e.g., "EN", "简", "繁")
    pub display_name: &'static str,
}

/// Lightweight info needed from a document for translation linking.
pub struct DocumentInfo {
    /// Index into the original documents array
    pub index: usize,
    /// Clean stem without language suffix (e.g., "my-trip" from "my-trip.zh-hans.md")
    pub clean_stem: String,
    /// Directory containing the document (e.g., "posts")
    pub directory: String,
    /// Document's declared BCP-47 tag — see [`TranslationLink::lang_tag`].
    pub lang_tag: String,
    /// URL path for the generated page
    pub url_path: String,
    /// Optional translationKey from frontmatter
    pub translation_key: Option<String>,
}

/// Build translation links for a set of documents.
///
/// Groups documents by:
/// 1. `(directory, clean_stem)` — files sharing the same base name in the same folder
/// 2. `translationKey` frontmatter — arbitrary cross-linking
///
/// Returns a Vec of translation link lists, indexed by document position.
/// Each entry contains links to OTHER translations (not the document itself).
pub fn build_translation_links(docs: &[DocumentInfo]) -> Vec<Vec<TranslationLink>> {
    use std::collections::HashMap;

    let mut result: Vec<Vec<TranslationLink>> = vec![vec![]; docs.len()];

    // Group by (directory, clean_stem)
    let mut stem_groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for doc in docs {
        let key = (doc.directory.clone(), doc.clean_stem.clone());
        stem_groups.entry(key).or_default().push(doc.index);
    }

    // Group by translationKey
    let mut key_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for doc in docs {
        if let Some(ref tkey) = doc.translation_key {
            key_groups.entry(tkey.clone()).or_default().push(doc.index);
        }
    }

    // Merge groups: combine stem and key groups, dedup
    let mut all_groups: Vec<Vec<usize>> = Vec::new();
    for (_, indices) in stem_groups {
        if indices.len() > 1 {
            all_groups.push(indices);
        }
    }
    for (_, indices) in key_groups {
        if indices.len() > 1 {
            all_groups.push(indices);
        }
    }

    // For each group, create cross-links
    for group in &all_groups {
        for &doc_idx in group {
            for &other_idx in group {
                if doc_idx == other_idx {
                    continue;
                }
                let other = &docs[other_idx];
                let link = TranslationLink {
                    lang_tag: other.lang_tag.clone(),
                    url_path: other.url_path.clone(),
                    // Labeled from the TAG. Going through the enum here showed
                    // "EN" on an authored `fr` translation, next to an
                    // `hreflang="fr"` on the same anchor.
                    display_name: crate::i18n::switcher_label(&other.lang_tag),
                };
                // Avoid duplicates (same url_path)
                if !result[doc_idx].iter().any(|l| l.url_path == link.url_path) {
                    result[doc_idx].push(link);
                }
            }
        }
    }

    // Sort each link list by url_path for deterministic output. `all_groups` is
    // built by draining two HashMaps, so a doc that lands in more than one group
    // (≥2 translations) would otherwise receive its links in HashMap-iteration
    // order — non-deterministic run-to-run. Single-translation pages are
    // unaffected (one element can't reorder).
    for links in &mut result {
        links.sort_by(|a, b| a.url_path.cmp(&b.url_path));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_doc_info(
        index: usize,
        clean_stem: &str,
        directory: &str,
        lang_tag: &str,
        url_path: &str,
        translation_key: Option<&str>,
    ) -> DocumentInfo {
        DocumentInfo {
            index,
            clean_stem: clean_stem.to_string(),
            directory: directory.to_string(),
            lang_tag: lang_tag.to_string(),
            url_path: url_path.to_string(),
            translation_key: translation_key.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_no_translations_single_doc() {
        let docs = vec![make_doc_info(0, "about", "", "en", "about.html", None)];
        let links = build_translation_links(&docs);
        assert!(links[0].is_empty());
    }

    #[test]
    fn test_stem_based_linking() {
        let docs = vec![
            make_doc_info(0, "my-trip", "posts", "en", "posts/my-trip.html", None),
            make_doc_info(1, "my-trip", "posts", "zh-Hans", "posts/my-trip.zh-hans.html", None),
        ];
        let links = build_translation_links(&docs);

        assert_eq!(links[0].len(), 1);
        assert_eq!(links[0][0].lang_tag, "zh-Hans");
        assert_eq!(links[0][0].url_path, "posts/my-trip.zh-hans.html");
        assert_eq!(links[0][0].display_name, "简");

        assert_eq!(links[1].len(), 1);
        assert_eq!(links[1][0].lang_tag, "en");
        assert_eq!(links[1][0].url_path, "posts/my-trip.html");
    }

    #[test]
    fn test_three_languages() {
        let docs = vec![
            make_doc_info(0, "about", "", "en", "about.html", None),
            make_doc_info(1, "about", "", "zh-Hans", "about.zh-hans.html", None),
            make_doc_info(2, "about", "", "zh-Hant", "about.zh-hant.html", None),
        ];
        let links = build_translation_links(&docs);

        assert_eq!(links[0].len(), 2);
        assert_eq!(links[1].len(), 2);
        assert_eq!(links[2].len(), 2);
    }

    #[test]
    fn test_translation_key_linking() {
        let docs = vec![
            make_doc_info(0, "hello-world", "posts", "en", "posts/hello-world.html", Some("hello")),
            make_doc_info(1, "ni-hao", "posts", "zh-Hans", "posts/ni-hao.html", Some("hello")),
        ];
        let links = build_translation_links(&docs);

        assert_eq!(links[0].len(), 1);
        assert_eq!(links[0][0].lang_tag, "zh-Hans");
        assert_eq!(links[0][0].url_path, "posts/ni-hao.html");

        assert_eq!(links[1].len(), 1);
        assert_eq!(links[1][0].lang_tag, "en");
    }

    #[test]
    fn test_different_directories_not_linked() {
        let docs = vec![
            make_doc_info(0, "about", "blog", "en", "blog/about.html", None),
            make_doc_info(1, "about", "docs", "zh-Hans", "docs/about.html", None),
        ];
        let links = build_translation_links(&docs);

        assert!(links[0].is_empty());
        assert!(links[1].is_empty());
    }

    #[test]
    fn a_siblings_declared_tag_reaches_the_link_not_its_ui_variant() {
        // Both docs resolve to `"en"` — the enum has no `fr` — so
        // without `lang_tag` the link back to the French page carried `en`,
        // which is not what that page's own `<html lang>` says.
        let mut fr = make_doc_info(1, "about", "", "en", "about.fr.html", None);
        fr.lang_tag = "fr".to_string();
        let docs = vec![make_doc_info(0, "about", "", "en", "about.html", None), fr];
        let links = build_translation_links(&docs);
        assert_eq!(links[0][0].lang_tag, "fr");
        assert_eq!(links[1][0].lang_tag, "en");
    }

    #[test]
    fn test_no_duplicate_links() {
        // Same pair linked by both stem and translationKey
        let docs = vec![
            make_doc_info(0, "about", "", "en", "about.html", Some("about-page")),
            make_doc_info(1, "about", "", "zh-Hans", "about.zh-hans.html", Some("about-page")),
        ];
        let links = build_translation_links(&docs);

        // Should only have 1 link each, not 2
        assert_eq!(links[0].len(), 1);
        assert_eq!(links[1].len(), 1);
    }

}
