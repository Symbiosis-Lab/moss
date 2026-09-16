//! Term model: authors and tags as listable dimensions.
//!
//! A *term* is a value of a term dimension — an author name from `author:`,
//! or a tag from `tags:`/inline `#tags`. Terms group pages the way folders
//! do: at build time each page's term values derive membership claims into
//! pseudo-folders (`authors/<slug>`, `tags/<slug>`), the same slot `also_in`
//! occupies, so the canonical listing selector and the synthetic folder-index
//! machinery serve term pages with zero new modes. Design:
//! `docs/archive/2026-09-01-tags-and-authors-design.md`.
//!
//! This module is the single owner of term identity: how a term name folds
//! for equality, how it slugs into a URL segment, and how a claim field
//! (`author_page:` / `tag_page:`) resolves. Pure, zero I/O — the derivation
//! pass that applies these rules to parsed documents lives in moss-build.

use serde::{Deserialize, Serialize};

/// URL namespace for author term pages (`/authors/<slug>/`).
pub const AUTHOR_NS: &str = "authors";
/// URL namespace for tag term pages (`/tags/<slug>/`).
pub const TAGS_NS: &str = "tags";

/// Case-insensitive identity key for a term name. Two names with the same
/// fold are the same term (first-seen original case is the display form) —
/// the rule inline-tag extraction has always used.
pub fn term_fold(name: &str) -> String {
    name.trim().to_lowercase()
}

/// URL segment for a term name: the standard text-to-slug rules (CJK
/// preserved, spaces to hyphens, ASCII lowercased). Distinct terms can
/// collide to one slug (`Scarly`/`scarly`); that is the merge behaviour we
/// want for case variants, and a diagnostic's job for true collisions.
///
/// Dots become hyphens before slugging: the pseudo-folder key round-trips
/// through `content_graph::generate_slug` on the render side, which treats a
/// trailing `.suffix` as a file extension and would strip it (`v1.0` → `v1`).
/// A dot-free segment passes through that path unchanged.
pub fn term_slug(name: &str) -> String {
    crate::slug::generate_slug(&name.trim().replace('.', "-"))
}

/// Pseudo-folder key for a term: `authors/<slug>` or `tags/<slug>`. This is
/// simultaneously the membership-claim string pushed beside `also_in` and the
/// URL directory of the generated term page.
pub fn term_folder_key(ns: &str, name: &str) -> String {
    format!("{}/{}", ns, term_slug(name))
}

/// Resolved form of an `author_page:` / `tag_page:` claim field: which term
/// name this page claims. `UseTitle` is the `true` form — the name is the
/// page's own title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TermClaim {
    /// `author_page: true` — claim the name equal to this page's title.
    UseTitle,
    /// `author_page: 馬欣宜` — claim this name explicitly (used when the
    /// page title differs from the term name).
    Name(String),
}

impl TermClaim {
    /// The claimed name, given the page's title for the `UseTitle` form.
    pub fn name<'a>(&'a self, title: &'a str) -> &'a str {
        match self {
            TermClaim::UseTitle => title,
            TermClaim::Name(n) => n,
        }
    }
}

/// Doc-level tag union: frontmatter tags first, then inline `#tags` not
/// already present (case-insensitive via [`term_fold`]). Deliberately NOT a
/// cascade union — scan-time cascade keeps its uniform child-overrides-folder
/// rule, and a doc with only inline tags correctly blocks folder-cascade
/// tags. Both absent stays `None` so the cascade still fills.
pub fn merge_tag_lists(
    fm_tags: Option<Vec<String>>,
    inline_tags: Vec<String>,
) -> Option<Vec<String>> {
    match (fm_tags, inline_tags.is_empty()) {
        (fm_tags, true) => fm_tags,
        (None, false) => Some(inline_tags),
        (Some(mut fm_tags), false) => {
            let present: std::collections::HashSet<String> =
                fm_tags.iter().map(|t| term_fold(t)).collect();
            fm_tags.extend(
                inline_tags
                    .into_iter()
                    .filter(|t| !present.contains(&term_fold(t))),
            );
            Some(fm_tags)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_is_trimmed_and_case_insensitive() {
        assert_eq!(term_fold(" ScarlyZ "), "scarlyz");
        assert_eq!(term_fold("馬欣宜"), "馬欣宜");
    }

    #[test]
    fn slug_preserves_cjk_and_hyphenates_spaces() {
        assert_eq!(term_slug("馬欣宜"), "馬欣宜");
        assert_eq!(term_slug("David Yang"), "david-yang");
        assert_eq!(term_slug("Web 2.0"), "web-2-0");
        assert_eq!(term_folder_key(AUTHOR_NS, "David Yang"), "authors/david-yang");
        assert_eq!(term_folder_key(TAGS_NS, "城市"), "tags/城市");
    }

    #[test]
    fn claim_name_resolves_use_title_to_the_page_title() {
        assert_eq!(TermClaim::UseTitle.name("馬欣宜"), "馬欣宜");
        assert_eq!(TermClaim::Name("Scarly".into()).name("ScarlyZ 的頁面"), "Scarly");
    }

    #[test]
    fn merge_prefers_frontmatter_and_dedupes_case_insensitively() {
        assert_eq!(
            merge_tag_lists(Some(vec!["City".into()]), vec!["city".into(), "散文".into()]),
            Some(vec!["City".into(), "散文".into()])
        );
        assert_eq!(merge_tag_lists(None, vec!["x".into()]), Some(vec!["x".into()]));
        assert_eq!(merge_tag_lists(None, vec![]), None);
        assert_eq!(merge_tag_lists(Some(vec![]), vec![]), Some(vec![]));
    }
}
