//! The note a page gets when its own embeds and its automatic child listing show the same pages.

use crate::build::types::ParsedDocument;

/// The URL paths of the pages a listing shows.
pub(crate) type ListedUrls = std::cell::RefCell<std::collections::BTreeSet<String>>;

/// `children` is what the page's child listing shows.
pub(crate) fn note_repeated_child_listing(doc: &ParsedDocument, children: &ListedUrls) {
    let repeated = children.borrow().intersection(&doc.embedded_listing_urls).count();
    if repeated > 0 {
        let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
        let noun = if repeated == 1 { "page is" } else { "pages are" };
        crate::build::cli_output::log_note!(
            "[{page}] {repeated} {noun} listed both by this page's embeds and by its automatic child listing below them; set `children: false` in its frontmatter to keep only the embeds."
        );
    }
}
