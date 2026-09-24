//! Path resolution and hashing utilities for static site generation.
//!
//! This module provides utilities for:
//! - Computing content hashes for change detection (using xxHash3 for speed)
//! - Resolving root-relative paths for site assets and pages

use crate::build::served_path::ServedPath;
use xxhash_rust::xxh3::xxh3_64;

/// Size of the read buffer used by [`compute_binary_hash_file`].
///
/// 64 KB is a good trade-off between syscall overhead and memory use.
/// Large media files (videos, high-res images) are streamed through this
/// buffer without loading the whole file into memory.
const HASH_FILE_BUF_SIZE: usize = 64 * 1024;

/// Compute hash of string content using xxHash3 (fast, good for change detection)
pub fn compute_content_hash(content: &str) -> String {
    format!("{:016x}", xxh3_64(content.as_bytes()))
}

/// Compute hash of binary content using xxHash3 (fast, good for change detection)
pub fn compute_binary_hash(content: &[u8]) -> String {
    format!("{:016x}", xxh3_64(content))
}

/// Streaming xxh3 hash of a file — produces the IDENTICAL digest as
/// `compute_binary_hash(&fs::read(path))` but reads in 64 KB chunks so
/// large assets (videos, high-res images) are never fully loaded into memory.
///
/// This is the manifest-hash function for source assets: every entry in the
/// `files` map of `SiteHashes` must use this digest so that
/// `deploy.rs::verify_file_bytes` (which calls `compute_binary_hash`) can
/// round-trip successfully.
pub fn compute_binary_hash_file(path: &std::path::Path) -> Result<String, String> {
    compute_binary_hash_file_with_heartbeat(path, &|| {})
}

/// [`compute_binary_hash_file`], calling `on_chunk` once per 64 KB read.
///
/// The caller that needs this is the publish path, which hashes a large file
/// BEFORE its first upload request: it credits no bytes and emits no progress,
/// so hashing a video on a slow disk (or an iCloud file that is materializing)
/// is indistinguishable from a wedged publish, and the stall watchdog would
/// cancel work that is going perfectly well.
///
/// It is a parameter rather than a call into `deploy::activity` because the
/// build has no business knowing a publish watchdog exists — and because the
/// SHA-256 arm of the same `HashAlgo::hash_file` already bumps at its own call
/// site, so this makes the two arms say the same thing in the same place.
///
/// **What that cost, said out loud.** The bump used to live INSIDE
/// [`compute_binary_hash_file`], so it fired for every caller. It now fires
/// only for callers that pass one, and the four build-side callers
/// (`media/video.rs`, `media/pipeline.rs` ×2, `manifest/backfill.rs`) pass the
/// no-op default — they cannot reach `deploy::activity` without re-creating
/// the edge this removed. Those hashes still run inside work a publish waits
/// on (`deploy::drain_in_flight_work`, `STALL_TIMEOUT` 300s), so a single file
/// whose read SUCCEEDS but takes over 300s with no other build event in the
/// window would now abort a publish that was progressing fine. Narrow, and not
/// nothing: moss#1060. Closing it properly is a reporter-shaped concern, which
/// is the port seam's business (NORTH-STAR:88, ratchet row (o)) — not a
/// re-added `crate::deploy` call.
pub fn compute_binary_hash_file_with_heartbeat(
    path: &std::path::Path,
    on_chunk: &dyn Fn(),
) -> Result<String, String> {
    use std::io::Read;
    use xxhash_rust::xxh3::Xxh3Default;

    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut hasher = Xxh3Default::new();
    let mut buf = vec![0u8; HASH_FILE_BUF_SIZE];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        on_chunk();
    }
    Ok(format!("{:016x}", hasher.digest()))
}

/// Compute a content-derived generation identity from a sealed manifest's files map.
///
/// The generation id is a 16-hex-character xxh3_64 over BTreeMap-sorted
/// `"{path}\x00{entry_value}\n"` byte sequences concatenated together.
/// Sorting by key before hashing is the single most critical correctness
/// requirement: `HashMap` iteration order is non-deterministic, so an unsorted
/// feed would produce a different id for the same content across processes.
///
/// Same `files` content always yields the same id (I2 invariant from Stage 1 design).
/// Co-located with `compute_binary_hash` to share the `xxh3_64` import.
pub fn compute_manifest_generation_id(files: &std::collections::HashMap<String, String>) -> String {
    use std::collections::BTreeMap;
    let sorted: BTreeMap<&String, &String> = files.iter().collect();
    let mut buf: Vec<u8> = Vec::with_capacity(files.len() * 64);
    for (path, entry_value) in &sorted {
        buf.extend_from_slice(path.as_bytes());
        buf.push(b'\x00');
        buf.extend_from_slice(entry_value.as_bytes());
        buf.push(b'\n');
    }
    format!("{:016x}", xxh3_64(&buf))
}

/// Path resolution utility that produces root-relative paths (prefixed with `/`).
///
/// When `dir_overrides` is set, `resolve_url()` maps filesystem directory segments
/// to their page-tree equivalents (e.g., `交互/sketch.html` → `interactive/sketch.html`).
pub struct PathResolver {
    /// Optional CSS version for cache busting (e.g., content hash)
    css_version: Option<String>,
    /// Optional user CSS version for cache busting (e.g., content hash of theme/style.css)
    user_css_version: Option<String>,
    /// Optional JS version for cache busting (e.g., content hash of theme.js)
    js_version: Option<String>,
    /// Optional user JS version for cache busting (e.g., content hash of theme/script.js)
    user_js_version: Option<String>,
    /// File-tree → page-tree directory overrides from folder index `url:` frontmatter.
    dir_overrides: std::collections::HashMap<String, String>,
    /// Favicon filename (e.g., "favicon.svg", "favicon.png", "favicon.ico")
    favicon_filename: String,
    /// Whether `assets/favicon-{16,32,180}.png` were rasterized alongside the
    /// SVG favicon. When true, `favicon_link()` emits PNG `<link>` tags for
    /// 16x16/32x32 in addition to the SVG, and `apple_touch_icon_link()`
    /// returns the 180x180 link.
    favicon_has_raster_pngs: bool,
}

impl PathResolver {
    pub fn new() -> Self {
        Self {
            css_version: None,
            user_css_version: None,
            js_version: None,
            user_js_version: None,
            dir_overrides: std::collections::HashMap::new(),
            favicon_filename: "favicon.svg".to_string(),
            favicon_has_raster_pngs: false,
        }
    }

    /// Set CSS version for cache busting (builder pattern)
    pub fn with_css_version(mut self, version: &str) -> Self {
        self.css_version = Some(version.to_string());
        self
    }

    /// Set user CSS version for cache busting (builder pattern)
    pub fn with_user_css_version(mut self, version: &str) -> Self {
        self.user_css_version = Some(version.to_string());
        self
    }

    /// Set JS version for cache busting (builder pattern)
    pub fn with_js_version(mut self, version: &str) -> Self {
        self.js_version = Some(version.to_string());
        self
    }

    /// Set user JS version for cache busting (builder pattern)
    pub fn with_user_js_version(mut self, version: &str) -> Self {
        self.user_js_version = Some(version.to_string());
        self
    }

    /// Set directory overrides for file-tree → page-tree path mapping (builder pattern)
    pub fn with_dir_overrides(mut self, overrides: std::collections::HashMap<String, String>) -> Self {
        self.dir_overrides = overrides;
        self
    }

    /// Set favicon filename (builder pattern)
    pub fn with_favicon_filename(mut self, filename: &str) -> Self {
        self.favicon_filename = filename.to_string();
        self
    }

    /// Mark that `assets/favicon-{16,32,180}.png` are available so
    /// `favicon_link()` and `apple_touch_icon_link()` emit the additional
    /// `<link>` tags.
    pub fn with_favicon_has_raster_pngs(mut self, has: bool) -> Self {
        self.favicon_has_raster_pngs = has;
        self
    }

    /// Generate root-relative CSS path.
    /// When a version is set, hash is baked into the filename: `/_moss/style.<hash>.css`.
    /// Without a version (preview/watch mode), returns `/_moss/style.css`.
    pub fn css_path(&self) -> String {
        match &self.css_version {
            Some(hash) => ServedPath::for_default_stylesheet_hashed(hash).to_relative_url(),
            None => ServedPath::for_default_stylesheet().to_relative_url(),
        }
    }

    /// Generate root-relative JavaScript path for theme.js.
    /// When a version is set, hash is baked into the filename: `/_moss/js/theme.<hash>.js`.
    /// Without a version (preview/watch mode), returns `/_moss/js/theme.js`.
    pub fn js_path(&self) -> String {
        match &self.js_version {
            Some(hash) => ServedPath::for_runtime_js_hashed("theme", hash)
                .expect("'theme' is a valid JS name")
                .to_relative_url(),
            None => ServedPath::for_runtime_js("theme").expect("'theme' is a valid JS name").to_relative_url(),
        }
    }

    /// The lazy-chunk pointers carried by the theme `<script>` tag.
    ///
    /// Every `Load::Lazy` script is content-hashed, so the source that
    /// `import()`s it cannot name it — it reads the URL off this tag instead
    /// (see `selection-actions.ts`'s `loadShareCard` and `hls-boot.ts`). One
    /// owner for all of them rather than one method per chunk: these values are
    /// build-global, byte-identical on every page, and adding the third one
    /// should not mean a third template variable threaded through four render
    /// paths. Attributes on the existing tag rather than extra inline
    /// `<script>`s, for the same reason: fewer elements, and a build-global
    /// value does not trip the preview morph-guard's `scriptsDiffer` reload.
    ///
    /// `hls_hash` is `None` on a site with no ladder, and then the attribute
    /// is absent rather than dangling. It reads as belt-and-braces — the only
    /// reader, `hls-boot.js`, is gated on the same fact and is not on the page
    /// either — but the snapshot suite priced the alternative: an
    /// unconditional attribute is 45 bytes on every page of every moss site,
    /// naming a file that most of them never emit.
    pub fn lazy_chunk_attrs(&self, share_card_hash: &str, hls_hash: Option<&str>) -> String {
        let mut out = format!(
            " data-share-card=\"{}\"",
            Self::hashed_js_url("share-card", share_card_hash)
        );
        if let Some(hash) = hls_hash {
            out.push_str(&format!(" data-hls=\"{}\"", Self::hashed_js_url("hls", hash)));
        }
        out
    }

    fn hashed_js_url(name: &str, hash: &str) -> String {
        ServedPath::for_runtime_js_hashed(name, hash)
            .expect("SITE_SCRIPTS names are valid")
            .to_relative_url()
    }

    /// Generate root-relative permalink for a document
    #[allow(dead_code)]
    pub fn generate_permalink(&self, url_path: &str) -> String {
        format!("/{}", url_path) // allow:served-path-url-construct (page permalink helper, derives user-content URL from url_path)
    }

    /// Resolve a URL path to a root-relative href.
    ///
    /// Absolute URLs (`http://`, `https://`) pass through unchanged.
    /// All other paths delegate to [`moss_core::resolve::output_url::pinned_url`],
    /// the ONE place a source path becomes a URL: same `dir_overrides` mapping,
    /// same leading `/`, and — the part this used to be missing — the same
    /// per-segment percent-encoding.
    ///
    /// **Why delegate rather than re-implement.** This function used to be a
    /// hand-rolled copy of `pinned_url` minus its `percent_encode_path_segments`
    /// call, which made moss emit two different URLs for one file: body embeds
    /// went through `pinned_url` and came out encoded, while listing-card covers
    /// came through here and shipped raw UTF-8 bytes. Everything downstream that
    /// has to RECOGNISE a URL then saw a shape it was not written against —
    /// `orphan_prune`'s reference scanner reads its token class straight off the
    /// encoder's output set, so raw non-ASCII covers read as unreferenced and
    /// were deleted, 404ing the live site (2026-08-06, `LOG-8D03-T1529-08-06`;
    /// diagnosis in docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md).
    /// A second implementation of a conversion is a second answer; keep one.
    ///
    /// Examples (without overrides):
    /// - `"posts/article/index.html"` → `"/posts/article/index.html"`
    /// - `"/assets/photo.jpg"` → `"/assets/photo.jpg"`
    /// - `"https://example.com"` → `"https://example.com"`
    ///
    /// Examples (with `"交互" → "interactive"` override):
    /// - `"交互/sketch.html"` → `"/interactive/sketch.html"`
    /// - `"assets/封面.webp"` → `"/assets/%E5%B0%81%E9%9D%A2.webp"`
    pub fn resolve_url(&self, url_path: &str) -> String {
        if url_path.starts_with("http://") || url_path.starts_with("https://") {
            return url_path.to_string();
        }
        // One converter, no exceptions. This used to be a hand-written copy of
        // `pinned_url` — same dir-override logic, same leading slash, minus the
        // percent-encoding — and that drift is what shipped raw UTF-8 cover URLs
        // the orphan pruner then read as unreferenced and deleted. Keep this a
        // delegation; `resolve_url_agrees_with_the_canonical_converter` fails if
        // it ever grows a second body. (No served-path allow-marker is needed:
        // check-served-paths.sh greps for a `format!` whose literal begins with
        // a slash, and this body no longer builds one. Do not spell that
        // pattern out here — the grep reads comments too, and quoting it is
        // itself a violation.)
        moss_core::resolve::output_url::pinned_url(url_path, &self.dir_overrides)
    }

    /// Generate root-relative user custom CSS path.
    /// When a version is set: `/_moss/theme/style.<hash>.css`.
    /// Without a version: `/_moss/theme/style.css`.
    pub fn user_css_path(&self) -> String {
        match &self.user_css_version {
            Some(hash) => ServedPath::for_custom_stylesheet_hashed(hash).to_relative_url(),
            None => ServedPath::for_custom_stylesheet().to_relative_url(),
        }
    }

    /// Generate root-relative user custom JS path.
    /// When a version is set: `/_moss/theme/script.<hash>.js`.
    /// Without a version: `/_moss/theme/script.js`.
    pub fn user_js_path(&self) -> String {
        match &self.user_js_version {
            Some(hash) => ServedPath::for_custom_script_hashed(hash).to_relative_url(),
            None => ServedPath::for_custom_script().to_relative_url(),
        }
    }

    /// Root-relative URL for a hashed runtime script: `/_moss/js/<name>.<hash>.js`.
    ///
    /// # Panics
    /// If `name` is not a valid JS asset name — every caller passes a
    /// [`SITE_SCRIPTS`](crate::build::emit::scripts::SITE_SCRIPTS) basename,
    /// which the totality test keeps valid.
    pub fn runtime_js_path(&self, name: &str, hash: &str) -> String {
        ServedPath::for_runtime_js_hashed(name, hash)
            .unwrap_or_else(|_| panic!("'{name}' is a valid JS name"))
            .to_relative_url()
    }

    /// A gated `<script src>` tag for a runtime script, or `""` when the gate
    /// is off — with the markup it acts on suppressed, the script would have
    /// nothing to attach to.
    ///
    /// **Every gate here is SITE-level, never per-page**, and that is load
    /// bearing rather than incidental: the preview's morph-guard compares the
    /// script set across a navigation and forces a full reload when it
    /// differs, so a tag that came and went per page would turn every such
    /// navigation into a reload.
    ///
    /// This is the one owner of runtime `<script>` tag construction — of ONE
    /// tag. Which tags the shell block contains, in what order and with which
    /// `defer`, is `emit::scripts::ScriptAssets::shell_tags` reading the
    /// `SITE_SCRIPTS` table (#1149); the direct callers left here are the
    /// media-collection page, which places its own. It was four byte-identical
    /// copies (preview, heading-anchor, math-copy,
    /// search), each with its own path helper whose only caller was its own
    /// tag function — the fifth copy would have arrived with margin
    /// sidenotes. Tag construction stays HERE rather than moving into
    /// `emit::scripts` alongside the gate that decides the FILE; see that
    /// module's header for why that split is deliberate.
    pub fn runtime_js_tag(&self, name: &str, hash: &str, enabled: bool, defer: bool) -> String {
        if !enabled {
            return String::new();
        }
        format!(
            "\n    <script src=\"{}\"{}></script>",
            self.runtime_js_path(name, hash),
            if defer { " defer" } else { "" }
        )
    }

    /// Full `user_js_tag` HTML: the `window.mossTheme.base` global followed by
    /// the user theme `<script src>`. Single source so the two emit sites
    /// (article/home render + folder-index render) can't drift.
    pub fn user_js_tag(&self) -> String {
        format!(
            "\n    <script>window.mossTheme={{base:new URL(\"{}\",location.href).href}};</script>\n    <script src=\"{}\"></script>",
            ServedPath::theme_mount_url(),
            self.user_js_path()
        )
    }

    /// Generate root-relative favicon path
    pub fn favicon_path(&self) -> String {
        let ext = self.favicon_filename
            .rsplit('.')
            .next()
            .unwrap_or("svg");
        ServedPath::for_favicon(ext)
            .map(|sp| sp.to_relative_url())
            .unwrap_or_else(|_| format!("/assets/{}", self.favicon_filename)) // allow:served-path-url-construct (fallback for malformed favicon ext rejected by for_favicon; normal path uses ServedPath::for_favicon)
    }

    /// Generate the full `<link>` tag for the favicon with correct MIME type.
    /// When raster PNGs are available, emits the SVG `<link>` plus PNG `<link>`s
    /// at 16x16 and 32x32 for older browsers and email/messaging clients.
    pub fn favicon_link(&self) -> String {
        let mime = match self.favicon_filename.rsplit('.').next() {
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            Some("ico") => "image/x-icon",
            _ => "image/svg+xml",
        };
        let mut out = format!(
            r#"<link rel="icon" type="{}" href="{}">"#,
            mime,
            self.favicon_path()
        );
        if self.favicon_has_raster_pngs {
            out.push_str(
                "\n    <link rel=\"icon\" type=\"image/png\" sizes=\"32x32\" href=\"/assets/favicon-32.png\">",
            );
            out.push_str(
                "\n    <link rel=\"icon\" type=\"image/png\" sizes=\"16x16\" href=\"/assets/favicon-16.png\">",
            );
        }
        out
    }

    /// Generate the apple-touch-icon `<link>` tag if raster PNGs are available.
    /// Returns `None` otherwise — the apple-touch-icon must be a real raster
    /// PNG (Apple Link Presentation can't use SVG and silently fails).
    pub fn apple_touch_icon_link(&self) -> Option<String> {
        if self.favicon_has_raster_pngs {
            Some(
                r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/favicon-180.png">"#
                    .to_string(),
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PathResolver tests - verify root-relative path generation
    #[test]
    fn test_path_resolver_returns_root_relative_paths() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.css_path(), "/_moss/style.css");
        assert_eq!(resolver.js_path(), "/_moss/js/theme.js");
        assert_eq!(resolver.favicon_path(), "/assets/favicon.svg");
    }

    #[test]
    fn test_favicon_png() {
        let resolver = PathResolver::new().with_favicon_filename("favicon.png");
        assert_eq!(resolver.favicon_path(), "/assets/favicon.png");
        assert_eq!(
            resolver.favicon_link(),
            r#"<link rel="icon" type="image/png" href="/assets/favicon.png">"#
        );
    }

    #[test]
    fn test_favicon_svg_link() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.favicon_link(),
            r#"<link rel="icon" type="image/svg+xml" href="/assets/favicon.svg">"#
        );
    }

    #[test]
    fn test_favicon_ico() {
        let resolver = PathResolver::new().with_favicon_filename("favicon.ico");
        assert_eq!(resolver.favicon_path(), "/assets/favicon.ico");
        assert_eq!(
            resolver.favicon_link(),
            r#"<link rel="icon" type="image/x-icon" href="/assets/favicon.ico">"#
        );
    }

    // Cache busting tests - CSS path should embed hash in filename
    #[test]
    fn test_css_path_with_cache_bust() {
        let resolver = PathResolver::new().with_css_version("abc123");
        assert_eq!(resolver.css_path(), "/_moss/style.abc123.css");
    }

    #[test]
    fn test_css_path_without_cache_bust() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.css_path(), "/_moss/style.css");
    }

    // resolve_url tests - root-relative URL resolution
    #[test]
    fn test_resolve_url_relative_path() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.resolve_url("posts/article/index.html"), "/posts/article/index.html");
        assert_eq!(resolver.resolve_url("rss.xml"), "/rss.xml");
    }

    #[test]
    fn test_resolve_url_absolute_http_unchanged() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("https://example.com/photo.jpg"),
            "https://example.com/photo.jpg"
        );
        assert_eq!(
            resolver.resolve_url("http://example.com/photo.jpg"),
            "http://example.com/photo.jpg"
        );
    }

    #[test]
    fn test_resolve_url_already_root_relative() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("/assets/photo.jpg"),
            "/assets/photo.jpg"
        );
    }

    #[test]
    fn test_resolve_url_asset_path() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("assets/photo.jpg"),
            "/assets/photo.jpg"
        );
    }

    // User CSS path tests
    #[test]
    fn test_user_css_path() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.user_css_path(), "/_moss/theme/style.css");
    }

    #[test]
    fn test_user_css_path_with_cache_bust() {
        let resolver = PathResolver::new().with_user_css_version("abc123");
        assert_eq!(resolver.user_css_path(), "/_moss/theme/style.abc123.css");
    }

    #[test]
    fn test_both_css_versions_set_independently() {
        let resolver = PathResolver::new()
            .with_css_version("css111")
            .with_user_css_version("user222");
        assert_eq!(resolver.css_path(), "/_moss/style.css111.css");
        assert_eq!(resolver.user_css_path(), "/_moss/theme/style.user222.css");
    }

    // dir_overrides tests — resolve_url maps file-tree paths to page-tree paths
    #[test]
    fn test_resolve_url_with_dir_overrides() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("交互".to_string(), "interactive".to_string());
        let resolver = PathResolver::new().with_dir_overrides(overrides);
        assert_eq!(
            resolver.resolve_url("交互/night-in-the-woods.html"),
            "/interactive/night-in-the-woods.html"
        );
    }

    #[test]
    fn test_resolve_url_with_nested_dir_overrides() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("文字".to_string(), "writings".to_string());
        overrides.insert("文字/游记".to_string(), "travel".to_string());
        let resolver = PathResolver::new().with_dir_overrides(overrides);
        assert_eq!(
            resolver.resolve_url("文字/游记/photo.jpg"),
            "/writings/travel/photo.jpg"
        );
    }

    #[test]
    fn test_resolve_url_no_overrides_unchanged() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("posts/article/index.html"),
            "/posts/article/index.html"
        );
    }

    #[test]
    fn test_resolve_url_overrides_ignore_absolute_urls() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("交互".to_string(), "interactive".to_string());
        let resolver = PathResolver::new().with_dir_overrides(overrides);
        assert_eq!(
            resolver.resolve_url("https://example.com/交互/test"),
            "https://example.com/交互/test"
        );
    }

    #[test]
    fn test_resolve_url_lowercases_title_case_folder() {
        // Regression: a Title-Case source folder (no override) must produce
        // a lowercase URL segment so the rendered <img src> matches the
        // markdown page URL, which compute_url_path also slugifies.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("News/new-hub.png"),
            "/news/new-hub.png"
        );
    }

    #[test]
    fn test_resolve_url_preserves_filename_case() {
        // Asset filenames must keep their original case so the URL matches
        // the actual file on disk. Only the directory portion is slugified.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("News/Winter-Song.mov"),
            "/news/Winter-Song.mov"
        );
        assert_eq!(
            resolver.resolve_url("Photos/GiorgioDeChirico.jpg"),
            "/photos/GiorgioDeChirico.jpg"
        );
    }

    #[test]
    fn test_resolve_url_nested_title_case_folders_lowercased() {
        // Every intermediate directory segment is slugified independently.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("My Section/Sub Section/Page.png"),
            "/my-section/sub-section/Page.png"
        );
    }

    #[test]
    fn test_resolve_url_webp_variant_in_title_case_folder() {
        // Image variants (webp from a jpg/png) flow through resolve_url
        // alongside the original. The variant filename keeps its case and
        // extension; only the directory is slugified.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("News/new-hub.webp"),
            "/news/new-hub.webp"
        );
        assert_eq!(
            resolver.resolve_url("Photos/GiorgioDeChirico.webp"),
            "/photos/GiorgioDeChirico.webp"
        );
    }

    #[test]
    fn test_resolve_url_override_wins_over_slugify() {
        // When a Title-Case folder has an override, the override target wins
        // over generic slugification.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("News".to_string(), "blog".to_string());
        let resolver = PathResolver::new().with_dir_overrides(overrides);
        assert_eq!(
            resolver.resolve_url("News/new-hub.png"),
            "/blog/new-hub.png"
        );
    }

    // -----------------------------------------------------------------------
    // resolve_url percent-encoding — the regression that broke the live site
    //
    // `resolve_url` used to be a hand-rolled copy of `output_url::pinned_url`
    // minus the encoding step, so covers went out raw while body images went
    // out encoded. The orphan pruner scans emitted HTML for asset tokens with
    // an ASCII-shaped regex; a raw CJK cover URL is invisible to it, so the
    // file was pruned as an orphan and 404'd. These assert the one property
    // that closes the class: whatever a caller hands `resolve_url`, what comes
    // back is a URL, encoded exactly once.
    // See docs/archive/2026-08-06-orphan-prune-false-negative-and-parse-cache-gate.md
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_url_percent_encodes_non_ascii_segments() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("獎項/封面.jpg"),
            "/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.jpg"
        );
    }

    #[test]
    fn resolve_url_percent_encodes_spaces_in_filenames() {
        // Directory case is slugified (space → `-`); the filename is not, so
        // its space must survive as `%20` rather than as a raw byte.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("News/Winter Song.mov"),
            "/news/Winter%20Song.mov"
        );
    }

    #[test]
    fn resolve_url_agrees_with_the_canonical_converter() {
        // The whole point of the fix: there is one converter, and `resolve_url`
        // is a thin wrapper over it. If these ever diverge again, the two
        // emitters are back and the pruner bug returns with them.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("交互".to_string(), "interactive".to_string());
        let resolver = PathResolver::new().with_dir_overrides(overrides.clone());
        for input in [
            "獎項/封面.jpg",
            "News/Winter Song.mov",
            "交互/night-in-the-woods.html",
            "posts/article/index.html",
            "img/a,b.png",
        ] {
            assert_eq!(
                resolver.resolve_url(input),
                moss_core::resolve::output_url::pinned_url(input, &overrides),
                "divergence on {input}"
            );
        }
    }

    #[test]
    fn resolve_url_leaves_external_urls_untouched() {
        // Encoding an absolute URL would mangle its `://` and query string.
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.resolve_url("https://example.com/獎項/封面.jpg?w=800"),
            "https://example.com/獎項/封面.jpg?w=800"
        );
    }

    // ---------------------------------------------------------------------------
    // compute_manifest_generation_id
    // ---------------------------------------------------------------------------

    #[test]
    fn generation_id_is_exactly_16_hex_chars() {
        let mut files = std::collections::HashMap::new();
        files.insert("index.html".to_string(), "100644:abc123".to_string());
        let id = super::compute_manifest_generation_id(&files);
        assert_eq!(id.len(), 16, "generation_id must be exactly 16 hex chars, got: {id}");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "generation_id must be lowercase hex, got: {id}"
        );
    }

    #[test]
    fn generation_id_same_content_same_id() {
        let mut files1 = std::collections::HashMap::new();
        files1.insert("index.html".to_string(), "100644:abc123".to_string());
        files1.insert("style.css".to_string(), "100644:def456".to_string());

        let mut files2 = std::collections::HashMap::new();
        // Insert in reverse order — HashMap does not preserve insertion order
        files2.insert("style.css".to_string(), "100644:def456".to_string());
        files2.insert("index.html".to_string(), "100644:abc123".to_string());

        assert_eq!(
            super::compute_manifest_generation_id(&files1),
            super::compute_manifest_generation_id(&files2),
            "same content inserted in different order must produce same generation_id"
        );
    }

    #[test]
    fn generation_id_different_content_different_id() {
        let mut files1 = std::collections::HashMap::new();
        files1.insert("index.html".to_string(), "100644:abc123".to_string());

        let mut files2 = std::collections::HashMap::new();
        files2.insert("index.html".to_string(), "100644:ffffff".to_string());

        assert_ne!(
            super::compute_manifest_generation_id(&files1),
            super::compute_manifest_generation_id(&files2),
            "different hash values must produce different generation_ids"
        );
    }

    #[test]
    fn generation_id_adding_file_changes_id() {
        let mut files1 = std::collections::HashMap::new();
        files1.insert("index.html".to_string(), "100644:abc123".to_string());

        let mut files2 = files1.clone();
        files2.insert("extra.html".to_string(), "100644:999999".to_string());

        assert_ne!(
            super::compute_manifest_generation_id(&files1),
            super::compute_manifest_generation_id(&files2),
            "adding a file must change generation_id"
        );
    }

    #[test]
    fn generation_id_empty_manifest_is_stable() {
        let files: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let id1 = super::compute_manifest_generation_id(&files);
        let id2 = super::compute_manifest_generation_id(&files);
        assert_eq!(id1, id2, "empty manifest must produce stable generation_id");
        assert_eq!(id1.len(), 16);
    }

    // ---------------------------------------------------------------------------
    // compute_binary_hash_file
    // ---------------------------------------------------------------------------

    /// Proves that the streaming file hasher produces the identical digest as
    /// the one-shot `compute_binary_hash(&bytes)`.  Uses a file larger than the
    /// internal read buffer (64 KB) so that the streaming path actually exercises
    /// multiple chunks.
    #[test]
    fn compute_binary_hash_file_matches_one_shot_for_multi_chunk_file() {
        use std::fs;
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create target/test-tmp");
        let tmp = tempfile::TempDir::new_in(&base)
        .unwrap();
        // Write 200 KB of data — well over the 64 KB buffer so streaming spans
        // multiple reads.
        let data: Vec<u8> = (0u32..51_200).flat_map(|i| i.to_le_bytes()).collect(); // 204 800 bytes
        let path = tmp.path().join("big_asset.bin");
        fs::write(&path, &data).unwrap();

        let one_shot = compute_binary_hash(&data);
        let streaming = compute_binary_hash_file(&path).expect("compute_binary_hash_file failed");

        assert_eq!(
            one_shot, streaming,
            "streaming hash must equal one-shot hash for multi-chunk file"
        );
        assert_eq!(streaming.len(), 16, "xxh3 hash must be exactly 16 hex chars");
    }

    // JS cache busting tests
    #[test]
    fn test_js_path_with_cache_bust() {
        let resolver = PathResolver::new().with_js_version("def456");
        assert_eq!(resolver.js_path(), "/_moss/js/theme.def456.js");
    }

    #[test]
    fn test_js_path_without_cache_bust() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.js_path(), "/_moss/js/theme.js");
    }

    // Permalink test
    #[test]
    fn test_generate_permalink() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.generate_permalink("posts/hello/index.html"), "/posts/hello/index.html");
        assert_eq!(resolver.generate_permalink("index.html"), "/index.html");
    }

    // User JS path tests
    #[test]
    fn test_user_js_path() {
        let resolver = PathResolver::new();
        assert_eq!(resolver.user_js_path(), "/_moss/theme/script.js");
    }

    #[test]
    fn test_user_js_path_with_cache_bust() {
        let resolver = PathResolver::new().with_user_js_version("js789");
        assert_eq!(resolver.user_js_path(), "/_moss/theme/script.js789.js");
    }

    #[test]
    fn test_all_versions_set_independently() {
        let resolver = PathResolver::new()
            .with_css_version("css111")
            .with_user_css_version("ucss222")
            .with_js_version("js333")
            .with_user_js_version("ujs444");
        assert_eq!(resolver.css_path(), "/_moss/style.css111.css");
        assert_eq!(resolver.user_css_path(), "/_moss/theme/style.ucss222.css");
        assert_eq!(resolver.js_path(), "/_moss/js/theme.js333.js");
        assert_eq!(resolver.user_js_path(), "/_moss/theme/script.ujs444.js");
    }

    /// One test per behaviour, not one per script: `runtime_js_tag` is the
    /// single owner, so a per-script copy of this would only re-assert
    /// `format!`. It replaces six such copies.
    #[test]
    fn runtime_js_path_is_name_dot_hash_dot_js() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.runtime_js_path("preview", "aabb1234"),
            "/_moss/js/preview.aabb1234.js"
        );
        // A hyphenated name is the case a naive join would break.
        assert_eq!(
            resolver.runtime_js_path("heading-anchor", "eeff9012"),
            "/_moss/js/heading-anchor.eeff9012.js"
        );
    }

    /// Whole-or-nothing on the gate: a site that opted out of a feature must
    /// not ship the script for markup it never emits. `defer` rides the same
    /// call because search is the one script that needs it.
    #[test]
    fn runtime_js_tag_is_whole_or_nothing_and_carries_defer() {
        let resolver = PathResolver::new();
        assert_eq!(
            resolver.runtime_js_tag("math-copy", "aabb3456", true, false),
            "\n    <script src=\"/_moss/js/math-copy.aabb3456.js\"></script>"
        );
        assert_eq!(resolver.runtime_js_tag("math-copy", "aabb3456", false, false), "");
        assert_eq!(
            resolver.runtime_js_tag("search", "ccdd7890", true, true),
            "\n    <script src=\"/_moss/js/search.ccdd7890.js\" defer></script>"
        );
    }
}
