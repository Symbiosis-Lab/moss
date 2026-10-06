//! The file-reference view of [`ContentGraph::resolve_path`], the one function
//! that decides which site file a target names.
//!
//! Standard images, shortcode and frontmatter asset paths, the editor's
//! classifier and the rename, delete and scan tools all ask "which file?"
//! through [`resolve_file_target`], which adds only what a caller of a file
//! needs on top of that function: a label for how the target matched, taken
//! from the resolver's own report, and "ambiguous" for files that are exactly
//! as near to the page as the pick. It never returns a path the graph does not
//! hold, so nothing outside the site is ever named.

use crate::content_graph::{ContentGraph, PathMatch};

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetProvenance {
    Literal,
    BareFuzzy,
    SeparatorFallback,
    CaseMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetResolution {
    Resolved { root_rel: String, provenance: AssetProvenance },
    /// Several files are equally near the page. `chosen` is the one the build
    /// links; `candidates` holds every tied file, `chosen` included, sorted.
    Ambiguous { chosen: String, candidates: Vec<String> },
    NotFound,
}

/// Which site file `target`, written in `from_source`, names, as
/// [`ContentGraph::resolve_path_with_ties`] answers it. A pick that is not a
/// real file (the synthetic home page of a folder with no note) is not a file
/// reference and counts as not found.
///
/// The label: written out exactly (from the page, or from the site root with
/// a leading `/`) is `Literal`; the right path in other letter case is
/// `CaseMismatch`; a root path written without the leading `/`, or a partial
/// path found by search, is `SeparatorFallback`; a bare name found by search
/// is `BareFuzzy`.
pub fn resolve_file_target(target: &str, from_source: &str, graph: &ContentGraph) -> AssetResolution {
    let Some(found) = graph.resolve_path_with_ties(target, from_source).filter(|r| graph.contains_path(&r.path))
    else {
        return AssetResolution::NotFound;
    };
    if !found.ties.is_empty() {
        let mut candidates = found.ties;
        candidates.push(found.path.clone());
        candidates.sort();
        return AssetResolution::Ambiguous { chosen: found.path, candidates };
    }
    let provenance = match found.matched {
        PathMatch::Written { exact_case: true } => AssetProvenance::Literal,
        PathMatch::FromRoot { exact_case: true } => AssetProvenance::SeparatorFallback,
        PathMatch::Written { .. } | PathMatch::FromRoot { .. } => AssetProvenance::CaseMismatch,
        PathMatch::Searched if target.contains('/') => AssetProvenance::SeparatorFallback,
        PathMatch::Searched => AssetProvenance::BareFuzzy,
    };
    AssetResolution::Resolved { root_rel: found.path, provenance }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(paths: &[&str], target: &str, from: &str) -> AssetResolution {
        resolve_file_target(target, from, &ContentGraph::from_paths(paths))
    }
    fn resolved(root_rel: &str, provenance: AssetProvenance) -> AssetResolution {
        AssetResolution::Resolved { root_rel: root_rel.into(), provenance }
    }

    #[test]
    fn literal_exact_relative_hit() {
        let r = resolve(&["assets/Hoon.JPG", "team/photo.jpg"], "./photo.jpg", "team/Team.md");
        assert_eq!(r, resolved("team/photo.jpg", AssetProvenance::Literal));
    }
    #[test]
    fn separator_fallback_to_root() {
        let r = resolve(&["assets/AGU2025.jpg"], "./assets/AGU2025.jpg", "News/2025-12-agu.md");
        assert_eq!(r, resolved("assets/AGU2025.jpg", AssetProvenance::SeparatorFallback));
    }
    #[test]
    fn case_mismatch_reports_the_real_case() {
        let r = resolve(&["assets/Hoon.JPG"], "./assets/Hoon.jpg", "Team.md");
        assert_eq!(r, resolved("assets/Hoon.JPG", AssetProvenance::CaseMismatch));
    }
    #[test]
    fn bare_name_is_found_by_search() {
        let r = resolve(&["assets/AGU2025.jpg"], "AGU2025.jpg", "News/post.md");
        assert_eq!(r, resolved("assets/AGU2025.jpg", AssetProvenance::BareFuzzy));
    }
    #[test]
    fn a_bare_name_at_the_root_is_found_by_search_in_any_case() {
        // A name without a folder never spells out a root path, even when the
        // only copy happens to sit at the root.
        assert_eq!(resolve(&["x.jpg"], "x.jpg", "News/post.md"), resolved("x.jpg", AssetProvenance::BareFuzzy));
        assert_eq!(resolve(&["X.jpg"], "x.jpg", "News/post.md"), resolved("X.jpg", AssetProvenance::BareFuzzy));
    }
    #[test]
    fn the_copy_beside_the_page_wins() {
        let r = resolve(&["News/photo.jpg", "assets/photo.jpg"], "photo.jpg", "News/post.md");
        assert_eq!(r, resolved("News/photo.jpg", AssetProvenance::Literal));
    }
    #[test]
    fn the_nearest_copy_wins_not_the_shallowest() {
        let r = resolve(&["z/far.jpg", "a/x/y/far.jpg"], "far.jpg", "a/x/page.md");
        assert_eq!(r, resolved("a/x/y/far.jpg", AssetProvenance::BareFuzzy));
    }
    #[test]
    fn equally_near_copies_are_ambiguous_with_the_builds_pick() {
        let r = resolve(&["a/photo.jpg", "b/photo.jpg"], "photo.jpg", "post.md");
        assert_eq!(r, AssetResolution::Ambiguous {
            chosen: "a/photo.jpg".into(),
            candidates: vec!["a/photo.jpg".into(), "b/photo.jpg".into()] });
    }
    #[test]
    fn absolute_path_resolves_from_root() {
        let r = resolve(&["assets/x.jpg"], "/assets/x.jpg", "News/post.md");
        assert_eq!(r, resolved("assets/x.jpg", AssetProvenance::Literal));
    }
    #[test]
    fn a_target_climbing_out_of_the_site_is_found_by_name_like_the_wiki_form() {
        let r = resolve(&["assets/x.jpg"], "../../etc/x.jpg", "News/post.md");
        assert_eq!(r, resolved("assets/x.jpg", AssetProvenance::SeparatorFallback));
    }
    #[test]
    fn a_name_written_in_another_unicode_form_is_still_written_exactly() {
        // The file is stored composed (NFC); the text is decomposed (NFD).
        let r = resolve(&["a/caf\u{e9}.jpg"], "cafe\u{301}.jpg", "a/p.md");
        assert_eq!(r, resolved("a/caf\u{e9}.jpg", AssetProvenance::Literal));
        let r = resolve(&["a/caf\u{e9}.jpg"], "Cafe\u{301}.jpg", "a/p.md");
        assert_eq!(r, resolved("a/caf\u{e9}.jpg", AssetProvenance::CaseMismatch));
    }
    #[test]
    fn a_name_that_literally_contains_percent_20_wins_over_the_decoded_one() {
        let r = resolve(&["a/my%20note.jpg", "a/my note.jpg"], "my%20note.jpg", "a/p.md");
        assert_eq!(r, resolved("a/my%20note.jpg", AssetProvenance::Literal));
        let r = resolve(&["a/my note.jpg"], "my%20note.jpg", "a/p.md");
        assert_eq!(r, resolved("a/my note.jpg", AssetProvenance::Literal));
    }
    #[test]
    fn a_folder_without_a_note_is_not_a_file() {
        let mut b = crate::content_graph::ContentGraphBuilder::new();
        b.add_file("notes/a.md", "");
        b.register_auto_index_dir("Gallery");
        let g = b.build();
        assert_eq!(resolve_file_target("Gallery", "p.md", &g), AssetResolution::NotFound);
    }
}
