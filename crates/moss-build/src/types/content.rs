//! Pure build-output data — the M6a boundary's "data" side.
//!
//! Everything here is plain data a future headless moss-build crate would
//! consume: the hash-manifest entry helpers, scan results, media metadata,
//! and the build hash manifest. No Tauri, no server, no process handles.
//!
//! Split out of `types.rs` 2026-08-10 (M5a); `types.rs` re-exports every
//! item at its old path, so consumers are unchanged.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};

use crate::build::types::SourceMetadata;
use crate::config::deployment::DeploymentConfig;

/// Manifest entry mode tags. POSIX file-mode octal strings, matching
/// `git ls-tree` and POSIX tar typeflag conventions.
pub const MODE_FILE: &str = "100644";
pub const MODE_SYMLINK: &str = "120000";

/// Build a regular-file manifest entry value: `100644:<hash>`.
pub fn file_entry(hash: &str) -> String {
    format!("{}:{}", MODE_FILE, hash)
}

/// Build a symlink manifest entry value: `120000:<sha256(target)>`.
/// The hash is over the target path string, not its byte content — that's
/// what gives the diff algorithm its content-addressed semantics for symlinks.
pub fn symlink_entry(target: &str) -> String {
    use sha2::{Digest, Sha256};
    let target_hash = format!("{:x}", Sha256::digest(target.as_bytes()));
    format!("{}:{}", MODE_SYMLINK, target_hash)
}

/// Parse a manifest entry value into its mode and hash.
///
/// Accepts:
/// - `<6-digit-octal-mode>:<hash>` — surfaces the literal mode regardless
///   of whether the dispatcher knows how to handle it. The deploy uploader
///   then rejects unknown modes with a precise error, which is much more
///   useful for diagnosing version-mismatch bugs than silently treating an
///   unknown mode as a regular file.
/// - bare hash (no 6-digit-octal-then-colon prefix) — treated as
///   `(MODE_FILE, hash)` for back-compat with `.moss/hashes.json` written
///   before 2026-04-28.
pub fn parse_entry(value: &str) -> (&str, &str) {
    // Boundary safety: index byte 6 only after asserting the value is long
    // enough AND ASCII through that position. The narrow check `is_ascii`
    // on the prefix slice avoids panicking on a malformed manifest with
    // multi-byte UTF-8 in the first 6 bytes.
    // `get(..6)` is `None` when byte 6 is not a char boundary, so a manifest
    // with multi-byte UTF-8 in the first 6 bytes falls through untagged.
    if let (Some(mode), Some(rest)) = (value.get(..6), value.get(6..)) {
        // Any 6-digit octal mode is recognized as a tagged entry. The
        // dispatcher (deploy.rs) decides whether the specific value is
        // supported — this gives accurate "unknown manifest mode <X>"
        // error messages instead of silently treating <X> as MODE_FILE.
        if mode.bytes().all(|b| b.is_ascii_digit()) {
            if let Some(path) = rest.strip_prefix(':') {
                return (mode, path);
            }
        }
    }
    (MODE_FILE, value)
}

#[cfg(test)]
mod entry_tests {
    use super::*;

    #[test]
    fn file_entry_round_trip() {
        let v = file_entry("abc123");
        assert_eq!(v, "100644:abc123");
        let (m, h) = parse_entry(&v);
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, "abc123");
    }

    #[test]
    fn symlink_entry_uses_target_hash() {
        let v = symlink_entry("resources/cities-heat-map-app");
        assert!(v.starts_with("120000:"));
        let (m, h) = parse_entry(&v);
        assert_eq!(m, MODE_SYMLINK);
        // sha256 hex is 64 chars
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn legacy_bare_hash_treated_as_file() {
        let bare = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let (m, h) = parse_entry(bare);
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, bare);
    }

    #[test]
    fn unknown_6digit_mode_surfaces_actual_mode() {
        // Forward-compatibility: `100755` (executable) doesn't have a
        // dispatcher today, but parse_entry surfaces the actual mode so
        // the dispatcher can produce an accurate error like "unknown
        // manifest mode 100755". Older parsers that silently treated this
        // as MODE_FILE would corrupt the hash slot.
        let (m, h) = parse_entry("100755:abc");
        assert_eq!(m, "100755");
        assert_eq!(h, "abc");
    }

    #[test]
    fn non_octal_six_char_prefix_is_passthrough() {
        // A 64-char hex hash could have a colon at position 6 (`abcdef:...`).
        // Don't false-trigger on it — the prefix isn't all digits.
        let v = "abcdef:rest-of-content";
        let (m, h) = parse_entry(v);
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, v);
    }

    #[test]
    fn empty_string_parses_as_legacy_file() {
        let (m, h) = parse_entry("");
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, "");
    }

    #[test]
    fn short_value_under_seven_chars_is_passthrough() {
        let (m, h) = parse_entry("abc");
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, "abc");
    }

    #[test]
    fn multibyte_prefix_does_not_panic() {
        // 4 chars of non-ASCII (each 3 bytes in UTF-8) makes byte 6 land
        // mid-codepoint. parse_entry must not slice mid-codepoint.
        let v = "\u{4E2D}\u{6587}:abc"; // "中文:abc"
        let (m, h) = parse_entry(v);
        // The prefix isn't 6 ASCII digits, so the value passes through.
        assert_eq!(m, MODE_FILE);
        assert_eq!(h, v);
    }
}

/// Metadata for a single file discovered during folder scanning.
///
/// Contains essential information needed for static site generation,
/// including file type classification and modification timestamps.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct FileInfo {
    /// Relative path from the scanned root directory
    pub path: String,
    /// File extension in lowercase (e.g., "md", "jpg", "html")
    pub file_type: String,
    /// File size in bytes
    pub size: u64,
    /// Unix timestamp as string, if available
    pub modified: Option<String>,
}

/// Metadata for media files (images/videos) including dimensions for placeholder generation.
///
/// Thumbnail-based extraction for performance, and dynamic SVG placeholders
/// with dominant color.
///
/// This struct extends FileInfo with media-specific metadata that enables:
/// - Generating properly-sized placeholder SVGs before images load
/// - Creating colored placeholder backgrounds from dominant color
/// - Correct aspect ratio preservation in responsive layouts
#[derive(Serialize, Deserialize, Debug, Clone, Type, Default)]
pub struct MediaMetadata {
    /// Relative path from the scanned root directory
    pub path: String,
    /// File extension in lowercase (e.g., "jpg", "png", "mp4")
    pub file_type: String,
    /// File size in bytes
    pub size: u64,
    /// Unix timestamp as string, if available
    pub modified: Option<String>,
    /// Image/video dimensions (width, height) - for placeholder SVG generation
    pub dimensions: Option<(u32, u32)>,
    /// Dominant color (raw scan format — see below) for placeholder
    /// backgrounds and folder-card colors.
    ///
    /// The scan layer stores this in two formats depending on file type:
    /// - Image scan (`extract_color_and_lqip`) → `#RRGGBB` raw average.
    /// - Video scan (`extract_video_dominant_color`) → already-WCAG-darkened
    ///   `hsla(...)` (because it routes through `extract_dominant_color`).
    ///
    /// CSS-color-agnostic consumers (SVG `fill=`, asset-rewriter pending
    /// state) take this value verbatim. Folder-card renderers must NOT —
    /// they need WCAG-AA contrast against white text. Use
    /// `MediaDimensionLookup::get_cover_color` (which delegates
    /// to `color_extract::prepare_cover_color`) to normalize.
    pub dominant_color: Option<String>,
    /// LQIP (Low Quality Image Placeholder) as a base64 data URI
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lqip_data_uri: Option<String>,
    /// Whether the source is an animated image (multi-frame GIF, or WebP with
    /// an ANIM chunk). Sniffed from the file header at scan time (bounded
    /// ≤4 KB read, gif/webp extensions only — every other extension is
    /// `false` without touching the file). Animated sources must never be
    /// resized/re-encoded, so this flag gates the responsive ladder
    /// (Phase B of the responsive image variants plan).
    #[serde(default)]
    pub is_animated: bool,
    // `webp_variant` field deleted 2026-05-20.
    // The snapshot mechanism was the parallel-oracle root cause of the
    // broken-hero bug. Synthesizer now always emits <picture>, AssetRegistry
    // handles placeholder lifecycle at request time.
}

/// Complete analysis of a folder's contents for static site generation.
///
/// Contains categorized file listings and inferred project characteristics
/// used to determine the optimal site generation strategy.
///
/// Media files use MediaMetadata to include dimensions and dominant color
/// for placeholder SVG generation during page load.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct ProjectStructure {
    /// Absolute path to the scanned directory
    pub root_path: String,
    /// Markdown and text files (.md, .markdown, etc.)
    pub markdown_files: Vec<FileInfo>,
    /// HTML files (.html, .htm)
    pub html_files: Vec<FileInfo>,
    /// Image files (.jpg, .png, .gif, .svg, etc.) with dimensions and dominant color
    pub image_files: Vec<MediaMetadata>,
    /// Video files (.mov, .mp4, .webm, .avi, .mkv) with dimensions
    pub video_files: Vec<MediaMetadata>,
    /// Jupyter notebook files (.ipynb).
    ///
    /// .ipynb files are JSON documents containing ordered cells: code (with
    /// outputs), markdown, or raw text. Same mental model as source HTML files
    /// (p5.js sketches, interactive embeds): detected during scan, processed
    /// during background phase.
    ///
    /// Build time: scan detects .ipynb (no download). Background phase downloads
    /// JupyterLite if not cached, generates viewer HTML with JupyterLite iframe.
    ///
    /// Run time: each notebook URL serves a page with JupyterLite iframe.
    /// JupyterLite (~20MB WASM) loads directly in the visitor's browser.
    /// `loading="lazy"` defers load until viewport scroll.
    pub notebook_files: Vec<FileInfo>,
    /// All other files including documents (.docx, .pages, etc.)
    pub other_files: Vec<FileInfo>,
    /// Total count of all discovered files
    pub total_files: usize,
    /// Detected homepage file path, if any
    pub homepage_file: Option<String>,
    /// Resolved FFmpeg binary path (lazy: only set when video files were found during scan).
    /// Carried through to BackgroundContext so build.rs can reuse it without re-downloading.
    #[serde(skip)]
    #[specta(skip)]
    pub ffmpeg_bin_path: Option<String>,
    /// Number of iCloud-evicted (cloud-only) files detected during the scan.
    /// Counted as a side effect of the directory walk — no extra I/O.
    #[serde(skip)]
    #[specta(skip)]
    pub evicted_count: usize,
    /// Absolute paths of files the scan classified as evicted (cloud-only).
    /// Same classification as `evicted_count`, kept as a list (not just a
    /// count) so the home-page bounded wait and the cloud-sync progress
    /// counter (`pipeline::build_inner`) can re-check materialization
    /// per-file.
    #[serde(skip)]
    #[specta(skip)]
    pub evicted_paths: Vec<std::path::PathBuf>,
    /// Whether the root directory has content subfolders (folders with .md/.html files).
    /// true = "organized mode" (root files are nav items, folders are sections)
    /// false = "flat mode" (root files are articles, keyword names also in nav)
    #[serde(skip)]
    #[specta(skip)]
    pub has_content_folders: bool,
    /// Whether the site contains language-prefix subtrees (top-level folders named
    /// with a known language code, e.g. `en/`, `zh-hans/`). Used to gate the root
    /// homepage's default-language-tree listing scope: when true, the root home lists
    /// only the default tree (docs not under a language-prefix folder). Computed once
    /// at scan, parallel to `has_content_folders`.
    #[serde(skip)]
    #[specta(skip)]
    pub has_language_trees: bool,
    /// Passthrough subtree roots (source-relative, trailing-slash-terminated).
    /// Files inside these directories are copied verbatim — no WebP/video conversion,
    /// no SPA meta injection. Auto-detected from `index.html` presence; overridable
    /// via `[build].passthrough` in `.moss/config.toml`.
    #[serde(skip)]
    #[specta(skip)]
    pub passthrough_roots: std::collections::HashSet<String>,
    /// Every directory that gets a synthetic folder-index page, as
    /// project-relative (raw, un-slugified) paths with `/` separators — e.g.
    /// `["my-1st-folder", "Notes/2024"]`. Unlike the file lists, this records
    /// directories that contain no files at all, so the renderer can emit an
    /// index page for EVERY folder including completely empty ones (a product
    /// decision: no folder 404s). The scanned root itself is NOT included
    /// (its page is the homepage).
    ///
    /// This is a narrower set than "every directory the walk saw", and the
    /// filtering happens once, in `scan.rs`, so the two consumers in the
    /// render's blocking phase can seed straight from it. Excluded/internal
    /// dirs (`.moss`, `.git`, dotfiles, `node_modules`) are pruned by the
    /// walk's `is_excluded_dir_name` filter; passthrough subtrees and
    /// `[editor].attachment_folder` storage are dropped afterwards, because
    /// both are directories the site has files in but no section for.
    ///
    /// NOT the asset-copy list — media variants are driven by `image_files` /
    /// `video_files` / `other_files`, which still cover every directory the
    /// walk entered. Computed during the walk, so no extra I/O.
    #[serde(skip)]
    #[specta(skip)]
    pub dirs: Vec<String>,
}

/// User configuration for site generation and publishing.
///
/// Contains site metadata, theme settings, and deployment preferences.
/// Can be stored in `.moss/config.toml` within project directories.
#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct MossConfig {
    /// Display name for the generated website
    pub site_name: Option<String>,
    /// Author name for metadata and attribution
    pub author: Option<String>,
    /// Theme identifier for styling the generated site
    pub theme: Option<String>,
    /// Local directory for generated site files
    pub output_dir: Option<String>,
    /// Base URL for absolute links (e.g., "<https://example.com>")
    pub base_url: Option<String>,
    /// Deployment configuration settings
    pub deployment: DeploymentConfig,
}

/// Result of static site generation process.
///
/// Contains summary information about the generated site
/// including page count and output location.
#[derive(Serialize, Deserialize, Debug, Type)]
pub struct SiteResult {
    /// Number of HTML pages generated
    pub page_count: usize,
    /// Path to the generated site build directory
    pub site_build_dir: String,
    /// Site metadata extracted from content
    pub site_title: String,
    /// Hash map of all generated files (path -> SHA-256 hash)
    /// Used for change detection in preview refresh
    #[specta(skip)]
    pub hashes: SiteHashes,
    /// Source paths skipped this build because they (or a page embedding
    /// them) were cloud-dataless placeholders. Non-empty means this build is
    /// incomplete — `pipeline::build_inner` uses it to skip
    /// `remove_stale_html` and to suppress a false cloud-sync "done" event.
    #[serde(skip)]
    #[specta(skip)]
    pub deferred_paths: Vec<std::path::PathBuf>,
    /// Media references in this site that point at no file on disk. A publish
    /// is refused while this is non-empty — see [`crate::missing_media`].
    #[serde(skip)]
    #[specta(skip)]
    pub missing_media: Vec<crate::build::types::MissingMedia>,
}

/// Hash map of generated site files for change detection.
///
/// Maps output file paths (relative to the active generation `.moss/build.nosync/current/`) to their SHA-256 content hashes.
/// Used for:
/// 1. Smart preview refresh - only refresh if current page's hash changed
/// 2. Future incremental uploads - only upload files whose hash changed
///
/// Persisted to `.moss/hashes.json` after each build.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SiteHashes {
    /// Map of output path to mode-tagged content hash.
    ///
    /// Wire format:
    ///   `<octal-mode>:<sha256>`
    ///   - `100644:<hash>` — regular file, hash of bytes
    ///   - `120000:<hash>` — symlink, hash of the target path string
    ///
    /// Legacy bare-hash values (no colon) deserialize as `100644:<hash>`
    /// for back-compat with `.moss/hashes.json` written before 2026-04-28.
    ///
    /// Use the helpers `file_entry()` / `symlink_entry()` to construct
    /// values; use `parse_entry()` to read them. The diff algorithm is
    /// equality of the value string — a mode change OR hash change both
    /// trigger re-upload, which is correct.
    pub files: HashMap<String, String>,
    /// Map of source path to metadata (hash + size + mtime for fast-path caching)
    #[serde(default)]
    pub sources: HashMap<String, SourceMetadata>,
    /// When this manifest's hashing began, as Unix seconds — git's
    /// "racily clean" clock. A source file whose mtime lands within one
    /// timestamp-granularity epsilon of this instant could have been
    /// rewritten *after* it was hashed without moving its mtime (1–2s
    /// filesystems: exFAT, SMB), so the sweep treats such entries as
    /// suspect and routes them to the hash tier instead of trusting the
    /// size+mtime fast path. `None` in manifests that predate the field —
    /// fail open (no racy demotion), never fail closed.
    /// See `crate::build::watch::mtime_is_racy`.
    #[serde(default)]
    pub captured_at: Option<u64>,
    /// The site URL this generation was built with — canonical links,
    /// og:url, rss/sitemap entries, and qr/*.svg all embed it. `None` in
    /// hashes.json written before this field existed; the deploy guard
    /// treats unknown provenance as stale and rebuilds once.
    #[serde(default)]
    pub site_url: Option<String>,
    /// Set of derived video output paths (converted .mp4 and .thumb.jpg files).
    /// These are tracked separately from source files because they're generated
    /// artifacts, not direct copies. Used by stale cleanup to preserve converted videos.
    #[serde(default)]
    pub video_outputs: HashSet<String>,
    /// Set of derived image output paths (generated .webp variants).
    /// Parallels `video_outputs`: generated artifacts have no entry in
    /// `files` (source is .jpg / .png, output is .webp), so stale cleanup
    /// would delete them without this protection.
    ///
    /// Populated by `build::image::update_image_hashes` after each image
    /// conversion pass; read by `media::pipeline::remove_stale_files` and
    /// `media::pipeline::compute_expected_dirs`.
    ///
    /// `#[serde(default)]` ensures existing on-disk `hashes.json` files
    /// (which lack this field) continue to deserialize cleanly.
    #[serde(default)]
    pub image_outputs: HashSet<String>,
    /// Variants a COMPLETE ship-time reference scan judged unreferenced, so
    /// the next build's producers do not make them again.
    ///
    /// The prune is the only pass that knows what "referenced" means with full
    /// information — it runs after every plugin, notebook and feed has written
    /// into staging — and both image producers run earlier. This field is how
    /// its verdict reaches them; the rationale and the failure it ends live on
    /// `build::media::orphan_prune::suppressed_variants`.
    #[serde(default)]
    pub pruned_image_outputs: HashSet<String>,
    /// Set of derived notebook output paths: JupyterLite assets under
    /// `jupyter/**`, viewer HTML wrappers (`<notebook>.html`), and `.ipynb`
    /// copies placed at their natural output path. Parallels
    /// `video_outputs` / `image_outputs`: generated artifacts with no
    /// `files` entry, so stale cleanup would delete them without this
    /// protection. Populated by `run_notebook_processing` in the background
    /// phase; read by `media::pipeline::remove_stale_files`,
    /// `media::pipeline::remove_stale_html`, and
    /// `media::pipeline::compute_expected_dirs`.
    ///
    // TODO: unify with video_outputs/image_outputs into a single
    // background_outputs set (or absorb into BuildContext::emit_artifact).
    #[serde(default)]
    pub notebook_outputs: HashSet<String>,
    /// SHA-256 of the moss binary itself. When this changes between builds
    /// (e.g. after a moss update that renames CSS classes or changes HTML
    /// templates), all output is considered stale and a full rebuild with
    /// browser refresh is triggered. Without this, cached HTML from a
    /// previous binary version would reference CSS classes or markup that
    /// no longer exist in the new binary's embedded stylesheet.
    ///
    /// Only affects HTML/CSS/JS regeneration — video conversion and other
    /// media asset caching is based on source content hashes, not the
    /// builder version, so those remain unaffected.
    ///
    /// The `alias` accepts the legacy `compiler_fingerprint` field name so
    /// existing `.moss/build.nosync/hashes.json` files from pre-rename moss
    /// deserialize cleanly; write always emits `builder_fingerprint`.
    #[serde(default, alias = "compiler_fingerprint")]
    pub builder_fingerprint: Option<String>,
    /// Source-path → output-path index for markdown pages.
    ///
    /// Keyed by project-relative source path (e.g. `"posts/Hello World.md"`),
    /// values are the output path key in `files` (e.g. `"posts/hello-world/index.html"`).
    /// Populated during the HTML write phase from `ParsedDocument.source_path`
    /// → `ParsedDocument.url_path` so consumers can answer "what output did
    /// this source produce?" without re-deriving the slug rules,
    /// language-suffix stripping, frontmatter `url:` overrides, or
    /// translation-home promotion that `compute_url_path` and the page_map
    /// apply.
    ///
    /// Used by `build::watch::build_rebuild_event_with_renames` to resolve
    /// the watcher's inode-paired rename events (source-domain paths) to
    /// output paths for `FileChangeEvent.moved_output_paths`. The index is also
    /// useful for reverse-search ("what source produced this URL?") and
    /// deploy-diff debugging.
    ///
    /// `#[serde(default)]` — old manifests without this field deserialize
    /// to an empty map; rename support degrades to the heuristic fallback
    /// in `find_output_for_source` until the next build heals the entry.
    ///
    /// **Coverage:** markdown pages only. Excluded:
    /// - Synthetic pages (auto-generated folder indexes, root home placeholder,
    ///   OG cards) — no `source_path`.
    /// - Notebook (`.ipynb`) sources — emitted via `HashBucket::NotebookOutputs`,
    ///   not through `ParsedDocument`. In-app `.ipynb` renames fall through
    ///   to the deletion path (iframe redirects home) — out of scope here.
    /// - Asset/media files (`.jpg`, `.mp4`, etc.) — same emission path,
    ///   same fall-through behavior.
    #[serde(default)]
    pub source_to_output: HashMap<String, String>,
    /// Source-path → title/date index for markdown pages, keyed identically
    /// to `source_to_output`.
    ///
    /// Exists for one consumer: `PendingManifest::carry_forward_deferred_page`
    /// reinstates a deferred page's OUTPUT (the previous build's HTML survives
    /// on disk), but had nothing to give the nav/listing pass, which reads a
    /// page's title and date from `ParsedDocument` — and a page this build
    /// could not read never produces one. Without this, a carried-forward
    /// page's URL kept resolving while it silently dropped out of every nav
    /// menu and listing page. Populated alongside `source_to_output` at every
    /// registration site (the HTML write loop, the homepage, and the carried
    /// set); read back by `carry_forward_deferred_page`.
    ///
    /// `#[serde(default)]` — old `hashes.json` files without this field
    /// deserialize to an empty map, so a deferred page carried forward under
    /// an old manifest simply has no nav/listing entry until the next build
    /// that can read it, same as before this field existed.
    #[serde(default)]
    pub page_meta: HashMap<String, PageMeta>,
}

/// A page's nav/listing-relevant metadata, carried alongside
/// `SiteHashes::source_to_output` so a carried-forward page keeps its title
/// and date. See `SiteHashes::page_meta`.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PageMeta {
    pub title: String,
    pub date: Option<String>,
}

impl SiteHashes {
    /// Create a new empty hash map
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            sources: HashMap::new(),
            captured_at: None,
            site_url: None,
            video_outputs: HashSet::new(),
            image_outputs: HashSet::new(),
            pruned_image_outputs: HashSet::new(),
            notebook_outputs: HashSet::new(),
            builder_fingerprint: None,
            source_to_output: HashMap::new(),
            page_meta: HashMap::new(),
        }
    }

    /// Insert a file hash
    pub fn insert(&mut self, path: String, hash: String) {
        self.files.insert(path, hash);
    }

    // ----- Typed accessors taking &ServedPath -----
    //
    // Code that mutates SiteHashes inner buckets MUST go through these
    // accessors so the manifest cannot end up with a non-normalized path.
    // The bare `insert` above predates the ServedPath chokepoint and is
    // retained for back-compat with code paths that already hold a String
    // they know is normalized; the grep-test in tests/served_path_invariant.rs
    // gates against new direct `.files.insert(...)` callers.

    /// Insert a file hash (typed). Accepts `&ServedPath` so the path is
    /// guaranteed normalized at compile time.
    pub fn insert_file_hash(&mut self, path: &crate::build::served_path::ServedPath, hash_entry: String) {
        self.files.insert(path.as_str().to_string(), hash_entry);
    }

    /// Insert an image-output marker (typed). Accepts `&ServedPath`.
    pub fn insert_image_output(&mut self, path: &crate::build::served_path::ServedPath) {
        self.image_outputs.insert(path.as_str().to_string());
    }

    /// Insert a video-output marker (typed).
    pub fn insert_video_output(&mut self, path: &crate::build::served_path::ServedPath) {
        self.video_outputs.insert(path.as_str().to_string());
    }

    /// Insert a notebook-output marker (typed).
    pub fn insert_notebook_output(&mut self, path: &crate::build::served_path::ServedPath) {
        self.notebook_outputs.insert(path.as_str().to_string());
    }

    /// Get changed files by comparing with previous hashes
    pub fn get_changed_files(&self, previous: &SiteHashes) -> Vec<String> {
        self.files
            .iter()
            .filter(|(path, new_hash)| previous.files.get(*path) != Some(new_hash))
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Get newly added files (in self but not in previous)
    pub fn get_new_files(&self, previous: &SiteHashes) -> Vec<String> {
        self.files
            .keys()
            .filter(|path| !previous.files.contains_key(*path))
            .cloned()
            .collect()
    }

    /// Get deleted files (in previous but not in self)
    pub fn get_deleted_files(&self, previous: &SiteHashes) -> Vec<String> {
        previous
            .files
            .keys()
            .filter(|path| !self.files.contains_key(*path))
            .cloned()
            .collect()
    }

    /// Look up source metadata by a folder-relative path (forward-slash form).
    ///
    /// The `sources` map is keyed by paths relative to the site folder, exactly
    /// as written by the asset pipeline. Callers must normalize absolute event
    /// paths to folder-relative form before calling this.
    ///
    /// Note: `sources` is read by both the build-output change-detection path
    /// and the watcher content-hash gate. Keep the format stable.
    pub fn source_metadata_for(&self, relative_path: &str) -> Option<&SourceMetadata> {
        self.sources.get(relative_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================
    // SiteHashes Tests
    // =========================================

    #[test]
    fn test_site_hashes_new() {
        let hashes = SiteHashes::new();
        assert!(hashes.files.is_empty());
    }

    // DO NOT REMOVE the `alias = "compiler_fingerprint"` on SiteHashes
    // without also bumping a major version and accepting that every
    // pre-rename `.moss/build.nosync/hashes.json` will trigger a silent full
    // rebuild on first load. The alias can be dropped once all active
    // sites have rebuilt post-rename.
    #[test]
    fn site_hashes_deserializes_legacy_compiler_fingerprint() {
        let legacy = r#"{
            "files": {},
            "sources": {},
            "video_outputs": [],
            "image_outputs": [],
            "notebook_outputs": [],
            "plugin_fingerprint": null,
            "compiler_fingerprint": "legacy-abc-123"
        }"#;
        let hashes: SiteHashes = serde_json::from_str(legacy)
            .expect("legacy JSON must deserialize");
        assert_eq!(
            hashes.builder_fingerprint.as_deref(),
            Some("legacy-abc-123"),
            "serde alias should route compiler_fingerprint -> builder_fingerprint",
        );
        let out = serde_json::to_string(&hashes).unwrap();
        assert!(out.contains("builder_fingerprint"), "write uses new name: {}", out);
        assert!(!out.contains("compiler_fingerprint"), "old name is gone on write: {}", out);
    }

    #[test]
    fn test_site_hashes_insert() {
        let mut hashes = SiteHashes::new();
        hashes.insert("index.html".to_string(), "abc123".to_string());
        assert_eq!(hashes.files.get("index.html"), Some(&"abc123".to_string()));
    }

    #[test]
    fn test_site_hashes_get_changed_files_modified() {
        let mut previous = SiteHashes::new();
        previous.insert("index.html".to_string(), "old_hash".to_string());
        previous.insert("about.html".to_string(), "same_hash".to_string());

        let mut current = SiteHashes::new();
        current.insert("index.html".to_string(), "new_hash".to_string());
        current.insert("about.html".to_string(), "same_hash".to_string());

        let changed = current.get_changed_files(&previous);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&"index.html".to_string()));
    }

    #[test]
    fn test_site_hashes_get_changed_files_new_file() {
        let previous = SiteHashes::new();

        let mut current = SiteHashes::new();
        current.insert("new_file.html".to_string(), "hash123".to_string());

        let changed = current.get_changed_files(&previous);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&"new_file.html".to_string()));
    }

    #[test]
    fn test_site_hashes_get_changed_files_no_changes() {
        let mut previous = SiteHashes::new();
        previous.insert("index.html".to_string(), "same_hash".to_string());

        let mut current = SiteHashes::new();
        current.insert("index.html".to_string(), "same_hash".to_string());

        let changed = current.get_changed_files(&previous);
        assert!(changed.is_empty());
    }

    #[test]
    fn test_site_hashes_get_new_files() {
        let mut previous = SiteHashes::new();
        previous.insert("old.html".to_string(), "hash1".to_string());

        let mut current = SiteHashes::new();
        current.insert("old.html".to_string(), "hash1".to_string());
        current.insert("new.html".to_string(), "hash2".to_string());

        let new_files = current.get_new_files(&previous);
        assert_eq!(new_files.len(), 1);
        assert!(new_files.contains(&"new.html".to_string()));
    }

    #[test]
    fn test_site_hashes_get_deleted_files() {
        let mut previous = SiteHashes::new();
        previous.insert("kept.html".to_string(), "hash1".to_string());
        previous.insert("deleted.html".to_string(), "hash2".to_string());

        let mut current = SiteHashes::new();
        current.insert("kept.html".to_string(), "hash1".to_string());

        let deleted = current.get_deleted_files(&previous);
        assert_eq!(deleted.len(), 1);
        assert!(deleted.contains(&"deleted.html".to_string()));
    }

    /// Test that SiteHashes has a video_outputs field for tracking derived video files.
    ///
    /// The bug: Converted .mp4 files are deleted by stale cleanup because they're
    /// not registered in site_hashes.files. We need a separate video_outputs field
    /// to track derived assets.
    #[test]
    fn test_site_hashes_has_video_outputs_field() {
        let hashes = SiteHashes::new();
        // video_outputs should be an empty HashSet by default
        assert!(hashes.video_outputs.is_empty(), "video_outputs should be empty on new SiteHashes");
    }

    /// Test that video_outputs can track derived video files.
    #[test]
    fn test_site_hashes_video_outputs_insert() {
        let mut hashes = SiteHashes::new();
        hashes.video_outputs.insert("videos/aimeili.mp4".to_string());
        assert!(hashes.video_outputs.contains("videos/aimeili.mp4"));
    }

    /// Test that video_outputs is included in serialization.
    /// This ensures video_outputs survives JSON round-trip to hashes.json.
    #[test]
    fn test_site_hashes_video_outputs_serialization() {
        let mut hashes = SiteHashes::new();
        hashes.video_outputs.insert("videos/test.mp4".to_string());

        let json = serde_json::to_string(&hashes).expect("serialize");
        let deserialized: SiteHashes = serde_json::from_str(&json).expect("deserialize");

        assert!(deserialized.video_outputs.contains("videos/test.mp4"));
    }

    // -----------------------------------------------------------------------
    // MediaMetadata LQIP field tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_media_metadata_with_lqip() {
        let meta = MediaMetadata {
            is_animated: false,
            path: "images/photo.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 12345,
            modified: None,
            dimensions: Some((1920, 1080)),
            dominant_color: Some("#ff5733".to_string()),
            lqip_data_uri: Some("data:image/jpeg;base64,/9j/4AAQ".to_string()),
        };

        let json = serde_json::to_string(&meta).expect("ser");
        let loaded: MediaMetadata = serde_json::from_str(&json).expect("deser");
        assert_eq!(
            loaded.lqip_data_uri.as_deref(),
            Some("data:image/jpeg;base64,/9j/4AAQ")
        );
    }

    #[test]
    fn test_media_metadata_without_lqip() {
        let meta = MediaMetadata {
            is_animated: false,
            path: "images/icon.svg".to_string(),
            file_type: "svg".to_string(),
            size: 500,
            modified: None,
            dimensions: Some((200, 200)),
            dominant_color: None,
            lqip_data_uri: None,
        };

        let json = serde_json::to_string(&meta).expect("ser");
        assert!(!json.contains("lqip_data_uri"), "None field must be omitted");
    }

    #[test]
    fn source_metadata_for_looks_up_by_relative_path() {
        let mut hashes = SiteHashes::new();
        hashes.sources.insert(
            "posts/hello.md".to_string(),
            SourceMetadata { hash: "abc".into(), size: 42, mtime: 1000, mtime_nanos: None, ctime: None, inode: None },
        );
        let meta = hashes.source_metadata_for("posts/hello.md").expect("present");
        assert_eq!(meta.size, 42);
        assert!(hashes.source_metadata_for("posts/missing.md").is_none());
    }
}
