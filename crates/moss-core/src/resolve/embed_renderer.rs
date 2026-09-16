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

use std::sync::OnceLock;

mod common;
pub mod folder_list;
use common::path_extension_lower;

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

/// Marker prefix emitted by [`MarkdownEmbedRenderer`].
///
/// Format: `<!-- moss-embed:PATH[#anchor] -->`. Resolved by src-tauri's
/// `resolve_embeds` (inlines target markdown content).
///
/// No `-<type>` suffix for historical reasons: this was the original embed
/// marker before typed embeds existed. New typed markers use
/// `moss-embed-<type>:` (see [`MARKER_IPYNB`], [`MARKER_TABLE`]).
pub const MARKER_MARKDOWN: &str = "moss-embed";

/// Marker prefix emitted by [`NotebookRenderer`].
///
/// Format: `<!-- moss-embed-ipynb:PATH[?query] -->`. Resolved by src-tauri
/// via nbconvert.
pub const MARKER_IPYNB: &str = "moss-embed-ipynb";

/// Marker prefix emitted by [`TableRenderer`].
///
/// Format: `<!-- moss-embed-table:PATH -->`. src-tauri reads the file and
/// calls [`crate::csv_table::render`] (a pure renderer).
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

    /// Page-level HTML fragments this renderer needs in `<head>`, injected
    /// once per page that contains at least one embed from this renderer.
    ///
    /// Example: `ModelViewerRenderer` returns a `<script type="module">` tag
    /// so that `<model-viewer>` custom elements work. The build pipeline
    /// collects and deduplicates these across all embeds on a page.
    ///
    /// Default: empty. Renderers with no page-level assets don't override.
    fn head_assets(&self) -> &[&'static str] {
        &[]
    }
}

/// Built-in renderer registry. Initialized lazily on first lookup.
///
/// Each renderer is a unit struct, so the pointer is to a zero-size `'static`
/// — no heap allocation ever. Future renderers (notebook, 3d, table, plugins)
/// get appended here as they ship.
///
/// Extension sets across renderers are currently disjoint. Adding overlap
/// (e.g., if a future renderer claims `.ogg` for video) would require
/// tie-break logic here; first-match-wins is the only implicit rule today.
fn registry() -> &'static [&'static dyn EmbedRenderer] {
    static INIT: OnceLock<Vec<&'static dyn EmbedRenderer>> = OnceLock::new();
    INIT.get_or_init(|| {
        vec![
            &MarkdownEmbedRenderer as &'static dyn EmbedRenderer,
            &IframeRenderer as &'static dyn EmbedRenderer,
            &PdfRenderer as &'static dyn EmbedRenderer,
            &AudioRenderer as &'static dyn EmbedRenderer,
            &VideoRenderer as &'static dyn EmbedRenderer,
            &NotebookRenderer as &'static dyn EmbedRenderer,
            &ModelViewerRenderer as &'static dyn EmbedRenderer,
            &TableRenderer as &'static dyn EmbedRenderer,
        ]
    })
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

use crate::heading::anchor::obsidian_heading_anchor;

use super::title_params::TitleParams;

/// Image file extensions recognized by `ImageRenderer`.
pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];

/// Map an `AlignSide` to its canonical title-param keyword (`"left"` or
/// `"right"`). Stage 2 reverses this via `AlignSide::from_keyword`-style
/// recognition (it accepts both `left` and `align-left`).
fn align_keyword(side: crate::media::AlignSide) -> &'static str {
    match side {
        crate::media::AlignSide::Left => "left",
        crate::media::AlignSide::Right => "right",
    }
}

/// Push `class` into `acc` only if not already present. Used when merging
/// pipe-alias passthrough classes with Pandoc attribute-block classes
/// (Decision #11: class lists union and dedupe).
fn add_class_dedup(acc: &mut Vec<String>, class: &str) {
    if !acc.iter().any(|c| c == class) {
        acc.push(class.to_string());
    }
}

/// Shared attribute-block fold for non-image renderers (iframe, pdf, audio,
/// video, 3D). Mirrors the ImageRenderer logic at a smaller scope: extract
/// recognized vocabulary (align) into typed params; pass through everything
/// else as `classes` + extra key=value attrs.
///
/// Lives here so all `render_link_markdown` consumers stay in lockstep when
/// Decision #11 (attribute-block-wins, class lists union+dedupe) evolves.
///
/// `pub(super)` for sibling-module use within `resolve/`. Originally exposed
/// so the Stage 1 native-markdown sweep (`wikilinks::stage1_sweep`) could
/// fold trailing `{...}` attribute blocks into native-image rewrites;
/// that sweep retired in Phase 3 PR2, but the visibility stays the same
/// shape so plugin-side callers can still reach it.
pub(super) fn fold_attrs_into_params(
    block: &crate::ast::attrs::AttrBlock,
    params: &mut TitleParams,
) {
    let mut classes: Vec<String> = Vec::new();
    let mut consumed_class_kv = false;

    for class in &block.classes {
        if let Some(side) = crate::media::AlignSide::from_keyword(class) {
            params.insert("align", align_keyword(side));
        } else {
            add_class_dedup(&mut classes, class);
        }
    }

    if let Some(w) = block.width {
        // Non-image renderers expose width on the wrapper as `data-width`
        // (see `render_link_markdown`), matching the pipe-alias path.
        params.insert("data-width", w);
    }

    for (k, v) in &block.kvs {
        if k == "class" {
            consumed_class_kv = true;
            for c in v.split_whitespace() {
                if let Some(side) = crate::media::AlignSide::from_keyword(c) {
                    params.insert("align", align_keyword(side));
                } else {
                    add_class_dedup(&mut classes, c);
                }
            }
        }
    }
    if !classes.is_empty() {
        params.insert("classes", classes.join(" "));
    }
    for (k, v) in &block.kvs {
        if consumed_class_kv && k == "class" {
            continue;
        }
        params.insert(k.clone(), v.clone());
    }
}

/// Escape alt text for markdown `![...](url)` syntax.
///
/// Brackets MUST be escaped; HTML entities are NOT needed (pulldown-cmark
/// handles `<` `>` `&` per CommonMark rules when alt text is rendered).
fn markdown_escape_alt(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

/// Shared Stage 1 emitter for the five non-image renderers (iframe, pdf,
/// audio, video, 3D). Produces a CommonMark link:
///
/// ```text
/// [filename](url "moss:kind=<kind> <params>")
/// ```
///
/// pulldown-cmark parses this as `Tag::Link`; Phase 1's Stage 2 dispatcher
/// keys off `moss:kind=` to choose the right HTML synthesizer.
///
/// `kind` is the canonical kind name (`iframe`, `pdf`, `audio`, `video`,
/// `3d`). `extra` is invoked to inject renderer-specific params; the helper
/// pre-fills `kind=` so callers only handle their own grammar.
fn render_link_markdown(
    embed: &ParsedEmbed<'_>,
    kind: &'static str,
    extra: impl FnOnce(&ParsedEmbed<'_>, &mut TitleParams),
) -> String {
    let url = embed.pinned_url;
    // The `moss:kind=…` markdown-title channel is retired: params (kind /
    // data-width / extras / attribute block) are no longer round-tripped
    // through a title. The accumulation stays as a no-op so the `extra` and
    // `fold_attrs_into_params` plumbing keeps being exercised. Anything
    // threading typed params into iframe / pdf / video / audio / 3D dispatch
    // must bypass this round-trip — `EmitKind::Inline` is the wrong channel
    // for typed structural data.
    let mut params = TitleParams::default();
    params.insert("kind", kind);
    if let Some(w) = embed.width {
        params.insert("data-width", w);
    }
    extra(embed, &mut params);
    if let Some(block) = &embed.attrs {
        fold_attrs_into_params(block, &mut params);
    }
    let _ = params;
    let name = file_stem(embed.resolved_path);
    format!("[{}]({})", markdown_escape_alt(&name), url)
}

// file_stem now lives in common.rs — imported via common::file_stem below.

// ---------------------------------------------------------------------------
// MarkdownEmbedRenderer
// ---------------------------------------------------------------------------

/// Renderer for markdown transclusion: `![[file.md]]` → `<!-- moss-embed:path -->`.
///
/// The marker comment is resolved later by src-tauri's embed resolver, which
/// reads the target file's content and splices it inline. This renderer does
/// not perform I/O.
#[derive(Debug)]
pub struct MarkdownEmbedRenderer;

impl EmbedRenderer for MarkdownEmbedRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["md"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        let anchor = build_embed_anchor(embed.section);
        RenderedEmbed::Deferred {
            marker: format!(
                "<!-- {}:{}{} -->",
                MARKER_MARKDOWN, embed.resolved_path, anchor
            ),
        }
    }
}

/// Build the anchor fragment for a markdown embed marker.
///
/// Preserves the `^` prefix on block references so the downstream embed
/// resolver can distinguish them from headings.
fn build_embed_anchor(section: Option<&str>) -> String {
    match section {
        None => String::new(),
        Some(s) if s.is_empty() => String::new(),
        Some(s) => {
            if s.starts_with('^') {
                format!("#{}", s)
            } else {
                format!("#{}", obsidian_heading_anchor(s))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IframeRenderer
// ---------------------------------------------------------------------------

/// Renderer for local HTML embeds: `![[file.html?query#frag|WxH]]` → `<iframe>`.
///
/// - `?query` is appended to the iframe `src` as URL query.
/// - `#fragment` is appended as URL fragment (order: path?query#frag).
/// - `|W` or `|WxH` becomes iframe width/height attributes via [`Sizing`].
/// - No sandbox attribute is set by default — noted as a follow-up.
#[derive(Debug)]
pub struct IframeRenderer;

impl EmbedRenderer for IframeRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["html", "htm"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(render_link_markdown(embed, "iframe", iframe_extra_params))
    }
}

/// Iframe-specific param extraction:
///
/// - `?query` and `#fragment` from the wikilink fold into a `src` param so
///   Stage 2 can reconstruct the URL it serves on the iframe element. They
///   are NOT re-inserted into the markdown URL slot because pulldown-cmark
///   would percent-encode them, which would break iframe `src` semantics
///   downstream.
/// - `|WxH` sizing in alias becomes `width=`/`height=` params.
/// - Non-sizing alias text becomes the `title=` param (used today as
///   accessible name / tooltip on the iframe).
fn iframe_extra_params(embed: &ParsedEmbed<'_>, params: &mut TitleParams) {
    if let Some(q) = embed.query {
        params.insert("query", q);
    }
    if let Some(f) = embed.section {
        params.insert("fragment", f);
    }
    let Some(alias) = embed.alias else {
        return;
    };
    match Sizing::parse(alias) {
        Some(Sizing::Width(w)) => {
            params.insert("width", w.to_css());
        }
        Some(Sizing::Box(w, h)) => {
            params.insert("width", w.to_css());
            params.insert("height", h.to_css());
        }
        None => {
            // Non-sizing text alias → iframe title.
            params.insert("title", alias);
        }
    }
}

// build_src, dim_attrs, html_escape_attr now live in common.rs — imported above.

// ---------------------------------------------------------------------------
// PdfRenderer
// ---------------------------------------------------------------------------

/// Renderer for PDF embeds: `![[report.pdf]]` → `<object type="application/pdf">`.
///
/// `<object>` has better keyboard navigation than `<iframe>` for PDFs and
/// supports inline fallback content for browsers that can't render PDFs natively.
#[derive(Debug)]
pub struct PdfRenderer;

impl EmbedRenderer for PdfRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["pdf"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(render_link_markdown(embed, "pdf", pdf_extra_params))
    }
}

/// PDF-specific params: viewer fragment (`#page=5`), sizing.
fn pdf_extra_params(embed: &ParsedEmbed<'_>, params: &mut TitleParams) {
    if let Some(q) = embed.query {
        params.insert("query", q);
    }
    if let Some(f) = embed.section {
        params.insert("fragment", f);
    }
    if let Some(alias) = embed.alias {
        match Sizing::parse(alias) {
            Some(Sizing::Width(w)) => {
                params.insert("width", w.to_css());
            }
            Some(Sizing::Box(w, h)) => {
                params.insert("width", w.to_css());
                params.insert("height", h.to_css());
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------
// AudioRenderer
// ---------------------------------------------------------------------------

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac", "m4a", "opus", "aac"];

/// Renderer for audio embeds: `![[song.mp3]]` → `<audio controls>`.
///
/// `preload=metadata` so the browser fetches duration/sample-rate but not the
/// full payload until the user presses play.
///
/// Output form: `<audio><source src="..." type="..."></audio>` (HTML5
/// multi-source). This is safe today because no audio extension rewriter
/// exists in src-tauri — audio files pass through unchanged. If a future
/// converter is introduced (e.g., `.flac→.mp3` for size, `.m4a→.opus` for
/// browser parity, see #504), this renderer must switch to the single
/// `src=` form for the same reason `VideoRenderer` did: the
/// `add_*_placeholder_attributes` regex pattern in
/// `src-tauri/src/build/media/placeholder.rs` matches `<tag\s+[^>]*?src=>`,
/// not nested `<source>` children. See #593 and the docstring on
/// `VideoRenderer` for the full failure mode.
#[derive(Debug)]
pub struct AudioRenderer;

impl EmbedRenderer for AudioRenderer {
    fn extensions(&self) -> &[&'static str] {
        AUDIO_EXTENSIONS
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(render_link_markdown(embed, "audio", audio_extra_params))
    }
}

/// Audio-specific params: source extension (for downstream MIME selection).
/// The historical author grammar exposed no per-embed audio flags, so today
/// the only extra param is the file extension. Future flags (`controls`,
/// `loop`, `autoplay`, `muted`) extend here.
fn audio_extra_params(embed: &ParsedEmbed<'_>, params: &mut TitleParams) {
    let ext = path_extension_lower(embed.resolved_path);
    if !ext.is_empty() {
        params.insert("ext", ext);
    }
}

// MIME-type selection for audio embeds now lives downstream (Phase 1
// Stage 2 picks the MIME from the `ext=` title param when synthesizing the
// `<audio><source>` HTML). moss-core's Stage 1 emits the file extension
// directly via `audio_extra_params`; the legacy `audio_mime_for_ext` helper
// is no longer needed here.

// ---------------------------------------------------------------------------
// VideoRenderer
// ---------------------------------------------------------------------------

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mov", "m4v"];

/// Renderer for video embeds: `![[clip.mp4]]` → `<video src="..." controls>`.
///
/// `|WxH` becomes width/height attrs. `preload=metadata` so the browser
/// fetches duration/dimensions but not the full payload until play.
///
/// Output form: single `src=` attribute on `<video>` (no `<source>` child),
/// no `type=` attribute. Two coupled reasons:
///
/// 1. **`type=` would go stale.** The downstream rewriter
///    `src-tauri/src/build/media/placeholder.rs::add_video_placeholder_attributes`
///    rewrites the `src` extension from `.mov` to `.mp4` after the renderer
///    runs (moss converts `.mov` source files to `.mp4` during build, so a
///    raw `.mov` reference would 404). Any explicit `type="video/quicktime"`
///    emitted here would survive the rewrite as a lie. Browser sniffing
///    from the rewritten URL extension is more reliable than a stale type
///    hint.
///
/// 2. **The rewriter regex requires single-`src=` form.** It matches
///    `<video\s+[^>]*?src="...">` — `src=` must be on the `<video>` tag
///    itself, not on a nested `<source>` child. With nested `<source>`,
///    the regex no-ops and the `.mov→.mp4` rewrite + `data-placeholder-src`
///    + `poster` + `data-thumb-src` injection all silently drop. This
///    constraint is load-bearing; see #592 for an integration test that
///    pins it across the cross-crate boundary, and #593 for the audio
///    asymmetry. This shape also matches the historical moss output that
///    liu-guo.com still ships.
///
/// Note: `.mov` is codec-dependent at the source. Safari plays QuickTime
/// natively; Chrome/Firefox accept the MIME but decode only if the
/// container's video codec is supported (usually H.264). The `.mov→.mp4`
/// rewriter solves this in practice — served files end as `.mp4`.
#[derive(Debug)]
pub struct VideoRenderer;

impl EmbedRenderer for VideoRenderer {
    fn extensions(&self) -> &[&'static str] {
        VIDEO_EXTENSIONS
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(render_link_markdown(embed, "video", video_extra_params))
    }
}

/// Video-specific params: sizing from alias. Author flags (controls, loop,
/// autoplay, muted, poster) extend here when wired up.
fn video_extra_params(embed: &ParsedEmbed<'_>, params: &mut TitleParams) {
    if let Some(alias) = embed.alias {
        match Sizing::parse(alias) {
            Some(Sizing::Width(w)) => {
                params.insert("width", w.to_css());
            }
            Some(Sizing::Box(w, h)) => {
                params.insert("width", w.to_css());
                params.insert("height", h.to_css());
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------
// NotebookRenderer
// ---------------------------------------------------------------------------

/// Renderer for Jupyter notebooks: `![[file.ipynb]]` → deferred marker.
///
/// Emits `<!-- moss-embed-ipynb:PATH -->` (with optional `?query` appended).
/// The real rendering happens in src-tauri via nbconvert or equivalent —
/// src-tauri resolves the marker post-pass.
#[derive(Debug)]
pub struct NotebookRenderer;

impl EmbedRenderer for NotebookRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["ipynb"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        // NOTE: embed.width is intentionally dropped at the marker boundary.
        // Notebook wrappers are emitted in src-tauri post-passes that don't
        // currently read width from the marker target. Track when those
        // switch to data-width emission — file a follow-up issue if needed.
        let target = match embed.query {
            Some(q) => format!("{}?{}", embed.resolved_path, q),
            None => embed.resolved_path.to_string(),
        };
        RenderedEmbed::Deferred {
            marker: format!("<!-- {}:{} -->", MARKER_IPYNB, target),
        }
    }
}

// ---------------------------------------------------------------------------
// ModelViewerRenderer (3D)
// ---------------------------------------------------------------------------

/// Page-level script import needed for `<model-viewer>` to work.
///
/// Loaded from Google's CDN. Pinned to a major version for stability.
/// If this URL becomes unavailable, self-host and update this constant.
const MODEL_VIEWER_SCRIPT: &str = "<script type=\"module\" src=\"https://ajax.googleapis.com/ajax/libs/model-viewer/3.4.0/model-viewer.min.js\"></script>";

/// Renderer for 3D model embeds: `![[model.glb|400x400]]` → `<model-viewer>`.
///
/// Requires the `<model-viewer>` custom element script, injected via
/// `head_assets` once per page that contains any `.glb`/`.gltf` embed.
#[derive(Debug)]
pub struct ModelViewerRenderer;

impl EmbedRenderer for ModelViewerRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["glb", "gltf"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(render_link_markdown(embed, "3d", model_viewer_extra_params))
    }

    fn head_assets(&self) -> &[&'static str] {
        &[MODEL_VIEWER_SCRIPT]
    }
}

/// 3D-viewer-specific params: sizing from alias. Author flags (`auto-rotate`,
/// `camera-controls`, `ar`) extend here when surfaced in the wikilink grammar.
fn model_viewer_extra_params(embed: &ParsedEmbed<'_>, params: &mut TitleParams) {
    if let Some(alias) = embed.alias {
        match Sizing::parse(alias) {
            Some(Sizing::Width(w)) => {
                params.insert("width", w.to_css());
            }
            Some(Sizing::Box(w, h)) => {
                params.insert("width", w.to_css());
                params.insert("height", h.to_css());
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------
// TableRenderer
// ---------------------------------------------------------------------------

/// Renderer for tabular data: `![[data.csv]]` → deferred marker.
///
/// Emits `<!-- moss-embed-table:PATH -->`. src-tauri reads the CSV/TSV file
/// and calls `moss_core::csv_table::render` (a pure renderer) in a post-pass.
#[derive(Debug)]
pub struct TableRenderer;

impl EmbedRenderer for TableRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["csv", "tsv"]
    }

    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        // NOTE: embed.width is intentionally dropped at the marker boundary.
        // Table wrappers are emitted in src-tauri post-passes (csv_table) that
        // don't currently read width from the marker target. Track when those
        // switch to data-width emission — file a follow-up issue if needed.
        RenderedEmbed::Deferred {
            marker: format!("<!-- {}:{} -->", MARKER_TABLE, embed.resolved_path),
        }
    }
}

#[cfg(test)]
#[path = "embed_renderer_tests.rs"]
mod tests;
