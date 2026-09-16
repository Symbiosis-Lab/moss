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
//! which routes through `resolve_asset_ref` + FsAssetIndex — FS-backed,
//! always available. This is the correct editor path for ALL reference kinds.

use crate::build::scan::article_map::ArticleMap;
use crate::editor::resolve::asset_resolver::FsAssetIndex;
use crate::editor::resolve::folder_index::EditorFolderIndex;
use crate::editor::resolve::url_index::ArticleMapIndex;
use crate::editor::ref_rewrite::{
    matches_target, rewrite_for_removal, rewrite_refs_by_raw_match, RefAmbiguity,
};
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
fn build_indexes(root: &Path) -> (FsAssetIndex, EditorFolderIndex, ArticleMapIndex) {
    let fs_assets = FsAssetIndex::new(root);
    // ArticleMap: empty is fine for LINK resolution — we never use
    // is_embed:false (see module doc). It is NOT optional for the folder
    // index, which reconstructs the build's folder-index URL set from it.
    let moss_dir = root.join(".moss");
    let map = ArticleMap::load(&moss_dir).unwrap_or_default();
    let fs_folders = EditorFolderIndex::new(root, &map);
    let article_idx = ArticleMapIndex::from_map(&map);
    (fs_assets, fs_folders, article_idx)
}


/// Scan every `.md` file in the project for references that resolve to
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

    for entry in walkdir::WalkDir::new(&canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            // Only .md files, not inside .moss/ or hidden dirs
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "markdown" { return false; }
            // Skip inside .moss/
            let rel = p.strip_prefix(&canonical_root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            !rel.starts_with(".moss/") && !rel.starts_with(".git/")
        })
    {
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

/// Remove every reference to each of `paths` from every `.md` file in the
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

/// Remove every reference to `target_abs` from every `.md` file under
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

    for entry in walkdir::WalkDir::new(canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "markdown" { return false; }
            let rel = p.strip_prefix(canonical_root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            !rel.starts_with(".moss/") && !rel.starts_with(".git/")
        })
    {
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
/// `[text](link)` reference to it. The project root is passed in explicitly:
/// the app's `rename_entry_with_refs` command and `moss rename` in both
/// binaries (`cli::rename`) call this same body.
pub fn rename_entry_with_refs_core(
    project_root: std::path::PathBuf,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    // Perform the OS rename first (same logic as rename_entry_inner).
    crate::vault::fs::rename_entry_inner(&project_root, old_path, new_path)?;

    // Now rewrite references project-wide.
    let canonical_root = std::fs::canonicalize(&project_root)
        .map_err(|e| format!("Cannot canonicalize root: {}", e))?;

    // Compute old root-relative (before rename, so use the path string directly).
    // The old path no longer exists on disk (it has been renamed), so we cannot
    // canonicalize it — strip the root prefix from the raw string instead.
    let old_root_rel = Path::new(&old_path)
        .strip_prefix(&canonical_root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| {
            // Fallback: strip the root prefix exactly ONCE (a repeated
            // trim_start_matches could chew through later path segments that
            // happen to equal the root string), then drop a single leading '/'.
            let stripped = old_path
                .strip_prefix(canonical_root.to_str().unwrap_or(""))
                .unwrap_or(&old_path);
            let stripped = stripped.strip_prefix('/').unwrap_or(stripped);
            stripped.replace('\\', "/")
        });

    // Compute new root-relative (after rename, the new path now exists)
    let new_abs = Path::new(&new_path);
    let new_abs_canonical = std::fs::canonicalize(new_abs)
        .map_err(|e| format!("Cannot canonicalize new path '{}': {}", new_path, e))?;
    let new_root_rel = new_abs_canonical
        .strip_prefix(&canonical_root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| new_path.trim_start_matches('/').to_string());

    let target_is_dir = new_abs_canonical.is_dir();

    // For a FILE rename, decide whether a BARE reference to the old entry is
    // safe to rewrite. One walk, two answers — see `RefAmbiguity`. The
    // newly-renamed file already carries the NEW name, so any remaining hit
    // is a genuine collision.
    let old_stem = Path::new(&old_root_rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(old_root_rel.as_str())
        .to_string();
    let old_name = Path::new(&old_root_rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(old_root_rel.as_str())
        .to_string();
    let amb = if target_is_dir {
        RefAmbiguity { stem_unique: true, name_unique: true } // unused for folders
    } else {
        let mut same_stem = 0usize;
        let mut same_name = 0usize;
        for entry in walkdir::WalkDir::new(&canonical_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let rel = p
                .strip_prefix(&canonical_root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if rel.starts_with(".moss/") || rel.starts_with(".git/") {
                continue;
            }
            if p.file_name().and_then(|s| s.to_str()) == Some(old_name.as_str()) {
                same_name += 1;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "markdown" {
                continue;
            }
            if p.file_stem().and_then(|s| s.to_str()) == Some(old_stem.as_str()) {
                same_stem += 1;
            }
        }
        RefAmbiguity { stem_unique: same_stem == 0, name_unique: same_name == 0 }
    };

    // Walk every .md and rewrite references from old to new.
    // Since the file has already been renamed, the old path no longer exists —
    // we use raw text matching via rewrite_refs_by_raw_match.
    for entry in walkdir::WalkDir::new(&canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "markdown" { return false; }
            let rel = p.strip_prefix(&canonical_root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            !rel.starts_with(".moss/") && !rel.starts_with(".git/")
        })
    {
        let file_path = entry.path();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // For rewrite, we need to find refs to old_root_rel (which no longer exists
        // on disk). The indexes are built from new state — the old path won't resolve.
        // We use raw text matching: any ref whose raw text matches old_root_rel or
        // whose stem matches the old filename stem.
        // Directory of the referencing file, for document-relative refs.
        let from_dir = file_path
            .parent()
            .and_then(|d| d.strip_prefix(&canonical_root).ok())
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let rewritten = rewrite_refs_by_raw_match(
            &source,
            &from_dir,
            &old_root_rel,
            &new_root_rel,
            target_is_dir,
            amb,
        );
        if rewritten != source {
            // allow:raw_write the vault's own .md source, rewritten in place after a rename -- not build output
            std::fs::write(file_path, &rewritten)
                .map_err(|e| format!("Failed to write '{}': {}", file_path.display(), e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "ref_scan_tests.rs"]
mod tests;
