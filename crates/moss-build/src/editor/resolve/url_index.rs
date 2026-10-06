//! `ArticleMapIndex` — a `UrlIndex` implementation over the inverted `ArticleMap`.
//! `FolderFacts` — what the last build LISTED under each folder, same map.
//!
//! Both are built once per loaded map. Lookups are O(1) hash operations.

use std::collections::HashMap;
use std::path::Path;
use moss_core::resolve::link_class::UrlIndex;
use crate::build::scan::article_map::ArticleMap;
use crate::editor::resolve::reference_resolver::FolderEmbedInfo;

/// A `UrlIndex` over a loaded `ArticleMap`. Built once per loaded map.
pub struct ArticleMapIndex {
    exact: std::collections::HashSet<String>,       // canonical URL keys (no leading slash)
    normalized: HashMap<String, Option<String>>,    // norm_key -> Some(/canonical/)|None(ambiguous)
    by_stem: HashMap<String, Option<String>>,       // lowercased filename stem -> Some(/url/)|None
    moved: HashMap<String, String>,                 // norm_key of a gone URL -> /canonical/ it lives at
    // Folder-index pages, keyed by their normalized SOURCE directory (not a
    // stem — a full path, so a bare leaf never matches a nested folder) ->
    // canonical /url/. A separate tier from `by_stem`, consulted only after
    // it misses, so a same-name page (which IS in `by_stem`) always wins and
    // `by_stem`'s own ambiguity-collapsing construction is never touched by
    // this. See `ArticleMap::folder_indexes`.
    folder_indexes: HashMap<String, String>,
}

fn norm(u: &str) -> String { u.trim_matches('/').to_lowercase() }
fn stem(source_path: &str) -> String {
    let file = source_path.rsplit('/').next().unwrap_or(source_path);
    file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).to_lowercase()
}

impl ArticleMapIndex {
    pub fn from_map(m: &ArticleMap) -> Self {
        let mut exact = std::collections::HashSet::new();
        let mut normalized: HashMap<String, Vec<String>> = HashMap::new();
        let mut stems: HashMap<String, Vec<String>> = HashMap::new();
        let mut add = |url: &str, source: &str| {
            let key = url.trim_matches('/').to_string();
            exact.insert(key.clone());
            let canonical = format!("/{}/", key).replace("//", "/");
            normalized.entry(norm(url)).or_default().push(canonical.clone());
            stems.entry(stem(source)).or_default().push(canonical);
        };
        for (url, source, _) in m.sourced_pages() { add(url, source); }
        // Synthesized index pages (index-less folders, generated term pages,
        // the term namespace roots) are deployed URLs with no source file: they
        // join the exact/normalized sets, never the stem set (nothing to open).
        for url in &m.generated {
            let key = url.trim_matches('/').to_string();
            exact.insert(key.clone());
            normalized.entry(norm(url)).or_default().push(format!("/{}/", key));
        }
        // A claimed term's generated URL is gone; the term lives at the claim.
        let moved = m.terms.iter()
            .filter_map(|(key, rec)| {
                let claim = rec.claimed_by.as_deref()?;
                Some((norm(key), format!("/{}", claim))) // pretty URL key: `about/ma/`, or `` for home
            })
            .collect();
        let collapse = |buckets: HashMap<String, Vec<String>>| {
            buckets.into_iter()
                .map(|(k, mut v)| { v.sort(); v.dedup(); (k, if v.len() == 1 { Some(v.remove(0)) } else { None }) })
                .collect()
        };
        // Folder-index pages, from the typed field the build populates
        // directly (never by eliminating `generated`/`terms`/`kinds` — see
        // the field doc). Keyed by the normalized full source directory, so
        // an exact match is required; there is no leaf/stem search over this
        // map, which is what keeps a bare `[[notes]]` from matching a nested
        // `obsidian/notes/` (out of scope in this version, on both sides).
        let folder_indexes = m
            .folder_indexes
            .iter()
            .map(|(dir, url)| (norm(dir), format!("/{}/", url.trim_matches('/'))))
            .collect();
        ArticleMapIndex {
            exact,
            normalized: collapse(normalized),
            by_stem: collapse(stems),
            moved,
            folder_indexes,
        }
    }
}

impl UrlIndex for ArticleMapIndex {
    fn lookup_exact(&self, url_path: &str) -> bool { self.exact.contains(url_path.trim_matches('/')) }
    fn lookup_normalized(&self, url_path: &str) -> Option<String> { self.normalized.get(&norm(url_path)).cloned().flatten() }
    fn lookup_moved(&self, url_path: &str) -> Option<String> { self.moved.get(&norm(url_path)).cloned() }
    // from_source ignored in v1 — stem lookup is global; relative-path refs are a future pass.
    //
    // A page always wins: `by_stem` is tried first and `folder_indexes` is
    // only ever consulted on a miss, so a same-named page and its folder can
    // never race — if the page exists, `by_stem`'s bucket for that stem is a
    // singleton and this returns at the first check.
    fn resolve_reference_to_url(&self, reference: &str, _from: &str) -> Option<String> {
        if let Some(found) = self.by_stem.get(&stem(reference)).cloned().flatten() {
            return Some(found);
        }
        self.folder_indexes.get(&reference.trim_matches('/').to_lowercase()).cloned()
    }
}

// ── FolderFacts — what the last build listed under a folder ──────────────

/// Pages a folder key holds, split by the two depths the build distinguishes.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildCounts {
    /// `depth:direct` — Articles AND sub-folder index pages directly under it.
    pub direct: u32,
    /// `depth:all` — Articles at any depth beneath it (folder pages excluded,
    /// `moss_core::page_kind`: `is_listable_at_depth_all` is Article-only).
    pub descendant: u32,
}

/// What the LAST BUILD listed under each folder, derived from the `ArticleMap`
/// — the build's own record of what it published. Never a filesystem walk:
/// that would reimplement the content graph and would not know slug overrides,
/// drafts, or `also_in`.
///
/// Built ONCE per resolve batch, beside `ArticleMapIndex`. Free functions over
/// the map would rescan it per target; this scans it once. Titles come from
/// the map too (`page_titles`), so no file is read.
///
/// FIDELITY — stated here because callers must not over-claim from it:
///  * `ArticleMap::pages` carries no frontmatter, so a `draft: true` FOLDER
///    page is counted even though the build drops it
///    (`build/folder_embed.rs`, the `is_listable` filter).
///  * `scope_default_tree` / `exclude_nav` are homepage-synthesised internals
///    (`moss_core::resolve::embed_renderer::folder_list`) and are not modelled.
///  * It is the last build's answer, not this second's.
///
/// The editor renders this as a bare number and makes no claim beyond it.
pub struct FolderFacts {
    /// pretty-URL folder key (`"awards/"`, or `""` for the root homepage) → counts
    counts: HashMap<String, ChildCounts>,
    /// folder key → project-relative source path of its index page
    index_source: HashMap<String, String>,
    /// on-disk source PARENT dir (project-relative, no trailing slash) → folder
    /// key, plus a lowercased twin. This is the slug-override bridge: a folder
    /// whose index is `評選/評選.md` but which publishes at `awards/` can only
    /// be found from the author-typed `評選` this way.
    by_source_dir: HashMap<String, String>,
    by_source_dir_ci: HashMap<String, String>,
    /// folder key → the index page's title, as the scan recorded it.
    titles: HashMap<String, String>,
}

/// Frontmatter listability, mirroring `build::types::ParsedDocument::is_listable`
/// (`draft != Some(true) && listed != Some(false)`; `slot_only` docs never enter
/// the map at all — `scan/article_map.rs` skips them before the insert).
fn fm_listable(fm: &HashMap<String, serde_json::Value>) -> bool {
    fm.get("draft").and_then(|v| v.as_bool()) != Some(true)
        && fm.get("listed").and_then(|v| v.as_bool()) != Some(false)
}

/// `also_in: [awards]` / `also_in: awards` → the folder ids it claims.
fn fm_also_in(fm: &HashMap<String, serde_json::Value>) -> Vec<String> {
    match fm.get("also_in") {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => {
            a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        }
        _ => vec![],
    }
}

/// Project-relative parent directory of a source path, no trailing slash.
/// `"評選/評選.md"` → `"評選"`; `"index.md"` → `""`.
fn parent_dir(source_path: &str) -> String {
    match source_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

/// A folder key's strict ancestors, root first, INCLUDING `""` and EXCLUDING
/// the doc's own key: `"awards/2024/x"` → `["", "awards/", "awards/2024/"]`.
///
/// Deliberately built by `split('/')` + `push_str` and never by byte-slicing a
/// URL. A folder key can be multibyte (`評選/` — the case this whole feature
/// exists for) and `&url[key.len()..]` panics when that byte offset lands
/// mid-UTF-8-sequence in an unrelated URL.
fn strict_ancestors(url: &str) -> Vec<String> {
    let body = url.trim_end_matches('/');
    let mut out = vec![String::new()];
    let mut acc = String::new();
    for seg in body.split('/') {
        acc.push_str(seg);
        acc.push('/');
        out.push(acc.clone());
    }
    out.pop(); // drop the doc's OWN key
    out
}

impl FolderFacts {
    /// One O(map) pass for every folder in the site.
    pub fn from_map(m: &ArticleMap) -> Self {
        let mut counts: HashMap<String, ChildCounts> = HashMap::new();
        let mut index_source = HashMap::new();
        let mut by_source_dir = HashMap::new();
        let mut by_source_dir_ci = HashMap::new();

        for (url, info) in &m.articles {
            if !fm_listable(&info.frontmatter) {
                continue;
            }
            account(&mut counts, url, true, &fm_also_in(&info.frontmatter));
        }
        for (url, src) in &m.pages {
            index_source.insert(url.clone(), src.clone());
            let dir = parent_dir(src);
            by_source_dir_ci.insert(dir.to_lowercase(), url.clone());
            by_source_dir.insert(dir, url.clone());
            // A folder page is listable at depth:direct only, so it is
            // accounted as a non-article child of its own parent.
            account(&mut counts, url, false, &[]);
        }

        FolderFacts {
            counts,
            index_source,
            by_source_dir,
            by_source_dir_ci,
            titles: m.page_titles.clone(),
        }
    }

    /// The folder key `target_path` names, or `None` when the map does not know
    /// this folder. Written to be independent of whether the classifier hands
    /// back URL space or on-disk space — see the four steps.
    fn key_for(&self, target_path: &str) -> Option<String> {
        let t = target_path.trim_matches('/');
        if t.is_empty() {
            // The root homepage: always a known key once anything was built.
            return Some(String::new());
        }
        let known = |k: String| -> Option<String> {
            if self.index_source.contains_key(&k) || self.counts.contains_key(&k) {
                Some(k)
            } else {
                None
            }
        };
        // 1. The author already typed URL space (or the folder is not overridden).
        if let Some(k) = known(format!("{}/", t)) {
            return Some(k);
        }
        // 2. Slug/punctuation normalisation — the same slugifier the folder
        //    index uses to reach the build's folder-index URL set.
        let slugged = moss_core::slug::slugify_path_segments(t);
        if let Some(k) = known(format!("{}/", slugged)) {
            return Some(k);
        }
        // 3. Reverse by on-disk source dir: `pages["awards/"] == "評選/評選.md"`
        //    → parent `評選` → key `awards/`. The slug-override case.
        if let Some(k) = self.by_source_dir.get(t) {
            return Some(k.clone());
        }
        self.by_source_dir_ci.get(&t.to_lowercase()).cloned()
    }

    /// Folder-listing facts for one embed target, or `None` when the map does
    /// not know this folder (the card then shows no count at all — "found and
    /// zero" and "unknown" must be different states on screen).
    pub fn lookup(&self, target_path: &str) -> Option<FolderEmbedInfo> {
        let key = self.key_for(target_path)?;
        let counts = self.counts.get(&key).copied().unwrap_or_default();
        Some(FolderEmbedInfo {
            title: self.titles.get(&key).filter(|t| !t.is_empty()).cloned(),
            direct_child_count: counts.direct,
            descendant_count: counts.descendant,
        })
    }

    /// The project-relative source file of this folder's index page, or `None`
    /// when the folder publishes no index source.
    ///
    /// This is what "following" a folder embed opens, and it is deliberately a
    /// FILE: the author-typed target is a URL, not a path, so joining it to the
    /// project root is wrong the moment a slug override is in play — a folder
    /// whose index is `評選/評選.md` publishes at `awards/`, and `<root>/awards`
    /// names nothing on disk. `key_for` already crosses that bridge for the
    /// counts; this reuses it so the card and the click agree by construction.
    ///
    /// `None` for a SYNTHESIZED folder index (a folder full of articles with no
    /// index source of its own): `ArticleMap::pages` only records a page that
    /// had a source, so there is nothing to open, and the caller renders no
    /// follow affordance rather than a broken one.
    pub fn index_source_for(&self, target_path: &str) -> Option<&str> {
        let key = self.key_for(target_path)?;
        let src = self.index_source.get(&key)?;
        // Defence in depth: the map is a file on disk, so its paths
        // are untrusted. Only a project-relative path with no `..` is handed
        // back — this one is joined to the root and opened in the editor.
        let rel = Path::new(src);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return None;
        }
        Some(src)
    }

}

/// Count one document into every folder it is listed under.
///
/// `is_article` distinguishes the two depths: `depth:direct` lists Articles AND
/// Folder pages, `depth:all` lists Articles only.
fn account(
    counts: &mut HashMap<String, ChildCounts>,
    url: &str,
    is_article: bool,
    also_in: &[String],
) {
    if url.is_empty() {
        return; // the root homepage is nobody's child
    }
    let ancestors = strict_ancestors(url);
    let parent = ancestors.last().cloned().unwrap_or_default();
    for a in &ancestors {
        let e = counts.entry(a.clone()).or_default();
        if is_article {
            e.descendant += 1;
        }
        if *a == parent {
            e.direct += 1;
        }
    }
    for f in also_in {
        let trimmed = f.trim_matches('/');
        let key = if trimmed.is_empty() {
            String::new()
        } else {
            format!("{}/", trimmed)
        };
        if ancestors.contains(&key) {
            continue; // already counted by prefix — the build's membership is an OR
        }
        // `folder_embed.rs` reaches the depth check through
        // `strip_prefix(..).is_none_or(..)`, so an `also_in` doc that does not
        // live under the folder is a DIRECT child by definition.
        let e = counts.entry(key).or_default();
        e.direct += 1;
        if is_article {
            e.descendant += 1;
        }
    }
}

#[cfg(test)]
#[path = "url_index_tests.rs"]
mod tests;
