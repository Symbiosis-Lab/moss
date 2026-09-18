//! Folder exclusion rules for static site generation.
//!
//! This module provides logic to determine which folders should be
//! excluded from content processing during site generation.

use std::collections::HashSet;
use std::path::Path;

use crate::types::content::FileInfo;

/// Directory names that are skipped entirely during scan and asset copy.
///
/// Historically this list also included `assets`, `images`, `static`, `public`,
/// `img`, `css`, `js`, `fonts` — the SSG convention (Jekyll/Hugo) for "static
/// asset" folders. That convention silently dropped media files in those folders
/// from the variant pipeline (no .mp4 transcode, no .webp variant), so a
/// `cover: ![[clip.MOV]]` referencing `assets/clip.MOV` would 404.
/// We now treat them as ordinary content directories: media gets variants,
/// raw HTML is copied as-is (no chrome injection), markdown becomes pages.
///
/// We also previously excluded any folder/file beginning with `_` to match
/// Jekyll's draft convention. That silently dropped Lightroom-exported photos
/// like `_43A2045.jpg` and Jekyll-style `_includes/` content from the build,
/// which surprised users more than it helped them. The `.` prefix rule
/// remains — it skips genuinely hidden files like `.git`, `.moss`, `.DS_Store`.
pub const EXCLUDED_DIR_NAMES: &[&str] = &[
    "node_modules",
];

/// Whether a *directory name* should be skipped during the WalkDir scan.
///
/// Callers must gate this on `entry.file_type().is_dir()` — `WalkDir`'s
/// `filter_entry` is entry-agnostic and would otherwise apply this predicate
/// to file entries, silently dropping any file matching one of these names
/// (the original cause of the `_43A2045.jpg` bug).
pub fn is_excluded_dir_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Special case: "." and ".." are navigation, not hidden folders
    if lower == "." || lower == ".." {
        return false;
    }
    // Hidden directories: .git, .moss, .DS_Store-as-dir, etc.
    if lower.starts_with('.') {
        return true;
    }
    EXCLUDED_DIR_NAMES.iter().any(|&n| n.to_lowercase() == lower)
}

/// Agent-instruction filenames that conventionally live at a project root.
///
/// Every coding agent that reads a root-anchored instruction file uses one of
/// these exact names — `AGENTS.md` (the cross-tool convention, Codex and Gemini
/// CLI), `CLAUDE.md`, `GEMINI.md`. They are tooling, not prose.
///
/// Verified 2026-08-03 on a real build: a root `AGENTS.md` published to
/// `/agents/index.html` with an OG image and an `llms.txt` entry, because the
/// scanner excluded only dot-prefixed entries and the filename is the title.
/// Any moss user running a coding agent in their folder was publishing its
/// instructions as a page.
pub const ROOT_AGENT_CONFIG_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "GEMINI.md"];

/// Whether a *file name* is one of the agent-instruction conventions.
///
/// Exact case, deliberately. Every agent convention spells these in caps;
/// matching case-insensitively would silently unpublish an author's
/// `Agents.md` essay, which is a worse failure than publishing a stray config
/// file. Root-ness is the caller's half of the test — see
/// [`skip_root_agent_config`].
pub fn is_agent_config_name(name: &str) -> bool {
    ROOT_AGENT_CONFIG_FILES.contains(&name)
}

/// Scan-time decision for a *file* entry: is this a root-level agent-instruction
/// file that must not become a page?
///
/// `depth` is `WalkDir`'s — the walk root is 0, so a file directly in the site
/// folder is 1. Root only, because `posts/agents.md` is an ordinary article
/// about agents and must keep publishing; only the source root is a location
/// tooling claims.
///
/// Logs on a match. The file is sitting in plain sight in the author's folder,
/// so its absence from the built site has to be explainable from the log.
pub fn skip_root_agent_config(name: &str, depth: usize) -> bool {
    if depth == 1 && is_agent_config_name(name) {
        log::info!("Skipping {name}: agent instructions are tooling, not a page");
        return true;
    }
    false
}

/// Watch-time counterpart to [`skip_root_agent_config`]: is `abs` a root-level
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

/// Finder's custom-icon carrier: the literal name `Icon` followed by a carriage
/// return. moss writes one into every folder it publishes
/// (`platform::stamp_published_folder`), so unlike most OS metadata this is a
/// file *moss itself* leaves in the user's source tree.
pub const FINDER_ICON_MARKER: &str = "Icon\r";

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

/// Files the OS drops into a folder it has merely looked at: Finder's icon
/// marker (which moss itself writes on every published folder) and its
/// thumbnail cache, Windows's thumbnail cache. Not the author's, so neither
/// the build nor the editor's file tree may show them — the scan asks through
/// [`is_os_metadata_file`], the tree through [`is_hidden`], and a name added
/// here vanishes from both. Until 2026-09-11 the tree kept its own copy of
/// this list, which was missing the icon marker: after the first publish a
/// file called `Icon` sat at the top of every vault.
pub fn is_os_junk_file(name: &str) -> bool {
    name == ".DS_Store" || name == FINDER_ICON_MARKER || name.eq_ignore_ascii_case("thumbs.db")
}

/// Compute the set of source-relative passthrough subtree roots.
///
/// A root is any non-root directory containing an `index.html` or `index.htm`.
/// Config entries (from `[build].passthrough`) can add directory roots or exact
/// scanned HTML files, and remove auto-detected roots (`!path` with `!` prefix).
/// Directory roots carry a trailing `/`; exact HTML files do not.
pub fn compute_passthrough_roots(html_files: &[FileInfo], config_entries: &[String]) -> HashSet<String> {
    let mut roots = HashSet::new();

    // Auto-detect: non-root directories that contain index.html / index.htm
    for file in html_files {
        let path = Path::new(&file.path);
        let is_index = path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("index.html") || n.eq_ignore_ascii_case("index.htm"))
            .unwrap_or(false);
        if !is_index {
            continue;
        }
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy();
            // Skip root: parent of a root-level file is "" or "."
            if parent_str.is_empty() || parent_str == "." {
                continue;
            }
            let key = format!("{}/", parent_str.trim_end_matches('/'));
            if !roots.contains(&key) {
                log::debug!(
                    "[scan] passthrough subtree auto-detected: {} (contains index.html; \
                     add \"!{}\" to [build].passthrough in .moss/config.toml to opt out)",
                    key,
                    key.trim_end_matches('/')
                );
            }
            roots.insert(key);
        }
    }

    // Apply config overrides. Normalize each entry the same way an on-disk
    // relative path would appear: no leading slash (paths are root-relative),
    // no trailing slash before we re-add our own. A leading slash in config
    // (e.g. "/app") would otherwise produce a root "/app/" that never matches
    // the slash-less relative paths from `strip_prefix`, silently dropping the
    // user's intent.
    for entry in config_entries {
        let (is_negation, raw) = match entry.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, entry.as_str()),
        };
        let normalized = raw.trim_matches('/');
        if normalized.is_empty() {
            // Empty or slash-only entry ("" / "/" / "!"): no meaningful root.
            continue;
        }
        let is_scanned_html_file = html_files.iter().any(|file| file.path == normalized);
        let key = if is_scanned_html_file {
            normalized.to_string()
        } else {
            format!("{}/", normalized)
        };
        if is_negation {
            roots.remove(&key);
        } else {
            roots.insert(key);
        }
    }

    roots
}

/// Whether `relative_path` is inside any passthrough subtree root.
///
/// Roots are stored with trailing `/` so this is an exact prefix match
/// that cannot accidentally match a sibling directory with a longer name
/// (e.g. root `app/` does not match `application/file.js`).
pub fn is_in_passthrough(relative_path: &str, passthrough_roots: &HashSet<String>) -> bool {
    passthrough_roots.iter().any(|root| {
        if root.ends_with('/') {
            relative_path.starts_with(root.as_str())
        } else {
            relative_path == root
        }
    })
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
/// Asked once, in the scan, so `ProjectStructure.dirs` already means "the
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

// ── Not the site's content: the shared tree/watch filter ───────────────────
//
// `is_hidden` answers "is this entry the author's material, or moss's own?"
// Three callers need the SAME answer or they disagree about what a project
// contains: the editor's file tree (which lists it), the watcher's
// RawFileCreated flash (which must not point at a file the tree will not
// show), and the scan's root agent-config rule above, which this predicate
// already delegates to. It lives here, with the other classification rules,
// because that is where its one hard case already lived.

/// Hidden entries that should not appear in the file tree.
pub const HIDDEN_ENTRIES: &[&str] = &[".git", "node_modules", "target"];

/// The `.moss/` paths the tree lists when `show_internal = true`, keyed the
/// way `is_hidden` sees them: project-root-relative, forward slashes. An
/// entry shows when it is one of these, sits below one, or is on the way to
/// one (`.moss/data` shows so `.moss/data/social` is reachable). Everything
/// else under `.moss/` (state.toml, build/, cache/, identity/, plugins/,
/// hashes.json, article-map.json, assets/) stays hidden.
///
/// Note: `assets/` is watcher-only (see `should_watch_moss_file` in
/// `build::watch::scope`, which reads `infra::moss_paths::MOSS_PATH_RULES`)
/// — it is build-managed media and not surfaced in the editor tree.
///
/// `data/` is listed for NOTHING. The 2026-09-04 writer audit asked of every
/// `.moss/` path who writes it and whether a hand edit survives, and only these
/// two answered "the user": `config.toml`, whose every in-app writer goes
/// through the format-preserving `infra::toml_rewrite` precisely because the
/// file is hand-written, and `theme/`, which nothing but the user ever writes.
/// `data/social/` was listed here until that audit and is not user-writable:
/// the background comment sync and the matters plugin re-serialize those files
/// wholesale, so an edit made in the editor survives only until the next sync,
/// and an autosave landing over a fresh sync is equally unguarded. The build
/// READING a directory is not the same question as the user writing it. Its
/// siblings were never listed for the same reason: `subscribers.csv` is a PII
/// list Settings edits with a real UI, `events*.json*` is the analytics stream,
/// and `email/drafts/` is auto-saved newsletter scratch.
/// See `docs/archive/2026-09-03-moss-folder-in-the-file-tree-audit-and-design.md`.
pub const MOSS_INTERNAL_ALLOWLIST: &[&str] = &[".moss/config.toml", ".moss/theme"];

/// Return true when an entry should be filtered out of the file tree.
///
/// `parent_relative` is the path of the containing directory relative to the
/// project root ("" for root, ".moss" when filtering children of `.moss/`).
pub fn is_hidden(name: &str, parent_relative: &str, show_internal: bool) -> bool {
    // Always hide these regardless of show_internal
    if HIDDEN_ENTRIES.contains(&name) || is_os_junk_file(name) {
        return true;
    }

    // At the project root, .moss is surfaced only when show_internal is on
    if parent_relative.is_empty() {
        if name == ".moss" {
            return !show_internal;
        }
        // All other dotfiles are hidden at the project root
        if name.starts_with('.') {
            return true;
        }
        // Agent-instruction files (AGENTS.md, CLAUDE.md, GEMINI.md) are the one
        // category that reads as prose and isn't. The scan already refuses to
        // publish them (`skip_root_agent_config`, above) — whoever wrote it,
        // it isn't the site's content, so it would otherwise sit in the tree
        // between `index.md` and `about.md`. Same standing as `.moss/`: present,
        // one toggle away.
        //
        // Hidden, not read-only. The version stamp is the consent boundary
        // (`cli::agents::sync::is_ours`), and editing the file is precisely how
        // an author takes it over — sealing it would leave no way to do that
        // from inside moss.
        if is_agent_config_name(name) {
            return !show_internal;
        }
        return false;
    }

    // Inside .moss/ (only reached with show_internal on, since `.moss` itself
    // is hidden otherwise): listed, below a listed path, or an ancestor of one.
    if parent_relative == ".moss" || parent_relative.starts_with(".moss/") {
        let full = format!("{parent_relative}/{name}");
        return !MOSS_INTERNAL_ALLOWLIST.iter().any(|allowed| {
            full == *allowed
                || full.starts_with(&format!("{allowed}/"))
                || allowed.starts_with(&format!("{full}/"))
        });
    }

    false
}


/// Extensions the build turns into pages.
///
/// The ONE definition in the repo: the scan's markdown arm calls it, and
/// `describe_source_role` reports its verdict to the editor so the frontend
/// never restates this list. A copy of it in TypeScript is the Rust↔TS parity
/// trap the preview follower's slot check already avoids.
pub fn is_page_source(extension: &str) -> bool {
    matches!(extension, "md" | "markdown" | "mdown" | "mkd")
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
/// hand-maintained mirror did (moss#1087) — calls this one instead.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_junk_hides_at_every_level_regardless_of_the_toggle() {
        for name in [".DS_Store", FINDER_ICON_MARKER, "Thumbs.db"] {
            for parent in ["", "posts"] {
                assert!(is_hidden(name, parent, false), "{name:?} in {parent:?}");
                assert!(is_hidden(name, parent, true), "{name:?} in {parent:?} with internals shown");
            }
        }
    }

    // --- is_hidden: root agent-instruction files ---

    #[test]
    fn root_agent_instruction_files_hide_with_the_internal_toggle() {
        for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
            assert!(is_hidden(name, "", false), "{name} must not sit among the author's prose");
            assert!(!is_hidden(name, "", true), "{name} must be reachable in internal mode");
        }
    }

    #[test]
    fn moss_lists_only_what_the_user_writes() {
        for name in ["config.toml", "theme"] {
            assert!(!is_hidden(name, ".moss", true), "{name} is the user's to edit");
        }
        for name in ["state.toml", "build", "identity", "plugins", "hashes.json", "data", "assets"] {
            assert!(is_hidden(name, ".moss", true), "{name} is moss's own, not the author's");
        }
        // `data/` is gone as a row, so nothing below it can be reached either.
        assert!(is_hidden("social", ".moss/data", true));
        assert!(is_hidden("matters.json", ".moss/data/social", true));
        // Below a listed path, everything shows.
        assert!(!is_hidden("grain.png", ".moss/theme", true));
        assert!(!is_hidden("Inter.woff2", ".moss/theme/fonts", true));
    }

    #[test]
    fn a_nested_agents_md_is_an_ordinary_file() {
        // `posts/AGENTS.md` is an article about agents. The scan publishes it
        // (`skip_root_agent_config` is root-only) and so the tree
        // shows it, in both modes.
        assert!(!is_hidden("AGENTS.md", "posts", false));
        assert!(!is_hidden("AGENTS.md", "posts", true));
    }

    #[test]
    fn a_lowercase_agents_md_at_the_root_stays_visible() {
        // Exact case, matching `is_agent_config_name`: an essay named
        // `Agents.md` is prose, and hiding it would be the worse failure.
        assert!(!is_hidden("Agents.md", "", false));
        assert!(!is_hidden("agents.md", "", false));
    }

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

    #[test]
    fn test_compute_passthrough_roots_detects_index_html() {
        use super::*;
        use crate::types::content::FileInfo;
        let html_files = vec![
            FileInfo { path: "app/index.html".to_string(), file_type: "html".to_string(), size: 0, modified: None },
            FileInfo { path: "about.html".to_string(), file_type: "html".to_string(), size: 0, modified: None },
        ];
        let roots = compute_passthrough_roots(&html_files, &[]);
        assert!(roots.contains("app/"), "should detect app/ via index.html");
        assert!(!roots.contains(""), "root-level about.html should not create root passthrough");
        assert_eq!(roots.len(), 1);
    }

    #[test]
    fn test_compute_passthrough_roots_root_index_html_ignored() {
        use super::*;
        use crate::types::content::FileInfo;
        let html_files = vec![
            FileInfo { path: "index.html".to_string(), file_type: "html".to_string(), size: 0, modified: None },
        ];
        let roots = compute_passthrough_roots(&html_files, &[]);
        assert!(roots.is_empty(), "root index.html must not trigger passthrough");
    }

    #[test]
    fn test_compute_passthrough_roots_config_add() {
        use super::*;
        use crate::types::content::FileInfo;
        let roots = compute_passthrough_roots(&[], &["raw-data".to_string()]);
        assert!(roots.contains("raw-data/"), "explicit config entry should add passthrough");
    }

    #[test]
    fn test_compute_passthrough_roots_config_adds_exact_file() {
        use super::*;
        use crate::types::content::FileInfo;
        let html_files = vec![FileInfo {
            path: "index.html".to_string(),
            file_type: "html".to_string(),
            size: 0,
            modified: None,
        }];
        let roots = compute_passthrough_roots(&html_files, &["index.html".to_string()]);
        assert!(roots.contains("index.html"));
        assert!(is_in_passthrough("index.html", &roots));
        assert!(!is_in_passthrough("index.html/child", &roots));
        assert!(!is_in_passthrough("index.html.bak", &roots));
    }

    #[test]
    fn test_compute_passthrough_roots_exact_file_negation() {
        use crate::types::content::FileInfo;
        let html_files = vec![FileInfo {
            path: "index.html".to_string(),
            file_type: "html".to_string(),
            size: 0,
            modified: None,
        }];
        let roots = compute_passthrough_roots(
            &html_files,
            &["index.html".to_string(), "!index.html".to_string()],
        );
        assert!(!is_in_passthrough("index.html", &roots));
    }

    #[test]
    fn test_compute_passthrough_roots_dotted_directory_stays_directory() {
        let roots = compute_passthrough_roots(&[], &["app.v2".to_string()]);
        assert!(roots.contains("app.v2/"));
        assert!(is_in_passthrough("app.v2/index.html", &roots));
    }

    #[test]
    fn test_compute_passthrough_roots_config_negation() {
        use super::*;
        use crate::types::content::FileInfo;
        let html_files = vec![
            FileInfo { path: "app/index.html".to_string(), file_type: "html".to_string(), size: 0, modified: None },
        ];
        let roots = compute_passthrough_roots(&html_files, &["!app".to_string()]);
        assert!(roots.is_empty(), "! prefix should remove auto-detected root");
    }

    #[test]
    fn test_is_in_passthrough() {
        use super::*;
        let mut roots = std::collections::HashSet::new();
        roots.insert("app/".to_string());
        assert!(is_in_passthrough("app/image.png", &roots));
        assert!(is_in_passthrough("app/sub/deep.js", &roots));
        assert!(!is_in_passthrough("other/image.png", &roots));
        assert!(!is_in_passthrough("appended/file.png", &roots), "must not prefix-match across separator");
    }

    #[test]
    fn test_compute_passthrough_roots_config_normalizes_leading_slash() {
        // A leading-slash config entry must still match slash-less relative
        // paths — otherwise the user's intent is silently dropped.
        let roots = compute_passthrough_roots(&[], &["/app".to_string()]);
        assert!(roots.contains("app/"), "leading slash should be trimmed to app/");
        assert!(is_in_passthrough("app/logo.png", &roots));
    }

    #[test]
    fn test_compute_passthrough_roots_config_skips_empty_entries() {
        use crate::types::content::FileInfo;
        // Empty string, bare slash, and a lone "!" must not create junk roots.
        let html_files = vec![
            FileInfo { path: "real/index.html".to_string(), file_type: "html".to_string(), size: 0, modified: None },
        ];
        let roots = compute_passthrough_roots(
            &html_files,
            &["".to_string(), "/".to_string(), "!".to_string()],
        );
        assert!(roots.contains("real/"), "auto-detected root survives");
        assert!(!roots.contains("/"), "bare slash must not become a root");
        assert_eq!(roots.len(), 1, "no junk roots from empty entries: {:?}", roots);
    }

    /// `.mdx` reads as markdown in the editor (`file-types.ts` maps it to the
    /// markdown language) but the build has no mdx arm, and `.markdown` is the
    /// mirror image. That asymmetry is why the editor asks for this verdict
    /// instead of testing extensions itself.
    #[test]
    fn page_sources_are_the_scan_arm_not_the_editor_language_set() {
        assert!(is_page_source("md"));
        assert!(is_page_source("markdown"));
        assert!(is_page_source("mdown"));
        assert!(is_page_source("mkd"));
        assert!(!is_page_source("mdx"));
        assert!(!is_page_source("html"));
        assert!(!is_page_source("txt"));
    }

    /// Pins `classify_extension` against every extension `scan.rs`'s own
    /// match block recognizes, so a bucket added there and forgotten here (or
    /// the reverse) fails immediately rather than silently drifting the way
    /// the sweep's hand-maintained mirror did (moss#1087). An extension
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
}
