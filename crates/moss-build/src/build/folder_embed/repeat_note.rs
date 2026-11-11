//! The page a listing is drawn on, and the note it gets when its own embeds
//! and its automatic child listing show the same pages.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use super::inline_body::IdScopes;
use crate::build::types::ParsedDocument;

/// The page the listings being drawn sit on, and what they have drawn on it.
pub(crate) struct ListingHost {
    pub url_path: String,
    /// Its language tag, to mark a row in another language.
    pub lang_tag: String,
    /// The pages its listings show.
    pub urls: RefCell<BTreeSet<String>>,
    /// Whether one of them is a `style:stream` listing, and the pages it draws whole.
    pub stream: Cell<bool>,
    pub streamed: RefCell<BTreeSet<String>>,
    /// The id prefixes the bodies drawn whole on it took.
    pub id_scopes: RefCell<IdScopes>,
}

impl ListingHost {
    pub(crate) fn of(doc: &ParsedDocument) -> Self {
        let own_html = format!("{}{}", doc.hero_html.as_deref().unwrap_or_default(), doc.html_content);
        Self {
            url_path: doc.url_path.clone(),
            lang_tag: doc.lang_tag.clone().unwrap_or_else(|| doc.lang.as_bcp47_attr().to_string()),
            urls: Default::default(),
            stream: Cell::new(false),
            streamed: Default::default(),
            id_scopes: RefCell::new(IdScopes::around(&own_html)),
        }
    }
}

/// `children` is what the page's child listing shows.
pub(crate) fn note_repeated_child_listing(doc: &ParsedDocument, children: &ListingHost) {
    let repeated = children.urls.borrow().intersection(&doc.embedded_listing_urls).count();
    if repeated > 0 {
        let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
        let noun = if repeated == 1 { "page is" } else { "pages are" };
        crate::build::cli_output::log_note!(
            "[{page}] {repeated} {noun} listed both by this page's embeds and by its automatic child listing below them; set `children: false` in its frontmatter to keep only the embeds."
        );
    }
}
