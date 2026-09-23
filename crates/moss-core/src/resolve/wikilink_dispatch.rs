//! Phase 3: Stage 2 entry point for wikilink embed dispatch.
//!
//! This module is the sole dispatcher for `[[…]]` / `![[…]]` events
//! emitted by pulldown-cmark with `Options::ENABLE_WIKILINKS`. The
//! src-tauri pipeline's `transform_events` (in
//! `src-tauri/src/build/markdown/pipeline.rs`) calls
//! [`dispatch_wikilink_embed_with_registry`] once per WikiLink-typed
//! event, swallows the event range, and substitutes the renderer-
//! produced HTML.
//!
//! # History
//!
//! - **PR1 (`c2fbdd593`)**: this module landed as a dormant API alongside
//!   the dispatch arm shape in `transform_events` (also dormant — gated
//!   by the absence of `ENABLE_WIKILINKS`).
//! - **PR2 (this change)**: enabled `ENABLE_WIKILINKS` at every
//!   `Parser::new_ext` site, wired the dispatcher closure into
//!   `transform_events`, and deleted the prior Stage 1 string-rewriter
//!   (`crates/moss-core/src/resolve/wikilinks.rs`, ~2155 LOC).
//!
//! # What this reuses
//!
//! - Extension routing goes through [`super::embed_renderer::lookup_renderer`]
//!   (the same registry the pre-PR2 Stage 1 resolver used). No parallel
//!   dispatcher.
//! - Anchor / query splitting on `dest_url` mirrors the pre-PR2
//!   `wikilinks::parse_wikilink_inner`'s `#` / `?` priority logic.
//! - Width-token extraction uses [`crate::media::extract_width_from_alias`].
//!
//! # What's new
//!
//! - [`parse_pothole_params`] reads the pothole text (the `bar` in
//!   `[[foo|bar]]`) and classifies it as one of:
//!   * empty — no pothole
//!   * width-token — Obsidian `[[img.jpg|400]]` shorthand
//!   * params — `width=400 align=left` (every-token-K=V rule)
//!   * alias — plain display text
//!
//!   The every-token-K=V rule (locked by arch review) prevents free-text
//!   captions like `alt text=cover` from being mis-parsed as `text=cover`.

use crate::asset_snapshot::AssetSnapshot;
use crate::content_graph::ContentGraph;
use crate::path_ext::path_extension;
use crate::media::{
    extract_placement_from_alias, extract_width_from_alias, parse_media_attrs, AlignSide, Fit,
    MediaAttrs, Placement, Position,
};

use super::embed_renderer::{
    lookup_renderer, EmbedRenderer, ParsedEmbed, RenderedEmbed, Sizing, IMAGE_EXTENSIONS,
};
use super::fuzzy_path::{resolve_reference, ResolvedRef};
use super::title_params::TitleParams;
use super::{Diagnostic, DiagnosticKind, LinkType, OutgoingLink};

/// Classification of pothole text (`|...` in `[[file|...]]`).
#[derive(Debug, Clone, PartialEq)]
pub enum PotholeContent {
    /// No pothole or pothole is whitespace-only.
    Empty,
    /// Obsidian width-token shorthand: `[[img.jpg|400]]`, `[[img.jpg|100%]]`,
    /// `[[img.jpg|200x150]]`. Carries the canonical width string
    /// (one of `body | wide | page | screen` after token-matching) and the
    /// trailing alias remainder (often empty).
    WidthToken {
        width: &'static str,
        rest_alias: String,
    },
    /// Typed params: every whitespace-separated token matched
    /// `^[a-z][a-z0-9_-]*=...`.
    Params(TitleParams),
    /// Plain alias display text (Obsidian default).
    Alias(String),
}

/// Result of splitting a wikilink `dest_url` into its `file`, `section`,
/// `query` components.
///
/// Pulldown-cmark hands us `dest_url` verbatim — `[[foo#bar?baz]]` arrives as
/// `dest_url="foo#bar?baz"`. We still need to split for renderer dispatch
/// (image / markdown / iframe / etc.) and for emitted-href construction.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitDestUrl<'a> {
    pub file: &'a str,
    pub section: Option<&'a str>,
    pub query: Option<&'a str>,
}

/// Output of [`dispatch_wikilink_embed`].
#[derive(Debug, Clone)]
pub struct WikilinkEmit {
    /// Rendered HTML or markdown to splice into the event stream.
    /// For an embed (`![[…]]`) this is the renderer's output. For a plain
    /// wikilink (`[[…]]`) this is a markdown link the caller can let
    /// pulldown-cmark re-parse, or a final HTML fragment.
    pub output: EmitKind,
    /// Outgoing link to register with ContentGraph.
    pub outgoing_link: Option<OutgoingLink>,
    /// Diagnostics (e.g. unresolved reference).
    pub diagnostics: Vec<Diagnostic>,
}

/// The shape of the dispatcher's emitted content. Mirrors
/// [`super::embed_renderer::RenderedEmbed`] for embeds, plus a separate
/// variant for non-embed wikilinks (`[[file]]`).
#[derive(Debug, Clone, PartialEq)]
pub enum EmitKind {
    /// Markdown-level text that downstream CommonMark will re-process.
    /// Example: image renderer returns `![alt](url)`.
    Inline(String),
    /// Final HTML — must NOT be re-parsed by the markdown engine.
    /// Example: iframe renderer.
    Html(String),
    /// A marker comment for a post-pass resolver (notebook, table, plugin).
    Deferred(String),
    /// A standard markdown link string. Used for non-embed wikilinks
    /// (`[[file]]` rather than `![[file]]`).
    Link(String),
    /// A typed AST block to splice in directly (image-embed synth-collapse).
    /// Unlike [`EmitKind::Html`] (which lands as an opaque `Block::Other`
    /// carrying no `BlockMeta`), a typed block placed 1:1 at `blocks[i]`
    /// inherits the source paragraph's `block_meta[i]` — so a lone image
    /// embed rendered as `Block::Figure` keeps its `data-source-line` in
    /// preview/site builds. The image arm uses this; non-image embeds keep
    /// emitting `EmitKind::Html`.
    Block(Box<crate::ast::node::Block>),
}

/// Parse pothole text using the every-token-K=V rule.
///
/// Order of attempts:
/// 1. Empty → [`PotholeContent::Empty`].
/// 2. Obsidian width-token (`400`, `100%`, `200x150`, `full`, etc.) via
///    [`extract_width_from_alias`] → [`PotholeContent::WidthToken`].
/// 3. Every whitespace-separated token matches `^[a-z][a-z0-9_-]*=...`
///    → [`PotholeContent::Params`].
/// 4. Otherwise → [`PotholeContent::Alias`].
///
/// The every-token rule is critical: `[[file|alt text=cover]]` must be
/// recognized as alias text (because `alt` is bare), not as a `text=cover`
/// param. See plan v2 revision notes.
pub fn parse_pothole_params(text: &str) -> PotholeContent {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PotholeContent::Empty;
    }

    // Step 2: try Obsidian width-token shorthand. This recognizes single
    // tokens like `400`, `100%`, `200x150`, `wide`, `full`, etc. The width
    // matcher only fires on isolated tokens; multi-word free text like
    // "wide angle photo" is not classified as a width token.
    let (width, rest_alias) = extract_width_from_alias(trimmed);
    if let Some(w) = width {
        return PotholeContent::WidthToken {
            width: w,
            rest_alias,
        };
    }

    // Step 3: every-token-K=V rule. Each whitespace-separated token must
    // match `^[a-z][a-z0-9_-]*=`. If ANY token fails the pattern, the
    // entire pothole falls through to alias.
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if !tokens.is_empty() && tokens.iter().all(|t| is_kv_token(t)) {
        let mut params = TitleParams::default();
        for token in &tokens {
            if let Some((k, v)) = token.split_once('=') {
                params.insert(k, v);
            }
        }
        return PotholeContent::Params(params);
    }

    // Step 4: fallback. Preserve original text exactly (caller may want
    // verbatim alias display).
    PotholeContent::Alias(text.to_string())
}

/// Test if a single token matches the K=V pattern: `^[a-z][a-z0-9_-]*=...`.
///
/// The key must start with a lowercase ASCII letter and continue with
/// lowercase ASCII letters / digits / underscore / hyphen, followed by
/// `=`. The value side is not constrained here.
fn is_kv_token(token: &str) -> bool {
    let Some((key, _value)) = token.split_once('=') else {
        return false;
    };
    if key.is_empty() {
        return false;
    }
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false; // empty key (guarded above, but keep the type-safe form)
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Split a pulldown-cmark wikilink `dest_url` into `file`, `section`, `query`.
///
/// Ported from the pre-Phase-3 `wikilinks::parse_wikilink_inner` (the
/// `before-pipe` half — the `|alias` part is handled by pulldown-cmark
/// via pothole events, so it doesn't appear in `dest_url`).
///
/// Whichever of `#` or `?` appears first in `dest_url` owns its tail; the
/// other is split out of that tail. Matches Obsidian's heading-ref priority
/// (`[[file#section]]`) while accepting URL-style mixes
/// (`[[file.html?x=1#frag]]`).
pub fn split_dest_url(dest_url: &str) -> SplitDestUrl<'_> {
    let hash_pos = dest_url.find('#');
    let query_pos = dest_url.find('?');

    // char-aligned: `h`/`q` are byte indices of ASCII `#`/`?`, each a
    // single-byte UTF-8 char. `h+1`/`q+1` step over the single byte.
    #[allow(clippy::string_slice)]
    match (hash_pos, query_pos) {
        (None, None) => SplitDestUrl {
            file: dest_url,
            section: None,
            query: None,
        },
        (Some(h), None) => SplitDestUrl {
            file: &dest_url[..h],
            section: Some(&dest_url[h + 1..]),
            query: None,
        },
        (None, Some(q)) => SplitDestUrl {
            file: &dest_url[..q],
            section: None,
            query: Some(&dest_url[q + 1..]),
        },
        (Some(h), Some(q)) if h < q => SplitDestUrl {
            file: &dest_url[..h],
            section: Some(&dest_url[h + 1..q]),
            query: Some(&dest_url[q + 1..]),
        },
        (Some(h), Some(q)) => SplitDestUrl {
            file: &dest_url[..q],
            section: Some(&dest_url[h + 1..]),
            query: Some(&dest_url[q + 1..h]),
        },
    }
}

/// Build the anchor fragment (e.g. `#getting-started` or `#block-id`) from a
/// section reference. Mirrors [`super::wikilinks`]'s `build_anchor`.
///
/// # Live-build scope (read before relying on this)
///
/// `build_anchor` is reached ONLY from [`dispatch_wikilink_form`] (the
/// `is_embed: false` branch). That branch is DORMANT in production: the sole
/// runtime caller of this dispatcher — the AST visitor
/// [`crate::ast::dispatch_wikilink_embeds`] — hard-codes `is_embed: true`
/// (it walks only `![[…]]` image embeds). The user-facing
/// `[[Page#Heading]]` text-link fragment slugging is performed instead by
/// `crate::ast::resolve_urls::slug_wikilink_suffix`, which this function
/// mirrors. Keep the two in sync; the resolve_urls tests are the real guards.
fn build_anchor(section: Option<&str>) -> String {
    use crate::heading::anchor::obsidian_heading_anchor;
    match section {
        None => String::new(),
        Some("") => String::new(),
        Some(s) => {
            if let Some(block_id) = s.strip_prefix('^') {
                format!("#{}", block_id)
            } else {
                format!("#{}", obsidian_heading_anchor(s))
            }
        }
    }
}

/// Phase 3 PR1: Stage 2 entry point for wikilink dispatch.
///
/// Reads a parsed wikilink (the `dest_url` and pothole-text fields from
/// pulldown-cmark's `Tag::Link { link_type: LinkType::WikiLink { has_pothole } }`
/// or `Tag::Image { … LinkType::WikiLink … }`) and produces rendered output
/// via the existing [`super::embed_renderer`] registry.
///
/// # Arguments
///
/// * `dest_url` — pulldown-cmark's `dest_url` (everything before `|` in
///   the source; may carry `#section` and/or `?query` fragments).
/// * `pothole` — the pothole text (everything after `|`), or `None` if
///   `has_pothole=false`.
/// * `is_embed` — `true` for `![[…]]` (image-form), `false` for `[[…]]`.
///   Routes embeds through the registry; routes plain wikilinks to a
///   standard markdown link.
/// * `graph` — content graph for path resolution.
/// * `from_path` — calling file's path (for relative URL computation +
///   diagnostics).
///
/// # Status (Phase 3 PR1, dormant)
///
/// This function compiles and is unit-tested, but no caller wires it in
/// at runtime yet. PR2 enables `ENABLE_WIKILINKS` and adds the call from
/// `src-tauri/src/build/markdown/pipeline.rs::transform_events`.
pub fn dispatch_wikilink_embed(
    dest_url: &str,
    pothole: Option<&str>,
    is_embed: bool,
    graph: &ContentGraph,
    from_path: &str,
    assets: &AssetSnapshot,
) -> WikilinkEmit {
    dispatch_wikilink_embed_with_lookup(
        dest_url,
        pothole,
        is_embed,
        graph,
        from_path,
        assets,
        &|ext| lookup_renderer(ext).map(|r| r as &dyn EmbedRenderer),
    )
}

/// Like [`dispatch_wikilink_embed`] but threads a custom registry lookup.
/// Used when the caller has plugin-registered renderers.
pub fn dispatch_wikilink_embed_with_registry(
    dest_url: &str,
    pothole: Option<&str>,
    is_embed: bool,
    graph: &ContentGraph,
    from_path: &str,
    assets: &AssetSnapshot,
    registry: &super::registry::RendererRegistry,
) -> WikilinkEmit {
    dispatch_wikilink_embed_with_lookup(
        dest_url,
        pothole,
        is_embed,
        graph,
        from_path,
        assets,
        &|ext| registry.lookup(ext).map(|r| r as &dyn EmbedRenderer),
    )
}

fn dispatch_wikilink_embed_with_lookup(
    dest_url: &str,
    pothole: Option<&str>,
    is_embed: bool,
    graph: &ContentGraph,
    from_path: &str,
    assets: &AssetSnapshot,
    lookup: &dyn Fn(&str) -> Option<&dyn EmbedRenderer>,
) -> WikilinkEmit {
    let split = split_dest_url(dest_url);
    let pothole_content = match pothole {
        None => PotholeContent::Empty,
        Some(s) => parse_pothole_params(s),
    };

    if is_embed {
        dispatch_embed_form(&split, pothole_content, graph, from_path, assets, lookup)
    } else {
        dispatch_wikilink_form(&split, pothole_content, graph, from_path)
    }
}

/// Reassemble a `SplitDestUrl` back into a single URL string.
///
/// Inverse of `split_dest_url`. Used before external-URL provider detection
/// so the full URL (including `?query` and `#fragment`) is available.
///
/// Note: always emits in canonical `?query#fragment` order regardless of the
/// original source order. For well-formed URLs (query before fragment) this is
/// byte-identical to the input. Degenerate `#fragment?query` inputs are silently
/// reordered — acceptable for external URL embeds where providers only accept
/// canonical query-first URLs.
fn reassemble_url(split: &SplitDestUrl<'_>) -> String {
    let mut url = split.file.to_string();
    if let Some(q) = split.query {
        url.push('?');
        url.push_str(q);
    }
    if let Some(s) = split.section {
        url.push('#');
        url.push_str(s);
    }
    url
}

/// Dispatch for `![[…]]` (embed form). Mirrors `resolve_embed`'s body.
fn dispatch_embed_form(
    split: &SplitDestUrl<'_>,
    pothole: PotholeContent,
    graph: &ContentGraph,
    from_path: &str,
    assets: &AssetSnapshot,
    lookup: &dyn Fn(&str) -> Option<&dyn EmbedRenderer>,
) -> WikilinkEmit {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // External URL embed: bypass ContentGraph resolution entirely.
    // Any http:// or https:// URL is synthesized as an iframe directly.
    // Provider detection (YouTube/Vimeo/CodePen) happens inside
    // synthesize_url_embed_html; unrecognised URLs get a generic <iframe>.
    if split.file.starts_with("http://") || split.file.starts_with("https://") {
        let full_url = reassemble_url(split);
        let html = crate::render::url_embed::synthesize_url_embed_html(
            &full_url,
            &pothole,
            assets,
        );
        return WikilinkEmit {
            output: EmitKind::Html(html),
            outgoing_link: None,
            diagnostics: vec![],
        };
    }

    // A trailing-slash embed (`![[/journal/]]`, the folder-list form) never
    // reaches this dispatcher for a real document: `resolve.rs`'s pre-pass
    // (`lower_transclusion_and_folder_wikilinks`) claims and emits the
    // folder-list marker itself, before pulldown-cmark — and therefore this
    // function — ever sees the document. This branch used to duplicate that
    // handling here as dead code; deleted rather than kept unreachable.

    // Resolve. Same logic as resolve_embed: empty file → same file;
    // non-empty → fuzzy resolve.
    let resolved = if split.file.is_empty() {
        ResolvedRef::Found(from_path.to_string())
    } else {
        resolve_reference(split.file, graph, from_path)
    };

    // Derive `alias` and `placement` for ParsedEmbed from the pothole.
    // For PotholeContent::Params we surface no alias; the params are
    // carried via TitleParams (consumers in PR4 onward can read them
    // directly without round-tripping through the `moss:` title channel).
    // For PR1 the params get folded into the renderer via the same path
    // Stage 1 uses today: there's no Stage-2 consumer yet, so we forward
    // alias as None when we have pure params.
    //
    // The placement is read off EVERY pipe segment, not just the first, so
    // `align-right|33%|A caption` keeps its float, its size and its caption.
    // What the placement did not claim is the alias every kind parses on its
    // own terms (sizing sugar, an iframe title, an image caption).
    let (alias_owned, placement): (Option<String>, Placement) = match &pothole {
        PotholeContent::Empty => (None, Placement::default()),
        PotholeContent::WidthToken { width, rest_alias } => {
            let (mut p, rest) = extract_placement_from_alias(rest_alias);
            p.width = Some(*width);
            (if rest.is_empty() { None } else { Some(rest) }, p)
        }
        PotholeContent::Params(params) => (None, placement_from_params(params)),
        PotholeContent::Alias(s) => {
            let (p, rest) = extract_placement_from_alias(s);
            (if rest.is_empty() { None } else { Some(rest) }, p)
        }
    };

    match resolved {
        ResolvedRef::Found(target_path) => {
            let outgoing = OutgoingLink {
                target_path: target_path.clone(),
                display_text: split.file.to_string(),
                link_type: LinkType::Embed,
            };

            let pinned_url = graph.pinned_url(&target_path);
            let parsed = ParsedEmbed {
                resolved_path: &target_path,
                from_path,
                pinned_url: &pinned_url,
                query: split.query,
                section: split.section,
                alias: alias_owned.as_deref(),
                placement: placement.clone(),
                attrs: None,
            };

            // Non-image wikilink embeds (video / pdf / audio / iframe / 3D)
            // route DIRECTLY to the typed-HTML synthesizer: derive
            // `TitleParams` from the pothole content and the pinned URL, then
            // hand them to the per-kind synthesizer. Routing them through
            // `EmitKind::Inline(markdown_link)` instead rendered them as plain
            // `<a href>` links, because the markdown round-trip drops the
            // title the params travelled in. Image embeds keep inline-markdown
            // emission so the `<picture>` / `<figure>` wrap stays on the path
            // that already worked.
            let ext = path_extension(&target_path);
            // Page-independent and case-canonical: same href from the vault root
            // and from a note nested three folders down (moss#903 bug 3).
            let url = pinned_url.clone();
            if let Some(synth_kind) = ext.as_deref().and_then(synth_kind_for_ext) {
                // What the placement did not take is a caption only when the
                // author wrote placement at all and the kind's own alias
                // grammar has no claim on it. Every pothole that means
                // something today therefore still means it: `|My Widget`
                // stays an iframe title, `|640x360` stays sizing.
                let caption: Option<String> = match parsed.alias {
                    Some(rest)
                        if !placement.is_empty() && !alias_is_sizing(synth_kind, rest) =>
                    {
                        Some(rest.to_string())
                    }
                    _ => None,
                };
                // With a caption, the whole placement — width, float AND
                // size — moves out to the wrapper: the width escape is a
                // direct-child selector, only the outermost element can
                // satisfy it, and a figure has no width of its own to fall
                // back on for its size either. CSS fills the inner element
                // to 100% of the figure.
                let element_placement = match caption {
                    Some(_) => Placement::default(),
                    None => placement.clone(),
                };
                let parsed = ParsedEmbed {
                    // A visible caption owns the text, so it is not repeated
                    // as the iframe's accessible name — the same reason a
                    // captioned figure blanks its image's `alt`.
                    alias: if caption.is_some() { None } else { parsed.alias },
                    placement: element_placement.clone(),
                    ..parsed
                };
                let params = build_synth_params(synth_kind, &parsed, &pothole);
                let html = match synth_kind {
                    SynthKind::Video => crate::render::video::synthesize_video_html(
                        &params,
                        &element_placement,
                        &url,
                        assets,
                    ),
                    SynthKind::Pdf => crate::render::pdf::synthesize_pdf_html(
                        &params,
                        &element_placement,
                        &url,
                        assets,
                    ),
                    SynthKind::Audio => crate::render::audio::synthesize_audio_html(
                        &params,
                        &element_placement,
                        &url,
                        assets,
                    ),
                    SynthKind::Iframe => crate::render::iframe::synthesize_iframe_html(
                        &params,
                        &element_placement,
                        &url,
                        assets,
                    ),
                    SynthKind::Model => crate::render::model::synthesize_model_html(
                        &params,
                        &element_placement,
                        &url,
                        assets,
                    ),
                };
                let html = match caption {
                    Some(ref c) => crate::render::placement::wrap_embed_with_caption(
                        &html, &placement, c,
                    ),
                    None => html,
                };
                return WikilinkEmit {
                    output: EmitKind::Html(html),
                    outgoing_link: Some(outgoing),
                    diagnostics,
                };
            }

            // Image embeds — the unified arm (image-embed synth-collapse).
            //
            // ALL `![[photo.jpg]]` forms route through here to a typed
            // `Block::Figure`, the SAME node the CommonMark `![](url)` path
            // produces. This replaced the prior split (a fit/position
            // "fast-path" that emitted bare `<picture>` + an
            // `ImageRenderer::render_to_markdown` round-trip that dropped
            // width via `let _ = params`). Six embed kinds already went
            // dispatch → synth → Html; image now matches via
            // dispatch → Block::Figure → render_document.
            //
            // Four sources of display params are assembled into the figure:
            //   1. width  ← `embed.width` (canonical WidthToken) → figure `data-width=`
            //   2. caption + alt ← `classify_image_alias` (structural → none,
            //      caption-text → both, empty → none; never `Some("")`)
            //   3. fit/position ← `build_image_media_attrs` → `to_inline_style()`
            //      → inner `<img>` `style=` (NOT the figure)
            //   4. align + class_names ← `build_image_media_attrs` → figure class list
            //
            // Emitting `EmitKind::Block` (not `EmitKind::Html`) keeps the
            // 1:1 `apply_emit` substitution so the figure inherits the source
            // paragraph's `block_meta` → `data-source-line` survives.
            //
            // `find_lone_wikilink_image` guarantees the dispatcher is only
            // reached for a lone embed (within its container), so the figure
            // shape is always correct here.
            if matches!(ext.as_deref(), Some(e) if IMAGE_EXTENSIONS.iter().any(|x| *x == e)) {
                let media = build_image_media_attrs(parsed.alias, &pothole, parsed.attrs.as_ref());
                // The alias reaching here is the placement-stripped remainder,
                // so what is left is either image display keywords or a caption.
                let alias_class = crate::media::classify_image_alias(parsed.alias);
                let alt = alias_class.caption.clone().unwrap_or_default();
                let caption: Option<Vec<crate::ast::node::Inline>> = alias_class
                    .caption
                    .map(|c| vec![crate::ast::node::Inline::Text(c)]);
                // `AlignSide::css_class()` returns the canonical
                // `moss-align-left` / `moss-align-right` class verbatim —
                // the same class the figure renderer appends.
                let align = placement.align.map(|side| side.css_class().to_string());
                let img_style = media.to_inline_style();
                // `Block::Figure.width` carries both width vocabularies: a
                // named token emits `data-width=`, a percent emits an inline
                // `style="width:NN%"`. A percent wins over a named width when
                // an author wrote both, the same rule `placement_attrs` holds
                // for every other embed: the escape's centring margins assume
                // the token's width, so both at once push the figure out of
                // the column.
                let figure_width: Option<String> = placement
                    .size
                    .clone()
                    .or_else(|| placement.width.map(str::to_string));
                let figure = crate::ast::node::Block::Figure {
                    image: crate::ast::node::Inline::Image {
                        // `Asset` is the canonical kind for an `<img src>`
                        // (matches resolve_urls' image-URL classification).
                        src: crate::ast::url::Url::resolved(
                            url.clone(),
                            crate::ast::url::UrlKind::Asset,
                        ),
                        alt,
                        title: None,
                        is_wikilink: true,
                        wikilink_pothole: None,
                    },
                    caption,
                    // Named token OR `"NN%"` percent; the node stores
                    // `Option<String>` (for Deserialize).
                    width: figure_width,
                    align,
                    class_names: media.class_names,
                    img_style,
                };
                return WikilinkEmit {
                    output: EmitKind::Block(Box::new(figure)),
                    outgoing_link: Some(outgoing),
                    diagnostics,
                };
            }

            let emit = match ext.as_deref().and_then(lookup) {
                Some(r) => match r.render(&parsed) {
                    RenderedEmbed::Inline(s) => EmitKind::Inline(s),
                    RenderedEmbed::Html(s) => EmitKind::Html(s),
                    RenderedEmbed::Deferred { marker } => EmitKind::Deferred(marker),
                },
                None => {
                    // Fallback: plain file link (Obsidian parity for
                    // unknown extensions).
                    EmitKind::Inline(format!("[{}]({})", split.file, url))
                }
            };

            WikilinkEmit {
                output: emit,
                outgoing_link: Some(outgoing),
                diagnostics,
            }
        }
        ResolvedRef::Unresolved => {
            diagnostics.push(Diagnostic {
                message: format!("Unresolved embed: ![[{}]]", split.file),
                source_path: from_path.to_string(),
                reference: split.file.to_string(),
                kind: crate::resolve::ext_kind::missing_reference_kind(path_extension(split.file).as_deref()),
            });

            WikilinkEmit {
                output: EmitKind::Inline(format!(
                    "[{}](moss-unresolved:{})",
                    split.file, split.file
                )),
                outgoing_link: Some(OutgoingLink {
                    target_path: split.file.to_string(),
                    display_text: split.file.to_string(),
                    link_type: LinkType::Embed,
                }),
                diagnostics,
            }
        }
    }
}

/// Dispatch for `[[…]]` (plain wikilink). Mirrors `resolve_wikilink`'s body
/// (the non-embed case).
///
/// DORMANT in the live build: the only production caller
/// ([`crate::ast::dispatch_wikilink_embeds`]) always passes `is_embed: true`,
/// so plain `[[…]]` text links never route here. They reach the typed AST as
/// `Inline::Link { is_wikilink: true }` and are resolved by
/// `crate::ast::resolve_urls` instead. This function remains as a tested
/// helper (and for any future plugin/CLI caller that passes `is_embed:
/// false`).
fn dispatch_wikilink_form(
    split: &SplitDestUrl<'_>,
    pothole: PotholeContent,
    graph: &ContentGraph,
    from_path: &str,
) -> WikilinkEmit {
    let mut diagnostics = Vec::new();

    // For plain wikilinks, only Alias-shaped potholes contribute to
    // display text. Width tokens and params are meaningless on a
    // non-embed wikilink — preserve Stage 1 behavior by ignoring them.
    let alias_display = match &pothole {
        PotholeContent::Alias(s) => Some(s.clone()),
        PotholeContent::WidthToken { rest_alias, .. } if !rest_alias.is_empty() => {
            Some(rest_alias.clone())
        }
        _ => None,
    };

    let display_text = if let Some(a) = alias_display {
        a
    } else if let Some(sec) = split.section {
        if split.file.is_empty() {
            sec.to_string()
        } else {
            format!("{} > {}", split.file, sec)
        }
    } else {
        split.file.to_string()
    };

    let resolved = if split.file.is_empty() {
        ResolvedRef::Found(from_path.to_string())
    } else {
        resolve_reference(split.file, graph, from_path)
    };

    match resolved {
        ResolvedRef::Found(target_path) => {
            let outgoing = OutgoingLink {
                target_path: target_path.clone(),
                display_text: display_text.clone(),
                link_type: LinkType::Wikilink,
            };

            let anchor = build_anchor(split.section);
            let link = if split.file.is_empty() {
                format!("[{}]({})", display_text, anchor)
            } else {
                format!(
                    "[{}](moss-resolved:{}{})",
                    display_text, target_path, anchor
                )
            };

            WikilinkEmit {
                output: EmitKind::Link(link),
                outgoing_link: Some(outgoing),
                diagnostics,
            }
        }
        ResolvedRef::Unresolved => {
            diagnostics.push(Diagnostic {
                message: format!("Unresolved wikilink: [[{}]]", split.file),
                source_path: from_path.to_string(),
                reference: split.file.to_string(),
                kind: DiagnosticKind::Other,
            });
            WikilinkEmit {
                output: EmitKind::Link(format!(
                    "[{}](moss-unresolved:{})",
                    display_text, split.file
                )),
                outgoing_link: Some(OutgoingLink {
                    target_path: split.file.to_string(),
                    display_text,
                    link_type: LinkType::Wikilink,
                }),
                diagnostics,
            }
        }
    }
}

/// Build [`MediaAttrs`] from a pothole's alias / params.
///
/// Two active sources of display vocabulary for image embeds:
///
/// - Alias form (`![[hero.jpg|cover left]]`) — whitespace-separated
///   display keywords. The pothole arrives as
///   [`PotholeContent::Alias`] or [`PotholeContent::WidthToken::rest_alias`]
///   when a width token preceded the keywords. `parse_media_attrs` decodes
///   them into typed `fit` / `position` / `align` fields.
/// - Params form (`![[hero.jpg|fit=cover position=left]]`) — every token
///   is `key=value`. The pothole arrives as
///   [`PotholeContent::Params`] carrying a `TitleParams` bag; we look up
///   `fit` / `position` / `align` by name and convert their values via the
///   per-enum `from_keyword`. Unknown keys flow through as `extra_attrs`.
///
/// Pandoc attribute blocks (`![[hero.jpg|cover]]{.theme-rounded x="y"}`) are
/// a third potential source, but [`ParsedEmbed::attrs`] is currently
/// hard-coded to `None` at the dispatcher's image branch (see
/// `dispatch_embed_form`). The `attrs` parameter is plumbed through for
/// future wiring; today the function ignores it. Don't grow the merge
/// logic here until a caller actually populates `parsed.attrs`.
fn build_image_media_attrs(
    alias: Option<&str>,
    pothole: &PotholeContent,
    _attrs: Option<&crate::ast::attrs::AttrBlock>,
) -> MediaAttrs {
    let mut media = MediaAttrs::default();

    // Source 1: alias form — the placement-stripped remainder, so
    // `wide cover` arrives here as `cover` and parses cleanly. Only fold
    // when the whole remainder is display keywords; anything else is caption
    // text and contributes no display params.
    if let Some(text) = alias.filter(|t| !t.is_empty()) {
        if crate::media::is_all_display_keywords(text) {
            let parsed = parse_media_attrs(text);
            media.fit = parsed.fit;
            media.position = parsed.position;
            // Alignment is `Placement`'s, read off the pipe before this runs.
            // `parse_media_attrs` doesn't populate `class_names` or
            // `extra_attrs` today (those come from Pandoc blocks, which
            // aren't wired). The extends here are forward-looking scaffolding
            // — harmless no-ops on current `MediaAttrs` shape.
            media.class_names.extend(parsed.class_names);
            for (k, v) in parsed.extra_attrs {
                media.extra_attrs.insert(k, v);
            }
        }
    }

    // Source 2: Params form (K=V pothole). Recognized keys override; the
    // rest flow through as `extra_attrs`.
    //
    // `style` is filtered OUT here because `synthesize_image_with_media_attrs`
    // builds the `style="…"` attribute from `MediaAttrs::to_inline_style()`;
    // letting an author-typed `style=foo` ALSO flow into `extra_attrs` would
    // emit two `style=` attributes on the same `<img>` and the browser would
    // honor the last one, silently dropping moss's object-fit / object-position.
    if let PotholeContent::Params(params) = pothole {
        for (k, v) in &params.params {
            match k.as_str() {
                "fit" => {
                    if let Some(fit) = Fit::from_keyword(v) {
                        media.fit = Some(fit);
                    }
                }
                "position" => {
                    if let Some(pos) = Position::from_keyword(v) {
                        media.position = Some(pos);
                    }
                }
                // `align`, `width` and `data-width` ride on the wrapper, not
                // the inner `<img>` — handled upstream by `Placement`.
                "align" | "width" | "data-width" => {}
                "classes" => {
                    for c in v.split_whitespace() {
                        if !media.class_names.iter().any(|x| x == c) {
                            media.class_names.push(c.to_string());
                        }
                    }
                }
                // Drop `style=` to avoid duplicate-attribute emission;
                // see function-level note above.
                "style" => {}
                _ => {
                    media.extra_attrs.insert(k.clone(), v.clone());
                }
            }
        }
    }

    media
}

/// Whether a kind's own alias grammar claims `rest` as sizing sugar rather
/// than leaving it to be read as a caption.
///
/// Audio has no alias grammar, so nothing is ever claimed from it.
fn alias_is_sizing(kind: SynthKind, rest: &str) -> bool {
    match kind {
        SynthKind::Video => {
            // `loop` is a flag, not text; what surrounds it decides.
            let remainder: Vec<&str> = rest
                .split_whitespace()
                .filter(|t| !t.eq_ignore_ascii_case("loop"))
                .collect();
            remainder.is_empty() || Sizing::parse(&remainder.join(" ")).is_some()
        }
        SynthKind::Pdf | SynthKind::Model | SynthKind::Iframe => Sizing::parse(rest).is_some(),
        SynthKind::Audio => false,
    }
}

/// Read the placement out of a `key=value` pothole.
///
/// The K=V form is the other way an author writes the same three values
/// (`![[report.pdf|align=right data-width=wide]]`), so it lands in the same
/// carrier rather than a second one. `width=` is not read here: on the K=V
/// form it is the HTML pixel width attribute, which the per-kind synth params
/// still own.
fn placement_from_params(params: &TitleParams) -> Placement {
    let mut placement = Placement::default();
    for (k, v) in &params.params {
        match k.as_str() {
            "align" => placement.align = AlignSide::from_keyword(v),
            "data-width" => match crate::media::match_width_token(v) {
                Some(w) => placement.width = Some(w),
                None => {
                    placement.size = crate::media::parse_image_width(v)
                        .filter(|p| p.ends_with('%'));
                }
            },
            _ => {}
        }
    }
    placement
}

/// Discriminant for the per-kind HTML synthesizer the dispatcher routes to
/// directly (Phase 3 PR4.5). Non-image / non-deferred extensions skip the
/// markdown round-trip and emit `EmitKind::Html` straight from the synth
/// function — see the dispatcher branch in `dispatch_embed_form`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SynthKind {
    Video,
    Pdf,
    Audio,
    Iframe,
    Model,
}

/// Classify a file extension into a [`SynthKind`] when the dispatcher should
/// emit final HTML directly. Returns `None` for image (`png`/`jpg`/...) —
/// which keeps its inline-markdown round-trip — and for deferred kinds
/// (`md`/`ipynb`/`csv`/`tsv`) which still need src-tauri post-passes.
///
/// The built-in renderers and their `EmbedRenderer::extensions()` slices
/// were deleted as unreachable; `ext_kind::reference_kind_for_ext` is the
/// single source of truth for this table now.
fn synth_kind_for_ext(ext: &str) -> Option<SynthKind> {
    use crate::resolve::ext_kind::{reference_kind_for_ext, ExtKind};
    match reference_kind_for_ext(ext) {
        ExtKind::Video => Some(SynthKind::Video),
        ExtKind::Pdf => Some(SynthKind::Pdf),
        ExtKind::Audio => Some(SynthKind::Audio),
        ExtKind::Iframe => Some(SynthKind::Iframe),
        ExtKind::Model => Some(SynthKind::Model),
        ExtKind::Image | ExtKind::Transclusion | ExtKind::Notebook | ExtKind::Table | ExtKind::Other => None,
    }
}

/// Build the [`TitleParams`] handed to a per-kind synthesizer.
///
/// This is the canonical reference for which params each synth function
/// reads. It once mirrored a set of per-kind `*_extra_params` helpers in
/// `embed_renderer.rs` that fed the retired `moss:title` round-trip; the
/// iframe/pdf/audio/video ones were deleted as dead code (their `render()`
/// was unreachable from the live dispatch path this function actually
/// serves). Notable shape:
///
/// - **`data-width`** carries the canonical wrapper width (`body | wide |
///   page | screen`) when the pothole was an Obsidian width-token. Synth
///   functions emit it as the `data-width=` attribute on the wrapping
///   element.
/// - **`width` / `height`** come from `|WxH` sizing aliases parsed via
///   [`Sizing`]. Pixel/percent/vh values are CSS-formatted.
/// - **`title`** (iframe only) carries non-sizing alias text as the
///   iframe's accessible name (legacy behaviour: `[[widget.html|My Widget]]`).
/// - **`query` / `fragment`** (iframe/pdf only) reconstruct the served URL
///   from the split dest-url — pulldown-cmark percent-encodes `?` and `#`
///   if they stay in the URL slot, so the dispatcher hands them out-of-band.
/// - **Pothole `Params`** are folded last so author-typed `width=400` etc.
///   override the alias-derived values (every-token-K=V rule wins).
fn build_synth_params(
    kind: SynthKind,
    embed: &ParsedEmbed<'_>,
    pothole: &PotholeContent,
) -> TitleParams {
    let mut params = TitleParams::default();

    // iframe / pdf carry ?query and #fragment out-of-band on the synth side.
    if matches!(kind, SynthKind::Iframe | SynthKind::Pdf) {
        if let Some(q) = embed.query {
            params.insert("query", q);
        }
        if let Some(f) = embed.section {
            params.insert("fragment", f);
        }
    }

    // Per-kind alias handling. `embed.alias` is the pothole's alias-shaped
    // remainder (already excludes width tokens) — for non-image kinds it
    // overwhelmingly looks like a `|WxH` sizing hint, but iframe also
    // supports free-text titles.
    if let Some(alias) = embed.alias {
        match kind {
            SynthKind::Video => {
                // Video alias supports a bare `loop` keyword (case-insensitive)
                // plus an optional `WxH` sizing hint in any order.
                // `![[clip.mp4|loop]]` → params["loop"]="1", no sizing.
                // `![[clip.mp4|640x360 loop]]` → sizing AND loop.
                // `![[clip.mp4|640x360]]` → sizing only (backward-compat).
                //
                // Strategy: tokenise on whitespace, extract the `loop` token,
                // then run Sizing::parse on the remaining tokens joined by a
                // space so multi-token sizing like "640x360" keeps working.
                let mut tokens: Vec<&str> = alias.split_whitespace().collect();
                let loop_pos = tokens
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case("loop"));
                if let Some(pos) = loop_pos {
                    tokens.remove(pos);
                    params.insert("loop", "1");
                }
                let remainder = tokens.join(" ");
                if !remainder.is_empty() {
                    match Sizing::parse(&remainder) {
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
            SynthKind::Pdf | SynthKind::Model => match Sizing::parse(alias) {
                Some(Sizing::Width(w)) => {
                    params.insert("width", w.to_css());
                }
                Some(Sizing::Box(w, h)) => {
                    params.insert("width", w.to_css());
                    params.insert("height", h.to_css());
                }
                None => {}
            },
            SynthKind::Iframe => match Sizing::parse(alias) {
                Some(Sizing::Width(w)) => {
                    params.insert("width", w.to_css());
                }
                Some(Sizing::Box(w, h)) => {
                    params.insert("width", w.to_css());
                    params.insert("height", h.to_css());
                }
                None => {
                    // Non-sizing alias text → iframe accessible name.
                    params.insert("title", alias);
                }
            },
            SynthKind::Audio => {
                // Audio synthesizer reads no alias-derived params today
                // (controls / preload defaults are unconditional). Leave
                // params untouched.
            }
        }
    }

    // Author-typed K=V params win over alias-derived values (every-token
    // rule already validated by `parse_pothole_params`).
    if let PotholeContent::Params(p) = pothole {
        for (k, v) in &p.params {
            params.insert(k.clone(), v.clone());
        }
    }

    params
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "wikilink_dispatch_tests.rs"]
mod tests;
