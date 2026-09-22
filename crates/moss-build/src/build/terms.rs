//! Term derivation: name-list fields (`author:`, `tags:`, and whatever a
//! site declares) become pseudo-folder memberships.
//!
//! Runs once per build, after every document is parsed and before the
//! synthetic folder-index machinery. For each kind's fields it derives a
//! membership claim in the term's pseudo-folder (`authors/<slug>`,
//! `tags/<slug>`, or a declared kind's own namespace) — pushed into the same
//! `also_in` slot an authored cross-listing uses — so the canonical selector
//! (`folder_embed::select_children_by_slug`), the synthetic-index loops in
//! `render/blocking.rs`, and the ADR-044 listing digest all serve term pages
//! with zero new modes. A page claiming a term (`author_page:` / `tag_page:`
//! / `editor_page:` / `jury_page:`) gets the resolved key in
//! `ParsedDocument::term_listing` — the one field the renderer and the
//! listing digest read — and suppresses the generated page for that term.
//! Term identity rules (fold, slug, key) are owned by `moss_core::terms`;
//! this pass only applies them to documents. Design:
//! `docs/archive/2026-09-01-tags-and-authors-design.md`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use moss_core::terms::term_folder_key;
use regex::Regex;

use crate::build::scan::article_map::to_pretty_url;
use crate::build::types::ParsedDocument;

mod kinds;
pub use kinds::{term_kinds, TermKind, BUILTIN_DEFAULT_FIELDS};

/// `[text](url)` and `[[wikilink]]` spans — what [`linked_spans`] scans for.
static LINK_SPAN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[[^\]]*\]\]|\[[^\]]*\]\([^)]*\)").expect("valid regex")
});

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
/// `tags/<slug>`, or a declared kind's own namespace). Built by
/// [`derive_terms`]; consumed by the synthetic-index loops (which keys to
/// seed, what title to show) and by the link pass (where a term name
/// points).
#[derive(Debug, Default)]
pub struct TermIndex {
    sites: BTreeMap<String, TermSite>,
    /// The kinds table this index was derived from. Persisted so a later
    /// reader (the article map, the editor's takeover resolver) sees the
    /// same kinds `derive_terms` used, instead of re-reading config. Read
    /// through [`Self::kinds`], not directly — same reasoning as `sites`.
    kinds: Vec<TermKind>,
    /// Term key, field name → the member `url_path`s that field claims, in
    /// scan order. Recorded in `derive_terms`'s membership pass at the same
    /// point a member's `also_in` entry is pushed, so the two can never
    /// disagree about who's a member. Feeds `ParsedDocument::term_sections`
    /// and the article map's per-field summary. Read through
    /// [`Self::members_by_field`].
    members_by_field: BTreeMap<(String, String), Vec<String>>,
    /// Term key → that term's listing split by field, resolved once in
    /// `derive_terms`'s second pass and shared by both pages that can host
    /// the listing: a claiming page reads its own copy off
    /// `ParsedDocument::term_sections`, and a generated term page — which
    /// has no document until the render layer synthesizes one — reads it
    /// here through [`Self::sections`]. One computation, two readers, so
    /// the claimed and unclaimed pages of the same term cannot disagree
    /// about which field a member came through.
    sections_by_term: BTreeMap<String, Vec<(Option<String>, Vec<String>)>>,
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

    /// Display title for a term pseudo-folder key. `None` for a namespace
    /// root, which is no term: its heading is the owning kind's own
    /// resolved `title` (see [`Self::kind_for`]).
    pub fn display(&self, key: &str) -> Option<&str> {
        self.sites.get(key).map(|s| s.display.as_str())
    }

    /// The kind whose namespace root this key is (`key == kind.key`), if
    /// any — `title` is already final (resolved once by `term_kinds`, never
    /// read with a fallback here).
    ///
    /// Only for a namespace this build actually put terms in. A kind exists
    /// whether or not anything feeds it (the built-ins survive their
    /// dimension being switched off, with an empty `fields`), and a site
    /// with `[terms] author = false` may well have a real directory called
    /// `authors/` — which keeps its own name, rather than being retitled by
    /// a namespace nothing is in.
    pub fn kind_for(&self, key: &str) -> Option<&TermKind> {
        self.namespace_in_use(key).then(|| self.kinds.iter().find(|k| k.key == key)).flatten()
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

    /// The kinds table this index was derived from — what the article map
    /// persists (`ArticleMap::kinds`) so the editor's takeover resolver sees
    /// the same kinds `derive_terms` used instead of re-reading config.
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
    /// trailing unlabelled group — the same value a claiming page carries
    /// in `ParsedDocument::term_sections`. The render layer's generated
    /// term page reads it from here because it has no document of its own.
    pub fn sections(&self, term_key: &str) -> Option<&[(Option<String>, Vec<String>)]> {
        self.sections_by_term.get(term_key).map(Vec::as_slice)
    }

    /// Whether any term exists in this namespace (drives whether the
    /// namespace root index is worth emitting).
    fn namespace_in_use(&self, ns: &str) -> bool {
        let prefix = format!("{}/", ns);
        self.sites.keys().any(|k| k.starts_with(&prefix))
    }

    /// The namespace roots (every declared kind's key) that have at least
    /// one term — a claimed one counts, since the claim page joins the root
    /// listing.
    pub fn roots_in_use(&self) -> impl Iterator<Item = &str> + '_ {
        self.kinds.iter().map(|k| k.key.as_str()).filter(move |ns| self.namespace_in_use(ns))
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

/// This doc's values for a name-list field (`author`, `tags`, `editor`,
/// `jury`), or an empty slice for a field name this build doesn't
/// recognize. The one accessor every name-list-field consumer in this
/// module goes through, so a schema addition needs exactly one new arm —
/// pinned by `every_name_list_field_has_a_real_accessor_arm`, which fails on
/// a wildcard silently standing in for a real one.
fn field_names<'a>(doc: &'a ParsedDocument, field: &str) -> &'a [String] {
    match field {
        "author" => &doc.author,
        // Frontmatter tags only — inline #hashtags are prose, not
        // cataloguing (see `fm_tags` on ParsedDocument).
        "tags" => doc.fm_tags.as_deref().unwrap_or(&[]),
        "editor" => &doc.editor,
        "jury" => &doc.jury,
        _ => &[],
    }
}

/// This doc's term-page claim for a name-list field (`author_page`,
/// `tag_page`, `editor_page`, `jury_page`), or `None` for a field name this
/// build doesn't recognize. Companion to [`field_names`].
fn term_claim_of<'a>(doc: &'a ParsedDocument, field: &str) -> Option<&'a moss_core::terms::TermClaim> {
    match field {
        "author" => doc.author_page.as_ref(),
        "tags" => doc.tag_page.as_ref(),
        "editor" => doc.editor_page.as_ref(),
        "jury" => doc.jury_page.as_ref(),
        _ => None,
    }
}

/// The frontmatter key a page claims this field's term page with. Almost
/// always `<field>_page`; `tags` is the one irregular pair, because the
/// field is plural and a claim is of one tag (`tag_page`). Anything that
/// WRITES a claim goes through here rather than concatenating the suffix,
/// which is how `tags_page` — a key nothing reads — got written once.
pub fn claim_field_key(field: &str) -> Option<&'static str> {
    match field {
        "author" => Some("author_page"),
        "tags" => Some("tag_page"),
        "editor" => Some("editor_page"),
        "jury" => Some("jury_page"),
        _ => None,
    }
}

/// Derive term memberships and claims from a build's kinds table. Mutates
/// documents in place:
///
/// - every field value of every kind pushes its pseudo-folder key into the
///   doc's `also_in` slot (skipping the doc that claims that very term, so a
///   term page does not list itself), and records the member's `url_path`
///   under `TermIndex::members_by_field`;
/// - the doc that WINS a term claim gets the term key in `term_listing` so it
///   hosts the member listing, unless the author already routed `children`
///   somewhere explicitly (their routing wins; the claim still resolves), and
///   gets `term_sections` — its listing split by field, plus a trailing
///   group for anything reachable only through a hand-authored `also_in:`.
///
/// Duplicate claims on one term: the lexicographically first `url_path` wins
/// (deterministic across builds), the loser is logged. A claim on a term no
/// page carries still creates the term (the claim IS an occurrence — an
/// author page for someone with no published works yet is valid and lists
/// nothing).
pub fn derive_terms(documents: &mut [ParsedDocument], kinds: Vec<TermKind>) -> TermIndex {
    let mut index = TermIndex::default();

    // Pass 1: claims. Resolve every *_page claim to its term key so pass 2
    // can exclude the claiming page from its own listing.
    for doc in documents.iter() {
        for kind in &kinds {
            for field in &kind.fields {
                let Some(claim) = term_claim_of(doc, field) else { continue };
                let name = claim.name(&doc.title).trim();
                let Some(key) = resolved_key(&kind.key, name) else { continue };
                let site = index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                    display: name.to_string(),
                    claimed_by: None,
                });
                // First URL wins, so the result is scan-order independent.
                let claim = to_pretty_url(&doc.url_path);
                match &site.claimed_by {
                    None => site.claimed_by = Some(claim),
                    Some(existing) if *existing <= claim => {
                        crate::build::cli_output::log_warn_problem!(
                            "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                            name, existing, claim
                        );
                    }
                    Some(existing) => {
                        crate::build::cli_output::log_warn_problem!(
                            "duplicate term claim: '{}' is already claimed by /{}; ignoring the claim on /{}",
                            name, claim, existing
                        );
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
    // `authors/馬欣宜`. A dedicated resolved field keeps the render layer and
    // the ADR-044 listing digest reading one unambiguous value.
    for doc in documents.iter_mut() {
        let claim_key = claimed_key(doc, &kinds, &index);
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

    // Pass 2: memberships. Every field value of every kind claims membership
    // in its pseudo-folder, beside any authored `also_in` — and records which
    // field it came through, in `members_by_field`.
    for doc in documents.iter_mut() {
        let own_claim = claimed_key(doc, &kinds, &index);
        let doc_url = to_pretty_url(&doc.url_path);
        // Dedup `also_in`: a page naming the same term through more than one
        // field of one kind (`author:` and `editor:` both crediting the same
        // person) must still occupy that pseudo-folder's membership list only
        // once — the per-field split still records each field's own
        // contribution in `members_by_field` below, which is a different
        // question ("which field did this member come through") that the
        // dedup here doesn't touch.
        let mut also_in_pushed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for kind in &kinds {
            for field in &kind.fields {
                let names = field_names(doc, field).to_vec();
                for name in &names {
                    let name = name.trim();
                    let Some(key) = resolved_key(&kind.key, name) else { continue };
                    // First-seen original case is the display form; later
                    // case variants merge into the same key by construction.
                    index.sites.entry(key.clone()).or_insert_with(|| TermSite {
                        display: name.to_string(),
                        claimed_by: None,
                    });
                    if own_claim.as_deref() == Some(key.as_str()) {
                        continue; // a term page never lists itself
                    }
                    if also_in_pushed.insert(key.clone()) {
                        doc.also_in.get_or_insert_with(Vec::new).push(key.clone());
                    }
                    index
                        .members_by_field
                        .entry((key, field.clone()))
                        .or_default()
                        .push(doc_url.clone());
                }
            }
        }
    }

    // Second small pass: resolve `term_sections` for every claiming page,
    // now that `members_by_field` is complete. A reverse scan of every
    // document's now-final `also_in` finds hand-authored cross-listing
    // (`also_in: [...]`) that names a term directly — membership no field
    // record claims — so it can still appear, in a trailing unlabelled
    // group, rather than being silently dropped.
    let mut all_members_by_term: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for doc in documents.iter() {
        let Some(also_in) = &doc.also_in else { continue };
        for key in also_in {
            if key.contains('/') {
                all_members_by_term.entry(key.clone()).or_default().push(to_pretty_url(&doc.url_path));
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
        let Some((ns, _)) = term_key.split_once('/') else { continue };
        let Some(kind) = kinds.iter().find(|k| k.key == ns) else { continue };
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
    for doc in documents.iter_mut() {
        let Some(term_key) = doc.term_listing.as_deref() else { continue };
        doc.term_sections = index.sections_by_term.get(term_key).cloned();
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
                crate::build::cli_output::log_warn_problem!(
                    "term page '{}' ({}) has no member pages — if that is unexpected, check the claimed name for typos",
                    site.display, claimer
                );
            }
        }
    }

    index.kinds = kinds;
    index
}

/// Make declared names in byline rows clickable (design rule 3).
///
/// For each page, every name-list field's value across every kind this
/// build's site declares — `author:`, and whatever else a kind names
/// (`editor:`, `jury:`, ...) — that appears verbatim in a `byline:` row
/// becomes a markdown link to that name's term page. Byline rows render
/// through the one inline-markdown path (`render/credits.rs`), so a link is
/// injected as markdown, not HTML. The byline stays the author's display
/// text ("no machine claim"); only names ALSO declared in a name-list field
/// are linked, and only where the author typed them.
///
/// Six guards:
/// 1. only a declared name is ever a candidate — free text in a byline
///    never becomes a link;
/// 2. a name whose occurrence already sits inside a markdown or wikilink
///    span is left alone (byte-range check, [`linked_spans`] — not the old
///    exact-substring `[Name]` check, which missed a co-credit inside one
///    link, `[Name, ed.](url)`, and a wikilink, `[[Name]]`, alike);
/// 3. longest name first, so "馬欣" never claims the middle of "馬欣宜"
///    when both are credited;
/// 4. a Latin-script name never matches glued inside a longer Latin word
///    ("Alex" inside "Alexander") — [`find_bounded`] requires no such
///    boundary at a Han/kana/hangul edge, where names commonly sit with no
///    separator at all ("谷夏攝" still links);
/// 5. a single-character name never auto-links — it can still be linked by
///    the author's own explicit markdown, which guard 2 leaves alone;
/// 6. the same declared name resolving from more than one field/kind warns
///    once, only if it actually matches a row (harmless otherwise); a
///    declared name that never appears in any byline row warns once, so a
///    typo between the frontmatter and the byline text reads as a build
///    diagnostic instead of a silently absent link.
///
/// Runs after [`derive_terms`] and before the render fingerprints are taken,
/// so a claim appearing anywhere moves the linked URL and with it the page's
/// surface — invalidation is free.
///
/// Renamed from `link_authors_in_bylines` in task A4 (a bare rename; the
/// candidate set was still `author:`-only there). This task widens it to
/// every kind's fields and adds guards 2 (fixed), 4, 5 and 6.
pub fn link_terms_in_bylines(documents: &mut [ParsedDocument], index: &TermIndex) {
    for doc in documents.iter_mut() {
        if doc.byline.is_empty() {
            continue;
        }
        // Every name-list field's values, across every kind this build's
        // site declares — not just author:. A name may legitimately repeat
        // across fields/kinds; guard 6 warns about that, once, only when it
        // actually resolves a link in some row.
        let mut declared: Vec<String> = Vec::new();
        for kind in &index.kinds {
            for field in &kind.fields {
                declared.extend(field_names(doc, field).iter().cloned());
            }
        }
        if declared.is_empty() {
            continue;
        }
        let mut occurrence_count: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for name in &declared {
            *occurrence_count.entry(name.as_str()).or_insert(0) += 1;
        }

        // Guard 3: longest name first, so a shorter co-credit never claims
        // the middle of a longer one sharing its prefix. Deduplicated so one
        // repeated declaration doesn't get matched (and warned about) twice.
        let mut unique_names: Vec<&str> = Vec::new();
        for name in &declared {
            if !unique_names.contains(&name.as_str()) {
                unique_names.push(name.as_str());
            }
        }
        unique_names.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));

        let own_url = format!("/{}", doc.url_path.trim_end_matches("index.html")); // allow:served-path-url-construct (self-link comparison only, never emitted)
        let links: Vec<(&str, String)> = unique_names
            .into_iter()
            .filter_map(|name| {
                let name = name.trim();
                // Markdown link syntax cannot carry these verbatim in link text.
                if name.is_empty() || name.contains(['[', ']', '(', ')']) {
                    return None;
                }
                // Guard 5: a single-character name never auto-links.
                if name.chars().count() == 1 {
                    return None;
                }
                // None: no kind's namespace has a term for this name.
                let url = index.kinds.iter().find_map(|kind| index.term_url(&kind.key, name))?;
                if url == own_url {
                    return None; // the term page itself never links to itself
                }
                Some((name, url))
            })
            .collect();

        let mut warned_duplicate: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut mentioned: std::collections::HashSet<&str> = std::collections::HashSet::new();

        for row in doc.byline.iter_mut() {
            let spans = linked_spans(row);
            // All match positions are found on the ORIGINAL row, longest name
            // first and non-overlapping, and the linked text is assembled
            // once at the end — so a shorter co-author name ("馬欣") can never
            // match inside the link already injected for a longer one
            // ("馬欣宜") and nest markdown.
            let mut matches: Vec<(usize, &str, &str)> = Vec::new();
            for (name, url) in &links {
                let Some((pos, end)) = find_bounded(row, name) else { continue };
                mentioned.insert(*name);
                // Guard 2: a name whose occurrence already sits inside a
                // linked span (markdown or wikilink) is left alone — the
                // author already linked it, in whatever form they chose.
                if spans.iter().any(|(s, e)| pos < *e && *s < end) {
                    continue;
                }
                let overlaps = matches.iter().any(|(p, n, _)| pos < *p + n.len() && *p < end);
                if overlaps {
                    continue;
                }
                if occurrence_count.get(name).copied().unwrap_or(0) > 1 && warned_duplicate.insert(name)
                {
                    crate::build::cli_output::log_warn_problem!(
                        "'{}' is declared more than once on {}; byline-linked from the first resolvable field",
                        name, doc.url_path
                    );
                }
                matches.push((pos, name, url.as_str()));
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

        // Guard 6, second half: a declared name that never showed up in any
        // byline row at all — not merely left unlinked by another guard —
        // is very likely a typo between the frontmatter and the byline text.
        for (name, _) in &links {
            if !mentioned.contains(name) {
                crate::build::cli_output::log_warn_problem!(
                    "'{}' is declared on {} but never appears in a byline row",
                    name, doc.url_path
                );
            }
        }
    }
}

/// Byte ranges of already-linked text in a row: `[text](url)` and
/// `[[wikilink]]` spans. Guard 2 in [`link_terms_in_bylines`].
fn linked_spans(row: &str) -> Vec<(usize, usize)> {
    LINK_SPAN_RE.find_iter(row).map(|m| (m.start(), m.end())).collect()
}

/// The first byte range where `name` occurs in `row` with a real boundary
/// on both sides. Guard 4 in [`link_terms_in_bylines`]: a Latin-script edge
/// (`Alex` glued inside `Alexander`) is rejected — the adjacent character
/// must not itself be an ASCII word character; a Han/kana/hangul (or any
/// other non-ASCII) edge is not rejected, since CJK text has no
/// space-separated word boundary to require in the first place.
fn find_bounded(row: &str, name: &str) -> Option<(usize, usize)> {
    row.match_indices(name).find_map(|(pos, matched)| {
        let end = pos + matched.len();
        let before_ok =
            row[..pos].chars().next_back().is_none_or(|c| !c.is_ascii_alphanumeric());
        let after_ok = row[end..].chars().next().is_none_or(|c| !c.is_ascii_alphanumeric());
        (before_ok && after_ok).then_some((pos, end))
    })
}

/// Pseudo-folder key for a term name, or `None` for a name with no
/// alphanumeric characters at all (empty, or punctuation-only like "！！！").
/// Such names have no slug of their own — `generate_slug` falls back to
/// "untitled", which would silently merge every punctuation-only value into
/// one shared `authors/untitled` term.
fn resolved_key(ns: &str, name: &str) -> Option<String> {
    if !name.chars().any(char::is_alphanumeric) {
        if !name.is_empty() {
            crate::build::cli_output::log_warn_problem!(
                "term name '{}' has no sluggable characters; skipping", name
            );
        }
        return None;
    }
    Some(term_folder_key(ns, name))
}

/// The pseudo-folder key this doc claims, if any, across every kind's
/// fields (the first field to resolve one wins — a page can be one term's
/// page, and two claims on one page is an authoring mistake worth a
/// warning).
/// Split a term page's already-fetched member slice into the groups its
/// resolved `sections` describe, dropping every group no member landed in.
///
/// `None` means "nothing to split": one surviving group, or none. The caller
/// then renders the member slice it already has, untouched — which is what
/// keeps a single-field kind's page byte-identical to a plain folder index,
/// heading and all. Deciding that here rather than at each render site is
/// what makes the rule one rule instead of two.
///
/// Takes no [`TermIndex`]: `sections` is a resolved value (a claiming page's
/// `term_sections`, or [`TermIndex::sections`] for a generated one) and
/// `members` is whatever the canonical child selector returned, already
/// sorted and limited. Group order follows `sections`; within a group,
/// member order follows `members`, so grouping never re-sorts.
pub fn term_member_groups<'a, 's>(
    sections: &'s [(Option<String>, Vec<String>)],
    members: &[&'a ParsedDocument],
) -> Option<Vec<(Option<&'s str>, Vec<&'a ParsedDocument>)>> {
    let mut groups: Vec<(Option<&'s str>, Vec<&'a ParsedDocument>)> = Vec::new();
    for (field, urls) in sections {
        let urls: std::collections::HashSet<&str> = urls.iter().map(String::as_str).collect();
        let docs: Vec<&'a ParsedDocument> = members
            .iter()
            .copied()
            .filter(|d| urls.contains(to_pretty_url(&d.url_path).as_str()))
            .collect();
        if docs.is_empty() {
            continue;
        }
        groups.push((field.as_deref(), docs));
    }
    // LABELLED groups decide this, not all of them. The trailing group is
    // whatever a hand-authored `also_in:` dragged in, and it exists on sites
    // that declare no kind at all — counting it would put "Author" over the
    // derived half of a one-field listing that develop renders flat.
    (groups.iter().filter(|(field, _)| field.is_some()).count() > 1).then_some(groups)
}

/// Render a term page's listing: one `<h2 class="moss-term-role">` per
/// labelled group, the trailing hand-authored group unlabelled and last, and
/// `None` when [`term_member_groups`] found nothing worth splitting — at
/// which point the caller renders the member slice it already has.
///
/// One function because there are two term pages for one term (the claiming
/// page, in `folder_embed`, and the generated one, in `render::blocking`)
/// and they must not drift. They already had: the generated page was reading
/// the FOLDER's language for chrome the site language owns.
pub fn render_term_sections<'a>(
    sections: Option<&[(Option<String>, Vec<String>)]>,
    members: &[&'a ParsedDocument],
    lang: crate::i18n::Language,
    render: impl Fn(&[&'a ParsedDocument]) -> String,
) -> Option<String> {
    let groups = term_member_groups(sections?, members)?;
    Some(
        groups
            .iter()
            .map(|(field, docs)| match field {
                Some(field) => format!("{}\n{}", term_role_heading(lang, field), render(docs)),
                None => render(docs),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Old-URL → new-URL pairs for every term whose namespace moved when a
/// declared kind took a built-in's default field: naming `author` in
/// `[terms.people]` moves everyone out of `authors/<slug>/` and into the
/// `people` namespace, and the old URL is already out there in links and
/// search results.
///
/// Recomputed from the kinds table on every build and never stored. There is
/// no move event to record and nothing to merge: the pairs exist exactly
/// while the config says the field belongs to another kind. Revert the
/// config and the next build simply stops emitting them — a build writes a
/// fresh generation rather than editing the last one, so the stub is not
/// left behind locally, and a published site's old URL is retracted by the
/// deploy pipeline's own removal of outputs that no page backs any more.
/// This function is only one of that mechanism's inputs; it adds nothing to
/// it. For the same reason none of this touches `redirects.json`, whose
/// rename detection is UID-based and structurally cannot see a namespace
/// move — a term has no UID and is not in `.articles`.
///
/// URLs are in the format the redirect emitter already uses: pretty, with a
/// trailing slash and no leading one for the source, root-relative for the
/// target.
pub fn kind_move_stubs(index: &TermIndex) -> Vec<(String, String)> {
    let mut stubs = Vec::new();
    for (builtin_ns, field) in BUILTIN_DEFAULT_FIELDS {
        // Moved, rather than merely switched off: the built-in has lost the
        // field AND some declared kind has gained it. `[terms] author =
        // false` empties the built-in too, and owes no redirect — those
        // pages are gone, not relocated.
        let builtin_kept_it =
            index.kinds.iter().any(|k| k.key == builtin_ns && k.fields.iter().any(|f| f == field));
        if builtin_kept_it {
            continue;
        }
        let Some(new_kind) = index
            .kinds
            .iter()
            .find(|k| k.key != builtin_ns && k.fields.iter().any(|f| f == field))
        else {
            continue;
        };
        for (key, site) in &index.sites {
            let Some((ns, slug)) = key.split_once('/') else { continue };
            if ns != new_kind.key {
                continue;
            }
            // Reachable through the moved field specifically. A person in
            // this namespace only through `editor:` never had an `authors/`
            // URL, so forwarding one would invent a page that never existed.
            let reached_through_moved_field = index
                .members_by_field
                .get(&(key.clone(), field.to_string()))
                .is_some_and(|members| !members.is_empty());
            if !reached_through_moved_field {
                continue;
            }
            let Some(target) = index.term_url(&new_kind.key, &site.display) else { continue };
            stubs.push((format!("{}/{}/", builtin_ns, slug), target));
        }
    }
    stubs
}

/// The `<h2>` above one field's group on a term page — "Editor" over the
/// pages that named this person through `editor:`. The class is in
/// moss-core's component table; the copy is `term_role_<field>` in
/// `i18n::strings`.
pub fn term_role_heading(lang: crate::i18n::Language, field: &str) -> String {
    format!(
        "<h2 class=\"moss-term-role\">{}</h2>",
        crate::build::media::cover::html_escape(crate::i18n::t(lang, &format!("term_role_{}", field)))
    )
}

fn claimed_key(doc: &ParsedDocument, kinds: &[TermKind], index: &TermIndex) -> Option<String> {
    let mut found: Option<String> = None;
    for kind in kinds {
        for field in &kind.fields {
            let Some(claim) = term_claim_of(doc, field) else { continue };
            let name = claim.name(&doc.title).trim();
            let Some(key) = resolved_key(&kind.key, name) else {
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
                    crate::build::cli_output::log_warn_problem!(
                        "page {} claims more than one term page; keeping {}",
                        doc.url_path, first
                    );
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::terms::{TermClaim, AUTHOR_NS, TAGS_NS};

    fn doc(url: &str, title: &str) -> ParsedDocument {
        ParsedDocument {
            url_path: url.to_string(),
            title: title.to_string(),
            ..Default::default()
        }
    }

    /// The two built-in kinds, both on — what `derive_terms(&mut docs, true,
    /// true)` used to mean before the `authors_on`/`tags_on` bools became a
    /// kinds table (task A4).
    fn both_kinds() -> Vec<TermKind> {
        vec![
            TermKind { key: AUTHOR_NS.to_string(), fields: vec!["author".to_string()], title: "Authors".to_string() },
            TermKind { key: TAGS_NS.to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string() },
        ]
    }

    #[test]
    fn synthetic_folders_are_the_unclaimed_terms_plus_the_roots_in_use() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["馬欣宜".into(), "Scarly".into()];
        docs[1].fm_tags = Some(vec!["城市".into()]);
        let index = derive_terms(&mut docs, both_kinds());
        // The claimed term is a real page; its root is still synthesized.
        assert_eq!(
            index.synthetic_folder_keys(),
            vec!["authors/scarly", "tags/城市", "authors", "tags"]
        );
        // A kind with no terms (author value present, but tags never used
        // here) has no root of its own — folded from the old
        // disabled_dimension_derives_nothing/author_value_derives_pseudo_folder_membership
        // pair (A2's built_in_kind_off_by_config_derives_nothing now covers
        // the "config turns a built-in off" half).
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Scarly".into()];
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(index.synthetic_folder_keys(), vec!["authors/scarly", "authors"]);
    }

    #[test]
    fn claim_replaces_generated_page_and_hosts_the_listing() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["馬欣宜".into()];
        let index = derive_terms(&mut docs, both_kinds());
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
        derive_terms(&mut docs, both_kinds());
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
        let index1 = derive_terms(&mut [a.clone(), b.clone()], both_kinds());
        let index2 = derive_terms(&mut [b, a], both_kinds());
        assert_eq!(index1.term_url("authors", "X").as_deref(), Some("/a/"));
        assert_eq!(index2.term_url("authors", "X").as_deref(), Some("/a/"));
    }

    #[test]
    fn explicit_children_routing_wins_over_claim_lowering() {
        let mut docs = vec![doc("about/ma/index.html", "馬欣宜")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].children_source = Some("[[News]]".into());
        let index = derive_terms(&mut docs, both_kinds());
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
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
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
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
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
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜[馬欣宜](/authors/馬欣宜/)");
        assert_eq!(docs[0].byline[1], "編｜[馬欣](/authors/馬欣/)");
    }

    #[test]
    fn punctuation_only_names_derive_no_term() {
        let mut docs = vec![doc("posts/a/index.html", "！！！")];
        docs[0].author = vec!["！！！".into()];
        docs[0].author_page = Some(TermClaim::UseTitle);
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert!(docs[0].also_in.is_none());
        assert!(docs[0].term_listing.is_none());
    }

    #[test]
    fn byline_rows_already_linked_by_the_author_are_left_alone() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Scarly".into()];
        docs[0].byline = vec!["by [Scarly](https://example.com)".into()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "by [Scarly](https://example.com)");
    }

    // The three pure `term_kinds()` tests (built-in-off, declared-moves-a-field,
    // unknown-field-is-a-diagnostic) moved to `terms/kinds.rs`'s own test
    // module with the code they test. This one integration test stays here:
    // it needs `derive_terms`/`TermIndex::roots_in_use` and the `doc()`
    // helper, both residents of this file, to prove the kinds table and the
    // derivation pass agree that a declared built-in key is one root, not two.

    /// Thermo-review must-fix: a declared `[terms.tags]` (or `[terms.authors]`)
    /// table used to be APPENDED after the unconditional built-in `RawKind`
    /// for the same key, so `term_kinds` produced two `TermKind`s sharing one
    /// key — `roots_in_use` and `synthetic_folder_keys` would seed the same
    /// pseudo-folder twice, and `members_by_field` would double-count every
    /// member. Declaring a built-in key must replace it, not duplicate it.
    #[test]
    fn declaring_a_built_in_key_replaces_it_rather_than_duplicating_it() {
        let cfg = crate::config::ConfigFile::parse(
            "[terms.tags]\nfields = [\"tags\"]\ntitle = \"Topics\"\n",
        )
        .unwrap();

        let raw = cfg.terms_kinds();
        let tags_raw: Vec<_> = raw.iter().filter(|k| k.key == TAGS_NS).collect();
        assert_eq!(tags_raw.len(), 1, "exactly one raw kind for the declared key: {raw:?}");

        let kinds = term_kinds(&cfg, crate::i18n::Language::En);
        let tags_kinds: Vec<_> = kinds.iter().filter(|k| k.key == TAGS_NS).collect();
        assert_eq!(tags_kinds.len(), 1, "exactly one resolved kind for the declared key: {kinds:?}");
        assert_eq!(tags_kinds[0].fields, vec!["tags".to_string()]);
        assert_eq!(tags_kinds[0].title, "Topics", "the declared title wins over the i18n default");

        // roots_in_use must not repeat a key derive_terms would otherwise
        // seed twice.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].fm_tags = Some(vec!["essays".to_string()]);
        let index = derive_terms(&mut docs, kinds);
        let roots: Vec<&str> = index.roots_in_use().collect();
        let unique: std::collections::HashSet<&str> = roots.iter().copied().collect();
        assert_eq!(roots.len(), unique.len(), "roots_in_use must not repeat a key: {roots:?}");
        assert_eq!(roots, vec![TAGS_NS], "the declared [terms.tags] is the only in-use root");
    }

    #[test]
    fn case_variants_merge_into_one_term_with_first_seen_display() {
        let mut docs = vec![doc("p1/index.html", "A"), doc("p2/index.html", "B")];
        docs[0].fm_tags = Some(vec!["City".into()]);
        docs[1].fm_tags = Some(vec!["city".into()]);
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(index.unclaimed_keys().count(), 1);
        assert_eq!(index.display("tags/city"), Some("City"));
    }

    // ── task A4: the cutover ─────────────────────────────────────────────

    #[test]
    fn claim_through_jury_page_hosts_the_listing() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["jury".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("about/kane/index.html", "Kane"), doc("posts/a/index.html", "A")];
        docs[0].jury_page = Some(TermClaim::UseTitle);
        docs[1].jury = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(docs[0].term_listing.as_deref(), Some("people/kane"));
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"people/kane".to_string()));
    }

    #[test]
    fn members_by_field_records_which_field_a_member_came_through() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        docs[0].editor = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(
            index.members_by_field.get(&("people/ada-lin".to_string(), "author".to_string())),
            Some(&vec!["posts/a/".to_string()])
        );
        assert_eq!(
            index.members_by_field.get(&("people/kane".to_string(), "editor".to_string())),
            Some(&vec!["posts/a/".to_string()])
        );
        // Cross-checked against the wrong field: a member recorded under
        // "author" must not also show up under "editor" for the same term.
        assert_eq!(
            index.members_by_field.get(&("people/ada-lin".to_string(), "editor".to_string())),
            None
        );
    }

    #[test]
    fn term_sections_keeps_a_trailing_group_for_hand_authored_also_in() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("about/chen/index.html", "陳小華"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        // A plain also_in cross-list — no `author:` value on either page
        // names 陳小華 — must still surface in the trailing group, not be
        // silently dropped.
        docs[1].also_in = Some(vec!["people/陳小華".to_string()]);
        derive_terms(&mut docs, kinds);
        let sections = docs[0].term_sections.as_ref().expect("the claiming page gets term_sections");
        assert_eq!(sections.len(), 2, "one field group + one trailing group: {sections:?}");
        assert_eq!(sections[0].0.as_deref(), Some("author"));
        assert!(sections[0].1.is_empty(), "nobody's author: field names 陳小華: {sections:?}");
        assert_eq!(sections[1].0, None, "the trailing group carries no field label");
        assert_eq!(sections[1].1, vec!["posts/a/".to_string()]);
    }

    #[test]
    fn every_name_list_field_has_a_real_accessor_arm() {
        // Companion to A2's name_list_fields_is_exactly_the_four_name_list_fields:
        // that test pins the SET; this one pins that `field_names` has a real
        // arm for each member of it, so a `_ => &[]` wildcard could not pass
        // as a match for a name the set still yields.
        let mut d = doc("posts/a/index.html", "A");
        d.author = vec!["Ada Lin".to_string()];
        d.fm_tags = Some(vec!["essays".to_string()]);
        d.editor = vec!["Kane".to_string()];
        d.jury = vec!["Kaneda".to_string()];
        for field in moss_core::schema_fields::name_list_fields() {
            assert!(
                !field_names(&d, field).is_empty(),
                "field_names(doc, \"{field}\") returned an empty slice — a wildcard arm \
                 could also produce that, so this isn't proof of a real accessor"
            );
        }
    }

    // ── task A9: per-field sections on a term page ───────────────────────

    #[test]
    fn a_kind_whose_namespace_is_unused_does_not_title_a_real_folder() {
        // `[terms] author = false` leaves the built-in kind in the table with
        // no fields. A real `authors/` directory on that site is an ordinary
        // folder and keeps its own name.
        let off = vec![TermKind {
            key: "authors".to_string(),
            fields: Vec::new(),
            title: "作者".to_string(),
        }];
        let mut docs = vec![doc("authors/index.html", "authors")];
        let index = derive_terms(&mut docs, off);
        assert!(
            index.kind_for("authors").is_none(),
            "an empty namespace must not lend its title to a real folder"
        );

        // The same kind, switched on and in use, does title its root.
        let on = vec![TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "作者".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, on);
        assert_eq!(index.kind_for("authors").map(|k| k.title.as_str()), Some("作者"));
    }

    #[test]
    fn a_hand_authored_also_in_does_not_earn_the_page_a_heading() {
        // An undeclared site — the built-in one-field `authors` kind — with a
        // page cross-listed onto a term by hand. That page lands in the
        // trailing unlabelled group, so counting groups would see two and
        // put "Author" over the derived half. develop renders one flat list,
        // and every such site's output is byte-identical or it is not.
        let kinds = vec![TermKind {
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A"), doc("posts/b/index.html", "B")];
        docs[0].author = vec!["Ada Lin".to_string()];
        docs[1].also_in = Some(vec!["authors/ada-lin".to_string()]);
        let index = derive_terms(&mut docs, kinds);
        let members: Vec<&ParsedDocument> = docs.iter().collect();
        let sections = index.sections("authors/ada-lin").expect("every term gets sections");
        assert!(
            term_member_groups(sections, &members).is_none(),
            "one labelled group plus a hand-authored trailer is still one flat listing"
        );
    }

    #[test]
    fn one_field_with_members_is_not_worth_splitting() {
        // Three declared fields, one of them used: the page renders as a
        // plain listing with no heading, which is what keeps a single-field
        // kind byte-identical to any other folder index.
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string(), "jury".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A"), doc("posts/b/index.html", "B")];
        docs[0].author = vec!["Ada Lin".to_string()];
        docs[1].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, kinds);
        let members: Vec<&ParsedDocument> = docs.iter().collect();
        let sections = index.sections("people/ada-lin").expect("every term gets sections");
        assert!(term_member_groups(sections, &members).is_none());
    }

    #[test]
    fn two_fields_with_members_split_into_labelled_groups_in_kind_order() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string(), "jury".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("seasons/one/index.html", "One"), doc("posts/a/index.html", "A")];
        docs[0].jury = vec!["Ada Lin".to_string()];
        docs[1].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, kinds);
        let members: Vec<&ParsedDocument> = docs.iter().collect();
        let sections = index.sections("people/ada-lin").expect("every term gets sections");
        let groups = term_member_groups(sections, &members).expect("two fields, two groups");
        let shape: Vec<(Option<&str>, Vec<&str>)> = groups
            .iter()
            .map(|(f, docs)| (*f, docs.iter().map(|d| d.url_path.as_str()).collect()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (Some("author"), vec!["posts/a/index.html"]),
                (Some("jury"), vec!["seasons/one/index.html"]),
            ],
            "groups follow kind.fields order, `editor` drops out with no members"
        );
    }

    #[test]
    fn a_generated_term_page_reads_the_same_sections_its_claimed_twin_would() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "jury".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A"), doc("seasons/one/index.html", "One")];
        docs[0].author = vec!["Kane".to_string()];
        docs[1].jury = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(index.unclaimed_keys().collect::<Vec<_>>(), vec!["people/kane"]);
        assert_eq!(
            index.sections("people/kane"),
            Some(
                [
                    (Some("author".to_string()), vec!["posts/a/".to_string()]),
                    (Some("jury".to_string()), vec!["seasons/one/".to_string()]),
                    (None, Vec::new()),
                ]
                .as_slice()
            ),
            "the index carries the split for a term no page claims"
        );
    }

    #[test]
    fn a_terms_two_pages_render_the_same_headings() {
        // The claimed page and the generated page of one term go through two
        // different listing pipelines. They read the split from two places —
        // the claiming document, and the index — and both render it here, so
        // the same kind cannot come out headed differently on the two.
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![
            doc("ada-lin/index.html", "Ada Lin"),
            doc("posts/a/index.html", "A"),
            doc("posts/b/index.html", "B"),
        ];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["Ada Lin".to_string()];
        docs[2].editor = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, kinds);
        let members: Vec<&ParsedDocument> = docs[1..].iter().collect();
        let render = |docs: &[&ParsedDocument]| {
            docs.iter().map(|d| d.url_path.as_str()).collect::<Vec<_>>().join(",")
        };

        let lang = crate::i18n::Language::ZhHant;
        // The claiming page's own copy, and the index's.
        let claimed = render_term_sections(docs[0].term_sections.as_deref(), &members, lang, render);
        let generated = render_term_sections(index.sections("people/ada-lin"), &members, lang, render);
        assert_eq!(claimed, generated);
        assert_eq!(
            claimed.unwrap(),
            "<h2 class=\"moss-term-role\">作者</h2>\nposts/a/index.html\n\
             <h2 class=\"moss-term-role\">編輯</h2>\nposts/b/index.html"
        );
    }

    #[test]
    fn a_role_heading_is_localised_copy_under_the_contract_class() {
        assert_eq!(
            term_role_heading(crate::i18n::Language::ZhHant, "editor"),
            "<h2 class=\"moss-term-role\">編輯</h2>"
        );
    }

    #[test]
    fn every_name_list_field_has_a_claim_key_the_schema_declares() {
        // Pins the two halves of one convention together: the field this
        // derivation reads, and the frontmatter key a caller writes to claim
        // its term page. `tags`/`tag_page` is the pair that does not follow
        // the suffix rule, and the only way to know is to look it up.
        for field in moss_core::schema_fields::name_list_fields() {
            let key = claim_field_key(field)
                .unwrap_or_else(|| panic!("name-list field `{field}` has no claim key"));
            assert!(
                moss_core::schema_fields::BUILTIN_FIELDS.iter().any(|f| f.name == key),
                "claim key `{key}` for `{field}` is not a declared schema field"
            );
        }
        assert_eq!(claim_field_key("tags"), Some("tag_page"), "the irregular pair");
        assert_eq!(claim_field_key("byline"), None, "not a name-list field");
    }

    // ── task A10: kind-move redirect stubs ───────────────────────────────

    /// `author` declared under `[terms.people]`, as `term_kinds` resolves it:
    /// the built-in loses the field, the declared kind gains it.
    fn author_moved_to_people() -> Vec<TermKind> {
        vec![
            TermKind { key: "authors".to_string(), fields: Vec::new(), title: "Authors".to_string() },
            TermKind { key: "tags".to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string() },
            TermKind {
                key: "people".to_string(),
                fields: vec!["author".to_string(), "editor".to_string()],
                title: "People".to_string(),
            },
        ]
    }

    #[test]
    fn author_moving_into_a_declared_kind_gets_a_fresh_redirect_stub() {
        let mut docs = vec![
            doc("ada-lin/index.html", "Ada Lin"),
            doc("posts/a/index.html", "A"),
            doc("posts/b/index.html", "B"),
        ];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["Ada Lin".to_string()];
        docs[2].author = vec!["Sam Okafor".to_string()];
        let index = derive_terms(&mut docs, author_moved_to_people());
        assert_eq!(
            kind_move_stubs(&index),
            vec![
                // Claimed: the old URL forwards to the claiming page.
                ("authors/ada-lin/".to_string(), "/ada-lin/".to_string()),
                // Unclaimed: to the generated page in the new namespace.
                ("authors/sam-okafor/".to_string(), "/people/sam-okafor/".to_string()),
            ]
        );
    }

    #[test]
    fn a_term_reached_only_through_a_field_that_did_not_move_gets_no_stub() {
        // Kane is in `people` through `editor:`, which was never `authors/`'s
        // field. Forwarding `authors/kane/` would invent a page that never was.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].editor = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, author_moved_to_people());
        assert_eq!(kind_move_stubs(&index), Vec::new());
    }

    #[test]
    fn stub_absent_when_the_move_reverts() {
        // The same site with the declaration removed: `author` is the
        // built-in's field again, the terms are back under `authors/`, and
        // nothing is owed a forward. Nothing had to be un-recorded for that
        // — the pairs only ever existed while the config said so.
        let reverted = vec![
            TermKind {
                key: "authors".to_string(),
                fields: vec!["author".to_string()],
                title: "Authors".to_string(),
            },
            TermKind { key: "tags".to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string() },
        ];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, reverted);
        assert_eq!(kind_move_stubs(&index), Vec::new());
    }

    #[test]
    fn a_dimension_switched_off_is_not_a_move() {
        // `[terms] author = false` empties the built-in with no declared kind
        // taking the field. Those pages are gone, not relocated.
        let off = vec![TermKind {
            key: "authors".to_string(),
            fields: Vec::new(),
            title: "Authors".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, off);
        assert_eq!(kind_move_stubs(&index), Vec::new());
    }

    // ── task A8 should-fix: also_in dedup ────────────────────────────────

    #[test]
    fn a_member_named_through_two_fields_of_one_kind_lists_only_once() {
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        docs[0].editor = vec!["Ada Lin".to_string()];
        derive_terms(&mut docs, kinds);
        let also = docs[0].also_in.as_ref().unwrap();
        let occurrences = also.iter().filter(|k| *k == "people/ada-lin").count();
        assert_eq!(occurrences, 1, "also_in must not repeat the same term key: {also:?}");
    }

    // ── task A5: link_terms_in_bylines, widened, all six guards ─────────
    //
    // Guard 6's two "warns once" tests need the CLI-problem counter, which
    // is a process-global static guarded by a lock private to
    // `cli_output_tests.rs` (a test here cannot take it) — they live there:
    // `duplicate_name_in_one_row_warns`, `declared_name_absent_from_any_byline_row_warns`.

    #[test]
    fn wikilink_own_link_is_never_double_wrapped() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Alex Rivera".to_string()];
        docs[0].byline = vec!["by [[Alex Rivera]]".to_string()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(
            docs[0].byline[0], "by [[Alex Rivera]]",
            "a name already wikilinked is left alone, not wrapped a second time"
        );
    }

    #[test]
    fn latin_edge_never_matches_inside_a_longer_word() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Alex".to_string()];
        docs[0].byline = vec!["by Alexander".to_string()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "by Alexander", "Alex must not match glued inside Alexander");
    }

    #[test]
    fn han_glued_credit_still_links() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["谷夏攝".to_string()];
        // Glued directly to more Han text, no separator — the common
        // Chinese byline shape this guard must not break.
        docs[0].byline = vec!["谷夏攝影師".to_string()];
        let index = derive_terms(&mut docs, both_kinds());
        let expected_url = index.term_url("authors", "谷夏攝").expect("term exists");
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], format!("[谷夏攝]({expected_url})影師"));
    }

    #[test]
    fn single_character_name_never_auto_links() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["陳".to_string()];
        docs[0].byline = vec!["文｜陳".to_string()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜陳", "a single-character name never auto-links");
    }

    #[test]
    fn author_moved_into_a_declared_kind_still_links_from_bylines() {
        // Regression: before this task, link_terms_in_bylines only ever
        // looked names up in the `authors` namespace — a field moved into a
        // declared kind (`[terms.people] fields = ["author"]`) silently
        // stopped linking, even though derive_terms itself already handled
        // the move correctly.
        let kinds = vec![TermKind {
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Alex Rivera".to_string()];
        docs[0].byline = vec!["文｜Alex Rivera".to_string()];
        let index = derive_terms(&mut docs, kinds);
        let expected_url = index.term_url("people", "Alex Rivera").expect("term exists");
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], format!("文｜[Alex Rivera]({expected_url})"));
    }
}
