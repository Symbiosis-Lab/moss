//! Term derivation: authors and tags become pseudo-folder memberships.
//!
//! Runs once per build, after every document is parsed and before the
//! synthetic folder-index machinery. For each enabled dimension it derives,
//! from `author:` / `tags:` values, a membership claim in the term's
//! pseudo-folder (`authors/<slug>`, `tags/<slug>`) — pushed into the same
//! `also_in` slot an authored cross-listing uses — so the canonical selector
//! (`folder_embed::select_children_by_slug`), the synthetic-index loops in
//! `render/blocking.rs`, and the ADR-044 listing digest all serve term pages
//! with zero new modes. A page claiming a term (`author_page:` / `tag_page:`)
//! gets the resolved key in `ParsedDocument::term_listing` — the one field the
//! renderer and the listing digest read — and suppresses the
//! generated page for that term. Term identity rules (fold, slug, key) are
//! owned by `moss_core::terms`; this pass only applies them to documents.
//! Design: `docs/archive/2026-09-01-tags-and-authors-design.md`.

use std::collections::BTreeMap;

use moss_core::terms::{term_folder_key, AUTHOR_NS, TAGS_NS};

use crate::build::scan::article_map::to_pretty_url;
use crate::build::types::ParsedDocument;

/// One term's build-time identity: where its page lives and what it is called.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TermSite {
    /// Display name — the first-seen original-case form of the term.
    pub display: String,
    /// Pretty URL key of the claiming page (`about/ma/`; `` for the home
    /// page), when a real page claims this term. `None` ⇒ the term page is
    /// generated at the pseudo-folder key. Persisted as-is in the article
    /// map, so the editor reads the claim instead of re-deriving it.
    pub claimed_by: Option<String>,
}

/// The vault's terms, keyed by pseudo-folder key (`authors/<slug>`,
/// `tags/<slug>`). Built by [`derive_terms`]; consumed by the synthetic-index
/// loops (which keys to seed, what title to show) and by the link pass
/// (where a term name points).
#[derive(Debug, Default)]
pub struct TermIndex {
    sites: BTreeMap<String, TermSite>,
}

impl TermIndex {
    /// Pseudo-folder keys that need a GENERATED term page — every term no
    /// real page claims. Sorted (BTreeMap) so page emission is deterministic.
    pub fn unclaimed_keys(&self) -> impl Iterator<Item = &str> {
        self.sites
            .iter()
            .filter(|(_, s)| s.claimed_by.is_none())
            .map(|(k, _)| k.as_str())
    }

    /// Display title for a term pseudo-folder key. `None` for the two
    /// namespace roots, which are no term: their heading is an i18n string
    /// (`i18n::term_root_title`, via `render/blocking.rs::synthetic_folder_doc`).
    pub fn display(&self, key: &str) -> Option<&str> {
        self.sites.get(key).map(|s| s.display.as_str())
    }

    /// Root-relative URL a mention of this term should link to: the claiming
    /// page when one exists, the generated pseudo-folder page otherwise.
    /// `None` when the term is unknown (nothing derived it — dimension off or
    /// value absent everywhere).
    pub fn term_url(&self, ns: &str, name: &str) -> Option<String> {
        let key = term_folder_key(ns, name);
        let site = self.sites.get(&key)?;
        Some(match &site.claimed_by {
            Some(claim) => format!("/{}", claim), // allow:served-path-url-construct (term link to the claiming page)
            None => format!("/{}/", key), // allow:served-path-url-construct (term link to the generated pseudo-folder page)
        })
    }

    /// Every derived term, keyed by pseudo-folder key, in key order. What the
    /// article map records so the editor reads claims instead of re-deriving
    /// them (ADR-019: one owner of link semantics).
    pub fn sites(&self) -> &BTreeMap<String, TermSite> {
        &self.sites
    }

    /// Whether any term exists in this namespace (drives whether the
    /// namespace root index is worth emitting).
    fn namespace_in_use(&self, ns: &str) -> bool {
        let prefix = format!("{}/", ns);
        self.sites.keys().any(|k| k.starts_with(&prefix))
    }

    /// The namespace roots (`authors`, `tags`) that have at least one term —
    /// a claimed one counts, since the claim page joins the root listing.
    pub fn roots_in_use(&self) -> impl Iterator<Item = &'static str> + '_ {
        [AUTHOR_NS, TAGS_NS]
            .into_iter()
            .filter(move |ns| self.namespace_in_use(ns))
    }

    /// Every pseudo-folder that needs a synthetic folder document and an
    /// index page: the unclaimed terms, plus the root of each namespace in
    /// use. The one list both folder-index synthesis blocks in
    /// `render/blocking.rs` seed from, so they cannot disagree about which
    /// pseudo-folders exist (a claimed term needs neither — the claiming page
    /// is a real document — but its root still does, and the root's own
    /// `authors/index.html` has no ancestor for the ancestor walk to register).
    pub fn synthetic_folder_keys(&self) -> Vec<String> {
        self.unclaimed_keys()
            .map(str::to_string)
            .chain(self.roots_in_use().map(str::to_string))
            .collect()
    }
}

/// Derive term memberships and claims. Mutates documents in place:
///
/// - every enabled term value pushes its pseudo-folder key into the doc's
///   `also_in` slot (skipping the doc that claims that very term, so an
///   author page does not list itself);
/// - the doc that WINS a term claim gets the term key in `term_listing` so it
///   hosts the member listing, unless the author already routed `children`
///   somewhere explicitly (their routing wins; the claim still resolves).
///
/// Duplicate claims on one term: the lexicographically first `url_path` wins
/// (deterministic across builds), the loser is logged. A claim on a term no
/// page carries still creates the term (the claim IS an occurrence — an
/// author page for someone with no published works yet is valid and lists
/// nothing).
pub fn derive_terms(
    documents: &mut [ParsedDocument],
    authors_on: bool,
    tags_on: bool,
) -> TermIndex {
    let mut index = TermIndex::default();
    if !authors_on && !tags_on {
        return index;
    }

    // Pass 1: claims. Resolve every author_page/tag_page to its term key so
    // pass 2 can exclude the claiming page from its own listing.
    for doc in documents.iter() {
        let claims = [
            (authors_on, AUTHOR_NS, &doc.author_page),
            (tags_on, TAGS_NS, &doc.tag_page),
        ];
        for (on, ns, claim) in claims {
            let Some(claim) = claim else { continue };
            if !on {
                continue;
            }
            let name = claim.name(&doc.title).trim();
            let Some(key) = resolved_key(ns, name) else {
                continue;
            };
            let site = index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                display: name.to_string(),
                claimed_by: None,
            });
            // First URL wins, so the result is scan-order independent.
            let claim = to_pretty_url(&doc.url_path);
            match &site.claimed_by {
                None => site.claimed_by = Some(claim),
                Some(existing) if *existing <= claim => {
                    log::warn!(
                        "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                        name, existing, claim
                    );
                }
                Some(existing) => {
                    log::warn!(
                        "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                        name, claim, existing
                    );
                    site.claimed_by = Some(claim);
                }
            }
        }
    }

    // The winning claimer hosts the term's member listing. Deliberately NOT
    // lowered onto `children_source`: that field is a wikilink reference that
    // resolves through stem extraction (`frontmatter_ref_to_stem` keeps only
    // the last path segment), which would mangle a pseudo-folder key like
    // `authors/馬欣宜`. A dedicated resolved field keeps the render layer and
    // the ADR-044 listing digest reading one unambiguous value.
    for doc in documents.iter_mut() {
        let claim_key = claimed_key(doc, authors_on, tags_on, &index);
        let Some(key) = claim_key else { continue };
        // The claim page takes the generated page's place in the NAMESPACE
        // ROOT listing too (`/authors/`, `/tags/`): membership in the root is
        // declared the same way term membership is, via `also_in`.
        if let Some((ns, _)) = key.split_once('/') {
            doc.also_in.get_or_insert_with(Vec::new).push(ns.to_string());
        }
        if doc.children_source.is_some() {
            continue; // author routed children explicitly; their routing wins
        }
        if doc.children == Some(false) {
            continue; // author suppressed the listing; the claim still resolves links
        }
        doc.term_listing = Some(key);
    }

    // Pass 2: memberships. Every term value claims membership in its
    // pseudo-folder, beside any authored `also_in`.
    for doc in documents.iter_mut() {
        let own_claim = claimed_key(doc, authors_on, tags_on, &index);
        let mut push = |index: &mut TermIndex, ns: &str, name: &str| {
            let name = name.trim();
            let Some(key) = resolved_key(ns, name) else {
                return;
            };
            // First-seen original case is the display form; later case
            // variants merge into the same key by construction.
            index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                display: name.to_string(),
                claimed_by: None,
            });
            if own_claim.as_deref() == Some(key.as_str()) {
                return; // a term page never lists itself
            }
            doc.also_in.get_or_insert_with(Vec::new).push(key);
        };
        if authors_on {
            let names = doc.author.clone();
            for name in &names {
                push(&mut index, AUTHOR_NS, name);
            }
        }
        if tags_on {
            // Frontmatter tags only — inline #hashtags are prose, not
            // cataloguing (see `fm_tags` on ParsedDocument).
            let tags = doc.fm_tags.clone().unwrap_or_default();
            for tag in &tags {
                push(&mut index, TAGS_NS, tag);
            }
        }
    }

    // Typo diagnostic (design §"what is genuinely required"): a claimed term
    // with no member pages is either an author with no published works yet
    // (valid) or a misspelled claim name; warn so the author can tell which.
    for (key, site) in &index.sites {
        if let Some(claimer) = &site.claimed_by {
            let has_member = documents
                .iter()
                .any(|d| d.also_in.as_ref().is_some_and(|a| a.contains(key)));
            if !has_member {
                log::warn!(
                    "term page '{}' ({}) has no member pages — if that is unexpected, check the claimed name for typos",
                    site.display, claimer
                );
            }
        }
    }

    index
}

/// Make author names in byline rows clickable (design rule 3).
///
/// For each page, every `author:` name that appears verbatim in a `byline:`
/// row becomes a markdown link to that author's term page — byline rows
/// render through the one inline-markdown path (`render/credits.rs`), so a
/// link is injected as markdown, not HTML. The byline stays the author's
/// display text ("no machine claim"); only names the author ALSO declared in
/// `author:` are linked, and only where the author typed them.
///
/// Runs after [`derive_terms`] and before the render fingerprints are taken,
/// so a claim appearing anywhere moves the linked URL and with it the page's
/// surface — invalidation is free.
pub fn link_authors_in_bylines(documents: &mut [ParsedDocument], index: &TermIndex) {
    for doc in documents.iter_mut() {
        if doc.byline.is_empty() || doc.author.is_empty() {
            continue;
        }
        // Longest name first, so "馬欣" never claims the middle of "馬欣宜"
        // when both are credited.
        let mut names: Vec<String> = doc.author.clone();
        names.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
        let own_url = format!("/{}", doc.url_path.trim_end_matches("index.html")); // allow:served-path-url-construct (self-link comparison only, never emitted)
        let links: Vec<(&str, String)> = names
            .iter()
            .filter_map(|name| {
                let name = name.trim();
                // Markdown link syntax cannot carry these verbatim in link text.
                if name.is_empty() || name.contains(['[', ']', '(', ')']) {
                    return None;
                }
                // None: dimension off, or the name derived nothing.
                let url = index.term_url(AUTHOR_NS, name)?;
                if url == own_url {
                    return None; // the term page itself never links to itself
                }
                Some((name, url))
            })
            .collect();
        for row in doc.byline.iter_mut() {
            // All match positions are found on the ORIGINAL row, longest name
            // first and non-overlapping, and the linked text is assembled
            // once at the end — so a shorter co-author name ("馬欣") can never
            // match inside the link already injected for a longer one
            // ("馬欣宜") and nest markdown.
            let mut matches: Vec<(usize, &str, &str)> = Vec::new();
            for (name, url) in &links {
                // Author intent: a row that already links this name (in any
                // way the author chose) is left alone.
                if row.contains(&format!("[{}]", name)) {
                    continue;
                }
                if let Some(pos) = row.find(name) {
                    let overlaps = matches
                        .iter()
                        .any(|(p, n, _)| pos < p + n.len() && *p < pos + name.len());
                    if !overlaps {
                        matches.push((pos, name, url));
                    }
                }
            }
            if matches.is_empty() {
                continue;
            }
            matches.sort_by_key(|(pos, ..)| *pos);
            let mut out = String::with_capacity(row.len() + matches.len() * 24);
            let mut cursor = 0;
            for (pos, name, url) in matches {
                out.push_str(&row[cursor..pos]);
                out.push('[');
                out.push_str(name);
                out.push_str("](");
                out.push_str(url);
                out.push(')');
                cursor = pos + name.len();
            }
            out.push_str(&row[cursor..]);
            *row = out;
        }
    }
}

/// Pseudo-folder key for a term name, or `None` for a name with no
/// alphanumeric characters at all (empty, or punctuation-only like "！！！").
/// Such names have no slug of their own — `generate_slug` falls back to
/// "untitled", which would silently merge every punctuation-only value into
/// one shared `authors/untitled` term.
fn resolved_key(ns: &str, name: &str) -> Option<String> {
    if !name.chars().any(char::is_alphanumeric) {
        if !name.is_empty() {
            log::warn!("term name '{}' has no sluggable characters; skipping", name);
        }
        return None;
    }
    Some(term_folder_key(ns, name))
}

/// The pseudo-folder key this doc claims, if any (author_page wins over
/// tag_page when both are set — a page can be one term's page, and two
/// claims on one page is an authoring mistake worth a warning).
fn claimed_key(
    doc: &ParsedDocument,
    authors_on: bool,
    tags_on: bool,
    index: &TermIndex,
) -> Option<String> {
    let candidates = [
        (authors_on, AUTHOR_NS, &doc.author_page),
        (tags_on, TAGS_NS, &doc.tag_page),
    ];
    let mut found: Option<String> = None;
    for (on, ns, claim) in candidates {
        let Some(claim) = claim else { continue };
        if !on {
            continue;
        }
        let name = claim.name(&doc.title).trim();
        let Some(key) = resolved_key(ns, name) else {
            continue;
        };
        // Only the winning claimer acts as the term page.
        if index.sites.get(&key).and_then(|s| s.claimed_by.as_deref())
            != Some(to_pretty_url(&doc.url_path).as_str())
        {
            continue;
        }
        match found {
            None => found = Some(key),
            Some(ref first) => {
                log::warn!(
                    "page {} claims both an author page and a tag page; keeping {}",
                    doc.url_path, first
                );
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::terms::TermClaim;

    fn doc(url: &str, title: &str) -> ParsedDocument {
        ParsedDocument {
            url_path: url.to_string(),
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn author_value_derives_pseudo_folder_membership() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["馬欣宜".into()];
        docs[0].fm_tags = Some(vec!["城市".into()]);
        let index = derive_terms(&mut docs, true, true);
        let also = docs[0].also_in.as_ref().unwrap();
        assert!(also.contains(&"authors/馬欣宜".to_string()));
        assert!(also.contains(&"tags/城市".to_string()));
        assert_eq!(index.term_url("authors", "馬欣宜").as_deref(), Some("/authors/馬欣宜/"));
        assert_eq!(index.unclaimed_keys().count(), 2);
    }

    #[test]
    fn synthetic_folders_are_the_unclaimed_terms_plus_the_roots_in_use() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["馬欣宜".into(), "Scarly".into()];
        docs[1].fm_tags = Some(vec!["城市".into()]);
        let index = derive_terms(&mut docs, true, true);
        // The claimed term is a real page; its root is still synthesized.
        assert_eq!(
            index.synthetic_folder_keys(),
            vec!["authors/scarly", "tags/城市", "authors", "tags"]
        );
        // A dimension with no terms has no root.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Scarly".into()];
        let index = derive_terms(&mut docs, true, true);
        assert_eq!(index.synthetic_folder_keys(), vec!["authors/scarly", "authors"]);
    }

    #[test]
    fn disabled_dimension_derives_nothing() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["X".into()];
        docs[0].fm_tags = Some(vec!["t".into()]);
        let index = derive_terms(&mut docs, false, true);
        let also = docs[0].also_in.clone().unwrap();
        assert_eq!(also, vec!["tags/t".to_string()]);
        assert!(index.term_url("authors", "X").is_none());
    }

    #[test]
    fn claim_replaces_generated_page_and_hosts_the_listing() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["馬欣宜".into()];
        let index = derive_terms(&mut docs, true, true);
        // The winning claimer hosts the member listing…
        assert_eq!(docs[0].term_listing.as_deref(), Some("authors/馬欣宜"));
        // …the term page is no longer generated…
        assert_eq!(index.unclaimed_keys().count(), 0);
        // …the claim page takes its place in the /authors/ root listing…
        assert!(docs[0].also_in.as_ref().unwrap().contains(&"authors".to_string()));
        // …links point at the claiming page…
        assert_eq!(index.term_url("authors", "馬欣宜").as_deref(), Some("/about/ma/"));
        // …and the member doc still claims membership for the selector.
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"authors/馬欣宜".to_string()));
    }

    #[test]
    fn a_term_page_never_lists_itself() {
        // The author page is itself authored by its subject.
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].author = vec!["馬欣宜".into()];
        derive_terms(&mut docs, true, true);
        // Root-listing membership yes, own-term membership no.
        assert_eq!(docs[0].also_in.as_deref(), Some(&["authors".to_string()][..]));
    }

    #[test]
    fn duplicate_claims_resolve_to_first_url_deterministically() {
        let mut a = doc("z/index.html", "X");
        a.author_page = Some(TermClaim::UseTitle);
        let mut b = doc("a/index.html", "X");
        b.author_page = Some(TermClaim::UseTitle);
        // Same result regardless of scan order.
        let index1 = derive_terms(&mut [a.clone(), b.clone()], true, true);
        let index2 = derive_terms(&mut [b, a], true, true);
        assert_eq!(index1.term_url("authors", "X").as_deref(), Some("/a/"));
        assert_eq!(index2.term_url("authors", "X").as_deref(), Some("/a/"));
    }

    #[test]
    fn explicit_children_routing_wins_over_claim_lowering() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].children_source = Some("[[News]]".into());
        let index = derive_terms(&mut docs, true, true);
        assert_eq!(docs[0].children_source.as_deref(), Some("[[News]]"));
        assert!(docs[0].term_listing.is_none());
        // The claim still resolves links and suppresses the generated page.
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert_eq!(index.term_url("authors", "馬欣宜").as_deref(), Some("/about/ma/"));
    }

    #[test]
    fn byline_names_link_to_term_pages() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["馬欣宜".into()];
        docs[0].byline = vec!["文｜馬欣宜".into(), "編輯｜其他人".into()];
        let index = derive_terms(&mut docs, true, true);
        link_authors_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜[馬欣宜](/authors/馬欣宜/)");
        // Names not declared in `author:` stay untouched.
        assert_eq!(docs[0].byline[1], "編輯｜其他人");
    }

    #[test]
    fn byline_link_points_at_a_claim_page_and_never_at_itself() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].author = vec!["馬欣宜".into()];
        docs[0].byline = vec!["馬欣宜".into()];
        docs[1].author = vec!["馬欣宜".into()];
        docs[1].byline = vec!["文｜馬欣宜".into()];
        let index = derive_terms(&mut docs, true, true);
        link_authors_in_bylines(&mut docs, &index);
        assert_eq!(docs[1].byline[0], "文｜[馬欣宜](/about/ma/)");
        // The claim page's own byline names its subject: no self-link.
        assert_eq!(docs[0].byline[0], "馬欣宜");
    }

    #[test]
    fn a_co_author_name_inside_a_longer_name_never_nests_links() {
        // "馬欣" is a substring of "馬欣宜"; matching must happen on the
        // original row only, or the shorter name matches inside the link
        // injected for the longer one and nests markdown.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["馬欣宜".into(), "馬欣".into()];
        docs[0].byline = vec!["文｜馬欣宜".into(), "編｜馬欣".into()];
        let index = derive_terms(&mut docs, true, true);
        link_authors_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜[馬欣宜](/authors/馬欣宜/)");
        assert_eq!(docs[0].byline[1], "編｜[馬欣](/authors/馬欣/)");
    }

    #[test]
    fn punctuation_only_names_derive_no_term() {
        let mut docs = vec![doc("posts/a/index.html", "！！！")];
        docs[0].author = vec!["！！！".into()];
        docs[0].author_page = Some(TermClaim::UseTitle);
        let index = derive_terms(&mut docs, true, true);
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert!(docs[0].also_in.is_none());
        assert!(docs[0].term_listing.is_none());
    }

    #[test]
    fn byline_rows_already_linked_by_the_author_are_left_alone() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Scarly".into()];
        docs[0].byline = vec!["by [Scarly](https://example.com)".into()];
        let index = derive_terms(&mut docs, true, true);
        link_authors_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "by [Scarly](https://example.com)");
    }

    #[test]
    fn case_variants_merge_into_one_term_with_first_seen_display() {
        let mut docs = vec![doc("p1/index.html", "A"), doc("p2/index.html", "B")];
        docs[0].fm_tags = Some(vec!["City".into()]);
        docs[1].fm_tags = Some(vec!["city".into()]);
        let index = derive_terms(&mut docs, true, true);
        assert_eq!(index.unclaimed_keys().count(), 1);
        assert_eq!(index.display("tags/city"), Some("City"));
    }
}
