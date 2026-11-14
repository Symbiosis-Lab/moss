//! Commands that create, save, rename and delete files in the folder.

use serde_json::Value;

use super::rename_undo::RenameRefsSummary;
use super::{resolve, lossy, site_root, with_moves_announced, Change, CommandHost, VaultPath};
use crate::editor::rename_plan::UndoResult;

fn resolve_all(host: &dyn CommandHost, paths: &[VaultPath]) -> Result<Vec<String>, String> {
    paths.iter().map(|p| resolve(host, p).map(|pb| lossy(&pb))).collect()
}

#[derive(serde::Deserialize, specta::Type)]
pub struct CreateFilesArgs {
    pub files: Vec<crate::vault::fs::NewFile>,
}

/// All files or none; returns their absolute paths in order.
pub async fn create_files(host: &dyn CommandHost, a: CreateFilesArgs) -> Result<Vec<String>, String> {
    let made = crate::vault::fs::create_files_inner(host.root(), &a.files)?;
    host.changed(Change::Created(&made));
    Ok(made)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateFolderArgs {
    pub parent_dir: String,
    pub name: String,
}

pub async fn create_folder(host: &dyn CommandHost, a: CreateFolderArgs) -> Result<String, String> {
    crate::vault::fs::create_folder_inner(host.root(), &a.parent_dir, &a.name)
}

#[derive(serde::Deserialize, specta::Type)]
pub struct PathArgs {
    pub path: VaultPath,
}

#[derive(serde::Deserialize, specta::Type)]
pub struct PathsArgs {
    pub paths: Vec<VaultPath>,
}

pub async fn delete_entry(host: &dyn CommandHost, a: PathArgs) -> Result<(), String> {
    delete_entries(host, PathsArgs { paths: vec![a.path] }).await
}

/// Moves each path to the OS trash; the core re-checks each one's containment.
pub async fn delete_entries(host: &dyn CommandHost, a: PathsArgs) -> Result<(), String> {
    crate::vault::fs::delete_entries_inner(host.root(), &resolve_all(host, &a.paths)?)
}

/// The references a delete of `paths` would break, for its confirmation. A
/// loose document's folder has no links to break. A reference to a file inside
/// a deleted folder is counted once.
pub async fn scan_references_for_delete(
    host: &dyn CommandHost,
    a: PathsArgs,
) -> Result<Vec<crate::editor::ref_scan::FileReferenceHit>, String> {
    let Ok(root) = site_root(host) else { return Ok(Vec::new()) };
    let mut hits = Vec::new();
    for path in &a.paths {
        let abs = resolve(host, path)?;
        if abs.exists() {
            hits.extend(crate::editor::ref_scan::scan_project_references_to(&abs, root)?);
        }
    }
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert((h.referencing_file.clone(), h.ref_text.clone(), h.line)));
    Ok(hits)
}

/// Copies a file or folder beside itself under the first free `-2`, `-3`, …
/// name, skipping `.moss/`, `node_modules/` and `.git/` inside a folder, and
/// returns the copy's path. Each copy waits for a cloud-evicted source; a
/// folder that cannot be copied whole leaves nothing behind.
pub async fn duplicate_entry(host: &dyn CommandHost, a: PathArgs) -> Result<String, String> {
    use crate::build::cloud_readiness::INTERACTIVE_DEADLINE;
    use crate::editor::copy_in::{copy_dir_for_import, copy_file_into_vault};
    let src = resolve(host, &a.path)?;
    if !src.exists() {
        return Err(format!("Path does not exist: {}", src.display()));
    }
    let (root, src_canonical) = crate::vault::fs::recheck_canonical(host.root(), &src)?;
    if src_canonical == root {
        return Err("Cannot duplicate the folder itself".to_string());
    }
    let dest = crate::editor::filesystem::resolve_collision(&src);
    if src.is_dir() {
        let mut lost = Vec::new();
        copy_dir_for_import(&src, &dest, std::time::Instant::now() + INTERACTIVE_DEADLINE, &mut lost);
        if !lost.is_empty() {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("Duplicated '{}' without {} of its entries", src.display(), lost.len()));
        }
    } else {
        copy_file_into_vault(&src, &dest, INTERACTIVE_DEADLINE)
            .map_err(|e| format!("Failed to duplicate '{}': {}", src.display(), e))?;
    }
    let made = lossy(&dest);
    host.changed(Change::Created(std::slice::from_ref(&made)));
    Ok(made)
}

/// Strips every reference to `paths` from the folder's pages (a loose
/// document's folder has none), then trashes them.
pub async fn clean_references_and_delete(host: &dyn CommandHost, a: PathsArgs) -> Result<(), String> {
    let paths = resolve_all(host, &a.paths)?;
    if let Ok(root) = site_root(host) {
        crate::editor::ref_scan::clean_references_to_paths(root, &paths)?;
    }
    crate::vault::fs::delete_entries_inner(host.root(), &paths)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RenameArgs {
    pub old_path: VaultPath,
    pub new_path: VaultPath,
}

/// Renames or moves one entry and rewrites every reference that follows it.
pub async fn rename_entry_with_refs(host: &dyn CommandHost, a: RenameArgs) -> Result<RenameRefsSummary, String> {
    let old = lossy(&resolve(host, &a.old_path)?);
    let new = lossy(&resolve(host, &a.new_path)?);
    apply_moves(host, &[(old, new)])
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MoveEntriesArgs {
    pub paths: Vec<VaultPath>,
    pub target_dir: VaultPath,
}

/// Moves entries into one folder (see [`crate::editor::rename_plan::destinations::into_folder`]
/// for where each lands), in one plan, with one undo.
pub async fn move_entries(host: &dyn CommandHost, a: MoveEntriesArgs) -> Result<RenameRefsSummary, String> {
    let target = resolve(host, &a.target_dir)?;
    let moves = crate::editor::rename_plan::destinations::into_folder(host.root(), &resolve_all(host, &a.paths)?, &target)?;
    apply_moves(host, &moves)
}

/// Applies absolute `(from, to)` moves all or none, announced to the host's
/// watcher, and rewrites every reference that follows them. In a loose
/// document's folder nothing is rewritten and there is no undo.
fn apply_moves(host: &dyn CommandHost, moves: &[(String, String)]) -> Result<RenameRefsSummary, String> {
    use crate::editor::{ref_scan, rename_plan};
    if moves.is_empty() {
        return Ok(RenameRefsSummary::empty());
    }
    let Ok(root) = site_root(host) else {
        let plan = ref_scan::plan_loose(host.root(), moves)?;
        with_moves_announced(host, &plan.pairs, || plan.apply())?;
        return Ok(RenameRefsSummary::empty());
    };
    let plan = rename_plan::plan_moves(root, moves)?;
    let pairs = ref_scan::page_file_pairs(&plan.file_moves);
    let applied = with_moves_announced(host, &pairs, || rename_plan::apply_planned_moves(root, &plan))?;
    Ok(host.rename_undo().summarize_and_store(root, applied))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UndoRenameArgs {
    pub undo_id: String,
}

/// Reverses the rename `undo_id` names, once, and only in the folder it ran in.
pub async fn undo_rename(host: &dyn CommandHost, a: UndoRenameArgs) -> Result<UndoResult, String> {
    let (root, applied) = host
        .rename_undo()
        .take(&a.undo_id)
        .ok_or_else(|| format!("Nothing to undo for id '{}' (already undone, or too old)", a.undo_id))?;
    if root != host.root() {
        return Err("Cannot undo — the folder has changed since this rename".to_string());
    }
    let pairs = crate::editor::ref_scan::page_file_pairs(&crate::editor::rename_plan::undo_file_moves(&root, &applied)?);
    with_moves_announced(host, &pairs, || crate::editor::rename_plan::undo_applied(&root, &applied))
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveEditorContentArgs {
    pub file_path: VaultPath,
    pub frontmatter: Value,
    pub body: String,
    pub expected_hash: String,
    #[serde(default)]
    pub keep_displaced: bool,
}

/// Writes the page if the disk still holds `expected_hash`, or merges what
/// another writer saved; a writer whose text was displaced hears it as an event.
pub async fn save_editor_content(
    host: &dyn CommandHost,
    a: SaveEditorContentArgs,
) -> Result<crate::editor::content::SaveResult, String> {
    let path = lossy(&resolve(host, &a.file_path)?);
    let r = host.saving(&path, &mut || {
        crate::editor::content::persist_editor_content(&path, &a.frontmatter, &a.body, &a.expected_hash, a.keep_displaced)
    })?;
    if let Some(event) = crate::editor::content::displaced_event(&path, &r) {
        host.publish(&event);
    }
    Ok(r)
}

#[derive(serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WriteEditorFileBinaryArgs {
    pub relative_path: VaultPath,
    pub data_base64: String,
}

/// Bytes the editor made (a pasted screenshot), written under the editor's
/// folder. Not the plugin API's binary write, which is scoped to a site and
/// fences `.moss/`: a loose document has no site.
pub async fn write_editor_file_binary(host: &dyn CommandHost, a: WriteEditorFileBinaryArgs) -> Result<(), String> {
    use base64::Engine;
    let path = resolve(host, &a.relative_path)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&a.data_base64)
        .map_err(|e| format!("Failed to decode base64 data: {e}"))?;
    crate::plugins::project_files::write_file_with_dirs(&path, &bytes)
}
