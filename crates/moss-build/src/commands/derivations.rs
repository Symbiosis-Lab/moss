//! Showing a folder's files as pages: the file tree's "show as pages" row.
//! The same functions `moss import <folder>` and `moss import --status` call.
//! Which plugins may run is the loader's admission verdict, so a plugin the
//! author has not allowed for this site contributes nothing here.

use std::path::{Component, Path, PathBuf};

use super::{site_root, CommandHost, NoArgs};
use crate::moss_paths::MossPaths;
use crate::vault::derive_folders::{set_folder_decision, Decision};
use crate::vault::derive_state::{site_registry, FolderDerivationState};
use crate::vault::derive_stubs::{accepted_files, write_stubs};

/// What the author chose for a folder, as the UI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DerivationDecision {
    Undecided,
    Show,
    Keep,
}

/// A file the plugin looked at and declined.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct SkippedDerivedFile {
    pub file: String,
    pub reason: String,
    pub message: String,
}

/// A file the plugin could not read the last time it tried.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FailedDerivedFile {
    pub file: String,
    pub message: String,
}

/// One folder and one derivation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct FolderDerivation {
    /// Site-relative with a trailing slash; `/` is the site root.
    pub folder: String,
    pub derivation_id: String,
    pub noun_one: String,
    pub noun_other: String,
    pub count: u32,
    pub decision: DerivationDecision,
    /// Pages the folder is still missing (0 unless shown).
    pub stubs_missing: u32,
    pub skipped: Vec<SkippedDerivedFile>,
    pub failed: Vec<FailedDerivedFile>,
}

/// The author's answer for a folder: show its files as pages with this
/// derivation, or keep them as files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FolderDerivationChoice {
    Show(String),
    Keep,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn wire(row: FolderDerivationState) -> FolderDerivation {
    FolderDerivation {
        folder: row.folder,
        derivation_id: row.derivation_id,
        noun_one: row.noun_one,
        noun_other: row.noun_other,
        count: count(row.count),
        decision: match row.decision {
            Decision::Undecided => DerivationDecision::Undecided,
            Decision::Show(_) => DerivationDecision::Show,
            Decision::Keep => DerivationDecision::Keep,
        },
        stubs_missing: count(row.stubs_missing),
        skipped: row
            .skipped
            .into_iter()
            .map(|s| SkippedDerivedFile { file: s.file, reason: s.reason, message: s.message })
            .collect(),
        failed: row.failed.into_iter().map(|f| FailedDerivedFile { file: f.file, message: f.message }).collect(),
    }
}

/// Every folder holding files an admitted plugin can turn into pages, with
/// what the author decided for each. Each file is looked up by content hash,
/// memoized by (path, size, mtime), so only the first call after a file
/// changes reads its bytes.
pub async fn derivation_state(host: &dyn CommandHost, _: NoArgs) -> Result<Vec<FolderDerivation>, String> {
    derivation_state_in(site_root(host)?)
}

fn derivation_state_in(site_root: &Path) -> Result<Vec<FolderDerivation>, String> {
    let site = MossPaths::new(site_root);
    let registry = site_registry(&site)?;
    Ok(crate::vault::derive_state::derivation_state(&site, &registry).into_iter().map(wire).collect())
}

#[derive(serde::Deserialize, specta::Type)]
pub struct SetFolderDerivationArgs {
    /// Site-relative, as [`FolderDerivation::folder`] names it.
    pub folder: String,
    pub choice: FolderDerivationChoice,
}

/// Records the choice for a folder; for `Show`, also writes the pages it is
/// missing, which can run the plugin's deriver. Returns the pages written,
/// site-relative. Undo is the other choice; pages already written stay.
pub async fn set_folder_derivation(host: &dyn CommandHost, a: SetFolderDerivationArgs) -> Result<Vec<String>, String> {
    set_folder_derivation_in(site_root(host)?, &a.folder, a.choice)
}

/// A site-relative folder (`reports/`, `/` for the root) as a directory inside the site.
fn folder_in_site(site_root: &Path, folder: &str) -> Result<PathBuf, String> {
    let relative = Path::new(folder.trim_matches('/'));
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!("{folder} is not a folder of this site"));
    }
    let path = site_root.join(relative);
    if !path.is_dir() {
        return Err(format!("{folder} is not a folder of this site"));
    }
    Ok(path)
}

fn set_folder_derivation_in(site_root: &Path, folder: &str, choice: FolderDerivationChoice) -> Result<Vec<String>, String> {
    let site = MossPaths::new(site_root);
    let folder_path = folder_in_site(site_root, folder)?;
    match choice {
        FolderDerivationChoice::Keep => {
            set_folder_decision(&site, &folder_path, Decision::Keep)?;
            Ok(Vec::new())
        }
        FolderDerivationChoice::Show(id) => {
            let registry = site_registry(&site)?;
            let entry = registry
                .entries()
                .find(|e| e.derivation_id == id && !accepted_files(&folder_path, e).is_empty())
                .ok_or_else(|| format!("no installed plugin derivation `{id}` accepts files in {folder}"))?;
            set_folder_decision(&site, &folder_path, Decision::Show(vec![entry.derivation_id.clone()]))?;
            Ok(write_stubs(&folder_path, entry)
                .into_iter()
                .filter_map(|p| p.strip_prefix(site_root).ok().map(|r| r.to_string_lossy().replace('\\', "/")))
                .collect())
        }
    }
}

// Unix-only: the tests drive a `#!/bin/sh` deriver script.
#[cfg(all(test, unix))]
#[path = "derivations_tests.rs"]
mod derivations_tests;
