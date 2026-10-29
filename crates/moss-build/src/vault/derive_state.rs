//! What the file tree and `moss import --status` say about folders holding
//! files a plugin can turn into pages.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use super::derive_folders::{decisions, folder_key, nearest_decision, Decision};
use super::derive_stubs::stubs_missing;
use crate::moss_paths::MossPaths;
use crate::system::derive::registry::{DerivationEntry, DerivationRegistry};
use crate::system::derive::{lookup_cached, lookup_failure, DeriveOutcome};

/// A file the plugin looked at and declined to turn into pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    pub file: String,
    pub reason: String,
    pub message: String,
}

/// A file the plugin could not read the last time it tried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedFile {
    pub file: String,
    pub message: String,
}

/// One folder and one derivation: how many files it holds, what was decided,
/// and what is wrong with some of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderDerivationState {
    /// Site-relative with a trailing slash; `/` is the site root.
    pub folder: String,
    pub derivation_id: String,
    pub noun_one: String,
    pub noun_other: String,
    pub count: usize,
    /// `Show` carries only this derivation's id. On the wire it is the bare
    /// tag: `undecided`, `show` or `keep`.
    #[serde(serialize_with = "decision_tag")]
    pub decision: Decision,
    /// Pages the folder is still missing (always 0 unless shown).
    pub stubs_missing: usize,
    pub skipped: Vec<SkippedFile>,
    /// Files whose last read failed (the deriver's message), until a read succeeds.
    pub failed: Vec<FailedFile>,
}

/// The plugins installed in the site, as a registry. When none of them
/// derives anything and some plugin was refused at load (not yet allowed with
/// `--allow-plugins`, withdrawn, too new), that is the error, in the loader's
/// own words: the build would refuse the same plugin, and a folder shown as
/// pages would then never be read.
pub fn site_registry(site: &MossPaths) -> Result<DerivationRegistry, String> {
    let root = site.project_root().to_str().ok_or("the site path is not valid UTF-8")?;
    let crate::plugins::discovery::InstalledPlugins { loaded, refused } =
        crate::plugins::discovery::scan_installed_plugins(root)?;
    let registry = DerivationRegistry::from_plugins(&loaded);
    if registry.is_empty() && !refused.is_empty() {
        let lines: Vec<String> =
            refused.iter().map(|(path, reason)| format!("plugin at {} will not run: {reason}", path.display())).collect();
        return Err(lines.join("\n"));
    }
    Ok(registry)
}

fn decision_tag<S: serde::Serializer>(decision: &Decision, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_str(match decision {
        Decision::Undecided => "undecided",
        Decision::Show(_) => "show",
        Decision::Keep => "keep",
    })
}

/// What the author's choice for a folder means for this derivation.
fn decision_for(decision: Option<&Decision>, entry: &DerivationEntry) -> Decision {
    match decision {
        Some(Decision::Keep) => Decision::Keep,
        Some(Decision::Show(ids)) if ids.contains(&entry.derivation_id) => {
            Decision::Show(vec![entry.derivation_id.clone()])
        }
        _ => Decision::Undecided,
    }
}

/// The files the site's derivations accept, by the folder holding them and
/// then by derivation. Walks what the build walks, so a folder the build
/// ignores is not offered.
fn accepted_in_site(
    site: &MossPaths,
    registry: &DerivationRegistry,
) -> BTreeMap<PathBuf, HashMap<(String, String), Vec<PathBuf>>> {
    let mut found: BTreeMap<PathBuf, HashMap<(String, String), Vec<PathBuf>>> = BTreeMap::new();
    let walk = walkdir::WalkDir::new(site.project_root())
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| crate::build::scan::classify::left_out_of_site(e).is_none())
        .filter_map(Result::ok);
    for file in walk.filter(|e| e.file_type().is_file()) {
        let owner = file.path().extension().and_then(|e| e.to_str()).and_then(|ext| registry.for_extension(ext));
        let (Some(owner), Some(folder)) = (owner, file.path().parent()) else { continue };
        found
            .entry(folder.to_path_buf())
            .or_default()
            .entry((owner.plugin_id.clone(), owner.derivation_id.clone()))
            .or_default()
            .push(file.into_path());
    }
    found
}

/// Every folder in the site that directly holds files some installed
/// derivation accepts, one row per derivation, in path order.
pub fn derivation_state(site: &MossPaths, registry: &DerivationRegistry) -> Vec<FolderDerivationState> {
    if registry.is_empty() {
        return Vec::new();
    }
    let decided = decisions(site);
    let mut rows = Vec::new();
    for (folder, by_derivation) in accepted_in_site(site, registry) {
        let Some(key) = folder_key(site, &folder) else { continue };
        for entry in registry.entries() {
            let Some(files) = by_derivation.get(&(entry.plugin_id.clone(), entry.derivation_id.clone())) else {
                continue;
            };
            let decision = decision_for(nearest_decision(&decided, &key), entry);
            let (mut skipped, mut failed) = (Vec::new(), Vec::new());
            for f in files {
                let Some(file) = f.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
                match lookup_cached(site, entry, f) {
                    Some(DeriveOutcome::Skip { reason, message }) => skipped.push(SkippedFile { file, reason, message }),
                    Some(DeriveOutcome::Derived { .. }) => {}
                    None => failed.extend(lookup_failure(site, entry, f).map(|message| FailedFile { file, message })),
                }
            }
            rows.push(FolderDerivationState {
                folder: key.clone(),
                derivation_id: entry.derivation_id.clone(),
                noun_one: entry.contribution.noun.one.clone(),
                noun_other: entry.contribution.noun.other.clone(),
                count: files.len(),
                stubs_missing: if matches!(decision, Decision::Show(_)) { stubs_missing(&folder, entry) } else { 0 },
                decision,
                skipped,
                failed,
            });
        }
    }
    rows
}

#[cfg(test)]
#[path = "derive_state_tests.rs"]
mod derive_state_tests;
