//! Terms per language edition: the edition-scoped term key and the lookups
//! that read it.

use moss_core::terms::term_folder_key;

use super::{TermIndex, TermKind, TermSite};

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
        let plain = if edition.is_empty() { key } else { &key[edition.len() + 1..] };
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
        self.sites.get(&key).map(|site| self.site_target(&key, site))
    }

    /// Where a link to this term goes: the claiming page, else its page.
    pub(super) fn site_target(&self, key: &str, site: &TermSite) -> String {
        match &site.claimed_by {
            Some(claim) => format!("/{claim}"), // allow:served-path-url-construct (term link to the claiming page)
            None => format!("/{key}/"), // allow:served-path-url-construct (term link to the generated page)
        }
    }

    /// Where an address from before terms were per edition forwards: the
    /// default edition's page for `plain` (a default-edition term or root
    /// key), else the first edition's, in tree order.
    pub(super) fn forward_target(&self, plain: &str) -> Option<String> {
        let mut editions: Vec<&str> = self.roots.iter().map(|root| root.edition.as_str()).collect();
        editions.sort_unstable();
        editions.dedup();
        editions.into_iter().find_map(|edition| {
            let key = scope(edition, plain);
            match self.sites.get(&key) {
                Some(site) => Some(self.site_target(&key, site)),
                None => self.roots.iter().any(|root| root.key == key).then(|| format!("/{key}/")), // allow:served-path-url-construct (term root redirect target)
            }
        })
    }
}

/// A key inside an edition's tree: `de` + `partners/ada` is `de/partners/ada`.
/// The default edition (`""`) keeps the plain key.
pub(super) fn scope(edition: &str, key: &str) -> String {
    if edition.is_empty() { key.to_string() } else { format!("{edition}/{key}") }
}

impl TermKind {
    /// The key of the term `name` as a page of `edition` names it
    /// (`de/partners/ada`). A place-typed kind is site-wide, since its map and
    /// gazetteer are keyed by the plain key, so it ignores the edition.
    pub fn key_in(&self, edition: &str, name: &str) -> String {
        scope(if self.is_place { "" } else { edition }, &term_folder_key(&self.key, name))
    }
}
