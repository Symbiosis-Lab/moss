//! Content generators for folder index pages.
//!
//! This module provides functions for generating specialized page content:
//! - Folder index pages listing articles in a folder
//! - Year-grouped article lists for homepages and indexes

use crate::build::components::{self, child_list::BuildCards, ArticleListItemProps};
use moss_core::page::card::{props_for, CardHost};
use crate::i18n::Language;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;
use moss_core::PageKind;

/// Generates a year-grouped article list for auto-index pages.
/// Uses the components module for consistent HTML generation.
///
/// # Arguments
/// * `documents` - All parsed documents
/// * `project` - Project structure with content folders
/// * `cards` - The build's card reader host: links, typesetting (Chinese
///   numerals in year headings and month prefixes) and the clock
/// * `minimal` - If true, renders with minimal styling (no underlines)
/// * `lang` - Language for UI strings
pub fn generate_year_grouped_article_list(
    documents: &[ParsedDocument],
    project: &ProjectStructure,
    cards: &BuildCards<'_>,
    minimal: bool,
    lang: Language,
) -> String {
    let articles: Vec<ArticleListItemProps> = documents
        .iter()
        .filter(|doc| {
            if doc.url_path == "index.html" { return false; }
            // Listing chrome — exclude draft / slot-only via the
            // canonical predicate (an earlier change routed `footer.md` etc.
            // through the documents slice for layout-slot wiring; the
            // `slot_only` arm of `is_listable` keeps them out of articles).
            if !doc.is_listable() { return false; }
            // A folder is navigation, not an article. Ask the content model,
            // the same way `moss_core::page::card::count_listable_articles` and
            // `select_children_by_slug` do. The structural test this replaced
            // ("does any other document live under this prefix?") called a
            // childless folder an article — which is how a synthetic index for
            // a directory of images ended up in the listing with the literal
            // date `Unknown`.
            if doc.kind != PageKind::Article { return false; }
            // Filter out nav items (they appear in navigation, not article list)
            if moss_core::page::listing::is_listing_nav_item(*doc, project.has_content_folders) {
                return false;
            }
            true
        })
        .map(|doc| ArticleListItemProps::from(&props_for(doc, documents, cards, None)))
        .collect();

    // This flattened, site-wide list has no single folder whose `sort:` could
    // apply — always newest-first.
    components::render_year_grouped_list(&articles, minimal, lang, cards.typesetting(), false)
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
