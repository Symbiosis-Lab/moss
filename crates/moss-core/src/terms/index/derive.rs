//! The passes that build a [`TermIndex`] from a site's pages.
use std::collections::BTreeMap;

use super::{TermIndex, TermKey, TermRoot, TermSite};
use crate::page::site_page::SitePage;
use crate::resolve::output_url::to_pretty_url;
use crate::terms::kinds::{scope, TermKind};

/// What the index adds to one page, in the host's page order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageTerms {
    /// Term and namespace-root keys the page joins, after its own `also_in:`.
    pub also_in: Vec<String>,
    /// The term whose member listing this page hosts, as its claimer.
    pub term_listing: Option<String>,
}

/// An index, what it adds to each page, and the problems found building it.
pub struct Derived {
    pub index: TermIndex,
    pub pages: Vec<PageTerms>,
    pub problems: Vec<String>,
}

pub fn derive_index<P: SitePage>(pages: &[P], kinds: Vec<TermKind>) -> Derived {
    let mut problems = Vec::new();
    let mut index = TermIndex::default();
    let mut also_in: Vec<Vec<String>> = pages.iter().map(|p| p.also_in().to_vec()).collect();
    let authored: Vec<usize> = also_in.iter().map(Vec::len).collect();
    let mut term_listing: Vec<Option<String>> = vec![None; pages.len()];

    // Pass 1: claims. Resolve every *_page claim to its term key so pass 2
    // can exclude the claiming page from its own listing.
    for doc in pages {
        for kind in &kinds {
            for field in &kind.fields {
                let Some(claim) = doc.term_claim(field) else { continue };
                let name = claim.trim();
                let Some(key) = resolved_key(kind, doc.edition(), name, &mut problems) else { continue };
                let site = index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                    display: name.to_string(),
                    claimed_by: None,
                    parent: None,
                });
                // First URL wins, so the result is scan-order independent.
                let claim = to_pretty_url(doc.url_path());
                match &site.claimed_by {
                    None => site.claimed_by = Some(claim),
                    Some(existing) if *existing <= claim => {
                        problems.push(format!(
                            "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                            name, existing, claim
                        ));
                    }
                    Some(existing) => {
                        problems.push(format!(
                            "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                            name, claim, existing
                        ));
                        site.claimed_by = Some(claim);
                    }
                }
            }
        }
    }

    // The winning claimer hosts the term's member listing. Deliberately NOT
    // lowered onto `children_source`: that field is a wikilink reference that
    // resolves through stem extraction (`frontmatter_ref_to_stem` keeps only
    // the last path segment), which would mangle a pseudo-folder key like
    // `authors/林小滿`. A dedicated resolved field keeps the render layer and
    // the listing-group digest reading one unambiguous value.
    for (i, doc) in pages.iter().enumerate() {
        let claim_key = claimed_key(doc, &kinds, &index, &mut problems);
        let Some(key) = claim_key else { continue };
        // The claim page takes the generated page's place in the NAMESPACE
        // ROOT listing too (`/authors/`, `/tags/`): membership in the root is
        // declared the same way term membership is, via `also_in`.
        if let Some(term) = TermKey::parse(&key) {
            also_in[i].push(term.root());
        }
        if doc.children_source().is_some() {
            continue; // author routed children explicitly; their routing wins
        }
        if doc.children_off() {
            continue; // author suppressed the listing; the claim still resolves links
        }
        term_listing[i] = Some(key);
    }

    // Pass 2: memberships. Every field value of every kind claims membership
    // in its pseudo-folder, beside any authored `also_in` — and records which
    // field it came through, in `members_by_field`. A place-typed kind's
    // resolved key also walks `kind.parents` upward (the roll-up: a page in
    // a city also lists on its region and country), pushing each ancestor
    // through the same steps as the immediate key.
    for (i, doc) in pages.iter().enumerate() {
        // A page reached only by its link joins no term: the term's page
        // would name the page, or the term, to the whole site.
        if doc.search_engines() == Some(false) || doc.hidden_home().is_some() {
            continue;
        }
        let own_claim = claimed_key(doc, &kinds, &index, &mut problems);
        let doc_edition = doc.edition().to_string();
        let doc_url = to_pretty_url(doc.url_path());
        // Dedup `also_in`: a page naming the same term through more than one
        // field of one kind (`author:` and `editor:` both crediting the same
        // person) must still occupy that pseudo-folder's membership list only
        // once — the per-field split still records each field's own
        // contribution in `members_by_field` below, which is a different
        // question ("which field did this member come through") that the
        // dedup here doesn't touch.
        let mut also_in_pushed: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Gates the `members_by_field` push, for the immediate key AND every
        // ancestor alike — keyed on `(key, field)`, not on `also_in_pushed`
        // (which only remembers keys already pushed into `also_in`, so it
        // neither stops an ancestor reached twice through two fields of one
        // document, nor a key reached both directly and as an ancestor).
        let mut members_pushed: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for kind in &kinds {
            for field in &kind.fields {
                let names = doc.term_values(field);
                for name in &names {
                    let name = name.trim();
                    let Some(key) = resolved_key(kind, &doc_edition, name, &mut problems) else { continue };
                    // First-seen original case is the display form; later
                    // case variants merge into the same key by construction.
                    index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                        display: name.to_string(),
                        claimed_by: None,
                        parent: None,
                    });
                    if own_claim.as_deref() != Some(key.as_str()) {
                        // a term page never lists itself
                        if also_in_pushed.insert(key.clone()) {
                            also_in[i].push(key.clone());
                        }
                        if members_pushed.insert((key.clone(), field.clone())) {
                            index
                                .members_by_field
                                .entry((key.clone(), field.clone()))
                                .or_default()
                                .push(doc_url.clone());
                        }
                    }
                    // The roll-up: walk `kind.parents` upward from `key`
                    // (empty for every non-place kind). `seen` guards a
                    // hand-built kind whose parents form a cycle.
                    let mut current = key.clone();
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    seen.insert(current.clone());
                    while let Some(parent_display) = kind.parents.get(&current) {
                        let anc = kind.key_in("", parent_display);
                        index.sites.entry(anc.clone()).or_insert_with(|| TermSite {
                            display: parent_display.clone(),
                            claimed_by: None,
                            parent: None,
                        });
                        if own_claim.as_deref() != Some(anc.as_str()) {
                            if also_in_pushed.insert(anc.clone()) {
                                also_in[i].push(anc.clone());
                            }
                            if members_pushed.insert((anc.clone(), field.clone())) {
                                index
                                    .members_by_field
                                    .entry((anc.clone(), field.clone()))
                                    .or_default()
                                    .push(doc_url.clone());
                            }
                        }
                        if !seen.insert(anc.clone()) {
                            break;
                        }
                        current = anc;
                    }
                }
            }
        }
    }

    // `TermSite.parent` and children-with-counts — resolved
    // once, in the sibling `rollup` module, now that pass 2's memberships
    // are final. See its own doc comment for why this is a separate sweep
    // rather than set inline during the ancestor walk above.
    resolve_hierarchy(&mut index, &kinds);

    // Second small pass: resolve `term_sections` for every claiming page,
    // now that `members_by_field` is complete. A reverse scan of every
    // document's now-final `also_in` finds hand-authored cross-listing
    // (`also_in: [...]`) that names a term directly — membership no field
    // record claims — so it can still appear, in a trailing unlabelled
    // group, rather than being silently dropped.
    let mut all_members_by_term: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (i, doc) in pages.iter().enumerate() {
        for key in &also_in[i] {
            if key.contains('/') {
                all_members_by_term.entry(key.clone()).or_default().push(to_pretty_url(doc.url_path()));
            }
        }
    }
    // Resolved for EVERY term, not only the claimed ones: a term no page
    // claims still gets a listing, rendered from a synthetic folder document
    // the render layer builds after this pass. Both readers take the same
    // value — the claiming page through its own `term_sections` below, the
    // generated page through `TermIndex::sections`.
    let mut sections_by_term: BTreeMap<String, Vec<(Option<String>, Vec<String>)>> = BTreeMap::new();
    for term_key in index.sites.keys() {
        let Some(term) = TermKey::parse(term_key) else { continue };
        let Some(kind) = kinds.iter().find(|k| k.key == term.ns) else { continue };
        let mut claimed_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut sections: Vec<(Option<String>, Vec<String>)> = kind
            .fields
            .iter()
            .map(|field| {
                let urls = index
                    .members_by_field
                    .get(&(term_key.clone(), field.clone()))
                    .cloned()
                    .unwrap_or_default();
                claimed_urls.extend(urls.iter().cloned());
                (Some(field.clone()), urls)
            })
            .collect();
        let trailing: Vec<String> = all_members_by_term
            .get(term_key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|u| !claimed_urls.contains(u))
            .collect();
        sections.push((None, trailing));
        sections_by_term.insert(term_key.clone(), sections);
    }
    index.sections_by_term = sections_by_term;

    // A claimed term
    // with no member pages is either an author with no published works yet
    // (valid) or a misspelled claim name; warn so the author can tell which.
    for (key, site) in &index.sites {
        if let Some(claimer) = &site.claimed_by {
            let has_member = also_in.iter().any(|a| a.contains(key));
            if !has_member {
                problems.push(format!(
                    "term page '{}' ({}) has no member pages — if that is unexpected, check the claimed name for typos",
                    site.display, claimer
                ));
            }
        }
    }

    index.kinds = kinds;
    let mut tiers: Vec<&str> = vec![""];
    tiers.extend(index.sites.keys().filter_map(|key| TermKey::parse(key)).map(|term| term.edition));
    tiers.sort_unstable();
    tiers.dedup();
    let roots: Vec<TermRoot> = index
        .kinds
        .iter()
        .enumerate()
        .flat_map(|(i, kind)| {
            tiers
                .iter()
                .filter(move |edition| edition.is_empty() || !kind.is_place)
                .map(move |edition| TermRoot { key: scope(edition, &kind.key), kind: i, edition: edition.to_string() })
        })
        .filter(|root| index.namespace_in_use(&root.key))
        .collect();
    index.roots = roots;
    let pages = also_in.into_iter().zip(authored).zip(term_listing)
        .map(|((mut also_in, authored), term_listing)| PageTerms { also_in: also_in.split_off(authored), term_listing })
        .collect();
    Derived { index, pages, problems }
}

fn resolved_key(kind: &TermKind, edition: &str, name: &str, problems: &mut Vec<String>) -> Option<String> {
    if !name.chars().any(char::is_alphanumeric) {
        if !name.is_empty() {
            problems.push(format!(
                "term name '{}' has no sluggable characters; skipping", name
            ));
        }
        return None;
    }
    Some(kind.key_in(edition, name))
}

fn claimed_key<P: SitePage>(doc: &P, kinds: &[TermKind], index: &TermIndex, problems: &mut Vec<String>) -> Option<String> {
    let mut found: Option<String> = None;
    for kind in kinds {
        for field in &kind.fields {
            let Some(claim) = doc.term_claim(field) else { continue };
            let name = claim.trim();
            let Some(key) = resolved_key(kind, doc.edition(), name, problems) else {
                continue;
            };
            // Only the winning claimer acts as the term page.
            if index.sites.get(&key).and_then(|s| s.claimed_by.as_deref())
                != Some(to_pretty_url(doc.url_path()).as_str())
            {
                continue;
            }
            match found {
                None => found = Some(key),
                Some(ref first) => {
                    problems.push(format!(
                        "page {} claims more than one term page; keeping {}",
                        doc.url_path(), first
                    ));
                }
            }
        }
    }
    found
}

/// `TermSite.parent`, and children-with-counts, once memberships are final.
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
    // may appear nowhere else (no page's `location:` names it), and without
    // a site it would get no page. `seen` guards a cyclic `parents` map.
    let sweep_keys: Vec<String> = index.sites.keys().cloned().collect();
    for start in sweep_keys {
        let Some((ns, _)) = start.split_once('/') else { continue };
        let Some(kind) = kinds.iter().find(|k| k.key == ns) else { continue };
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(start.clone());
        let mut current = start;
        while let Some(parent_display) = kind.parents.get(&current) {
            let anc_key = kind.key_in("", parent_display);
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

    // Resolve direct children once for authored and generated term pages.
    let mut children_by_term: BTreeMap<String, Vec<(String, String, usize)>> = BTreeMap::new();
    for (key, site) in &index.sites {
        let Some(parent_key) = &site.parent else { continue };
        let count = distinct_member_count(&index.members_by_field, key);
        children_by_term.entry(parent_key.clone()).or_default().push((
            site.display.clone(),
            TermIndex::site_target(key, site),
            count,
        ));
    }
    index.children_by_term = children_by_term;
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
