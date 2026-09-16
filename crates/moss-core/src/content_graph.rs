//! In-memory index of content files for fuzzy path resolution.
//!
//! `ContentGraph` is the read-only query structure built by `ContentGraphBuilder`.
//! It supports Obsidian-style fuzzy path resolution: exact path, filename-only,
//! folder notes, and ambiguity tiebreaking by longest common directory prefix.
//!
//! Pure Rust, zero I/O.

use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

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
/// `pub(crate)` so the wikilink-completion ranker (`link_completions`) folds
/// paths identically to the resolver when scoring same-language / tree
/// proximity, keeping the completion order aligned with how links resolve.
pub(crate) fn normalize_path(path: &str) -> String {
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

/// Extract the filename (with extension) from a path.
fn filename_with_ext(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
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

/// Score a candidate's extension against the reference's extension for the
/// ambiguity tiebreaker. Returns 1 only when the reference carries an
/// extension AND the candidate's matches it (case-insensitive). Returns 0
/// otherwise so bare references — which can't express extension intent —
/// keep their existing tiebreaker behavior.
fn ext_match_score(ref_ext: Option<&str>, candidate: &str) -> u8 {
    let Some(want) = ref_ext else { return 0 };
    match path_extension(candidate) {
        Some(have) if have == want => 1,
        _ => 0,
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
/// - `"news/Farewell, and Erase on BroadwayWorld.md"`
///   -> `"news/farewell-and-erase-on-broadwayworld"`
/// - `"posts/Hello (World)!.md"` -> `"posts/hello-world"`
/// - `"posts/foo--bar.md"` -> `"posts/foo-bar"`
/// - `"image.png"` -> `"image"`
/// - `"视频/视频.md"` -> `"视频/视频"`  (non-ASCII passes through)
pub fn generate_slug(relative_path: &str) -> String {
    // Normalize separators
    let normalized = relative_path.replace('\\', "/");

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

    /// Exact-case asset index: original-case paths for O(1) membership checks.
    asset_exact: HashSet<String>,

    /// Lowercased path -> Vec<original-case paths> for case-insensitive lookup.
    asset_ci: HashMap<String, Vec<String>>,

    /// The build's source-directory → URL-slug overrides, so this graph can
    /// answer "what URL is this file served at" (see [`Self::pinned_url`]) and
    /// not just "which file does this reference mean". Empty unless the host
    /// installed them via [`Self::with_output_overrides`]; an empty map still
    /// yields base slugification, which is what every case-fold bug needed.
    output_overrides: HashMap<String, String>,
}

impl ContentGraph {
    /// Install the build's source-directory → URL-slug overrides, making this
    /// graph the authority on emitted URLs as well as on resolution.
    ///
    /// The host calls this once, right after the overrides are computed
    /// (`build_page_map` in src-tauri), and every emitter downstream reads the
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
    /// recurring bug class this method exists to end (moss#903 bug 3). A
    /// reference that does not resolve gets a `Diagnostic`, not a guessed path.
    pub fn pinned_url(&self, root_rel: &str) -> String {
        crate::resolve::output_url::pinned_url(root_rel, &self.output_overrides)
    }

    /// **Single source of truth for target resolution in moss.**
    ///
    /// Every link syntax — wikilinks `[[x]]`, standard markdown links
    /// `[t](x)`, image refs `![](x)`, embeds `![[x]]`, frontmatter refs —
    /// MUST resolve through this function. See the resolve pipeline in
    /// [`crate::resolve::resolve_content`] and the prose overview in
    /// `moss/docs/reference/link-resolution.md` for the per-syntax call sites.
    ///
    /// Downstream code (the compiler's URL-prettifier, for instance)
    /// receives already-resolved hrefs and MUST NOT reimplement any
    /// part of this chain. Adding a parallel resolver was the root
    /// cause of the `[文字](文字.md)` regression on sites using folder
    /// notes.
    ///
    /// Resolution chain (first match wins):
    /// 1. Exact normalized path
    /// 2. Exact + `.md`
    /// 3. Filename match (case-insensitive, without extension)
    /// 4. Filename + `.md` match
    /// 5. Folder note: `reference/index.md` or `reference/<reference>.md`
    ///
    /// Ambiguity tiebreakers, applied in order:
    /// candidates whose extension matches the reference's extension win first
    /// (e.g. `![[scale-compare.png]]` prefers a `.png` sibling over a `.html`
    /// sibling — only applies when the reference carries an extension);
    /// candidates in the same language tree as the source are preferred next;
    /// then longest common directory prefix with `from_path`; then alphabetical
    /// by normalized path (so results are independent of registration order
    /// when all earlier keys tie).
    pub fn resolve_path(&self, reference: &str, from_path: &str) -> Option<String> {
        let norm_ref = normalize_path(reference);
        let norm_from = normalize_path(from_path);
        let ref_ext = path_extension(&norm_ref);

        // Language-tree prefix of the source file, if any.
        // E.g. "zh-hans/about.md" -> Some("zh-hans").  Used to prefer
        // same-language-tree candidates when the reference is bare (no slash).
        let from_lang = crate::home::lang_tree_prefix(&norm_from);

        // 1. Exact path match
        if self.path_index.contains_key(&norm_ref) {
            return Some(self.files[self.path_index[&norm_ref]].clone());
        }

        // 1b. Bare reference (no slash) from a language-tree source:
        // prefer a same-language-tree sibling before falling back to root.
        // e.g. ![[footer]] from "zh-hans/about.md" should match
        //      "zh-hans/footer.md" if it exists, not root "footer.md".
        if !norm_ref.contains('/') {
            if let Some(lang) = from_lang {
                let scoped = format!("{}/{}", lang, norm_ref);
                if let Some(&idx) = self.path_index.get(&scoped) {
                    return Some(self.files[idx].clone());
                }
                let scoped_md = format!("{}/{}.md", lang, norm_ref);
                if let Some(&idx) = self.path_index.get(&scoped_md) {
                    return Some(self.files[idx].clone());
                }
            }
        }

        // 2. Exact + .md
        let with_md = format!("{}.md", norm_ref);
        if self.path_index.contains_key(&with_md) {
            return Some(self.files[self.path_index[&with_md]].clone());
        }

        // 2b. Suffix match for partial paths (Obsidian shortest-path resolution).
        // e.g. "游记/index.md" matches "文字/游记/index.md"
        // Also handles vault-root prefix: "刘果/交互实验/index.md" → try
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
                // Try exact + .md
                let with_md = format!("{}.md", subpath);
                if self.path_index.contains_key(&with_md) {
                    return Some(self.files[self.path_index[&with_md]].clone());
                }

                // Try suffix match (sub-path as suffix of a longer graph path)
                let suffix = format!("/{}", subpath);
                let candidates: Vec<usize> = self.files.iter().enumerate()
                    .filter(|(_, f)| normalize_path(f).ends_with(&suffix))
                    .map(|(i, _)| i)
                    .collect();
                if candidates.len() == 1 {
                    return Some(self.files[candidates[0]].clone());
                }
                if candidates.len() > 1 {
                    let from_dirs = dir_components(&norm_from);
                    let best = candidates.iter().copied().max_by_key(|&idx| {
                        // self.files stores original (pre-normalized) paths for
                        // filesystem fidelity; re-normalize here to compare
                        // against norm_from and lang_tree_prefix output.
                        let normalized = normalize_path(&self.files[idx]);
                        let candidate_dirs = dir_components(&normalized);
                        let tree_match = lang_tree_match(&normalized, from_lang);
                        let ext_match = ext_match_score(ref_ext.as_deref(), &normalized);
                        // Final key: alphabetical-by-path, ascending (Reverse so
                        // smaller path wins under max_by_key). Removes residual
                        // dependence on registration order when all other keys
                        // tie — see "then alphabetical" in the doc comment.
                        (
                            ext_match,
                            tree_match,
                            common_prefix_len(&candidate_dirs, &from_dirs),
                            std::cmp::Reverse(normalized.clone()),
                        )
                    });
                    if let Some(idx) = best {
                        return Some(self.files[idx].clone());
                    }
                }
            }
        }

        // 3/4. Filename match (stem, case-insensitive)
        // Skip stem matching when the reference is a multi-component path with an
        // index stem — falling back to just "index" would match every index.md in
        // the vault and return an arbitrary wrong result.
        let ref_stem = normalize_component(
            filename_stem(filename_with_ext(&norm_ref)),
        );
        let skip_stem = norm_ref.contains('/') && crate::home::is_index_stem(&ref_stem);
        if !skip_stem {
            if let Some(candidates) = self.filename_index.get(&ref_stem) {
                if candidates.len() == 1 {
                    return Some(self.files[candidates[0]].clone());
                }
                // Ambiguity tiebreakers, in priority order:
                //   1. Reference-extension match (only when the reference has
                //      an extension — otherwise this term is constant)
                //   2. Same language tree as the source (or both tree-less)
                //   3. Longest common directory prefix with from_path
                let from_dirs = dir_components(&norm_from);
                let best = candidates
                    .iter()
                    .copied()
                    .max_by_key(|&idx| {
                        // self.files stores original (pre-normalized) paths for
                        // filesystem fidelity; re-normalize here to compare
                        // against norm_from and lang_tree_prefix output.
                        let normalized = normalize_path(&self.files[idx]);
                        let candidate_dirs = dir_components(&normalized);
                        let tree_match = lang_tree_match(&normalized, from_lang);
                        let ext_match = ext_match_score(ref_ext.as_deref(), &normalized);
                        // Final key: alphabetical-by-path, ascending (Reverse so
                        // smaller path wins under max_by_key). Removes residual
                        // dependence on registration order when all other keys
                        // tie — see "then alphabetical" in the doc comment.
                        (
                            ext_match,
                            tree_match,
                            common_prefix_len(&candidate_dirs, &from_dirs),
                            std::cmp::Reverse(normalized.clone()),
                        )
                    });
                if let Some(idx) = best {
                    return Some(self.files[idx].clone());
                }
            }
        }

        // 5. Folder note: a folder reference resolves to that folder's home
        // file — either a recognized index stem (`<ref>/index.md`, in priority
        // order) or the self-named note (`<ref>/<leaf>.md`).
        let folder_note = |base: &str| -> Option<String> {
            for stem in crate::home::INDEX_STEMS {
                let folder_index = format!("{}/{}.md", base, stem);
                if let Some(&idx) = self.path_index.get(&folder_index) {
                    return Some(self.files[idx].clone());
                }
            }
            let leaf = base.rsplit('/').next().unwrap_or(base);
            let self_named = format!("{}/{}.md", base, leaf);
            self.path_index
                .get(&self_named)
                .map(|&idx| self.files[idx].clone())
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

    /// All file paths in insertion order.
    pub fn all_files(&self) -> &[String] {
        &self.files
    }

    // -----------------------------------------------------------------------
    // Exact-case asset index — backed by real-case paths, NOT the lowercased
    // path_index / filename_index. Task 6 wires these to the AssetIndex trait.
    // -----------------------------------------------------------------------

    /// Return `true` iff `p` is present in the graph with exactly this casing.
    pub fn asset_contains(&self, p: &str) -> bool {
        self.asset_exact.contains(p)
    }

    /// Case-insensitive membership: return the first canonical real-case path
    /// whose lowercased form equals `p.to_lowercase()`, or `None`.
    pub fn asset_contains_ci(&self, p: &str) -> Option<String> {
        self.asset_ci.get(&p.to_lowercase()).and_then(|v| v.first().cloned())
    }

    /// Return all real-case paths whose lowercased form ends with `/<suffix>`
    /// (or equals `suffix` exactly). Results are sorted for determinism.
    pub fn asset_find_by_suffix(&self, suffix: &str) -> Vec<String> {
        let ls = suffix.to_lowercase();
        let mut v: Vec<String> = self.asset_exact.iter().filter(|p| {
            let lp = p.to_lowercase();
            lp.ends_with(&ls)
                && (lp.len() == ls.len()
                    || lp.as_bytes()[lp.len() - ls.len() - 1] == b'/')
        }).cloned().collect();
        v.sort();
        v
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
    asset_exact: HashSet<String>,
    asset_ci: HashMap<String, Vec<String>>,
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

        // Skip duplicates: if this normalized path is already registered, don't
        // add another entry to `files` or `filename_index`.
        if self.path_index.contains_key(&norm) {
            return;
        }

        let idx = self.files.len();

        // Build filename stem index
        let stem = filename_stem(&norm).to_owned();
        self.filename_index.entry(stem).or_default().push(idx);

        // Build path index
        self.path_index.insert(norm.clone(), idx);

        // Slug map
        self.slug_map.insert(norm.clone(), slug.to_owned());

        // Store original path (preserve casing for filesystem operations)
        self.files.push(relative_path.to_string());

        // Exact-case asset index: keyed on real-case path, NOT normalized.
        self.asset_exact.insert(relative_path.to_string());
        self.asset_ci
            .entry(relative_path.to_lowercase())
            .or_default()
            .push(relative_path.to_string());
    }

    /// Consume the builder and produce an immutable [`ContentGraph`].
    pub fn build(self) -> ContentGraph {
        ContentGraph {
            files: self.files,
            filename_index: self.filename_index,
            path_index: self.path_index,
            slug_map: self.slug_map,
            asset_exact: self.asset_exact,
            asset_ci: self.asset_ci,
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

    // 8. Unresolved returns None
    #[test]
    fn test_unresolved_returns_none() {
        let g = sample_graph();

        assert_eq!(g.resolve_path("nonexistent", ""), None);
        assert_eq!(g.resolve_path("posts/missing.md", ""), None);
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
            generate_slug("news/Farewell, and Erase on BroadwayWorld.md"),
            "news/farewell-and-erase-on-broadwayworld"
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

    // Vault-root prefix: "刘果/交互实验/index.md" should resolve to "交互实验/index.md"
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
            g.resolve_path("刘果/交互实验/index.md", ""),
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

    #[test]
    fn exact_case_asset_index() {
        let g = ContentGraph::from_paths(&["assets/Hoon.JPG", "News/post.md"]);
        assert!(g.asset_contains("assets/Hoon.JPG"));
        assert!(!g.asset_contains("assets/hoon.jpg")); // exact case
        assert_eq!(
            g.asset_contains_ci("assets/hoon.jpg").as_deref(),
            Some("assets/Hoon.JPG")
        );
        assert_eq!(
            g.asset_find_by_suffix("Hoon.JPG"),
            vec!["assets/Hoon.JPG".to_string()]
        );
    }
}
