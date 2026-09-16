//! Classification of moss page/source-file kinds.
//!
//! moss recognizes exactly three kinds of page-producing entities in a
//! source tree. This enum is the canonical way to tell them apart — do
//! NOT use `is_index`, filename checks, or extension sniffing at call
//! sites. The kind is set once at ingestion (for markdown files) or
//! synthesis (for asset pages) and read everywhere else.
//!
//! # Variants
//!
//! * [`PageKind::Article`] — a markdown file that is not a folder index.
//!   Authored prose content. Listed in children at any depth.
//! * [`PageKind::Folder`] — a markdown file recognized by
//!   [`crate::home::is_home_file`]. Represents the directory it lives in
//!   (e.g. `文字/文字.md`, `docs/index.md`, `blog/readme.md`). Listed in
//!   children only at depth 1 (at depth `all`, its descendants are listed
//!   directly).
//!
//! # Why an enum, not a bool
//!
//! The previous model used `is_index: bool` and inferred the rest from
//! context. That made the children-listing filter impossible to get right.
//! A third `Asset` variant (synthetic per-image pages) existed until the
//! image-as-page feature was removed in 2026-07; nothing produced it, so it
//! was dropped rather than left as an unreachable state. See
//! `moss/docs/reference/page-kinds.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// Markdown file, not a folder index.
    Article,
    /// Markdown file recognized as a folder index by
    /// [`crate::home::is_home_file`].
    Folder,
}

impl PageKind {
    /// True when pages of this kind appear in children listings at
    /// `children_depth: all`.
    pub fn is_listable_at_depth_all(self) -> bool {
        matches!(self, PageKind::Article)
    }

    /// True when pages of this kind appear in children listings at
    /// `children_depth: direct`.
    pub fn is_listable_at_depth_direct(self) -> bool {
        matches!(self, PageKind::Article | PageKind::Folder)
    }
}

impl Default for PageKind {
    fn default() -> Self {
        PageKind::Article
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listable_at_depth_all_is_article_only() {
        assert!(PageKind::Article.is_listable_at_depth_all());
        assert!(!PageKind::Folder.is_listable_at_depth_all());
    }

    #[test]
    fn listable_at_depth_direct_is_article_and_folder() {
        assert!(PageKind::Article.is_listable_at_depth_direct());
        assert!(PageKind::Folder.is_listable_at_depth_direct());
    }

    #[test]
    fn default_is_article() {
        assert_eq!(PageKind::default(), PageKind::Article);
    }
}
