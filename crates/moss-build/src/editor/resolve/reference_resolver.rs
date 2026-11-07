//! Host adapter for the portable editor reference resolver.
use crate::build::scan::article_map::ArticleMap;
use super::asset_resolver::{compute_from_source, reference_input, enrich, EditorScope};
pub use moss_core::resolve::editor::{RefTarget, EditorReferenceResolution, FolderEmbedInfo};
use std::path::Path;

/// Read one snapshot, resolve in core, then add host paths and asset metadata.
pub fn resolve_references_batch(targets: &[RefTarget], from_file: &str, project_root: &Path, scope: EditorScope) -> Vec<EditorReferenceResolution> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let from = compute_from_source(Path::new(from_file), &root);
    let mut input = reference_input(&root, scope, &from, targets);
    if scope == EditorScope::Site {
        // A missing map is distinct from a built site's empty URL index.
        let paths = crate::moss_paths::MossPaths::from_moss_dir(root.join(".moss"));
        if paths.article_map().is_file() {
            input.url_index = ArticleMap::load(&root.join(".moss")).ok().map(|map| super::url_index::reference_map(&map));
        }
    }
    moss_core::resolve::resolve_references(&input).into_iter().map(|mut out| {
        out.resolved = out.resolved.and_then(|asset| enrich(asset, &root));
        out.resolved_path = out.resolved_path.map(|path| root.join(path).to_string_lossy().to_string());
        out
    }).collect()
}

#[cfg(test)]
use moss_core::resolve::reference::ReferenceKind;
#[cfg(test)]
#[path = "reference_resolver_tests.rs"]
mod tests;
