//! The card a term draws in a listing: how many listable pages carry the term,
//! and the picture of the first of them in the order the term page lists them.
//! A claimed term's card is its claiming page's own card; the claiming page
//! keeps its own picture and takes the first member's only when it has none.

use std::collections::{HashMap, HashSet};

use super::{TermIndex, TermKey};
use crate::build::scan::article_map::to_pretty_url;
use crate::build::types::{ParsedDocument, TermCard};

/// Sets `term_card` on the document that draws each term's card: the
/// pseudo-folder for an unclaimed term, the claiming page for a claimed one.
pub fn set_term_cards(documents: &mut [ParsedDocument], index: &TermIndex) {
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut by_pretty: HashMap<String, usize> = HashMap::new();
    for (i, doc) in documents.iter().enumerate() {
        by_path.insert(doc.url_path.clone(), i);
        if doc.is_listable() {
            by_pretty.insert(to_pretty_url(&doc.url_path), i);
        }
    }
    let assigned: Vec<(usize, TermCard)> = index
        .term_keys()
        .filter_map(|key| {
            let (target, claimed) = match index.claimed_by(key) {
                Some(url) => (*by_pretty.get(url)?, true),
                None => (*by_path.get(&format!("{key}/index.html"))?, false),
            };
            let mut card = term_card(key, index, documents, &by_pretty)?;
            let kind_key = TermKey::parse(key).map_or(key, |term| term.ns);
            let lang = documents[target].lang.as_bcp47_attr();
            card.count_setting = index.kinds().iter().find(|kind| kind.key == kind_key).and_then(|kind| kind.count_in(lang));
            if claimed && documents[target].listing_cover().is_some() {
                card.cover = None;
            }
            Some((target, card))
        })
        .collect();
    for (i, card) in assigned {
        documents[i].term_card = Some(card);
    }
}

fn term_card(
    key: &str,
    index: &TermIndex,
    documents: &[ParsedDocument],
    by_pretty: &HashMap<String, usize>,
) -> Option<TermCard> {
    let mut members = HashSet::new();
    let mut cover = None;
    for (_, urls) in index.sections(key)? {
        let docs: Vec<&ParsedDocument> = urls
            .iter()
            .filter_map(|url| by_pretty.get(url).copied())
            .map(|i| {
                members.insert(i);
                &documents[i]
            })
            .collect();
        if cover.is_none() {
            let mut sorted = docs.clone();
            sorted.sort_by(|a, b| moss_core::sort::cmp_date_axis(&date_key(a), &date_key(b), false));
            cover = sorted.into_iter().find_map(|doc| doc.listing_cover().map(str::to_string));
        }
    }
    (!members.is_empty()).then(|| TermCard { count: members.len(), cover, ..Default::default() })
}

/// The term page sorts each group by date whatever its axis (the generated
/// listing re-sorts every group), so the card orders members the same way.
fn date_key(doc: &ParsedDocument) -> moss_core::sort::DateSortKey<'_> {
    moss_core::sort::DateSortKey {
        date: doc.date.as_deref(),
        is_folder: false,
        label: &doc.label,
        url_path: &doc.url_path,
    }
}
