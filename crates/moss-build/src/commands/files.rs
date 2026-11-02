//! Commands that create, save, rename and delete files in the folder.

use serde_json::Value;

use super::rename_undo::RenameRefsSummary;
use super::{CommandHost, VaultPath};
use crate::editor::rename_plan::UndoResult;

fn resolve_all(host: &dyn CommandHost, paths: &[VaultPath]) -> Result<Vec<String>, String> {
    paths.iter().map(|p| host.resolve(p).map(|pb| pb.to_string_lossy().into_owned())).collect()
}

#[derive(serde::Deserialize, specta::Type)]
pub struct CreateFilesArgs {
    pub files: Vec<crate::vault::fs::NewFile>,
}

/// All files or none; returns their absolute paths in order.
pub async fn create_files(host: &dyn CommandHost, a: CreateFilesArgs) -> Result<Vec<String>, String> {
    crate::vault::fs::create_files_inner(host.root(), &a.files)
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

pub async fn scan_references_for_delete(
    host: &dyn CommandHost,
    a: PathArgs,
) -> Result<Vec<crate::editor::ref_scan::FileReferenceHit>, String> {
    crate::editor::ref_scan::scan_project_references_to(&host.resolve(&a.path)?, host.root())
}

/// Strips every reference to `paths` from the folder's pages, then trashes them.
pub async fn clean_references_and_delete(host: &dyn CommandHost, a: PathsArgs) -> Result<(), String> {
    let paths = resolve_all(host, &a.paths)?;
    crate::editor::ref_scan::clean_references_to_paths(host.root(), &paths)?;
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
    let old = host.resolve(&a.old_path)?;
    let new = host.resolve(&a.new_path)?;
    let root = host.root();
    let applied =
        crate::editor::ref_scan::rename_entry_with_refs_core(root.to_path_buf(), &old.to_string_lossy(), &new.to_string_lossy())?;
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
    crate::editor::rename_plan::undo_applied(&root, &applied)
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
    let path = host.resolve(&a.file_path)?.to_string_lossy().into_owned();
    let r = crate::editor::content::persist_editor_content(&path, &a.frontmatter, &a.body, &a.expected_hash, a.keep_displaced)?;
    if let Some(event) = crate::editor::content::displaced_event(&path, &r) {
        host.publish(&event);
    }
    Ok(r)
}
