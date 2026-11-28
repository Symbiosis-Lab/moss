//! Post-pass-2 hierarchy resolution: `TermSite.parent`,
//! children-with-counts, and flagging a real document that sits at a
//! place-typed namespace root. Split out of `terms.rs` (sibling-file
//! pattern) so the per-document membership derivation (`derive_terms`
//! itself) stays apart from the once-per-build resolution that reads its
//! finished output — the two grow independently and neither needs the
//! other's internals.

use super::TermIndex;
use crate::build::types::ParsedDocument;

/// A real folder index sitting exactly at a place-typed namespace root
/// (`places/index.md`, no `place_page:` claim — a claim always resolves to
/// a sub-key, never the bare root) still hosts that kind's term map;
/// `render/html.rs` reads this flag instead of re-deriving root membership
/// from `TermIndex`, which it cannot see (same reasoning as
/// `ParsedDocument::term_sections` in `derive_terms`). Called from
/// `derive_terms` after `index.kinds` is set, since
/// [`TermIndex::place_namespace_roots`] reads it.
pub fn flag_place_namespace_roots(documents: &mut [ParsedDocument], index: &TermIndex) {
    let place_roots: std::collections::HashSet<&str> = index.place_namespace_roots().collect();
    if place_roots.is_empty() {
        return;
    }
    for doc in documents.iter_mut() {
        if let Some(folder) = doc.url_path.strip_suffix("/index.html") {
            if place_roots.contains(folder) {
                doc.is_place_namespace_root = true;
            }
        }
    }
}
