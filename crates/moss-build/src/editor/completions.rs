//! Link-target completion: everything an author can link to, joined from the
//! source tree and the last build's article map, then ranked by the pure
//! `moss_core::link_completions` ranker.
//!
//! The author links to a *thing*; moss writes the address. Which address is
//! decided by the syntax the caret is in and whether the author opened with
//! `/` (`InsertCtx::url_space`), never by the caller. The four kinds of
//! thing: a page or asset file (walked from disk), a folder (derived from the
//! file paths, an offer to descend), and a page the build synthesizes
//! (read from the map: it has a URL and nothing on disk). Headings are a
//! separate ask, for one page.
//!
//! All I/O is here; the ranker sees `Target`s.

use std::collections::BTreeSet;
use std::path::Path;

use moss_core::link_completions::{insert_for, rank_completions, InsertCtx, LinkSyntax, Target, TargetKind};
use moss_core::resolve::ext_kind::{reference_kind_for_ext, ExtKind};

use crate::build::scan::article_map::ArticleMap;
use crate::editor::filesystem::walk_source_files;

/// Max ranked results returned to the editor. Large CJK sites have hundreds
/// of notes; the dropdown only needs the top slice.
const RESULT_CAP: usize = 50;

/// One completion row, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct WikilinkCompletion {
    /// The exact text accepting the row writes into the link.
    pub insert: String,
    /// The thing's name: page title, filename, `dir/`, term text, heading.
    pub label: String,
    /// The dimmer second column. The insert text when it differs from the
    /// label (so the author learns the address on the spot); the heading's
    /// level (`H2`) when the two coincide.
    pub detail: Option<String>,
    pub kind: TargetKind,
}

/// Rank every link target of the project against `prefix`.
///
/// `from_file` is the file being edited, absolute (the editor's form) or
/// project-relative (the cover picker's); it is relativized here because the
/// ranker's proximity bias reads project-relative paths. `allowed_kinds`
/// restricts the list to assets of the given kinds — `None` is unrestricted,
/// `Some(&[])` matches nothing by design (an asset-only context with no
/// acceptable kind offers nothing rather than everything).
pub fn link_completions(
    project_path: &str,
    from_file: &str,
    syntax: LinkSyntax,
    prefix: &str,
    allowed_kinds: Option<&[ExtKind]>,
) -> Vec<WikilinkCompletion> {
    let map = ArticleMap::load(&Path::new(project_path).join(".moss")).unwrap_or_default();
    let mut targets = collect_targets(project_path, &map);
    if let Some(kinds) = allowed_kinds {
        targets.retain(|t| match t {
            Target::Asset { source } => kinds.contains(&reference_kind_for_ext(&ext_of(source))),
            _ => false,
        });
    }
    let from_rel = rel_to_root(Path::new(from_file), Path::new(project_path));
    let ctx = InsertCtx { syntax, prefix, from_rel: &from_rel };
    rank_completions(&targets, &ctx)
        .into_iter()
        .take(RESULT_CAP)
        .map(|i| row(&targets[i], &ctx))
        .collect()
}

/// Rank the headings of one project-relative page against `prefix`. Reads the
/// file path-guarded; an unreadable file yields no rows (autocomplete
/// degrades silently).
pub fn heading_completions(
    project_path: &str,
    relative_page: &str,
    syntax: LinkSyntax,
    prefix: &str,
) -> Result<Vec<WikilinkCompletion>, String> {
    let targets = heading_targets(project_path, relative_page)?;
    let ctx = InsertCtx { syntax, prefix, from_rel: relative_page };
    Ok(rank_completions(&targets, &ctx)
        .into_iter()
        .take(RESULT_CAP)
        .map(|i| row(&targets[i], &ctx))
        .collect())
}

/// Every target the project offers: pages and assets from the source walk
/// (pages joined to the map's URL and title), the folders those paths pass
/// through, and the map's synthesized pages.
fn collect_targets(project_path: &str, map: &ArticleMap) -> Vec<Target> {
    let by_source: std::collections::HashMap<&str, (String, &str)> = map
        .sourced_pages()
        .map(|(key, source, title)| (source, (ArticleMap::canonical_url(key), title)))
        .collect();

    let mut targets = Vec::new();
    let mut folders = BTreeSet::new();
    walk_source_files(project_path, project_path, &mut |_, rel| {
        let source = rel.replace('\\', "/");
        for (i, _) in source.match_indices('/') {
            folders.insert(source[..i].to_string());
        }
        if rel.ends_with(".md") {
            let (url, title) = by_source
                .get(rel)
                .map(|(u, t)| (u.clone(), t.to_string()))
                .unwrap_or_default();
            targets.push(Target::Page { source, title, url });
        } else {
            targets.push(Target::Asset { source });
        }
    });
    targets.extend(folders.into_iter().map(|source| Target::Folder { source }));

    // A claimed term lives at its claiming page, which the walk already
    // offered; only an unclaimed term is a page of its own. The `generated`
    // list also holds index-less folder pages and the term namespace roots,
    // whose display is their last URL segment.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (key, term) in &map.terms {
        if term.claimed_by.is_some() {
            continue;
        }
        let url = ArticleMap::canonical_url(key);
        if seen.insert(url.clone()) {
            targets.push(Target::Generated { url, display: term.display.clone() });
        }
    }
    for key in &map.generated {
        let url = ArticleMap::canonical_url(key);
        if seen.insert(url.clone()) {
            let display = key.trim_matches('/').rsplit('/').next().unwrap_or("").to_string();
            targets.push(Target::Generated { url, display });
        }
    }
    targets
}

/// The headings of one page as targets, parsed with the SITE's math setting.
///
/// The inserted text becomes a `[[Page#Heading]]` anchor, and the anchor it
/// has to match is the one the render path emits — which resolves
/// `[site].math` exactly this way. Where the two configs disagree they
/// produce DIFFERENT text and therefore different slugs: with math off,
/// intraword `*` is eaten as emphasis first, so `# Dual $V^*$ end` extracts
/// as `Dual $V^$ end` and slugs to an anchor the published page does not
/// have. Absent key => on, same as everywhere else.
fn heading_targets(project_path: &str, relative_page: &str) -> Result<Vec<Target>, String> {
    crate::vault::fs::rejects_traversal(relative_page)?;
    let project_root = Path::new(project_path);
    let file_path = project_root.join(relative_page);
    crate::vault::fs::validate_entry_path(project_root, &file_path.to_string_lossy())?;
    let Ok(src) = std::fs::read_to_string(&file_path) else {
        return Ok(Vec::new());
    };
    let math = crate::build::site_config::get_site_math(project_path)
        .ok()
        .flatten()
        .unwrap_or(true);
    let config = moss_core::ast::ParseConfig { math, ..Default::default() };
    Ok(moss_core::heading::extract::extract_headings_with_config(&src, &config)
        .into_iter()
        .map(|h| Target::Heading { text: h.text, slug: h.slug, level: h.level })
        .collect())
}

/// Resolve a file the author picked in the SYSTEM FILE DIALOG into the same
/// row shape the search returns, so every surface feeds one accept path.
///
/// A pick names one exact file, so the reference is always the exact form
/// (`asset_ref_relative`), never a bare filename a stem-keyed resolver could
/// re-point later when a same-named file appears elsewhere.
pub fn asset_ref_in(
    project_path: &Path,
    abs_path: &str,
    from_page: &str,
) -> Result<WikilinkCompletion, String> {
    // Containment twice: the shared string guard first, then again on the
    // canonical form. `validate_entry_path` deliberately does not
    // canonicalize (it must work for paths that do not exist yet), and its
    // own docs say a command whose path MUST exist has to repeat the check
    // canonically to defeat a symlink escape. A picked file always exists.
    let picked = crate::vault::fs::validate_entry_path(project_path, abs_path)?;
    let picked = std::fs::canonicalize(&picked)
        .map_err(|e| format!("Cannot read the picked file: {e}"))?;
    let root = std::fs::canonicalize(project_path)
        .map_err(|e| format!("Cannot read the project folder: {e}"))?;
    if !picked.starts_with(&root) {
        return Err("Paths must be within the project directory".to_string());
    }
    if picked.is_dir() {
        return Err("Pick a file, not a folder".to_string());
    }
    let source = rel_to_root(&picked, &root);
    // An extension moss cannot classify would produce a reference that
    // renders as a bare link at best — refuse it here rather than emit a
    // token the build will not resolve.
    let ext = ext_of(&source);
    if reference_kind_for_ext(&ext) == ExtKind::Other {
        return Err(format!("moss does not handle .{ext} files"));
    }
    let from_rel = rel_to_root(Path::new(from_page), project_path);
    let target = Target::Asset { source };
    let ctx = InsertCtx { syntax: LinkSyntax::Inline, prefix: "", from_rel: &from_rel };
    Ok(row(&target, &ctx))
}

fn row(target: &Target, ctx: &InsertCtx<'_>) -> WikilinkCompletion {
    let insert = insert_for(target, ctx);
    let label = target.label();
    let detail = match target {
        Target::Heading { level, .. } if insert == label => Some(format!("H{level}")),
        _ if insert == label => None,
        _ => Some(insert.clone()),
    };
    WikilinkCompletion { insert, label, detail, kind: target.kind() }
}

/// Lowercased extension without the dot; empty when there is none.
fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `path` expressed relative to `root`, forward-slashed; an already-relative
/// or out-of-tree path falls through unchanged.
fn rel_to_root(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[path = "completions_tests.rs"]
mod tests;
