//! What every page of one build reads identically, built once by the caller.
//!
//! Anything here used to be built by the page renderer itself, once per page,
//! so re-rendering N pages paid for N copies of the same answer. The media
//! index is the expensive one: it walks every image and video in the vault,
//! and on an image-heavy site rebuilding it per page was most of the cost of
//! re-rendering every page after a nav edit.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::build::emit::scripts::ScriptAssets;
use crate::build::media::dimensions::MediaDimensionLookup;
use crate::build::types::ParsedDocument;
use crate::build::scan::classify::folder_index_keys;
use crate::build::terms::TermIndex;
use crate::types::content::ProjectStructure;

/// `'d` is the build's documents, which the cached reading orders point into.
pub struct BuildShared<'d> {
    /// The build's one script snapshot, so every page's `<script src>` names
    /// a file that same snapshot emitted.
    pub scripts: ScriptAssets,
    /// Dimensions, colors and LQIP for the vault's media, read by the cards,
    /// covers and embeds a page body resolves. Registry-less: page bodies
    /// have never folded in the build's encoded variants.
    pub media_lookup: MediaDimensionLookup,
    /// Each folder's prev/next reading order.
    pub sequences: SequenceChains<'d>,
    /// The calendar files this build writes and the pages that link them. Empty
    /// until the build plans it from its documents.
    pub calendar: crate::build::feeds::calendar::CalendarPlan,
}

impl BuildShared<'_> {
    /// Takes the build's script snapshot and indexes `project`'s images and
    /// videos under the build's slug overrides.
    pub fn new(
        scripts: ScriptAssets,
        project: &ProjectStructure,
        dir_overrides: &HashMap<String, String>,
    ) -> Self {
        Self {
            scripts,
            media_lookup: MediaDimensionLookup::new(
                &project.image_files,
                &project.video_files,
                dir_overrides,
                None,
            ),
            sequences: SequenceChains::default(),
            calendar: Default::default(),
        }
    }
}

/// Each folder's ordered prev/next chain, computed by the first of its pages
/// to ask and read by the rest. Ordering a folder sorts all of its children,
/// so doing it for every page made a folder of n pages cost n² log n.
#[derive(Default)]
pub struct SequenceChains<'d> {
    by_parent: Mutex<HashMap<String, Arc<OnceLock<Arc<[&'d ParsedDocument]>>>>>,
}

impl<'d> SequenceChains<'d> {
    /// The chain of the folder whose index is `parent_index`, ordered by
    /// `order` if no page has asked for it yet.
    pub fn chain(
        &self,
        parent_index: &str,
        order: impl FnOnce() -> Vec<&'d ParsedDocument>,
    ) -> Arc<[&'d ParsedDocument]> {
        // Take the folder's cell under the lock and order outside it, so pages
        // of other folders never wait on this one's sort.
        let cell = Arc::clone(
            self.by_parent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(parent_index.to_string())
                .or_default(),
        );
        Arc::clone(cell.get_or_init(|| order().into()))
    }
}

/// Generated folder membership is resolved once for both tree and emission.
pub(super) struct FolderIndexEntry {
    pub folder: String,
    pub document_index: usize,
}

pub(super) struct FolderIndexPlan {
    pub entries: Vec<FolderIndexEntry>,
}

impl FolderIndexPlan {
    pub fn resolve(
        documents: &mut Vec<ParsedDocument>,
        project: &ProjectStructure,
        dir_overrides: &HashMap<String, String>,
        terms: &TermIndex,
        input_evidence: &crate::build::cloud_ledger::InputEvidence,
        make_document: impl Fn(&str) -> ParsedDocument,
    ) -> Self {
        // The boolean records a document descendant, rather than retaining
        // child indices that listing selection would have to recompute anyway.
        let mut candidates: BTreeMap<String, bool> = BTreeMap::new();
        for doc in documents.iter().filter(|d| !d.slot_only && d.url_path != "index.html") {
            let parts: Vec<&str> = doc.url_path.split('/').collect();
            for depth in 1..parts.len().saturating_sub(1) {
                candidates.insert(parts[..depth].join("/"), true);
            }
        }
        for (_, key) in folder_index_keys(&project.dirs, dir_overrides) {
            candidates.entry(key).or_insert(false);
        }
        for key in terms.synthetic_folder_keys() {
            candidates.entry(key).or_insert(false);
        }

        // A pending authored index or a directory whose only pages are
        // pending must not be replaced by a finished synthetic page.
        let pending_pages: Vec<String> = input_evidence.snapshot().into_iter()
            .filter(|(_, entry)| matches!(entry.role,
                crate::build::cloud_ledger::InputRole::PageMetadata | crate::build::cloud_ledger::InputRole::PageContent)
                && matches!(entry.state,
                    crate::build::cloud_ledger::InputState::Pending { .. } | crate::build::cloud_ledger::InputState::ReadError { .. }))
            .map(|(path, _)| path).collect();
        let suppressed: HashSet<String> = folder_index_keys(&project.dirs, dir_overrides)
            .filter(|(raw, _)| {
                let prefix = format!("{raw}/");
                let pending_only = pending_pages.iter().any(|path| path.starts_with(&prefix))
                    && !documents.iter().any(|doc| doc.source_path.as_ref().is_some_and(|path| path.starts_with(&prefix)));
                let authored_index_pending = pending_pages.iter().any(|path| {
                    let source = std::path::Path::new(path);
                    source.parent().is_some_and(|parent| parent == std::path::Path::new(raw))
                        && source.file_stem().and_then(|s| s.to_str()).is_some_and(|stem|
                            moss_core::home::is_home_file(stem, raw.rsplit('/').next().unwrap_or(raw)))
                });
                pending_only || authored_index_pending
            }).map(|(_, key)| key).collect();

        let authored: HashSet<String> = documents.iter()
            .filter(|d| d.source_path.is_some())
            .filter_map(|d| d.url_path.strip_suffix("/index.html").map(str::to_string))
            .collect();
        let translation_roots: HashSet<&str> = documents.iter()
            .filter(|d| d.translations.iter().any(|t| t.url_path == "index.html"))
            .filter_map(|d| d.url_path.strip_suffix("/index.html"))
            .filter(|root| !root.contains('/'))
            .collect();
        // Preserve the served set: indexless language trees with descendants
        // have an index; empty language directories and authored translation
        // trees do not acquire generated indexes.
        let folders: Vec<String> = candidates.into_iter().filter_map(|(folder, has_children)| {
            let top = folder.split('/').next().unwrap_or(&folder);
            let empty_language = !has_children
                && crate::i18n::path::resolve_language_from_folder(top).is_some();
            (!authored.contains(&folder) && !suppressed.contains(&folder)
                && !empty_language && !translation_roots.contains(top))
                .then_some(folder)
        }).collect();

        let entries = folders.into_iter().map(|folder| {
            let document_index = documents.len();
            documents.push(make_document(&folder));
            FolderIndexEntry { folder, document_index }
        }).collect();
        Self { entries }
    }
}

#[cfg(test)]
#[path = "folder_index_plan_tests.rs"]
mod folder_index_plan_tests;
