//! Resolver-driven rename/move planning and application.
//!
//! `rename_entry_with_refs_core` used to rewrite references by pattern-matching
//! their raw text against the renamed entry's old/new path — a hand-picked
//! subset of the rules the real resolver (`classify_reference` /
//! `resolve_asset_ref`) already knows, so any reference the resolver accepted
//! through a route the pattern-matcher didn't know (a folder's self-named
//! note reached by its bare filename, a suffix-fallback match, …) went stale
//! silently after a rename.
//!
//! This module makes the invariant explicit instead: a rename/move never
//! changes what a reference resolves to, modulo mapping old paths to new
//! ones. For every reference: resolve it against the PRE-move tree; if it
//! still resolves to the mapped target from its POST-move location, leave it
//! byte-identical; otherwise rewrite it, preserving its authored form where
//! possible, and VERIFY the rewritten text actually resolves before using it.
//!
//! Both the "pre" and "post" resolution passes run against an in-memory
//! [`ContentGraph`] built from a plain path list — no filesystem access during
//! planning, so `plan_moves` can run against a hypothetical rename and answer
//! "what would change" without touching disk (the desktop's confirmation
//! modal, and CLI dry-runs). `apply_planned_moves` is the only place that
//! performs I/O.
//!
//! `rename_entry_with_refs_core` — the existing public entry point used by
//! both the app's rename command and `moss rename` — is now a one-element
//! wrapper: `plan_moves` + `apply_planned_moves` for a batch of exactly one.

mod path_list_folder_index;

use std::collections::HashSet;
use std::path::Path;

use moss_core::ast::resolve_urls::GraphAssetIndex;
use moss_core::content_graph::{ContentGraph, ContentGraphBuilder};
use moss_core::resolve::md_extract::{extract_md_references, extract_structural_asset_refs, RefSyntax};
use moss_core::resolve::fuzzy_path::{
    percent_decoded_fallback, percent_encode_path_segments, resolve_reference_with_percent_fallback, ResolvedRef,
};
use moss_core::resolve::reference::{classify_reference, ReferenceContext, ReferenceKind};

use crate::build::folder_index::NoUrlIndex;
use crate::editor::ref_rewrite::{apply_edits, match_and_retarget, render_bare_value, Edit};
use path_list_folder_index::{collect_dirs, PathListFolderIndex};

// ── Public, serializable plan/apply/undo types ──────────────────────────────
// Derives mirror `FileReferenceHit` (ref_scan.rs): the desktop passes these
// across the Tauri boundary to render the confirmation modal and back.

/// One entry being renamed or moved, project-relative to both sides.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PlannedMove {
    pub old_path: String,
    pub new_path: String,
    pub is_dir: bool,
}

/// One reference edit a rename/move requires.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PlannedEdit {
    /// Project-relative path of the referencing file, AT ITS POST-MOVE
    /// location (identical to the pre-move path unless the referencing file
    /// is itself one of the moved entries, or lives inside one).
    pub file: String,
    /// 1-based line number in the file's pre-move content.
    pub line: u32,
    /// Byte range in the file's pre-move content that this edit replaces.
    pub byte_from: usize,
    pub byte_to: usize,
    /// The exact bytes currently at `byte_from..byte_to` — the apply step's
    /// staleness check.
    pub old_text: String,
    pub new_text: String,
}

/// The result of [`plan_moves`]: nothing on disk has changed yet.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RenamePlan {
    pub moves: Vec<PlannedMove>,
    pub edits: Vec<PlannedEdit>,
}

/// One edit actually written by [`apply_planned_moves`], with its position
/// AFTER being written — the span [`undo_applied`] later checks and restores.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppliedEdit {
    pub file: String,
    pub byte_from: usize,
    pub byte_to: usize,
    pub old_text: String,
    pub new_text: String,
}

/// A file [`apply_planned_moves`] or [`undo_applied`] left untouched because
/// its recorded text was no longer at the recorded position.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SkippedFile {
    pub file: String,
    pub reason: String,
}

/// The result of [`apply_planned_moves`]: exactly what happened, precise
/// enough for [`undo_applied`] to reverse it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RenameApplyResult {
    pub moves: Vec<PlannedMove>,
    pub edits: Vec<AppliedEdit>,
    pub skipped: Vec<SkippedFile>,
}

/// The result of [`undo_applied`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UndoResult {
    pub restored_files: Vec<String>,
    pub skipped: Vec<SkippedFile>,
}

// ── Internal: the move-set primitive shared by plan/apply/undo ─────────────

/// One (old, new) path pair the planner treats as atomic, root-relative.
/// Includes both the caller's own moves and the folder-note carries
/// [`expand_with_home_carry`] predicts.
#[derive(Debug, Clone)]
struct ResolvedMove {
    old: String,
    new: String,
    is_dir: bool,
}

/// Map `path` through `moves`: an exact hit wins over a directory-prefix hit,
/// so a folder-note's own (more specific) file-level mapping is not shadowed
/// by its folder's coarser prefix substitution.
fn map_path(path: &str, moves: &[ResolvedMove]) -> String {
    for m in moves {
        if path == m.old {
            return m.new.clone();
        }
    }
    for m in moves {
        if m.is_dir {
            if let Some(rest) = path.strip_prefix(&format!("{}/", m.old)) {
                return format!("{}/{}", m.new, rest);
            }
        }
    }
    path.to_string()
}

/// Predict `rename_self_named_home`'s (vault/fs.rs) effect for every
/// directory move in `top_level`, using `files` — the file list reflecting
/// the state BEFORE these moves run — to detect a self-named home file.
/// Mirrors that function's exact matching rules (lang-suffix aware, index
/// stems excluded) so the two never drift: this fn only ever PREDICTS what
/// the real OS-level rename in `apply_planned_moves` will do.
fn expand_with_home_carry(top_level: &[ResolvedMove], files: &[String]) -> Vec<ResolvedMove> {
    let mut out = top_level.to_vec();
    let file_set: HashSet<&str> = files.iter().map(|s| s.as_str()).collect();
    for m in top_level {
        if !m.is_dir {
            continue;
        }
        let old_name = match m.old.rsplit('/').next() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let new_name = match m.new.rsplit('/').next() {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        let candidate = format!("{}/{}.md", m.old, old_name);
        if !file_set.contains(candidate.as_str()) {
            continue;
        }
        // candidate's stem is exactly old_name by construction.
        let stem = old_name;
        let bare_stem = moss_core::home::strip_lang_suffix(stem).unwrap_or(stem);
        if moss_core::home::is_index_stem(bare_stem) {
            continue;
        }
        if bare_stem.to_lowercase() != old_name.to_lowercase() {
            continue;
        }
        let new_filename = match stem.rsplit_once('.') {
            Some((_, suffix)) if bare_stem != stem => format!("{new_name}.{suffix}.md"),
            _ => format!("{new_name}.md"),
        };
        out.push(ResolvedMove {
            old: candidate,
            new: format!("{}/{}", m.new, new_filename),
            is_dir: false,
        });
    }
    out
}

fn dirname(root_rel: &str) -> &str {
    root_rel.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Minimal `../`-relative spelling of `to_path` from `from_dir` (both
/// root-relative, filesystem shape). Deliberately NOT `fuzzy_path`'s
/// `relative_asset_path`: that percent-encodes segments for an HTML `href`,
/// and this text is written back into markdown SOURCE, where percent-encoding
/// would be a regression an author never asked for.
fn relative_root_path(from_dir: &str, to_path: &str) -> String {
    let from_parts: Vec<&str> = if from_dir.is_empty() { vec![] } else { from_dir.split('/').collect() };
    let to_parts: Vec<&str> = to_path.split('/').collect();
    let common = from_parts.iter().zip(to_parts.iter()).take_while(|(a, b)| a == b).count();
    let ups = from_parts.len() - common;
    let mut segs: Vec<&str> = std::iter::repeat("..").take(ups).collect();
    segs.extend_from_slice(&to_parts[common..]);
    segs.join("/")
}

// ── Which resolver the BUILD actually uses for this reference ──────────────
//
// `classify_reference`/`resolve_asset_ref` is NOT the build's resolver for
// plain links and (non-embed) wikilinks. Traced via
// `crates/moss-core/src/ast/resolve_urls.rs::resolve_link_urls`: a
// `[text](url)` link or a bare `[[note]]` wikilink resolves through
// `fuzzy_path::resolve_reference` → `ContentGraph::resolve_path`
// (content_graph.rs), which is the one function its own doc comment calls
// "the single source of truth for target resolution in moss" — and it
// differs from `resolve_asset_ref` in two ways that matter here:
// `ContentGraph::resolve_path`'s folder-note fallback (content_graph.rs
// ~495-538) fires unconditionally for a bare name, where
// `classify_reference`'s folder arm (reference.rs ~212-235) only enters for
// a leading or trailing `/`; and an ambiguous bare stem always resolves to
// SOME file there (an ext/page/tree/common-prefix/alphabetical tiebreak,
// content_graph.rs ~463-491), where `resolve_asset_ref` reports `Ambiguous`
// (no target) and stops.
//
// Standard `![alt](path)` image syntax and structural spans (gallery/hero
// bodies, frontmatter asset fields) are genuinely `resolve_asset_ref`'s:
// confirmed via `resolve_urls.rs`'s `resolve_image_urls` (Phase 1), which
// drives `resolve_asset_ref` off a `ContentGraph`-backed `AssetIndex` the
// same way `GraphAssetIndex` here does. Embed wikilink syntax
// (`![[x]]`/`![[x|attrs]]`) is grouped with assets deliberately, not because
// it is provably `resolve_asset_ref`-routed for every target kind, but
// because a folder embed specifically (`![[folder/]]`) IS —
// `crates/moss-build/src/build/folder_embed.rs` calls `classify_reference`
// directly for that marker — and because no divergence has been proven for
// the non-folder embed case; a follow-up should re-verify that assumption
// the same way this fix verified the link/wikilink one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefRoute {
    /// `resolve_asset_ref` via `classify_reference` — standard image syntax,
    /// embed wikilinks, and every structural span.
    AssetOrEmbed,
    /// `ContentGraph::resolve_path` — markdown links and non-embed
    /// wikilinks.
    PageGraph,
}

fn ref_route(syntax: &RefSyntax) -> RefRoute {
    match syntax {
        RefSyntax::MarkdownImage { .. }
        | RefSyntax::WikilinkStemEmbed
        | RefSyntax::WikilinkPathEmbed
        | RefSyntax::WikilinkAliasedEmbed { .. }
        | RefSyntax::StructuralAsset => RefRoute::AssetOrEmbed,
        // A definition's destination is what `[text][id]` resolves into once
        // pulldown-cmark inlines it — the build renders it as an ordinary
        // link, so it takes the same resolver a MarkdownLink does.
        RefSyntax::MarkdownLink { .. }
        | RefSyntax::Definition { .. }
        | RefSyntax::WikilinkStem
        | RefSyntax::WikilinkPath
        | RefSyntax::WikilinkAliased { .. } => RefRoute::PageGraph,
    }
}

/// Resolve `text` (already anchor-stripped for the `PageGraph` route; the
/// `AssetOrEmbed` route strips it internally) the same way the build would
/// for a reference of this route.
fn resolve_by_route(
    route: RefRoute,
    text: &str,
    from_source: &str,
    ctx: &ReferenceContext,
    graph: &ContentGraph,
) -> Option<String> {
    match route {
        RefRoute::AssetOrEmbed => classify_reference(text, from_source, true, ctx).target_path,
        RefRoute::PageGraph => {
            let (base, _) = split_ref_suffix(text);
            // Same resolver + percent-decode fallback `resolve_link_urls`
            // (ast/resolve_urls.rs) uses for the build's own link
            // resolution, so the two never disagree on a percent-encoded
            // destination.
            match resolve_reference_with_percent_fallback(base, graph, from_source) {
                ResolvedRef::Found(p) => Some(p),
                ResolvedRef::Unresolved => None,
            }
        }
    }
}

/// Split a reference's `?query`/`#anchor` suffix off its path (earliest of
/// the two wins, mirroring `ast::resolve_urls::split_path_suffix`). Both
/// halves are opaque; a rewrite reattaches `suffix` verbatim.
fn split_ref_suffix(text: &str) -> (&str, &str) {
    let q = text.find('?');
    let h = text.find('#');
    let cut = match (q, h) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    match cut {
        Some(pos) => (&text[..pos], &text[pos..]),
        None => (text, ""),
    }
}

// ── The per-reference resolve → verify → escalate decision ─────────────────

/// Resolve `raw_text` (a `RawRef.text`, or a structural `AssetPathSpan.path`)
/// against the pre-move tree, with the resolver `route` says the build uses
/// for this reference kind; if it no longer resolves to the mapped target
/// from its post-move location, produce a rewritten form that does — trying
/// the reference's own authored shape first, then increasingly explicit
/// forms, verifying each with the SAME per-route resolver before accepting
/// it.
///
/// `Ok(None)` means: leave the reference exactly as authored (it was already
/// unresolved, or it still resolves to the right place). `Err` means no
/// producible form verifies — a defensive case that should not occur for any
/// real, still-existing target (root-absolute of a real path always resolves
/// via both routes' exact-match tier), surfaced rather than silently
/// emitting a reference that would 404.
#[allow(clippy::too_many_arguments)]
fn plan_one_ref(
    raw_text: &str,
    route: RefRoute,
    from_dir_pre: &str,
    from_source_pre: &str,
    from_source_post: &str,
    ctx_pre: &ReferenceContext,
    ctx_post: &ReferenceContext,
    graph_pre: &ContentGraph,
    graph_post: &ContentGraph,
    expanded_moves: &[ResolvedMove],
) -> Result<Option<String>, String> {
    // target_is_dir only has meaning on the AssetOrEmbed route: a folder
    // reference there is a distinct `FolderListing`/`FolderIndexIframe`
    // kind whose `target_path` IS the folder. `ContentGraph::resolve_path`
    // never returns a bare directory — its folder-note step always resolves
    // through to a FILE (the folder's home page) — so a PageGraph reference
    // is never itself "a directory" to retarget.
    let (t, target_is_dir) = match route {
        RefRoute::AssetOrEmbed => {
            let resolved = classify_reference(raw_text, from_source_pre, true, ctx_pre);
            let is_dir =
                matches!(resolved.kind, ReferenceKind::FolderListing | ReferenceKind::FolderIndexIframe);
            (resolved.target_path, is_dir)
        }
        RefRoute::PageGraph => (resolve_by_route(route, raw_text, from_source_pre, ctx_pre, graph_pre), false),
    };
    let Some(t) = t else {
        // Unresolved before the move: leave it alone (case 4 of the
        // invariant). On the AssetOrEmbed route this also covers an
        // ambiguous bare reference — `resolve_asset_ref` reports
        // `Ambiguous`, which carries no `target_path`, indistinguishable
        // from "not found" here. The PageGraph route has no such case:
        // `ContentGraph::resolve_path` always picks a deterministic winner
        // via its own tiebreak chain rather than reporting ambiguity, so a
        // reference on that route reaches `None` only when the build itself
        // would show it unresolved.
        return Ok(None);
    };
    let map_t = map_path(&t, expanded_moves);

    if resolve_by_route(route, raw_text, from_source_post, ctx_post, graph_post).as_deref()
        == Some(map_t.as_str())
    {
        return Ok(None); // still resolves to the same (mapped) place, byte-identical
    }

    // A PageGraph target with no REGISTERED file behind it is the synthetic
    // `<dir>/index.md` the folder-note fallback manufactures for an
    // auto-index dir (content_graph.rs ~512-523); no authored text ever
    // equals that string, so retargeting swaps in `dirname(t)`/`dirname(map_t)`
    // with `target_is_dir = true`, same as a real AssetOrEmbed folder ref.
    let synthetic_auto_index = route == RefRoute::PageGraph && !target_is_dir && !graph_pre.contains_path(&t);
    let (retarget_t, retarget_map_t, target_is_dir) = if synthetic_auto_index {
        (dirname(&t).to_string(), dirname(&map_t).to_string(), true)
    } else {
        (t.clone(), map_t.clone(), target_is_dir)
    };

    let (base_text, suffix) = split_ref_suffix(raw_text);
    let from_dir_post = dirname(from_source_post);
    let verify = |candidate: &str| -> bool {
        resolve_by_route(route, candidate, from_source_post, ctx_post, graph_post).as_deref()
            == Some(map_t.as_str())
    };

    // Obsidian (wikilinks off) writes a percent-encoded destination
    // (`my%20note.md`) for a path with a space or non-ASCII character; the
    // resolvers above already decode it as a fallback. A rewrite has to
    // reproduce that authored style, or an escalated candidate with a
    // literal space would emit a destination CommonMark can't parse as one
    // path.
    // `retarget_root_relative`'s own "same authored shape" comparisons are
    // never fooled by this: they run on `base_text` before this encoding is
    // applied, so an already-encoded reference that still needs no rewrite
    // is untouched by any of this.
    let needs_percent_encoding = percent_decoded_fallback(base_text).is_some();
    let maybe_encode = |s: String| if needs_percent_encoding { percent_encode_path_segments(&s) } else { s };

    // Attempt 1: same authored shape. Produced against the PRE-move
    // directory (the context the text was actually written in) — the shape
    // survives even when the referencing file itself moved; correctness is
    // never assumed, only what `verify` (against the POST-move context)
    // confirms.
    if let Some(candidate) = match_and_retarget(base_text, from_dir_pre, &retarget_t, &retarget_map_t, target_is_dir) {
        let full = format!("{}{suffix}", maybe_encode(candidate));
        if verify(&full) {
            return Ok(Some(full));
        }
    }

    // Attempt 2: explicit document-relative from the (possibly new) location.
    let mut doc_rel = relative_root_path(from_dir_post, &retarget_map_t);
    if target_is_dir {
        doc_rel.push('/');
    }
    let full = format!("{}{suffix}", maybe_encode(doc_rel));
    if verify(&full) {
        return Ok(Some(full));
    }

    // Attempt 3: explicit root-absolute — always resolves for a real path
    // on either route (an exact leading-`/` match is each resolver's first
    // tier).
    let mut abs = format!("/{retarget_map_t}");
    if target_is_dir {
        abs.push('/');
    }
    let full = format!("{}{suffix}", maybe_encode(abs));
    if verify(&full) {
        return Ok(Some(full));
    }

    Err(format!(
        "could not produce a resolving rewrite for reference {raw_text:?} (resolved to {t:?}, mapped to {map_t:?})"
    ))
}

// ── In-memory index construction ────────────────────────────────────────────

struct Indexes {
    graph: ContentGraph,
    folders: PathListFolderIndex,
}

impl Indexes {
    fn build(paths: &[String]) -> Self {
        let mut builder = ContentGraphBuilder::new();
        for p in paths {
            builder.add_file(p, "");
        }
        // Mirrors `build_content_graph`'s own call: without it, a bare
        // `[[Folder]]` wikilink to a note-less, auto-indexed folder resolved
        // on the real site but looked unresolved here, so rename left it alone.
        crate::build::scan::classify::register_auto_index_dirs(&mut builder, &collect_dirs(paths));
        Indexes { graph: builder.build(), folders: PathListFolderIndex::build(paths) }
    }
}

/// Walk every file under `canonical_root` (skipping `.moss/` and `.git/`),
/// root-relative, forward-slashed.
fn walk_all_files(canonical_root: &Path) -> Vec<String> {
    walkdir::WalkDir::new(canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let rel = e.path().strip_prefix(canonical_root).ok()?.to_string_lossy().replace('\\', "/");
            if rel.starts_with(".moss/") || rel.starts_with(".git/") {
                None
            } else {
                Some(rel)
            }
        })
        .collect()
}

fn is_markdown(rel: &str) -> bool {
    let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("");
    ext == "md" || ext == "markdown"
}

fn root_relative_existing(canonical_root: &Path, abs: &Path) -> Result<String, String> {
    let canon = std::fs::canonicalize(abs).map_err(|e| format!("Cannot canonicalize '{}': {}", abs.display(), e))?;
    canon
        .strip_prefix(canonical_root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("'{}' is not inside the project root", abs.display()))
}

/// `new_path` never exists yet, so it cannot be canonicalized — strip the
/// root prefix textually instead (mirrors the existing fallback in
/// `rename_entry_with_refs_core`).
fn root_relative_new(canonical_root: &Path, new_path: &str) -> String {
    let stripped = new_path.strip_prefix(canonical_root.to_str().unwrap_or("")).unwrap_or(new_path);
    stripped.strip_prefix('/').unwrap_or(stripped).replace('\\', "/")
}

fn line_number(source: &str, byte_from: usize) -> u32 {
    source.get(..byte_from).unwrap_or_default().chars().filter(|&c| c == '\n').count() as u32 + 1
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Plan a batch of renames/moves. Pure with respect to disk writes: reads the
/// project to build the pre/post-move reference indexes, writes nothing.
///
/// `moves` are `(old_abs, new_abs)` pairs — same convention
/// `rename_entry_with_refs_core` already took. `old_abs` must currently
/// exist; `new_abs` must not yet. Rejects a batch where one entry sits inside
/// another moved entry (a caller bug that would otherwise half-apply).
pub fn plan_moves(project_root: &Path, moves: &[(String, String)]) -> Result<RenamePlan, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).map_err(|e| format!("Cannot canonicalize root: {}", e))?;

    let mut resolved: Vec<ResolvedMove> = Vec::new();
    for (old, new) in moves {
        let old_abs = Path::new(old);
        if !old_abs.exists() {
            return Err(format!("source path does not exist: {old}"));
        }
        let is_dir = old_abs.is_dir();
        let old_rel = root_relative_existing(&canonical_root, old_abs)?;
        let new_rel = root_relative_new(&canonical_root, new);
        resolved.push(ResolvedMove { old: old_rel, new: new_rel, is_dir });
    }

    for i in 0..resolved.len() {
        for j in 0..resolved.len() {
            if i == j {
                continue;
            }
            if resolved[j].old == resolved[i].old || resolved[j].old.starts_with(&format!("{}/", resolved[i].old)) {
                return Err(format!(
                    "cannot move '{}' — it is inside another entry in the same batch ('{}')",
                    resolved[j].old, resolved[i].old
                ));
            }
        }
    }

    let pre_files = walk_all_files(&canonical_root);
    let expanded = expand_with_home_carry(&resolved, &pre_files);
    let post_files: Vec<String> = pre_files.iter().map(|p| map_path(p, &expanded)).collect();

    let idx_pre = Indexes::build(&pre_files);
    let assets_pre = GraphAssetIndex(&idx_pre.graph);
    let urls_pre = NoUrlIndex;
    let ctx_pre = ReferenceContext { assets: &assets_pre, folders: &idx_pre.folders, urls: &urls_pre };

    let idx_post = Indexes::build(&post_files);
    let assets_post = GraphAssetIndex(&idx_post.graph);
    let urls_post = NoUrlIndex;
    let ctx_post = ReferenceContext { assets: &assets_post, folders: &idx_post.folders, urls: &urls_post };

    let mut edits: Vec<PlannedEdit> = Vec::new();
    for rel in pre_files.iter().filter(|r| is_markdown(r)) {
        let abs = canonical_root.join(rel);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let from_source_post = map_path(rel, &expanded);
        let from_dir_pre = dirname(rel);

        for rr in extract_md_references(&source) {
            if let Some(new_text) = plan_one_ref(
                &rr.text,
                ref_route(&rr.syntax),
                from_dir_pre,
                rel,
                &from_source_post,
                &ctx_pre,
                &ctx_post,
                &idx_pre.graph,
                &idx_post.graph,
                &expanded,
            )? {
                edits.push(PlannedEdit {
                    file: from_source_post.clone(),
                    line: line_number(&source, rr.ref_from),
                    byte_from: rr.ref_from,
                    byte_to: rr.ref_to,
                    old_text: source[rr.ref_from..rr.ref_to].to_string(),
                    new_text,
                });
            }
        }
        for span in extract_structural_asset_refs(&source) {
            if let Some(new_path_text) = plan_one_ref(
                &span.path,
                RefRoute::AssetOrEmbed,
                from_dir_pre,
                rel,
                &from_source_post,
                &ctx_pre,
                &ctx_post,
                &idx_pre.graph,
                &idx_post.graph,
                &expanded,
            )? {
                let new_value = render_bare_value(&span.container, span.quote, &new_path_text, &span.attrs);
                edits.push(PlannedEdit {
                    file: from_source_post.clone(),
                    line: line_number(&source, span.value.start),
                    byte_from: span.value.start,
                    byte_to: span.value.end,
                    old_text: source[span.value.start..span.value.end].to_string(),
                    new_text: new_value,
                });
            }
        }
    }

    Ok(RenamePlan {
        moves: resolved.into_iter().map(|m| PlannedMove { old_path: m.old, new_path: m.new, is_dir: m.is_dir }).collect(),
        edits,
    })
}

/// Apply a [`RenamePlan`]: OS-rename every entry (via `rename_entry_inner`,
/// which carries a folder's self-named home file the same way a plain
/// rename does), then write exactly the planned edits. Before writing each
/// file, every one of its edits must still match its recorded position and
/// text — a mismatch (the file changed since planning) skips the WHOLE file
/// rather than clobbering it or guessing a new position.
pub fn apply_planned_moves(project_root: &Path, plan: &RenamePlan) -> Result<RenameApplyResult, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).map_err(|e| format!("Cannot canonicalize root: {}", e))?;

    for mv in &plan.moves {
        let old_abs = canonical_root.join(&mv.old_path);
        let new_abs = canonical_root.join(&mv.new_path);
        crate::vault::fs::rename_entry_inner(
            &canonical_root,
            &old_abs.to_string_lossy(),
            &new_abs.to_string_lossy(),
        )?;
    }

    let mut file_order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for e in &plan.edits {
        if seen.insert(e.file.clone()) {
            file_order.push(e.file.clone());
        }
    }

    let mut applied_edits = Vec::new();
    let mut skipped = Vec::new();
    for file in file_order {
        let file_edits: Vec<&PlannedEdit> = plan.edits.iter().filter(|e| e.file == file).collect();
        let abs = canonical_root.join(&file);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                skipped.push(SkippedFile { file, reason: "file not found after the move".to_string() });
                continue;
            }
        };
        let stale = file_edits.iter().any(|e| source.get(e.byte_from..e.byte_to) != Some(e.old_text.as_str()));
        if stale {
            skipped.push(SkippedFile { file, reason: "reference text changed since planning".to_string() });
            continue;
        }
        let to_apply: Vec<Edit> =
            file_edits.iter().map(|e| Edit { from: e.byte_from, to: e.byte_to, text: e.new_text.clone() }).collect();
        let rewritten = apply_edits(&source, to_apply);
        // allow:raw_write the vault's own .md source, rewritten in place after a rename -- not build output
        std::fs::write(&abs, &rewritten).map_err(|e| format!("Failed to write '{}': {}", abs.display(), e))?;
        for e in file_edits {
            applied_edits.push(AppliedEdit {
                file: file.clone(),
                byte_from: e.byte_from,
                byte_to: e.byte_from + e.new_text.len(),
                old_text: e.old_text.clone(),
                new_text: e.new_text.clone(),
            });
        }
    }

    Ok(RenameApplyResult { moves: plan.moves.clone(), edits: applied_edits, skipped })
}

/// Reverse an [`apply_planned_moves`] result: rename every entry back (in
/// reverse order), then restore each edit's original text — with the same
/// staleness check, so a page a viewer edited after the rename is skipped
/// rather than clobbered. Restores the ORIGINAL bytes even when the forward
/// rewrite escalated a bare reference to an explicit path.
pub fn undo_applied(project_root: &Path, applied: &RenameApplyResult) -> Result<UndoResult, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).map_err(|e| format!("Cannot canonicalize root: {}", e))?;

    // The reverse move-set, built from the CURRENT (post-apply, pre-undo)
    // file list so the folder-note carry prediction sees the state the
    // forward apply actually left behind.
    let current_files = walk_all_files(&canonical_root);
    let reversed_top: Vec<ResolvedMove> = applied
        .moves
        .iter()
        .map(|m| ResolvedMove { old: m.new_path.clone(), new: m.old_path.clone(), is_dir: m.is_dir })
        .collect();
    let expanded_reverse = expand_with_home_carry(&reversed_top, &current_files);

    for mv in applied.moves.iter().rev() {
        let cur_abs = canonical_root.join(&mv.new_path);
        let restored_abs = canonical_root.join(&mv.old_path);
        crate::vault::fs::rename_entry_inner(
            &canonical_root,
            &cur_abs.to_string_lossy(),
            &restored_abs.to_string_lossy(),
        )?;
    }

    let mut file_order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for e in &applied.edits {
        if seen.insert(e.file.clone()) {
            file_order.push(e.file.clone());
        }
    }

    let mut restored_files = Vec::new();
    let mut skipped = Vec::new();
    for post_apply_file in file_order {
        let pre_apply_file = map_path(&post_apply_file, &expanded_reverse);
        let file_edits: Vec<&AppliedEdit> = applied.edits.iter().filter(|e| e.file == post_apply_file).collect();
        let abs = canonical_root.join(&pre_apply_file);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => {
                skipped.push(SkippedFile { file: pre_apply_file, reason: "file not found while undoing".to_string() });
                continue;
            }
        };
        let stale = file_edits.iter().any(|e| source.get(e.byte_from..e.byte_to) != Some(e.new_text.as_str()));
        if stale {
            skipped.push(SkippedFile { file: pre_apply_file, reason: "reference text changed since the rename".to_string() });
            continue;
        }
        let to_apply: Vec<Edit> =
            file_edits.iter().map(|e| Edit { from: e.byte_from, to: e.byte_to, text: e.old_text.clone() }).collect();
        let restored = apply_edits(&source, to_apply);
        // allow:raw_write undoing a rename's own reference rewrite -- not build output
        std::fs::write(&abs, &restored).map_err(|e| format!("Failed to write '{}': {}", abs.display(), e))?;
        restored_files.push(pre_apply_file);
    }

    Ok(UndoResult { restored_files, skipped })
}

#[cfg(test)]
#[path = "rename_plan_tests.rs"]
mod tests;
