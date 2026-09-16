//! Page map construction for the static site generator.
//!
//! This module handles the file-tree to page-tree path mapping:
//! - `compute_home_file_winners()` — determines which file wins as the index
//!   page when multiple candidates exist in a folder.
//! - `resolve_path_with_overrides()` — the single mechanism for rewriting
//!   directory components according to URL overrides from folder index files.
//! - `build_page_map()` — pre-scans all markdown files to compute
//!   `source_path -> url_path` mappings including cascading URL overrides.

use std::path::Path;

use super::slug::slugify_path_segments;

mod frontmatter_cache;
pub(crate) use frontmatter_cache::{build_page_map_and_external_urls_cached, FrontmatterScanCache};
#[cfg(test)]
pub(crate) use frontmatter_cache::build_page_map_and_external_urls_cached_with_evicted;

mod folder_lang;
pub(crate) use folder_lang::{folder_of, resolve_folder_languages, FolderLangCache};

/// Pre-scan markdown frontmatter to elect each folder's home file, returning a
/// map from `parent_directory → file_path`. A file in this map wins its
/// folder's home slot regardless of filename — see [`compute_home_file_winners`].
///
/// This is the supported way to make `en/Liu Guo.md` (or any other
/// non-INDEX_STEM, non-self-named filename) the homepage of a folder.
/// Without this, moss falls back to the filename-only detection
/// in [`moss_core::home::detect_home_file_in_folder`] and the file lands
/// at a slug-based URL (`en/liu-guo/`) while moss synthesizes an empty
/// `en/index.html` titled `"En"` (issue #587).
///
/// # Election rules
///
/// 1. **Anchor** — a file that is its OWN folder's home: `home == Some(true)`
///    OR [`moss_core::home::is_home_file`] (self-named, index-stem, or
///    lang-suffixed, case-insensitive). The folder basename for a root-level
///    file is the project root's name, read from [`VaultRoot::name`] — the same
///    string `generate_blocking_content` feeds to
///    [`compute_home_file_winners`], so the two maps can never disagree.
/// 2. **Direct promotion** — every `home == Some(true)` file becomes its
///    folder's home in the override map (filename irrelevant).
/// 3. **Translation groups** — files are grouped by `translationKey` *value*
///    (any value; files with no key are skipped).
/// 4. **Inherited promotion** — for each group containing ≥1 anchor, every
///    member whose folder has no anchor of its own becomes its folder's home.
///    Rationale: the `en/` translation of a home inherits `/en/` home-ness via
///    the shared key, without needing its own marker. The `translationKey`
///    *value* never means "home" — only the translation *relationship* (sharing
///    a key with an anchor) propagates home-ness.
/// 5. **Precedence** — a folder's own anchor (`home: true` or name-based)
///    always beats an inherited candidate. The override map only carries the
///    overriding (frontmatter-marker or inherited) winners; name-based anchors
///    are resolved by [`compute_home_file_winners`] from filenames alone, so an
///    inherited candidate is suppressed for any folder that has a name-based
///    anchor.
///
/// At most one promotion per folder; if a folder gets multiple direct or
/// multiple inherited candidates, the first one (alphabetical by filename)
/// wins for determinism and a `log::warn!` names the demoted file(s).
pub(crate) fn compute_home_overrides(
    markdown_files: &[crate::types::content::FileInfo],
    root: &crate::vault::paths::VaultRoot,
) -> std::collections::HashMap<String, String> {
    compute_home_overrides_with_evicted(markdown_files, root, &crate::build::icloud::is_evicted)
}

/// Same as [`compute_home_overrides`] with an injectable eviction predicate —
/// see docs/archive/2026-07-31-cloud-download-waiting-mode.md Stage 3. A
/// dataless (cloud-evicted) source file is skipped exactly like a read
/// error (`Err(_) => continue`, just below) rather than blocking this
/// serial, single-threaded scan on the OS materializing it.
pub(crate) fn compute_home_overrides_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    root: &crate::vault::paths::VaultRoot,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;

    /// Per-file facts collected in a single frontmatter read.
    struct FileFacts {
        path: String,
        parent: String,
        filename: String,
        home: Option<bool>,
        translation_key: Option<String>,
        /// `true` when the file is its own folder's home by NAME
        /// (self-named / index-stem / lang-suffixed) — the name-based anchor.
        name_anchor: bool,
    }

    // THE root name, resolved once at the entry point. This line used to be
    // `source_path.file_name()...unwrap_or("")` — which read `""` for a dot-path
    // root while `blocking.rs`, eight lines away, patched around the same problem
    // with a local canonicalize fallback. Two consumers of one string, two answers.
    let root_basename = root.name();
    let source_path = root.path();

    let mut facts: Vec<FileFacts> = Vec::new();
    for fi in markdown_files {
        let abs = source_path.join(&fi.path);
        if is_evicted(&abs) {
            crate::build::cloud_readiness::request_download(&abs);
            continue;
        }
        let content = match std::fs::read_to_string(&abs) {
            Ok(c) => c,
            // The is_evicted gate above catches the common case; this catches
            // the race, and anything else. Dropping a page from the site map
            // is too consequential to do without saying so.
            Err(e) => {
                log::warn!("page map skipping unreadable '{}': {}", fi.path, e);
                continue;
            }
        };

        // ONE parser (ADR-020): same path the editor + build pipeline use.
        let (home, translation_key): (Option<bool>, Option<String>) =
            if crate::build::markdown::is_simplified_frontmatter(&content) {
                let (fm, _) = crate::build::markdown::parse_simplified_frontmatter(&content);
                (fm.home, fm.translation_key)
            } else {
                let fm = crate::build::markdown::parse_typed_frontmatter(&content);
                (fm.home, fm.translation_key)
            };

        let parent = Path::new(&fi.path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let filename = Path::new(&fi.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Folder basename for self-named detection: the parent dir's basename,
        // or the project root's basename for a root-level file. Mirrors how the
        // pipeline derives `parent_name` in `build_page_map`.
        let folder_basename = if parent.is_empty() {
            root_basename
        } else {
            Path::new(&parent)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
        };
        let stem_lower = Path::new(&fi.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let name_anchor =
            moss_core::home::is_home_file(&stem_lower, &folder_basename.to_lowercase());

        facts.push(FileFacts {
            path: fi.path.clone(),
            parent,
            filename,
            home,
            translation_key,
            name_anchor,
        });
    }

    // A folder "has its own anchor" when some file in it is an anchor —
    // either `home: true` (direct) or name-based (self-named / index-stem).
    let mut folders_with_anchor: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for f in &facts {
        if f.home == Some(true) || f.name_anchor {
            folders_with_anchor.insert(f.parent.clone());
        }
    }

    // Collect per-folder candidates as (filename, path) so we can apply the
    // shared deterministic election + multi-claim warning. A `reason` label
    // makes the warning accurate for both the direct and inherited paths.
    let mut by_folder: HashMap<String, Vec<(String, String)>> = HashMap::new();

    // Rule 2: direct promotion — every `home: true` file.
    for f in &facts {
        if f.home == Some(true) {
            by_folder
                .entry(f.parent.clone())
                .or_default()
                .push((f.filename.clone(), f.path.clone()));
        }
    }

    // Rules 3+4: translation groups → inherited promotion.
    // Group files by translationKey value (skip files with no key).
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in facts.iter().enumerate() {
        if let Some(key) = &f.translation_key {
            groups.entry(key.clone()).or_default().push(i);
        }
    }
    for members in groups.values() {
        // Does this group contain an anchor (rules 1: home:true OR name-based)?
        let has_anchor = members
            .iter()
            .any(|&i| facts[i].home == Some(true) || facts[i].name_anchor);
        if !has_anchor {
            continue;
        }
        // Name an anchor for the inheritance diagnostic below (any one — the
        // anchor's identity does not affect the result, only group membership).
        let anchor_path = members
            .iter()
            .find(|&&i| facts[i].home == Some(true) || facts[i].name_anchor)
            .map(|&i| facts[i].path.clone())
            .unwrap_or_default();
        // Inherited promotion: every member whose folder has NO anchor of its
        // own becomes its folder's home. (Rule 5 precedence: skip folders that
        // already have a direct/name anchor.) A member that is itself the
        // anchor lives in a folder with an anchor, so it is skipped here — it is
        // already handled by rule 2 (if home:true) or by name-detection.
        for &i in members {
            let f = &facts[i];
            if folders_with_anchor.contains(&f.parent) {
                continue;
            }
            // Diagnostic: this folder's home is elected purely by inheritance (no
            // local marker/index/self-name). A shared arbitrary translationKey is
            // a weaker intent signal than a name/marker, so make the "why is this
            // a home?" answer discoverable.
            // One line per inheriting folder, every build — on a large
            // multilingual site that scales with the tree, so DEBUG. Turn on
            // debug logging when the question is actually "why is this a home?".
            log::debug!(
                target: "i18n",
                "`{}` inherits home-ness for `{}` via translation link to anchor `{}`",
                f.path,
                if f.parent.is_empty() { "/" } else { f.parent.as_str() },
                anchor_path,
            );
            by_folder
                .entry(f.parent.clone())
                .or_default()
                .push((f.filename.clone(), f.path.clone()));
        }
    }

    let mut overrides = HashMap::new();
    let mut warned_folders: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (parent, mut candidates) in by_folder {
        // Dedup: a file could be added by both direct + inherited paths.
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        candidates.dedup_by(|a, b| a.1 == b.1);
        if candidates.len() > 1 {
            warned_folders.insert(parent.clone());
            let display_folder = if parent.is_empty() { "/" } else { parent.as_str() };
            let losers: Vec<&str> = candidates[1..].iter().map(|(_, p)| p.as_str()).collect();
            log::warn!(
                target: "i18n",
                "{} files claim the home slot in `{}`; using `{}` (alphabetical first), ignoring: {}",
                candidates.len(),
                display_folder,
                candidates[0].1,
                losers.join(", "),
            );
        }
        if let Some((_, path)) = candidates.into_iter().next() {
            overrides.insert(parent, path);
        }
    }

    // Broadened multi-claim diagnostic — NAME collisions. The warning above fires
    // only for marker/inherited candidates collected in `by_folder`. A name-only
    // collision (a self-named file + an `index`-stem, or marker + name) is resolved
    // silently by `detect_home_file_in_folder` precedence downstream, so the demoted
    // file becomes a regular page with no diagnostic (the source of stray `/index/`
    // nav items). Surface it. Gate on ≥1 name anchor AND skip folders the loop
    // above already warned, so pure marker/inherited collisions (and the rare
    // double-marker that is also name-anchored) never double-fire here.
    let mut name_candidates: HashMap<String, Vec<String>> = HashMap::new();
    let mut folder_has_name_anchor: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for f in &facts {
        if f.name_anchor || f.home == Some(true) {
            name_candidates
                .entry(f.parent.clone())
                .or_default()
                .push(f.path.clone());
            if f.name_anchor {
                folder_has_name_anchor.insert(f.parent.clone());
            }
        }
    }
    for (folder, mut cands) in name_candidates {
        if !folder_has_name_anchor.contains(&folder) || warned_folders.contains(&folder) {
            continue;
        }
        cands.sort();
        cands.dedup();
        if cands.len() >= 2 {
            let display = if folder.is_empty() { "/" } else { folder.as_str() };
            log::warn!(
                target: "i18n",
                "{} files claim the home slot in `{}` by name/marker: {} — moss elects by precedence (home:true › index › self-named); the rest become regular pages",
                cands.len(),
                display,
                cands.join(", "),
            );
        }
    }

    overrides
}

/// Groups markdown files by their parent directory, then uses
/// `detect_home_file_in_folder()` to pick one winner per group.
/// Returns a set of winning file paths (relative, e.g. "index.md" or "recipes/index.md").
///
/// `home_overrides` (from [`compute_home_overrides`]) wins over filename-based
/// detection: any folder listed there gets its overriding file as the
/// home, regardless of what `detect_home_file_in_folder` would pick from
/// filenames alone.
pub(crate) fn compute_home_file_winners(
    markdown_files: &[crate::types::content::FileInfo],
    root_folder_name: &str,
    home_overrides: &std::collections::HashMap<String, String>,
) -> std::collections::HashSet<String> {
    use std::collections::HashMap;

    // Group filenames by parent directory
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for fi in markdown_files {
        let parent = Path::new(&fi.path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let filename = Path::new(&fi.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        groups.entry(parent).or_default().push(filename);
    }

    let mut winners = std::collections::HashSet::new();
    for (parent_dir, filenames) in &groups {
        // home: true marker wins outright when set.
        if let Some(override_path) = home_overrides.get(parent_dir) {
            winners.insert(override_path.clone());
            continue;
        }

        // Determine the folder name for self-named detection
        let folder_name = if parent_dir.is_empty() {
            root_folder_name.to_string()
        } else {
            Path::new(parent_dir)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        };
        let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
        if let Some(winner_filename) = moss_core::home::detect_home_file_in_folder(&refs, &folder_name) {
            let winner_path = if parent_dir.is_empty() {
                winner_filename.to_string()
            } else {
                format!("{}/{}", parent_dir, winner_filename)
            };
            winners.insert(winner_path);
        }
    }
    winners
}

/// The single mechanism for all file-tree → page-tree path mapping.
///
/// Walks path segments, replacing any directory component that has a url
/// override from a folder index. This is used by every output path in the
/// pipeline — HTML pages, static assets, video conversions, placeholder
/// SVGs, AssetRegistry keys, and hashes.json entries.
///
/// # Invariant: keys are raw, values are slug-form
///
/// `dir_overrides` keys are the **raw** cumulative source-path of a folder
/// (e.g. `"News"`, `"News/Sub Section"`, `"视频"`). Values are the **slug**
/// form of the override target from the folder index's `url:` frontmatter
/// (e.g. `"blog"`, `"video"`). Both `lookup` and `path` arguments here are
/// raw source paths, so this function never sees slugified input. Phase 2
/// of `build_page_map` slugifies the prefix before stripping it off
/// `url_path` (see `slugify_path_segments`), because by the time Phase 2
/// runs, `url_path` itself has already been slugified by `compute_url_path`.
///
/// # Call sites
///
/// - `build_page_map()` — HTML page url_paths (this module)
/// - `PathResolver::resolve_url()` — HTML references: covers, og:image, cards (paths.rs)
/// - `copy_deferred_assets()` — static asset output paths (build.rs)
/// - `run_video_conversion()` — video .mp4 and .thumb.jpg output paths (build.rs)
/// - `update_video_hashes()` — hashes.json video_outputs keys (build.rs)
/// - `generate_blocking_content()` — placeholder SVG output paths (render.rs)
/// - `generate_blocking_content()` — AssetRegistry pending keys (render.rs)
///
/// # Slugification of directory segments
///
/// Every directory segment in the output is slug-form. If a directory has
/// an override, the override value (already slug-form by construction) wins.
/// Otherwise the raw source segment is run through [`generate_slug`], so a
/// Title-Case folder like `News/` produces `news/` regardless of override
/// status. Children of un-overridden Title-Case folders therefore land
/// under the same slug as the page itself, keeping URL and filesystem
/// output paths consistent on case-sensitive web servers.
///
/// The **last** segment is preserved verbatim, because callers pass full
/// file paths (e.g. `"News/Winter-Song.mov"`) and asset filenames must
/// keep their case so the rendered `<img src>` matches the file on disk.
///
/// # Pitfall: directory-only paths require a second pass
///
/// If you call this with a directory path (no filename), the returned last
/// segment is NOT slugified. Don't use the raw return value as a URL prefix
/// — wrap it in [`slugify_path_segments`] first. `build_page_map` Phase 2
/// is the one such caller in moss today and applies that wrap. Any new
/// directory-only call site must do the same; otherwise a Title-Case last
/// segment will leak back into URLs whenever an unrelated folder in the
/// same site has a `url:` override (which makes Phase 2 fire).
///
/// # Examples
///
/// Given `dir_overrides = {"视频" → "video"}`:
/// - `"视频/aimeili.mov"` → `"video/aimeili.mov"`
/// - `"视频/sub/clip.mp4"` → `"video/sub/clip.mp4"`
/// - `"News/chps-new-hub.png"` (no override) → `"news/chps-new-hub.png"`
/// - `"News/Winter-Song.mov"` (no override) → `"news/Winter-Song.mov"`
/// - `"News"` (directory only, no override) → `"News"`
pub(crate) fn resolve_path_with_overrides(
    path: &str,
    overrides: &std::collections::HashMap<String, String>,
) -> String {
    moss_core::resolve::output_url::resolve_path_with_overrides(path, overrides)
}

/// Build the file-tree → page-tree mapping for all markdown files.
///
/// This pre-scans frontmatter to compute final url_paths including
/// cascading url overrides from folder index files. The returned HashMap
/// maps source file paths (relative, e.g. "posts/hello.md") to their
/// final url_paths (e.g. "posts/hello/index.html").
///
/// The page_map is passed to `process_markdown_file()` so each file can
/// look up its pre-computed url_path instead of computing it inline.
///
/// Production now calls [`build_page_map_and_external_urls_cached`] instead
/// (same logic, cached, and merged with the external-url scan into one
/// read+parse pass). This uncached form is kept `#[cfg(test)]` as the
/// differential-test oracle the cached path is checked against, plus the
/// pre-existing tests that exercise it directly.
#[cfg(test)]
pub(crate) fn build_page_map(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
) -> (std::collections::HashMap<String, String>, std::collections::HashMap<String, String>) {
    build_page_map_with_evicted(
        markdown_files,
        source_path,
        root_folder_name,
        home_file_winners,
        home_overrides,
        &crate::build::icloud::is_evicted,
    )
}

/// Same as [`build_page_map`] with an injectable eviction predicate — see
/// docs/archive/2026-07-31-cloud-download-waiting-mode.md Stage 3.
#[cfg(test)]
pub(crate) fn build_page_map_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> (std::collections::HashMap<String, String>, std::collections::HashMap<String, String>) {
    use std::collections::HashMap;

    // Phase 1: Compute initial url_path for each file and collect dir overrides
    let mut entries: Vec<(String, String, bool)> = Vec::new();
    let mut dir_overrides: HashMap<String, String> = HashMap::new();

    for file_info in markdown_files {
        let file_path = &file_info.path;
        let source_file_path = source_path.join(file_path);

        // Cloud-dataless source: skip like a read error rather than blocking
        // this serial, single-threaded scan on materialization.
        if is_evicted(&source_file_path) {
            crate::build::cloud_readiness::request_download(&source_file_path);
            continue;
        }

        // Read and parse frontmatter only
        let content = match std::fs::read_to_string(&source_file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("url map skipping unreadable '{}': {}", file_path, e);
                continue;
            }
        };

        // Parse frontmatter to get url override (ONE parser, ADR-020).
        let frontmatter_url: Option<String> = if crate::build::markdown::is_simplified_frontmatter(&content) {
            let (fm, _) = crate::build::markdown::parse_simplified_frontmatter(&content);
            fm.url
        } else {
            crate::build::markdown::parse_typed_frontmatter(&content).url
        };

        let (url_path, is_index, dir_override) = page_map_entry(
            file_path,
            frontmatter_url.as_deref(),
            root_folder_name,
            home_file_winners,
            home_overrides,
        );
        if let Some((dir, slug)) = dir_override {
            dir_overrides.insert(dir, slug);
        }

        entries.push((file_path.to_string(), url_path, is_index));
    }

    apply_cascading_dir_overrides(&mut entries, &dir_overrides);

    let page_map = entries.into_iter().map(|(k, v, _)| (k, v)).collect();
    (page_map, dir_overrides)
}

/// The per-file half of [`build_page_map_with_evicted`]'s Phase 1: given a
/// file's already-extracted `url:` frontmatter override, computes its
/// `(url_path, is_index)` and, for a winning index file with a `url:`
/// override, the `(dir, slug)` pair to fold into `dir_overrides`.
///
/// Factored out so [`build_page_map_and_external_urls_cached_with_evicted`]
/// can share it: that path gets `frontmatter_url` from a cache instead of a
/// fresh read, but everything downstream of the extraction is identical.
pub(super) fn page_map_entry(
    file_path: &str,
    frontmatter_url: Option<&str>,
    root_folder_name: &str,
    home_file_winners: &std::collections::HashSet<String>,
    home_overrides: &std::collections::HashMap<String, String>,
) -> (String, bool, Option<(String, String)>) {
    // Determine is_index using same logic as process_markdown_file
    let filename_lower = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let parent_name = if file_path.contains('/') {
        Path::new(file_path)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
    } else {
        root_folder_name
    };
    let filename_match_is_home = moss_core::home::is_home_file(&filename_lower, parent_name);
    let mut is_index = filename_match_is_home;

    // Demote non-winner index files (same logic as in generate_blocking_content)
    let is_home_translation = if filename_match_is_home && !home_file_winners.contains(file_path) {
        // Check if this is a language-suffixed variant of an index stem
        // (e.g., index.zh-hans.md alongside the winning index.md).
        // These should keep the winner's URL so resolve_duplicate_slugs_with_lang
        // can properly deduplicate them, rather than getting a directory URL.
        let has_lang_suffix = moss_core::home::strip_lang_suffix(&filename_lower).is_some();
        is_index = false;
        has_lang_suffix
    } else {
        false
    };

    // Promote home-override files (the `home: true` marker) — files
    // that won the home slot via frontmatter, not filename. These need
    // to go to `<folder>/index.html` so the folder home is the file's
    // URL, not a synthesized empty page (issue #587).
    let is_home_override = home_overrides.values().any(|p| p == file_path);
    if !filename_match_is_home && is_home_override {
        is_index = true;
    }

    // Derive clean_stem: strip extension, take last segment
    let filename_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled");
    // Pre-scan only needs the clean stem (strips any language suffix).
    // The doc's resolved language is computed later in the markdown
    // pipeline where the full priority chain matters.
    let clean_stem = crate::i18n::clean_stem_only(filename_stem);

    // For language-suffixed home translations, compute the URL as if still
    // an index file so it collides with the winner and gets deduplicated.
    let url_path = crate::build::markdown::compute_url_path(
        file_path,
        is_index || is_home_translation,
        frontmatter_url,
        &clean_stem,
    );

    // Collect url override for index files (for cascading to children)
    let dir_override = if is_index {
        frontmatter_url.and_then(|url_override| {
            let dir = Path::new(file_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if dir.is_empty() {
                None
            } else {
                Some((dir.to_string(), crate::build::markdown::generate_slug(url_override)))
            }
        })
    } else {
        None
    };

    (url_path, is_index, dir_override)
}

/// Phase 2 of [`build_page_map_with_evicted`]: rewrite every entry's
/// `url_path` prefix for cascading `dir_overrides`.
///
/// For non-index files: resolve source_dir through overrides, rewrite url_path prefix.
/// For index files: compute_url_path already replaced the OWN segment, but
///   ancestor segments still need rewriting (e.g. 文字/travel/index.html → writings/travel/index.html).
pub(super) fn apply_cascading_dir_overrides(
    entries: &mut [(String, String, bool)],
    dir_overrides: &std::collections::HashMap<String, String>,
) {
    if dir_overrides.is_empty() {
        return;
    }
    for (file_path, url_path, is_index) in entries.iter_mut() {
        let source_dir = Path::new(file_path.as_str())
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("");
        if source_dir.is_empty() {
            continue;
        }

        // Compare and rewrite using fully-slugified directory paths.
        //
        // resolve_path_with_overrides preserves the LAST segment verbatim
        // because most callers pass file paths whose last segment is a
        // filename. Phase 2 always passes directory-only paths, so we
        // re-slugify the result to drop any case in the trailing segment
        // (e.g. "my-section/Sub Section" → "my-section/sub-section").
        //
        // Comparing slug-form vs slug-form means we only enter the
        // strip-and-replace branch when an override actually changed the
        // path — not when slugification alone would. Without this, a
        // sibling Title-Case folder with no override would corrupt its
        // children's url_paths whenever ANY override exists in the site.
        if *is_index {
            let grandparent = Path::new(source_dir)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if grandparent.is_empty() {
                continue;
            }
            let new_grandparent_raw = resolve_path_with_overrides(grandparent, dir_overrides);
            let new_grandparent = slugify_path_segments(&new_grandparent_raw);
            let slugified_grandparent = slugify_path_segments(grandparent);
            if new_grandparent != slugified_grandparent {
                if let Some(rest) = url_path.strip_prefix(slugified_grandparent.as_str()) {
                    let rest = rest.trim_start_matches('/');
                    *url_path = format!("{}/{}", new_grandparent, rest);
                }
            }
        } else {
            let new_parent_raw = resolve_path_with_overrides(source_dir, dir_overrides);
            let new_parent = slugify_path_segments(&new_parent_raw);
            let slugified_source = slugify_path_segments(source_dir);
            if new_parent != slugified_source {
                if let Some(rest) = url_path.strip_prefix(slugified_source.as_str()) {
                    let rest = rest.trim_start_matches('/');
                    *url_path = if new_parent.is_empty() {
                        rest.to_string()
                    } else {
                        format!("{}/{}", new_parent, rest)
                    };
                }
            }
        }
    }
}

/// Pre-scans every markdown file's frontmatter for an `external_url:` field
/// and returns a `source_path → external_url` map of the entries that have
/// one set to an absolute http(s) URL.
///
/// Mirrors `build_page_map`'s pre-scan but extracts a different field. See
/// moss#679 (JSON Feed 1.1 linkblog pattern) for why `external_url:` exists.
///
/// Production now calls [`build_page_map_and_external_urls_cached`] instead,
/// which folds this scan into the same cached read+parse pass as
/// `build_page_map`. This uncached form is kept `#[cfg(test)]` as the
/// differential-test oracle the cached path is checked against, plus the
/// pre-existing tests that exercise it directly.
#[cfg(test)]
pub(crate) fn build_external_url_map(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
) -> std::collections::HashMap<String, String> {
    build_external_url_map_with_evicted(markdown_files, source_path, &crate::build::icloud::is_evicted)
}

/// Same as [`build_external_url_map`] with an injectable eviction predicate —
/// see docs/archive/2026-07-31-cloud-download-waiting-mode.md Stage 3.
#[cfg(test)]
pub(crate) fn build_external_url_map_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    source_path: &Path,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;

    let mut out: HashMap<String, String> = HashMap::new();

    for file_info in markdown_files {
        let file_path = &file_info.path;
        let source_file_path = source_path.join(file_path);
        if is_evicted(&source_file_path) {
            crate::build::cloud_readiness::request_download(&source_file_path);
            continue;
        }
        let content = match std::fs::read_to_string(&source_file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("external-url map skipping unreadable '{}': {}", file_path, e);
                continue;
            }
        };

        // ONE parser (ADR-020): same path the editor + build pipeline use.
        let external_url: Option<String> = if crate::build::markdown::is_simplified_frontmatter(&content) {
            let (fm, _) = crate::build::markdown::parse_simplified_frontmatter(&content);
            fm.external_url
        } else {
            crate::build::markdown::parse_typed_frontmatter(&content).external_url
        };

        if let Some(url) = external_url {
            // http(s) only — same safety guard as the card-href substitution
            // in page.rs and the wikilink resolver in pipeline.rs.
            // Warn before silently dropping so the user knows why the field
            // is being ignored. See moss#684.
            warn_invalid_external_url(&url, file_path);
            if is_valid_external_url(&url) {
                out.insert(file_path.clone(), url);
            }
        }
    }

    out
}

/// Returns `true` when `url` is an absolute `http(s):` URL accepted as a valid
/// `external_url` value.  Any other non-empty string (relative path, `ftp://`,
/// `javascript:`, `data:`, …) is considered invalid and should trigger a
/// warning — see `warn_invalid_external_url`.
///
/// Pure predicate with no side effects; kept separate so it can be tested
/// independently of the logger. See moss#684.
pub(crate) fn is_valid_external_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Emits a `log::warn!` when `raw_url` is set but is not an absolute http(s)
/// URL.  `file_path` is used only for the diagnostic message.
///
/// Call this before the http(s) guard that silently drops the value so the
/// user learns *why* the field is being ignored. See moss#684.
pub(super) fn warn_invalid_external_url(raw_url: &str, file_path: &str) {
    if !raw_url.is_empty() && !is_valid_external_url(raw_url) {
        log::warn!(
            target: "frontmatter",
            "{}: `external_url: {:?}` is not an absolute http(s) URL and will be ignored. \
             Use a full URL starting with https:// or http://.",
            file_path,
            raw_url,
        );
    }
}

/// Read `external_url:` from a doc's raw frontmatter, returning it ONLY when
/// it's an absolute `http(s):` URL. Anything else (relative path, `javascript:`,
/// `data:`, empty) returns `None` — that's the single safety guard for every
/// site that consumes this field (card href, canonical link, sitemap, RSS).
///
/// Emits a build warning when the field is set to a non-http(s) value so the
/// user is informed rather than silently ignored. See moss#684.
///
/// Centralized here so future call sites can't accidentally weaken the guard.
/// See moss#679.
pub fn external_url(
    raw_frontmatter: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    let raw = raw_frontmatter
        .get("external_url")
        .and_then(|v| v.as_str())?;
    if !is_valid_external_url(raw) {
        warn_invalid_external_url(raw, "<unknown>");
        return None;
    }
    Some(raw.to_string())
}

/// Read `publisher:` from a doc's raw frontmatter as the card kicker source.
/// Returns the publisher string when it's a non-empty YAML string, `None`
/// otherwise (missing, empty, or a non-string YAML value like a number/list).
///
/// Centralized so the three child-summary/grid-card emitters share one
/// extractor — matches the `external_url` pattern above.
pub fn publisher(
    raw_frontmatter: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    raw_frontmatter
        .get("publisher")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
#[path = "page_map_tests.rs"]
mod tests;
