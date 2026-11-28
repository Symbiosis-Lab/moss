//! What a listing reads from a build's documents to draw its cards: a row's
//! language, a card's subtitle and text, and [`BuildCards`], the build's
//! [`CardHost`](moss_core::page::card::CardHost). The rows themselves are
//! drawn in `moss_core::render::cards`.

use crate::build::assets::paths::PathResolver;
use crate::build::types::ParsedDocument;

pub use moss_core::page::card::{root_attrs, source_file_attr, ChildItemProps, RowLang, EVENT_SEP};
pub use moss_core::render::cards::row::ArticleListItemProps;
pub(crate) use moss_core::render::cards::link_or_box;

/// Set when `doc` is written in another language than the page listing it.
pub(crate) fn row_lang(doc: &ParsedDocument, listing_tag: &str, ui: crate::i18n::Language) -> Option<RowLang> {
    let tag = doc.lang_tag.as_deref().unwrap_or_else(|| doc.lang.as_bcp47_attr());
    (!tag.eq_ignore_ascii_case(listing_tag)).then(|| RowLang {
        tag: tag.to_string(),
        name: crate::i18n::language_name(ui, tag),
        listing_tag: listing_tag.to_string(),
    })
}

/// The page's `subtitle:` frontmatter, trimmed; `None` when absent or blank.
pub(crate) fn card_subtitle(doc: &ParsedDocument) -> Option<String> {
    moss_core::page::card::trimmed(doc.raw_frontmatter.get("subtitle").and_then(|v| v.as_str()))
}

/// The text a card shows when the author wrote no excerpt: the frontmatter
/// `description:`, else the page's first `byline:` row. The folder listing's
/// excerpt fallback and the incremental listing hash read it too, so a card's
/// text is chosen here and nowhere else.
pub(crate) fn explicit_card_text(doc: &ParsedDocument) -> Option<String> {
    moss_core::page::card::card_text(doc.description.as_deref(), doc.byline.first().map(String::as_str))
}

/// What a card reads from the build beyond the page's own facts: the path
/// resolver with the site's directory overrides, and the other passes' results
/// on the document (a term's card, places, credits, section, body pictures).
pub(crate) struct BuildCards<'a> {
    resolver: PathResolver,
    typesetting: Option<&'a str>,
    now: String,
}

impl<'a> BuildCards<'a> {
    pub(crate) fn new(dir_overrides: &std::collections::HashMap<String, String>, typesetting: Option<&'a str>, now: chrono::NaiveDateTime) -> Self {
        Self { resolver: PathResolver::new().with_dir_overrides(dir_overrides.clone()), typesetting, now: crate::build::folder_embed::upcoming::minute(now) }
    }
}

impl moss_core::page::card::CardHost<ParsedDocument> for BuildCards<'_> {
    fn resolve_url(&self, path: &str) -> String { self.resolver.resolve_url(path) }
    fn typesetting(&self) -> Option<&str> { self.typesetting }
    fn now(&self) -> &str { &self.now }
    fn term_card(&self, doc: &ParsedDocument) -> Option<moss_core::page::card::TermCardFacts> {
        doc.term_card.clone()
    }
    fn body_cover(&self, doc: &ParsedDocument) -> Option<String> { doc.card_cover.clone() }
    fn place_names(&self, doc: &ParsedDocument) -> Option<String> { doc.place_names.clone() }
    fn credits(&self, doc: &ParsedDocument) -> Option<String> { doc.card_extras.clone() }
    fn section(&self, doc: &ParsedDocument) -> Option<crate::build::types::SectionAccent> { doc.section.clone() }
    fn pictures_after_cover(&self, doc: &ParsedDocument, n: usize) -> Vec<String> {
        moss_core::page::card_pictures::card_pictures_after_cover(doc.listing_cover(), doc.hero_image_url.as_deref(), &doc.body_cover_candidates, n)
    }
}

#[cfg(test)]
#[path = "child_list_tests.rs"]
mod tests;
