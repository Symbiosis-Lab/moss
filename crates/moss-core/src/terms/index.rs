//! A site's terms: which pages each term lists, who claims its page, and the
//! namespace roots in use. Built by [`derive_index`] over any host's pages.
use std::collections::BTreeMap;

use super::kinds::{scope, TermKind};

mod derive;
pub use derive::{derive_index, Derived, PageTerms};

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
    /// This term's parent's pseudo-folder key (`places/japan`), when the
    /// term belongs to a place-typed kind and that kind's `parents` map
    /// names one after resolution. Left out of the JSON when absent, so a
    /// term with no parent adds no `"parent": null` to the article map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// The vault's terms, keyed by pseudo-folder key (`authors/<slug>`,
/// `tags/<slug>`, or a declared kind's own namespace). Built by
/// [`derive::derive_index`]; read by term page generation and by the link
/// pass (where a term name points).
#[derive(Debug, Clone, Default)]
pub struct TermIndex {
    sites: BTreeMap<String, TermSite>,
    /// The kinds this index was derived from, so a later reader sees the
    /// same kinds rather than re-reading config.
    kinds: Vec<TermKind>,
    /// Term key, field name → the member `url_path`s that field claims, in
    /// scan order. Recorded where a member's `also_in` entry is pushed, so
    /// the two cannot disagree about who is a member.
    members_by_field: BTreeMap<(String, String), Vec<String>>,
    /// Term key → that term's listing split by field, computed once so a
    /// claiming page and a generated term page cannot disagree about which
    /// field a member came through.
    sections_by_term: BTreeMap<String, Vec<(Option<String>, Vec<String>)>>,
    /// Term key → its direct children's `(display, url, count)`, where
    /// `count` is the number of distinct member URLs across the child's own
    /// `members_by_field` entries (not a raw sum over fields, which would
    /// double-count a document reaching the child through two fields).
    /// Roll-up-inclusive: a region's count includes every city under it.
    children_by_term: BTreeMap<String, Vec<(String, String, usize)>>,
    /// Every namespace root in use, in kind order then edition order.
    roots: Vec<TermRoot>,
}

/// A namespace root in use in one edition.
#[derive(Debug, Clone)]
struct TermRoot {
    /// `partners`, `de/partners`.
    key: String,
    /// Index into `TermIndex::kinds`.
    kind: usize,
    /// The edition's tree, `""` for the default edition.
    edition: String,
}

/// A kind's entry in one edition's nav bar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TermNavEntry {
    /// The edition's root page key (`partners`, `de/partners`).
    pub root: String,
    pub label: String,
    pub weight: Option<i32>,
    /// The edition's tree, `""` for the default edition.
    pub edition: String,
}

impl TermIndex {
    /// Every term key, claimed or not.
    pub fn term_keys(&self) -> impl Iterator<Item = &str> {
        self.sites.keys().map(String::as_str)
    }

    /// The pretty URL of the page that claims this term, if one does.
    pub fn claimed_by(&self, term_key: &str) -> Option<&str> {
        self.sites.get(term_key)?.claimed_by.as_deref()
    }

    /// Pseudo-folder keys that need a GENERATED term page — every term no
    /// real page claims. Sorted (BTreeMap) so page emission is deterministic.
    pub fn unclaimed_keys(&self) -> impl Iterator<Item = &str> {
        self.sites
            .iter()
            .filter(|(_, s)| s.claimed_by.is_none())
            .map(|(k, _)| k.as_str())
    }

    /// Display title for a term pseudo-folder key. `None` for a namespace
    /// root, which is no term: its heading is the owning kind's own
    /// resolved `title` (see [`Self::kind_for`]).
    pub fn display(&self, key: &str) -> Option<&str> {
        self.sites.get(key).map(|s| s.display.as_str())
    }

    /// The kind whose namespace root this key is (`partners`, `de/partners`),
    /// with the root's edition.
    ///
    /// Only for a namespace this build actually put terms in. A kind exists
    /// whether or not anything feeds it (the built-ins survive their
    /// dimension being switched off, with an empty `fields`), and a site
    /// with `[terms] author = false` may well have a real directory called
    /// `authors/` — which keeps its own name, rather than being retitled by
    /// a namespace nothing is in.
    pub fn kind_for(&self, key: &str) -> Option<(&TermKind, &str)> {
        self.roots.iter().find(|root| root.key == key).map(|root| (&self.kinds[root.kind], root.edition.as_str()))
    }

    /// Every derived term, keyed by pseudo-folder key, in key order. What the
    /// article map records so the editor reads claims instead of re-deriving
    /// them (one owner of link semantics).
    pub fn sites(&self) -> &BTreeMap<String, TermSite> {
        &self.sites
    }

    /// The kinds this index was derived from.
    pub fn kinds(&self) -> &[TermKind] {
        &self.kinds
    }

    /// Term key, field name → the member `url_path`s that field claims. What
    /// the article map's `fields_with_members` summarizes (term key → the
    /// subset of that kind's fields with at least one member).
    pub fn members_by_field(&self) -> &BTreeMap<(String, String), Vec<String>> {
        &self.members_by_field
    }

    /// This term's listing split by field, in `kind.fields` order with a
    /// trailing unlabelled group.
    pub fn sections(&self, term_key: &str) -> Option<&[(Option<String>, Vec<String>)]> {
        self.sections_by_term.get(term_key).map(Vec::as_slice)
    }

    /// This term's direct children, `(display, url, count)`.
    /// `None` for a term with no children (every non-place term, and a leaf
    /// place).
    pub fn children(&self, term_key: &str) -> Option<&[(String, String, usize)]> {
        self.children_by_term.get(term_key).map(Vec::as_slice)
    }

    /// Whether any term exists under this root key.
    fn namespace_in_use(&self, root: &str) -> bool {
        let prefix = format!("{}/", root);
        self.sites.keys().any(|k| k.starts_with(&prefix))
    }

    /// The namespace roots (each declared kind's key, once per edition) that
    /// have at least one term — a claimed one counts, since the claim page
    /// joins the root listing.
    pub fn roots_in_use(&self) -> impl Iterator<Item = &str> + '_ {
        self.roots.iter().map(|root| root.key.as_str())
    }

    /// The nav-bar entries of the kinds in `nav` (see
    /// `ConfigFile::terms_nav`) that have a root page, in the order given, one
    /// per edition that has a root of the kind, labelled with the kind's name
    /// in that edition (`TermKind::title_in`).
    pub fn nav_entries(&self, nav: &[(String, Option<i32>)]) -> Vec<TermNavEntry> {
        let mut out = Vec::new();
        for (ns, weight) in nav {
            for root in self.roots.iter().filter(|root| self.kinds[root.kind].key == *ns) {
                let label = self.kinds[root.kind].title_in(&root.edition);
                out.push(TermNavEntry { root: root.key.clone(), label, weight: *weight, edition: root.edition.clone() });
            }
        }
        out
    }

    /// Every pseudo-folder that needs a synthetic folder document and an
    /// index page: the unclaimed terms, plus the root of each namespace in
    /// use. A claimed term needs neither, since its claiming page is real,
    /// but its root still does.
    pub fn synthetic_folder_keys(&self) -> Vec<String> {
        self.unclaimed_keys()
            .map(str::to_string)
            .chain(self.roots_in_use().map(str::to_string))
            .collect()
    }

    /// The bare namespace-root keys among [`Self::synthetic_folder_keys`]
    /// (no `/`) whose kind is place-typed — every folder a real `index.md`
    /// can host that kind's term map on. An individual place term
    /// (`places/kyoto`) is reached through its claim instead.
    pub fn place_namespace_roots(&self) -> impl Iterator<Item = &str> + '_ {
        self.roots.iter().filter(|root| self.kinds[root.kind].is_place).map(|root| root.key.as_str())
    }
}


/// A term key as the article map records it, split: `partners/ada-example` is
/// the default edition's, `de/partners/ada-example` the edition `de`'s. Never
/// a namespace root's key: `de/partners` would read as namespace `de`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermKey<'a> {
    /// The edition's tree, `""` for the default edition.
    pub edition: &'a str,
    pub ns: &'a str,
    pub slug: &'a str,
    plain: &'a str,
}

impl<'a> TermKey<'a> {
    /// A slug never contains `/`, so the number of segments tells the two
    /// shapes apart. `None` for anything that is not a term key.
    pub fn parse(key: &'a str) -> Option<Self> {
        let mut parts = key.split('/');
        let (edition, ns, slug) = match (parts.next()?, parts.next()?, parts.next(), parts.next()) {
            (ns, slug, None, _) => ("", ns, slug),
            (edition, ns, Some(slug), None) => (edition, ns, slug),
            _ => return None,
        };
        let plain = if edition.is_empty() { key } else { key.split_once('/').map_or(key, |(_, rest)| rest) };
        Some(Self { edition, ns, slug, plain })
    }

    /// The default edition's key for the same name.
    pub fn plain(&self) -> &'a str {
        self.plain
    }

    /// The key of the namespace root this term is listed on (`de/partners`).
    pub fn root(&self) -> String {
        scope(self.edition, self.ns)
    }
}

impl TermIndex {
    /// The term's page in `edition` (`""` for the default one): the claiming
    /// page, else the generated one.
    pub fn term_url_in(&self, edition: &str, ns: &str, name: &str) -> Option<String> {
        let key = self.kinds.iter().find(|k| k.key == ns)?.key_in(edition, name);
        self.sites.get(&key).map(|site| Self::site_target(&key, site))
    }

    /// Every page of the place-typed kind `namespace`, keyed by term key, for a map dot to link to.
    pub fn place_pages(&self, namespace: &str) -> std::collections::BTreeMap<String, String> {
        let prefix = format!("{namespace}/");
        self.sites
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(key, site)| (key.clone(), Self::site_target(key, site)))
            .collect()
    }

    /// Where a link to this term goes: the claiming page, else its page.
    pub fn site_target(key: &str, site: &TermSite) -> String {
        match &site.claimed_by {
            Some(claim) => format!("/{claim}"), // allow:served-path-url-construct (term link to the claiming page)
            None => format!("/{key}/"), // allow:served-path-url-construct (term link to the generated page)
        }
    }

    /// Where an address from before terms were per edition forwards: the
    /// default edition's page for `plain` (a default-edition term or root
    /// key), else the first edition's, in tree order.
    pub fn forward_target(&self, plain: &str) -> Option<String> {
        let mut editions: Vec<&str> = self.roots.iter().map(|root| root.edition.as_str()).collect();
        editions.sort_unstable();
        editions.dedup();
        editions.into_iter().find_map(|edition| {
            let key = scope(edition, plain);
            match self.sites.get(&key) {
                Some(site) => Some(Self::site_target(&key, site)),
                None => self.roots.iter().any(|root| root.key == key).then(|| format!("/{key}/")), // allow:served-path-url-construct (term root redirect target)
            }
        })
    }
}
