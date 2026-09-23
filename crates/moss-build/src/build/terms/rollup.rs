//! Post-pass-2 hierarchy resolution: `TermSite.parent`, breadcrumbs, and
//! children-with-counts. Split out of `terms.rs` (sibling-file pattern) so
//! the per-document membership derivation (`derive_terms` itself) stays
//! apart from the once-per-build resolution that reads its finished
//! output — the two grow independently and neither needs the other's
//! internals.

use std::collections::BTreeMap;

use moss_core::terms::term_folder_key;

use super::{TermIndex, TermKind, TermSite};

/// `TermSite.parent`, breadcrumbs, and children-with-counts, resolved once
/// here after [`super::derive_terms`]'s pass 2 memberships are final.
/// Mutates `index` in place.
pub fn resolve_hierarchy(index: &mut TermIndex, kinds: &[TermKind]) {
    // `TermSite.parent`, in a separate sweep after pass 2 finishes — not set
    // inline during the ancestor walk above, which only runs for a
    // document's actual MEMBERS and would miss a place only a `place_page:
    // true` CLAIM created (a page naming a place nothing's `location:` ever
    // points at). This sweep visits every site in `index.sites` regardless
    // of how it got there, so a claimed-only place gets `parent` set
    // exactly like a member-reached one.
    //
    // Walks each starting key's FULL chain, not just the immediate parent,
    // and creates a missing ancestor's `TermSite` along the way (`display:
    // parent_display, claimed_by: None`) — a claimed leaf place's ancestor
    // may never separately appear in `index.sites` on its own (no
    // document's `location:` names it, and `attach_parents` itself creates
    // no sites), and without one it would have no page to breadcrumb-link
    // to, and no page would be generated for it at all. A4's own
    // cycle-guard already makes `kind.parents` a fixed point; `seen` here
    // is a defensive backstop, not a load-bearing cap.
    let sweep_keys: Vec<String> = index.sites.keys().cloned().collect();
    for start in sweep_keys {
        let Some((ns, _)) = start.split_once('/') else { continue };
        let Some(kind) = kinds.iter().find(|k| k.key == ns) else { continue };
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(start.clone());
        let mut current = start;
        while let Some(parent_display) = kind.parents.get(&current) {
            let anc_key = term_folder_key(&kind.key, parent_display);
            index.sites.entry(anc_key.clone()).or_insert_with(|| TermSite {
                display: parent_display.clone(),
                claimed_by: None,
                parent: None,
            });
            if let Some(site) = index.sites.get_mut(&current) {
                site.parent = Some(anc_key.clone());
            }
            if !seen.insert(anc_key.clone()) {
                break;
            }
            current = anc_key;
        }
    }

    // Breadcrumb and children, resolved once here (same "one computation,
    // two readers" pattern as `sections_by_term` in `derive_terms`), now
    // that every site's `parent` is final. A non-place term simply never
    // has a `parent`, so both maps end up with nothing recorded for it —
    // the emptiness check in the render layer is the only gate needed.
    let mut breadcrumbs_by_term: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for key in index.sites.keys() {
        let mut chain: Vec<(String, String)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(key.clone());
        let mut current = index.sites.get(key).and_then(|s| s.parent.clone());
        // A4's own cycle-guard already made the gazetteer's `parents` a
        // fixed point; `seen` here is a defensive backstop, not a load-
        // bearing cap.
        while let Some(anc_key) = current {
            if !seen.insert(anc_key.clone()) {
                break;
            }
            let Some(anc_site) = index.sites.get(&anc_key) else { break };
            chain.push((anc_site.display.clone(), site_url(&anc_key, anc_site)));
            current = anc_site.parent.clone();
        }
        if chain.is_empty() {
            continue;
        }
        chain.reverse(); // oldest ancestor first
        breadcrumbs_by_term.insert(key.clone(), chain);
    }
    index.breadcrumbs_by_term = breadcrumbs_by_term;

    let mut children_by_term: BTreeMap<String, Vec<(String, String, usize)>> = BTreeMap::new();
    for (key, site) in &index.sites {
        let Some(parent_key) = &site.parent else { continue };
        let count = distinct_member_count(&index.members_by_field, key);
        children_by_term.entry(parent_key.clone()).or_default().push((
            site.display.clone(),
            site_url(key, site),
            count,
        ));
    }
    index.children_by_term = children_by_term;
}

/// The root-relative URL a term key's own site resolves to — the claiming
/// page when one exists, the generated pseudo-folder page otherwise. Same
/// logic [`TermIndex::term_url`] applies from a `(ns, name)` pair; this
/// version takes an already-resolved key and site, for the breadcrumb/
/// children pass, which already has both in hand.
fn site_url(key: &str, site: &TermSite) -> String {
    match &site.claimed_by {
        Some(claim) => format!("/{}", claim), // allow:served-path-url-construct (term link to the claiming page)
        None => format!("/{}/", key), // allow:served-path-url-construct (term link to the generated pseudo-folder page)
    }
}

/// The number of DISTINCT member URLs across every field this term key was
/// reached through — not a raw sum over fields, which would double-count a
/// document reaching the term through two different fields (each field
/// legitimately records that document once).
fn distinct_member_count(
    members_by_field: &BTreeMap<(String, String), Vec<String>>,
    key: &str,
) -> usize {
    let mut urls: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for ((k, _field), list) in members_by_field {
        if k == key {
            urls.extend(list.iter().map(String::as_str));
        }
    }
    urls.len()
}
