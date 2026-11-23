//! What a whole-site pass reads about each page.
//!
//! The build's parsed document and a host's page record both implement
//! [`SitePage`], so a pass written once over the trait draws the same page
//! around an article for the build and for every host.

use crate::frontmatter_typed::SeriesField;
use crate::sort::{ResolvedSort, SortField, SortableDoc, SortableLabel};

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

/// Every fact a whole-site pass reads about one page, as owned values. A host
/// builds these from the site's texts; the build's documents produce the same
/// through [`PageFacts::from_page`], so a test can hold the two equal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PageFacts {
    pub url_path: String,
    /// The event `start` if the page has one, else its `date`.
    pub date: Option<String>,
    pub weight: Option<i32>,
    pub declared_sort: Option<SortField>,
    pub clean_stem: String,
    pub is_folder_index: bool,
    pub order_match_name: String,
    pub label: String,
    pub layout: Option<String>,
    pub series: Option<SeriesField>,
    pub draft: Option<bool>,
    pub listed: Option<bool>,
    pub search_engines: Option<bool>,
    pub slot_only: bool,
    pub hidden_home: Option<String>,
    pub direct_children_sort: Option<ResolvedSort>,
}

impl PageFacts {
    pub fn from_page<P: SitePage + ?Sized>(page: &P) -> Self {
        Self {
            url_path: page.url_path().to_string(),
            date: page.date().map(str::to_string),
            weight: page.weight(),
            declared_sort: page.declared_sort().cloned(),
            clean_stem: page.clean_stem().to_string(),
            is_folder_index: page.is_folder_index(),
            order_match_name: page.order_match_name().to_string(),
            label: page.label().to_string(),
            layout: page.layout().map(str::to_string),
            series: page.series().cloned(),
            draft: page.draft(),
            listed: page.listed(),
            search_engines: page.search_engines(),
            slot_only: page.slot_only(),
            hidden_home: page.hidden_home().map(str::to_string),
            direct_children_sort: page.direct_children_sort().cloned(),
        }
    }
}

impl SortableDoc for PageFacts {
    fn url_path(&self) -> &str { &self.url_path }
    fn date(&self) -> Option<&str> { self.date.as_deref() }
    fn weight(&self) -> Option<i32> { self.weight }
    fn declared_sort(&self) -> Option<&SortField> { self.declared_sort.as_ref() }
    fn clean_stem(&self) -> &str { &self.clean_stem }
    fn is_folder_index(&self) -> bool { self.is_folder_index }
    fn order_match_name(&self) -> &str { &self.order_match_name }
}

impl SortableLabel for PageFacts {
    fn label(&self) -> &str { &self.label }
}

impl SitePage for PageFacts {
    fn layout(&self) -> Option<&str> { self.layout.as_deref() }
    fn series(&self) -> Option<&SeriesField> { self.series.as_ref() }
    fn draft(&self) -> Option<bool> { self.draft }
    fn listed(&self) -> Option<bool> { self.listed }
    fn search_engines(&self) -> Option<bool> { self.search_engines }
    fn slot_only(&self) -> bool { self.slot_only }
    fn hidden_home(&self) -> Option<&str> { self.hidden_home.as_deref() }
    fn direct_children_sort(&self) -> Option<&ResolvedSort> { self.direct_children_sort.as_ref() }
}
