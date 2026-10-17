//! Redirect stubs for term addresses that forward elsewhere: a moved namespace
//! and a claimed term's generated address. Both are recomputed from the index every build.

use super::{TermIndex, BUILTIN_DEFAULT_FIELDS};

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
        let builtin_kept_it = index
            .kinds
            .iter()
            .any(|k| k.key == builtin_ns && k.fields.iter().any(|f| f == field));
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
        // The per-name stubs below cover every member's old `authors/<slug>/`
        // URL, but the old namespace ROOT — `authors/` itself, the listing an
        // external link may point at directly — has no member to inherit one
        // from. It simply stops being emitted once the field moves (the
        // generated namespace-index loop only emits pages for namespaces
        // still in use), so without this it 404s even though every page it
        // used to list has a working forward. One stateless stub, same as
        // the per-name ones, to the new kind's own root.
        stubs.push((format!("{}/", builtin_ns), format!("/{}/", new_kind.key))); // allow:served-path-url-construct (term-namespace-root redirect target)
        for (key, site) in &index.sites {
            let Some((ns, slug)) = key.split_once('/') else {
                continue;
            };
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
            let Some(target) = index.term_url(&new_kind.key, &site.display) else {
                continue;
            };
            stubs.push((format!("{}/{}/", builtin_ns, slug), target));
        }
    }
    stubs
}

/// A claimed term writes no generated page, so its generated address
/// forwards to the claiming page. That keeps "a link to a term page never
/// 404s" true for claimed terms too.
pub fn claim_stubs(index: &TermIndex) -> Vec<(String, String)> {
    let mut stubs = Vec::new();
    for (key, site) in &index.sites {
        let Some(claim) = &site.claimed_by else {
            continue;
        };
        let generated = format!("{}/", key); // allow:served-path-url-construct (generated term address, the redirect source)
        stubs.push((generated, format!("/{}", claim))); // allow:served-path-url-construct (term redirect target, the claiming page)
    }
    stubs
}
