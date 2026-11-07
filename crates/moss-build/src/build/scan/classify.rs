//! Folder exclusion rules for static site generation.
//!
//! This module provides logic to determine which folders should be
//! excluded from content processing during site generation.

use std::collections::HashSet;
use std::path::Path;

use crate::types::content::FileInfo;
pub use moss_core::source_edit::policy::{
    HIDDEN_ENTRIES, MOSS_INTERNAL_ALLOWLIST, HiddenReason, is_hidden, is_hidden_reason,
    SpecialFileKind, special_file_kind, is_page_source, is_page_path,
    ROOT_AGENT_CONFIG_FILES, is_agent_config_name, FINDER_ICON_MARKER, is_os_junk_file,
    EXCLUDED_DIR_NAMES, is_excluded_dir_name, LeftOut, compute_passthrough_roots, is_in_passthrough,
};

/// The one rule for which files belong to the site: `Some` for an entry a walk
/// of the site folder must not read, saying why. The build's scan and every
/// editor walk that must see the files the build sees ask this, so a file the
/// build cannot link is never offered or found by the editor either.
pub fn left_out_of_site(entry: &walkdir::DirEntry) -> Option<LeftOut> {
    left_out(entry.path(), entry.file_type().is_dir(), entry.depth())
}

/// [`left_out_of_site`] that also leaves out the `layer_sources`, as absolute
/// folder paths spelled the way the walk spells them.
pub fn left_out_with_layer_sources(entry: &walkdir::DirEntry, layer_sources: &[std::path::PathBuf]) -> Option<LeftOut> {
    if entry.file_type().is_dir() && layer_sources.iter().any(|s| s == entry.path()) {
        return Some(LeftOut::LayerSource);
    }
    left_out_of_site(entry)
}

/// [`left_out_of_site`] for an entry at `path`, `depth` levels below the site
/// folder (a file directly in it is at depth 1), for a reader that lists
/// folders itself instead of walking with `walkdir`.
pub fn left_out(path: &Path, is_dir: bool, depth: usize) -> Option<LeftOut> {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    moss_core::source_edit::policy::left_out(&name, is_dir, depth, || path.join(".moss").is_dir())
}

/// Watch-time counterpart to [`left_out_of_site`]: is `abs` a root-level
/// agent-instruction file of the site at `root`?
///
/// A file the scan skips cannot change one byte of the built site, so a change
/// to one must never trigger a rebuild. That is load-bearing rather than merely
/// tidy: moss writes the root `AGENTS.md` itself when that setting is on, and
/// without this its own write would rebuild the whole site — during a build, in
/// the two watch paths that classify as `BuildTrigger::Full`.
///
/// `watch::evaluate_gate` consults this ahead of `MOSS_WATCH_NO_GATE`. That
/// switch exists to bisect stale-content reports against the content-hash gate;
/// this suppression is categorical, and re-enabling it would only restore the
/// self-triggered rebuild.
///
/// Both sides are canonicalized because the watcher reports canonical paths on
/// macOS (`/private/var`) while the caller may hold the symlinked spelling.
pub fn is_root_agent_config(root: &Path, abs: &Path) -> bool {
    let Some(name) = abs.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !is_agent_config_name(name) {
        return false;
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    abs.parent().is_some_and(|parent| canon(parent) == canon(root))
}

/// Whether a *file name* is OS metadata that must never reach a live site.
///
/// Single predicate for the asset walk and the file watcher, so a name can
/// never be excluded from one and not the other.
///
/// `Icon\r` is the case worth knowing about: its data fork is empty (the icon
/// lives in the resource fork), so letting it through hashes it to the
/// empty-blob digest, which the CAS rejects as corrupt — a `Failed to link`
/// warning plus a wasted cache miss on *every* build of *every* folder the user
/// has published. It is not dot-prefixed, so the `.`-prefix rule misses it.
pub fn is_os_metadata_file(name: &str) -> bool {
    // Dot-prefixed: .DS_Store, .gitignore, .gitkeep, … Slug-normalization also
    // rewrites these (.DS_Store → .ds-store), so the manifest entry stops
    // matching the on-disk name — a deploy-time ENOENT on case-sensitive
    // filesystems.
    name.starts_with('.') || is_os_junk_file(name)
}

/// Whether the scanned directory `relative_path` is a section of the site, and
/// therefore gets a synthetic folder-index page.
///
/// Two kinds do not. A passthrough subtree is a pre-built app that owns its own
/// `index.html`; seeding a synthetic doc for its root would register a
/// moss-listing hash colliding with the verbatim copy, and seeding nested dirs
/// would inject listing pages the app never had. An attachment folder is
/// storage — where the editor drops a page's images — so it is not a section:
/// no listing page, no listing row, no nav entry.
///
/// Asked once, in the scan (together with [`holds_only_prebuilt_apps`]), so `ProjectStructure.dirs` already means "the
/// directories that get an index page" by the time the render reads it.
///
/// This decides the index-page seed ONLY. The media pipelines run off
/// `image_files` / `video_files` / `other_files`, which the walk still fills
/// from inside these directories — deliberately, because excluding asset
/// folders from the WALK is the bug `EXCLUDED_DIR_NAMES` was emptied to fix
/// (see the note on that constant): it dropped every variant and 404'd a
/// `cover:` pointing into `assets/`.
pub fn gets_index_page(
    relative_path: &str,
    passthrough_roots: &HashSet<String>,
    attachment_folder: &str,
) -> bool {
    // The trailing `/` lets the root dir itself match the trailing-slash-
    // normalised `passthrough_roots`, not just its descendants (see
    // `compute_passthrough_roots`).
    !is_in_passthrough(&format!("{relative_path}/"), passthrough_roots)
        && !moss_core::attachment::is_attachment_dir(attachment_folder, relative_path)
}

/// Whether `dir` only carries pre-built apps: a passthrough subtree lives
/// beneath it and no page (`page_paths`: markdown and notebooks) does. Such a
/// folder has nothing of its own to list — the app is the content — so it gets
/// no index page. Exact-file passthrough roots (no trailing `/`) are not apps
/// beneath a folder and do not count.
pub fn holds_only_prebuilt_apps(
    dir: &str,
    passthrough_roots: &HashSet<String>,
    page_paths: &[&str],
) -> bool {
    let prefix = format!("{dir}/");
    passthrough_roots.iter().any(|r| r.ends_with('/') && r.starts_with(&prefix))
        && !page_paths.iter().any(|p| p.starts_with(&prefix))
}

/// The site-root `assets/` folder and every folder beneath it, while no page
/// (`page_paths`: markdown and notebooks) lives beneath `dir`. Files kept
/// there (a favicon, imported pictures and documents) are storage, not a
/// section, so the folder gets no index page. A page beneath it makes it a
/// section like any other. The exact lowercase name is the documented one;
/// `Assets/` stays an ordinary folder.
pub fn holds_only_storage(dir: &str, page_paths: &[&str]) -> bool {
    let is_storage = dir == "assets" || dir.starts_with("assets/");
    let prefix = format!("{dir}/");
    is_storage && !page_paths.iter().any(|p| p.starts_with(&prefix))
}

/// Directories whose `index.html` is copied verbatim: those holding a source
/// file of exactly that name inside a passthrough root. Such a directory has no
/// room for a page of its own at its folder address, so no markdown file or
/// generated listing may take it.
///
/// Keys are source-relative directory paths without a trailing `/`.
pub fn verbatim_index_dirs(html_files: &[FileInfo], passthrough_roots: &HashSet<String>) -> HashSet<String> {
    html_files
        .iter()
        .filter(|f| is_in_passthrough(&f.path, passthrough_roots))
        .filter_map(|f| f.path.strip_suffix("/index.html"))
        .map(str::to_string)
        .collect()
}

/// The folders whose markdown cannot be the home: [`verbatim_index_dirs`]
/// except a language tree's root whose `index.html` is named exactly in
/// `[build].passthrough`. That pair behaves like the site root's: the
/// `index.md` stays the home and is the document's metadata.
pub fn reserved_home_dirs(html_files: &[FileInfo], passthrough_roots: &HashSet<String>) -> HashSet<String> {
    let mut dirs = verbatim_index_dirs(html_files, passthrough_roots);
    dirs.retain(|d| !(moss_core::home::is_known_language_code(d) && passthrough_roots.contains(&format!("{d}/{INDEX_HTML}"))));
    dirs
}

/// Each scanned directory that gets a folder-index page, paired with the
/// page-tree key that index lives at.
///
/// Appending `/index.html` runs every directory segment through the same
/// slug/override resolver the page-tree keys use and then drops the leaf,
/// which is what makes these keys comparable with the doc-derived prefixes.
/// A directory mapping to the empty key is skipped — the site's homepage is
/// not a folder index. A directory under a language-prefix root (bare, or
/// nested — `zh-hans/`, `zh-hans/docs/`) is skipped too, by the same
/// `resolve_language_from_folder` predicate the render's own auto-index loop
/// applies to its combined folder set (`render/blocking.rs`): a folder under
/// a language tree never gets an auto-generated index there, so admitting it
/// here would hand a caller a key the render never emits a page for.
///
/// `dirs` is already narrowed by the scan to the directories that should have
/// a page at all (`gets_index_page`, above); the reserved-device-name filter
/// lives in `slug.rs`. This is the one place the source-directory-to-key
/// mapping is written — its callers (the render's two folder-index synthesis
/// blocks, plus `build_article_map`'s `folder_indexes` field and the content
/// graph's auto-index registration) all read it from here so they can never
/// drift from each other or from what the renderer actually emits.
pub(crate) fn folder_index_keys<'a>(
    dirs: &'a [String],
    dir_overrides: &'a std::collections::HashMap<String, String>,
) -> impl Iterator<Item = (&'a String, String)> + 'a {
    dirs.iter().filter_map(move |dir| {
        let top_segment = dir.split('/').next().unwrap_or(dir);
        if crate::i18n::path::resolve_language_from_folder(top_segment).is_some() {
            return None;
        }
        let mapped = crate::build::scan::page_map::resolve_path_with_overrides(
            &format!("{dir}/index.html"),
            dir_overrides,
        );
        let key = mapped
            .strip_suffix("/index.html")
            .filter(|_| !crate::build::scan::slug::warn_reserved_folder(dir, &mapped))?;
        (!key.is_empty()).then(|| (dir, key.to_string()))
    })
}

/// Register every directory that gets an auto-generated folder-index page on
/// a [`moss_core::content_graph::ContentGraphBuilder`], so `resolve_path`'s
/// folder-note fallback can answer a bare top-level `[[Folder]]` wikilink.
/// Reuses [`folder_index_keys`] — the same function the render loop calls to
/// decide which folders actually get a synthetic index — so this can never
/// register a folder the renderer does not generate a page for.
///
/// An empty override map is safe here: overrides only rename a folder's URL,
/// which `pinned_url` applies later once the real `dir_overrides` is
/// installed (`with_output_overrides`); they never add or remove which
/// directories are eligible. Only the computed URL half of each pair is
/// discarded — `resolve_path` recomputes it via `pinned_url` at lookup time,
/// so a stale unoverridden URL is never cached into the graph.
pub(crate) fn register_auto_index_dirs(
    builder: &mut moss_core::content_graph::ContentGraphBuilder,
    dirs: &[String],
) {
    for (dir, _url_key) in folder_index_keys(dirs, &std::collections::HashMap::new()) {
        builder.register_auto_index_dir(dir);
    }
}

// ── Not the site's content: the shared tree/watch filter ───────────────────
//
// `is_hidden` answers "is this entry the author's material, or moss's own?"
// Two callers need the SAME answer or they disagree about what a project
// contains: the editor's file tree (which lists it) and the watcher's
// RawFileCreated flash (which must not point at a file the tree will not
// show). It lives here, with the other classification rules.

/// Whether a folder-relative path is one of moss's own files: `.moss` itself
/// or anything inside it (in any letter case, since a folder's file system may
/// ignore case) that is neither on `MOSS_INTERNAL_ALLOWLIST` nor below an
/// entry of it. `.moss` itself counts, so the folder is never copied, renamed
/// or moved into as a whole.
pub fn is_moss_internal(relative: &Path) -> bool {
    let full = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/");
    if full != ".moss" && !full.starts_with(".moss/") {
        return false;
    }
    !MOSS_INTERNAL_ALLOWLIST.iter().any(|allowed| {
        full == *allowed || full.starts_with(&format!("{allowed}/"))
    })
}

/// The name of the HTML file a passthrough entry can pair a page's settings with.
const INDEX_HTML: &str = "index.html";

/// Folders (project-relative, `""` for the root) whose `index.md` is the
/// settings of a copied-as-is `index.html` that `[build].passthrough` names
/// exactly. Read once per tree build.
///
/// Decided by the build's own passthrough functions over the config-named
/// files that exist, so it cannot disagree with the build: the root and a
/// language tree's root qualify (their `index.md` stays the home), a deeper
/// folder does not (its `index.md` is evicted from the home election and is
/// an ordinary page at its own address).
pub fn page_settings_dirs(project_path: &str) -> HashSet<String> {
    let config = crate::build::site_config::get_build_passthrough(project_path).unwrap_or_default();
    let existing: Vec<&str> = config.iter()
        .filter(|e| !e.starts_with('!'))
        .map(|e| e.trim_matches('/'))
        .filter(|p| Path::new(project_path).join(p).is_file())
        .collect();
    moss_core::source_edit::policy::page_settings_dirs(&config, &existing)
}

/// Which bucket of `ProjectStructure` a file extension belongs to — the pure,
/// I/O-free half of `scan_folder`'s per-file classification (`scan.rs`).
/// Frontmatter parsing, ffprobe dimensions, and cache lookups all happen
/// *after* a file has already been placed in a bucket; none of that decides
/// which bucket it goes in, and none of it belongs here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanBucket {
    /// Markdown-family page sources — see [`is_page_source`].
    Page,
    Html,
    /// Raster images the scan extracts placeholder metadata for.
    Image,
    /// Video files the scan probes via ffmpeg for dimensions.
    Video,
    /// Jupyter notebooks.
    Notebook,
    /// Word-processor documents (`.pages`, `.docx`, `.doc`).
    Document,
    /// Everything else. Still scanned — the build copies it through as an
    /// asset — just not one of the categories above.
    Other,
}

/// Classify a lowercased file extension the way `scan_folder` does.
///
/// `scan.rs` calls this directly, so the scan cannot drift from its own
/// classifier. It is `pub` for the same reason [`is_page_source`] is: a
/// caller outside this crate that needs to know whether a path is one the
/// scan would treat as site content — rather than re-deriving its own
/// extension list and drifting from the scan the way the file-watch sweep's
/// hand-maintained mirror once did — calls this one instead.
pub fn classify_extension(extension: &str) -> ScanBucket {
    if is_page_source(extension) {
        return ScanBucket::Page;
    }
    match extension {
        "html" | "htm" => ScanBucket::Html,
        "jpg" | "jpeg" | "png" | "gif" | "svg" | "webp" | "avif" => ScanBucket::Image,
        "mov" | "mp4" | "webm" | "avi" | "mkv" | "m4v" => ScanBucket::Video,
        "ipynb" => ScanBucket::Notebook,
        "pages" | "docx" | "doc" => ScanBucket::Document,
        _ => ScanBucket::Other,
    }
}

/// Every folder (site-relative, `/`-separated) above some file the scan kept.
/// A symlink counts, because the asset copy recreates it in the output; OS
/// metadata (`.DS_Store`, Finder's `Icon\r`) does not, as in the asset walk.
/// An iCloud placeholder (`.name.icloud`) counts: it stands for a real file
/// still in the cloud, though its dot-name would read as OS metadata.
pub fn folders_holding_files<'a>(files: impl Iterator<Item = &'a Path>, root: &Path) -> HashSet<String> {
    let mut holding = HashSet::new();
    for file in files {
        let is_stub = crate::build::icloud::icloud_stub_target(file).is_some();
        if !is_stub && file.file_name().and_then(|n| n.to_str()).is_some_and(is_os_metadata_file) {
            continue;
        }
        let Ok(rel) = file.strip_prefix(root) else { continue };
        let rel = moss_core::slug::normalize_separators(&rel.to_string_lossy());
        for (i, _) in rel.match_indices('/') {
            holding.insert(rel[..i].to_string());
        }
    }
    holding
}

/// The folders among `dirs` with no file beneath them, keeping only the
/// topmost of a nested run, so `a/` is named once rather than `a/` and `a/b/`.
pub fn topmost_empty_folders(dirs: &[String], holding_files: &HashSet<String>) -> Vec<String> {
    let empty: Vec<&String> = dirs.iter().filter(|d| !holding_files.contains(*d)).collect();
    empty
        .iter()
        .filter(|d| !empty.iter().any(|p| d.len() > p.len() && d.starts_with(p.as_str()) && d.as_bytes()[p.len()] == b'/'))
        .map(|d| (*d).clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matching_excluded_dirs() {
        // assets/ and images/ are now treated as ordinary content folders so
        // that media files in them (e.g. assets/cover.MOV referenced via a
        // wikilink) reach the variant pipeline.
        assert!(!is_excluded_dir_name("assets"));
        assert!(!is_excluded_dir_name("images"));
        assert!(!is_excluded_dir_name("static"));
        assert!(!is_excluded_dir_name("public"));
        assert!(!is_excluded_dir_name("css"));
        assert!(!is_excluded_dir_name("js"));
        assert!(!is_excluded_dir_name("fonts"));
        // Underscore-prefixed names are no longer excluded — Lightroom-exported
        // photos (`_43A2045.jpg`) and Jekyll-style `_includes/` are content too.
        assert!(!is_excluded_dir_name("_drafts"));
        assert!(!is_excluded_dir_name("_hidden"));
        assert!(!is_excluded_dir_name("_includes"));
        assert!(!is_excluded_dir_name("_43A2045.jpg"));
        // Dot-prefixed names remain excluded — these are genuinely hidden.
        assert!(is_excluded_dir_name(".git"));
        assert!(is_excluded_dir_name(".moss"));
        assert!(is_excluded_dir_name(".DS_Store"));
        assert!(!is_excluded_dir_name("posts"));
        assert!(!is_excluded_dir_name("docs"));
        // Special case: . and .. are navigation, not hidden folders
        assert!(!is_excluded_dir_name("."));
        assert!(!is_excluded_dir_name(".."));
        // node_modules stays excluded — usually huge and never user content
        assert!(is_excluded_dir_name("node_modules"));
    }

    /// Pins `classify_extension` against every extension `scan.rs`'s own
    /// match block recognizes, so a bucket added there and forgotten here (or
    /// the reverse) fails immediately rather than silently drifting the way
    /// the sweep's hand-maintained mirror once did. An extension
    /// outside all of these is `Other` — the scan still consumes it, just
    /// uncategorized — which is why the fallback cases below assert `Other`
    /// rather than being left unchecked.
    #[test]
    fn classify_extension_pins_every_bucket_the_scan_recognizes() {
        for ext in ["md", "markdown", "mdown", "mkd"] {
            assert_eq!(classify_extension(ext), ScanBucket::Page, "{ext}");
        }
        for ext in ["html", "htm"] {
            assert_eq!(classify_extension(ext), ScanBucket::Html, "{ext}");
        }
        for ext in ["jpg", "jpeg", "png", "gif", "svg", "webp", "avif"] {
            assert_eq!(classify_extension(ext), ScanBucket::Image, "{ext}");
        }
        for ext in ["mov", "mp4", "webm", "avi", "mkv", "m4v"] {
            assert_eq!(classify_extension(ext), ScanBucket::Video, "{ext}");
        }
        assert_eq!(classify_extension("ipynb"), ScanBucket::Notebook);
        for ext in ["pages", "docx", "doc"] {
            assert_eq!(classify_extension(ext), ScanBucket::Document, "{ext}");
        }
        for ext in ["css", "js", "woff2", "pdf", "mp3", "unknownext", ""] {
            assert_eq!(classify_extension(ext), ScanBucket::Other, "{ext}");
        }
    }

    fn site_with_passthrough(files: &[&str], passthrough: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let path = dir.path().join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        std::fs::write(dir.path().join(".moss/config.toml"), format!("[build]\npassthrough = {passthrough}\n")).unwrap();
        dir
    }

    #[test]
    fn page_settings_dirs_pair_exactly_named_index_html_with_the_root_and_language_roots() {
        let site = site_with_passthrough(
            &["index.html", "fr/index.html", "docs/index.html", "lab/index.html"],
            r#"["index.html", "fr/index.html", "docs/index.html"]"#,
        );
        let dirs = page_settings_dirs(site.path().to_str().unwrap());
        // `docs/index.md` is evicted from the home election by the build, so it
        // is an ordinary page; `lab/` is auto-detected but never named.
        assert_eq!(dirs, HashSet::from([String::new(), "fr".to_string()]));
    }

    #[test]
    fn an_undeclared_index_html_pairs_nothing() {
        let site = site_with_passthrough(&["index.html", "fr/index.html"], r#"["other"]"#);
        assert!(page_settings_dirs(site.path().to_str().unwrap()).is_empty());
    }

    #[test]
    fn a_negated_or_missing_entry_pairs_nothing() {
        let site = site_with_passthrough(&["fr/index.html"], r#"["!fr/index.html", "index.html"]"#);
        assert!(page_settings_dirs(site.path().to_str().unwrap()).is_empty());
    }
}
