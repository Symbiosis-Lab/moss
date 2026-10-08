//! Resolver-driven rename/move planning and application.
//!
//! Pattern-matching a reference's raw text against the renamed entry's
//! old/new path covers only a hand-picked subset of the rules the real
//! resolver (`classify_reference` / `ContentGraph::resolve_path`) knows, so a reference
//! the resolver accepts through another route (a folder's self-named note
//! reached by its bare filename, a suffix-fallback match, …) would go stale
//! silently after a rename. So the invariant is explicit: a rename/move never
//! changes what a reference resolves to, modulo mapping old paths to new
//! ones. For every reference: resolve it against the PRE-move tree; if it
//! still resolves to the mapped target from its POST-move location, leave it
//! byte-identical; otherwise rewrite it, preserving its authored form where
//! possible, and VERIFY the rewritten text actually resolves before using it.
//!
//! Both the "pre" and "post" resolution passes run against an in-memory
//! [`ContentGraph`] built from a plain path list, and the addresses pages will
//! be served at are worked out from the files where they are now — planning
//! reads the project and never writes, so `plan_moves` can run against a
//! hypothetical rename and answer "what would change" without changing
//! anything (the desktop's confirmation modal, and CLI dry-runs).
//! `apply_planned_moves` is the only place that writes.
//!
//! `rename_entry_with_refs_core` — the public entry point used by
//! both the app's rename command and `moss rename` — is a one-element
//! wrapper: `plan_moves` + `apply_planned_moves` for a batch of exactly one.

mod apply;
mod path_list_folder_index;
mod served_address;

use std::collections::HashSet;
use std::path::Path;

use moss_core::content_graph::{ContentGraph, ContentGraphBuilder};
use moss_core::resolve::md_extract::{extract_md_references, extract_structural_asset_refs, RefSyntax};
use moss_core::resolve::fuzzy_path::{
    percent_decoded_fallback, resolve_reference, ResolvedRef,
};
use moss_core::resolve::reference::{classify_reference, ReferenceContext, ReferenceKind};

use crate::build::folder_index::NoUrlIndex;
use crate::editor::ref_rewrite::{
    dirname, exact_spelling, exact_style, match_and_retarget, relative_root_path, render_bare_value, render_destination,
    dest_form, DestForm, ExactStyle, WIKI_UNWRITABLE,
};
use path_list_folder_index::{collect_dirs, PathListFolderIndex};
use served_address::Addresses;

pub use apply::{apply_planned_moves, undo_applied};

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

/// Predict the home-file carry (`home_carry_names` in vault/fs.rs) for every
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

// ── Which entry the BUILD uses for this reference ──────────────────────────
//
// Every reference form names its file through `ContentGraph::resolve_path`, so
// both routes below agree on which file a target means, including which of
// several same-named files is nearest the page. They differ only in what
// surrounds that answer: the asset route goes through `classify_reference`,
// which also knows trailing-slash folder references (`![[folder/]]`) and the
// kind of file found; the page route calls the resolver directly and so also
// reaches a folder's home page by a bare folder name. Standard `![alt](path)`
// images, embed wikilinks and structural spans (gallery/hero bodies,
// frontmatter asset fields) take the asset route; markdown links and
// non-embed wikilinks take the page route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefRoute {
    /// `classify_reference` — standard image syntax, embed wikilinks, and
    /// every structural span.
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

/// [`exact_style`] of a finished destination `text` (suffix and percent
/// escapes included), the way the reader will see it.
fn exact_style_of_text(text: &str, from_dir: &str, target: &str, is_dir: bool, wiki: bool) -> Option<ExactStyle> {
    let (base, _) = split_ref_suffix(text);
    exact_style(base, from_dir, target, is_dir, wiki).or_else(|| {
        percent_decoded_fallback(base).and_then(|decoded| exact_style(&decoded, from_dir, target, is_dir, wiki))
    })
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
            // The resolver the build's own links go through, which retries a
            // percent-encoded destination decoded, so the two never disagree.
            match resolve_reference(base, graph, from_source) {
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

/// A root-relative link that names a page by the address the site serves it at.
struct ServedPage {
    page: String,
    /// The address, slash-terminated (`/docs/`) whichever way it was written.
    address: String,
}

/// The build keeps a link starting with `/` verbatim, so one written as a
/// published address names whichever page the site serves there, however that
/// address is spelled: a slugged name, a frontmatter `url:`, a numbered or
/// language-prefixed address. The trailing `/` is optional (`/docs` and
/// `/docs/` are one address). `None` for anything else, including a path to a
/// file such as `/files/a.pdf`, which stays on the path route.
fn served_page_of(base_text: &str, addresses: &Addresses) -> Option<ServedPage> {
    if !base_text.starts_with('/') {
        return None;
    }
    let slashed = |s: &str| if s.ends_with('/') { s.to_string() } else { format!("{s}/") };
    [Some(base_text.to_string()), percent_decoded_fallback(base_text)]
        .into_iter()
        .flatten()
        .map(|a| slashed(&a))
        .find_map(|address| addresses.page_at(&address).map(|p| ServedPage { page: p.to_string(), address }))
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
    form: DestForm,
    from_source_pre: &str,
    from_source_post: &str,
    ctx_pre: &ReferenceContext,
    ctx_post: &ReferenceContext,
    graph_pre: &ContentGraph,
    graph_post: &ContentGraph,
    expanded_moves: &[ResolvedMove],
    addresses: &Addresses,
) -> Result<Option<String>, String> {
    let (base_text, suffix) = split_ref_suffix(raw_text);
    let wiki = form == DestForm::Wiki;
    // `/` is the site's front door whatever file is served there.
    if base_text == "/" && route == RefRoute::PageGraph && !wiki {
        return Ok(None);
    }
    let served = (route == RefRoute::PageGraph && !wiki)
        .then(|| served_page_of(base_text, addresses))
        .flatten();

    // target_is_dir only has meaning on the AssetOrEmbed route: a folder
    // reference there is a distinct `FolderListing`/`FolderIndexIframe`
    // kind whose `target_path` IS the folder. `ContentGraph::resolve_path`
    // never returns a bare directory — its folder-note step always resolves
    // through to a FILE (the folder's home page) — so a PageGraph reference
    // is never itself "a directory" to retarget.
    let (t, target_is_dir) = match route {
        _ if served.is_some() => (served.as_ref().map(|s| s.page.clone()), false),
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
        // invariant). Both routes pick the file the build links even when
        // several are equally near (the editor only flags that as ambiguous),
        // so a reference reaches `None` only when the build itself would show
        // it unresolved.
        return Ok(None);
    };
    let map_t = map_path(&t, expanded_moves);
    let from_dir_post = dirname(from_source_post);

    // A published address is compared as an address: the page keeps the URL
    // the build serves it at (a root page becoming its own folder's home does)
    // or it does not, whatever happened to its file name.
    if served.as_ref().is_some_and(|s| addresses.after_moves(&map_t).ok() == Some(s.address.as_str())) {
        return Ok(None);
    }

    // A PageGraph target with no REGISTERED file behind it is the synthetic
    // `<dir>/index.md` the folder-note fallback manufactures for an
    // auto-index dir (content_graph.rs ~512-523); no authored text ever
    // equals that string, so retargeting swaps in `dirname(t)`/`dirname(map_t)`
    // with `target_is_dir = true`, same as a real AssetOrEmbed folder ref.
    let synthetic_auto_index = route == RefRoute::PageGraph && !target_is_dir && !graph_pre.contains_path(&t);

    // Did the authored text name its file EXACTLY (as a path from the page,
    // from the root, or as a published address)? Then it has to keep doing so,
    // in the same style: the resolver's name search would still find a moved
    // file through a path that is now dead for every other tool.
    let exact_of = |from_dir: &str, target: &str| exact_style_of_text(base_text, from_dir, target, target_is_dir, wiki);
    let exact = match served {
        Some(_) => Some(ExactStyle::root_address(base_text.ends_with('/'))),
        None if synthetic_auto_index => None,
        None => exact_of(dirname(from_source_pre), &t),
    };
    let resolves_post = resolve_by_route(route, raw_text, from_source_post, ctx_post, graph_post).as_deref()
        == Some(map_t.as_str());
    let unchanged = match &exact {
        // The address differs (the check above): never "still resolves".
                Some(style) => resolves_post && exact_of(from_dir_post, &map_t).is_some_and(|post| post.same_kind(style)),
        None => resolves_post,
    };
    if unchanged {
        return Ok(None); // still names the same (mapped) place, byte-identical
    }

    let (retarget_t, retarget_map_t, target_is_dir) = if synthetic_auto_index {
        (dirname(&t).to_string(), dirname(&map_t).to_string(), true)
    } else {
        (t.clone(), map_t.clone(), target_is_dir)
    };

    let no_rewrite = || {
        if wiki && map_t.contains(WIKI_UNWRITABLE) {
            return format!(
                "the new name {map_t:?} cannot be written in a wikilink, which cannot carry '|', '#', '?' or ']'; the link {raw_text:?} in {from_source_pre} was not rewritten"
            );
        }
        format!("could not produce a resolving rewrite for reference {raw_text:?} in {from_source_pre} (resolved to {t:?}, mapped to {map_t:?})")
    };
    let verify = |candidate: &str| -> bool {
        resolve_by_route(route, candidate, from_source_post, ctx_post, graph_post).as_deref()
            == Some(map_t.as_str())
    };

    // Obsidian (wikilinks off) writes a percent-encoded destination
    // (`my%20note.md`) for a path with a space or non-ASCII character; the
    // resolvers above already decode it as a fallback. A rewrite reproduces
    // that authored style; otherwise `render_destination` escapes only what
    // would break the destination. Either way a candidate that would not
    // parse back as the same destination is dropped (`None`).
    // `retarget_root_relative`'s own "same authored shape" comparisons are
    // never fooled by this: they run on `base_text` before any encoding, so
    // an already-encoded reference that still needs no rewrite is untouched.
    let percent_style = percent_decoded_fallback(base_text).is_some();
    let render = |candidate: &str| render_destination(form, candidate, suffix, percent_style);

    // An exact path stays exact, in the authored style, or in the same anchor's
    // full-path form when the style no longer fits. Besides resolving, the
    // candidate must still name the file exactly by itself, so a form that only
    // the resolver's name search follows is never accepted: no candidate means
    // the error below, and nothing is changed.
    if let Some(style) = exact.as_ref().filter(|s| s.is_root_address()) {
        // The build keeps a destination starting with `/` verbatim, so a link
        // written as a published address must carry the address the build
        // serves the new file at, which is not spelled from the file name.
        let address = addresses.after_moves(&map_t).map_err(|why| {
            format!("cannot rewrite the address {raw_text:?} in {from_source_pre}: {map_t:?} {}", why.reason())
        })?;
        let text = render(style.respell_address(address, &map_t)).ok_or_else(no_rewrite)?;
        return Ok((text != raw_text).then_some(text));
    }
    if let Some(style) = &exact {
        for style in std::iter::once(*style).chain(style.full_path()) {
            let Some(mut full) = exact_spelling(&style, &map_t, from_dir_post).and_then(|c| render(&c)) else {
                continue;
            };
            // A wikilink with no `/` is a name, so a page-relative one gets
            // `./` to stay a path. A root-relative one for a file at the root
            // is just its name, and the resolver tries the exact root path
            // before it searches: `verify` confirms that.
            let root_level = wiki && style.is_root_bare() && !full.contains('/');
            if wiki && !full.contains('/') && !root_level {
                full.insert_str(0, "./");
            }
            let named_exactly = root_level
                || exact_style_of_text(&full, from_dir_post, &map_t, target_is_dir, wiki)
                    .is_some_and(|post| post.same_kind(&style));
            if named_exactly && verify(&full) {
                return Ok(Some(full));
            }
        }
        return Err(no_rewrite());
    }

    // Attempt 1: same authored shape for a reference that was never an exact
    // path (a bare name, a suffix match). `verify` (against the POST-move
    // context) confirms the result; correctness is never assumed.
    if let Some(full) = match_and_retarget(base_text, &retarget_t, &retarget_map_t, target_is_dir)
        .and_then(|c| render(&c))
    {
        if verify(&full) {
            return Ok(Some(full));
        }
    }

    // Attempt 2: explicit document-relative from the (possibly new) location.
    let mut doc_rel = relative_root_path(from_dir_post, &retarget_map_t);
    if target_is_dir {
        doc_rel.push('/');
    }
    if let Some(full) = render(&doc_rel) {
        if verify(&full) {
            return Ok(Some(full));
        }
    }

    // Attempt 3: explicit root-absolute — always resolves for a real path
    // on either route (an exact leading-`/` match is each resolver's first
    // tier).
    let mut abs = format!("/{retarget_map_t}");
    if target_is_dir {
        abs.push('/');
    }
    if let Some(full) = render(&abs) {
        if verify(&full) {
            return Ok(Some(full));
        }
    }

    Err(no_rewrite())
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

/// Every file of the site under `canonical_root`, root-relative and
/// forward-slashed, as the build's folder walk finds them: a symbolic link is
/// not followed, and nothing [`left_out_of_site`](crate::build::scan::classify::left_out_of_site) is walked.
fn walk_all_files(canonical_root: &Path) -> Vec<String> {
    walkdir::WalkDir::new(canonical_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| crate::build::scan::classify::left_out_of_site(e).is_none())
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| Some(e.path().strip_prefix(canonical_root).ok()?.to_string_lossy().replace('\\', "/")))
        .collect()
}

fn is_page(rel: &str) -> bool {
    crate::build::scan::classify::is_page_path(Path::new(rel))
}

/// The files whose own links a rename rewrites: the site's pages, the agent
/// instruction files in the site folder, and every page being moved wherever
/// it comes from (a hidden folder, a site of its own). The build leaves the
/// last two out of the site, but their links still have to keep working.
/// Symbolic links are never followed.
fn pages_to_rewrite(canonical_root: &Path, site_files: &[String], moves: &[ResolvedMove]) -> Vec<String> {
    // Inside a moved entry, whatever the build would leave out of the site
    // (`node_modules`, a hidden folder, a nested site) stays out; the entry
    // itself is kept even when it is one of those.
    let files_under = |rel: &str, depth: usize, site_rule: bool| -> Vec<String> {
        let site_depth = rel.split('/').filter(|c| !c.is_empty()).count();
        walkdir::WalkDir::new(canonical_root.join(rel))
            .follow_links(false)
            .max_depth(depth)
            .into_iter()
            .filter_entry(|e| {
                !site_rule
                    || e.depth() == 0
                    || crate::build::scan::classify::left_out(e.path(), e.file_type().is_dir(), site_depth + e.depth()).is_none()
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| Some(e.path().strip_prefix(canonical_root).ok()?.to_string_lossy().replace('\\', "/")))
            .collect()
    };
    let agent_files = files_under("", 1, false).into_iter().filter(|p| crate::build::scan::classify::is_agent_config_name(p));
    let moved = moves.iter().flat_map(|m| files_under(&m.old, usize::MAX, true));
    let mut seen = HashSet::new();
    site_files.iter().cloned().chain(agent_files).chain(moved).filter(|p| is_page(p) && seen.insert(p.clone())).collect()
}

fn root_relative_existing(canonical_root: &Path, abs: &Path) -> Result<String, String> {
    let canon = std::fs::canonicalize(abs).map_err(|e| format!("Cannot canonicalize '{}': {}", abs.display(), e))?;
    canon
        .strip_prefix(canonical_root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("'{}' is not inside the project root", abs.display()))
}

/// `new_path` never exists yet, so validate and canonicalize its existing
/// parent, then append the final component before deriving the root-relative
/// path. This also collapses filesystem aliases such as macOS's `/var` →
/// `/private/var`.
fn root_relative_new(canonical_root: &Path, new_path: &str) -> Result<String, String> {
    let new_path = Path::new(new_path);
    let file_name = new_path
        .file_name()
        .ok_or_else(|| format!("Invalid destination path: {}", new_path.display()))?;
    let parent = new_path
        .parent()
        .ok_or_else(|| format!("Invalid destination path: {}", new_path.display()))?;
    let (_, canonical_parent) = crate::vault::fs::recheck_canonical(canonical_root, parent)?;
    canonical_parent
        .join(file_name)
        .strip_prefix(canonical_root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("'{}' is not inside the project root", new_path.display()))
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
        // Reject traversal in the authored spelling before any canonicalization
        // can erase it. The same guard protects the direct rename door.
        crate::vault::fs::rejects_traversal(old)?;
        crate::vault::fs::rejects_traversal(new)?;
        let old_abs = Path::new(old);
        if !old_abs.exists() {
            return Err(format!("source path does not exist: {old}"));
        }
        let is_dir = old_abs.is_dir();
        let old_rel = root_relative_existing(&canonical_root, old_abs)?;
        let new_rel = root_relative_new(&canonical_root, new)?;
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
    let urls_pre = NoUrlIndex;
    let ctx_pre = ReferenceContext { assets: &idx_pre.graph, folders: &idx_pre.folders, urls: &urls_pre };

    let idx_post = Indexes::build(&post_files);
    let urls_post = NoUrlIndex;
    let ctx_post = ReferenceContext { assets: &idx_post.graph, folders: &idx_post.folders, urls: &urls_post };

    let addresses = Addresses::new(&canonical_root, &pre_files, &post_files);

    let mut edits: Vec<PlannedEdit> = Vec::new();
    for rel in &pages_to_rewrite(&canonical_root, &pre_files, &resolved) {
        let abs = canonical_root.join(rel);
        let source = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let from_source_post = map_path(rel, &expanded);

        for rr in extract_md_references(&source) {
            if let Some(new_text) = plan_one_ref(
                &rr.text,
                ref_route(&rr.syntax),
                dest_form(&source, &rr),
                rel,
                &from_source_post,
                &ctx_pre,
                &ctx_post,
                &idx_pre.graph,
                &idx_post.graph,
                &expanded,
                &addresses,
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
                DestForm::Bare,
                rel,
                &from_source_post,
                &ctx_pre,
                &ctx_post,
                &idx_pre.graph,
                &idx_post.graph,
                &expanded,
                &addresses,
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

#[cfg(test)]
#[path = "rename_plan_tests.rs"]
mod tests;
