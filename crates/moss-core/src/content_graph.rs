//! In-memory index of content files for fuzzy path resolution.
//!
//! `ContentGraph` is the read-only query structure built by `ContentGraphBuilder`.
//! It supports Obsidian-style fuzzy path resolution: exact path, filename-only,
//! folder notes, and ambiguity tiebreaking by longest common directory prefix.
//!
//! Pure Rust, zero I/O.

use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

use crate::page_kind::PAGE_EXTENSIONS;
use crate::path_ext::path_extension;

// ---------------------------------------------------------------------------
// Path normalization helpers
// ---------------------------------------------------------------------------

/// NFC-normalize and lowercase a single path component.
fn normalize_component(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase()
}

/// NFC-normalize and lowercase every component of a `/`-separated path.
/// Also normalises backslashes to forward slashes and collapses runs of
/// separators.
///
/// This is the key the graph matches paths by, so anything that has to agree
/// with the resolver on whether two spellings name the same file (the
/// completion ranker, a reader that lists folders itself) folds with it.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|c| !c.is_empty())
        .map(normalize_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// Extract the filename stem (no extension) from a normalized path.
fn filename_stem(normalized: &str) -> &str {
    let filename = normalized.rsplit('/').next().unwrap_or(normalized);
    match filename.rsplit_once('.') {
        // Guard against `pos == 0` (e.g. ".gitignore"): treat the whole name
        // as the stem rather than returning an empty stem.
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => filename,
    }
}


/// `target` written from the folder `base_dir`, with `.` and `..` resolved and
/// separators normalized, in the letter case written; `None` when it climbs
/// above the site root.
pub fn join_written(base_dir: &str, target: &str) -> Option<String> {
    let joined = format!("{base_dir}/{target}").replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    Some(parts.join("/"))
}

/// Whether two paths are the same text once both are NFC-normalized.
fn same_nfc(a: &str, b: &str) -> bool {
    a.nfc().eq(b.nfc())
}

/// Return the directory prefix components of a path as a Vec.
/// `pub(crate)` — shared with `link_completions` (see `normalize_path`).
pub(crate) fn dir_components(path: &str) -> Vec<&str> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 1 {
        vec![]
    } else {
        parts[..parts.len() - 1].to_vec()
    }
}

/// Count the length of the longest common prefix between two component lists.
/// `pub(crate)` — shared with `link_completions` (see `normalize_path`).
pub(crate) fn common_prefix_len(a: &[&str], b: &[&str]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Score a candidate's page-ness against a bare (extensionless) reference.
///
/// A reference that names no extension can't express asset intent, so a
/// stem collision between a page and a same-named asset — `示例圖.md` next
/// to `示例圖.jpg` — must not fall through to the alphabetical tiebreaker,
/// where an asset's extension can sort ahead of the page's for no reason a reader
/// would recognize (`[[示例圖]]` resolved to the plate, not the page). A
/// reference that carries an extension only ever reaches files with that
/// extension, so this term stays 0 for every candidate in that case.
///
/// The second term ranks the page extensions in [`PAGE_EXTENSIONS`] order,
/// `.md` first; [`ContentGraph::nearest`] applies it after nearness, so it
/// only settles pages that are otherwise exactly as near.
fn page_preference_score(ref_ext: Option<&str>, candidate: &str) -> (u8, usize) {
    let pos = path_extension(candidate).and_then(|ext| PAGE_EXTENSIONS.iter().position(|p| *p == ext));
    match pos {
        Some(pos) if ref_ext.is_none() => (1, PAGE_EXTENSIONS.len() - pos),
        _ => (0, 0),
    }
}

/// Score a candidate path's language-tree alignment with the source's
/// language-tree prefix.
///
/// Both inputs should be normalized (lowercase).  Returns 1 when the candidate
/// is in the same language tree as the source (either both share the same
/// language prefix, or both are tree-less/root-level), 0 otherwise.
///
/// This is used as a tiebreaker in [`ContentGraph::resolve_path`] so that
/// `![[footer]]` from `zh-hans/about.md` picks `zh-hans/footer.md` over a
/// root-level `footer.md`, and conversely root sources prefer root candidates.
fn lang_tree_match(candidate: &str, from_lang: Option<&str>) -> u8 {
    let cand_lang = crate::home::lang_tree_prefix(candidate);
    match (from_lang, cand_lang) {
        (Some(f), Some(c)) if f.eq_ignore_ascii_case(c) => 1,
        (None, None) => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Slug generation
// ---------------------------------------------------------------------------

/// Generate a URL slug from a relative file path.
///
/// Strips the file extension, normalizes separators to `/`, lowercases, and
/// sanitizes each segment: drops ASCII punctuation that is neither alphanumeric
/// nor a word separator, normalizes spaces/underscores to hyphens, collapses
/// runs of hyphens, trims edges. Non-ASCII characters (CJK, Cyrillic, Greek,
/// etc.) pass through unchanged.
///
/// Examples:
/// - `"posts/Hello World.md"` -> `"posts/hello-world"`
/// - `"guides/Setup.md"` -> `"guides/setup"`
/// - `"news/Hello, and Goodbye on NewsWire.md"`
///   -> `"news/hello-and-goodbye-on-newswire"`
/// - `"posts/Hello (World)!.md"` -> `"posts/hello-world"`
/// - `"posts/foo--bar.md"` -> `"posts/foo-bar"`
/// - `"image.png"` -> `"image"`
/// - `"视频/视频.md"` -> `"视频/视频"`  (non-ASCII passes through)
pub fn generate_slug(relative_path: &str) -> String {
    // Compose (NFC) first: macOS/iCloud names are decomposed and a combining mark is not alphanumeric.
    let normalized = relative_path.nfc().collect::<String>().replace('\\', "/");

    // Strip extension only when the last `.` lives inside the trailing
    // segment AND has at least one character before it. This preserves the
    // original `dot_pos > last_slash` semantics, including the dotfile case
    // (`.gitignore`, `.bashrc`) where the leading dot must be kept as part
    // of the stem rather than yielding an empty string.
    let last_segment = normalized.rsplit('/').next().unwrap_or(&normalized);
    let stem_in_segment = match last_segment.rsplit_once('.') {
        Some((stem, _ext)) if !stem.is_empty() => Some(stem),
        _ => None,
    };
    let prefix = match normalized.rsplit_once('/') {
        Some((p, _)) => Some(p),
        None => None,
    };
    let without_ext: String = match (prefix, stem_in_segment) {
        (Some(p), Some(stem)) => format!("{p}/{stem}"),
        (None, Some(stem)) => stem.to_string(),
        _ => normalized.clone(),
    };

    // Sanitize each path segment independently so hyphen-collapse + edge-trim
    // operate within a segment without touching the path separators.
    without_ext
        .split('/')
        .map(sanitize_slug_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Sanitize a single path segment: drop ASCII punctuation, normalize
/// space/underscore to hyphen, collapse runs of hyphens, trim edges.
fn sanitize_slug_segment(segment: &str) -> String {
    let lowered = segment.to_lowercase();

    let mut buf = String::with_capacity(lowered.len());
    for c in lowered.chars() {
        if c.is_alphanumeric() {
            buf.push(c);
        } else if c == ' ' || c == '-' || c == '_' {
            buf.push('-');
        }
        // else: drop ASCII punctuation (',', '.', '!', '(', ')', etc.) and
        // control characters.
    }

    // Collapse consecutive hyphens, then trim leading/trailing.
    let mut collapsed = String::with_capacity(buf.len());
    let mut prev_hyphen = false;
    for c in buf.chars() {
        if c == '-' {
            if !prev_hyphen {
                collapsed.push('-');
            }
            prev_hyphen = true;
        } else {
            collapsed.push(c);
            prev_hyphen = false;
        }
    }
    collapsed.trim_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// ContentGraph — the immutable, queryable index
// ---------------------------------------------------------------------------

/// How [`ContentGraph::resolve_path`] reached the file it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMatch {
    /// The target is the file's path, written from the page's folder or,
    /// with a leading `/`, from the site root. `exact_case` is false when the
    /// letter case written differs from the file's.
    Written { exact_case: bool },
    /// The target is the file's path from the site root, written with a
    /// folder but without a leading `/` on a page that is not at the root.
    FromRoot { exact_case: bool },
    /// Found by name or partial path, with a page extension added, or as a
    /// folder's note.
    Searched,
}

/// The answer of [`ContentGraph::resolve_path_with_ties`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathResolution {
    /// The file the target names.
    pub path: String,
    /// The other files exactly as near to the page as `path` (same page
    /// preference, language tree and shared directory depth) that lost only
    /// to the alphabetical tiebreak. The editor reports a reference as
    /// ambiguous exactly when this is non-empty; the build links `path`.
    pub ties: Vec<String>,
    /// How `path` was reached.
    pub matched: PathMatch,
}

/// An in-memory index of all content files.
///
/// Created via [`ContentGraphBuilder::build`]. All lookups are
/// case-insensitive (NFC-normalized, lowercased).
#[derive(Debug, Clone)]
pub struct ContentGraph {
    /// All file paths (normalized), in insertion order.
    files: Vec<String>,

    /// Normalized filename stem (no extension, lowercase) -> list of file indices.
    filename_index: HashMap<String, Vec<usize>>,

    /// Normalized full path -> file index.
    path_index: HashMap<String, usize>,

    /// Normalized full path -> slug.
    slug_map: HashMap<String, String>,

    /// The build's source-directory → URL-slug overrides, so this graph can
    /// answer "what URL is this file served at" (see [`Self::pinned_url`]) and
    /// not just "which file does this reference mean". Empty unless the host
    /// installed them via [`Self::with_output_overrides`]; an empty map still
    /// yields base slugification, which is what every case-fold bug needed.
    output_overrides: HashMap<String, String>,

    /// Directories the scan marked as getting an auto-generated folder-index
    /// page (no `index.md` of their own): normalized key -> original-case
    /// directory. Consulted only after both file-backed folder-note checks
    /// miss, inside `resolve_path`'s `folder_note` closure — so a `[[Folder]]`
    /// wikilink can resolve to that folder's SYNTHETIC index, the same page
    /// the renderer's auto-index loop emits. Registered by the host (once,
    /// alongside the scan) via [`ContentGraphBuilder::register_auto_index_dir`]
    /// from the same source the renderer itself reads its folder set from —
    /// so this graph never resolves a folder the renderer does not actually
    /// generate a page for. Empty unless the host registers anything, which
    /// keeps every existing caller (tests included) unaffected.
    auto_index_dirs: HashMap<String, String>,

    /// Every spelling, sorted, of a path held by files differing only in letter
    /// case (a case-sensitive disk); `files` keeps one of them, and a path
    /// written from the page or the root gets the twin it spells exactly.
    case_twins: HashMap<String, Vec<String>>,

    /// Normalized names of the folders at the site root that hold a file.
    top_dirs: HashSet<String>,
}

impl ContentGraph {
    /// Install the build's source-directory → URL-slug overrides, making this
    /// graph the authority on emitted URLs as well as on resolution.
    ///
    /// The host calls this once, right after the overrides are computed
    /// (`build_page_map` in moss-build), and every emitter downstream reads the
    /// answer off the same graph it already holds. Sites that don't map any
    /// directory still benefit: base slugification (`MIRROR/` → `mirror/`) runs
    /// with an empty map.
    pub fn with_output_overrides(mut self, overrides: HashMap<String, String>) -> Self {
        self.output_overrides = overrides;
        self
    }

    /// **The one URL a resolved reference may be emitted as.**
    ///
    /// `root_rel` is a source path this graph resolved (via
    /// [`Self::resolve_path`] or the asset engine). The result is the pinned,
    /// root-absolute, case-canonical URL the site serves that file at — see
    /// [`crate::resolve::output_url::pinned_url`] for the properties this
    /// guarantees.
    ///
    /// **Never** re-derive an emitted URL from a folder name, a referencing
    /// page's depth, or a case-insensitive retry: those are the four-times-
    /// recurring bug class this method exists to end. A
    /// reference that does not resolve gets a `Diagnostic`, not a guessed path.
    pub fn pinned_url(&self, root_rel: &str) -> String {
        crate::resolve::output_url::pinned_url(root_rel, &self.output_overrides)
    }

    /// **The one function that decides which site file a target names.**
    ///
    /// Every reference form — wikilinks `[[x]]`, standard markdown links
    /// `[t](x)`, image refs `![](x)`, embeds `![[x]]`, shortcode and
    /// frontmatter asset paths, the editor's classifier and the rename,
    /// delete and scan tools — MUST get its file from this function. The only
    /// thing layered on top is [`crate::resolve::asset_class::resolve_file_target`],
    /// which turns the [`PathMatch`] this function reports into a label and
    /// reports files exactly as near as the pick as ambiguous; it matches
    /// nothing itself. See the resolve pipeline in
    /// [`crate::resolve::resolve_content`] for the per-syntax call sites.
    ///
    /// A leading `/` starts at the site root (the exact path is tried first
    /// like any other), and only files the graph holds are ever returned, so
    /// nothing outside the site can be named.
    ///
    /// Downstream code (the compiler's URL-prettifier, for instance)
    /// receives already-resolved hrefs and MUST NOT reimplement any
    /// part of this chain; a parallel resolver diverges on folder notes.
    ///
    /// Resolution chain (first match wins):
    /// 0. The target written from the referencing page's folder (`.` and `..`
    ///    resolved), as an exact path, plus a page extension, or, for a slashed target
    ///    without an extension, the file of that name in that folder. A
    ///    leading `/` skips this step, and so does a target starting with a
    ///    language folder when the page is itself in a language tree.
    /// 1. The path from the site root (`.` and `..` resolved)
    /// 2. Exact + a page extension (`.md`, `.markdown`, `.mdown`, `.mkd`)
    /// 3. Filename match (case-insensitive, without extension); a target
    ///    that names an extension only matches files with that extension
    /// 4. Filename + a page extension
    /// 5. Folder note: `reference/index.md` or `reference/<reference>.md`
    ///    (`.md` only, as the build elects a folder's home),
    ///    else — if the host registered it via [`ContentGraphBuilder::register_auto_index_dir`] —
    ///    the directory's synthetic `<dir>/index.md`, for a folder with no
    ///    note of its own
    ///
    /// Ambiguity tiebreakers, applied in order:
    /// only when the reference is bare (no extension), a page candidate
    /// wins over a same-stem asset (`[[示例圖]]` prefers the page over the
    /// sibling `.jpg` — a bare reference can't express asset intent, so a
    /// caller that wants the asset must name its extension); candidates in
    /// the same language tree as the source are preferred next;
    /// then longest common directory prefix with `from_path`; then, for a bare
    /// reference, `.md` before `.markdown`, `.mdown`, `.mkd`; then alphabetical
    /// by normalized path (so results are independent of registration order
    /// when all earlier keys tie).
    ///
    /// A target that matches nothing as written is tried once more
    /// percent-decoded: Obsidian writes `my%20note.md` for a real
    /// "my note.md". A file whose name literally contains `%20` still wins,
    /// because the text as written goes first.
    pub fn resolve_path(&self, reference: &str, from_path: &str) -> Option<String> {
        self.resolve_path_with_ties(reference, from_path).map(|r| r.path)
    }

    /// [`resolve_path`](Self::resolve_path), also reporting how the file was
    /// reached and which other files were exactly as near to `from_path` as
    /// the winner.
    pub fn resolve_path_with_ties(&self, reference: &str, from_path: &str) -> Option<PathResolution> {
        let resolve = |text: &str| {
            let mut ties = Vec::new();
            let mut matched = PathMatch::Searched;
            let path = self.resolve_ranked(text, from_path, &mut ties, &mut matched)?;
            Some(PathResolution { path, ties, matched })
        };
        resolve(reference).or_else(|| {
            crate::resolve::fuzzy_path::percent_decoded_fallback(reference).and_then(|d| resolve(&d))
        })
    }

    /// The page at the normalized path `key` plus a page extension, `.md` first.
    fn page_at(&self, key: &str) -> Option<String> {
        PAGE_EXTENSIONS.iter().find_map(|ext| self.path_index.get(&format!("{key}.{ext}"))).map(|&i| self.files[i].clone())
    }

    /// The file at index `idx` as `written` spells it, and whether the case
    /// matches: of case twins, the one spelled exactly, else the kept one.
    fn spelled_as(&self, idx: usize, written: &str) -> (String, bool) {
        let twins = self.case_twins.get(&normalize_path(written)).into_iter().flatten();
        match twins.chain([&self.files[idx]]).find(|t| same_nfc(t, written)) {
            Some(exact) => (exact.clone(), true),
            None => (self.files[idx].clone(), false),
        }
    }

    /// The nearest of several same-named files to the page `norm_from`: a
    /// page over a same-stem asset for a bare reference, then the same language tree, then the longest shared
    /// directory prefix, then `.md` over the other page extensions, then alphabetical by path. Pushes every other
    /// candidate that ties with the winner on all but the alphabetical key
    /// onto `ties`.
    fn nearest(
        &self,
        candidates: &[usize],
        ref_ext: Option<&str>,
        norm_from: &str,
        from_lang: Option<&str>,
        ties: &mut Vec<String>,
    ) -> Option<usize> {
        let from_dirs = dir_components(norm_from);
        // self.files stores original (pre-normalized) paths for filesystem
        // fidelity; re-normalize to compare against norm_from.
        let rank = |idx: usize| {
            let normalized = normalize_path(&self.files[idx]);
            let tree_match = lang_tree_match(&normalized, from_lang);
            let (page_score, extension_rank) = page_preference_score(ref_ext, &normalized);
            let near = common_prefix_len(&dir_components(&normalized), &from_dirs);
            ((page_score, tree_match, near, extension_rank), normalized)
        };
        let best = candidates
            .iter()
            .copied()
            .max_by_key(|&idx| {
                let (key, normalized) = rank(idx);
                (key, std::cmp::Reverse(normalized))
            })?;
        let best_key = rank(best).0;
        ties.extend(
            candidates
                .iter()
                .copied()
                .filter(|&idx| idx != best && rank(idx).0 == best_key)
                .map(|idx| self.files[idx].clone()),
        );
        Some(best)
    }

    /// The file `reference` names when written from the folder of `from_path`
    /// (`.` and `..` resolved), if the graph holds it: the exact path, then the
    /// path plus a page extension, then, for a target without an extension, the file of
    /// that name in that folder (page over same-named asset, as for a bare name).
    fn named_from_page(
        &self,
        reference: &str,
        from_path: &str,
        ref_ext: Option<&str>,
        from_lang: Option<&str>,
        ties: &mut Vec<String>,
        matched: &mut PathMatch,
    ) -> Option<String> {
        let from = from_path.replace('\\', "/");
        let written = join_written(crate::resolve::parent_dir(&from), reference)?;
        let joined = normalize_path(&written);
        if let Some(&idx) = self.path_index.get(&joined) {
            let (path, exact_case) = self.spelled_as(idx, &written);
            *matched = PathMatch::Written { exact_case };
            return Some(path);
        }
        if let Some(page) = self.page_at(&joined) {
            return Some(page);
        }
        let norm_ref = normalize_path(reference);
        let parts: Vec<&str> = joined.split('/').collect();
        let (name, dir) = parts.split_last()?;
        if ref_ext.is_some() || !norm_ref.contains('/') {
            return None;
        }
        let candidates: Vec<usize> = self
            .filename_index
            .get(&normalize_component(name))?
            .iter()
            .copied()
            .filter(|&i| dir_components(&normalize_path(&self.files[i])).as_slice() == dir)
            .collect();
        let idx = self.nearest(&candidates, None, &normalize_path(from_path), from_lang, ties)?;
        Some(self.files[idx].clone())
    }

    fn resolve_ranked(
        &self,
        reference: &str,
        from_path: &str,
        ties: &mut Vec<String>,
        matched: &mut PathMatch,
    ) -> Option<String> {
        let norm_ref = normalize_path(reference);
        let norm_from = normalize_path(from_path);
        let ref_ext = path_extension(&norm_ref);

        // Only slashes (`/`, `//`) name the site root itself. The answer is
        // "/", not "": `normalize_children` (frontmatter_union.rs) reads an
        // empty `children:` value as "no listing", while the folder-embed code
        // already maps "/" to the root folder and `pinned_url` serves both at
        // "/". An empty reference (`[[]]`) names nothing.
        if !reference.is_empty() && norm_ref.is_empty() {
            return Some("/".to_string());
        }

        // Language-tree prefix of the source file, if any.
        // E.g. "zh-hans/about.md" -> Some("zh-hans").  Used to prefer
        // same-language-tree candidates when the reference is bare (no slash).
        let from_lang = crate::home::lang_tree_prefix(&norm_from);

        // 0. A target written from the page's folder names that file when the
        // file exists, whatever else the name could also match. `../c/p.png`
        // from `a/b/` is `a/c/p.png`, not the nearer `a/b/c/p.png` the search
        // below would otherwise prefer. A leading `/` starts at the site root
        // instead and skips this step. So does a target naming a language tree
        // the site has, from a page inside one (`en/x` from `zh-hans/` is the
        // English page, never `zh-hans/en/x`); any other folder called `uk` or
        // `id` is an ordinary folder.
        let names_other_tree = from_lang.is_some()
            && crate::home::lang_tree_prefix(&norm_ref).is_some_and(|tree| self.top_dirs.contains(tree));
        if !reference.starts_with(['/', '\\']) && !names_other_tree {
            if let Some(found) =
                self.named_from_page(reference, from_path, ref_ext.as_deref(), from_lang, ties, matched)
            {
                return Some(found);
            }
        }

        // 1. The path from the site root, `.` and `..` resolved.
        let from_root = join_written("", reference).unwrap_or_default();
        if let Some(&idx) = self.path_index.get(&normalize_path(&from_root)) {
            let (path, exact_case) = self.spelled_as(idx, &from_root);
            // A bare name spells out no folder, so finding it at the root is
            // a search like any other.
            *matched = if reference.starts_with(['/', '\\']) {
                PathMatch::Written { exact_case }
            } else if from_root.contains('/') {
                PathMatch::FromRoot { exact_case }
            } else {
                PathMatch::Searched
            };
            return Some(path);
        }

        // 1b. From a page inside a language tree, the same path inside that
        // tree, before step 2 tries the site root: `[[work/spring-show]]` from
        // `zh-hans/blog/post.md` is `zh-hans/work/spring-show.md` when that
        // exists. A reference naming a language tree itself is never re-scoped.
        if let Some(lang) = from_lang.filter(|_| crate::home::lang_tree_prefix(&norm_ref).is_none()) {
            let scoped = format!("{lang}/{norm_ref}");
            if let Some(found) = self.path_index.get(&scoped).map(|&i| self.files[i].clone()).or_else(|| self.page_at(&scoped)) {
                return Some(found);
            }
        }

        // 2. Exact + a page extension
        if let Some(page) = self.page_at(&norm_ref) {
            return Some(page);
        }

        // 2b. Suffix match for partial paths (Obsidian shortest-path resolution).
        // e.g. "游记/index.md" matches "文字/游记/index.md"
        // Also handles vault-root prefix: "山居/交互实验/index.md" → try
        // progressively shorter sub-paths until a match is found.
        if norm_ref.contains('/') {
            let parts: Vec<&str> = norm_ref.split('/').collect();
            // start=0 tries the full path as suffix; start=1.. strips leading components
            for start in 0..parts.len().saturating_sub(1) {
                let subpath = parts[start..].join("/");
                if !subpath.contains('/') {
                    break; // Single component — handled by filename stem match below
                }

                // Try exact match on the sub-path
                if self.path_index.contains_key(&subpath) {
                    return Some(self.files[self.path_index[&subpath]].clone());
                }
                if let Some(page) = self.page_at(&subpath) {
                    return Some(page);
                }

                // Try suffix match (sub-path as suffix of a longer graph path)
                let suffix = format!("/{}", subpath);
                let candidates: Vec<usize> = self.files.iter().enumerate()
                    .filter(|(_, f)| normalize_path(f).ends_with(&suffix))
                    .map(|(i, _)| i)
                    .collect();
                if let Some(idx) = self.nearest(&candidates, ref_ext.as_deref(), &norm_from, from_lang, ties) {
                    return Some(self.files[idx].clone());
                }
            }
        }

        // 3/4. Filename match (stem, case-insensitive)
        // Skip stem matching when the reference is a multi-component path with an
        // index stem — falling back to just "index" would match every index.md in
        // the vault and return an arbitrary wrong result.
        let ref_stem = normalize_component(filename_stem(&norm_ref));
        let skip_stem = norm_ref.contains('/') && crate::home::is_index_stem(&ref_stem);
        // A target that names an extension only names a file with it: `chart.png`
        // is never the page `chart.md`, nor `logo.png` the file `logo.svg`.
        if !skip_stem {
            if let Some(candidates) = self.filename_index.get(&ref_stem) {
                let candidates: Vec<usize> = candidates
                    .iter()
                    .copied()
                    .filter(|&i| ref_ext.is_none() || path_extension(&self.files[i]) == ref_ext)
                    .collect();
                if let Some(idx) = self.nearest(&candidates, ref_ext.as_deref(), &norm_from, from_lang, ties) {
                    return Some(self.files[idx].clone());
                }
            }
        }

        // 5. Folder note: the page the build elects as the folder's home by
        // name (`home::detect_home_file_in_folder`): an index stem in priority
        // order, then the self-named note, as `.md` only. The build publishes
        // an `index.mdown` as an ordinary page, never as the folder's.
        let folder_note = |base: &str| -> Option<String> {
            let leaf = base.rsplit('/').next().unwrap_or(base);
            let home = crate::home::INDEX_STEMS.iter().chain([&leaf]).find_map(|stem| self.path_index.get(&format!("{base}/{stem}.md")));
            if let Some(&idx) = home {
                return Some(self.files[idx].clone());
            }
            // No file backs this folder: the synthetic index page the
            // renderer generates for it, if the scan registered one, at
            // `<original-case dir>/index.md` so headings keep the author's case.
            self.auto_index_dirs
                .get(base)
                .map(|orig_case_dir| format!("{orig_case_dir}/index.md"))
        };

        // 5a. Language-tree-scoped folder note: a bare folder reference like
        // `docs/` written inside a `zh-hans/` page should resolve to the
        // same-language `zh-hans/docs/index.md`, not the root `docs/index.md`.
        // Mirrors the bare-name language scoping at step 1b. Skipped when the
        // reference already names a language tree explicitly (handled below).
        if let Some(lang) = from_lang {
            if crate::home::lang_tree_prefix(&norm_ref).is_none() {
                let scoped = format!("{}/{}", lang, norm_ref);
                if let Some(found) = folder_note(&scoped) {
                    return Some(found);
                }
            }
        }

        // 5b. Folder note in the reference's own namespace (root fallback).
        if let Some(found) = folder_note(&norm_ref) {
            return Some(found);
        }

        None
    }

    /// Return the slug for the given path, if registered.
    pub fn get_slug(&self, path: &str) -> Option<&str> {
        let norm = normalize_path(path);
        self.slug_map.get(&norm).map(|s| s.as_str())
    }

    /// `true` iff `path` names a real, registered file — O(1) via the same
    /// normalized-path index `resolve_path`'s exact-match tier uses, rather
    /// than a linear scan of [`all_files`](Self::all_files). Distinguishes a
    /// real file from a synthetic path `resolve_path` only ever manufactures
    /// (the auto-index folder-note fallback's `<dir>/index.md`), which by
    /// construction is never itself registered.
    pub fn contains_path(&self, path: &str) -> bool {
        self.path_index.contains_key(&normalize_path(path))
    }

    /// All file paths in insertion order.
    pub fn all_files(&self) -> &[String] {
        &self.files
    }

    /// Build a graph from a bare list of file paths (no slugs).
    ///
    /// Each file is registered with an empty slug. Useful for tests and for
    /// lightweight index construction in integration scenarios where only asset
    /// lookup (not slug routing) is needed.
    pub fn from_paths(paths: &[&str]) -> ContentGraph {
        let mut b = ContentGraphBuilder::new();
        for &p in paths {
            b.add_file(p, "");
        }
        b.build()
    }
}

// ---------------------------------------------------------------------------
// ContentGraphBuilder
// ---------------------------------------------------------------------------

/// Incrementally builds a [`ContentGraph`].
///
/// Call `add_file` as content is scanned, then `build()` to obtain the
/// immutable graph.
#[derive(Debug, Default)]
pub struct ContentGraphBuilder {
    files: Vec<String>,
    filename_index: HashMap<String, Vec<usize>>,
    path_index: HashMap<String, usize>,
    slug_map: HashMap<String, String>,
    auto_index_dirs: HashMap<String, String>,
    case_twins: HashMap<String, Vec<String>>,
    top_dirs: HashSet<String>,
}

impl ContentGraphBuilder {
    /// Create a new, empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a content file.
    ///
    /// `relative_path` is the path relative to the source root (e.g.
    /// `"posts/hello.md"`). `slug` is the URL slug for this file.
    pub fn add_file(&mut self, relative_path: &str, slug: &str) {
        let norm = normalize_path(relative_path);

        // One entry per normalized path. Twins that differ only in letter case
        // (possible on a case-sensitive disk) keep the byte-smaller spelling,
        // so the pick never depends on the order a directory listing returned;
        // every spelling is remembered for a target that writes one exactly.
        if let Some(&idx) = self.path_index.get(&norm) {
            let twins = self.case_twins.entry(norm.clone()).or_insert_with(|| vec![self.files[idx].clone()]);
            if !twins.iter().any(|t| t == relative_path) {
                twins.push(relative_path.to_string());
                twins.sort();
            }
            if relative_path < self.files[idx].as_str() {
                self.files[idx] = relative_path.to_string();
                self.slug_map.insert(norm, slug.to_owned());
            }
            return;
        }

        let idx = self.files.len();
        if let Some((top, _)) = norm.split_once('/') {
            self.top_dirs.insert(top.to_owned());
        }

        // Build filename stem index
        let stem = filename_stem(&norm).to_owned();
        self.filename_index.entry(stem).or_default().push(idx);

        // Build path index
        self.path_index.insert(norm.clone(), idx);

        // Slug map
        self.slug_map.insert(norm.clone(), slug.to_owned());

        // Store original path (preserve casing for filesystem operations)
        self.files.push(relative_path.to_string());
    }

    /// Register a directory that gets an auto-generated folder-index page —
    /// the input to `resolve_path`'s synthetic-folder-note fallback (see the
    /// field doc on [`ContentGraph::auto_index_dirs`]). `dir` is the
    /// original-case, project-relative directory path; the graph keys by its
    /// normalized form so a reference in any case still matches, and keeps
    /// `dir` verbatim as the value so the resolved page's H1/breadcrumb can
    /// show the author's own casing.
    pub fn register_auto_index_dir(&mut self, dir: &str) {
        self.auto_index_dirs.insert(normalize_path(dir), dir.to_string());
    }

    /// Consume the builder and produce an immutable [`ContentGraph`].
    pub fn build(self) -> ContentGraph {
        ContentGraph {
            files: self.files,
            filename_index: self.filename_index,
            path_index: self.path_index,
            slug_map: self.slug_map,
            auto_index_dirs: self.auto_index_dirs,
            case_twins: self.case_twins,
            top_dirs: self.top_dirs,
            // Installed by the host via `with_output_overrides` once the build's
            // page map is known; the builder itself is scan-time and has none.
            output_overrides: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Convenience: build a graph with common test files.
    fn sample_graph() -> ContentGraph {
        let mut b = ContentGraphBuilder::new();
        b.add_file("posts/hello.md", "/posts/hello");
        b.add_file("posts/world.md", "/posts/world");
        b.add_file("guides/hello.md", "/guides/hello");
        b.add_file("projects/index.md", "/projects");
        b.add_file("notes/daily/daily.md", "/notes/daily");
        b.build()
    }

    // 1. Basic file addition and resolution
    #[test]
    fn test_builder_adds_file() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("notes/first.md", "/notes/first");
        let g = b.build();

        assert_eq!(g.all_files(), &["notes/first.md"]);
        assert_eq!(
            g.resolve_path("notes/first.md", ""),
            Some("notes/first.md".into())
        );
    }

    // 2. Case-insensitive filename lookup
    #[test]
    fn test_filename_index_case_insensitive() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("Notes/MyFile.md", "/notes/myfile");
        let g = b.build();

        // Lookup with different casing — should return original path
        assert_eq!(
            g.resolve_path("myfile", ""),
            Some("Notes/MyFile.md".into())
        );
        assert_eq!(
            g.resolve_path("MYFILE", ""),
            Some("Notes/MyFile.md".into())
        );
        assert_eq!(
            g.resolve_path("MyFile", ""),
            Some("Notes/MyFile.md".into())
        );
    }

    // 3. Lookup without .md extension
    #[test]
    fn test_filename_index_without_extension() {
        let g = sample_graph();

        // "world" (no extension) should find "posts/world.md"
        assert_eq!(
            g.resolve_path("world", ""),
            Some("posts/world.md".into())
        );
    }

    // 4. Ambiguous filename resolved by longest common directory prefix
    #[test]
    fn test_ambiguous_resolved_by_common_prefix() {
        let g = sample_graph();

        // "hello" is ambiguous: posts/hello.md vs guides/hello.md
        // from "posts/other.md" -> posts/hello.md should win
        assert_eq!(
            g.resolve_path("hello", "posts/other.md"),
            Some("posts/hello.md".into())
        );

        // from "guides/other.md" -> guides/hello.md should win
        assert_eq!(
            g.resolve_path("hello", "guides/other.md"),
            Some("guides/hello.md".into())
        );
    }

    // 7. Folder note resolution: [[projects]] -> projects/index.md
    #[test]
    fn test_folder_note_resolution() {
        let g = sample_graph();

        assert_eq!(
            g.resolve_path("projects", ""),
            Some("projects/index.md".into())
        );
    }

    // 7a. Folder-note resolution prefers the source's language tree.
    // A bare folder reference like `docs/` written inside a `zh-hans/` page
    // must resolve to the same-language `zh-hans/docs/index.md`, not the
    // root-level `docs/index.md`. Mirrors the bare-name language scoping at
    // step 1b for the folder-note (step 5) path.
    #[test]
    fn test_folder_note_prefers_same_language_tree() {
        let g = ContentGraph::from_paths(&[
            "docs/index.md",
            "zh-hans/docs/index.md",
            "zh-hans/index.md",
        ]);

        // From a zh-hans page, `docs/` resolves to the zh-hans docs folder.
        assert_eq!(
            g.resolve_path("docs/", "zh-hans/index.md"),
            Some("zh-hans/docs/index.md".into())
        );

        // From a root page, `docs/` still resolves to the root docs folder.
        assert_eq!(
            g.resolve_path("docs/", "index.md"),
            Some("docs/index.md".into())
        );
    }

    // 7a-fallback. When no same-language folder note exists, a language-tree
    // page falls back to the root folder note rather than failing.
    #[test]
    fn test_folder_note_falls_back_to_root_when_no_language_sibling() {
        let g = ContentGraph::from_paths(&["docs/index.md", "zh-hans/index.md"]);

        assert_eq!(
            g.resolve_path("docs/", "zh-hans/index.md"),
            Some("docs/index.md".into())
        );
    }

    // 1b. A path-shaped reference (contains a slash) from a language-tree
    // source prefers the same-language sibling over the root, as a bare
    // reference does: `[[work/spring-show]]` from a zh-hans page is
    // `zh-hans/work/spring-show.md`, not the root `work/spring-show.md`.
    #[test]
    fn test_path_reference_prefers_same_language_tree() {
        let g = ContentGraph::from_paths(&[
            "work/spring-show.md",
            "zh-hans/work/spring-show.md",
            "zh-hans/index.md",
        ]);

        // The bare form already worked before this fix.
        assert_eq!(
            g.resolve_path("spring-show", "zh-hans/index.md"),
            Some("zh-hans/work/spring-show.md".into())
        );

        // The path form must land on the same page.
        assert_eq!(
            g.resolve_path("work/spring-show", "zh-hans/index.md"),
            Some("zh-hans/work/spring-show.md".into())
        );

        // From a root page, the path form still resolves to the root page.
        assert_eq!(
            g.resolve_path("work/spring-show", "index.md"),
            Some("work/spring-show.md".into())
        );
    }

    // 1b-fallback. When no same-language sibling exists, a language-tree
    // page's path reference falls back to the exact path rather than failing.
    #[test]
    fn test_path_reference_falls_back_to_exact_path_when_no_language_sibling() {
        let g = ContentGraph::from_paths(&["work/spring-show.md", "zh-hans/index.md"]);

        assert_eq!(
            g.resolve_path("work/spring-show", "zh-hans/index.md"),
            Some("work/spring-show.md".into())
        );
    }

    // 1b-explicit. An author who names a language tree explicitly in the
    // reference itself is never re-scoped — [[en/work/spring-show]] from a
    // zh-hans page must still resolve to the named English page even when an
    // (adversarial, same-shape) zh-hans sibling exists at that literal path.
    #[test]
    fn test_path_reference_with_explicit_language_prefix_is_not_rescoped() {
        let g = ContentGraph::from_paths(&[
            "en/work/spring-show.md",
            "zh-hans/en/work/spring-show.md",
            "zh-hans/index.md",
        ]);

        assert_eq!(
            g.resolve_path("en/work/spring-show", "zh-hans/index.md"),
            Some("en/work/spring-show.md".into())
        );
    }

    // A target that names an extension names a file of that extension: the
    // name search never answers `chart.png` with a page or another format.
    #[test]
    fn a_name_with_an_extension_matches_only_that_extension() {
        let g = ContentGraph::from_paths(&["notes/chart.md", "img/logo.svg", "a/note.md", "b/photo.jpg"]);
        assert_eq!(g.resolve_path("chart.png", "a/page.md"), None);
        assert_eq!(g.resolve_path("logo.png", "a/page.md"), None);
        assert_eq!(g.resolve_path("LOGO.SVG", "a/page.md"), Some("img/logo.svg".into()));
        // Without an extension a name still finds the page, then any file.
        assert_eq!(g.resolve_path("note", "x.md"), Some("a/note.md".into()));
        assert_eq!(g.resolve_path("note.md", "x.md"), Some("a/note.md".into()));
        assert_eq!(g.resolve_path("photo", "x.md"), Some("b/photo.jpg".into()));
        assert_eq!(g.resolve_path("photo.jpg", "x.md"), Some("b/photo.jpg".into()));
    }

    // Two files whose paths differ only in letter case (a case-sensitive disk)
    // are one key; which of them the graph keeps must not depend on the order
    // a directory listing happened to return them in.
    #[test]
    fn of_two_case_twins_the_same_one_is_kept_whatever_the_order() {
        let a = ContentGraph::from_paths(&["a/photo.jpg", "a/Photo.jpg"]);
        let b = ContentGraph::from_paths(&["a/Photo.jpg", "a/photo.jpg"]);
        assert_eq!(a.resolve_path("photo.jpg", "a/p.md"), b.resolve_path("photo.jpg", "a/p.md"));
        assert_eq!(a.all_files(), b.all_files());
    }

    // A target without an extension names a page of any page extension, as
    // `.md`, before a same-named file that is not a page; a bare image name
    // still finds the image when no page shares it.
    #[test]
    fn a_target_without_an_extension_finds_a_page_of_any_page_extension() {
        let g = ContentGraph::from_paths(&["notes/note.markdown", "img/note.jpg"]);
        assert_eq!(g.resolve_path("note", "x.md"), Some("notes/note.markdown".into()));
        let g = ContentGraph::from_paths(&["posts/note.mdown", "other/note.md"]);
        assert_eq!(g.resolve_path("posts/note", "other/x.md"), Some("posts/note.mdown".into()));
        let g = ContentGraph::from_paths(&["posts/note.mkd", "note.md"]);
        assert_eq!(g.resolve_path("note", "posts/a.md"), Some("posts/note.mkd".into()));
        let g = ContentGraph::from_paths(&["zh-hans/footer.mdown", "footer.md", "zh-hans/blog/p.md"]);
        assert_eq!(g.resolve_path("footer", "zh-hans/blog/p.md"), Some("zh-hans/footer.mdown".into()));
        let g = ContentGraph::from_paths(&["archive/archive.mkd"]);
        assert_eq!(g.resolve_path("archive", "x.md"), Some("archive/archive.mkd".into()));
        let g = ContentGraph::from_paths(&["a/photo.jpg", "b/photo.mdown"]);
        assert_eq!(g.resolve_path("photo.jpg", "b/x.md"), Some("a/photo.jpg".into()));
        let g = ContentGraph::from_paths(&["a/photo.jpg", "b/other.mdown"]);
        assert_eq!(g.resolve_path("photo", "b/x.md"), Some("a/photo.jpg".into()));
    }

    // Of pages that differ only in extension and are equally near, `.md` wins
    // and the others are not reported as ties; nearness still comes first.
    #[test]
    fn of_equally_near_pages_the_md_one_wins_without_a_tie() {
        let g = ContentGraph::from_paths(&["a/note.markdown", "a/note.md", "a/note.mdown"]);
        let r = g.resolve_path_with_ties("note", "x.md").unwrap();
        assert_eq!((r.path.as_str(), r.ties.len()), ("a/note.md", 0));
        let g = ContentGraph::from_paths(&["a/note.md", "b/note.markdown"]);
        assert_eq!(g.resolve_path("note", "b/x.md"), Some("b/note.markdown".into()));
    }

    // A folder's note is the page the build makes its home: `index.md` and the
    // other home names as `.md` only, so `readme.md` beats `index.mdown`, which
    // the build publishes as an ordinary page.
    #[test]
    fn a_folder_note_is_the_home_the_build_elects() {
        let g = ContentGraph::from_paths(&["docs/index.mdown", "docs/readme.md"]);
        assert_eq!(g.resolve_path("docs", "x.md"), Some("docs/readme.md".into()));
        let g = ContentGraph::from_paths(&["docs/index.mdown", "docs/a.md"]);
        assert_eq!(g.resolve_path("docs", "x.md"), None);
        let names = ["index.mdown", "readme.md"];
        assert_eq!(crate::home::detect_home_file_in_folder(&names, "docs"), Some("readme.md"));
    }

    // From a page in a language tree, a target whose first folder only looks
    // like a language code is read from the page's folder unless the site has
    // a top-level folder of that name; `en/x` still means the English tree.
    #[test]
    fn a_language_shaped_folder_is_a_language_tree_only_when_the_site_has_one() {
        let g = ContentGraph::from_paths(&["zh-hans/travel/a.md", "zh-hans/travel/uk/p.jpg", "zh-hans/travel/a/uk/p.jpg"]);
        let r = g.resolve_path_with_ties("uk/p.jpg", "zh-hans/travel/a.md").unwrap();
        assert_eq!((r.path.as_str(), r.matched), ("zh-hans/travel/uk/p.jpg", PathMatch::Written { exact_case: true }));
        let g = ContentGraph::from_paths(&["zh-hans/travel/a.md", "zh-hans/travel/uk/p.jpg", "uk/p.jpg"]);
        assert_eq!(g.resolve_path("uk/p.jpg", "zh-hans/travel/a.md"), Some("uk/p.jpg".into()));
    }

    // A reference spelled exactly like one of two case twins names that twin,
    // in every form; a spelling matching neither gets the kept one.
    #[test]
    fn a_reference_spelled_like_one_case_twin_names_that_twin() {
        for order in [["a/photo.jpg", "a/Photo.jpg"], ["a/Photo.jpg", "a/photo.jpg"]] {
            let g = ContentGraph::from_paths(&order);
            let r = g.resolve_path_with_ties("photo.jpg", "a/p.md").unwrap();
            assert_eq!((r.path.as_str(), r.matched), ("a/photo.jpg", PathMatch::Written { exact_case: true }));
            let r = g.resolve_path_with_ties("Photo.jpg", "a/p.md").unwrap();
            assert_eq!((r.path.as_str(), r.matched), ("a/Photo.jpg", PathMatch::Written { exact_case: true }));
            let r = g.resolve_path_with_ties("/a/photo.jpg", "b/p.md").unwrap();
            assert_eq!((r.path.as_str(), r.matched), ("a/photo.jpg", PathMatch::Written { exact_case: true }));
            assert_eq!(g.resolve_path("../a/photo.jpg", "b/p.md"), Some("a/photo.jpg".into()));
            let r = g.resolve_path_with_ties("PHOTO.jpg", "a/p.md").unwrap();
            assert_eq!((r.path.as_str(), r.matched), ("a/Photo.jpg", PathMatch::Written { exact_case: false }));
        }
        // Twins differing in a folder's case: the path written names its twin.
        let g = ContentGraph::from_paths(&["A/x.jpg", "a/x.jpg"]);
        let r = g.resolve_path_with_ties("x.jpg", "a/p.md").unwrap();
        assert_eq!((r.path.as_str(), r.matched), ("a/x.jpg", PathMatch::Written { exact_case: true }));
        let r = g.resolve_path_with_ties("x.jpg", "A/p.md").unwrap();
        assert_eq!((r.path.as_str(), r.matched), ("A/x.jpg", PathMatch::Written { exact_case: true }));
    }

    // A target that matches nothing as written is tried percent-decoded, in
    // every form: `[[my%20note]]` and an encoded separator both find the file.
    #[test]
    fn a_percent_encoded_target_is_tried_decoded() {
        let g = ContentGraph::from_paths(&["notes/my note.md", "a/b.png", "c/100%.png"]);
        assert_eq!(g.resolve_path("my%20note", "x.md"), Some("notes/my note.md".into()));
        assert_eq!(g.resolve_path("a%2Fb.png", "x.md"), Some("a/b.png".into()));
        assert_eq!(g.resolve_path("a%2fb.png", "x.md"), Some("a/b.png".into()));
        // A `%` that is not an escape is matched as written.
        assert_eq!(g.resolve_path("100%.png", "x.md"), Some("c/100%.png".into()));
    }

    // A folder whose name happens to be a language code (`uk`, `id`) is an
    // ordinary folder to a page outside any language tree: the path written
    // from the page's folder names its file.
    #[test]
    fn a_language_shaped_folder_beside_an_ordinary_page_is_read_from_the_page() {
        let g = ContentGraph::from_paths(&["trips/index.md", "trips/uk/day1.jpg", "trips/2023/uk/day1.jpg"]);
        assert_eq!(g.resolve_path("uk/day1.jpg", "trips/index.md"), Some("trips/uk/day1.jpg".into()));
        let g = ContentGraph::from_paths(&["notes/a.md", "notes/id/card.png", "id/card.png"]);
        assert_eq!(g.resolve_path("id/card.png", "notes/a.md"), Some("notes/id/card.png".into()));
    }

    // 7b. Self-named folder note: [[daily]] -> notes/daily/daily.md
    #[test]
    fn test_self_named_folder_note_resolution() {
        // "daily" as a filename stem appears in the filename index,
        // so it resolves via step 3 rather than step 5.
        let g = sample_graph();

        assert_eq!(
            g.resolve_path("daily", ""),
            Some("notes/daily/daily.md".into())
        );
    }

    // 7c. Self-named folder note via path
    #[test]
    fn test_self_named_folder_note_via_path() {
        let mut b = ContentGraphBuilder::new();
        // Only register the self-named note, no filename stem shortcut
        b.add_file("archive/archive.md", "/archive");
        let g = b.build();

        // Path-based reference should find it via the folder-note fallback
        assert_eq!(
            g.resolve_path("archive", ""),
            Some("archive/archive.md".into())
        );
    }

    // 7d. Auto-index folder note: a folder registered via
    // `register_auto_index_dir` (no `index.md`, no self-named note of its
    // own) resolves to its synthetic `<dir>/index.md`, case-insensitively.
    #[test]
    fn test_auto_index_dir_resolves_when_no_real_note_exists() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("essays/entry.md", "/essays/entry");
        b.register_auto_index_dir("Essays");
        let g = b.build();

        assert_eq!(g.resolve_path("essays", ""), Some("Essays/index.md".into()));
        assert_eq!(g.resolve_path("Essays", ""), Some("Essays/index.md".into()));
    }

    // 7e. A real folder note (self-named or an index stem) wins over the
    // registered auto-index dir — steps 1-4 and the file-backed checks
    // inside `folder_note` all run before the auto-index fallback.
    #[test]
    fn test_real_folder_note_wins_over_auto_index_registration() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("news/index.md", "/news");
        b.register_auto_index_dir("news");
        let g = b.build();

        assert_eq!(g.resolve_path("news", ""), Some("news/index.md".into()));
    }

    // 7f. A directory that was never registered does not resolve through
    // this fallback — an unregistered folder must not silently match.
    #[test]
    fn test_unregistered_dir_does_not_resolve_via_auto_index_fallback() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("essays/entry.md", "/essays/entry");
        let g = b.build();

        assert_eq!(g.resolve_path("essays", ""), None);
    }

    // 8. Unresolved returns None
    #[test]
    fn test_unresolved_returns_none() {
        let g = sample_graph();

        assert_eq!(g.resolve_path("nonexistent", ""), None);
        assert_eq!(g.resolve_path("posts/missing.md", ""), None);
    }

    #[test]
    fn test_root_reference_resolves_to_slash() {
        // `[[/]]` names the vault root, not a missing file — resolve_path is
        // the single source of truth for wikilink resolution, so it must
        // agree with the "/" root convention that resolve_folder_id and
        // frontmatter_ref_to_stem already fold down to the empty folder-id
        // for `children: '[[/]]'`. Resolving to a literal "/" (not "") matters
        // for the frontmatter substitution: `normalize_children` treats an
        // empty children value as "no listing", so "" would silently drop
        // the listing again.
        let g = sample_graph();

        assert_eq!(g.resolve_path("/", "posts/hello.md"), Some("/".to_string()));
        assert_eq!(g.resolve_path("//", "posts/hello.md"), Some("/".to_string()));
        // An actually-empty reference (`[[]]`) is a different, still-unresolved
        // case — it never named a root at all.
        assert_eq!(g.resolve_path("", "posts/hello.md"), None);
    }

    // 9. Exact relative path wins over filename
    #[test]
    fn test_exact_path_match() {
        let g = sample_graph();

        // Exact path should resolve directly, even though "hello" is ambiguous
        assert_eq!(
            g.resolve_path("guides/hello.md", "posts/other.md"),
            Some("guides/hello.md".into())
        );
    }

    // 10. Partial path match: "posts/hello" matches "posts/hello.md"
    #[test]
    fn test_partial_path_match() {
        let g = sample_graph();

        assert_eq!(
            g.resolve_path("posts/hello", ""),
            Some("posts/hello.md".into())
        );
        assert_eq!(
            g.resolve_path("posts/world", ""),
            Some("posts/world.md".into())
        );
    }

    // Slug lookup
    #[test]
    fn test_get_slug() {
        let g = sample_graph();

        assert_eq!(g.get_slug("posts/hello.md"), Some("/posts/hello"));
        assert_eq!(g.get_slug("Posts/Hello.md"), Some("/posts/hello"));
        assert_eq!(g.get_slug("nope.md"), None);
    }

    #[test]
    fn test_contains_path() {
        let g = sample_graph();

        assert!(g.contains_path("posts/hello.md"));
        // Case-insensitive, like every other path_index lookup.
        assert!(g.contains_path("Posts/Hello.md"));
        // A synthetic path resolve_path can manufacture (an auto-index
        // folder's home page) but never registers as a real file.
        assert!(!g.contains_path("posts/index.md"));
        assert!(!g.contains_path("nope.md"));
    }

    // all_files preserves insertion order
    #[test]
    fn test_all_files_order() {
        let g = sample_graph();

        assert_eq!(
            g.all_files(),
            &[
                "posts/hello.md",
                "posts/world.md",
                "guides/hello.md",
                "projects/index.md",
                "notes/daily/daily.md",
            ]
        );
    }

    // Unicode normalization (NFC)
    #[test]
    fn test_unicode_normalization() {
        let mut b = ContentGraphBuilder::new();
        // e + combining acute accent (NFD)
        b.add_file("caf\u{0065}\u{0301}.md", "/cafe");
        let g = b.build();

        // Lookup with NFC form (precomposed e-acute) — returns original NFD form
        assert_eq!(
            g.resolve_path("caf\u{00e9}.md", ""),
            Some("caf\u{0065}\u{0301}.md".into())
        );
        // Lookup with NFD form — returns original NFD form
        assert_eq!(
            g.resolve_path("caf\u{0065}\u{0301}.md", ""),
            Some("caf\u{0065}\u{0301}.md".into())
        );
    }

    // generate_slug tests
    #[test]
    fn generate_slug_gives_nfd_and_nfc_spellings_the_same_slug() {
        // macOS/iCloud write file names decomposed (NFD); links are composed.
        assert_eq!(
            generate_slug("caf\u{0065}\u{0301}-au-lait.md"),
            generate_slug("caf\u{00e9}-au-lait.md")
        );
        assert_eq!(generate_slug("caf\u{0065}\u{0301}-au-lait.md"), "caf\u{00e9}-au-lait");
        // Hangul syllable written as jamo (NFD) keeps its letters.
        assert_eq!(generate_slug("\u{1112}\u{1161}\u{11ab}.md"), generate_slug("\u{d55c}.md"));
    }

    #[test]
    fn test_generate_slug_strips_extension() {
        assert_eq!(generate_slug("posts/hello.md"), "posts/hello");
        assert_eq!(generate_slug("image.png"), "image");
    }

    #[test]
    fn test_generate_slug_lowercases() {
        assert_eq!(generate_slug("Posts/Hello.md"), "posts/hello");
    }

    #[test]
    fn test_generate_slug_replaces_spaces() {
        assert_eq!(generate_slug("posts/Hello World.md"), "posts/hello-world");
    }

    #[test]
    fn test_generate_slug_normalizes_backslashes() {
        assert_eq!(generate_slug("posts\\hello.md"), "posts/hello");
    }

    #[test]
    fn test_generate_slug_no_extension() {
        assert_eq!(generate_slug("readme"), "readme");
    }

    #[test]
    fn test_generate_slug_dotfile_keeps_leading_dot() {
        // Regression: a refactor of the extension-stripping branch (commit
        // 0d128270e) accidentally yielded an empty stem for `.gitignore` and
        // `.bashrc` because `rsplit_once('.')` returns `("", "gitignore")` and
        // an `is_empty()` guard wasn't in place. Pin the original semantics:
        // when the dot is at position 0 of the last segment, treat the whole
        // segment as the stem.
        assert_eq!(generate_slug(".gitignore"), "gitignore");
        assert_eq!(generate_slug(".bashrc"), "bashrc");
        assert_eq!(generate_slug("posts/.hidden"), "posts/hidden");
    }

    #[test]
    fn test_generate_slug_deep_path() {
        assert_eq!(
            generate_slug("deep/path/to/file.txt"),
            "deep/path/to/file"
        );
    }

    #[test]
    fn test_generate_slug_strips_ascii_punctuation() {
        assert_eq!(
            generate_slug("news/Hello, and Goodbye on NewsWire.md"),
            "news/hello-and-goodbye-on-newswire"
        );
        assert_eq!(generate_slug("posts/Hello (World)!.md"), "posts/hello-world");
        assert_eq!(generate_slug("posts/it's-mine.md"), "posts/its-mine");
        assert_eq!(generate_slug("posts/foo:bar.md"), "posts/foobar");
    }

    #[test]
    fn test_generate_slug_collapses_consecutive_hyphens() {
        assert_eq!(generate_slug("posts/foo--bar.md"), "posts/foo-bar");
        assert_eq!(generate_slug("posts/foo - bar.md"), "posts/foo-bar");
        assert_eq!(generate_slug("posts/a---b.md"), "posts/a-b");
    }

    #[test]
    fn test_generate_slug_trims_leading_trailing_hyphens_per_segment() {
        assert_eq!(generate_slug("posts/-hello.md"), "posts/hello");
        assert_eq!(generate_slug("posts/hello-.md"), "posts/hello");
    }

    #[test]
    fn test_generate_slug_preserves_non_ascii() {
        assert_eq!(generate_slug("视频/视频.md"), "视频/视频");
        assert_eq!(
            generate_slug("posts/AI 带来写作的黄金时代.md"),
            "posts/ai-带来写作的黄金时代"
        );
    }

    #[test]
    fn test_generate_slug_preserves_path_separators() {
        assert_eq!(generate_slug("a/b/c.md"), "a/b/c");
        assert_eq!(generate_slug("a, b/c.md"), "a-b/c");
    }

    // When both index.md and self-named exist, filename stem match (step 3)
    // resolves "recipes" to recipes/recipes.md (unique stem match).
    // This is correct: the self-named note IS the folder's page in Obsidian links.
    #[test]
    fn test_resolve_self_named_via_filename_stem() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("recipes/index.md", "/recipes");
        b.add_file("recipes/recipes.md", "/recipes/recipes");
        let g = b.build();

        // "recipes" matches filename stem "recipes" → recipes/recipes.md (step 3)
        assert_eq!(
            g.resolve_path("recipes", "other.md"),
            Some("recipes/recipes.md".into())
        );
    }

    // When only index.md exists (no self-named), folder note fallback (step 5) works
    #[test]
    fn test_resolve_folder_note_fallback_to_index() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("recipes/index.md", "/recipes");
        b.add_file("recipes/pasta.md", "/recipes/pasta");
        let g = b.build();

        assert_eq!(
            g.resolve_path("recipes", "other.md"),
            Some("recipes/index.md".into())
        );
    }

    // Suffix match: partial path resolves when a deeper file ends with the reference
    #[test]
    fn test_suffix_match_partial_path() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("文字/游记/index.md", "/文字/游记");
        b.add_file("index.md", "/");
        let g = b.build();

        // "游记/index.md" doesn't exist at root, but "文字/游记/index.md" ends with it
        assert_eq!(
            g.resolve_path("游记/index.md", "index.md"),
            Some("文字/游记/index.md".into())
        );
    }

    // Suffix match with ambiguity uses from_path tiebreaker
    #[test]
    fn test_suffix_match_ambiguous_uses_tiebreaker() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("a/游记/index.md", "/a/游记");
        b.add_file("b/游记/index.md", "/b/游记");
        let g = b.build();

        // From "a/other.md", should prefer "a/游记/index.md"
        assert_eq!(
            g.resolve_path("游记/index.md", "a/other.md"),
            Some("a/游记/index.md".into())
        );
        // From "b/other.md", should prefer "b/游记/index.md"
        assert_eq!(
            g.resolve_path("游记/index.md", "b/other.md"),
            Some("b/游记/index.md".into())
        );
    }

    // Vault-root prefix: "山居/交互实验/index.md" should resolve to "交互实验/index.md"
    // by stripping the leading component that doesn't match any graph path.
    // This matches Obsidian's behavior where vault name can prefix markdown links.
    #[test]
    fn test_vault_root_prefix_resolves_correctly() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("交互实验/index.md", "/交互实验");
        b.add_file("文字/分布式信息网络/index.md", "/文字/分布式信息网络");
        let g = b.build();

        // Should resolve to 交互实验/index.md, NOT 文字/分布式信息网络/index.md
        assert_eq!(
            g.resolve_path("山居/交互实验/index.md", ""),
            Some("交互实验/index.md".into())
        );
    }

    // Progressive sub-path stripping with non-index files
    #[test]
    fn test_vault_root_prefix_non_index() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("posts/hello.md", "/posts/hello");
        b.add_file("guides/hello.md", "/guides/hello");
        let g = b.build();

        // "mysite/posts/hello.md" should resolve to "posts/hello.md"
        assert_eq!(
            g.resolve_path("mysite/posts/hello.md", ""),
            Some("posts/hello.md".into())
        );
    }

    // Progressive sub-path: deeper nesting still works
    #[test]
    fn test_vault_root_prefix_deep_nesting() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("文字/游记/index.md", "/文字/游记");
        let g = b.build();

        // "vault/文字/游记/index.md" should find "文字/游记/index.md"
        assert_eq!(
            g.resolve_path("vault/文字/游记/index.md", ""),
            Some("文字/游记/index.md".into())
        );
    }

    // resolve_path preserves original casing of stored file paths
    #[test]
    fn test_resolve_path_preserves_original_case() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("音乐/Winter-Song.mov", "音乐/winter-song");
        let g = b.build();

        // Lookup with different casing should return original path
        assert_eq!(
            g.resolve_path("winter-song.mov", ""),
            Some("音乐/Winter-Song.mov".into())
        );
        assert_eq!(
            g.resolve_path("Winter-Song.mov", ""),
            Some("音乐/Winter-Song.mov".into())
        );
    }

    // all_files preserves original casing
    #[test]
    fn test_all_files_preserves_original_case() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("Notes/MyFile.md", "/notes/myfile");
        b.add_file("Posts/Hello-World.md", "/posts/hello-world");
        let g = b.build();

        assert_eq!(
            g.all_files(),
            &["Notes/MyFile.md", "Posts/Hello-World.md"]
        );
    }

    // ---------------------------------------------------------------------
    // Stem-collision: extension-aware tiebreaker
    //
    // When `![[scale-compare.png]]` and `![[scale-compare.html]]` are siblings,
    // the wikilink author's extension carries intent: `.png` should resolve to
    // the image, `.html` to the HTML file. Without an extension preference the
    // tiebreaker reduces to candidate registration order, which is brittle.
    // ---------------------------------------------------------------------

    #[test]
    fn stem_collision_prefers_matching_extension_png() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("interactive/scale-compare.png", "/interactive/scale-compare.png");
        b.add_file("interactive/scale-compare.html", "/interactive/scale-compare.html");
        let g = b.build();

        assert_eq!(
            g.resolve_path("scale-compare.png", "interactive/article.md"),
            Some("interactive/scale-compare.png".into())
        );
    }

    #[test]
    fn stem_collision_prefers_matching_extension_html() {
        let mut b = ContentGraphBuilder::new();
        b.add_file("interactive/scale-compare.png", "/interactive/scale-compare.png");
        b.add_file("interactive/scale-compare.html", "/interactive/scale-compare.html");
        let g = b.build();

        assert_eq!(
            g.resolve_path("scale-compare.html", "interactive/article.md"),
            Some("interactive/scale-compare.html".into())
        );
    }

    #[test]
    fn stem_collision_independent_of_registration_order() {
        // Same as above, with reverse insertion order. Result must not change.
        let mut b = ContentGraphBuilder::new();
        b.add_file("interactive/scale-compare.html", "/interactive/scale-compare.html");
        b.add_file("interactive/scale-compare.png", "/interactive/scale-compare.png");
        let g = b.build();

        assert_eq!(
            g.resolve_path("scale-compare.png", "interactive/article.md"),
            Some("interactive/scale-compare.png".into())
        );
        assert_eq!(
            g.resolve_path("scale-compare.html", "interactive/article.md"),
            Some("interactive/scale-compare.html".into())
        );
    }

    #[test]
    fn stem_collision_bare_ref_unchanged() {
        // A reference without an extension MUST keep existing behavior:
        // tiebreaker falls back to (lang_tree, common_prefix). The only
        // observable change is that ext-aware refs are now deterministic.
        let mut b = ContentGraphBuilder::new();
        b.add_file("interactive/scale-compare.png", "/interactive/scale-compare.png");
        b.add_file("interactive/scale-compare.html", "/interactive/scale-compare.html");
        let g = b.build();

        // No extension on ref: returns *some* candidate (current behavior),
        // we just assert the call succeeds rather than pinning the choice.
        assert!(g.resolve_path("scale-compare", "interactive/article.md").is_some());
    }

    #[test]
    fn stem_collision_bare_ref_prefers_page_over_asset() {
        // A folder holding both a page and a same-stem asset — 示例圖.md
        // beside 示例圖.jpg, one per work in a painting archive — must resolve
        // a bare `[[示例圖]]` to the page. Before this test, the ambiguity
        // fell through to the alphabetical tiebreaker, where ".jpg" sorts
        // ahead of ".md" and the link silently became a dead label card
        // pointing at the plate. ".jpg" alphabetically precedes ".md", so
        // this fails without the page-preference term.
        let mut b = ContentGraphBuilder::new();
        b.add_file("作品/示例圖.md", "/作品/示例圖.md");
        b.add_file("作品/示例圖.jpg", "/作品/示例圖.jpg");
        let g = b.build();

        assert_eq!(
            g.resolve_path("示例圖", "其他/note.md"),
            Some("作品/示例圖.md".into())
        );
    }

    #[test]
    fn stem_collision_bare_ref_extension_intent_still_wins() {
        // A caller that names the extension explicitly still gets the asset
        // — page preference only applies to a bare reference, which can't
        // express asset intent in the first place.
        let mut b = ContentGraphBuilder::new();
        b.add_file("作品/示例圖.md", "/作品/示例圖.md");
        b.add_file("作品/示例圖.jpg", "/作品/示例圖.jpg");
        let g = b.build();

        assert_eq!(
            g.resolve_path("示例圖.jpg", "其他/note.md"),
            Some("作品/示例圖.jpg".into())
        );
    }

    #[test]
    fn stem_collision_md_wins_over_html_sibling() {
        // The most common real case: a wikilink to `.md` (or no-extension
        // markdown ref) should not get hijacked by a `.html` sibling that
        // happens to be registered later.
        let mut b = ContentGraphBuilder::new();
        b.add_file("notes/guide.md", "/notes/guide");
        b.add_file("notes/guide.html", "/notes/guide.html");
        let g = b.build();

        assert_eq!(
            g.resolve_path("guide.md", "notes/index.md"),
            Some("notes/guide.md".into())
        );
    }

    #[test]
    fn stem_collision_suffix_match_arm() {
        // The suffix-match tiebreaker (ContentGraph::resolve_path step 2b)
        // also benefits from extension preference. Reference is multi-component
        // (`a/scale.png`) so it goes through the suffix-match arm, not the
        // bare-stem arm.
        let mut b = ContentGraphBuilder::new();
        b.add_file("vault/a/scale.png", "/vault/a/scale.png");
        b.add_file("vault/a/scale.html", "/vault/a/scale.html");
        let g = b.build();

        assert_eq!(
            g.resolve_path("a/scale.png", "vault/notes/article.md"),
            Some("vault/a/scale.png".into())
        );
    }

    #[test]
    fn stem_collision_suffix_match_arm_bare_ref_prefers_page() {
        // Same page-preference rule as the bare-stem arm, but exercised
        // through the suffix-match arm (step 2b) with a folder-qualified bare
        // reference — the two arms carry duplicated tiebreaker logic and both
        // must apply the rule.
        let mut b = ContentGraphBuilder::new();
        b.add_file("vault/作品/示例圖.md", "/vault/作品/示例圖.md");
        b.add_file("vault/作品/示例圖.jpg", "/vault/作品/示例圖.jpg");
        let g = b.build();

        assert_eq!(
            g.resolve_path("作品/示例圖", "vault/other/note.md"),
            Some("vault/作品/示例圖.md".into())
        );
    }

    #[test]
    fn stem_collision_ext_match_overrides_lang_tree() {
        // Pin priority: extension match wins even when a lang-tree candidate
        // exists. Without this, a `![[foo.png]]` in zh-hans/note.md against
        // siblings (zh-hans/foo.html + en/foo.png) would surprise users by
        // returning the .html file just because it shares a language tree.
        let mut b = ContentGraphBuilder::new();
        b.add_file("zh-hans/foo.html", "/zh-hans/foo.html");
        b.add_file("en/foo.png", "/en/foo.png");
        let g = b.build();

        assert_eq!(
            g.resolve_path("foo.png", "zh-hans/note.md"),
            Some("en/foo.png".into())
        );
    }

    #[test]
    fn stem_collision_alphabetical_final_tiebreaker() {
        // Bare ref + sibling stems: no extension intent, both same lang-tree,
        // equal common-prefix. The final alphabetical tiebreaker must make
        // the result independent of registration order.
        let mut b1 = ContentGraphBuilder::new();
        b1.add_file("notes/photo.png", "/notes/photo.png");
        b1.add_file("notes/photo.html", "/notes/photo.html");
        let g1 = b1.build();

        let mut b2 = ContentGraphBuilder::new();
        b2.add_file("notes/photo.html", "/notes/photo.html");
        b2.add_file("notes/photo.png", "/notes/photo.png");
        let g2 = b2.build();

        // "notes/photo.html" < "notes/photo.png" alphabetically → .html wins
        // in both insertion orders.
        let r1 = g1.resolve_path("photo", "notes/index.md");
        let r2 = g2.resolve_path("photo", "notes/index.md");
        assert_eq!(r1, r2, "result must not depend on registration order");
        assert_eq!(r1, Some("notes/photo.html".into()));
    }

    #[test]
    fn the_nearest_copy_wins_and_only_a_true_tie_is_reported() {
        let g = ContentGraph::from_paths(&["z/far.jpg", "a/x/y/far.jpg", "a/b/dup.jpg", "a/c/dup.jpg"]);
        // Nearest beats shallowest, with nothing tied.
        assert_eq!(
            g.resolve_path_with_ties("far.jpg", "a/x/page.md").map(|r| (r.path, r.ties)),
            Some(("a/x/y/far.jpg".to_string(), vec![]))
        );
        // Two copies equally near: the alphabetical first wins, the other is reported.
        assert_eq!(
            g.resolve_path_with_ties("dup.jpg", "a/x/page.md").map(|r| (r.path, r.ties)),
            Some(("a/b/dup.jpg".to_string(), vec!["a/c/dup.jpg".to_string()]))
        );
        // A copy sharing one more directory with the page is not a tie.
        assert_eq!(
            g.resolve_path_with_ties("dup.jpg", "a/c/page.md").map(|r| (r.path, r.ties)),
            Some(("a/c/dup.jpg".to_string(), vec![]))
        );
        assert_eq!(g.resolve_path("dup.jpg", "a/x/page.md").as_deref(), Some("a/b/dup.jpg"));
    }

    #[test]
    fn stem_collision_case_insensitive_extension() {
        // Author may write `.PNG`; should still match `.png` candidate.
        let mut b = ContentGraphBuilder::new();
        b.add_file("interactive/photo.PNG", "/interactive/photo.png");
        b.add_file("interactive/photo.html", "/interactive/photo.html");
        let g = b.build();

        assert_eq!(
            g.resolve_path("photo.png", "interactive/article.md"),
            Some("interactive/photo.PNG".into())
        );
    }
}
