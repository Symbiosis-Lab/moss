//! Image HTML synthesizer — the single entry point for emitting `<img>` /
//! `<picture>` markup in moss output.
//!
//! See [`docs/reference/structural-html-emission.md`](../../../../docs/reference/structural-html-emission.md)
//! for the architectural principle: structural HTML decisions are made at the
//! typed-data layer (pulldown-cmark events, shortcode AST, typed component
//! props), with all three call sites converging on the function in this
//! module. Regex post-passes are reserved for non-markdown-origin attribute
//! injection only.
//!
//! # Migration state (post-Step-7, 2026-05-16)
//!
//! Steps 1-7 of the structural-html-emission migration are complete:
//! - Step 1: extracted `synthesize_image_html`
//! - Step 2: routed markdown `Tag::Image` events
//! - Step 3: routed `:::hero` shortcode image
//! - Step 4: routed link-preview favicon
//! - Step 6: routed every cover image path (folder cards, child summary
//!   cards, folder index hero, photo/video gallery thumbnails) through
//!   `render_cover_html`
//! - Step 7: retired `wrap_img_in_picture` (the structural part of the
//!   legacy regex post-pass). The synthesizer now owns every `<picture>`
//!   wrap in moss output. `add_image_placeholder_attributes` survives as
//!   the attribute-injection seam for the documented carve-outs (see
//!   below).
//!
//! The byte-shape contract is captured by snapshot tests at the bottom of
//! this file. They are the line of defense against accidental output
//! drift; any future change to attribute order, quoting, or whitespace
//! must update them deliberately.
//!
//! Later steps will:
//! - Step 8: switch `MarkdownStandalone` to a `<figure class="moss-image">`
//!   wrapper (breaking change for user themes — staged separately)
//! - Add AVIF `<source>` lines once the image pipeline produces AVIF
//! - Drop the inline LQIP `style=` in favor of a wrapper CSS custom prop
//!
//! # Step-8 contract: synthesizer owns the outer `<figure>` (planned)
//!
//! `transform_events` currently wraps the synthesizer's `MarkdownStandalone`
//! output in its own `<figure>` for the three caption-pattern branches
//! (image+emphasis, separate-emphasis, implicit-figure). After Step 8 the
//! synthesizer emits `<figure class="moss-image">` itself; if `transform_events`
//! still wraps, the output will be `<figure><figure class="moss-image">...</figure>
//! <figcaption>...</figcaption></figure>` — invalid double-wrap.
//!
//! The Step-8 contract: caption flows into the synthesizer via
//! `MarkdownStandalone { caption: Option<&str> }` (or a richer
//! `CaptionMarkdown` for emphasis-in-caption support), and the three
//! caption-pattern branches collapse into a single `Event::Html(
//! synthesize_image_html(..., MarkdownStandalone { caption }))` emission.
//! The `<figcaption>` becomes the synthesizer's responsibility, NOT
//! `transform_events`. Captures the spec at
//! `docs/reference/structural-html-emission.md#output-shape`.
//!
//! # Carve-outs: bare `<img>` emitters not routed through the synthesizer
//!
//! Four emission paths land bare `<img>` HTML in the output stream that
//! does NOT flow through `synthesize_image_html`. They rely on the regex
//! post-pass (`build/media/placeholder.rs::add_image_placeholder_attributes`)
//! for attribute injection (dims/loading/decoding/LQIP). None of them
//! are bugs — each has a documented architectural reason to stay outside
//! the synthesizer:
//!
//! - **Site logo** (`build/components/nav.rs::render_logo`) — themed UI
//!   affordance, not content. The logo has its own CSS sizing
//!   (`.site-logo { height: 1.8em }`) and does not participate in
//!   LQIP/dims/WebP-variant rendering.
//! - **RSS read-tracking pixel** (`build/feeds/rss.rs`) — 1×1 `<img>` not
//!   rendered visibly; the synthesizer's dims fallback (800×600) and LQIP
//!   would be wrong for this case.
//! - **Email body images** (`infra/newsletter.rs`) — email clients (Gmail,
//!   Outlook, Apple Mail) do not consistently support `<picture>` or
//!   `data-placeholder-src`-driven hydration. Keep flat for cross-client
//!   degradation.
//! - **Raw HTML `<img>` in markdown source** — author-written
//!   `<img src="...">` literally embedded in `.md` files. pulldown-cmark
//!   emits these as `Event::Html` opaque pass-through, so they never reach
//!   `Tag::Image` and are not a moss-controlled emitter. Treated as user
//!   input, the markdown HTML is opaque to the synthesizer and gets only
//!   the additive attribute injection pass.
//!
//! Photography/video gallery thumbnails — previously a carve-out — were
//! folded into the synthesizer in Step 7's commit. The **review colophon
//! cover** (`build/features/review.rs::render_colophon`) — also previously
//! a carve-out — was folded in 2026-05-16. Both use
//! `ImageContext::FolderCardCover` (container-bounded thumbnail semantics).
//!
//! These four remaining carve-outs are flagged here so future maintenance
//! does not drop their attribute injection. Step 7 retired the structural
//! part of the regex (`wrap_img_in_picture`); the surviving
//! `add_image_placeholder_attributes` provides additive attrs only for
//! these bare-img paths.

use crate::asset_paths::{
    deployed_width, is_ladder_source_ext, is_webp_source_ext, ladder_rungs, to_webp, to_webp_rung,
};
use crate::asset_snapshot::{AssetSnapshot, FALLBACK_HEIGHT, FALLBACK_WIDTH};
use crate::contract::sizes as ctx_sizes;
// Same XML-safe escaping used everywhere else in moss for attribute values.
use crate::media::html_escape;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Where the image lives in the document, which determines the wrapper
/// element and attribute set.
///
/// Step-1 implementation supports only `MarkdownInline` (the default emission
/// shape from pulldown-cmark's serializer plus the legacy regex pair's added
/// attributes). Other variants are scaffolded so the call sites in Steps 3-6
/// can pass them without breaking the byte-shape contract; the synthesizer
/// produces the same output for all variants until the wrapper-change step.
///
/// Step 8 (2026-05-17) made these contexts diverge structurally:
/// - `MarkdownStandalone { caption }` → `<figure class="moss-image">…[<figcaption>]…</figure>` wrapper
/// - `MarkdownInline` → bare `<img>` (or `<picture><img></picture>`)
/// - `Hero` → bare `<img>` (the hero shortcode wraps with `<header>`)
/// - `FolderCardCover` → bare `<img>` (`.moss-card-cover > ` wraps)
/// - `LinkPreview` → bare `<img>` (link-preview anchor wraps)
/// - `Favicon` → bare 16×16 `<img>` with no `<picture>`, no LQIP
///
/// Not `Copy` (the embedded `&str` caption would force a lifetime on
/// every consumer); cloning is cheap (borrow) and the call sites pass by
/// value through the synthesizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageContext<'a> {
    /// Image-only paragraph in markdown — emits `<figure class="moss-image">`
    /// around the synthesized `<picture>`/`<img>` output. Used by
    /// `transform_events`'s three caption-pattern branches:
    ///
    /// - **image+emphasis**: `![alt](src) *caption*` — `caption = Some(emphasis_text)`
    /// - **separate-emphasis**: image-only paragraph followed by emphasis-
    ///   only paragraph — `caption = Some(emphasis_text)`
    /// - **implicit figure**: image-only paragraph with non-empty alt and
    ///   `[site].implicit_figure = true` — `caption = Some(alt_text)`
    ///
    /// `caption = None` means "figure wrap but no `<figcaption>`" (reserved
    /// for future callers that want the wrapper structurally without prose).
    ///
    /// `width = Some("body|wide|page|screen")` emits `data-width="..."` on
    /// the outer `<figure>` element per spec § P9. `None` omits the
    /// attribute entirely so themes can target the absence via
    /// `:not([data-width])`. `full` aliases to `screen` upstream — values
    /// reaching this struct are already in canonical value-space.
    MarkdownStandalone {
        caption: Option<&'a str>,
        width: Option<&'a str>,
        /// Editorial runaround alignment, surfaced as a CSS class on the
        /// outer `<figure>`. Phase 1 C1 (2026-05-25): Stage 2 dispatcher
        /// passes `Some("moss-align-left")` / `Some("moss-align-right")`
        /// when the markdown image's title carries `moss:align=left|right`.
        /// `None` omits the class.
        align: Option<&'a str>,
        /// Arbitrary CSS class names from `moss:classes="foo bar"` title
        /// params (Phase 1 C1). Each entry is appended to the figure's
        /// `class="moss-image …"` attribute, space-separated. Empty slice
        /// leaves the class list at its default (`moss-image`).
        class_names: &'a [String],
        /// Arbitrary `key="value"` HTML attributes from leftover
        /// `moss:` title params (Phase 1 C1). The dispatcher passes
        /// every param that isn't a known field (kind/width/align/classes)
        /// through here so future params propagate without code changes.
        /// Keys are emitted in BTreeMap order for stable byte shape.
        /// Empty map emits no extra attributes.
        extra_attrs: &'a BTreeMap<String, String>,
    },
    /// Image inside prose, a list, a table cell, or a callout body.
    /// Always emits bare `<img>` or `<picture><img></picture>`.
    MarkdownInline,
    /// `:::hero` shortcode body image. The hero wrapper handles layout.
    Hero,
    /// Folder-card cover or child-summary cover image.
    FolderCardCover,
    /// External-link preview thumbnail image.
    LinkPreview,
    /// Favicon for a link-preview card. Bare 16×16 `<img>`, no `<picture>`,
    /// no LQIP, no responsive variants.
    Favicon,

    /// Phase 2 scaffold (filled by Phase 2B agent): site-header / nav logo.
    /// Bare `<img>` with `class="site-logo"`, no LQIP, no `<picture>`, no
    /// `loading="lazy"` (logo is above-the-fold). CSS handles sizing
    /// (`.site-logo { height: 1.8em }`).
    SiteLogo,
    /// Phase 2 scaffold (filled by Phase 2C agent): RSS read-tracking pixel.
    /// 1×1 invisible `<img>`. No `loading="lazy"` (the pixel must fire on
    /// read for tracking). No LQIP, no `<picture>`.
    TrackingPixel,
    /// Phase 2 scaffold (filled by Phase 2D agent): newsletter email body
    /// image. Email-client-safe HTML subset: no `<picture>`, no `data-*`,
    /// inline `style="display:block;max-width:100%;height:auto"` for
    /// responsive email layouts. Explicit dims when known (callers pass
    /// `Option<u32>` for width/height; missing → omit per email-client
    /// tolerance).
    EmailBody {
        width: Option<u32>,
        height: Option<u32>,
    },

    /// Phase 2E v5 PR3 (2026-05-26): `:::gallery` body image. Below-the-
    /// fold thumbnail (`loading="lazy"`); same `<picture>`/dims/LQIP byte
    /// shape as `MarkdownInline`. The outer `.moss-gallery-item` wrapper
    /// is owned by the gallery shortcode's `DefaultHooks` impl in
    /// `crates/moss-core/src/ast/hooks.rs`; this variant emits just the
    /// inner `<picture><img></picture>` / `<img>` shape so the wrapper
    /// can sit around it.
    ///
    /// Distinct from `MarkdownInline` so the gallery's per-item style
    /// passthrough (object-position from `MediaAttrs`) has a typed home
    /// to evolve into; today both contexts produce the same inner byte
    /// shape via `synthesize_inner`.
    GalleryThumb,

    /// Phase 2E PR2 (2026-05-26): bare `<img>` emission with no `<picture>`
    /// wrap, no `<source>`, no LQIP, no width/height, no `loading` attr.
    /// Used by the hero typed-renderer fallback path
    /// (`typed_renderers.rs::render_hero_html_typed`) when no manifest
    /// (`MediaDimensionLookup`) is in scope — i.e. test / fragment-render
    /// paths where `AssetRegistry::set_pending` has NOT been called for
    /// the source's `.webp` companion.
    ///
    /// The asset-publish invariant (see `.claude/CLAUDE.md` § "Asset
    /// publish invariant") requires us to NEVER emit a
    /// `<source srcset="*.webp">` for an unregistered variant — the
    /// preview server cannot return placeholder bytes for an URL that
    /// AssetRegistry doesn't know about, and `<picture>` does not recover
    /// from a chosen-source 404. `HeroBare` is the explicit opt-out for
    /// code paths that run before set_pending.
    ///
    /// The byte shape mirrors the pre-Phase-2E fallback exactly:
    /// `<img src="X" alt="Y"[ STYLE] />` where `STYLE` is the inline
    /// style fragment (already escaped) threaded through
    /// [`ImageRenderOptions::extra_attrs`] by the caller. The `class` and
    /// `eager` options are ignored — the hero `<header>` wraps and CSS
    /// handles loading priority.
    HeroBare,
}

/// Optional rendering attributes a caller may pass.
///
/// The defaults preserve the current regex-pass byte shape:
/// - `loading="lazy"` unless `eager=true` (which switches to `loading="eager"
///   fetchpriority="high"`)
/// - No extra inline style
/// - No extra CSS classes on the inner `<img>`
#[derive(Debug, Default, Clone)]
pub struct ImageRenderOptions<'a> {
    /// Above-the-fold loading hint. When true, emits
    /// `loading="eager" fetchpriority="high"` instead of `loading="lazy"`.
    pub eager: bool,
    /// CSS classes to add to the inner `<img>`. Used by link-preview
    /// favicons (`link-preview-favicon`) and similar UI affordances.
    pub class: Option<&'a str>,
    /// Raw extra HTML attribute fragment appended after the standard
    /// attribute block (e.g., `style="object-fit:cover;object-position:50% 50%"`
    /// for `:::hero` covers carrying `MediaAttrs`). The caller is responsible
    /// for HTML-escaping values inside this fragment.
    pub extra_attrs: Option<&'a str>,
    /// Explicit `sizes=` override for the srcset ladder. When `Some`, wins
    /// over the [`ImageContext`]-derived default — used by callers that know
    /// the rendered slot better than the context does: a figure carrying a
    /// `data-width` token ([`crate::contract::sizes::sizes_for_data_width`])
    /// or an image inside a `.moss-grid` cell
    /// ([`crate::contract::sizes::sizes_for_grid_cell`]).
    pub sizes: Option<&'a str>,
}

/// Synthesize the HTML for an image reference.
///
/// `src` is the resolved URL as it should appear in `<img src=>` (already
/// passed through the link resolver and dir_overrides for CJK sites).
///
/// `alt` is the accessible name. Empty string is permitted for decorative
/// images; callers in WCAG-sensitive contexts should pass meaningful text.
///
/// `assets` is the [`AssetSnapshot`] holding pre-fetched per-path dimensions,
/// LQIP data URIs, dominant colors, and registered variant kinds (WebP/AVIF).
/// Phase 1 of the unified-image-emission migration (2026-05-25) replaced the
/// prior `Option<&MediaDimensionLookup>` parameter with this typed contract —
/// `MediaDimensionLookup` still populates the snapshot in `pipeline.rs`'s
/// `build_asset_snapshot` boundary, but the synthesizer no longer probes it
/// directly. Callers that don't have a populated snapshot (test/fragment-
/// render paths) pass `&AssetSnapshot::new()`; the synthesizer then emits
/// fallback dims (800×600) and no LQIP/color style.
///
/// `context` and `options` describe the call site. `Favicon` short-circuits
/// to a 16×16 bare `<img>` (no manifest, no LQIP, no `<picture>`).
///
/// Byte-shape contract (preserved through Phase 1's data-source switch):
///
/// - With no `<picture>` wrap (non-raster):
///   `<img src="X" width="W" height="H" loading="lazy" style="…" alt="Y" />`
/// - Raster originals (png/jpg/jpeg):
///   `<picture><source srcset="X.webp" type="image/webp"><img src="X" width="W" height="H" loading="lazy" style="…" alt="Y" /></picture>`
/// - Inline style is `background-image:url(LQIP);background-size:cover` when
///   the snapshot has LQIP; `background-color:#RRGGBB` when only dominant
///   color is available; absent when neither.
/// - For `eager: true`: `loading="eager" fetchpriority="high"` replaces
///   `loading="lazy"`.
pub fn synthesize_image_html(
    src: &str,
    alt: &str,
    assets: &AssetSnapshot,
    context: ImageContext<'_>,
    options: &ImageRenderOptions<'_>,
) -> String {
    // Favicon short-circuit: hardcoded 16×16, no snapshot lookup, no <picture>.
    // Matches the current emission shape in
    // `build/markdown/typed_renderers.rs::render_link_preview`. `assets` is
    // intentionally unused — favicons are UI affordances that never
    // participate in the variant manifest.
    if matches!(context, ImageContext::Favicon) {
        let class_attr = options
            .class
            .map(|c| format!(r#" class="{}""#, html_escape(c)))
            .unwrap_or_default();
        return format!(
            r#"<img{} src="{}" width="16" height="16" alt="{}">"#,
            class_attr,
            html_escape(src),
            html_escape(alt),
        );
    }

    // Phase 2 scaffold (filled by Phase 2 carve-out agents): the three former
    // bare-<img> carve-outs become first-class synthesizer contexts. Each
    // short-circuits before synthesize_inner (which assumes the standard
    // <picture>/LQIP/dims pipeline that's wrong for these elements).
    // Site-logo short-circuit (Phase 2B carve-out): bare `<img>` with
    // `class="site-logo"`, no `<picture>`, no LQIP, no `loading="lazy"` —
    // the logo is above-the-fold; CSS handles sizing
    // (`.site-logo { height: 1.8em }`). Attribute order
    // (`class`, `src`, `alt`, `aria-hidden`) preserves the pre-Phase-2
    // byte shape emitted by `nav.rs::generate_navigation`.
    if matches!(context, ImageContext::SiteLogo) {
        // `extra_attrs` rides directly after `class` (the editor preview's
        // `data-source-fm="logo"` annotation); `None` keeps the byte shape.
        let extra = options
            .extra_attrs
            .map(|a| format!(" {a}"))
            .unwrap_or_default();
        return format!(
            r#"<img class="site-logo"{} src="{}" alt="{}" aria-hidden="true">"#,
            extra,
            html_escape(src),
            html_escape(alt),
        );
    }
    if matches!(context, ImageContext::TrackingPixel) {
        // Phase 2C: RSS read-tracking pixel. 1×1 invisible <img>.
        // NO loading="lazy" — the pixel must fire on read for tracking.
        // NO LQIP, NO <picture>, NO alt text (empty alt for invisible
        // decoration). Self-closing form because this commonly lands in
        // RSS feed XML (CDATA-wrapped <description>).
        return format!(
            r#"<img src="{}" alt="" width="1" height="1" />"#,
            html_escape(src),
        );
    }
    if matches!(context, ImageContext::HeroBare) {
        // Phase 2E PR2 (2026-05-26): no-snapshot hero fallback. Byte
        // shape matches the pre-PR2 emission at
        // `typed_renderers.rs::render_hero_html_typed` lines 554-557:
        // `<img src="X" alt="Y"[ STYLE] />`. `assets` is intentionally
        // unused (the caller passes an empty snapshot via the no-
        // manifest branch); the snapshot is part of the signature only
        // for symmetry with the other contexts. `options.class` and
        // `options.eager` are ignored — hero chrome (CSS / `<header>`)
        // handles styling and loading priority.
        //
        // The inline `style=` fragment that the legacy fallback baked
        // directly into `format!` is now threaded through
        // `options.extra_attrs` (the caller pre-escapes the value and
        // omits the leading space, matching the existing extra_attrs
        // contract — the synthesizer prepends a single space).
        let extra = options
            .extra_attrs
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        return format!(
            r#"<img src="{}" alt="{}"{} />"#,
            html_escape(src),
            html_escape(alt),
            extra,
        );
    }
    if let ImageContext::EmailBody { width, height } = context {
        // Phase 2D (2026-05-25): email-client-safe <img> for newsletter body
        // images. Email clients do NOT support <picture>, do NOT support
        // data-* attributes, and frequently strip <style> blocks — inline
        // `style=` on <img> plus explicit width/height attrs is the
        // cross-client minimum for responsive layouts. width/height are
        // Option<u32>: omitted when unknown (e.g. remote URLs that aren't in
        // AssetSnapshot). Email clients tolerate missing dims at the cost of
        // a small layout shift.
        let mut dims = String::new();
        if let Some(w) = width {
            dims.push_str(&format!(r#" width="{}""#, w));
        }
        if let Some(h) = height {
            dims.push_str(&format!(r#" height="{}""#, h));
        }
        return format!(
            r#"<img src="{}" alt="{}"{} style="display:block;max-width:100%;height:auto;" />"#,
            html_escape(src),
            html_escape(alt),
            dims,
        );
    }

    // Step 8: the inner `<img>` / `<picture>` is shape-equivalent across
    // every non-favicon context. The wrapping `<figure class="moss-image">`
    // and optional `<figcaption>` are the only context-dependent
    // structure. Compute the inner first, then wrap if requested.
    //
    // The context decides the `sizes=` value for the srcset ladder
    // (responsive-image-variants Task 3): full-bleed surfaces (hero,
    // `data-width="screen|full"` figures) span the viewport; wide/page
    // figures span their escape band (ADR-021 Corollary 2, the data-width
    // CSS in site.css); cards/gallery thumbs occupy grid cells; everything
    // else renders in the content column. `options.sizes` overrides all of
    // it — the caller (figure renderer with a data-width token, grid cell)
    // knows the slot better than the context does. Only emitted when the
    // ladder is non-empty — see synthesize_inner.
    let sizes_value: &str = match options.sizes {
        Some(s) => s,
        None => match &context {
            ImageContext::Hero => ctx_sizes::SIZES_FULL_BLEED,
            ImageContext::MarkdownStandalone { width: Some(w), .. } => {
                ctx_sizes::sizes_for_data_width(w).unwrap_or(ctx_sizes::SIZES_BODY)
            }
            ImageContext::FolderCardCover | ImageContext::LinkPreview => ctx_sizes::SIZES_CARD,
            ImageContext::GalleryThumb => ctx_sizes::SIZES_GALLERY,
            _ => ctx_sizes::SIZES_BODY,
        },
    };
    let inner = synthesize_inner(src, alt, assets, options, sizes_value);

    match context {
        ImageContext::MarkdownStandalone {
            caption,
            width,
            align,
            class_names,
            extra_attrs,
        } => wrap_in_figure_full(&inner, caption, width, align, class_names, extra_attrs),
        // All other variants are "no outer wrapper" — caller-owned chrome
        // (hero `<header>`, folder card container, link preview anchor)
        // surrounds the bare img/picture output.
        _ => inner,
    }
}

/// Wrap an already-shaped image fragment in the standalone-image
/// figure container.
///
/// `inner_html` is either:
/// - The output of `synthesize_inner` (markdown `Tag::Image` flowing
///   through `synthesize_image_html`), or
/// - A resolve-phase `<img …>` / `<video …>` HTML string emitted by
///   moss-core's wikilink-lowering (`![[file|display-params]]`).
///
/// Output shape:
///
/// - `caption = Some(text)`, `width = None`:
///   ```html
///   <figure class="moss-image"><picture>…<img …></picture>
///   <figcaption>text</figcaption></figure>
///   ```
/// - `caption = None`, `width = Some("screen")`:
///   ```html
///   <figure class="moss-image" data-width="screen"><picture>…<img …></picture></figure>
///   ```
/// - `width = None` omits the attribute entirely so themes can target
///   the absence via `:not([data-width])`. Per spec § P9, `data-width`
///   sits on the wrapper element (here, `<figure>`) rather than the
///   inner `<img>`.
///
/// Caption text is HTML-escaped at the boundary. Future work: allow
/// markdown formatting inside the caption via an explicit
/// `CaptionMarkdown` variant on `ImageContext`.
///
/// `pub(super)` so `build/markdown/pipeline.rs` can call this directly
/// for the raw-HTML media branch of `emit_standalone_figure_image`
/// without duplicating the wrapper byte shape. The synthesizer is the
/// single source of truth for `<figure class="moss-image">` — when the
/// wrapper class evolves (e.g. `moss-image moss-image--auto` per the
/// Step-8 spec), only this function changes.
pub fn wrap_in_figure(
    inner_html: &str,
    caption: Option<&str>,
    width: Option<&str>,
) -> String {
    // 3-arg shorthand kept for the raw-HTML media branch in
    // pipeline.rs::emit_standalone_figure_image (wikilink display-keyword
    // images that don't carry moss: title params — no align / extra
    // classes / extra attrs). Delegates to the canonical wrapper so the
    // byte shape stays defined in exactly one place.
    let empty_classes: &[String] = &[];
    let empty_attrs: BTreeMap<String, String> = BTreeMap::new();
    wrap_in_figure_full(inner_html, caption, width, None, empty_classes, &empty_attrs)
}

/// Canonical `<figure>`-wrapping function consumed by both `synthesize_image_html`
/// for `MarkdownStandalone` and the 3-arg compatibility shim `wrap_in_figure`.
///
/// Class list assembly: `class="moss-image{ align_class?}{ class_names…}"`.
/// Extra attrs render as `key="escaped_value"` in BTreeMap order, after
/// `data-width=` and before the inner content.
pub(super) fn wrap_in_figure_full(
    inner_html: &str,
    caption: Option<&str>,
    width: Option<&str>,
    align: Option<&str>,
    class_names: &[String],
    extra_attrs: &BTreeMap<String, String>,
) -> String {
    // `width` here is a closed-set &'static str from `match_width_token`
    // ("body" | "wide" | "page" | "screen"). The `html_escape` call is
    // defensive belt-and-braces — it never actually substitutes — and is
    // kept for symmetry with the embed-renderer side's `html_escape_attr`.
    let width_attr = width
        .map(|w| format!(r#" data-width="{}""#, html_escape(w)))
        .unwrap_or_default();

    // Compose the class attribute: `moss-image` first (the structural
    // hook), then the optional align class, then any author-supplied
    // class names. Single space separator keeps the byte shape stable
    // across the empty / align-only / class-only / both permutations.
    let mut class_value = String::from("moss-image");
    if let Some(a) = align {
        class_value.push(' ');
        class_value.push_str(a);
    }
    for cn in class_names {
        if cn.is_empty() {
            continue;
        }
        class_value.push(' ');
        class_value.push_str(cn);
    }
    let class_attr = format!(r#" class="{}""#, html_escape(&class_value));

    // Extra attrs are emitted in BTreeMap order (deterministic byte shape).
    let mut extra = String::new();
    for (k, v) in extra_attrs {
        extra.push(' ');
        extra.push_str(k);
        extra.push_str(r#"=""#);
        extra.push_str(&html_escape(v));
        extra.push('"');
    }

    match caption {
        Some(text) => format!(
            r#"<figure{class}{w}{extra}>{inner}<figcaption>{cap}</figcaption></figure>"#,
            class = class_attr,
            w = width_attr,
            extra = extra,
            inner = inner_html,
            cap = html_escape(text),
        ),
        None => format!(
            r#"<figure{class}{w}{extra}>{inner}</figure>"#,
            class = class_attr,
            w = width_attr,
            extra = extra,
            inner = inner_html,
        ),
    }
}

/// Synthesize the inner `<img>` (or `<picture><img></picture>`) without
/// the standalone-figure wrapper. Shared by every non-favicon context.
///
/// `sizes_value` is the context-resolved `sizes=` attribute value
/// (`contract::sizes`); it is only emitted when the source is wide enough
/// to have ladder rungs — narrow/unknown-dims sources keep the legacy
/// single-URL `<source>` byte shape.
fn synthesize_inner(
    src: &str,
    alt: &str,
    assets: &AssetSnapshot,
    options: &ImageRenderOptions<'_>,
    sizes_value: &str,
) -> String {
    // Phase B (Task 12): a webp SOURCE is already webp — to_webp(src) == src, so
    // a `<picture><source srcset=to_webp(src)>` would emit a `<source>` byte-
    // identical to the inner `<img>` (pointless). Instead, emit the responsive
    // ladder DIRECTLY on the `<img>` via `srcset`+`sizes`. This branch MUST run
    // BEFORE `is_raster_original` — which now also matches webp (webp joined
    // `is_ladder_source_ext` in Phase B) — so webp never falls into the
    // `<picture>` conversion path below.
    //
    // Animated webp gets NO ladder: `assets.is_animated(src)` (scan-derived,
    // Task 9/10) → empty ladder → the base `<img>` is byte-identical to today's
    // bare webp emission. Small / unknown-dims webp is likewise byte-identical.
    // This is the ONLY census site that passes a non-`false` animated flag; the
    // pipeline sites keep `false` (canonical rationale + the EXIF-orientation
    // agreement live on `asset_paths::ladder_rungs`). base_url == src: the served
    // base webp IS the source (`to_webp(src) == src`).
    if is_webp_source(src) {
        return match resolve_ladder(assets, src, lookup_animated(assets, src)) {
            None => render_img_tag(src, alt, assets, options, None),
            Some((rungs, base_w)) => {
                let srcset = build_srcset(src, src, rungs, base_w);
                render_img_tag(src, alt, assets, options, Some((&srcset, sizes_value)))
            }
        };
    }

    let img_tag = render_img_tag(src, alt, assets, options, None);

    // For raster originals, always emit <picture><source srcset=X.webp>.
    // This markup is MODE-INDEPENDENT — the on-disk HTML is identical in
    // preview and publish. The webp is encoded in the BACKGROUND for ALL modes
    // (blocking.rs registers the variant Pending; a BackgroundHandle worker
    // runs the encode). Two mechanisms keep the webp URL live without a 404:
    //   • Preview: the server serves the FULL ORIGINAL source bytes (source
    //     passthrough, preview/server/router.rs) for the not-yet-encoded
    //     variant URL, so the first paint is sharp; a Failed encode instead
    //     surfaces a warning SVG (preview/server/placeholder.rs).
    //   • Publish: the seal/persist task AWAITS the background drain barrier
    //     before sealing, so the sealed/deployed generation always contains the
    //     encoded .webp on disk (ADR-013 by construction).
    // So the URL is always live in both modes.
    //
    // We must never emit a <source> that might 404 because a chosen <source>
    // 404 is non-recoverable inside <picture> per HTML spec §
    // "update-the-source-set" + § "update-the-image-data": browser commits to
    // the chosen URL, fetch fails, image state goes to broken, error fires —
    // browser does NOT walk back to the inner <img>.
    //
    // For non-raster sources (svg, favicons via Favicon context), no variant
    // exists; emit the bare <img>.
    //
    // Pattern: explicit promise model. See
    // docs/archive/2026-05-20-image-variant-honest-mirror.md (Layer 3).
    if is_raster_original(src) {
        // to_webp(src) inherits the dir_overrides + relative-prefix already
        // applied to `src` by the upstream renderer. Swapping the extension
        // on `src` keeps the emitted URL aligned with the AssetRegistry's
        // registered key (blocking.rs's set_pending uses the same to_webp
        // derivation). It is the `<picture>` base descriptor URL here.
        let srcset_path = to_webp(src);
        // `false`: png/jpg/jpeg are never animated through this path (animated
        // gif/webp never reach `is_raster_original`; APNG is flattened by the
        // base+rung encodes alike). Canonical agreement rationale + the
        // EXIF-orientation agreement: `asset_paths::ladder_rungs` census doc.
        // `resolve_ladder` is `Some` only when rungs exist; unknown dims →
        // `None` → the legacy single-URL `<source>` shape.
        match resolve_ladder(assets, src, false) {
            None => format!(
                r#"<picture><source srcset="{}" type="image/webp">{}</picture>"#,
                html_escape(&encode_srcset_url(&srcset_path)),
                img_tag,
            ),
            Some((rungs, base_w)) => {
                let srcset = build_srcset(src, &srcset_path, rungs, base_w);
                format!(
                    r#"<picture><source srcset="{}" type="image/webp" sizes="{}">{}</picture>"#,
                    html_escape(&srcset),
                    html_escape(sizes_value),
                    img_tag,
                )
            }
        }
    } else {
        img_tag
    }
}

/// Returns true when `src` is a raster original that always gets a webp
/// variant from moss's image pipeline. EMISSION side of the shared ladder
/// gate: extracts `src`'s extension and delegates membership to
/// [`is_ladder_source_ext`] — the ONE predicate the pipeline census sites
/// (registration/encode/sweep/heal) also consume.
///
/// After Phase B (Task 12) this also matches webp, so `synthesize_inner`
/// checks [`is_webp_source`] FIRST and routes webp to the `<img srcset>`
/// branch — a webp reaching THIS predicate's `<picture>` branch would emit a
/// useless `<source>` identical to the inner `<img>`. Reaching here therefore
/// means png/jpg/jpeg in practice (the webp branch already returned).
///
/// Note: this check is extension-only. `collect_images_for_conversion` applies
/// additional content-based filters (e.g. `SkipReason::NotAnImage` for files
/// whose magic bytes don't match the declared format). A file that passes
/// `is_raster_original` here but is filtered by `NotAnImage` will NOT have
/// `set_pending` called for it — the synthesizer still emits a `<picture>`, but
/// its `<source>` webp URL is unregistered, so the preview server falls through
/// to ServeDir and the variant 404s. It does NOT serve the LQIP placeholder —
/// that path is reserved for `set_pending` (Pending) entries. Benign in
/// practice: this only occurs for genuinely corrupt files (e.g. an HTML 404
/// page saved as .png) that are not referenced from content.
fn is_raster_original(src: &str) -> bool {
    src.rsplit_once('.')
        .is_some_and(|(_, ext)| is_ladder_source_ext(ext))
}

/// Returns true when `src` is a webp SOURCE (extension `.webp`, case-
/// insensitive). EMISSION side of the webp-vs-conversion split: a webp source
/// carries the responsive ladder directly on `<img srcset>` (no `<picture>`),
/// so `synthesize_inner` tests this before [`is_raster_original`]. Delegates
/// to [`is_webp_source_ext`] — the single extension gate in `asset_paths`.
fn is_webp_source(src: &str) -> bool {
    src.rsplit_once('.')
        .is_some_and(|(_, ext)| is_webp_source_ext(ext))
}

/// Try several path normalizations against `AssetSnapshot.dimensions` so the
/// synthesizer matches the same set of input forms the prior
/// `MediaDimensionLookup::get` handled. `src` may arrive as the resolved URL
/// with a leading `/` (review colophon covers, cover.rs absolute paths) or
/// with a `./` / `../` relative prefix (CJK dir_overrides); scan stores keys
/// in plain relative form. The probe order mirrors the lookup's:
///
/// 1. exact match
/// 2. leading-`/` stripped (absolute-to-relative)
/// 3. leading `./` / `../` stripped (relative normalization)
///
/// Returns `None` when none of the variants is in the snapshot. The caller
/// supplies the fallback (800×600 for dims, no style for LQIP / color).
fn lookup_dims(assets: &AssetSnapshot, src: &str) -> Option<(u32, u32)> {
    probe_paths(src, |p| assets.dims(&p))
}

/// Whether `src` is a scan-flagged animated source. Probes the SAME path
/// normalizations as [`lookup_dims`] against `AssetSnapshot.animated` (keyed
/// identically to `dimensions`, both populated per-source in
/// `build_asset_snapshot`), so a webp found for dims is found for animation
/// too. Missing everywhere → `false` (test/fragment-render paths with an empty
/// snapshot treat sources as non-animated). Only the webp ladder branch
/// consults this — png/jpg/jpeg are never animated through the `<picture>`
/// path (see `asset_paths::ladder_rungs` census doc).
fn lookup_animated(assets: &AssetSnapshot, src: &str) -> bool {
    probe_paths(src, |p| assets.animated.get(&p).copied()).unwrap_or(false)
}

/// Resolve the responsive ladder for `src`: the rung widths (strictly below the
/// deployed base) and the deployed base WIDTH — or `None` when dims are unknown
/// (snapshot miss) or the ladder is empty (small/animated source). Shared by
/// BOTH emission paths (webp `<img srcset>` and png/jpg `<picture><source>`) so
/// the `lookup_dims → ladder_rungs → is_empty → deployed_width` derivation
/// CANNOT drift between them — and it must agree with registration/encode,
/// which call the identical `ladder_rungs`/`deployed_width` (see the
/// deterministic-agreement contract on [`crate::asset_paths::ladder_rungs`],
/// including its EXIF-orientation agreement).
fn resolve_ladder(
    assets: &AssetSnapshot,
    src: &str,
    is_animated: bool,
) -> Option<(&'static [u32], u32)> {
    lookup_dims(assets, src).and_then(|(w, h)| {
        let rungs = ladder_rungs(w, h, is_animated);
        if rungs.is_empty() {
            None
        } else {
            Some((rungs, deployed_width(w, h)))
        }
    })
}

/// Assemble a `srcset` value: one `to_webp_rung(src, w) {w}w` candidate per
/// rung, then the base descriptor `{base_url} {base_w}w`. Shared by BOTH
/// emission paths so the rung-URL derivation and descriptor shape cannot drift
/// between them (and must agree with what registration/encode name). The ONLY
/// difference is `base_url`: a webp SOURCE passes `src` itself (the served base
/// IS the source — `to_webp(src) == src`); a png/jpg/jpeg source passes
/// `to_webp(src)` (the converted `<picture>` base). Rung URLs derive from `src`
/// in both cases. Caller HTML-escapes the returned value.
fn build_srcset(src: &str, base_url: &str, rungs: &[u32], base_w: u32) -> String {
    let mut parts: Vec<String> = rungs
        .iter()
        .map(|w| format!("{} {}w", encode_srcset_url(&to_webp_rung(src, *w)), w))
        .collect();
    parts.push(format!("{} {}w", encode_srcset_url(base_url), base_w));
    parts.join(", ")
}

/// Percent-encode literal commas (`,` → `%2C`) in one `srcset` candidate URL.
///
/// `srcset` is a comma-delimited candidate list, so a literal comma inside a
/// URL — which a comma-named source (`a,b.jpg`) produces, since
/// `percent_encode_path_segments` keeps `,` literal by design — would mis-split
/// the list in browsers AND in `html_post`'s `rewrite_srcset_candidates`.
/// Comma is the ONLY char touched (`src` was already encoded upstream and never
/// carries a `%2C`, so no double-encode); applied ONLY to `srcset` candidates —
/// the base `<img src>` keeps its single, unambiguous literal comma. The
/// deployed file has a literal comma on disk; a static server decodes `%2C` → `,`
/// when resolving (verified for the preview server in `router.rs`'s `%2C` test).
fn encode_srcset_url(url: &str) -> String {
    url.replace(',', "%2C")
}

fn lookup_lqip<'a>(assets: &'a AssetSnapshot, src: &str) -> Option<&'a str> {
    probe_paths(src, |p| assets.lqip(&p))
}

fn lookup_color<'a>(assets: &'a AssetSnapshot, src: &str) -> Option<&'a String> {
    probe_paths(src, |p| assets.dominant_color.get(&p))
}

fn probe_paths<T>(src: &str, mut probe: impl FnMut(PathBuf) -> Option<T>) -> Option<T> {
    if let Some(v) = probe_normalized(src, &mut probe) {
        return Some(v);
    }
    // BUG 6.2 (belt-and-suspenders): body/wikilink images arrive percent-encoded
    // (`Europe%20-%20A%20Prophecy`), but snapshot keys are the RAW source path.
    // Decode `%XX` and re-probe so the encoded URL reverses to the source key.
    // Pure + zero-I/O; on invalid/lone `%` `percent_decode` returns the input
    // unchanged, so we only re-probe when decoding actually changed something.
    let decoded = percent_decode(src);
    if decoded != src {
        if let Some(v) = probe_normalized(&decoded, &mut probe) {
            return Some(v);
        }
    }
    None
}

/// Probe `src` plus its leading-`/` and leading-`./`/`../`-stripped forms.
fn probe_normalized<T>(src: &str, probe: &mut impl FnMut(PathBuf) -> Option<T>) -> Option<T> {
    if let Some(v) = probe(PathBuf::from(src)) {
        return Some(v);
    }
    let stripped = src.strip_prefix('/').unwrap_or(src);
    if stripped != src {
        if let Some(v) = probe(PathBuf::from(stripped)) {
            return Some(v);
        }
    }
    let mut s: &str = src;
    while let Some(rest) = s.strip_prefix("./").or_else(|| s.strip_prefix("../")) {
        s = rest;
    }
    if s != src {
        if let Some(v) = probe(PathBuf::from(s)) {
            return Some(v);
        }
    }
    None
}

/// Percent-decode `%XX` byte sequences in a URL path (pure, zero-I/O).
/// The single implementation lives next to the encoder it inverts; see
/// [`crate::resolve::fuzzy_path::percent_decode_path`].
fn percent_decode(path: &str) -> String {
    crate::resolve::fuzzy_path::percent_decode_path(path)
}

/// Emit just the `<img>` tag with all attributes. Internal helper for
/// `synthesize_image_html` — exposed as `pub(crate)` only for snapshot tests
/// that want to assert against the bare img output without the optional
/// `<picture>` wrapper.
///
/// `srcset_sizes` is `Some((srcset, sizes))` ONLY for the Phase-B webp ladder
/// (Task 12), which carries the responsive candidates on the `<img>` itself
/// rather than a `<source>`. When `Some`, ` srcset="…" sizes="…"` is emitted
/// immediately after `src=` (both values HTML-escaped, matching the
/// `<picture>` path's escaping). When `None` — every other caller and every
/// non-laddered webp — the output is BYTE-IDENTICAL to the pre-Task-12 shape.
pub(crate) fn render_img_tag(
    src: &str,
    alt: &str,
    assets: &AssetSnapshot,
    options: &ImageRenderOptions<'_>,
    srcset_sizes: Option<(&str, &str)>,
) -> String {
    // AssetSnapshot's `dims` is keyed by PathBuf; the src arrives as the
    // resolved URL the upstream renderer baked (potentially absolute, e.g.
    // `/image/cover.jpg`). Scan stores relative keys (`image/cover.jpg`),
    // so the synthesizer probes both forms via `lookup_dims`. Snapshot
    // lookups absent → fall back to the legacy 800×600 (matches the prior
    // `MediaDimensionLookup::get` semantics before the Phase 1 B1 migration).
    // Stem fallback (extension-mismatch — e.g. `.mov` vs `.mp4`) was
    // previously handled in MediaDimensionLookup::get; since AssetSnapshot
    // exposes only exact-path access, that case will need follow-up if
    // production sites rely on it (likely only for video posters, which
    // are out of this Tag::Image path).
    let (width, height) = lookup_dims(assets, src).unwrap_or((FALLBACK_WIDTH, FALLBACK_HEIGHT));

    let class_attr = options
        .class
        .map(|c| format!(r#" class="{}""#, html_escape(c)))
        .unwrap_or_default();

    let (loading_attr, fetchpriority_attr) = if options.eager {
        (r#" loading="eager""#, r#" fetchpriority="high""#)
    } else {
        (r#" loading="lazy""#, "")
    };

    // Suppress LQIP / dominant-color style when extra_attrs already carries
    // a style= attribute (e.g., `:::hero {attrs="cover-fit=contain"}` passes
    // `style="object-fit:contain"` through extra_attrs). The browser would
    // honor the LAST style= it sees and drop the LQIP, so emitting both
    // produces malformed HTML and loses the placeholder. The legacy regex
    // pass (`placeholder.rs:413-422`) had the same has_style guard; this
    // preserves parity. Future work: merge the two declarations into a
    // single style= via a typed `ImageRenderOptions::media_attrs` field so
    // the synthesizer owns escaping end-to-end (impl-review item 9).
    let extra_has_style = options
        .extra_attrs
        .map(|s| s.contains("style="))
        .unwrap_or(false);

    let style_attr = if extra_has_style {
        String::new()
    } else if let Some(lqip) = lookup_lqip(assets, src) {
        format!(
            r#" style="background-image:url({});background-size:cover""#,
            lqip
        )
    } else if let Some(color) = lookup_color(assets, src) {
        format!(r#" style="background-color:{}""#, color)
    } else {
        String::new()
    };

    let extra = options
        .extra_attrs
        .map(|s| format!(" {}", s))
        .unwrap_or_default();

    // Phase B webp ladder (Task 12): responsive candidates ride the `<img>`
    // itself. Emitted right after `src=` and before `width=`. Empty for every
    // other caller, keeping the byte shape identical to the pre-Task-12 tag.
    let srcset_attr = match srcset_sizes {
        Some((srcset, sizes)) => format!(
            r#" srcset="{}" sizes="{}""#,
            html_escape(srcset),
            html_escape(sizes),
        ),
        None => String::new(),
    };

    // `data-placeholder-src` removed 2026-05-20: the iframe-bridge handler
    // now matches by URL substring against `src` / `srcset` (see
    // frontend/bridge/iframe-bridge.ts, moss-asset-ready branch). The
    // AssetRegistry's promise model + the preview server's URL-keyed lookup
    // make the attribute redundant. See
    // docs/archive/2026-05-20-image-variant-honest-mirror.md (Layer 3).
    //
    // Inline LQIP via `background-image: url(data:image/jpeg;base64,…)` is
    // kept — legitimate production technique (cf. Vercel `blurDataURL`,
    // nextjs.org/docs/app/api-reference/components/image). Shows a blurred
    // preview instantly while the actual bytes are being decoded.
    format!(
        r#"<img{class_attr} src="{src_esc}"{srcset} width="{w}" height="{h}"{loading}{fetch}{style} alt="{alt}"{extra} />"#,
        class_attr = class_attr,
        src_esc = html_escape(src),
        srcset = srcset_attr,
        w = width,
        h = height,
        loading = loading_attr,
        fetch = fetchpriority_attr,
        style = style_attr,
        alt = html_escape(alt),
        extra = extra,
    )
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
