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
//! `is_index: bool` and inferring the rest from context made the
//! children-listing filter impossible to get right.

use serde::{Deserialize, Serialize};

/// File extensions, lowercase, that the build turns into pages, `md` first.
/// The resolver tries them in this order when a target names no extension.
pub const PAGE_EXTENSIONS: [&str; 4] = ["md", "markdown", "mdown", "mkd"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// Markdown file, not a folder index.
    Article,
    /// Markdown file recognized as a folder index by
    /// [`crate::home::is_home_file`].
    Folder,
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
    fn default_is_article() {
        assert_eq!(PageKind::default(), PageKind::Article);
    }
}
