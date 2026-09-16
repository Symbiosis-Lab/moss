//! Forward and backward link/embed edges between pages (moss#922 Stage 4).
//!
//! `DepGraph` answers "who links to / embeds this page?" — the question
//! Stage 5's facade-gated render skip needs to widen a changed page's
//! minimal render set to its backlinks. It is a standalone type, not a
//! `ContentGraph` field: `ContentGraph` is a structural path index built
//! from file paths alone (zero content reads, see `content_graph.rs`)
//! while `DepGraph` is built from parsed per-page edges — same
//! type-vs-populator split as `ContentGraph` itself (the type here; the
//! `Vec<ParsedDocument>` → edges glue lives in `src-tauri`, which is not a
//! moss-core dependency).
//!
//! Rebuilt fresh every build from scratch — cheap (a handful of `HashMap`
//! inserts over ~one page count), same argument as why `ContentGraph`
//! itself is never persisted. See the "Don't persist ContentGraph; don't
//! use ObjectStore" section of
//! `docs/archive/2026-07-31-incremental-build-facade-diff.md`.

use crate::resolve::embeds::MAX_EMBED_DEPTH;
use crate::resolve::{LinkType, OutgoingLink};
use std::collections::HashMap;

/// Directed link/embed edges between pages, keyed by source path.
///
/// Built once per build via [`DepGraph::build`]; queried via [`DepGraph::backlinks`]
/// and [`DepGraph::back_embeds`]. `Embed` edges are a subset already present
/// in `forward_links`/`backlinks` — `forward_embeds`/`back_embeds` narrow to
/// just `LinkType::Embed` because embeds are more render-relevant than plain
/// links (a transcluded page's body IS part of the embedding page's output).
#[derive(Debug, Clone, Default)]
pub struct DepGraph {
    forward_links: HashMap<String, Vec<String>>,
    forward_embeds: HashMap<String, Vec<String>>,
    backlinks: HashMap<String, Vec<String>>,
    back_embeds: HashMap<String, Vec<String>>,
}

impl DepGraph {
    /// Build a `DepGraph` from each page's source path and the outgoing
    /// links it resolved during parsing. Order of `pages` does not affect
    /// the result — edge lists within a bucket follow input order for
    /// determinism, but no page's presence depends on any other's.
    pub fn build<'a, I>(pages: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a [OutgoingLink])>,
    {
        let mut graph = DepGraph::default();
        for (source_path, outgoing) in pages {
            for link in outgoing {
                graph
                    .forward_links
                    .entry(source_path.to_string())
                    .or_default()
                    .push(link.target_path.clone());
                graph
                    .backlinks
                    .entry(link.target_path.clone())
                    .or_default()
                    .push(source_path.to_string());
                if link.link_type == LinkType::Embed {
                    graph
                        .forward_embeds
                        .entry(source_path.to_string())
                        .or_default()
                        .push(link.target_path.clone());
                    graph
                        .back_embeds
                        .entry(link.target_path.clone())
                        .or_default()
                        .push(source_path.to_string());
                }
            }
        }
        graph
    }

    /// Fold in transclusion edges recorded by the resolve phase (moss#922
    /// Stage 7).
    ///
    /// `pairs` are `(target, immediate_embedder)` — exactly the shape
    /// `ResolveResult::embed_deps` produces (`resolve/embeds.rs`), i.e. DIRECT
    /// one-hop edges: for `index.md` embedding `a.md` embedding `b.md` the
    /// resolver reports `[("a.md", "index.md"), ("b.md", "a.md")]`. The second
    /// element is the file the marker was found in, NOT the top-level page
    /// being resolved, so grouping by it yields a correct adjacency list and
    /// multi-hop chains are answered by [`Self::embed_closure`], never by
    /// filtering pairs on the page under test (which would silently miss
    /// `b.md` as a dependency of `index.md`).
    ///
    /// This exists as a separate builder step, `ContentGraph::with_output_overrides`
    /// style, because these edges are produced by a different phase than
    /// `outgoing_links`: transclusion is spliced from disk bytes during resolve,
    /// before the AST dispatcher that populates `outgoing_links` ever runs, so
    /// no page→page `LinkType::Embed` link exists to carry them.
    ///
    /// Duplicate edges (the same nested pair is reported once per ancestor that
    /// transitively embeds it) are collapsed.
    pub fn with_embed_pairs<'a, I>(mut self, pairs: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        for (target, embedder) in pairs {
            let forward = self.forward_embeds.entry(embedder.to_string()).or_default();
            if !forward.iter().any(|t| t == target) {
                forward.push(target.to_string());
            }
            let back = self.back_embeds.entry(target.to_string()).or_default();
            if !back.iter().any(|s| s == embedder) {
                back.push(embedder.to_string());
            }
        }
        self
    }

    /// Every file whose bytes are spliced into `path`'s markdown, transitively.
    ///
    /// A breadth-first walk of `forward_embeds` from `path`, excluding `path`
    /// itself, bounded by [`MAX_EMBED_DEPTH`] — the same limit
    /// `resolve_embeds_inner` stops recursing at, so the closure never claims a
    /// dependency on content the resolver refused to splice. Cycles terminate
    /// on the visited set.
    ///
    /// This is the parse cache's validity input (moss#922 Stage 7): `path`'s
    /// cached `ParsedDocument` is only reusable if every member of this set
    /// still hashes to what it hashed to when the entry was written.
    pub fn embed_closure(&self, path: &str) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut closure: Vec<String> = Vec::new();
        let mut frontier: Vec<String> = vec![path.to_string()];
        for _ in 0..MAX_EMBED_DEPTH {
            if frontier.is_empty() {
                break;
            }
            let mut next: Vec<String> = Vec::new();
            for node in &frontier {
                for target in self.forward_embeds(node) {
                    if target != path && seen.insert(target.clone()) {
                        closure.push(target.clone());
                        next.push(target.clone());
                    }
                }
            }
            frontier = next;
        }
        closure
    }

    /// Pages `path` links to (any `LinkType`), in resolution order. Empty if
    /// `path` has no outgoing links or is not a source in this graph.
    pub fn forward_links(&self, path: &str) -> &[String] {
        self.forward_links.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Pages `path` embeds (`LinkType::Embed` only). Subset of `forward_links`.
    pub fn forward_embeds(&self, path: &str) -> &[String] {
        self.forward_embeds.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Pages that link to `path` (any `LinkType`). Empty if nothing links here.
    pub fn backlinks(&self, path: &str) -> &[String] {
        self.backlinks.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Pages that embed `path` (`LinkType::Embed` only). Subset of `backlinks`.
    pub fn back_embeds(&self, path: &str) -> &[String] {
        self.back_embeds.get(path).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(target: &str, link_type: LinkType) -> OutgoingLink {
        OutgoingLink {
            target_path: target.to_string(),
            display_text: target.to_string(),
            link_type,
        }
    }

    #[test]
    fn empty_graph_has_no_edges() {
        let graph = DepGraph::build(std::iter::empty());
        assert!(graph.backlinks("a.md").is_empty());
        assert!(graph.forward_links("a.md").is_empty());
    }

    #[test]
    fn forward_and_backlinks_are_reciprocal() {
        let a_links = [link("b.md", LinkType::Wikilink)];
        let graph = DepGraph::build([("a.md", a_links.as_slice())]);
        assert_eq!(graph.forward_links("a.md"), ["b.md"]);
        assert_eq!(graph.backlinks("b.md"), ["a.md"]);
        assert!(graph.backlinks("a.md").is_empty());
    }

    #[test]
    fn embed_edges_are_a_subset_of_link_edges() {
        let a_links = [
            link("b.md", LinkType::Wikilink),
            link("c.md", LinkType::Embed),
        ];
        let graph = DepGraph::build([("a.md", a_links.as_slice())]);
        assert_eq!(graph.forward_links("a.md"), ["b.md", "c.md"]);
        assert_eq!(graph.forward_embeds("a.md"), ["c.md"]);
        assert_eq!(graph.backlinks("c.md"), ["a.md"]);
        assert_eq!(graph.back_embeds("c.md"), ["a.md"]);
        assert!(graph.back_embeds("b.md").is_empty());
    }

    #[test]
    fn multiple_sources_linking_the_same_target_accumulate() {
        let a_links = [link("shared.md", LinkType::Wikilink)];
        let b_links = [link("shared.md", LinkType::Wikilink)];
        let graph = DepGraph::build([
            ("a.md", a_links.as_slice()),
            ("b.md", b_links.as_slice()),
        ]);
        assert_eq!(graph.backlinks("shared.md"), ["a.md", "b.md"]);
    }

    #[test]
    fn embed_pairs_group_by_their_immediate_embedder() {
        // `resolve_embeds` reports the file the MARKER was found in, so a
        // nested chain arrives as two one-hop edges, not two edges from the
        // top-level page.
        let graph = DepGraph::default()
            .with_embed_pairs([("a.md", "index.md"), ("b.md", "a.md")]);
        assert_eq!(graph.forward_embeds("index.md"), ["a.md"]);
        assert_eq!(graph.forward_embeds("a.md"), ["b.md"]);
        assert_eq!(graph.back_embeds("b.md"), ["a.md"]);
    }

    #[test]
    fn embed_closure_follows_multiple_hops() {
        // THE regression this design exists for: index.md embeds a.md embeds
        // b.md. A flat filter over `embed_deps` pairs whose source is
        // "index.md" would return only a.md and leave index.md silently stale
        // when b.md is edited.
        let graph = DepGraph::default()
            .with_embed_pairs([("a.md", "index.md"), ("b.md", "a.md"), ("c.md", "b.md")]);
        let mut closure = graph.embed_closure("index.md");
        closure.sort();
        assert_eq!(closure, ["a.md", "b.md", "c.md"]);
        assert_eq!(graph.embed_closure("b.md"), ["c.md"]);
        assert!(graph.embed_closure("c.md").is_empty());
    }

    #[test]
    fn embed_closure_terminates_on_a_cycle() {
        let graph = DepGraph::default().with_embed_pairs([("b.md", "a.md"), ("a.md", "b.md")]);
        let mut closure = graph.embed_closure("a.md");
        closure.sort();
        // `a.md` itself is never reported as its own dependency.
        assert_eq!(closure, ["b.md"]);
    }

    #[test]
    fn embed_closure_stops_at_the_resolver_depth_limit() {
        // A chain longer than MAX_EMBED_DEPTH: the resolver refuses to splice
        // past the limit, so the closure must not claim a dependency there.
        let names: Vec<String> = (0..MAX_EMBED_DEPTH + 5).map(|i| format!("{i}.md")).collect();
        let pairs: Vec<(&str, &str)> = names
            .windows(2)
            .map(|w| (w[1].as_str(), w[0].as_str()))
            .collect();
        let graph = DepGraph::default().with_embed_pairs(pairs);
        assert_eq!(graph.embed_closure("0.md").len(), MAX_EMBED_DEPTH);
    }

    #[test]
    fn duplicate_embed_pairs_are_collapsed() {
        // The same nested pair is reported once per ancestor that transitively
        // embeds it, so the raw input is full of duplicates.
        let graph = DepGraph::default()
            .with_embed_pairs([("b.md", "a.md"), ("b.md", "a.md"), ("b.md", "a.md")]);
        assert_eq!(graph.forward_embeds("a.md"), ["b.md"]);
        assert_eq!(graph.back_embeds("b.md"), ["a.md"]);
    }

    #[test]
    fn embed_pairs_compose_with_link_edges() {
        let a_links = [link("b.md", LinkType::Wikilink)];
        let graph = DepGraph::build([("a.md", a_links.as_slice())])
            .with_embed_pairs([("c.md", "a.md")]);
        assert_eq!(graph.forward_links("a.md"), ["b.md"]);
        assert_eq!(graph.forward_embeds("a.md"), ["c.md"]);
        assert_eq!(graph.back_embeds("c.md"), ["a.md"]);
    }

    #[test]
    fn unknown_path_returns_empty_slice() {
        let graph = DepGraph::build(std::iter::empty());
        assert!(graph.forward_links("nowhere.md").is_empty());
        assert!(graph.forward_embeds("nowhere.md").is_empty());
        assert!(graph.back_embeds("nowhere.md").is_empty());
    }
}
