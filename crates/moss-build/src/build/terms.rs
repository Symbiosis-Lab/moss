//! Term derivation: name-list fields (`author:`, `tags:`, and whatever a
//! site declares) become pseudo-folder memberships.
//!
//! Runs once per build, after every document is parsed and before the
//! synthetic folder-index machinery. For each kind's fields it derives a
//! membership claim in the term's pseudo-folder (`authors/<slug>`,
//! `tags/<slug>`, or a declared kind's own namespace) — pushed into the same
//! `also_in` slot an authored cross-listing uses — so the canonical selector
//! (`folder_embed::select_children_by_slug`), the synthetic-index loops in
//! `render/blocking.rs`, and the listing-group digest all serve term pages
//! with zero new modes. A page claiming a term (`author_page:` / `tag_page:`
//! / `editor_page:` / `jury_page:`) gets the resolved key in
//! `ParsedDocument::term_listing` — the one field the renderer and the
//! listing digest read — and suppresses the generated page for that term.
//! Term identity rules (fold, slug, key) are owned by `moss_core::terms`;
//! this pass only applies them to documents.

use std::sync::LazyLock;

use regex::Regex;

use crate::build::scan::article_map::to_pretty_url;
use crate::build::types::ParsedDocument;

mod roles;
mod listing_placement;
pub(crate) use listing_placement::insert_before_sources;
pub use roles::{set_card_credits, term_role_heading, term_role_label};
mod lines;
pub use lines::{set_term_lines, TermLine};
mod kinds;
pub use kinds::{declared_name_list_fields, term_kinds, TermKind, BUILTIN_DEFAULT_FIELDS};
// The editor's chip-completion readout of a finished build's kinds table —
// a pure reader of `ArticleMap`, never a deriver. Sibling file for the same
// reason `kinds.rs` and `rollup.rs` are: it grows from a different concern
// (what the editor asks for) than derivation does.
mod editor_read;
pub use editor_read::{list_vault_terms_in, KindTerms, VaultTerms};
// Location inheritance for a work's companions — the first thing
// `derive_terms` does below. Sibling file, same reasoning as `rollup.rs`.
// `pub(crate)`, not private: the places-explorer data emitter
// (`build::place_map::places_data`) reuses this module's `group_by_work`,
// the one folder/self-named-home election this pass already performs, so
// the two never carry two answers to "is this folder a work".
pub(crate) mod inherit;
// The gazetteer's `parent` links, attached to place-typed kinds at the
// config stage — the only place the gazetteer type crosses into the terms
// machinery. `derive_terms` itself never reads it.
pub mod places;
// `TermSite.parent` and children-with-counts — the once-per-
// build resolution that reads pass 2's finished memberships. See its own
// doc comment for why it is a sibling file rather than inline here.
mod rollup;
mod stubs;
pub use moss_core::terms::index::TermKey;
mod translations;
mod cards;
pub use cards::{set_term_cards, with_term_pages};
pub use translations::link_term_translations;
pub use stubs::{claim_stubs, edition_move_stubs, kind_move_stubs, place_slug_stubs};

/// `[text](url)` and `[[wikilink]]` spans — what [`linked_spans`] scans for.
static LINK_SPAN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[[^\]]*\]\]|\[[^\]]*\]\([^)]*\)").expect("valid regex")
});

pub use moss_core::terms::index::{TermIndex, TermNavEntry, TermSite};

/// This doc's values for a name-list field. A name outside the five built-ins
/// is site-declared, read from the page's own frontmatter like `author:`.
pub(crate) fn field_names(doc: &ParsedDocument, field: &str) -> Vec<String> {
    match field {
        "author" => doc.author.clone(),
        // Frontmatter tags only — inline #hashtags are prose, not
        // cataloguing (see `fm_tags` on ParsedDocument).
        "tags" => doc.fm_tags.clone().unwrap_or_default(),
        "editor" => doc.editor.clone(),
        "jury" => doc.jury.clone(),
        "location" => doc.location.clone(),
        declared => raw_frontmatter_value(doc, declared)
            .and_then(|v| moss_core::frontmatter_union::normalize_name_list(&v).ok())
            .unwrap_or_default(),
    }
}

/// A raw frontmatter value as the YAML value the shared normalizers take.
fn raw_frontmatter_value(doc: &ParsedDocument, key: &str) -> Option<serde_yaml::Value> {
    let value = doc.raw_frontmatter.get(key)?;
    serde_json::from_value(value.clone()).ok()
}

/// This doc's term-page claim for a name-list field, or `None`. Companion to
/// [`field_names`].
pub(crate) fn term_claim_of(doc: &ParsedDocument, field: &str) -> Option<moss_core::terms::TermClaim> {
    match field {
        "author" => doc.author_page.clone(),
        "tags" => doc.tag_page.clone(),
        "editor" => doc.editor_page.clone(),
        "jury" => doc.jury_page.clone(),
        "location" => doc.place_page.clone(),
        declared => {
            let key = claim_field_key(declared);
            let value = raw_frontmatter_value(doc, &key)?;
            moss_core::frontmatter_union::normalize_term_claim(&value).ok().flatten()
        }
    }
}

pub use moss_core::terms::claim_field_key;

/// Derive term memberships and claims from a build's kinds table. Mutates
/// documents in place:
///
/// - first, [`inherit::inherit_work_locations`] fills a blank companion
///   `location:` from its work's self-named home, so what follows sees it;
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
///
/// Terms are per edition (`ParsedDocument::edition`): a page's terms, claims
/// and links stay inside the language tree it sits in.
pub fn derive_terms(documents: &mut [ParsedDocument], kinds: Vec<TermKind>) -> TermIndex {
    moss_core::page::inherit::mirror_place_fields_into_location(documents, &kinds, |d| &mut d.location);
    inherit::inherit_work_locations(documents);
    let derived = moss_core::terms::index::derive_index(documents, kinds);
    for problem in &derived.problems {
        crate::build::cli_output::log_warn_problem!("{problem}");
    }
    let index = derived.index;
    for (doc, added) in documents.iter_mut().zip(derived.pages) {
        if !added.also_in.is_empty() {
            doc.also_in.get_or_insert_with(Vec::new).extend(added.also_in);
        }
        if added.term_listing.is_some() {
            doc.term_listing = added.term_listing;
        }
        let Some(term_key) = doc.term_listing.as_deref() else { continue };
        doc.term_sections = index.sections(term_key).map(<[_]>::to_vec);
        doc.place_children = index.children(term_key).map(<[_]>::to_vec);
    }
    rollup::flag_place_namespace_roots(documents, &index);
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
/// 3. longest name first, so "林小" never claims the middle of "林小滿"
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
pub fn link_terms_in_bylines(documents: &mut [ParsedDocument], index: &TermIndex) {
    for doc in documents.iter_mut() {
        if doc.byline.is_empty() {
            continue;
        }
        // Every name-list field's values, across every kind this build's
        // site declares — not just author:. A name may legitimately repeat
        // across fields/kinds; guard 6 warns about that, once, only when it
        // actually resolves a link in some row.
        //
        // A place-typed kind is skipped here at the source: a place name
        // (`location:`) is never credited in a byline — it gets its own
        // automatic place line from `set_term_lines` — so it must never
        // become a byline-linking candidate, and must never trip guard 6's
        // "declared but never in a byline row" warning, which fired for
        // every located page before this exclusion existed.
        let mut declared: Vec<String> = Vec::new();
        for kind in index.kinds() {
            if kind.is_place {
                continue;
            }
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

        let edition = doc.edition.clone();
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
                let url = index.kinds().iter().find_map(|kind| index.term_url_in(&edition, &kind.key, name))?;
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
            // once at the end — so a shorter co-author name ("林小") can never
            // match inside the link already injected for a longer one
            // ("林小滿") and nest markdown.
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

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::terms::term_folder_key;
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
            TermKind { cards: false, key: AUTHOR_NS.to_string(), fields: vec!["author".to_string()], title: "Authors".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
            TermKind { cards: false, key: TAGS_NS.to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
        ]
    }

    #[test]
    fn a_term_is_keyed_by_the_edition_of_the_page_that_names_it() {
        let mut docs = vec![doc("posts/a/index.html", "A"), doc("de/posts/b/index.html", "B")];
        docs[0].author = vec!["Ada".into()];
        docs[1].author = vec!["Ada".into(), "Nur Deutsch".into()];
        docs[1].edition = "de".into();
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(
            index.sites().keys().map(String::as_str).collect::<Vec<_>>(),
            ["authors/ada", "de/authors/ada", "de/authors/nur-deutsch"]
        );
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"de/authors/ada".to_string()));
        assert!(!docs[1].also_in.as_ref().unwrap().contains(&"authors/ada".to_string()));
        assert_eq!(index.term_url_in("de", "authors", "Ada").as_deref(), Some("/de/authors/ada/"));
        assert_eq!(index.term_url_in("", "authors", "Ada").as_deref(), Some("/authors/ada/"));
        assert_eq!(index.roots_in_use().collect::<Vec<_>>(), ["authors", "de/authors"]);
        assert_eq!(index.kind_for("de/authors").map(|(k, edition)| (k.key.as_str(), edition)), Some(("authors", "de")));
    }

    #[test]
    fn synthetic_folders_are_the_unclaimed_terms_plus_the_roots_in_use() {
        let mut docs = vec![doc("about/ma/index.html", "林小滿"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["林小滿".into(), "Scarly".into()];
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

    /// One place kind, one non-place kind, both in use: only the place
    /// kind's bare root comes back, never the tag root — a namespace root's
    /// KIND decides whether a real `index.md` there can host a term map, not
    /// merely whether it is a root at all.
    #[test]
    fn place_namespace_roots_are_the_bare_keys_whose_kind_is_place() {
        let kinds = vec![
            TermKind {
                cards: false,
                key: "places".to_string(),
                fields: vec!["location".to_string()],
                title: "Places".to_string(),
                titles: Vec::new(), counts: Vec::new(), is_place: true,
                parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
            },
            TermKind {
                cards: false,
                key: TAGS_NS.to_string(),
                fields: vec!["tags".to_string()],
                title: "Tags".to_string(),
                titles: Vec::new(), counts: Vec::new(), is_place: false,
                parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
            },
        ];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Kyoto".into()];
        docs[0].fm_tags = Some(vec!["城市".into()]);
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(index.place_namespace_roots().collect::<Vec<_>>(), vec!["places"]);
    }

    /// A real folder index at the bare place-kind root gets flagged so
    /// `render/html.rs` — which cannot see `TermIndex` — knows to splice the
    /// term map below this page's own content. A real index one level
    /// DEEPER (`places/kyoto/index.html`, not itself a claim) must NOT get
    /// it: only the bare root is a namespace root, and an individual place
    /// term is reached through `term_listing` instead.
    #[test]
    fn a_real_index_at_the_place_root_is_flagged_a_deeper_one_is_not() {
        let kinds = vec![TermKind {
            cards: false,
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: true,
            parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![
            doc("places/index.html", "Places"),
            doc("places/kyoto/index.html", "Kyoto"),
            doc("posts/a/index.html", "A"),
        ];
        docs[2].location = vec!["Kyoto".into()];
        derive_terms(&mut docs, kinds);
        assert!(docs[0].is_place_namespace_root, "the real root index must be flagged");
        assert!(!docs[1].is_place_namespace_root, "a deeper real index is not a namespace root");
        assert!(!docs[2].is_place_namespace_root, "an ordinary page is not a namespace root");
    }

    #[test]
    fn claim_replaces_generated_page_and_hosts_the_listing() {
        let mut docs = vec![doc("about/ma/index.html", "林小滿"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["林小滿".into()];
        let index = derive_terms(&mut docs, both_kinds());
        // The winning claimer hosts the member listing…
        assert_eq!(docs[0].term_listing.as_deref(), Some("authors/林小滿"));
        // …the term page is no longer generated…
        assert_eq!(index.unclaimed_keys().count(), 0);
        // …the claim page takes its place in the /authors/ root listing…
        assert!(docs[0].also_in.as_ref().unwrap().contains(&"authors".to_string()));
        // …links point at the claiming page…
        assert_eq!(index.term_url_in("", "authors", "林小滿").as_deref(), Some("/about/ma/"));
        // …and the member doc still claims membership for the selector.
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"authors/林小滿".to_string()));
    }

    #[test]
    fn a_term_page_never_lists_itself() {
        // The author page is itself authored by its subject.
        let mut docs = vec![doc("about/ma/index.html", "林小滿")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].author = vec!["林小滿".into()];
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
        assert_eq!(index1.term_url_in("", "authors", "X").as_deref(), Some("/a/"));
        assert_eq!(index2.term_url_in("", "authors", "X").as_deref(), Some("/a/"));
    }

    #[test]
    fn explicit_children_routing_wins_over_claim_lowering() {
        let mut docs = vec![doc("about/ma/index.html", "林小滿")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].children_source = Some("[[News]]".into());
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(docs[0].children_source.as_deref(), Some("[[News]]"));
        assert!(docs[0].term_listing.is_none());
        // The claim still resolves links and suppresses the generated page.
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert_eq!(index.term_url_in("", "authors", "林小滿").as_deref(), Some("/about/ma/"));
    }

    #[test]
    fn byline_names_link_to_term_pages() {
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["林小滿".into()];
        docs[0].byline = vec!["文｜林小滿".into(), "編輯｜其他人".into()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜[林小滿](/authors/林小滿/)");
        // Names not declared in `author:` stay untouched.
        assert_eq!(docs[0].byline[1], "編輯｜其他人");
    }

    #[test]
    fn byline_link_points_at_a_claim_page_and_never_at_itself() {
        let mut docs = vec![doc("about/ma/index.html", "林小滿"), doc("posts/a/index.html", "A")];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[0].author = vec!["林小滿".into()];
        docs[0].byline = vec!["林小滿".into()];
        docs[1].author = vec!["林小滿".into()];
        docs[1].byline = vec!["文｜林小滿".into()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[1].byline[0], "文｜[林小滿](/about/ma/)");
        // The claim page's own byline names its subject: no self-link.
        assert_eq!(docs[0].byline[0], "林小滿");
    }

    #[test]
    fn a_co_author_name_inside_a_longer_name_never_nests_links() {
        // "林小" is a substring of "林小滿"; matching must happen on the
        // original row only, or the shorter name matches inside the link
        // injected for the longer one and nests markdown.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["林小滿".into(), "林小".into()];
        docs[0].byline = vec!["文｜林小滿".into(), "編｜林小".into()];
        let index = derive_terms(&mut docs, both_kinds());
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], "文｜[林小滿](/authors/林小滿/)");
        assert_eq!(docs[0].byline[1], "編｜[林小](/authors/林小/)");
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

        let kinds = term_kinds(&cfg, "en");
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["jury".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
    fn claim_through_place_page_hosts_the_listing() {
        let kinds = vec![TermKind {
            cards: false,
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: true, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("about/kyoto/index.html", "Kyoto"), doc("posts/a/index.html", "A")];
        docs[0].place_page = Some(TermClaim::UseTitle);
        docs[1].location = vec!["Kyoto".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(docs[0].term_listing.as_deref(), Some("places/kyoto"));
        assert_eq!(index.unclaimed_keys().count(), 0);
        assert!(docs[1].also_in.as_ref().unwrap().contains(&"places/kyoto".to_string()));
    }

    // ── task A5: the roll-up ──────────────────────────────────────────────

    fn places_kind_with_parents(parents: &[(&str, &str)]) -> TermKind {
        TermKind {
            cards: false,
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: true,
            parents: parents.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            explorer: None, line: None, slugs: Default::default(),
        }
    }

    #[test]
    fn a_place_slug_names_its_page_everywhere_and_the_old_address_forwards() {
        let mut kind = places_kind_with_parents(&[("places/xuhui", "上海")]);
        kind.slugs.insert("上海".to_string(), "shanghai".to_string());
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Xuhui".to_string()];
        let index = derive_terms(&mut docs, vec![kind]);
        let also_in = docs[0].also_in.as_ref().unwrap();
        assert!(also_in.contains(&"places/shanghai".to_string()), "{also_in:?}");
        assert!(!also_in.iter().any(|k| k.contains("上海")), "{also_in:?}");
        assert_eq!(index.term_url_in("", "places", "上海").as_deref(), Some("/places/shanghai/"));
        assert_eq!(place_slug_stubs(&index), vec![("places/上海/".to_string(), "/places/shanghai/".to_string())]);
    }

    #[test]
    fn a_page_in_a_city_appears_on_the_countrys_listing_through_parent_rollup() {
        let kinds = vec![places_kind_with_parents(&[("places/kyoto", "Japan")])];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Kyoto".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert!(docs[0].also_in.as_ref().unwrap().contains(&"places/kyoto".to_string()));
        assert!(docs[0].also_in.as_ref().unwrap().contains(&"places/japan".to_string()));
        assert_eq!(
            index.members_by_field().get(&("places/japan".to_string(), "location".to_string())),
            Some(&vec!["posts/a/".to_string()])
        );
    }

    #[test]
    fn a_page_in_a_city_rolls_up_through_a_multi_level_chain_to_a_country_with_no_gazetteer_row_of_its_own(
    ) {
        // The umbrella spec's own worked example, reproduced with neutral
        // names: city → region → country, and the country-level ancestor
        // gets its `TermSite` from THIS pass — `attach_parents` (A4) never
        // creates one, since no gazetteer row named "Japan" itself.
        let kinds = vec![places_kind_with_parents(&[
            ("places/kyoto", "Kansai"),
            ("places/kansai", "Japan"),
        ])];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Kyoto".to_string()];
        let index = derive_terms(&mut docs, kinds);
        for key in ["places/kyoto", "places/kansai", "places/japan"] {
            assert!(
                docs[0].also_in.as_ref().unwrap().contains(&key.to_string()),
                "missing {key} in also_in: {:?}",
                docs[0].also_in
            );
        }
        assert!(index.sites().contains_key("places/japan"), "Japan gets a TermSite from this pass");
    }

    #[test]
    fn roll_up_records_the_right_field_on_a_multi_field_place_kind() {
        let mut kind = places_kind_with_parents(&[("places/kyoto", "Japan")]);
        kind.fields = vec!["location".to_string(), "editor".to_string()];
        let kinds = vec![kind];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Kyoto".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(
            index.members_by_field().get(&("places/japan".to_string(), "location".to_string())),
            Some(&vec!["posts/a/".to_string()]),
            "the roll-up records the field the city was reached through, not a flat merge"
        );
        assert_eq!(
            index.members_by_field().get(&("places/japan".to_string(), "editor".to_string())),
            None,
            "no editor: value named this document, so it must not appear under that field"
        );
    }

    #[test]
    fn a_document_naming_a_city_and_its_country_is_counted_once_in_the_country() {
        let kinds = vec![places_kind_with_parents(&[("places/kyoto", "Japan")])];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["Kyoto".to_string(), "Japan".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(
            index
                .members_by_field()
                .get(&("places/japan".to_string(), "location".to_string()))
                .map(Vec::len),
            Some(1),
            "Japan reached both directly and as Kyoto's ancestor must be counted once"
        );
    }

    #[test]
    fn ancestor_walk_terminates_on_a_cycle_longer_than_break_cycles_max_hops() {
        // Ten nodes, one longer than `places::break_cycles`'s own 8-hop cap
        // — and built by hand rather than through `attach_parents`, so that
        // guard never runs at all. `break_cycles` is a config-stage
        // precaution over a gazetteer-sourced `parents` map; a `TermKind`
        // reaching this pass some other way (hand-built here, but the same
        // shape a future caller could construct) has no such promise
        // upstream, so this walk needs its own backstop rather than relying
        // on one it never sees run.
        let mut parents = std::collections::BTreeMap::new();
        for i in 0..10 {
            let key = term_folder_key("places", &format!("n{i}"));
            let parent_name = format!("n{}", (i + 1) % 10);
            parents.insert(key, parent_name);
        }
        let kinds = vec![TermKind {
            cards: false,
            key: "places".to_string(),
            fields: vec!["location".to_string()],
            title: "Places".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: true,
            parents,
            explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].location = vec!["n0".to_string()];
        // Hangs forever without the `seen` guard — this call is the test.
        let index = derive_terms(&mut docs, kinds);

        let also_in = docs[0].also_in.as_ref().expect("every node on the cycle is a member");
        for i in 0..10 {
            let key = format!("places/n{i}");
            assert!(also_in.contains(&key), "missing {key} in also_in: {also_in:?}");
        }
        assert_eq!(
            also_in.iter().filter(|k| k.starts_with("places/n")).count(),
            10,
            "each node on the cycle recorded exactly once, not looped: {also_in:?}"
        );
        assert_eq!(index.sites().len(), 10, "no duplicate TermSite created by looping back around");
    }

    #[test]
    fn a_claimed_place_with_no_members_keeps_its_parent() {
        let kinds = vec![places_kind_with_parents(&[("places/kyoto", "Japan")])];
        let mut docs = vec![doc("about/kyoto/index.html", "Kyoto")];
        docs[0].place_page = Some(TermClaim::UseTitle);
        // No document's `location:` ever names Kyoto — the claim is the
        // only thing that puts it in `index.sites` — so only the post-pass-2
        // sweep, not the ancestor walk (which never runs for this document),
        // can set its `parent`.
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(index.sites().get("places/kyoto").and_then(|s| s.parent.as_deref()), Some("places/japan"));
    }

    // ── place children ─────────────────────────────────────────────────

    #[test]
    fn non_place_kind_has_no_place_children() {
        // A people-kind claiming page: `TermSite::parent` is never set for a
        // non-place kind (`kind.parents` stays empty), so
        // `children_by_term` ends up with nothing
        // recorded for it — the emptiness check the render layer applies is
        // the only guard, replacing the dropped `is_place` gate.
        let kinds = vec![TermKind {
            cards: false,
            key: "people".to_string(),
            fields: vec!["jury".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false,
            parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("about/kane/index.html", "Kane")];
        docs[0].jury_page = Some(TermClaim::UseTitle);
        derive_terms(&mut docs, kinds);
        assert_eq!(docs[0].place_children, None);
    }

    #[test]
    fn members_by_field_records_which_field_a_member_came_through() {
        let kinds = vec![TermKind {
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        docs[0].editor = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, kinds);
        assert_eq!(
            index.members_by_field().get(&("people/ada-lin".to_string(), "author".to_string())),
            Some(&vec!["posts/a/".to_string()])
        );
        assert_eq!(
            index.members_by_field().get(&("people/kane".to_string(), "editor".to_string())),
            Some(&vec!["posts/a/".to_string()])
        );
        // Cross-checked against the wrong field: a member recorded under
        // "author" must not also show up under "editor" for the same term.
        assert_eq!(
            index.members_by_field().get(&("people/ada-lin".to_string(), "editor".to_string())),
            None
        );
    }

    #[test]
    fn term_sections_keeps_a_trailing_group_for_hand_authored_also_in() {
        let kinds = vec![TermKind {
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
        // Companion to the name-list flag test in moss-core's schema_fields_tests:
        // that test pins the SET; this one pins that `field_names` has a real
        // arm for each member of it, so a `_ => &[]` wildcard could not pass
        // as a match for a name the set still yields.
        let mut d = doc("posts/a/index.html", "A");
        d.author = vec!["Ada Lin".to_string()];
        d.fm_tags = Some(vec!["essays".to_string()]);
        d.editor = vec!["Kane".to_string()];
        d.jury = vec!["Kaneda".to_string()];
        d.location = vec!["Kyoto".to_string()];
        for field in ["author", "tags", "editor", "jury", "location"] {
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
            cards: false,
            key: "authors".to_string(),
            fields: Vec::new(),
            title: "作者".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("authors/index.html", "authors")];
        let index = derive_terms(&mut docs, off);
        assert!(
            index.kind_for("authors").is_none(),
            "an empty namespace must not lend its title to a real folder"
        );

        // The same kind, switched on and in use, does title its root.
        let on = vec![TermKind {
            cards: false,
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "作者".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Ada Lin".to_string()];
        let index = derive_terms(&mut docs, on);
        assert_eq!(index.kind_for("authors").map(|(k, _)| k.title.as_str()), Some("作者"));
    }

    #[test]
    fn a_hand_authored_also_in_does_not_earn_the_page_a_heading() {
        // An undeclared site — the built-in one-field `authors` kind — with a
        // page cross-listed onto a term by hand. That page lands in the
        // trailing unlabelled group, so counting groups would see two and
        // put "Author" over the derived half. develop renders one flat list,
        // and every such site's output is byte-identical or it is not.
        let kinds = vec![TermKind {
            cards: false,
            key: "authors".to_string(),
            fields: vec!["author".to_string()],
            title: "Authors".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string(), "jury".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string(), "jury".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "jury".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
        for field in ["author", "tags", "editor", "jury", "location"] {
            let key = claim_field_key(field);
            assert!(
                moss_core::schema_fields::BUILTIN_FIELDS.iter().any(|f| f.name == key),
                "claim key `{key}` for `{field}` is not a declared schema field"
            );
        }
        assert_eq!(claim_field_key("tags"), "tag_page", "the irregular pair");
        assert_eq!(claim_field_key("conductor"), "conductor_page", "a declared field follows the suffix rule");
    }

    // ── task A10: kind-move redirect stubs ───────────────────────────────

    /// `author` declared under `[terms.people]`, as `term_kinds` resolves it:
    /// the built-in loses the field, the declared kind gains it.
    fn author_moved_to_people() -> Vec<TermKind> {
        vec![
            TermKind { cards: false, key: "authors".to_string(), fields: Vec::new(), title: "Authors".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
            TermKind { cards: false, key: "tags".to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
            TermKind {
                cards: false,
                key: "people".to_string(),
                fields: vec!["author".to_string(), "editor".to_string()],
                title: "People".to_string(),
                titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
                // The old namespace root itself, stateless and unconditional
                // on any particular member — an external link to `authors/`
                // must not 404 just because every member below it moved.
                ("authors/".to_string(), "/people/".to_string()),
                // Claimed: the old URL forwards to the claiming page.
                ("authors/ada-lin/".to_string(), "/ada-lin/".to_string()),
                // Unclaimed: to the generated page in the new namespace.
                ("authors/sam-okafor/".to_string(), "/people/sam-okafor/".to_string()),
            ]
        );
    }

    #[test]
    fn a_claimed_term_gets_a_stub_to_its_claiming_page_and_an_unclaimed_one_does_not() {
        let mut docs = vec![
            doc("ada-example/index.html", "Ada Example"),
            doc("posts/a/index.html", "A"),
            doc("posts/b/index.html", "B"),
        ];
        docs[0].author_page = Some(TermClaim::UseTitle);
        docs[1].author = vec!["Ada Example".to_string()];
        docs[2].author = vec!["Sam Example".to_string()];
        let index = derive_terms(&mut docs, both_kinds());
        assert_eq!(
            claim_stubs(&index),
            vec![("authors/ada-example/".to_string(), "/ada-example/".to_string())]
        );
    }

    #[test]
    fn a_term_reached_only_through_a_field_that_did_not_move_gets_no_per_name_stub() {
        // Kane is in `people` through `editor:`, which was never `authors/`'s
        // field. Forwarding `authors/kane/` would invent a page that never
        // was — but the field STILL moved at the kind level, so the
        // namespace-root stub is unconditional and present regardless.
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].editor = vec!["Kane".to_string()];
        let index = derive_terms(&mut docs, author_moved_to_people());
        assert_eq!(
            kind_move_stubs(&index),
            vec![("authors/".to_string(), "/people/".to_string())]
        );
    }

    /// `BUILTIN_DEFAULT_FIELDS` pairs `tags` with the `tags` field the same
    /// way it pairs `authors` with `author` — one loop over that table
    /// builds every root stub, so a moved `tags/` root needs no separate
    /// code path to redirect correctly.
    #[test]
    fn a_moved_tags_namespace_root_also_gets_a_redirect() {
        let moved = vec![
            TermKind { cards: false, key: "authors".to_string(), fields: vec!["author".to_string()], title: "Authors".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
            TermKind { cards: false, key: "tags".to_string(), fields: Vec::new(), title: "Tags".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
            TermKind {
                cards: false,
                key: "topics".to_string(),
                fields: vec!["tags".to_string()],
                title: "Topics".to_string(),
                titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
            },
        ];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].tags = Some(vec!["craft".to_string()]);
        let index = derive_terms(&mut docs, moved);
        assert!(
            kind_move_stubs(&index).contains(&("tags/".to_string(), "/topics/".to_string())),
            "a moved tags namespace root must redirect to the new kind's root: {:?}",
            kind_move_stubs(&index)
        );
    }

    #[test]
    fn stub_absent_when_the_move_reverts() {
        // The same site with the declaration removed: `author` is the
        // built-in's field again, the terms are back under `authors/`, and
        // nothing is owed a forward. Nothing had to be un-recorded for that
        // — the pairs only ever existed while the config said so.
        let reverted = vec![
            TermKind {
                cards: false,
                key: "authors".to_string(),
                fields: vec!["author".to_string()],
                title: "Authors".to_string(),
                titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
            },
            TermKind { cards: false, key: "tags".to_string(), fields: vec!["tags".to_string()], title: "Tags".to_string(), titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default() },
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
            cards: false,
            key: "authors".to_string(),
            fields: Vec::new(),
            title: "Authors".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string(), "editor".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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
        let expected_url = index.term_url_in("", "authors", "谷夏攝").expect("term exists");
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
            cards: false,
            key: "people".to_string(),
            fields: vec!["author".to_string()],
            title: "People".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("posts/a/index.html", "A")];
        docs[0].author = vec!["Alex Rivera".to_string()];
        docs[0].byline = vec!["文｜Alex Rivera".to_string()];
        let index = derive_terms(&mut docs, kinds);
        let expected_url = index.term_url_in("", "people", "Alex Rivera").expect("term exists");
        link_terms_in_bylines(&mut docs, &index);
        assert_eq!(docs[0].byline[0], format!("文｜[Alex Rivera]({expected_url})"));
    }

    #[test]
    fn a_card_credit_names_its_term_kind_and_the_slug_of_its_term_page() {
        let kinds = vec![TermKind {
            cards: true,
            key: "marks".to_string(),
            fields: vec!["mark".to_string()],
            title: "Marks".to_string(),
            titles: Vec::new(), counts: Vec::new(), is_place: false, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
        }];
        let mut docs = vec![doc("works/a/index.html", "A")];
        docs[0].raw_frontmatter.insert("mark".into(), serde_json::json!("Held, not online"));
        set_card_credits(&mut docs, &kinds);
        let credits = docs[0].card_extras.as_deref().expect("a credit");
        assert!(credits.contains(r#"<span class="moss-credit" data-term="marks" data-value="held-not-online">Held, not online"#), "{credits}");
    }
}
