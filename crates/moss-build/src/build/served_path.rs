//! A served path is the canonical identity of any asset moss produces.
//!
//! For every ServedPath p:
//!   disk_path = output_root.join(p.as_str())
//!   relative  = "/" + p.as_str()
//!   absolute  = site_url.to_absolute(relative)
//!
//! There is no other way to write a file inside the build output, and no
//! other way to emit a URL into HTML, RSS, sitemap, manifest, or JSON-LD.
//! Reviewers: any format!() or string concat producing a "/..." path in
//! the build pipeline outside this module is a blocker.
//!
//! Construction is fallible via `from_source` (returns
//! `Result<Self, ServedPathError>`); the type rejects `.moss/`,
//! `_moss/` (in user input), `..`, leading `/`, and empty inputs at
//! parse time. Generated artifacts use named constructors
//! (`for_og_card`, `for_sitemap`, `for_theme_asset`, etc.) whose served
//! paths are decided in one place. `for_jupyterlite_asset(&Path)` is
//! the one named bypass for byte-for-byte third-party assets.
//!
//! # History (case-sensitivity origin)
//!
//! `ServedPath` is the only type accepted by the build pipeline's emit and
//! register APIs. Constructing one runs `slugify_path_segments` on the input,
//! making it impossible to write or register an un-normalized path.
//!
//! Why this exists: a source `Resources/orbit-model.ipynb` used to produce
//! a notebook viewer at `Resources/orbit-model.html` (capital R preserved).
//! On case-insensitive APFS this worked; on Dropbox CloudStorage (case-sensitive
//! FUSE) and Linux servers, the on-disk file landed at lowercase while the
//! manifest registered the capital path, causing deploy ENOENT on canonicalize.

use crate::build::scan::slug::slugify_dir_path;
use std::fmt;

/// Directory every auto-generated OG card is written under, shared by
/// [`ServedPath::for_og_card`] and the incremental render skip's card
/// carry-forward (`build/render/blocking.rs`), which
/// recognizes cards by this prefix alone.
pub const OG_CARD_PREFIX: &str = "_moss/og/";

/// Reserved prefix for the email-fallback math PNGs. Append-only by design:
/// an entry here is never dropped for being unreadable, because
/// the published site still serves it and un-promising it would delete it
/// from a live site.
pub const MATH_PNG_PREFIX: &str = "_moss/math/";

/// Directory every downloaded external-link cover image is written under.
/// See [`ServedPath::for_remote_cover`].
pub const REMOTE_COVER_PREFIX: &str = "_moss/link/";

/// A normalized served path. Directory segments are slugged; the basename
/// is preserved verbatim.
///
/// Why preserve the basename: third-party JS bundles (JupyterLite, MathJax,
/// p5.js) load asset files by their exact filenames. Slug-rewriting
/// `MathJax_Main-Bold.woff` to `mathjax-main-bold.woff` produces a 404 at
/// runtime. Directory segments, however, leak into URL routing on
/// case-sensitive servers (a real site's bug) so they must be normalized.
///
/// For *pretty URLs* of articles, the filename slug is applied separately
/// inside `compute_url_path` via `generate_slug(stem)` — that's intentional.
///
/// Construct via [`ServedPath::from_source`] (user-derived paths) or one of
/// the named `for_*` constructors (generated artifacts). The inner `String`
/// is private so the only way to produce a non-passthrough ServedPath is
/// through the normalizer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServedPath(String);

/// Failure modes for ServedPath construction. Each variant corresponds to
/// one invariant the type enforces at parse time.
#[derive(Debug)]
pub enum ServedPathError {
    /// Path contains the literal segment `.moss/` — moss's internal
    /// workdir namespace, blocked by Caddy at the edge. Generated
    /// artifacts go to `_moss/` instead.
    ReservedMossPrefix,
    /// Path contains `..` — would let a user-source path escape the
    /// served root.
    ParentEscape,
    /// Path begins with `/`. ServedPath is relative-to-root by
    /// construction; the leading slash is added by `to_relative_url`.
    AbsolutePath,
    /// Input string is empty or whitespace-only after trim.
    Empty,
    /// Constructor input doesn't match the expected shape (e.g.,
    /// for_og_card got a non-hex hash, for_rss got a slug with `/`).
    InvalidInput(&'static str),
}

impl std::fmt::Display for ServedPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReservedMossPrefix => write!(f, "path contains forbidden segment '.moss/'"),
            Self::ParentEscape => write!(f, "path contains '..' segment"),
            Self::AbsolutePath => write!(f, "path is absolute (must be relative to served root)"),
            Self::Empty => write!(f, "path is empty"),
            Self::InvalidInput(what) => write!(f, "invalid input: {}", what),
        }
    }
}

impl std::error::Error for ServedPathError {}

/// Single source of truth for the theme mount directory (without leading `/` or
/// trailing `/`).  All `for_custom_*` / `for_theme_asset` / `theme_mount_url`
/// constructors are built on top of this constant so a rename can never silently
/// diverge.
const THEME_MOUNT: &str = "_moss/theme";

/// Single source of truth for the Pagefind search-index mount directory
/// (without leading `/` or trailing `/`). `for_search_asset` and
/// `search_mount_url` are both built on it; the client runtime reads the URL
/// form off `search_mount_url()`.
const SEARCH_MOUNT: &str = "_moss/pagefind";

impl ServedPath {
    /// Normalize a source-derived path. Slugs directory segments, preserves
    /// the final basename verbatim. Idempotent — re-wrapping is safe.
    ///
    /// Returns `Err` if the input is empty, absolute, contains `..`, or
    /// contains the reserved segment `.moss/` or `_moss/`.
    pub fn from_source(s: &str) -> Result<Self, ServedPathError> {
        Self::validate_source(s)?;
        Ok(ServedPath(slugify_dir_path(s)))
    }

    /// Rehydrate a path previously stored by the build in a metadata cache.
    /// Cached paths are already generated and normalized, so unlike source
    /// paths they may use moss's reserved `_moss/` namespace.
    pub(crate) fn from_cached(s: &str) -> Result<Self, ServedPathError> {
        let normalized = Self::normalize_relative(s)?;
        if normalized.split('/').any(|segment| segment == ".") {
            return Err(ServedPathError::InvalidInput("cached path contains '.' segment"));
        }
        if normalized.split('/').any(|segment| segment == ".moss") {
            return Err(ServedPathError::ReservedMossPrefix);
        }
        Ok(ServedPath(normalized))
    }

    /// Run all parse-time invariants for user-source paths. Rejects both
    /// `.moss/` (moss internal workdir) and `_moss/` (moss framework
    /// namespace, reserved for build-emitted artifacts). The `for_*`
    /// constructors that legitimately produce `_moss/...` paths bypass
    /// this validator by constructing their inner String directly.
    fn validate_source(s: &str) -> Result<(), ServedPathError> {
        let trimmed = Self::normalize_relative(s)?;
        if trimmed.split('/').any(|seg| seg == ".moss" || seg == "_moss") {
            return Err(ServedPathError::ReservedMossPrefix);
        }
        Ok(())
    }

    /// Normalize separators and validate the invariants shared by source and
    /// cached relative paths. Keeping this at the boundary prevents a Windows
    /// `..\\secret` from crossing into filesystem or HTML path handling.
    fn normalize_relative(s: &str) -> Result<String, ServedPathError> {
        let trimmed = moss_core::slug::normalize_separators(s.trim());
        if trimmed.is_empty() {
            return Err(ServedPathError::Empty);
        }
        if trimmed.starts_with('/') {
            return Err(ServedPathError::AbsolutePath);
        }
        if trimmed.split('/').any(|segment| segment == "..") {
            return Err(ServedPathError::ParentEscape);
        }
        Ok(trimmed)
    }

    /// The single legitimate third-party asset bypass: JupyterLite ships
    /// case-sensitive paths whose basenames are referenced by hardcoded
    /// runtime JS strings. Re-slugging would 404 at runtime. This
    /// constructor is the only way to produce a ServedPath whose disk
    /// layout matches the source bytes verbatim.
    ///
    /// Grep for callers of this method to audit every place where the
    /// served-path invariants are intentionally bypassed.
    pub fn for_jupyterlite_asset(rel: &std::path::Path) -> Self {
        ServedPath(rel.to_string_lossy().into_owned())
    }

    /// Auto-generated OG card. Hash is 16 lowercase hex chars (validated).
    /// The producer is `og_card::content_hash` which truncates SHA-256 to
    /// 8 bytes -> 16 hex chars; any length mismatch means the call site
    /// passed something other than that function's output.
    pub fn for_og_card(content_hash: &str) -> Result<Self, ServedPathError> {
        if content_hash.is_empty() {
            return Err(ServedPathError::InvalidInput("og card hash is empty"));
        }
        if content_hash.len() != 16 {
            return Err(ServedPathError::InvalidInput("og card hash must be 16 hex chars"));
        }
        if !content_hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
            return Err(ServedPathError::InvalidInput("og card hash must be lowercase hex"));
        }
        Ok(ServedPath(format!("{}{}.png", OG_CARD_PREFIX, content_hash)))
    }

    /// Email/RSS math PNG. Hash is 16 lowercase hex chars
    /// (validated), produced by `emit::math_png::content_hash` — SHA-256 over
    /// the full render input tuple, truncated to 8 bytes. Passthrough
    /// constructor on purpose: `from_source` slug-lowercases directory
    /// segments, and any future change to that normalization must never move
    /// a math PNG — the URL is baked into already-sent emails and cached
    /// forever by Gmail's image proxy / Apple MPP.
    pub fn for_math_png(content_hash: &str) -> Result<Self, ServedPathError> {
        if content_hash.is_empty() {
            return Err(ServedPathError::InvalidInput("math png hash is empty"));
        }
        if content_hash.len() != 16 {
            return Err(ServedPathError::InvalidInput("math png hash must be 16 hex chars"));
        }
        if !content_hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
            return Err(ServedPathError::InvalidInput("math png hash must be lowercase hex"));
        }
        Ok(ServedPath(format!("{}{}.png", MATH_PNG_PREFIX, content_hash)))
    }

    /// A downloaded external-link cover image (an external `:::grid` card's
    /// `og:image`/`twitter:image`), named by the content hash of the
    /// downloaded IMAGE BYTES (16 hex chars of the object-store oid) —
    /// never by the linked page's URL or the remote image URL, and never
    /// under a source-derived path (`from_source` rejects `_moss/`
    /// entirely). Two pages whose og:image resolves to identical bytes
    /// share one cover. See `build::media::remote_cover`. `ext` is the
    /// format the download was sniffed as (never trusted from the URL or a
    /// `Content-Type` header alone) — one of the raster extensions the
    /// synthesizer's `<picture>` gate recognizes.
    pub fn for_remote_cover(content_hash: &str, ext: &str) -> Result<Self, ServedPathError> {
        if content_hash.is_empty() {
            return Err(ServedPathError::InvalidInput("remote cover hash is empty"));
        }
        if content_hash.len() != 16 {
            return Err(ServedPathError::InvalidInput("remote cover hash must be 16 hex chars"));
        }
        if !content_hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
            return Err(ServedPathError::InvalidInput("remote cover hash must be lowercase hex"));
        }
        if !matches!(ext, "jpg" | "jpeg" | "png" | "webp") {
            return Err(ServedPathError::InvalidInput("remote cover extension must be jpg, jpeg, png, or webp"));
        }
        Ok(ServedPath(format!("{}{}.{}", REMOTE_COVER_PREFIX, content_hash, ext)))
    }

    /// Site favicon. `ext` is a bare extension (no dot, no slash).
    pub fn for_favicon(ext: &str) -> Result<Self, ServedPathError> {
        if ext.is_empty() {
            return Err(ServedPathError::InvalidInput("favicon ext is empty"));
        }
        if ext.contains('.') || ext.contains('/') {
            return Err(ServedPathError::InvalidInput("favicon ext must not contain '.' or '/'"));
        }
        Ok(ServedPath(format!("assets/favicon.{}", ext)))
    }

    /// Apple touch icon. Path is fixed.
    pub fn for_apple_touch_icon() -> Self {
        ServedPath("assets/apple-touch-icon.png".to_string())
    }

    /// Sitemap. Path is fixed.
    pub fn for_sitemap() -> Self {
        ServedPath("sitemap.xml".to_string())
    }

    /// RSS feed. `slug` may be empty (root) or a single segment (no slashes).
    pub fn for_rss(slug: &str) -> Result<Self, ServedPathError> {
        if slug.contains('/') {
            return Err(ServedPathError::InvalidInput("rss slug must be a single segment"));
        }
        let inner = if slug.is_empty() {
            "rss.xml".to_string()
        } else {
            format!("{}/rss.xml", slug)
        };
        Ok(ServedPath(inner))
    }

    /// LLMs.txt. Path is fixed.
    pub fn for_llms_txt() -> Self {
        ServedPath("llms.txt".to_string())
    }

    /// robots.txt. Path is fixed.
    pub fn for_robots_txt() -> Self {
        ServedPath("robots.txt".to_string())
    }

    /// moss default icon — the fallback site icon when the site provides
    /// none. Embedded in the binary; emitted to disk once per build that
    /// needs it.
    pub fn for_moss_default_icon() -> Self {
        ServedPath("_moss/default-icon.png".to_string())
    }

    /// Default theme CSS, emitted from the binary-embedded default stylesheet
    /// asset.
    pub fn for_default_stylesheet() -> Self {
        ServedPath("_moss/style.css".to_string())
    }

    /// User theme override CSS — content authored by the user in
    /// `.moss/theme/style.css`, served verbatim under the theme mount.
    pub fn for_custom_stylesheet() -> Self {
        ServedPath(format!("{}/style.css", THEME_MOUNT))
    }

    /// User runtime JS override — content authored by the user in
    /// `.moss/theme/script.js`, served verbatim under the theme mount.
    pub fn for_custom_script() -> Self {
        ServedPath(format!("{}/script.js", THEME_MOUNT))
    }

    /// moss runtime JavaScript module. `name` is the bare filename stem
    /// without extension (e.g., "theme", "iframe-bridge"); the constructor
    /// adds the `.js` and the `_moss/js/` prefix. Path-traversal-safe:
    /// rejects names containing `/`, `.`, or empty input.
    pub fn for_runtime_js(name: &str) -> Result<Self, ServedPathError> {
        if name.is_empty() {
            return Err(ServedPathError::InvalidInput("runtime JS name is empty"));
        }
        if name.contains('/') || name.contains('.') {
            return Err(ServedPathError::InvalidInput("runtime JS name must be a bare stem"));
        }
        Ok(ServedPath(format!("_moss/js/{}.js", name)))
    }

    /// moss runtime JavaScript module with content-hash embedded in filename.
    /// `name` is the bare stem (no dots, no slashes); `hash` is the 16-hex xxh3
    /// string from `compute_binary_hash`. Output: `_moss/js/{name}.{hash}.js`.
    /// Hyphens are permitted (e.g. "thumb-swap", "heading-anchor").
    pub fn for_runtime_js_hashed(name: &str, hash: &str) -> Result<Self, ServedPathError> {
        if name.is_empty() {
            return Err(ServedPathError::InvalidInput("runtime JS name is empty"));
        }
        if name.contains('/') || name.contains('.') {
            return Err(ServedPathError::InvalidInput(
                "runtime JS name must be a bare stem (no dots or slashes)",
            ));
        }
        Ok(ServedPath(format!("_moss/js/{}.{}.js", name, hash)))
    }

    /// Default theme CSS with content-hash embedded in filename.
    /// `hash` is the 16-hex xxh3 string from `compute_binary_hash`.
    /// Output: `_moss/style.{hash}.css`.
    pub fn for_default_stylesheet_hashed(hash: &str) -> Self {
        ServedPath(format!("_moss/style.{}.css", hash))
    }

    /// Feature stylesheet with content-hash embedded in filename.
    /// Output: `_moss/css/{name}.{hash}.css`.
    ///
    /// The `_moss/css/` mount is separate from the bare `_moss/style.<hash>.css`
    /// of the main sheet: these resolve a phase later (see
    /// `build::emit::feature_styles`) and are linked from a slot, not from the
    /// page template.
    pub fn for_feature_stylesheet_hashed(name: &str, hash: &str) -> Result<Self, ServedPathError> {
        if name.is_empty() {
            return Err(ServedPathError::InvalidInput("feature CSS name is empty"));
        }
        if name.contains('/') || name.contains('.') {
            return Err(ServedPathError::InvalidInput(
                "feature CSS name must be a bare stem (no dots or slashes)",
            ));
        }
        Ok(ServedPath(format!("_moss/css/{}.{}.css", name, hash)))
    }

    /// User theme override CSS with content-hash embedded in filename.
    /// Output: `_moss/theme/style.{hash}.css`.
    pub fn for_custom_stylesheet_hashed(hash: &str) -> Self {
        ServedPath(format!("{}/style.{}.css", THEME_MOUNT, hash))
    }

    /// User runtime JS override with content-hash embedded in filename.
    /// Output: `_moss/theme/script.{hash}.js`.
    pub fn for_custom_script_hashed(hash: &str) -> Self {
        ServedPath(format!("{}/script.{}.js", THEME_MOUNT, hash))
    }

    /// A single file inside the Pagefind search-index bundle.
    ///
    /// `rel` is the path *within* the bundle as Pagefind reports it from
    /// `PagefindIndex::get_files()` (e.g. `pagefind.js`, `wasm.en.pagefind`,
    /// `fragment/en_1a2b3c.pf_fragment`). Pagefind's own runtime resolves
    /// sibling chunk URLs relative to the directory it was loaded from, and
    /// those names are baked into `pagefind-entry.json` — so this constructor
    /// is a **passthrough** on `rel` (no slugging), like
    /// [`for_jupyterlite_asset`][Self::for_jupyterlite_asset]. Lowercasing a
    /// directory segment here would 404 at query time.
    ///
    /// Path-traversal-safe: rejects empty input, absolute paths, and `..`.
    /// Originally planned as `for_search_index()`; named
    /// `for_search_asset` because it addresses one file in a tree, not a
    /// single index blob.
    pub fn for_search_asset(rel: &str) -> Result<Self, ServedPathError> {
        let trimmed = moss_core::slug::normalize_separators(rel.trim());
        if trimmed.is_empty() {
            return Err(ServedPathError::Empty);
        }
        if trimmed.starts_with('/') {
            return Err(ServedPathError::AbsolutePath);
        }
        if trimmed.split('/').any(|seg| seg == "..") {
            return Err(ServedPathError::ParentEscape);
        }
        Ok(ServedPath(format!("{}/{}", SEARCH_MOUNT, trimmed)))
    }

    /// Root-relative URL of the search-index mount, with a trailing slash so
    /// it works as a base for `new URL(rel, base)` and as Pagefind's
    /// `bundlePath`. Example: "/_moss/pagefind/".
    pub fn search_mount_url() -> &'static str {
        "/_moss/pagefind/"
    }

    /// Whether a manifest key addresses the search-index bundle.
    ///
    /// Manifest-key domain (no leading slash), which is why this is not
    /// `search_mount_url().strip_prefix('/')` at every call site.
    pub fn is_search_asset(key: &str) -> bool {
        key.starts_with(SEARCH_MOUNT)
            && key.as_bytes().get(SEARCH_MOUNT.len()) == Some(&b'/')
    }

    /// moss preview manifest — the JSON file the moss editor uses to
    /// reconcile the build output with the source tree.
    pub fn for_previews_manifest() -> Self {
        ServedPath("_moss/previews.json".to_string())
    }

    /// Designer assets dropped into `.moss/theme/` (textures, fonts, video
    /// overlays referenced from the user's CSS / JS). `rel` is the path of the
    /// asset relative to `.moss/theme/`. Lands under `_moss/theme/<rel>`, a
    /// verbatim mirror of `.moss/theme/`, so relative URL references inside the
    /// user's CSS (`url("grain.png")`) and the entry `style.css`/`script.js`
    /// all sit as siblings under `/_moss/theme/`.
    pub fn for_theme_asset(rel: &str) -> Result<Self, ServedPathError> {
        if rel.is_empty() {
            return Err(ServedPathError::InvalidInput("theme asset path is empty"));
        }
        // Reuse the source-input validator: rejects `.moss/`, `_moss/`, `..`,
        // leading `/`, and empty. (`_moss/theme/` is fine as our PREFIX but
        // rejected as a user-supplied segment within `rel`.)
        Self::validate_source(rel)?;
        Ok(ServedPath(format!("{}/{}", THEME_MOUNT, rel)))
    }

    /// Root-relative URL of the theme mount, with a trailing slash so it works
    /// as a base for `new URL(rel, base)`. Single source of truth for the
    /// `window.mossTheme.base` global. Example: "/_moss/theme/".
    pub fn theme_mount_url() -> &'static str {
        "/_moss/theme/"
    }

    /// Borrow as `&str` for filesystem joins, manifest keys, log messages.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Move out into a `String`. Crate-internal: external callers must not
    /// be able to convert back and insert raw strings into untyped maps.
    /// Within `build::manifest` and `types::SiteHashes` accessors this is
    /// the legitimate way to produce a `String` key for storage.
    pub(crate) fn into_string(self) -> String {
        self.0
    }

    /// Disk location given the build output root.
    /// Example: ServedPath("assets/foo.png").to_disk("/tmp/site") -> /tmp/site/assets/foo.png
    pub fn to_disk(&self, output_root: &std::path::Path) -> std::path::PathBuf {
        output_root.join(&self.0)
    }

    /// Root-relative URL. Always begins with '/'.
    /// Example: ServedPath("assets/foo.png").to_relative_url() -> "/assets/foo.png"
    pub fn to_relative_url(&self) -> String {
        format!("/{}", self.0)
    }

    /// Absolute URL composed via SiteUrl::to_absolute.
    /// Example: ServedPath("assets/foo.png").to_absolute_url(&site) -> "https://example.com/assets/foo.png"
    ///
    /// Works for any SiteUrl, including the localhost defaults used by
    /// preview / non-deployed builds. Callers that emit absolute URLs
    /// into long-lived HTML (canonical, og:url, sitemap, RSS) should
    /// gate on `site_url.is_deployed()` first to avoid baking
    /// `http://localhost/...` into production output.
    pub fn to_absolute_url(&self, site_url: &crate::build::site_url::SiteUrl) -> String {
        site_url.to_absolute(&self.to_relative_url())
    }
}

impl fmt::Display for ServedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ServedPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::path::Path> for ServedPath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

// ---------------------------------------------------------------------------
// How ship treats a served path
// ---------------------------------------------------------------------------
//
// Here rather than in `ship` because `manifest` has to ask it (`register_held`
// refuses a path ship would rewrite) and `ship` depends on `manifest`. This
// module depends on neither. `ship` re-exports both names, and keeps
// `apply_transform`, the byte rewrite the enum selects.

/// How a file should be transformed when the ship pass writes it into a generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShipTransform {
    /// Copy the file byte-for-byte. Used for all non-HTML artifacts.
    CopyAsIs,
    /// Strip preview-only source-annotation attributes before writing to the generation.
    /// Used for `.html` / `.htm` files.
    StripPreviewAttrs,
}

/// Classify a relative path into the transform that the ship pass should apply.
///
/// It used to take an `annotations_present` flag, for a publish build that
/// emitted no `data-source-*` attributes and could skip the regex. No such
/// build exists: `emit_source_lines` (`pipeline.rs:1251`) is a literal `true`,
/// so every caller passed `true` and the other arm was a no-op waiting to be
/// wrong — it also skipped `ship`'s `STRIP_NO_PREVIEW_MARKER`, which is not
/// annotation-dependent at all.
///
/// Public so tests can verify classification without running a full ship.
pub fn transform_for(rel_path: &str) -> ShipTransform {
    if rel_path.ends_with(".html") || rel_path.ends_with(".htm") {
        ShipTransform::StripPreviewAttrs
    } else {
        ShipTransform::CopyAsIs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_for_html_returns_strip() {
        assert_eq!(
            transform_for("index.html"),
            ShipTransform::StripPreviewAttrs
        );
        assert_eq!(
            transform_for("articles/foo/index.html"),
            ShipTransform::StripPreviewAttrs
        );
        assert_eq!(
            transform_for("legacy.htm"),
            ShipTransform::StripPreviewAttrs
        );
    }

    #[test]
    fn transform_for_non_html_returns_copy() {
        assert_eq!(transform_for("style.css"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("og/home.png"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("rss.xml"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("data.json"), ShipTransform::CopyAsIs);
        assert_eq!(transform_for("video.mp4"), ShipTransform::CopyAsIs);
    }

    #[test]
    fn lowercases_directory_segments() {
        assert_eq!(
            ServedPath::from_source("Resources/orbit-model.html").unwrap().as_str(),
            "resources/orbit-model.html"
        );
    }

    #[test]
    fn lowercases_dirs_preserves_basename() {
        // Lowercase-only on dirs; basename verbatim. Spaces in dir names are
        // preserved (URLs with spaces are valid, browser encodes).
        assert_eq!(
            ServedPath::from_source("My Photos/Sunset.jpg").unwrap().as_str(),
            "my photos/Sunset.jpg"
        );
    }

    #[test]
    fn preserves_jupyterlite_asset_basenames_and_at_dirs() {
        // Real bug-trigger A: MathJax fonts have capital-cased filenames
        // hardcoded in JupyterLite's runtime JS. Lowercase-rewriting the
        // basename would 404. Preserved.
        assert_eq!(
            ServedPath::from_source("jupyter/static/lab/MathJax_Main-Bold.woff").unwrap().as_str(),
            "jupyter/static/lab/MathJax_Main-Bold.woff"
        );
        // Real bug-trigger B: JupyterLite ships extensions under npm-scope
        // dirs like `@jupyter-notebook/`. The earlier slug rule rewrote
        // `@` to `at` (via generate_slug), producing `atjupyter-notebook/`
        // in the manifest while disk had `@jupyter-notebook/` — manifest/
        // disk mismatch. Lowercase-only preserves `@`.
        assert_eq!(
            ServedPath::from_source("jupyter/extensions/@jupyterlite/p5-kernel/install.json").unwrap().as_str(),
            "jupyter/extensions/@jupyterlite/p5-kernel/install.json"
        );
    }

    #[test]
    fn idempotent() {
        let once = ServedPath::from_source("Foo Bar/Baz.html").unwrap();
        let twice = ServedPath::from_source(once.as_str()).unwrap();
        assert_eq!(once, twice);
        // Confirm dirs lowercased, basename verbatim.
        assert_eq!(once.as_str(), "foo bar/Baz.html");
    }

    #[test]
    fn preserves_cjk() {
        assert_eq!(
            ServedPath::from_source("文档/介绍.html").unwrap().as_str(),
            "文档/介绍.html"
        );
    }

    #[test]
    fn handles_already_normalized() {
        assert_eq!(
            ServedPath::from_source("resources/foo.html").unwrap().as_str(),
            "resources/foo.html"
        );
    }

    #[test]
    fn empty_input_now_errors() {
        assert!(matches!(ServedPath::from_source(""), Err(ServedPathError::Empty)));
    }

    #[test]
    fn leading_slash_now_errors() {
        assert!(matches!(
            ServedPath::from_source("/foo/bar.html"),
            Err(ServedPathError::AbsolutePath)
        ));
    }

    #[test]
    fn punctuated_filenames_preserved() {
        // Filename preservation: `photo (1).jpeg` stays exactly as the user
        // typed it. URLs with spaces/parens are valid (encoded by the
        // browser) and the user's link in markdown points at the same name.
        assert_eq!(
            ServedPath::from_source("Images/photo (1).jpeg").unwrap().as_str(),
            "images/photo (1).jpeg"
        );
    }

    #[test]
    fn for_jupyterlite_asset_passthrough() {
        let p = ServedPath::for_jupyterlite_asset(std::path::Path::new("Jupyter/STATIC/MathJax_Main-Bold.woff"));
        assert_eq!(p.as_str(), "Jupyter/STATIC/MathJax_Main-Bold.woff");
    }

    #[test]
    fn backslash_normalized_to_forward_slash() {
        // moss treats `\` as a path separator everywhere: a Windows-style source
        // path must normalize to the `/`-form (intermediate dir slugged, leaf
        // preserved), NOT round-trip the backslash into the output URL.
        let out = ServedPath::from_source("Foo\\Bar.html").unwrap();
        assert_eq!(out.as_str(), "foo/Bar.html");
        // ...and the `..` traversal guard must catch a backslash-hidden escape
        // (`..\secret` would otherwise be one segment and bypass validate_source).
        assert!(matches!(
            ServedPath::from_source("..\\secret"),
            Err(ServedPathError::ParentEscape)
        ));
    }

    #[test]
    fn to_disk_joins_output_root_with_inner_path() {
        let sp = ServedPath::from_source("assets/foo.png").unwrap();
        let root = std::path::Path::new("/tmp/build/site");
        assert_eq!(sp.to_disk(root), std::path::PathBuf::from("/tmp/build/site/assets/foo.png"));
    }

    #[test]
    fn to_relative_url_prepends_leading_slash() {
        let sp = ServedPath::from_source("assets/foo.png").unwrap();
        assert_eq!(sp.to_relative_url(), "/assets/foo.png");
    }

    #[test]
    fn to_absolute_url_uses_site_url_to_absolute() {
        let sp = ServedPath::from_source("assets/foo.png").unwrap();
        let site = crate::build::site_url::SiteUrl::parse("https://example.com").unwrap();
        assert_eq!(sp.to_absolute_url(&site), "https://example.com/assets/foo.png");
    }

    // --- validate_source tests ---

    #[test]
    fn validate_source_rejects_dotmoss_segment() {
        assert!(matches!(
            ServedPath::validate_source(".moss/og/abc.png"),
            Err(ServedPathError::ReservedMossPrefix)
        ));
        assert!(matches!(
            ServedPath::validate_source("assets/.moss/foo"),
            Err(ServedPathError::ReservedMossPrefix)
        ));
    }

    #[test]
    fn validate_source_rejects_underscore_moss_segment_in_user_input() {
        assert!(matches!(
            ServedPath::validate_source("_moss/og/abc.png"),
            Err(ServedPathError::ReservedMossPrefix)
        ));
        assert!(matches!(
            ServedPath::validate_source("assets/_moss/foo"),
            Err(ServedPathError::ReservedMossPrefix)
        ));
    }

    #[test]
    fn validate_source_accepts_filenames_containing_moss() {
        assert!(ServedPath::validate_source("notes/my-moss-archive.md").is_ok());
        assert!(ServedPath::validate_source("_moss-archive/file.txt").is_ok());
    }

    #[test]
    fn validate_source_rejects_parent_escape() {
        assert!(matches!(
            ServedPath::validate_source("foo/../bar"),
            Err(ServedPathError::ParentEscape)
        ));
    }

    #[test]
    fn validate_source_rejects_leading_slash() {
        assert!(matches!(
            ServedPath::validate_source("/foo/bar"),
            Err(ServedPathError::AbsolutePath)
        ));
    }

    #[test]
    fn validate_source_rejects_empty_and_whitespace() {
        assert!(matches!(ServedPath::validate_source(""), Err(ServedPathError::Empty)));
        assert!(matches!(ServedPath::validate_source("   "), Err(ServedPathError::Empty)));
    }

    #[test]
    fn cached_paths_use_relative_separator_and_segment_guards() {
        assert!(matches!(
            ServedPath::from_cached(".."),
            Err(ServedPathError::ParentEscape)
        ));
        assert!(matches!(
            ServedPath::from_cached(r"..\secret"),
            Err(ServedPathError::ParentEscape)
        ));
        assert!(matches!(
            ServedPath::from_cached("."),
            Err(ServedPathError::InvalidInput(_))
        ));
        assert!(matches!(
            ServedPath::from_cached("/_moss/link/cover.png"),
            Err(ServedPathError::AbsolutePath)
        ));
        assert_eq!(
            ServedPath::from_cached("_moss/link/cover.png").unwrap().as_str(),
            "_moss/link/cover.png"
        );
    }

    // --- for_* constructor tests ---

    #[test]
    fn for_og_card_lives_under_underscore_moss_og() {
        let sp = ServedPath::for_og_card("87ba30f2b3c09ca9").unwrap();
        assert_eq!(sp.as_str(), "_moss/og/87ba30f2b3c09ca9.png");
        assert_eq!(sp.to_relative_url(), "/_moss/og/87ba30f2b3c09ca9.png");
    }

    #[test]
    fn for_remote_cover_lives_under_underscore_moss_link() {
        let sp = ServedPath::for_remote_cover("87ba30f2b3c09ca9", "jpg").unwrap();
        assert_eq!(sp.as_str(), "_moss/link/87ba30f2b3c09ca9.jpg");
        assert_eq!(sp.to_relative_url(), "/_moss/link/87ba30f2b3c09ca9.jpg");
    }

    #[test]
    fn for_remote_cover_rejects_non_hex_hash() {
        assert!(matches!(
            ServedPath::for_remote_cover("not-hex!", "png"),
            Err(ServedPathError::InvalidInput(_))
        ));
    }

    #[test]
    fn for_remote_cover_rejects_a_disallowed_extension() {
        assert!(matches!(
            ServedPath::for_remote_cover("87ba30f2b3c09ca9", "svg"),
            Err(ServedPathError::InvalidInput(_))
        ));
        assert!(matches!(
            ServedPath::for_remote_cover("87ba30f2b3c09ca9", "gif"),
            Err(ServedPathError::InvalidInput(_))
        ));
    }

    #[test]
    fn for_og_card_rejects_non_hex_hash() {
        assert!(matches!(
            ServedPath::for_og_card("not-hex!"),
            Err(ServedPathError::InvalidInput(_))
        ));
        assert!(matches!(
            ServedPath::for_og_card(""),
            Err(ServedPathError::InvalidInput(_))
        ));
    }

    #[test]
    fn for_og_card_rejects_wrong_length_hash() {
        // 15 chars - too short
        assert!(matches!(
            ServedPath::for_og_card("87ba30f2b3c09ca"),
            Err(ServedPathError::InvalidInput(_))
        ));
        // 32 chars - too long (full SHA-256 hex, not the truncated form)
        assert!(matches!(
            ServedPath::for_og_card("87ba30f2b3c09ca987ba30f2b3c09ca9"),
            Err(ServedPathError::InvalidInput(_))
        ));
    }

    #[test]
    fn for_math_png_lives_under_underscore_moss_math() {
        let sp = ServedPath::for_math_png("87ba30f2b3c09ca9").unwrap();
        assert_eq!(sp.as_str(), "_moss/math/87ba30f2b3c09ca9.png");
        assert_eq!(sp.to_relative_url(), "/_moss/math/87ba30f2b3c09ca9.png");
    }

    #[test]
    fn for_math_png_rejects_bad_hashes() {
        assert!(ServedPath::for_math_png("").is_err());
        assert!(ServedPath::for_math_png("not-hex!").is_err());
        assert!(ServedPath::for_math_png("87ba30f2b3c09ca").is_err()); // 15
        assert!(ServedPath::for_math_png("87BA30F2B3C09CA9").is_err()); // uppercase
        assert!(
            ServedPath::for_math_png("87ba30f2b3c09ca987ba30f2b3c09ca9").is_err() // 32
        );
    }

    #[test]
    fn for_favicon_with_supported_ext() {
        assert_eq!(ServedPath::for_favicon("png").unwrap().as_str(), "assets/favicon.png");
        assert_eq!(ServedPath::for_favicon("svg").unwrap().as_str(), "assets/favicon.svg");
    }

    #[test]
    fn for_favicon_rejects_dotted_or_slashed_ext() {
        assert!(ServedPath::for_favicon(".png").is_err());
        assert!(ServedPath::for_favicon("png/").is_err());
        assert!(ServedPath::for_favicon("").is_err());
    }

    #[test]
    fn for_apple_touch_icon_fixed_path() {
        assert_eq!(ServedPath::for_apple_touch_icon().as_str(), "assets/apple-touch-icon.png");
    }

    #[test]
    fn for_sitemap_fixed_path() {
        assert_eq!(ServedPath::for_sitemap().as_str(), "sitemap.xml");
    }

    #[test]
    fn for_rss_with_slug() {
        assert_eq!(ServedPath::for_rss("blog").unwrap().as_str(), "blog/rss.xml");
    }

    #[test]
    fn for_rss_empty_slug_lands_at_root() {
        assert_eq!(ServedPath::for_rss("").unwrap().as_str(), "rss.xml");
    }

    #[test]
    fn for_rss_rejects_slashed_slug() {
        assert!(ServedPath::for_rss("a/b").is_err());
    }

    #[test]
    fn for_llms_txt_fixed_path() {
        assert_eq!(ServedPath::for_llms_txt().as_str(), "llms.txt");
    }

    #[test]
    fn for_robots_txt_fixed_path() {
        assert_eq!(ServedPath::for_robots_txt().as_str(), "robots.txt");
    }

    #[test]
    fn for_moss_default_icon_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_moss_default_icon().as_str(), "_moss/default-icon.png");
    }

    #[test]
    fn for_default_stylesheet_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_default_stylesheet().as_str(), "_moss/style.css");
    }

    #[test]
    fn for_custom_stylesheet_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_custom_stylesheet().as_str(), "_moss/theme/style.css");
    }

    #[test]
    fn for_custom_script_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_custom_script().as_str(), "_moss/theme/script.js");
    }

    #[test]
    fn for_runtime_js_lives_under_underscore_moss_js() {
        assert_eq!(ServedPath::for_runtime_js("theme").unwrap().as_str(), "_moss/js/theme.js");
        assert_eq!(ServedPath::for_runtime_js("iframe-bridge").unwrap().as_str(), "_moss/js/iframe-bridge.js");
    }

    #[test]
    fn for_runtime_js_rejects_unsafe_names() {
        assert!(ServedPath::for_runtime_js("").is_err());
        assert!(ServedPath::for_runtime_js("foo/bar").is_err());
        assert!(ServedPath::for_runtime_js("theme.js").is_err()); // has dot
    }

    #[test]
    fn for_search_asset_lives_under_underscore_moss_pagefind() {
        assert_eq!(
            ServedPath::for_search_asset("pagefind.js")
                .unwrap()
                .as_str(),
            "_moss/pagefind/pagefind.js"
        );
        assert_eq!(
            ServedPath::for_search_asset("fragment/en_1a2b3c.pf_fragment")
                .unwrap()
                .as_str(),
            "_moss/pagefind/fragment/en_1a2b3c.pf_fragment"
        );
    }

    #[test]
    fn for_search_asset_is_passthrough_on_case() {
        // Pagefind bakes its chunk filenames into pagefind-entry.json and
        // resolves them relative to the bundle dir — slug-lowercasing a
        // segment here would 404 at query time.
        assert_eq!(
            ServedPath::for_search_asset("Fragment/EN_ABC.pf_fragment")
                .unwrap()
                .as_str(),
            "_moss/pagefind/Fragment/EN_ABC.pf_fragment"
        );
    }

    #[test]
    fn for_search_asset_rejects_path_escapes() {
        assert!(ServedPath::for_search_asset("").is_err());
        assert!(ServedPath::for_search_asset("   ").is_err());
        assert!(ServedPath::for_search_asset("/absolute.js").is_err());
        assert!(ServedPath::for_search_asset("../escape.js").is_err());
        assert!(ServedPath::for_search_asset("..\\escape.js").is_err());
    }

    /// Binding test, mirroring `theme_mount_url_matches_theme_mount_constant`.
    #[test]
    fn search_mount_url_matches_search_mount_constant() {
        assert_eq!(
            ServedPath::search_mount_url(),
            format!("/{}/", SEARCH_MOUNT),
            "search_mount_url() must be /{SEARCH_MOUNT}/ — update the static str if SEARCH_MOUNT changes"
        );
    }

    #[test]
    fn for_previews_manifest_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_previews_manifest().as_str(), "_moss/previews.json");
    }

    #[test]
    fn for_theme_asset_lives_under_underscore_moss() {
        assert_eq!(ServedPath::for_theme_asset("background.png").unwrap().as_str(), "_moss/theme/background.png");
        assert_eq!(ServedPath::for_theme_asset("fonts/MyFont.woff").unwrap().as_str(), "_moss/theme/fonts/MyFont.woff");
    }

    #[test]
    fn for_theme_asset_rejects_path_escapes() {
        assert!(ServedPath::for_theme_asset("").is_err());
        assert!(ServedPath::for_theme_asset("../escape.png").is_err());
        assert!(ServedPath::for_theme_asset("/absolute.png").is_err());
    }

    /// Binding test: `theme_mount_url()` must always equal `"/" + THEME_MOUNT + "/"`.
    /// If THEME_MOUNT ever diverges from the hardcoded static str in `theme_mount_url`,
    /// this test will catch the drift at compile/test time.
    #[test]
    fn theme_mount_url_matches_theme_mount_constant() {
        assert_eq!(
            ServedPath::theme_mount_url(),
            format!("/{}/", THEME_MOUNT),
            "theme_mount_url() must be /{THEME_MOUNT}/ — update the static str if THEME_MOUNT changes"
        );
    }

    #[test]
    fn for_runtime_js_hashed_embeds_hash_in_stem() {
        let sp = ServedPath::for_runtime_js_hashed("theme", "abcd1234ef567890").unwrap();
        assert_eq!(sp.as_str(), "_moss/js/theme.abcd1234ef567890.js");
    }

    #[test]
    fn for_runtime_js_hashed_rejects_unsafe_names() {
        assert!(ServedPath::for_runtime_js_hashed("", "abc").is_err());
        assert!(ServedPath::for_runtime_js_hashed("foo/bar", "abc").is_err());
        assert!(ServedPath::for_runtime_js_hashed("theme.js", "abc").is_err());
        // Hyphens are valid (thumb-swap, heading-anchor use them).
        assert!(ServedPath::for_runtime_js_hashed("thumb-swap", "abc").is_ok());
        assert!(ServedPath::for_runtime_js_hashed("heading-anchor", "abc").is_ok());
    }

    #[test]
    fn for_default_stylesheet_hashed_embeds_hash() {
        let sp = ServedPath::for_default_stylesheet_hashed("abcd1234ef567890");
        assert_eq!(sp.as_str(), "_moss/style.abcd1234ef567890.css");
    }

    #[test]
    fn for_custom_stylesheet_hashed_embeds_hash() {
        let sp = ServedPath::for_custom_stylesheet_hashed("abcd1234ef567890");
        assert_eq!(sp.as_str(), "_moss/theme/style.abcd1234ef567890.css");
    }

    #[test]
    fn for_custom_script_hashed_embeds_hash() {
        let sp = ServedPath::for_custom_script_hashed("abcd1234ef567890");
        assert_eq!(sp.as_str(), "_moss/theme/script.abcd1234ef567890.js");
    }

    #[test]
    fn hashed_constructor_to_relative_url_is_root_relative() {
        let sp = ServedPath::for_runtime_js_hashed("theme", "abc123").unwrap();
        assert_eq!(sp.to_relative_url(), "/_moss/js/theme.abc123.js");
    }
}
