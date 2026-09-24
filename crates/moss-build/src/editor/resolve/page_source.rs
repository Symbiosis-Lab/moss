//! Disk-truth resolution for a preview URL's editable source.
//!
//! The article-map is the fast path for "what file backs this URL", and
//! `commands::resolve_page_source_inner` tries it first. This module is the
//! other half: what the editor can work out from the filesystem alone when the
//! map is absent, stale, or behind — a freshly-written home the build has not
//! registered yet, an empty folder no article nests under, a CJK directory
//! whose served slug has to be reversed.
//!
//! It lives here rather than beside the command because that is this
//! directory's charter (see `editor/resolve.rs`): every module here answers one
//! question about a reference, and "which real file/folder does this URL point
//! at" is the editor's version of exactly that question.

use crate::build::scan::scan::home_marker_of;

/// Recover a folder's REAL on-disk identity from the ArticleMap when a URL
/// maps to a directory with no editable home file. Uses children's real
/// `source_path` (case/space preserving) — NOT a slug reversal of the URL.
///
/// `folder_url` is the normalized, trailing-slash-trimmed URL (e.g. `my-blog`
/// or `my-blog/2024`). Returns `(dir_path_abs, dir_name)` or `(None, None)`
/// when no child article nests under the folder URL (→ a generated page with
/// no backing folder).
pub fn derive_folder_identity(
    map: &crate::build::scan::article_map::ArticleMap,
    folder_path: &std::path::Path,
    folder_url: &str,
) -> (Option<String>, Option<String>) {
    use std::path::{Path, PathBuf};
    if folder_url.is_empty() {
        return (None, None); // root is handled via isRoot, never via dir_path
    }
    let depth = folder_url.split('/').count();
    let prefix = format!("{folder_url}/");
    for (url, info) in &map.articles {
        let url_trim = url.trim_end_matches('/');
        let nests = url_trim == folder_url || url_trim.starts_with(&prefix);
        if !nests || info.source_path.is_empty() {
            continue;
        }
        let comps: Vec<_> = Path::new(&info.source_path).components().collect();
        // child source has the file at depth+… ; the folder is the first
        // `depth` path components (moss preserves dir structure in URLs).
        if comps.len() <= depth {
            continue;
        }
        let dir_rel: PathBuf = comps[..depth].iter().collect();
        let dir_name = match dir_rel.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        let dir_abs = folder_path.join(&dir_rel).to_string_lossy().to_string();
        return (Some(dir_abs), Some(dir_name));
    }
    // No child article nests under this URL: an empty (or index-less) folder the
    // build hasn't populated yet — the ArticleMap can't help. Fall back to DISK
    // TRUTH (mirrors `detect_root_home_source`'s source-of-truth disk read for
    // the root): reverse the served-URL slug to a real on-disk directory. A
    // synthesized page (RSS/sitemap/tag/404/asset) with no backing directory
    // degrades to (None, None) → no false folder button.
    derive_folder_identity_from_disk(folder_path, folder_url, &map.dir_overrides)
}

/// Disk-truth fallback for [`derive_folder_identity`]: walk the real directory
/// tree segment by segment, reversing each served-URL slug back to its on-disk
/// directory. Honors `dir_overrides` (CJK renames) exactly as
/// `resolve_path_with_overrides` does in the FORWARD (source→URL) direction, and
/// falls back to `generate_slug(dir_name)` for un-overridden segments.
///
/// This is what recovers an EMPTY folder's editor button: `resolve_page_source`
/// derives no source file and the ArticleMap holds no child, but the directory
/// exists on disk, so a real `(dir_abs, dir_name)` is returned and the editor
/// mounts the "Create folder page" CTA.
///
/// Returns `(dir_abs, dir_name)` for a real backing directory, or `(None, None)`
/// when ANY URL segment has no matching on-disk subdirectory — so a synthesized
/// page never gets a false button. Internal/dot-prefixed dirs (`.moss`, `.git`)
/// are skipped so their slug can never masquerade as a content folder.
fn derive_folder_identity_from_disk(
    folder_path: &std::path::Path,
    folder_url: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> (Option<String>, Option<String>) {
    use moss_core::slug::generate_slug;
    if folder_url.is_empty() {
        return (None, None);
    }
    let mut current = folder_path.to_path_buf();
    let mut cumulative_src = String::new();
    for segment in folder_url.split('/') {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => return (None, None),
        };
        let mut matched: Option<std::path::PathBuf> = None;
        for entry in entries.filter_map(|e| e.ok()) {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            // Never let an internal/hidden dir's slug match a content URL segment.
            if name.starts_with('.') || name.starts_with('_') {
                continue;
            }
            let src_key = if cumulative_src.is_empty() {
                name.clone()
            } else {
                format!("{cumulative_src}/{name}")
            };
            // Forward slug for THIS segment: dir_overrides win, else generate_slug.
            let slug = dir_overrides
                .get(&src_key)
                .cloned()
                .unwrap_or_else(|| generate_slug(&name));
            if slug == segment {
                matched = Some(entry.path());
                cumulative_src = src_key;
                break;
            }
        }
        match matched {
            Some(p) => current = p,
            None => return (None, None),
        }
    }
    match current.file_name().map(|n| n.to_string_lossy().to_string()) {
        Some(n) => (Some(current.to_string_lossy().to_string()), Some(n)),
        None => (None, None),
    }
}

/// What the disk says about a folder's home page — the site root's, or a
/// subfolder's.
///
/// Three answers, not two, because a read failure is not a fact about the
/// folder: under the dataless fail-fast policy a `home: true` marker on an
/// evicted file cannot be proven, and folding that into "there is no home"
/// is what put the "Create home file" CTA over a real home.
pub enum HomeVerdict {
    /// A home file, named relative to the folder.
    File(String),
    /// Undecidable *right now*: no home is provable, and at least one markdown
    /// file in the folder has a marker that could not be read because its
    /// bytes are still in the cloud. The caller shows a waiting surface and
    /// asks again on arrival — it must never render this as "this folder has
    /// no home page".
    StillArriving,
    /// Provably no intentional home file. The CTA is correct here.
    None,
}

/// Source-truth detection of the ROOT home file on disk.
///
/// The build-derived `ArticleMap.pages[""]` is the fast path for the root home,
/// but it lags a freshly-written home: a Matters import drops a self-named
/// `<folder>.md` (with `home: true`) into an empty folder *before* the build
/// registers it in the page map. While the map is behind, the editor's root
/// resolve would fall to the "Create home file" CTA even though a real home
/// exists — and the editor never re-resolves the root surface, so it stays
/// stuck. This consults the filesystem (the editor's source of truth) so the
/// home opens immediately, independent of build timing.
///
/// Only UNAMBIGUOUS homes resolve here: a `home: true` marker, an index stem
/// (`index`/`readme`/…), or a self-named folder note (`<folder>.md`). The
/// priority-5 "first document alphabetically" fallback that
/// `detect_home_file_in_folder` uses is deliberately gated out — for a root
/// with content but no intentional home the editor must keep showing the CTA,
/// not silently adopt a random article as the home.
pub fn detect_root_home_source(root: &crate::vault_root::VaultRoot) -> HomeVerdict {
    detect_home_source(root.path(), root.name(), true)
}

/// [`detect_home_source_with`] with the production marker prober. `is_root`
/// selects the root's fallback gate (see the election below).
pub fn detect_home_source(dir: &std::path::Path, name: &str, is_root: bool) -> HomeVerdict {
    detect_home_source_with(dir, name, is_root, &home_marker_of)
}

/// [`detect_root_home_source`] with the marker probe injected, so the
/// still-in-the-cloud arm is testable — no test can create a dataless file.
///
/// The rule, stated once: **a proven answer always wins; only the absence of
/// one is undecidable.** An unreadable marker never overrides a home the
/// filesystem already proves (a root with `index.md` stays editable while some
/// unrelated note downloads), and it is never counted as a proven negative
/// either — with nothing provable and something unreadable, the honest answer
/// is [`HomeVerdict::StillArriving`].
///
/// It deliberately does NOT elect an unreadable file as the home, even when it
/// is the only candidate: that would resurrect the "silently adopt a random
/// article" failure the unambiguity gate below exists to prevent, and the
/// waiting surface costs the user only the seconds until the bytes land.
pub fn detect_home_source_with(
    dir: &std::path::Path,
    name: &str,
    is_root: bool,
    marker: &dyn Fn(&std::path::Path) -> Option<bool>,
) -> HomeVerdict {
    // Top-level files only — a folder's home lives directly in it.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HomeVerdict::None;
    };
    let entries: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let is_page = |f: &str| {
        std::path::Path::new(f)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| crate::build::scan::classify::is_page_source(&e.to_lowercase()))
    };
    // `home: true` markers win over filename rules (folder-rename resilient),
    // mirroring the renderer + build (`compute_home_overrides`). Unreadable
    // markers are held aside as UNKNOWN rather than dropped.
    let mut marked: Vec<String> = Vec::new();
    let mut unproven = false;
    for f in entries.iter().filter(|f| is_page(f)) {
        match marker(&dir.join(f)) {
            Some(true) => marked.push(f.clone()),
            Some(false) => {}
            None => unproven = true,
        }
    }
    let filenames: Vec<&str> = entries.iter().map(|s| s.as_str()).collect();
    let marked_refs: Vec<&str> = marked.iter().map(|s| s.as_str()).collect();
    let provable = moss_core::home::detect_home_file_in_folder_marked(&filenames, name, &marked_refs)
        .filter(|candidate| {
            // ROOT ONLY: gate out the priority-5 alphabetical fallback — the
            // root surface keeps its "create home page" CTA rather than
            // silently adopting a random article as the SITE home (the
            // standing 2026-06-22 decision; the tree applies the same gate in
            // `filesystem.rs`). Subfolders keep the full election, as the tree
            // and the build (`compute_home_file_winners`) do: until 2026-09-05
            // the resolver dissented here and showed the create-home CTA over
            // a folder whose one article the build already serves as its home.
            if !is_root {
                return true;
            }
            let stem = std::path::Path::new(candidate)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(candidate);
            marked_refs.iter().any(|m| m.eq_ignore_ascii_case(candidate))
                || moss_core::home::is_home_file(stem, name)
        });
    match (provable, unproven) {
        (Some(home), _) => HomeVerdict::File(home.to_string()),
        (None, true) => HomeVerdict::StillArriving,
        (None, false) => HomeVerdict::None,
    }
}

/// The root's home verdict as the `PageSource` the editor resolves to, or
/// `None` when resolution should carry on down the ordinary path.
///
/// [`HomeVerdict::StillArriving`] is the arm that has to exist: it is the only way
/// "I could not tell yet" reaches the frontend as something other than "there
/// is nothing here", which is the shape that renders the wrong pane.
pub fn root_page_source(
    home: HomeVerdict,
    folder_path: &std::path::Path,
) -> Option<super::links::PageSource> {
    let root = |source_path, source_pending| super::links::PageSource {
        source_path,
        is_dir: true,
        is_article: false,
        syndicated: vec![],
        takeover: None,
        source_pending,
    };
    match home {
        HomeVerdict::File(rel) => {
            let full = folder_path.join(&rel);
            full.exists()
                .then(|| root(Some(full.to_string_lossy().to_string()), false))
        }
        HomeVerdict::StillArriving => Some(root(None, true)),
        HomeVerdict::None => None,
    }
}

/// A real subfolder's `PageSource` from its own election on disk. The build's
/// article map lags a freshly-seeded folder (a folder template's home), so
/// the URL resolve reaches here with a `FolderHome` takeover for a folder
/// that may already have a home: the elected file is the source, a marker
/// still in the cloud is pending (the root's HomeVerdict rule, applied to
/// every folder), and only a folder with provably no home keeps the offer. Root
/// goes through [`root_page_source`], whose election gates the fallback.
pub fn folder_page_source(takeover: super::takeover::Takeover) -> super::links::PageSource {
    let dir = takeover.files.first().map(|f| std::path::PathBuf::from(&f.dir));
    // A translation (`index.zh-hans.md`) is not displaced by the
    // default-language home the same directory already holds: its recipe ran
    // the election itself and the URL is the translation's, not the home's.
    let translation = takeover.files.last().is_some_and(|f| {
        std::path::Path::new(&f.name).file_stem().and_then(|s| moss_core::home::lang_suffix(s.to_str()?)).is_some()
    });
    let home = dir
        .as_deref()
        .filter(|_| !translation)
        .and_then(|d| Some(detect_home_source(d, d.file_name()?.to_str()?, false)))
        .unwrap_or(HomeVerdict::None);
    let (source_path, source_pending, takeover) = match (home, dir) {
        (HomeVerdict::File(rel), Some(d)) => (Some(d.join(rel).to_string_lossy().to_string()), false, None),
        (HomeVerdict::StillArriving, _) => (None, true, None),
        _ => (None, false, Some(takeover)),
    };
    super::links::PageSource { source_path, is_dir: true, is_article: false, syndicated: vec![], takeover, source_pending }
}

#[cfg(test)]
#[path = "page_source_tests.rs"]
mod tests;
