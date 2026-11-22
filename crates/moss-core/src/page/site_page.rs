//! What a whole-site pass reads about each page.
//!
//! The build's parsed document and a host's page record both implement
//! [`SitePage`], so a pass written once over the trait draws the same page
//! around an article for the build and for every host.

use crate::frontmatter_typed::SeriesField;
use crate::sort::{ResolvedSort, SortableDoc, SortableLabel};

/// The site's not-found page, served at `/404/`.
pub const NOT_FOUND_PAGE: &str = "404/index.html";

/// The per-page facts the whole-site passes read, beyond those sorting reads
/// (including the label chrome shows, [`SortableLabel`]).
///
/// Every method returns a borrow, so a pass that walks every page allocates
/// nothing per page.
pub trait SitePage: SortableDoc + SortableLabel {
    /// The layout the author wrote.
    fn layout(&self) -> Option<&str>;
    fn series(&self) -> Option<&SeriesField>;
    fn draft(&self) -> Option<bool>;
    fn listed(&self) -> Option<bool>;
    fn search_engines(&self) -> Option<bool>;
    /// A file whose content fills a slot (`footer.md`, a partial), not a page.
    fn slot_only(&self) -> bool;
    /// The hidden folder holding this page, as a url-path prefix.
    fn hidden_home(&self) -> Option<&str>;
    /// The folder's resolved sort for its direct children; `None` on a page.
    fn direct_children_sort(&self) -> Option<&ResolvedSort>;

    fn is_not_found_page(&self) -> bool {
        self.url_path() == NOT_FOUND_PAGE
    }

    /// Whether a listing of `folder` may name this page at all: false only
    /// for a page in a hidden folder that `folder` is outside of.
    fn reaches(&self, folder: &str) -> bool {
        self.hidden_home().is_none_or(|home| folder.starts_with(home))
    }

    /// Whether a listing of the folder `folder` (`a/b/`, url space; `""` for
    /// the root) shows this page. False for a draft, a page listed off-feed
    /// (`listed: false`) or kept from search engines, a slot file, the
    /// not-found page, and a page in a hidden folder `folder` is outside of.
    /// Every listing asks this, so a new visibility flag is added here once.
    fn is_listable_in(&self, folder: &str) -> bool {
        self.draft() != Some(true)
            && self.listed() != Some(false)
            && self.search_engines() != Some(false)
            && !self.slot_only()
            && !self.is_not_found_page()
            && self.reaches(folder)
    }

    /// Did the author write `layout: article` on this page? The authored
    /// intent, not the resolved template, which is also an article for every
    /// nested page that said nothing: a folder home is a step in its siblings'
    /// series only when its author called it a piece.
    fn is_article_layout(&self) -> bool {
        self.layout() == Some("article")
    }

    /// The folder's sort for its direct children, by date when none was resolved.
    fn resolve_for_direct_children(&self) -> ResolvedSort {
        self.direct_children_sort().cloned().unwrap_or(ResolvedSort {
            axis: crate::sort::SortAxis::Date,
            explicit_order: None,
            series_default: false,
        })
    }
}
