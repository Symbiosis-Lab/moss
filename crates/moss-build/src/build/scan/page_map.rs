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

use crate::build::scan::path_collision::on_disk;


mod frontmatter_cache;
pub(crate) use frontmatter_cache::{build_page_map_and_external_urls_cached, FrontmatterScanCache};
#[cfg(test)]
pub(crate) use frontmatter_cache::build_page_map_and_external_urls_cached_with_evicted;

mod home_election;
pub(crate) use home_election::{elect_homes, elect_homes_with_evicted, is_home_translation, root_language};

mod folder_lang;
pub(crate) use folder_lang::{folder_of, resolve_folder_languages, FolderLangCache};
pub(crate) use moss_core::page::page_map::{apply_cascading_dir_overrides, folder_homes, page_map_entry};

/// Pre-scan markdown frontmatter to elect each folder's home file, returning a
/// map from `parent_directory → file_path`. A file in this map wins its
/// folder's home slot regardless of filename — see [`compute_home_file_winners`].
///
/// This is the supported way to make `en/Mountain Home.md` (or any other
/// non-INDEX_STEM, non-self-named filename) the homepage of a folder.
/// Without this, moss falls back to the filename-only detection
/// in [`moss_core::home::detect_home_file_in_folder`] and the file lands
/// at a slug-based URL (`en/mountain-home/`) while moss synthesizes an empty
/// `en/index.html` titled `"En"`.
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
    let source_path = root.path();
    let is_evicted = &crate::build::icloud::is_evicted_and_requested;
    compute_home_overrides_with_evicted(markdown_files, root, &|p| on_disk(source_path, p), is_evicted)
}

/// Same as [`compute_home_overrides`] with an injectable eviction predicate,
/// and each file read from `locate(path)` (the path under the site folder for
/// every build). A dataless (cloud-evicted) source file is skipped exactly
/// like a read error (`Err(_) => continue`, just below) rather than blocking
/// this serial, single-threaded scan on the OS materializing it.
pub(crate) fn compute_home_overrides_with_evicted(
    markdown_files: &[crate::types::content::FileInfo],
    root: &crate::vault::paths::VaultRoot,
    locate: &dyn Fn(&str) -> std::path::PathBuf,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> std::collections::HashMap<String, String> {
    let mut read: Vec<(String, Option<bool>, Option<String>)> = Vec::new();
    for fi in markdown_files {
        let abs = locate(&fi.path);
        if is_evicted(&abs) {
            continue;
        }
        let content = match std::fs::read_to_string(&abs) {
            Ok(c) => c,
            // The is_evicted gate above catches the common case; this catches
            // the race, and anything else. Dropping a page from the site map
            // is too consequential to do without saying so.
            Err(e) => {
                log::info!("page map skipping unreadable '{}': {}", fi.path, e);
                continue;
            }
        };
        let (home, translation_key) = if crate::build::markdown::is_simplified_frontmatter(&content) {
            let (fm, _) = crate::build::markdown::parse_simplified_frontmatter(&content);
            (fm.home, fm.translation_key)
        } else {
            let fm = crate::build::markdown::parse_typed_frontmatter(&content);
            (fm.home, fm.translation_key)
        };
        read.push((fi.path.clone(), home, translation_key));
    }
    let claims: Vec<moss_core::page::page_map::HomeClaim<'_>> = read.iter()
        .map(|(path, home, key)| moss_core::page::page_map::HomeClaim { path, home: *home, translation_key: key.as_deref() })
        .collect();
    let (overrides, notices) = moss_core::page::page_map::home_overrides(&claims, root.name());
    for notice in notices {
        if notice.warn {
            log::warn!(target: "i18n", "{}", notice.text);
        } else {
            log::debug!(target: "i18n", "{}", notice.text);
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
    moss_core::page::page_map::home_file_winners(markdown_files.iter().map(|f| f.path.as_str()), root_folder_name, home_overrides)
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
/// raw source paths, so this function never sees slugified input. Pass 2
/// of `build_page_map` slugifies the prefix before stripping it off
/// `url_path` (see `slugify_path_segments`), because by the time pass 2
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
/// — wrap it in [`moss_core::slug::slugify_path_segments`] first. `build_page_map` pass 2
/// is the one such caller in moss today and applies that wrap. Any new
/// directory-only call site must do the same; otherwise a Title-Case last
/// segment will leak back into URLs whenever an unrelated folder in the
/// same site has a `url:` override (which makes pass 2 fire).
///
/// # Examples
///
/// Given `dir_overrides = {"视频" → "video"}`:
/// - `"视频/aimeili.mov"` → `"video/aimeili.mov"`
/// - `"视频/sub/clip.mp4"` → `"video/sub/clip.mp4"`
/// - `"News/new-hub.png"` (no override) → `"news/new-hub.png"`
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
        &crate::build::icloud::is_evicted_and_requested,
    )
}

/// Same as [`build_page_map`] with an injectable eviction predicate.
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

    // Compute initial url_path for each file and collect dir overrides
    let mut entries: Vec<(String, String, bool)> = Vec::new();
    let mut dir_overrides: HashMap<String, String> = HashMap::new();

    for file_info in markdown_files {
        let file_path = &file_info.path;
        let source_file_path = on_disk(source_path, file_path);

        // Cloud-dataless source: skip like a read error rather than blocking
        // this serial, single-threaded scan on materialization.
        if is_evicted(&source_file_path) {
            continue;
        }

        // Read and parse frontmatter only
        let content = match std::fs::read_to_string(&source_file_path) {
            Ok(c) => c,
            Err(e) => {
                log::info!("url map skipping unreadable '{}': {}", file_path, e);
                continue;
            }
        };

        // Parse frontmatter to get url override (ONE parser).
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

/// The per-file half of [`build_page_map_with_evicted`]'s pass 1: given a
/// file's already-extracted `url:` frontmatter override, computes its
/// `(url_path, is_index)` and, for a winning index file with a `url:`
/// override, the `(dir, slug)` pair to fold into `dir_overrides`.
///
/// Every page of `markdown_files` as slug deduplication
/// ([`crate::build::markdown::resolve_duplicate_slugs_with_lang`]) receives it,
/// in `markdown_files` order: the address [`page_map_entry`] gives it, whether
/// it is its folder's home, its language as `process_markdown_file` resolves
/// it, and whether it fills a slot instead of being a page. Each file is read
/// from `locate(path)` instead of from `path` under the site folder, so this
/// answers for a tree whose files have not been moved there yet. It reads the
/// build's folder-language cache and writes no cache.
pub(crate) fn pages_before_dedup(
    markdown_files: &[crate::types::content::FileInfo],
    root: &crate::vault::paths::VaultRoot,
    reserved: &std::collections::HashSet<String>,
    site: &crate::i18n::detect::SiteLang,
    locate: &dyn Fn(&str) -> std::path::PathBuf,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> Vec<crate::build::types::ParsedDocument> {
    let (overrides, winners) = elect_homes_with_evicted(markdown_files, root, reserved, locate, is_evicted);
    let mut scan = FrontmatterScanCache::default();
    let homes = folder_homes(&winners, &overrides, root.name());
    let members_config = crate::build::scan::members::config_declares_members(root.path());
    let scanned = frontmatter_cache::scan_frontmatter_urls_with_evicted(markdown_files, &homes, locate, &mut scan, is_evicted, members_config);
    let (entries, _, _) = frontmatter_cache::assemble_page_map(markdown_files, &scanned, root.name(), &winners, &overrides);
    let mut folder_cache = FolderLangCache::load(&crate::moss_paths::MossPaths::new(root.path()).cache_folder_lang());
    let folder_langs =
        folder_lang::resolve_folder_languages_with(markdown_files, locate, is_evicted, &mut folder_cache, &scan, site.declared);

    let mut pages = Vec::new();
    for (path, url_path, is_index) in entries {
        let stem = Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("untitled");
        let slot_only = crate::build::footer::is_slot_only(&path, crate::build::footer::ShipFacts { partial: scan.is_partial(&path) });
        let folder = crate::i18n::folder_language(&path, folder_langs.get(&folder_of(&path)).copied(), slot_only);
        let (lang, _) = crate::i18n::resolve_document_language(scan.declared_lang(&path), stem, site.language(), folder);
        let page = crate::build::types::ParsedDocument {
            url_path,
            kind: if is_index { moss_core::PageKind::Folder } else { moss_core::PageKind::Article },
            lang,
            slot_only,
            source_path: Some(path),
            ..Default::default()
        };
        crate::build::scan::slug::admit_unless_reserved_device_output(&mut pages, page);
    }
    pages
}

/// Pre-scans every markdown file's frontmatter for an `external_url:` field
/// and returns a `source_path → external_url` map of the entries that have
/// one set to an absolute http(s) URL.
///
/// Mirrors `build_page_map`'s pre-scan but extracts a different field
/// (JSON Feed 1.1 linkblog pattern) for why `external_url:` exists.
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
    build_external_url_map_with_evicted(markdown_files, source_path, &crate::build::icloud::is_evicted_and_requested)
}

/// Same as [`build_external_url_map`] with an injectable eviction predicate.
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
        let source_file_path = on_disk(source_path, file_path);
        if is_evicted(&source_file_path) {
            continue;
        }
        let content = match std::fs::read_to_string(&source_file_path) {
            Ok(c) => c,
            Err(e) => {
                log::info!("external-url map skipping unreadable '{}': {}", file_path, e);
                continue;
            }
        };

        // ONE parser: same path the editor + build pipeline use.
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
            // is being ignored.
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
/// independently of the logger.
pub(crate) fn is_valid_external_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Returns `true` when `url` can be a nav bar entry's target: an absolute
/// http(s) URL, `mailto:`, `tel:`, an in-page `#anchor`, or a site path
/// (`/path`, `/path#anchor`). A wider set than [`is_valid_external_url`] on
/// purpose — a nav item may point at an address or a section, but a card,
/// canonical link or sitemap entry may not. `javascript:`, `data:` and
/// protocol-relative `//host` stay out.
pub(crate) fn is_valid_nav_target(url: &str) -> bool {
    is_valid_external_url(url)
        || url.starts_with("mailto:")
        || url.starts_with("tel:")
        || url.starts_with('#')
        || (url.starts_with('/') && !url.starts_with("//"))
}

/// A page's `external_url:` as its nav bar entry's target, or `None` when
/// the page has none (or it is not a [`is_valid_nav_target`]), in which case
/// the entry links to the page itself. Silent: the scan already warned.
pub fn nav_external_href(
    raw_frontmatter: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<&str> {
    raw_frontmatter
        .get("external_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|u| is_valid_nav_target(u))
}

/// Where a listing row or card for `doc` links instead of its local address: a
/// page-less entry's [`pageless_row_href`], else a page's [`external_url`].
pub fn outlet_href(doc: &crate::build::types::ParsedDocument) -> Option<String> {
    if doc.is_pageless_entry() {
        pageless_row_href(&doc.raw_frontmatter)
    } else {
        external_url(&doc.raw_frontmatter)
    }
}

/// Where a page-less listing row links: an http(s) outlet or a site path to a
/// file or folder the site serves (a PDF, a hand-made HTML folder), the same
/// targets a page-less nav entry takes. `None` draws the row unlinked.
pub fn pageless_row_href(
    raw_frontmatter: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    nav_external_href(raw_frontmatter)
        .filter(|u| is_valid_external_url(u) || (u.starts_with('/') && !u.starts_with("//")))
        .map(str::to_string)
}

/// Emits a `log::warn!` when `raw_url` is set but is not a value any
/// consumer of `external_url:` accepts (see [`is_valid_nav_target`]).
/// `file_path` is used only for the diagnostic message.
///
/// Call this before the http(s) guard that silently drops the value so the
/// user learns *why* the field is being ignored.
pub(super) fn warn_invalid_external_url(raw_url: &str, file_path: &str) {
    if !raw_url.is_empty() && !is_valid_nav_target(raw_url.trim()) {
        log::warn!(
            target: "frontmatter",
            "{}: `external_url: {:?}` is not an absolute http(s) URL and will be ignored. \
             Use a full URL starting with https://, or mailto:, tel:, #anchor or /path for a nav entry.",
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
/// user is informed rather than silently ignored.
///
/// Centralized here so future call sites can't accidentally weaken the guard.
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
