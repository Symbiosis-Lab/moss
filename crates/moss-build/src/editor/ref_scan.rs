//! Reference scan engine — find and rewrite every reference to a project
//! file, used by rename-with-refs and delete-with-refs. The app's three
//! `#[tauri::command]` wrappers stay app-side in `editor/ref_scan.rs`; the
//! bodies live here so `moss rename` answers in both binaries.
//!
//! ## Why is_embed:true everywhere?
//!
//! `classify_reference(is_embed:false)` routes through `classify_link` →
//! ArticleMapIndex (the *deployed URL space*), which may not exist in the
//! editor (no published site). Calling it with `is_embed:false` for a
//! `[[note]]` or `[text](note.md)` reference returns NotFound/no target_path
//! in the editor context. We ALWAYS call `classify_reference(is_embed:true)`,
//! which resolves through the one resolver (`ContentGraph::resolve_path`) over
//! the project's files read from disk, always available. This is the correct
//! editor path for ALL reference kinds, and picks the file the build links.

use crate::build::scan::article_map::ArticleMap;
use crate::build::scan::classify::{is_page_path, left_out_of_site, LeftOut};
use crate::editor::resolve::asset_resolver::project_graph;
use moss_core::content_graph::ContentGraph;
use crate::editor::resolve::folder_index::EditorFolderIndex;
use crate::editor::resolve::url_index::ArticleMapIndex;
use crate::editor::ref_rewrite::{matches_target, rewrite_for_removal};
use moss_core::resolve::md_extract::{
    extract_md_references, extract_structural_asset_refs, AssetPathSpan,
};
use moss_core::resolve::reference::{classify_reference, ReferenceContext};
use std::path::Path;

/// One reference found in a file that points to a given target.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FileReferenceHit {
    /// Absolute path of the file that contains the reference.
    pub referencing_file: String,
    /// The raw reference text (the inner part of the wikilink / markdown link).
    pub ref_text: String,
    /// 1-based line number of the reference in the referencing file.
    pub line: u32,
}

/// Build the three indexes from the project root (canonicalized).
/// Reused by scan and rewrite to avoid rebuilding per-file.
pub(crate) fn build_indexes(root: &Path) -> (ContentGraph, EditorFolderIndex, ArticleMapIndex) {
    let fs_assets = project_graph(root);
    // ArticleMap: empty is fine for LINK resolution — we never use
    // is_embed:false (see module doc). It is NOT optional for the folder
    // index, which reconstructs the build's folder-index URL set from it.
    let moss_dir = root.join(".moss");
    let map = ArticleMap::load(&moss_dir).unwrap_or_default();
    let fs_folders = EditorFolderIndex::new(root, &map);
    let article_idx = ArticleMapIndex::from_map(&map);
    (fs_assets, fs_folders, article_idx)
}


/// Every page whose links point into the site under `root`: each file the
/// build's scan turns into a page ([`left_out_of_site`], [`is_page_path`]),
/// so a nested site or a hidden folder is never scanned or rewritten, plus the
/// root agent instructions and dot-prefixed pages. Those are not published and so are never the file
/// a reference names, but they are the author's own files and their links are
/// kept current like any page's.
fn site_pages(root: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| matches!(left_out_of_site(e), None | Some(LeftOut::AgentInstructions | LeftOut::HiddenFile)))
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_page_path(e.path()))
}

/// Scan every page ([`site_pages`]) for references that resolve to
/// `target_abs` (absolute path). Returns one hit per reference found.
///
/// ALWAYS calls `classify_reference(is_embed:true)` — see module doc.
pub fn scan_project_references_to(
    target_abs: &Path,
    project_root: &Path,
) -> Result<Vec<FileReferenceHit>, String> {
    let canonical_root = std::fs::canonicalize(project_root)
        .map_err(|e| format!("Cannot canonicalize root: {}", e))?;
    let canonical_target = std::fs::canonicalize(target_abs)
        .map_err(|e| format!("Cannot canonicalize target '{}': {}", target_abs.display(), e))?;

    // Compute root-relative target path (forward slashes).
    let target_root_rel = canonical_target
        .strip_prefix(&canonical_root)
        .map_err(|_| format!("Target '{}' is not inside project root", target_abs.display()))?
        .to_string_lossy()
        .replace('\\', "/");

    let target_is_dir = canonical_target.is_dir();

    let (fs_assets, fs_folders, article_idx) = build_indexes(&canonical_root);
    let ctx = ReferenceContext {
        assets: &fs_assets,
        folders: &fs_folders,
        urls: &article_idx,
    };

    let mut hits = Vec::new();

    for entry in site_pages(&canonical_root) {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue, // Skip unreadable files silently
        };

        // Root-relative from_source for the classifier.
        let from_source = file_path
            .strip_prefix(&canonical_root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        // (ref_text, byte offset) from BOTH passes: bracketed markdown tokens
        // and structural asset paths (bare gallery lines, `image=` attrs,
        // frontmatter `cover:`). Without the second, deleting an image
        // referenced only by a bare gallery line showed "no references".
        let found: Vec<(String, usize)> = extract_md_references(&source)
            .into_iter()
            .map(|rr| (rr.text, rr.byte_from))
            .chain(
                extract_structural_asset_refs(&source)
                    .into_iter()
                    .map(|sp: AssetPathSpan| (sp.path, sp.value.start)),
            )
            .collect();

        for (ref_text, byte_from) in found {
            // ALWAYS is_embed:true — see module doc.
            let resolved = classify_reference(&ref_text, &from_source, true, &ctx);
            let target_path = match &resolved.target_path {
                Some(p) => p.as_str(),
                None => continue, // NotFound / Link / External / Anchor → skip
            };

            if !matches_target(target_path, &target_root_rel, target_is_dir) {
                continue;
            }

            // Compute 1-based line number from byte offset.
            let line = source
                .get(..byte_from)
                .unwrap_or_default()
                .chars()
                .filter(|&c| c == '\n')
                .count() as u32
                + 1;

            hits.push(FileReferenceHit {
                referencing_file: file_path.to_string_lossy().to_string(),
                ref_text,
                line,
            });
        }
    }

    Ok(hits)
}

/// Remove every reference to each of `paths` from every page ([`site_pages`]) in the
/// project. The app's `clean_references_and_delete` command trashes the paths
/// after this; the body itself touches nothing but the referencing files.
pub fn clean_references_to_paths(project_root: &Path, paths: &[String]) -> Result<(), String> {
    let canonical_root = std::fs::canonicalize(project_root)
        .map_err(|e| format!("Cannot canonicalize root: {}", e))?;
    let (fs_assets, fs_folders, article_idx) = build_indexes(&canonical_root);
    let ctx = ReferenceContext {
        assets: &fs_assets,
        folders: &fs_folders,
        urls: &article_idx,
    };
    for path in paths {
        let abs = Path::new(path);
        if !abs.exists() {
            continue;
        }
        clean_references_to(&canonical_root, abs, &ctx)?;
    }
    Ok(())
}

/// Remove every reference to `target_abs` from every page ([`site_pages`]) under
/// `canonical_root`. The delete-side counterpart of
/// `rename_entry_with_refs_core`.
fn clean_references_to(
    canonical_root: &Path,
    target_abs: &Path,
    ctx: &ReferenceContext<'_>,
) -> Result<(), String> {
    let canonical_target = match std::fs::canonicalize(target_abs) {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    // A target outside the project root is an error, not a silent "" (which
    // would coerce every ref to "match" the empty root-relative prefix).
    let target_root_rel = canonical_target
        .strip_prefix(canonical_root)
        .map_err(|_| format!("Path '{}' is not inside project root", target_abs.display()))?
        .to_string_lossy()
        .replace('\\', "/");
    let target_is_dir = canonical_target.is_dir();

    for entry in site_pages(canonical_root) {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let from_source = file_path
            .strip_prefix(canonical_root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let rewritten =
            rewrite_for_removal(&source, &from_source, &target_root_rel, ctx, target_is_dir);
        if rewritten != source {
            // allow:raw_write the vault's own .md source, rewritten in place after a rename -- not build output
            std::fs::write(file_path, &rewritten)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;
        }
    }
    Ok(())
}


/// OS-rename the entry, then rewrite every project-wide `[[wikilink]]` /
/// `[text](link)` reference to it: `moss rename`'s one-move wrapper around
/// [`crate::editor::rename_plan`]'s resolver-driven plan and apply.
///
/// Returns the [`crate::editor::rename_plan::RenameApplyResult`]
/// `apply_planned_moves` produced — precise enough for
/// `rename_plan::undo_applied` to reverse — rather than discarding it: a
/// caller that never gets this back (the CLI prints and moves on) has no way
/// to offer "undo" for a rename it just made.
pub fn rename_entry_with_refs_core(
    project_root: std::path::PathBuf,
    old_path: &str,
    new_path: &str,
) -> Result<crate::editor::rename_plan::RenameApplyResult, String> {
    let plan = crate::editor::rename_plan::plan_moves(
        &project_root,
        &[(old_path.to_string(), new_path.to_string())],
    )?;
    crate::editor::rename_plan::apply_planned_moves(&project_root, &plan)
}

/// The page files among `file_moves` (root-relative `(old, new)` pairs), for
/// a host's watcher to read as renames: a folder move lists every page in it.
pub fn page_file_pairs(file_moves: &[(String, String)]) -> Vec<(String, String)> {
    file_moves.iter().filter(|(_, new)| is_page_path(Path::new(new))).cloned().collect()
}

/// Renames or moves in a loose document's folder, which is not a site: no
/// link is rewritten. `pairs` are the root-relative page moves (those inside a
/// moved folder too), for a host's watcher; [`Self::apply`] moves everything or
/// nothing.
pub struct LoosePlan {
    pub pairs: Vec<(String, String)>,
    moves: Vec<(std::path::PathBuf, String)>,
}

/// Plan `moves` (absolute `(old, new)`): each source must be inside the folder
/// and each destination still free.
pub fn plan_loose(project_root: &Path, moves: &[(String, String)]) -> Result<LoosePlan, String> {
    let mut planned = Vec::with_capacity(moves.len());
    for (old, new) in moves {
        let (_, old_c) = crate::vault::fs::recheck_canonical(project_root, Path::new(old))?;
        if Path::new(new).exists() {
            return Err(format!("'{new}' already exists"));
        }
        planned.push((old_c, new.clone()));
    }
    // Spelled relative to the folder as given; a path outside it is not announced.
    let rel = |p: &Path| p.strip_prefix(project_root).ok().map(|r| r.to_string_lossy().replace('\\', "/"));
    let mut pairs = Vec::new();
    for (old, new) in moves {
        let (old, new) = (Path::new(old), Path::new(new));
        for entry in walkdir::WalkDir::new(old).into_iter().filter_map(Result::ok) {
            let inner = entry.path().strip_prefix(old).unwrap_or(Path::new(""));
            let moved = if inner.as_os_str().is_empty() { new.to_path_buf() } else { new.join(inner) };
            if is_page_path(&moved) {
                if let (Some(o), Some(n)) = (rel(entry.path()), rel(&moved)) {
                    pairs.push((o, n));
                }
            }
        }
    }
    Ok(LoosePlan { pairs, moves: planned })
}

impl LoosePlan {
    /// A failure puts back the entries already moved.
    pub fn apply(&self) -> Result<(), String> {
        let mut done: Vec<&(std::path::PathBuf, String)> = Vec::new();
        for mv in &self.moves {
            if let Err(e) = std::fs::rename(&mv.0, &mv.1) {
                for (back, from) in done.iter().rev().map(|m| (&m.0, &m.1)) {
                    let _ = std::fs::rename(from, back);
                }
                return Err(format!("Failed to move '{}': {e}", mv.0.display()));
            }
            done.push(mv);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "ref_scan_tests.rs"]
mod tests;
