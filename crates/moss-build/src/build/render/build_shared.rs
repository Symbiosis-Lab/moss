//! What every page of one build reads identically, built once by the caller.
//!
//! Anything here used to be built by the page renderer itself, once per page,
//! so re-rendering N pages paid for N copies of the same answer. The media
//! index is the expensive one: it walks every image and video in the vault,
//! and on an image-heavy site rebuilding it per page was most of the cost of
//! re-rendering every page after a nav edit.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::build::assets::theme_modules::ThemeModules;
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

    /// The theme's `script.mjs` and sibling modules; empty without one.
    pub theme_modules: ThemeModules,
    /// The build's one clock: every past/upcoming decision (an event page's
    /// `data-when`, its listing cards, the ordering) reads this, so a build
    /// that crosses an event's end cannot disagree with itself.
    pub now: chrono::NaiveDateTime,
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
            theme_modules: ThemeModules::default(),
            now: crate::build::folder_embed::upcoming::now(),
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
    /// `(old address, target)` for each listing a passthrough folder no longer
    /// gets, for the redirect table.
    pub passthrough_stubs: Vec<(String, String)>,
}

impl FolderIndexPlan {
    pub fn resolve(
        documents: &mut Vec<ParsedDocument>,
        project: &ProjectStructure,
        verbatim_dirs: &HashSet<String>,
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
        let term_pages: HashSet<String> = terms.synthetic_folder_keys().into_iter().collect();
        for key in &term_pages {
            candidates.entry(key.clone()).or_insert(false);
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
                // A folder whose only pages are partials has nothing to show:
                // its sources are never built, so an index for it would be a
                // live, empty page at an address the author never made.
                let mut under = documents.iter().filter(|doc| doc.source_path.as_ref().is_some_and(|path| path.starts_with(&prefix)));
                let partials_only = under.clone().next().is_some() && under.all(|doc| doc.slot_only);
                pending_only || authored_index_pending || partials_only
            }).map(|(_, key)| key).collect();

        // A folder whose `index.html` is copied verbatim already has its page.
        let verbatim: HashSet<String> =
            verbatim_dirs
                .iter()
                .filter_map(|dir| {
                    let mapped = crate::build::scan::page_map::resolve_path_with_overrides(
                        &format!("{dir}/index.html"),
                        dir_overrides,
                    );
                    mapped.strip_suffix("/index.html").map(str::to_string)
                })
                .collect();

        // A passthrough folder is served as the author laid it out: pages
        // inside it render, but it and the folders within get no listing.
        let passthrough: Vec<String> = project.passthrough_roots.iter()
            .filter_map(|root| root.strip_suffix('/'))
            .filter_map(|dir| {
                crate::build::scan::page_map::resolve_path_with_overrides(&format!("{dir}/index.html"), dir_overrides)
                    .strip_suffix("/index.html").map(str::to_string)
            })
            .collect();
        for doc in documents.iter_mut() {
            let folder = doc.url_path.rsplit_once('/').map_or("", |(dir, _)| dir);
            doc.passthrough_root = passthrough.iter().find(|root| crate::build::types::folder_within(root, folder)).cloned();
        }

        let authored: HashSet<String> = documents.iter()
            .filter(|d| d.source_path.is_some())
            .filter_map(|d| d.url_path.strip_suffix("/index.html").map(str::to_string))
            .collect();
        // Follows authored keys, not `translations`, so pairing by path never
        // changes which pages are built. The breadcrumb's translation-root test
        // (`trail_tail` in components/nav.rs) reads linked translations instead,
        // because it only affects presentation.
        let home_key = documents.iter()
            .find(|d| d.url_path == "index.html")
            .and_then(|d| d.translation_key.as_deref());
        let translation_roots: HashSet<&str> = documents.iter()
            .filter(|d| home_key.is_some() && d.translation_key.as_deref() == home_key)
            .filter_map(|d| d.url_path.strip_suffix("/index.html"))
            .filter(|root| !root.contains('/'))
            .collect();
        // Preserve the served set: indexless language trees with descendants
        // have an index; empty language directories and authored translation
        // trees do not acquire generated indexes. A term page is a page of
        // its edition, never a folder index of the tree it sits in.
        let mut folders: Vec<String> = Vec::new();
        let mut withheld: Vec<String> = Vec::new();
        for (folder, has_children) in candidates {
            let top = folder.split('/').next().unwrap_or(&folder);
            let empty_language = !has_children && crate::i18n::path::resolve_language_from_folder(top).is_some();
            let tree_suppressed = empty_language || translation_roots.contains(top);
            let in_passthrough = passthrough.iter().any(|root| crate::build::types::folder_within(root, &folder));
            if authored.contains(&folder) || suppressed.contains(&folder) || verbatim.contains(&folder)
                || !(term_pages.contains(&folder) || !tree_suppressed)
            {
                continue;
            }
            if in_passthrough { withheld.push(folder) } else { folders.push(folder) }
        }
        // Each withheld listing forwards to the nearest folder page still served.
        let passthrough_stubs = withheld.iter().map(|folder| {
            let target = std::iter::successors(folder.rsplit_once('/').map(|(parent, _)| parent), |p| p.rsplit_once('/').map(|(parent, _)| parent))
                .find(|p| authored.contains(*p) || verbatim.contains(*p) || folders.iter().any(|f| f == p))
                .map_or_else(|| "/".to_string(), |p| format!("/{p}/")); // allow:served-path-url-construct (the redirect target is the parent page's address)
            (format!("{folder}/"), target) // allow:served-path-url-construct (the address the listing had, the redirect source)
        }).collect();

        let entries = folders.into_iter().map(|folder| {
            let document_index = documents.len();
            documents.push(make_document(&folder));
            FolderIndexEntry { folder, document_index }
        }).collect();
        Self { entries, passthrough_stubs }
    }
}

/// Report each page whose address is also the output path of a source `.html`
/// file. The asset copy serves the page and drops the file, so one of them is
/// lost: a build problem, not a quiet override.
///
/// A file named exactly in `[build].passthrough` replaces its page by the
/// author's own declaration and is not reported.
pub(super) fn report_verbatim_collisions(
    documents: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &HashMap<String, String>,
) {
    let (html_files, passthrough_roots) = (&project.html_files, &project.passthrough_roots);
    let sources: HashMap<String, &str> = html_files
        .iter()
        .filter(|f| !passthrough_roots.contains(&f.path))
        .map(|f| (crate::build::scan::page_map::resolve_path_with_overrides(&f.path, dir_overrides), f.path.as_str()))
        .collect();
    for doc in documents.iter().filter(|d| !d.slot_only) {
        let Some(&file) = sources.get(&doc.url_path) else { continue };
        let page = match &doc.source_path {
            Some(source) => format!("`{source}`"),
            None => "a generated folder page".to_string(),
        };
        crate::build::cli_output::log_warn_problem!(
            "{page} and `{file}` are both published at {}: the page is served and the file is lost",
            crate::build::served_path::served_address(&doc.url_path)
        );
    }
}

/// Withhold the body of every page whose address a passthrough `.html` file
/// owns, and warn once per such file whose body was not empty. The file is what
/// readers get, so its markdown is only metadata: its title and description
/// still serve the site, but its text must not reach `llms.txt`, a feed or any
/// other surface built from the documents.
pub(super) fn withhold_shadowed_bodies(
    documents: &mut [ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &HashMap<String, String>,
) {
    let owned: HashSet<String> = project
        .html_files
        .iter()
        .filter(|f| crate::build::scan::classify::is_in_passthrough(&f.path, &project.passthrough_roots))
        .map(|f| crate::build::scan::page_map::resolve_path_with_overrides(&f.path, dir_overrides))
        .collect();
    for doc in documents.iter_mut().filter(|d| !d.slot_only && owned.contains(&d.url_path)) {
        let Some(source) = &doc.source_path else { continue };
        if !doc.content.trim().is_empty() {
            crate::build::cli_output::log_warn_problem!(
                "`{source}` has text, but it is never published because the HTML document at {} owns the page",
                crate::build::served_path::served_address(&doc.url_path)
            );
        }
        doc.content.clear();
        doc.html_content.clear();
    }
}

/// The old addresses of language-tree homes whose `index.md` used to be
/// published beside the tree's verbatim `index.html`, as `(old address, target)`
/// pairs for the redirect table. The markdown is now that home's metadata, so
/// `/fr/index/` forwards to `/fr/`.
pub(super) fn shadowed_edition_home_stubs(
    verbatim_dirs: &HashSet<String>,
    home_file_winners: &HashSet<String>,
) -> Vec<(String, String)> {
    let mut stubs: Vec<(String, String)> = home_file_winners
        .iter()
        .filter_map(|file| {
            let (dir, name) = file.split_once('/')?;
            let stem = name.rsplit_once('.')?.0.to_lowercase();
            (verbatim_dirs.contains(dir) && moss_core::home::is_known_language_code(dir) && !stem.contains('/'))
                .then(|| (format!("{dir}/{stem}/"), format!("/{dir}/"))) // allow:served-path-url-construct (the address before the home was metadata, the redirect source)
        })
        .collect();
    stubs.sort();
    stubs
}

#[cfg(test)]
#[path = "folder_index_plan_tests.rs"]
mod folder_index_plan_tests;
