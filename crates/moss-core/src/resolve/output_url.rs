//! Pure output-path + output-relative-URL helpers shared by every asset/embed/
//! folder emitter. `resolve_path_with_overrides` slugifies intermediate dir
//! segments (generate_slug) and preserves the leaf; `pinned_url` is the one
//! way a resolved source path becomes a URL in emitted HTML;
//! `reference_output_url` computes the relative URL between two source paths IN
//! OUTPUT SPACE so a shared mixed-case prefix cancels (the Bug A fix,
//! generalized).
//!
//! NOTE: this calls `crate::slug::generate_slug` (the text-to-slug primitive
//! page_map.rs's original used via the `super::slug::generate_slug` re-export),
//! NOT `crate::content_graph::generate_slug` (the path-to-key transform that
//! strips extensions). The two differ; `resolve_path_with_overrides` only ever
//! slugifies intermediate *directory* segments, never the leaf filename.

use crate::resolve::fuzzy_path::relative_asset_path;
use crate::slug::{generate_slug, normalize_separators};
use std::collections::HashMap;

pub fn resolve_path_with_overrides(path: &str, overrides: &HashMap<String, String>) -> String {
    // Normalize `\`→`/` so a Windows-authored source path slugs into the same
    // nested output as its `/`-form (otherwise the whole path is one leaf segment).
    let normalized = normalize_separators(path);
    let segments: Vec<&str> = normalized.split('/').collect();
    let last_idx = segments.len().saturating_sub(1);
    let mut resolved: Vec<String> = Vec::new();
    let mut cumulative = String::new();
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            cumulative.push('/');
        }
        cumulative.push_str(seg);
        if let Some(override_slug) = overrides.get(&cumulative) {
            resolved.push(override_slug.clone());
        } else if i == last_idx {
            // Preserve filename / leaf-segment case so asset file references
            // (covers, video sources, image variants) keep matching the
            // file on disk.
            resolved.push((*seg).to_string());
        } else {
            // Slugify intermediate directory segments so source folder
            // casing/punctuation cannot leak into the URL or output path.
            resolved.push(generate_slug(seg));
        }
    }
    resolved.join("/")
}

/// The **pinned URL** for a resolved source path: the one URL the site serves
/// that file at, independent of which page references it.
///
/// This is the only sanctioned way a graph-resolved reference (asset, embed,
/// non-page file link) becomes a URL in emitted HTML. Properties that make it
/// correct by construction:
///
/// - **Root-absolute.** The referencing page's depth cannot enter the result,
///   so the same target resolves identically from the vault root and from a
///   note nested three folders down. The pretty-URL `../` compensation that
///   page-relative asset URLs need (`article.md` is served at `article/`, one
///   level deeper than the source file) has nothing to compensate for.
/// - **Case-canonical.** Intermediate directory segments go through the same
///   `resolve_path_with_overrides` the asset copier uses to place the file, so
///   a mixed-case source folder (`MIRROR/`) yields the slug the bytes actually
///   land under (`/mirror/`). No caller re-derives it by string surgery on a
///   folder name, and no caller needs a case-insensitive retry.
/// - **Encoded once.** Every segment is percent-encoded here, at the single
///   point where the path becomes a URL.
///
/// `root_rel` is a source path relative to the vault root, as returned by
/// `ContentGraph::resolve_path` / the asset engine. A leading `/` is tolerated
/// (author-written absolute refs arrive that way) and re-emitted.
///
/// Prefer [`crate::content_graph::ContentGraph::pinned_url`], which carries the
/// build's overrides — call this directly only where no graph is in scope.
pub fn pinned_url(root_rel: &str, overrides: &HashMap<String, String>) -> String {
    let stripped = root_rel.strip_prefix('/').unwrap_or(root_rel);
    let mapped = resolve_path_with_overrides(stripped, overrides);
    format!(
        "/{}",
        crate::resolve::fuzzy_path::percent_encode_path_segments(&mapped)
    )
}

/// Output-relative URL from `from_source`'s page to `target_source`, both
/// mapped through `resolve_path_with_overrides` so the result matches the
/// slugified output tree.
pub fn reference_output_url(
    from_source: &str,
    target_source: &str,
    overrides: &HashMap<String, String>,
) -> String {
    let from_out = resolve_path_with_overrides(from_source, overrides);
    let target_out = resolve_path_with_overrides(target_source, overrides);
    relative_asset_path(&from_out, &target_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_intermediate_preserves_leaf() {
        let o = HashMap::new();
        assert_eq!(
            resolve_path_with_overrides("Resources/cities-heat-map-app/index.html", &o),
            "resources/cities-heat-map-app/index.html"
        );
        assert_eq!(resolve_path_with_overrides("My App/index.html", &o), "my-app/index.html");
    }

    #[test]
    fn resolve_path_handles_backslash_separators() {
        let o = HashMap::new();
        // A backslash-separated source (Windows) must resolve to the same `/`-form
        // output — intermediate dir slugged, leaf case preserved — not collapse
        // into one segment (the asset-URL 404 bug).
        assert_eq!(
            resolve_path_with_overrides("Sub Dir\\Winter-Song.mov", &o),
            "sub-dir/Winter-Song.mov"
        );
        assert_eq!(
            resolve_path_with_overrides("Sub Dir\\Winter-Song.mov", &o),
            resolve_path_with_overrides("Sub Dir/Winter-Song.mov", &o),
        );
        assert!(!resolve_path_with_overrides("A\\B\\index.html", &o).contains('\\'));
    }

    #[test]
    fn pinned_url_is_depth_independent_and_case_canonical() {
        let o = HashMap::new();
        // The same target, referenced from any depth, is the same URL — and the
        // mixed-case source folder resolves to the slug the file lands under.
        assert_eq!(
            pinned_url("MIRROR/在場/cover-IMG.png", &o),
            "/mirror/%E5%9C%A8%E5%A0%B4/cover-IMG.png"
        );
        // Leading `/` (author-written absolute ref) is tolerated, not doubled.
        assert_eq!(
            pinned_url("/MIRROR/cover-IMG.png", &o),
            pinned_url("MIRROR/cover-IMG.png", &o)
        );
        // An explicit override wins over base slugification.
        let mut with_override = HashMap::new();
        with_override.insert("图片".to_string(), "images".to_string());
        assert_eq!(
            pinned_url("图片/photo.jpg", &with_override),
            "/images/photo.jpg"
        );
        // Spaces in a directory name slug away; the leaf keeps its bytes and is
        // encoded (never re-encoded — `pinned_url` takes SOURCE paths, and is
        // the single point where a path becomes a URL).
        assert_eq!(
            pinned_url("My Photos/a b.jpg", &o),
            "/my-photos/a%20b.jpg"
        );
    }

    #[test]
    fn output_url_cancels_shared_mixed_case_prefix() {
        let o = HashMap::new();
        assert_eq!(
            reference_output_url("Resources/index.md", "Resources/app/index.html", &o),
            "app/index.html"
        );
        assert_eq!(
            reference_output_url("Research.md", "Resources/app/index.html", &o),
            "resources/app/index.html"
        );
    }
}
