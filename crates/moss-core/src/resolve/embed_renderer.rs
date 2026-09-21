//! Renderer registry for `![[file]]` embeds.
//!
//! Each renderer maps a file extension (or extension family) to an output
//! format. The caller resolves the embed target via the ContentGraph, then
//! dispatches to the renderer for the target's extension. Unknown extensions
//! fall back to a file link (Obsidian parity) — that fallback lives in the
//! caller, not here.
//!
//! # moss-core ↔ src-tauri boundary
//!
//! moss-core is pure: no filesystem, no network, no async. This constrains
//! what a renderer can do:
//!
//! - **Pure renderers** (image, iframe, audio, video, 3D, table) — return
//!   `RenderedEmbed::Inline(markdown)` or `RenderedEmbed::Html(html)`. No I/O.
//!   The string is spliced directly into the compiled output.
//! - **I/O-bound renderers** (markdown transclusion, notebook, PDF preview) —
//!   return `RenderedEmbed::Deferred { marker }`. src-tauri runs a post-pass
//!   (`resolve_embeds` in `embeds.rs`) that reads the target file and splices
//!   its rendered content into the marker.
//!
//! Plugin-registered renderers (Phase E) must follow the same rule: if they
//! need I/O, they emit a marker and register a corresponding resolver on the
//! src-tauri side.

mod common;
pub mod folder_list;

// Re-export the canonical 4-char attribute escaper so src-tauri synthesizers
// (pdf / iframe / model / audio / video) can share one definition instead of
// inlining private copies that drifted apart (moss-core's was 4 chars; some
// synthesizers via `moss_core::media::html_escape` was 5 chars including
// `'` → `&#39;`). The 4-char form is correct per HTML5: apostrophe is safe
// inside `"…"` attributes.
pub use common::{file_stem, html_escape_attr};

// ---------------------------------------------------------------------------
// Reserved classnames (HTML/CSS contract, per moss#508)
// ---------------------------------------------------------------------------

/// Base class applied to all typed-embed output elements.
///
/// Theme authors may target `.moss-embed` to style the wrapper of any embed;
/// renderer-specific classes (e.g. [`CLASS_EMBED_IFRAME`]) extend the base.
/// The CSS that ships with moss is defined in src-tauri (see issue #508 for
/// the HTML/CSS contract).
pub const CLASS_EMBED: &str = "moss-embed";

/// Applied to iframe renderer output (Phase B).
pub const CLASS_EMBED_IFRAME: &str = "moss-embed-iframe";

/// Applied to PDF renderer output (Phase C).
pub const CLASS_EMBED_PDF: &str = "moss-embed-pdf";

/// Applied to audio renderer output (Phase C).
pub const CLASS_EMBED_AUDIO: &str = "moss-embed-audio";

/// Applied to video renderer output (Phase C).
pub const CLASS_EMBED_VIDEO: &str = "moss-embed-video";

/// Applied to notebook renderer output (Phase D).
pub const CLASS_EMBED_NOTEBOOK: &str = "moss-embed-notebook";

/// Applied to 3D model renderer output (Phase D).
pub const CLASS_EMBED_3D: &str = "moss-embed-3d";

/// Applied to tabular-data renderer output (Phase D).
pub const CLASS_EMBED_TABLE: &str = "moss-embed-table";

// ---------------------------------------------------------------------------
// Deferred-marker prefixes (contract with src-tauri resolvers)
// ---------------------------------------------------------------------------

/// Marker prefix for a markdown transclusion embed (`![[file.md]]`).
///
/// Format: `<!-- moss-embed:PATH[#anchor] -->`. Emitted by `resolve.rs`'s
/// pre-pass (`lower_transclusion_and_folder_wikilinks`) and resolved by
/// src-tauri's `resolve_embeds` (inlines target markdown content).
///
/// No `-<type>` suffix for historical reasons: this was the original embed
/// marker before typed embeds existed. New typed markers use
/// `moss-embed-<type>:` (see [`MARKER_IPYNB`], [`MARKER_TABLE`]).
pub const MARKER_MARKDOWN: &str = "moss-embed";

/// Marker prefix for a Jupyter notebook embed (`![[file.ipynb]]`).
///
/// Format: `<!-- moss-embed-ipynb:PATH[?query] -->`. Emitted by `resolve.rs`'s
/// pre-pass and resolved by src-tauri via nbconvert.
pub const MARKER_IPYNB: &str = "moss-embed-ipynb";

/// Marker prefix for a tabular-data embed (`![[file.csv]]`/`![[file.tsv]]`).
///
/// Format: `<!-- moss-embed-table:PATH -->`. Emitted by `resolve.rs`'s
/// pre-pass; src-tauri reads the file and calls [`crate::csv_table::render`]
/// (a pure renderer).
pub const MARKER_TABLE: &str = "moss-embed-table";

// Re-export folder_list marker constants for convenience.
pub use folder_list::{MARKER_END, MARKER_FOLDER_LIST};

/// An embed that has been parsed and path-resolved, ready for rendering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedEmbed<'a> {
    /// Resolved target path, as returned by the ContentGraph.
    pub resolved_path: &'a str,
    /// The calling file's path — identifies the referencing note, and is NOT an
    /// input to the emitted URL (see [`Self::pinned_url`]).
    pub from_path: &'a str,
    /// The URL the site serves [`Self::resolved_path`] at, pinned once by the
    /// dispatcher via `ContentGraph::pinned_url`. Emit it verbatim; re-deriving
    /// a URL per referencing page is moss#903 bug 3.
    pub pinned_url: &'a str,
    /// `?query` from the source wikilink, without the leading `?`.
    pub query: Option<&'a str>,
    /// `#fragment` from the source wikilink, without the leading `#`.
    /// For `.md` renderers this is a heading/block-ref marker (block refs
    /// keep their `^` prefix). For every other renderer this is a URL fragment.
    pub section: Option<&'a str>,
    /// `|pipe-content` from the source wikilink — with any spec § P9 width
    /// token already split out into [`Self::width`]. Image renderer uses
    /// this for display keywords / size; other renderers parse per their
    /// convention.
    pub alias: Option<&'a str>,
    /// Canonical width value (`body | wide | page | screen`) extracted from
    /// the pipe-alias by the wikilink resolver. `None` means the author
    /// did not include a width token; renderers omit `data-width` in that
    /// case so themes can target the default via `:not([data-width])`.
    ///
    /// `full` is normalised to `screen` upstream — values reaching here
    /// are already in value-space terms (see
    /// [`crate::media::match_width_token`]).
    pub width: Option<&'static str>,
    /// Trailing Pandoc `{.class key=value}` attribute block, if present.
    ///
    /// Per Decision #8 of the unified-image-emission architecture, Pandoc-
    /// style attribute blocks are the canonical author surface for moss-
    /// vocabulary attributes; the pipe-keyword form remains as compat sugar.
    /// When both are present, the attribute block wins on typed-field
    /// conflicts (Decision #11); class lists union+dedupe.
    pub attrs: Option<crate::ast::attrs::AttrBlock>,
}

/// Output of a renderer.
///
/// The variant tells the caller what further processing (if any) the string
/// needs. See the module-level doc for the moss-core ↔ src-tauri boundary rule.
#[derive(Debug, PartialEq, Eq)]
pub enum RenderedEmbed {
    /// Markdown-level text that will be processed by CommonMark downstream.
    /// Example: `![alt](url)` from the image renderer.
    Inline(String),
    /// Final HTML to splice into the output — must NOT be re-processed by the
    /// markdown parser. Example: `<iframe …>` from the iframe renderer.
    Html(String),
    /// A marker comment for a post-pass resolver to expand with file I/O.
    ///
    /// Format convention: `<!-- <prefix>:<target> -->` where `<prefix>`
    /// uniquely identifies the resolver (e.g. `moss-embed-ipynb`,
    /// `moss-embed-table`, `moss-embed-plugin-<plugin-name>`) and
    /// `<target>` is the body the resolver parses (commonly a path,
    /// optionally with `?query#fragment|alias`).
    ///
    /// The resolver lives in src-tauri (where async and I/O are allowed).
    /// Built-in prefixes are exported as pub const: [`MARKER_MARKDOWN`],
    /// [`MARKER_IPYNB`], [`MARKER_TABLE`]. Plugin-registered renderers
    /// emit `moss-embed-plugin-<plugin-name>:` — see
    /// [`super::registry`] for the full two-pass dispatch design.
    Deferred { marker: String },
}

/// A single dimension with a unit.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Dim {
    Px(u32),
    Percent(f32),
    Vh(f32),
}

impl Dim {
    /// Render this dimension as a CSS length string.
    pub fn to_css(self) -> String {
        match self {
            Dim::Px(n) => format!("{}px", n),
            Dim::Percent(v) => {
                if v.fract() == 0.0 {
                    format!("{}%", v as i64)
                } else {
                    format!("{}%", v)
                }
            }
            Dim::Vh(v) => {
                if v.fract() == 0.0 {
                    format!("{}vh", v as i64)
                } else {
                    format!("{}vh", v)
                }
            }
        }
    }

    /// Parse one dimension. Accepts: `200`, `200px`, `100%`, `80vh`.
    /// Returns None on any parse failure.
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some(rest) = s.strip_suffix('%') {
            return rest.trim().parse::<f32>().ok().map(Dim::Percent);
        }
        if let Some(rest) = s.strip_suffix("vh") {
            return rest.trim().parse::<f32>().ok().map(Dim::Vh);
        }
        if let Some(rest) = s.strip_suffix("px") {
            return rest.trim().parse::<u32>().ok().map(Dim::Px);
        }
        s.parse::<u32>().ok().map(Dim::Px)
    }
}

/// Parsed `|WxH` sizing hint from a wikilink pipe segment.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Sizing {
    /// `|200` or `|100%` — width only.
    Width(Dim),
    /// `|200x150` or `|100%x600` — width × height.
    Box(Dim, Dim),
}

impl Sizing {
    /// Parse a pipe segment. Returns None if the string does not look like a
    /// sizing hint — callers can then fall through to their own parser
    /// (e.g. image display keywords).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some((w, h)) = s.split_once('x') {
            let wd = Dim::parse(w)?;
            let hd = Dim::parse(h)?;
            return Some(Sizing::Box(wd, hd));
        }
        Dim::parse(s).map(Sizing::Width)
    }
}

/// A renderer converts a `ParsedEmbed` into its rendered form.
pub trait EmbedRenderer: std::fmt::Debug + Send + Sync {
    /// Extensions this renderer claims (lowercase, without leading dot).
    fn extensions(&self) -> &[&'static str];

    /// Render the embed. Must be pure; moss-core is I/O-free.
    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed;
}

/// Always empty: every extension that once had a built-in `EmbedRenderer`
/// here is now claimed earlier in the live pipeline (`resolve.rs`'s
/// pre-pass, or `synth_kind_for_ext`).
fn registry() -> &'static [&'static dyn EmbedRenderer] {
    &[]
}

/// Look up a renderer by file extension (case-insensitive, no leading dot).
pub fn lookup_renderer(ext: &str) -> Option<&'static dyn EmbedRenderer> {
    if ext.is_empty() {
        return None;
    }
    registry()
        .iter()
        .copied()
        .find(|r| r.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

// ---------------------------------------------------------------------------
// ImageRenderer
// ---------------------------------------------------------------------------

/// Image file extensions recognized by `ImageRenderer`.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];

#[cfg(test)]
#[path = "embed_renderer_tests.rs"]
mod tests;
