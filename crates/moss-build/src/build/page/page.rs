//! Content generators for folder index pages.
//!
//! This module provides functions for generating specialized page content:
//! - Folder index pages listing articles in a folder
//! - Year-grouped article lists for homepages and indexes

use crate::build::scan::article_map;
use crate::build::components::{self, extract_date_from_doc, ArticleListItemProps};
use crate::i18n::Language;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;
use moss_core::PageKind;

/// Count the ARTICLES a reader will actually find under `prefix`, subfolders
/// included, excluding the document at `self_url`.
///
/// This is the number a folder card renders as "N articles" / "N 篇" — always
/// every descendant article, independent of what the folder's own page lists
/// when opened (`direct` vs `all` per `children_depth`; see
/// `child_list::props_for_document`). Both callers of this function (the card
/// and the incremental-listing digest that has to re-fire when the card's
/// count would change) want exactly that number, so there is no direct-only
/// mode to select — a `flatten` parameter existed here until it was always
/// `true` at every call site.
///
/// They used to be computed by DIFFERENT rules entirely: the listing asked
/// `PageKind` (`is_listable_at_depth_*`) while this counted `!= Folder`, which
/// silently counted synthetic asset pages (a folder of 83 illustrations
/// advertised "82 篇") and dropped real sub-sections (a folder with one
/// sub-section advertised "0 articles").
///
/// * **Kind** — only [`PageKind::Article`] counts. A folder note is navigation,
///   not an article; assets are never listed at all. Kind eligibility is the
///   content-model election owned by `moss_core::page_kind` — read it, never
///   re-derive it from paths or filenames.
/// * **Visibility** — `is_listable()` excludes drafts, `listed: false` and
///   slot-only chrome, same as every other listing surface (`folder_embed.rs`).
///
/// Accepts both `&[ParsedDocument]` and `&[&ParsedDocument]` via `std::borrow::Borrow`.
pub(crate) fn count_listable_articles<D: std::borrow::Borrow<ParsedDocument>>(
    docs: &[D],
    prefix: &str,
    self_url: &str,
) -> usize {
    docs.iter()
        .map(|d| d.borrow())
        .filter(|d| {
            d.url_path != self_url
                && d.url_path.starts_with(prefix)
                && d.kind == moss_core::PageKind::Article
                && d.is_listable()
        })
        .count()
}

/// Generates a year-grouped article list for auto-index pages.
/// Uses the components module for consistent HTML generation.
///
/// # Arguments
/// * `documents` - All parsed documents
/// * `project` - Project structure with content folders
/// * `minimal` - If true, renders with minimal styling (no underlines)
/// * `lang` - Language for UI strings
/// * `typesetting` - The page's resolved typesetting, which decides whether the
///   year headings and month prefixes are written in Chinese numerals
pub fn generate_year_grouped_article_list(
    documents: &[ParsedDocument],
    project: &ProjectStructure,
    minimal: bool,
    lang: Language,
    typesetting: Option<&str>,
) -> String {
    let articles: Vec<ArticleListItemProps> = documents
        .iter()
        .filter(|doc| {
            if doc.url_path == "index.html" { return false; }
            // Listing chrome — exclude draft / slot-only via the
            // canonical predicate (PR7b/moss#599 routed `footer.md` etc.
            // through the documents slice for layout-slot wiring; the
            // `slot_only` arm of `is_listable` keeps them out of articles).
            if !doc.is_listable() { return false; }
            // A folder is navigation, not an article. Ask the content model,
            // the same way `count_listable_articles` above and
            // `select_children_by_slug` do. The structural test this replaced
            // ("does any other document live under this prefix?") called a
            // childless folder an article — which is how a synthetic index for
            // a directory of images ended up in the listing with the literal
            // date `Unknown`.
            if doc.kind != PageKind::Article { return false; }
            // Filter out nav items (they appear in navigation, not article list)
            if crate::build::components::nav::is_listing_nav_item(doc, project.has_content_folders) {
                return false;
            }
            true
        })
        .map(|doc| {
            let (date_display, date_raw, _explicit) = extract_date_from_doc(doc, &project.root_path);
            ArticleListItemProps {
                date_display,
                date_raw,
                url: article_map::to_pretty_url(&doc.url_path),
                // Article listing is chrome; use the plain-text label.
                title: doc.label.clone(),
                url_path: doc.url_path.clone(),
            }
        })
        .collect();

    components::render_year_grouped_list(&articles, minimal, lang, typesetting)
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
