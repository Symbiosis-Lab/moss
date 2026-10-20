//! The pages of one term in different editions are translations of each other:
//! `/partners/ada-example/` and `/de/partners/ada-example/` switch to one
//! another and advertise each other as `hreflang` alternates, as paired pages do.

use std::collections::BTreeMap;

use super::{TermIndex, TermKey};
use crate::build::types::ParsedDocument;
use crate::i18n::link::TranslationLink;

/// The term (or namespace root) a document is the page of, as the key of the
/// default edition's page for it: a claiming page carries its term, a
/// generated or root page sits at the key.
fn term_identity<'a>(doc: &'a ParsedDocument, index: &'a TermIndex) -> Option<&'a str> {
    if let Some(key) = doc.term_listing.as_deref() {
        return TermKey::parse(key).map(|term| term.plain());
    }
    let key = doc.url_path.strip_suffix("/index.html")?;
    if index.sites.contains_key(key) {
        return TermKey::parse(key).map(|term| term.plain());
    }
    index.kind_for(key).map(|(kind, _)| kind.key.as_str())
}

/// Link every term page to the same term's pages in the other editions.
/// Run once the generated term pages exist and before anything fingerprints
/// the documents. A page already paired with a page of that language keeps
/// its own pairing.
pub fn link_term_translations(documents: &mut [ParsedDocument], index: &TermIndex) {
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, doc) in documents.iter().enumerate() {
        if let Some(identity) = term_identity(doc, index) {
            groups.entry(identity).or_default().push(i);
        }
    }
    let mut added: Vec<(usize, TranslationLink)> = Vec::new();
    for members in groups.values().filter(|m| m.len() > 1) {
        for &from in members {
            for &to in members {
                let (a, b) = (&documents[from], &documents[to]);
                let Some(tag) = b.lang_tag.as_deref() else { continue };
                if from == to || a.lang_tag.as_deref() == Some(tag) {
                    continue;
                }
                if a.translations.iter().any(|t| t.lang_tag == tag || t.url_path == b.url_path) {
                    continue;
                }
                added.push((
                    from,
                    TranslationLink {
                        lang_tag: tag.to_string(),
                        url_path: b.url_path.clone(),
                        display_name: crate::i18n::switcher_label(tag),
                    },
                ));
            }
        }
    }
    let mut linked = std::collections::BTreeSet::new();
    for (i, link) in added {
        documents[i].translations.push(link);
        linked.insert(i);
    }
    for i in linked {
        documents[i].translations.sort_by(|a, b| a.url_path.cmp(&b.url_path));
    }
}
