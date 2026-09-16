//! Editor-facing reference resolution — the single editor path that resolves
//! EVERY reference kind (image/file embed, folder, link) through the shared
//! `moss_core::resolve::reference::classify_reference` classifier.
//!
//! This is the ONE editor resolution command — it replaced the former separate
//! `editor_resolve_assets` + `resolve_links` commands (now deleted). One call
//! builds the three indexes ONCE — `FsAssetIndex` (assets), `EditorFolderIndex` (folders),
//! `ArticleMapIndex` (deployed URLs) — and routes each target through the same
//! classifier the build pipeline uses, so the editor's lint/preview decisions
//! can never drift from the published site.
//!
//! All filesystem I/O lives here; `moss_core` stays pure (indexes are injected
//! via `ReferenceContext`).

use crate::build::scan::article_map::ArticleMap;
use crate::editor::resolve::url_index::{ArticleMapIndex, FolderFacts};
use moss_core::resolve::asset_class::AssetProvenance;
use moss_core::resolve::reference::{
    classify_reference, ReferenceContext, ReferenceKind, ResolvedReference,
};
use std::path::Path;

/// One reference to resolve: the raw target text plus whether it appeared in an
/// embed context (`![[...]]` / `![](...)`) or a plain link (`[[...]]` / `[](...)`).
/// `is_embed` is what drives the classifier's Link-vs-embed fork.
// NOTE: `specta` is a NON-optional dependency in src-tauri (no `specta`
// cargo feature exists in this crate), so the gated `cfg_attr(feature =
// "specta", ...)` form would never actually derive `specta::Type` and the
// `#[specta::specta]` command would fail to compile. Match the existing
// editor type `ResolvedAsset`: derive bare.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, specta::Type)]
pub struct RefTarget {
    pub text: String,
    pub is_embed: bool,
}

/// What the LAST BUILD listed under a folder, read from the `ArticleMap`.
/// Attached to `FolderListing` embeds so the editor's card can state what will
/// be listed and how, without reimplementing the build's selection.
///
/// The fidelity limits of the underlying derivation are documented on
/// [`crate::editor::resolve::url_index::FolderFacts`]; the editor renders these
/// counts as a bare number and makes no prose claim about build output.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, specta::Type)]
pub struct FolderEmbedInfo {
    /// Title of the folder's index page. None when there is no index source or
    /// it cannot be read.
    pub title: Option<String>,
    /// Pages listed at `depth:direct` (articles + sub-folder index pages).
    pub direct_child_count: u32,
    /// Pages listed at `depth:all` (articles only, at any depth).
    pub descendant_count: u32,
}

/// Always-filled envelope for one resolved reference: never a bare Option, so
/// the editor can show lint diagnostics for unresolved/ambiguous references too.
/// This is the SINGLE envelope every editor consumer reads (image render,
/// link/asset lint, hover, cmd-click nav).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, specta::Type)]
pub struct EditorReferenceResolution {
    /// The original reference text that was classified.
    pub target: String,
    /// Whether the reference was resolved in embed context.
    pub is_embed: bool,
    /// The classified kind (Image, Iframe, Folder kinds, Link, External, …).
    pub kind: ReferenceKind,
    /// For file/asset embed kinds: the enriched asset (path, URL, mime, LQIP).
    /// None for non-asset kinds and for a TOCTOU miss.
    pub resolved: Option<crate::editor::resolve::asset_resolver::ResolvedAsset>,
    /// For Link kinds: the resolved deployed URL (carried from the classifier).
    pub url: Option<String>,
    /// For Link kinds with a `#fragment`: the anchor.
    pub anchor: Option<String>,
    /// The absolute source FILE this reference opens when followed, for the two
    /// kinds that have one: a resolved internal Link (via `source_path_for_url`)
    /// and a FolderListing embed (via `FolderFacts::index_source_for`, which is
    /// the folder's index source, NOT the folder directory).
    ///
    /// Always a file, never a directory — every consumer opens it. A folder that
    /// publishes no index source of its own leaves this `None`, so the editor
    /// shows no follow affordance rather than one that lands nowhere.
    pub resolved_path: Option<String>,
    /// Human-readable diagnostic note (separator-fallback / case-mismatch / …).
    pub message: Option<String>,
    /// For Ambiguous: all candidate paths.
    pub candidates: Vec<String>,
    /// For FolderListing embeds: what the last build listed. None for every
    /// other kind, and for a folder the article map does not know.
    pub folder: Option<FolderEmbedInfo>,
}

/// Asset/file-embed kinds whose `target_path` should be enriched into a
/// `ResolvedAsset` (reusing the asset resolver's enrich helper).
fn is_asset_embed_kind(kind: &ReferenceKind) -> bool {
    matches!(
        kind,
        ReferenceKind::Image
            | ReferenceKind::Iframe
            | ReferenceKind::Pdf
            | ReferenceKind::Video
            | ReferenceKind::Audio
            | ReferenceKind::Model
    )
}

/// Compute a project-root-relative, forward-slash `from_source` for the
/// classifier. The editor passes a project-root-relative `from_file` (e.g.
/// `News/post.md`, from `getCurrentEntry().path`); an already-relative path IS
/// the root-relative source. An absolute path is stripped of the project-root
/// prefix; if that fails it falls back to the raw string (matching the
/// tolerance of `resolve_links`).
fn compute_from_source(from_file: &str, project_root: &Path) -> String {
    let p = Path::new(from_file);
    if p.is_relative() {
        return from_file.replace('\\', "/");
    }
    p.strip_prefix(project_root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| from_file.replace('\\', "/"))
}

/// Resolve a batch of references, building the three indexes ONCE and routing
/// every target through the shared `classify_reference`.
///
/// - `FsAssetIndex` / `EditorFolderIndex` operate on the canonicalized project root
///   (so `strip_prefix` is stable on macOS where `/tmp` symlinks).
/// - `ArticleMapIndex` is built from the on-disk `ArticleMap` (mirrors
///   `resolve_links`); an empty/missing map means Link targets simply don't
///   resolve to a URL (the classifier returns `NotFound` for a bare note name).
pub fn resolve_references_batch(
    targets: &[RefTarget],
    from_file: &str,
    project_root: &Path,
) -> Vec<EditorReferenceResolution> {
    // Canonicalize once so the FS-backed indexes strip_prefix correctly.
    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    // Build the three indexes ONCE (outside the per-target loop). The map is
    // loaded FIRST: the folder index reconstructs the build's folder-index URL
    // set from it (slug overrides live there, not on disk).
    let moss_dir = canonical_root.join(".moss");
    let map = ArticleMap::load(&moss_dir).unwrap_or_default();
    let fs_assets = crate::editor::resolve::asset_resolver::FsAssetIndex::new(&canonical_root);
    let fs_folders =
        crate::editor::resolve::folder_index::EditorFolderIndex::new(&canonical_root, &map);
    let article_idx = ArticleMapIndex::from_map(&map);
    // A fourth index, same discipline: ONE O(map) pass for every folder in the
    // document instead of a rescan (plus a file read) per folder embed. Every
    // `FileChanged` sets `revalidateAll`, so this runs on every rebuild.
    let folder_facts = FolderFacts::from_map(&map);

    let from_source = compute_from_source(from_file, &canonical_root);

    targets
        .iter()
        .map(|t| {
            let target_norm = t.text.replace('\\', "/");
            let resolved: ResolvedReference = classify_reference(
                &target_norm,
                &from_source,
                t.is_embed,
                &ReferenceContext {
                    assets: &fs_assets,
                    folders: &fs_folders,
                    urls: &article_idx,
                },
            );

            enrich_resolution(t, resolved, &canonical_root, &map, &folder_facts, &fs_folders)
        })
        .collect()
}

/// The file that acts as a folder's home page, read from the folder itself.
///
/// The choice is `moss_core::home::detect_home_file_in_folder` — the build's
/// own rule, priority-ordered (`index.md` outranks `README.md`, which outranks
/// a self-named folder note). Picking a different one would send the editor to
/// a file the site does not publish.
///
/// Reached only when the ArticleMap has no live index source for the folder —
/// before the first build, for a folder added since, or when the recorded one
/// has been renamed away. One directory read per such embed, and none at all
/// for a folder the last build already recorded.
fn home_file_in(dir: &Path) -> Option<std::path::PathBuf> {
    let folder_name = dir.file_name()?.to_string_lossy().to_string();
    // An unreadable ENTRY is skipped, not fatal: one bad dirent must not hide
    // an index.md sitting beside it.
    let names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let pick = moss_core::home::detect_home_file_in_folder(&refs, &folder_name)?;
    // …but only its NAMED tiers. `detect_home_file_in_folder`'s last resort is
    // "first document alphabetically", which is an election, not a name — and
    // whether the build elects that file or synthesizes a listing instead is
    // decided by `page_map`, not here. Offering to open a file the card may not
    // even be showing is worse than offering nothing, so an unnamed pick is
    // dropped: the ArticleMap answers that case once a build has run.
    let stem = Path::new(pick).file_stem()?.to_string_lossy().to_string();
    if !moss_core::home::is_home_file(&stem, &folder_name) {
        return None;
    }
    Some(dir.join(pick))
}

/// Map a `ResolvedReference` (from the classifier) into the editor envelope,
/// filling per-kind detail:
///   - asset/file embeds → `resolved` (reusing asset_resolver's enrich helper),
///   - Link → `url` (from classify) + `anchor` + absolute `resolved_path`
///     (via `source_path_for_url`).
fn enrich_resolution(
    t: &RefTarget,
    resolved: ResolvedReference,
    canonical_root: &Path,
    map: &ArticleMap,
    folder_facts: &FolderFacts,
    folders: &dyn moss_core::resolve::folder_class::FolderIndex,
) -> EditorReferenceResolution {
    // Extract the Link anchor (only the Link variant carries one).
    let anchor = match &resolved.kind {
        ReferenceKind::Link { anchor } => anchor.clone(),
        _ => None,
    };

    let mut out = EditorReferenceResolution {
        target: t.text.clone(),
        is_embed: t.is_embed,
        kind: resolved.kind.clone(),
        resolved: None,
        url: resolved.url.clone(),
        anchor,
        resolved_path: None,
        message: resolved.message.clone(),
        candidates: resolved.candidates.clone(),
        folder: None,
    };

    // Asset/file embed: enrich target_path into a ResolvedAsset, reusing the
    // asset resolver's single enrich implementation (source-only, no registry).
    if is_asset_embed_kind(&resolved.kind) {
        if let Some(ref root_rel) = resolved.target_path {
            let provenance = resolved.provenance.unwrap_or(AssetProvenance::Literal);
            if let Some(asset) = crate::editor::resolve::asset_resolver::enrich_root_rel(
                root_rel,
                provenance,
                canonical_root,
            ) {
                out.resolved = Some(asset);
            }
        }
    }

    // Folder listing: attach what the last build listed there, so the editor
    // card can say what will be listed and how (never a rendered child grid —
    // that would reimplement the build inside the editor).
    //
    // …and where following it lands: the folder's INDEX SOURCE, resolved through
    // the same `key_for` bridge the counts use. Joining the author-typed target
    // to the root instead would break on exactly the case the card handles best
    // — `獎項/獎項.md` publishing at `awards/`, where `<root>/awards` names no
    // directory at all.
    if matches!(resolved.kind, ReferenceKind::FolderListing) {
        if let Some(ref folder_rel) = resolved.target_path {
            out.folder = folder_facts.lookup(folder_rel);
            // …and only if it is still there. The ArticleMap is the LAST
            // build's record, so an index renamed or deleted since would
            // otherwise render a card that offers to open a file that is gone.
            // `is_file` is the same check `asset_resolver::enrich` makes, and it
            // is true for an iCloud-dataless file (which stats fine), so this
            // does not confuse "evicted" with "absent".
            //
            // The map can also simply not know the folder — before the first
            // build, or for a folder added since — while `EditorFolderIndex`
            // classified it as a listing straight from disk. Falling back to
            // the folder's own home file keeps the affordance from depending
            // on whether a build has run.
            //
            // Each candidate is checked BEFORE falling through to the next: a
            // recorded index that has since been renamed must hand over to the
            // folder's current home file, not veto it.
            out.resolved_path = folder_facts
                .index_source_for(folder_rel)
                .map(|src| canonical_root.join(src))
                .filter(|abs| abs.is_file())
                .or_else(|| home_file_in(&canonical_root.join(folder_rel)))
                .filter(|abs| abs.is_file())
                .map(|abs| abs.to_string_lossy().to_string());
        }
    }

    // A folder that ships its own `index.html` renders as an iframe rather than
    // a listing, and it was the last card kind with nothing to open — the same
    // asymmetry the transclusion arm removes, one kind over.
    if matches!(resolved.kind, ReferenceKind::FolderIndexIframe) {
        if let Some(ref folder_rel) = resolved.target_path {
            // The classifier already found the file — `index.html` OR
            // `index.htm` — so ask it rather than guessing one of the two.
            out.resolved_path = folders
                .dir_has_static_index(folder_rel)
                .map(|name| canonical_root.join(folder_rel).join(name))
                .filter(|abs| abs.is_file())
                .map(|abs| abs.to_string_lossy().to_string());
        }
    }

    // A markdown/notebook/table embed — `![[Some Note]]`, the commonest embed
    // there is — resolves to a real source file the classifier already found in
    // `target_path`, and nothing else fills a path for it. Without this arm the
    // pdf card beside it can be followed and it cannot, for no reason the author
    // can see. Same `is_file` guard as the folder arm: the classifier resolved
    // against an index, so confirm the file is still on disk.
    if matches!(
        resolved.kind,
        ReferenceKind::Transclusion | ReferenceKind::Notebook | ReferenceKind::Table
    ) {
        if let Some(ref rel) = resolved.target_path {
            out.resolved_path = Some(canonical_root.join(rel))
                .filter(|abs| abs.is_file())
                .map(|abs| abs.to_string_lossy().to_string());
        }
    }

    // Link: fill the absolute source path for cmd-click navigation. The
    // classifier already populated `url`; `source_path_for_url` returns a
    // project-root-relative source which we join to an absolute path.
    // A Moved link carries the canonical URL of the page that replaced the
    // gone one, so the same arm sends cmd-click to the claiming page's source.
    if matches!(resolved.kind, ReferenceKind::Link { .. } | ReferenceKind::Moved { .. }) {
        if let Some(url) = &resolved.url {
            if let Some(src) = crate::editor::resolve::links::source_path_for_url(map, url) {
                if !src.is_empty() {
                    out.resolved_path =
                        Some(canonical_root.join(&src).to_string_lossy().to_string());
                }
            }
        }
    }

    out
}

#[cfg(test)]
#[path = "reference_resolver_tests.rs"]
mod tests;
