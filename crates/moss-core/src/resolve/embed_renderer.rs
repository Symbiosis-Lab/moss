//! Shared vocabulary for `![[file]]` embed dispatch.
//!
//! The actual dispatch logic lives in [`super::wikilink_dispatch`] (extension
//! routing: a pre-pass claims markdown/notebook/table, `synth_kind_for_ext`
//! claims video/pdf/audio/iframe/3D, and an image-extension check claims the
//! rest — anything left over falls back to a plain file link, Obsidian
//! parity). This module holds what that dispatch and its moss-build
//! resolvers share: [`ParsedEmbed`] (the resolved-embed input every
//! synthesizer takes), the reserved HTML/CSS class names, the deferred-marker
//! prefixes moss-build's post-pass resolvers match on, and small formatting
//! helpers ([`file_stem`], [`html_escape_attr`], [`Sizing`]/[`Dim`]).
//!
//! moss-core is pure: no filesystem, no network, no async. A renderer that
//! needs file content (markdown transclusion, notebook, CSV/TSV) can't read
//! it here — [`crate::resolve`]'s pre-pass emits one of the `MARKER_*`
//! comment prefixes below instead, and moss-build's post-pass
//! (`resolve_embeds` / `resolve_deferred_markers` in `embeds.rs`) reads the
//! target file and splices the resolved content into the marker.

mod common;
pub mod folder_list;

// Re-export the canonical 4-char attribute escaper so moss-core's own synthesizers
// (pdf / iframe / model / audio / video) can share one definition instead of
// inlining private copies that drifted apart (moss-core's was 4 chars; some
// synthesizers via `moss_core::media::html_escape` was 5 chars including
// `'` → `&#39;`). The 4-char form is correct per HTML5: apostrophe is safe
// inside `"…"` attributes.
pub use common::{file_stem, html_escape_attr};

use crate::media::Placement;

// ---------------------------------------------------------------------------
// Reserved classnames (HTML/CSS contract)
// ---------------------------------------------------------------------------

/// Base class applied to all typed-embed output elements.
///
/// Theme authors may target `.moss-embed` to style the wrapper of any embed;
/// renderer-specific classes (e.g. [`CLASS_EMBED_IFRAME`]) extend the base.
/// The CSS that ships with moss is defined in moss-build.
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
// Deferred-marker prefixes (contract with moss-build's resolvers)
// ---------------------------------------------------------------------------

/// Marker prefix for a markdown transclusion embed (`![[file.md]]`).
///
/// Format: `<!-- moss-embed:PATH[#anchor] -->`. Emitted by `resolve.rs`'s
/// pre-pass (`lower_transclusion_and_folder_wikilinks`) and resolved by
/// [`super::embeds::resolve_embeds`] (inlines target markdown content).
///
/// No `-<type>` suffix for historical reasons: this was the original embed
/// marker before typed embeds existed. New typed markers use
/// `moss-embed-<type>:` (see [`MARKER_IPYNB`], [`MARKER_TABLE`]).
pub const MARKER_MARKDOWN: &str = "moss-embed";

/// Marker prefix for a Jupyter notebook embed (`![[file.ipynb]]`).
///
/// Format: `<!-- moss-embed-ipynb:PATH[?query] -->`. Emitted by `resolve.rs`'s
/// pre-pass and resolved by moss-build's notebook handler (an inline
/// JupyterLite viewer `<iframe>`, not a static nbconvert render).
pub const MARKER_IPYNB: &str = "moss-embed-ipynb";

/// Marker prefix for a tabular-data embed (`![[file.csv]]`/`![[file.tsv]]`).
///
/// Format: `<!-- moss-embed-table:PATH -->`. Emitted by `resolve.rs`'s
/// pre-pass; moss-build reads the file and calls [`crate::csv_table::render`]
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
    /// a URL per referencing page is a recurring regression.
    pub pinned_url: &'a str,
    /// `?query` from the source wikilink, without the leading `?`.
    pub query: Option<&'a str>,
    /// `#fragment` from the source wikilink, without the leading `#`.
    /// For `.md` renderers this is a heading/block-ref marker (block refs
    /// keep their `^` prefix). For every other renderer this is a URL fragment.
    pub section: Option<&'a str>,
    /// `|pipe-content` from the source wikilink — with the placement
    /// vocabulary already split out into [`Self::placement`]. Image renderer
    /// uses this for display keywords / size; other renderers parse per
    /// their convention.
    pub alias: Option<&'a str>,
    /// Width, float side and float size, read off the pipe by
    /// [`crate::media::extract_placement_from_alias`]. An empty
    /// [`Placement`] means the author wrote none of the three; renderers
    /// then omit `data-width` so themes can target the default via
    /// `:not([data-width])`.
    ///
    /// `full` is normalised to `screen` upstream — values reaching here
    /// are already in value-space terms (see
    /// [`crate::media::match_width_token`]).
    pub placement: Placement,
    /// Trailing Pandoc `{.class key=value}` attribute block, if present.
    ///
    /// Per the unified-image-emission architecture, Pandoc-
    /// style attribute blocks are the canonical author surface for moss-
    /// vocabulary attributes; the pipe-keyword form remains as compat sugar.
    /// When both are present, the attribute block wins on typed-field
    /// conflicts; class lists union+dedupe.
    pub attrs: Option<crate::ast::attrs::AttrBlock>,
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

// ---------------------------------------------------------------------------
// Image extensions
// ---------------------------------------------------------------------------

/// Image file extensions the wikilink dispatcher routes to the figure arm.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];

#[cfg(test)]
#[path = "embed_renderer_tests.rs"]
mod tests;
