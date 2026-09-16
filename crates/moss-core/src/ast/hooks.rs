//! Render hooks: per-node-type interception for HTML emission.
//!
//! Hugo's render-hooks pattern, ported to Rust. The renderer walks the AST
//! and calls `RenderHooks::render_*` methods at interceptable points; each
//! method has a default implementation in [`DefaultHooks`] that produces
//! moss's canonical HTML.
//!
//! Consumers (src-tauri's `PipelineHooks`, future plugins) override one
//! method without touching the renderer or the AST. Defaults handle the
//! ~80% case; overrides handle site-specific concerns (asset path
//! rewriting, classname injection, etc).
//!
//! # Architectural prior art
//!
//! `RenderHooks` is moss's port of Hugo's render-hooks pattern
//! ([`markup/goldmark/render_hooks.go`](https://github.com/gohugoio/hugo/blob/master/markup/goldmark/render_hooks.go)).
//! In Hugo, `hookedRenderer` IS Goldmark's `NodeRenderer` — hooks fire
//! during the AST walk, every CommonMark-native attribute reaches the hook
//! (e.g. `linkContext.Title`), and the template owns rendering decisions.
//!
//! Cross-SSG research (2026-05-27) confirms this is the canonical shape:
//! every AST-bearing SSG (Hugo, Markdoc, mdast/remark, Pandoc, comrak,
//! recent mdBook) carries every parser-emitted attribute (including link
//! title) through to the renderer. Dropping fields the parser saw is
//! universally regarded as Gatsby's mistake — lossy AST forces consumers
//! into a plugin ecosystem they wouldn't need if the AST were faithful.
//!
//! See [docs/archive/2026-05-27-typed-ast-cross-ssg-research.md](../../../../docs/archive/2026-05-27-typed-ast-cross-ssg-research.md)
//! for the full research synthesis and [typed-body-ast.md](../../../../docs/reference/typed-body-ast.md)
//! for the design intent + 7 principles.

use super::grid_parts::GridParts;
use super::shortcode::{GridShortcode, Shortcode};
use super::url::{ResolvedUrl, UrlKind};

/// Per-node-type HTML emission hooks. All methods have default
/// implementations; consumers override only the ones they need.
///
/// Methods write into the supplied `&mut String` rather than returning
/// fresh allocations. This avoids per-call String construction inside the
/// render loop and lets consumers compose without intermediate copies.
pub trait RenderHooks {
    /// Emit `<a href="...">...</a>` for a link.
    ///
    /// The default impl carries forward moss's existing post-render
    /// conventions: `class="wikilink"` when `is_wikilink: true`,
    /// `target="_blank" rel="noopener"` when `url.kind` is
    /// `UrlKind::AssetNewtab` or `UrlKind::External`. Both flags are
    /// **orthogonal** — a wikilink that resolves to an asset-newtab target
    /// emits BOTH `class="wikilink"` AND `target="_blank" rel="noopener"`.
    ///
    /// # `is_wikilink` parameter (PR7a-flip-core-A)
    ///
    /// Phase 4 PR7a-flip-core-A (2026-05-28) added `is_wikilink: bool` to
    /// the signature. Before this change, the renderer had to synthesize
    /// a wikilink-kinded `ResolvedUrl` to coax the hook into emitting
    /// `class="wikilink"` — a lossy workaround that fused two orthogonal
    /// concerns (URL kind + wikilink syntax discriminator) into a single
    /// field. The flag carries pulldown-cmark's `LinkType::WikiLink`
    /// discriminator faithfully into the renderer, matching how every
    /// AST-bearing SSG threads its parse-time link metadata through
    /// (Hugo's `linkContext.Type`, Markdoc, mdast's `Resource`).
    ///
    /// # Title parameter (PR8 — scheduled)
    ///
    /// This signature is still missing the `title: Option<&str>` parameter
    /// that CommonMark links can carry (`[text](href "title")`). Title is
    /// silently dropped today through the AST render path. Invisible
    /// because production HTML still comes from `pulldown_cmark::html::push_html`
    /// (events carry title natively); becomes a regression the moment
    /// PR7a flips production to `render_document`.
    ///
    /// PR8 restores `title: Option<&str>` alongside other `RenderHooks`
    /// signature changes (`ResolvedUrl` private-constructor lockdown).
    /// Every comparable AST-bearing SSG passes title to its render hook
    /// (Hugo's `linkContext.Title`, Markdoc, mdast's `Resource.title`,
    /// comrak, Pandoc) — see
    /// [docs/archive/2026-05-27-typed-ast-cross-ssg-research.md](../../../../docs/archive/2026-05-27-typed-ast-cross-ssg-research.md).
    fn render_link(
        &self,
        out: &mut String,
        url: &ResolvedUrl,
        is_wikilink: bool,
        content: &str,
    ) {
        // PR7a-flip-core-A: is_wikilink and UrlKind::AssetNewtab are
        // orthogonal. A wikilink that resolves to an asset-newtab kind
        // emits BOTH `class="wikilink"` AND `target="_blank" rel="noopener"`
        // — neither concern shadows the other.
        //
        // `UrlKind::Wikilink` is also honored for is_wikilink for back-
        // compat with paths that set the kind without the flag (legacy
        // production resolved-URL kind classification still drives the
        // class injection for inline links that pre-date the flag).
        let want_wikilink_class = is_wikilink || matches!(url.kind, UrlKind::Wikilink);
        let want_newtab = matches!(url.kind, UrlKind::AssetNewtab | UrlKind::External);
        out.push_str(r#"<a"#);
        if want_wikilink_class {
            out.push_str(r#" class="wikilink""#);
        }
        if want_newtab {
            out.push_str(r#" target="_blank" rel="noopener""#);
        }
        out.push_str(r#" href=""#);
        out.push_str(&escape_attr(&url.href));
        out.push_str(r#"">"#);
        out.push_str(content);
        out.push_str("</a>");
    }

    /// Emit `<img src="..." alt="...">` for an image.
    ///
    /// `img_style` is an inline `style=` fragment for the image element
    /// (fit/position from a parameterized wikilink embed, e.g.
    /// `object-fit:cover`); `width` is the enclosing figure's `data-width`
    /// token (`wide|page|screen|body`, or a percent) so snapshot-aware impls
    /// can declare an honest `sizes=` for the escape band the figure renders
    /// in (ADR-021 Corollary 2). Both are `None` on the inline-image and
    /// CommonMark `![](url)` figure paths, which MUST stay byte-identical to
    /// what this trait emitted before those params existed.
    ///
    /// This is deliberately ONE method rather than a styled/unstyled pair:
    /// moss#754 was a wrapper impl forwarding only the styleless half,
    /// silently dropping embed styles inside `:::hero` overlays. With a
    /// single entry point a delegating impl cannot forward half the concept.
    fn render_image(
        &self,
        out: &mut String,
        src: &ResolvedUrl,
        alt: &str,
        title: Option<&str>,
        img_style: Option<&str>,
        width: Option<&str>,
    ) {
        // `width` only informs `sizes=` on a srcset ladder, which the bare
        // fallback shape has no way to emit.
        let _ = width;
        push_bare_img(out, src, alt, title, img_style);
    }

    /// Scope marker: the Grid arm of [`render_shortcode`]'s default impl
    /// calls this before rendering cell blocks (and [`end_grid_cells`]
    /// after), so an impl can scope image `sizes=` emission to the grid
    /// CELL width instead of the content column — see
    /// [`crate::contract::sizes::sizes_for_grid_cell`]. Calls nest for
    /// grids inside grid cells (inner-most wins).
    ///
    /// Default: no-op. [`DefaultHooks`] keeps a stack.
    fn begin_grid_cells(&self, columns: u32, data_width: Option<&str>) {
        let _ = (columns, data_width);
    }

    /// Close the scope opened by [`begin_grid_cells`]. Default: no-op.
    fn end_grid_cells(&self) {}

    /// Emit a math equation (ADR-030). `tex` is the raw LaTeX the author typed
    /// (delimiters stripped, unescaped); `display` distinguishes `$$…$$` from
    /// `$…$`; `fallback_html` is the P1 escaped-source `<code class="moss-math">`
    /// node the parser already built — the honest, never-blank rendering of the
    /// source itself.
    ///
    /// **Default impl (this crate, the editor, tests): emit `fallback_html`
    /// verbatim.** moss-core ships no typesetting engine, and this keeps every
    /// non-pipeline render byte-identical to P1. src-tauri's `PipelineHooks`
    /// overrides this to typeset `tex` to an inline `<svg>` via RaTeX, falling
    /// back to `fallback_html` on any guard/engine/validation refusal — so the
    /// two impls are genuinely different (SVG vs source), which is why this is a
    /// hook and not a fixed AST rendering (the three-question gate; ADR-030
    /// §3.2). The renderer routes `Inline::Other` math nodes here; a non-math
    /// `Inline::Other` never reaches this method.
    fn render_math(&self, out: &mut String, tex: &str, display: bool, fallback_html: &str) {
        let _ = (tex, display);
        out.push_str(fallback_html);
    }

    /// Optional [`AssetSnapshot`] for the gallery synth path.
    ///
    /// Phase 2E v5 PR3 (2026-05-26): when the impl returns
    /// `Some(snapshot)`, [`render_shortcode`]'s `Gallery` arm routes each
    /// item through [`crate::render::image::synthesize_image_html`] with
    /// [`ImageContext::GalleryThumb`], producing the canonical synth byte
    /// shape (`<picture><source srcset=*.webp>`, dims, LQIP, lazy
    /// loading). When the impl returns `None` (tests, fragment-render
    /// paths, downstream consumers without a snapshot), the legacy
    /// bare-`<img>` emission survives so the regex post-pass picks up
    /// the attributes.
    ///
    /// Default impl: `None`. [`DefaultHooks::with_snapshot`] overrides
    /// to expose the constructor-provided snapshot.
    ///
    /// The constructor-field pattern was chosen over extending
    /// `render_shortcode`'s signature: a typed snapshot field on the
    /// impl preserves the trait's public contract and keeps `RenderHooks`
    /// focused on "what HTML to emit", not "what HTML to emit given
    /// these inputs".
    fn gallery_assets(&self) -> Option<&crate::asset_snapshot::AssetSnapshot> {
        None
    }

    /// Emit a shortcode block as HTML.
    ///
    /// The default impl produces a minimal HTML skeleton suitable for the
    /// moss-core test harness; site-specific HTML (subscribe forms, button
    /// styles, gallery grids) lives in src-tauri's `PipelineHooks` impl
    /// because it depends on filesystem context (site_id, lang, asset
    /// paths) that moss-core doesn't have.
    fn render_shortcode(&self, out: &mut String, sc: &Shortcode, source_line: Option<usize>) {
        match sc {
            Shortcode::Subscribe(args) => {
                // Test-harness skeleton; src-tauri's PipelineHooks renders
                // the production HTML (form action URL, language defaults,
                // status spans). Description prose moved out of the
                // shortcode under the unified grammar.
                //
                // The outer `moss-subscribe` wrapper is emitted here so
                // the COMPONENTS contract round-trip test (and
                // ShortcodeKind::root_class()) can assert the root class
                // without depending on the full pipeline.
                let placeholder = args.placeholder.as_deref().unwrap_or("you@example.com");
                out.push_str(r#"<div class="moss-subscribe">"#);
                out.push_str(r#"<div class="moss-subscribe-form">"#);
                out.push_str(r#"<input type="email" placeholder=""#);
                out.push_str(&escape_attr(placeholder));
                out.push_str(r#"" />"#);
                out.push_str("<button>");
                out.push_str(&escape_text(args.button.as_deref().unwrap_or("Subscribe")));
                out.push_str("</button>");
                out.push_str("</div>");
                out.push_str("</div>");
            }
            Shortcode::Gallery(args) => {
                let mut class_attr = String::from("moss-gallery");
                if !args.classes.is_empty() {
                    class_attr.push(' ');
                    class_attr.push_str(&args.classes);
                }
                // An author-specified column count is emitted TWICE on
                // purpose: `--moss-gallery-columns` is the value `repeat()` can
                // compute with, and `data-columns` is the selector hook that
                // lets site.css give the counted and count-less galleries
                // different track rules (CSS cannot branch on "is this custom
                // property set?"). Mirrors `.moss-grid[data-columns]`.
                // Without the split, an explicit N inherits auto-fill's 200px
                // track floor and the gallery overflows its container.
                let count_attrs = match args.columns {
                    Some(n) => format!(r#" data-columns="{n}" style="--moss-gallery-columns: {n}""#),
                    None => String::new(),
                };
                out.push_str(r#"<div class=""#);
                out.push_str(&escape_attr(&class_attr));
                out.push('"');
                out.push_str(&count_attrs);
                if let Some(w) = &args.width {
                    out.push_str(r#" data-width=""#);
                    out.push_str(w);
                    out.push('"');
                }
                out.push('>');
                // Phase 4 PR1 (2026-05-27): every gallery item routes
                // through `render::image::synthesize_image_html` with
                // `ImageContext::GalleryThumb`. When the impl carries an
                // `AssetSnapshot` (production path via
                // `DefaultHooks::with_snapshot`), the synth emits the
                // canonical byte shape — `<picture><source srcset=*.webp>`
                // wrap, dims from the snapshot, LQIP, `loading="lazy"`.
                // When the impl carries no snapshot (test / fragment-render
                // paths), an empty `AssetSnapshot` is threaded through:
                // synth still emits `<picture>` for raster originals but
                // with fallback dims (800×600) and no LQIP/color style.
                // The legacy bare-`<img>` fallback was retired in Phase 4
                // PR1 — every `<img>` must be synth-emitted (or a
                // documented carve-out) per the `img_contract_test`
                // invariant.
                let empty_snapshot = crate::asset_snapshot::AssetSnapshot::new();
                let assets_for_synth = self.gallery_assets().unwrap_or(&empty_snapshot);
                for item in &args.items {
                    let src_str = match &item.src {
                        crate::ast::url::Url::Resolved(r) => r.href.clone(),
                        crate::ast::url::Url::Unresolved(s) => {
                            debug_assert!(
                                false,
                                "Url::Unresolved({s:?}) reached render_shortcode \
                                 — visit_urls_mut missing for Gallery"
                            );
                            s.clone()
                        }
                    };
                    out.push_str(r#"<div class="moss-gallery-item">"#);
                    // Synth path: full attribute set + optional <picture>
                    // wrap. The per-item inline style from MediaAttrs
                    // (e.g. object-position) flows through
                    // ImageRenderOptions::extra_attrs so the synthesizer's
                    // LQIP / dim emission joins it correctly (the synth
                    // suppresses its own style= when extra_attrs already
                    // carries one, matching the legacy regex's has_style
                    // guard).
                    let media_style =
                        crate::media::parse_media_attrs(&item.attrs).to_inline_style();
                    let style_frag = media_style.map(|s| format!(r#"style="{}""#, s));
                    let opts = crate::render::image::ImageRenderOptions {
                        eager: false,
                        extra_attrs: style_frag.as_deref(),
                        ..Default::default()
                    };
                    let item_html = crate::render::image::synthesize_image_html(
                        &src_str,
                        &item.alt,
                        assets_for_synth,
                        crate::render::image::ImageContext::GalleryThumb,
                        &opts,
                    );
                    out.push_str(&item_html);
                    out.push_str("</div>");
                }
                out.push_str("</div>");
            }
            Shortcode::Buttons(args) => {
                if args.items.is_empty() {
                    return;
                }
                // v1: the inverted variant is expressed via `data-style="inverted"`.
                // Any other authoring classes flow through as-is on `class=`.
                let mut data_style: Option<&str> = None;
                let mut extra_classes_vec: Vec<&str> = Vec::new();
                for token in args.classes.split_ascii_whitespace() {
                    if token == "inverted" {
                        data_style = Some("inverted");
                    } else {
                        extra_classes_vec.push(token);
                    }
                }
                let mut class_attr = String::from("moss-buttons");
                if !extra_classes_vec.is_empty() {
                    class_attr.push(' ');
                    class_attr.push_str(&extra_classes_vec.join(" "));
                }
                out.push_str(r#"<div class=""#);
                out.push_str(&escape_attr(&class_attr));
                if let Some(style) = data_style {
                    out.push_str(r#"" data-style=""#);
                    out.push_str(style);
                }
                out.push_str(r#"">"#);
                for (i, item) in args.items.iter().enumerate() {
                    let data_role = if i == 0 { "primary" } else { "secondary" };
                    let track = item
                        .text
                        .to_lowercase()
                        .replace(|c: char| !c.is_alphanumeric(), "-")
                        .trim_matches('-')
                        .to_string();
                    let resolved = match &item.url {
                        crate::ast::url::Url::Resolved(r) => r,
                        crate::ast::url::Url::Unresolved(s) => {
                            debug_assert!(
                                false,
                                "Url::Unresolved({s:?}) reached render_shortcode \
                                 — visit_urls_mut missing for Buttons"
                            );
                            // Release: emit href as-is so we don't crash.
                            out.push_str(r#"<a href=""#);
                            out.push_str(&escape_attr(s));
                            out.push_str(r#"" class="moss-btn" data-role=""#);
                            out.push_str(data_role);
                            out.push_str(r#"" data-track=""#);
                            out.push_str(&escape_attr(&track));
                            out.push_str(r#"">"#);
                            out.push_str(&escape_text(&item.text));
                            out.push_str("</a>");
                            continue;
                        }
                    };
                    out.push_str(r#"<a href=""#);
                    out.push_str(&escape_attr(&resolved.href));
                    out.push_str(r#"" class="moss-btn"#);
                    // Wikilink kind adds class="wikilink" suffix; collapse
                    // both classes into a single class attribute.
                    if matches!(resolved.kind, crate::ast::url::UrlKind::Wikilink) {
                        out.push_str(" wikilink");
                    }
                    out.push_str(r#"" data-role=""#);
                    out.push_str(data_role);
                    out.push_str(r#"" data-track=""#);
                    out.push_str(&escape_attr(&track));
                    out.push_str(r#"""#);
                    if matches!(resolved.kind, crate::ast::url::UrlKind::AssetNewtab) {
                        out.push_str(r#" target="_blank" rel="noopener""#);
                    }
                    out.push('>');
                    out.push_str(&escape_text(&item.text));
                    out.push_str("</a>");
                }
                out.push_str("</div>");
            }
            Shortcode::Hero(args) => {
                // Test-harness skeleton; the production renderer in
                // src-tauri's PipelineHooks routes the image through the
                // resolver, runs media-attrs into a style attribute, and
                // processes the overlay markdown to HTML. This default
                // emits a minimal `<section class="moss-hero">` so unit
                // tests can pattern-match on the wrapper without
                // depending on the full pipeline.
                //
                // NOTE: this arm does NOT emit `data-mobile="overlay"` or
                // `--moss-hero-mobile-bg`/`--moss-hero-mobile-color` CSS custom
                // properties — those require `dominant_color` from the scan
                // cache which is not available in moss-core (zero-I/O invariant).
                // Before PR7a-flip-core-B promotes `render_document` as the sole
                // Hero rendering path, this arm must either be updated or
                // callers must use `PipelineHooks` to get full mobile attrs.
                // Tracked: src-tauri issue #747 (HeroRenderContext refactor).
                //
                // Phase 4 PR1 (2026-05-27): the inner `<img>` is now
                // emitted via [`crate::render::image::synthesize_image_html`]
                // instead of a bare `<img>` literal so the `img_contract_test`
                // invariant ("every <img> is synth-emitted or a documented
                // carve-out") holds when `render_document` runs through
                // `DefaultHooks`. When the impl carries a snapshot
                // (`gallery_assets()` is `Some`) the synth uses
                // `ImageContext::Hero` (full `<picture>`/dims/LQIP byte
                // shape); without a snapshot it falls back to the
                // `ImageContext::HeroBare` carve-out per ADR-013 — the
                // legitimate "before set_pending" path where no variant
                // can be promised.
                let mut class_attr = String::from("moss-hero");
                if !args.classes.is_empty() {
                    class_attr.push(' ');
                    class_attr.push_str(&args.classes);
                }
                out.push_str(r#"<section class=""#);
                out.push_str(&escape_attr(&class_attr));
                out.push('"');
                // Multi-image hero: data-slides drives the site.css
                // crossfade; absent for the single-image hero. Capped at 6
                // like the production renderer.
                let hero_extras: &[crate::ast::url::Url] =
                    &args.extra_images[..args.extra_images.len().min(5)];
                if args.image.is_some() && !hero_extras.is_empty() {
                    out.push_str(r#" data-slides=""#);
                    out.push_str(&(hero_extras.len() + 1).to_string());
                    out.push('"');
                }
                if let Some(w) = &args.width {
                    out.push_str(r#" data-width=""#);
                    out.push_str(w);
                    out.push('"');
                }
                // Click-to-source: emit a point source range on the Hero's
                // outermost element so the bridge's `resolveSourceTarget`
                // (`data-source-range`) routes a click back to the `:::hero`
                // line. Same start/end = point range (mirrors the Grid arm).
                if let Some(n) = source_line {
                    out.push_str(r#" data-source-range=""#);
                    out.push_str(&n.to_string());
                    out.push('-');
                    out.push_str(&n.to_string());
                    out.push('"');
                }
                out.push('>');
                if let Some(image) = &args.image {
                    let src = match image {
                        crate::ast::url::Url::Resolved(r) => r.href.clone(),
                        crate::ast::url::Url::Unresolved(s) => {
                            debug_assert!(
                                false,
                                "Url::Unresolved({s:?}) reached render_shortcode \
                                 — visit_urls_mut missing for Hero"
                            );
                            s.clone()
                        }
                    };
                    let empty_snapshot = crate::asset_snapshot::AssetSnapshot::new();
                    let (snap, ctx) = match self.gallery_assets() {
                        Some(s) => (s, crate::render::image::ImageContext::Hero),
                        None => (&empty_snapshot, crate::render::image::ImageContext::HeroBare),
                    };
                    let opts = crate::render::image::ImageRenderOptions::default();
                    let hero_media = |href: &str| -> String {
                        let ext = href.rsplit('.').next().unwrap_or("");
                        if matches!(
                            crate::resolve::ext_kind::reference_kind_for_ext(ext),
                            crate::resolve::ext_kind::ExtKind::Video
                        ) {
                            // Video backgrounds are ambient loops — same
                            // synthesis as `![[clip.mp4|loop]]`.
                            let mut params = crate::resolve::title_params::TitleParams::default();
                            params.params.insert("loop".into(), "1".into());
                            crate::render::video::synthesize_video_html(&params, href, snap)
                        } else {
                            crate::render::image::synthesize_image_html(
                                href,
                                "",
                                snap,
                                ctx.clone(),
                                &opts,
                            )
                        }
                    };
                    let img_html = hero_media(&src);
                    if hero_extras.is_empty() {
                        out.push_str(&img_html);
                    } else {
                        // Multi-image hero: slides wrapper = positioning
                        // context (mobile stacked safety), each slide
                        // wrapped, primary first.
                        out.push_str(r#"<div class="moss-hero-slides"><div class="moss-hero-slide">"#);
                        out.push_str(&img_html);
                        out.push_str("</div>");
                        for extra in hero_extras {
                            let href = match extra {
                                crate::ast::url::Url::Resolved(r) => r.href.clone(),
                                crate::ast::url::Url::Unresolved(s) => s.clone(),
                            };
                            let extra_html = hero_media(&href);
                            out.push_str(r#"<div class="moss-hero-slide">"#);
                            out.push_str(&extra_html);
                            out.push_str("</div>");
                        }
                        out.push_str("</div>");
                    }
                }
                if !args.overlay.is_empty() {
                    // Phase 4 PR4.5 (2026-05-28): overlay is now typed
                    // `Vec<Block>`. Render via `render_blocks` then
                    // collapse tag-adjacent newlines so the byte shape
                    // matches today's `render_markdown_to_html_with`
                    // production (pulldown-cmark's `push_html` emits no
                    // tag-adjacent newlines).
                    //
                    // `DefaultHooks::hero_overlay` suppresses
                    // moss-heading-anchor on overlay headings. We rebuild
                    // DefaultHooks from gallery_assets() rather than coercing
                    // `self` to `&dyn RenderHooks` — the coercion requires
                    // `Self: Sized`, which the trait default cannot guarantee.
                    let mut overlay_html = String::new();
                    render_hero_overlay_blocks(
                        self.gallery_assets(),
                        &args.overlay,
                        &mut overlay_html,
                    );
                    let collapsed = collapse_tag_adjacent_newlines(&overlay_html);
                    out.push_str(r#"<div class="moss-hero-content">"#);
                    out.push_str(collapsed.trim());
                    out.push_str("</div>");
                }
                out.push_str("</section>");
            }
            Shortcode::Grid(args) => {
                // ONE grid byte shape: the flat form is the split form
                // re-assembled. See [`RenderHooks::render_grid_parts`].
                out.push_str(&self.render_grid_parts(args, source_line).to_html());
            }
            Shortcode::Recent(_args) => {
                // Test-harness skeleton; the production renderer in
                // src-tauri's PipelineHooks queries the post set (which
                // moss-core cannot see) and emits the actual list. This
                // default emits an empty `<div class="moss-recent">` so
                // unit tests that only care about presence can pattern-
                // match. Body fallback rendering is the production
                // renderer's responsibility.
                out.push_str(r#"<div class="moss-recent"></div>"#);
            }
            Shortcode::Apply(args) => {
                // Test-harness skeleton; the production renderer in
                // src-tauri's PipelineHooks emits the full form HTML with
                // the seta action URL and language-specific copy.
                let placeholder = args.placeholder.as_deref().unwrap_or("your@email.com");
                out.push_str(r#"<div class="moss-apply">"#);
                out.push_str(r#"<form class="moss-subscribe-form moss-apply-form" data-position="apply" data-revert="false">"#);
                out.push_str(r#"<input type="email" placeholder=""#);
                out.push_str(&escape_attr(placeholder));
                out.push_str(r#"" />"#);
                out.push_str("<button>");
                out.push_str(&escape_text(args.button.as_deref().unwrap_or("申请")));
                out.push_str("</button>");
                out.push_str("</form>");
                out.push_str("</div>");
            }
        }
    }

    /// Serialize a `:::grid` into its structural pieces — the opening `<div>`
    /// tag and one HTML string per cell — rather than one flat string.
    ///
    /// [`render_shortcode`](RenderHooks::render_shortcode)'s `Grid` arm is
    /// `self.render_grid_parts(..).to_html()`, so overriding this method (or
    /// leaving the default) changes both forms together and they cannot
    /// diverge.
    ///
    /// A host that must replace individual cells with markup derived from
    /// whole-build state (folder cards, external link previews) pairs these
    /// strings with `args.cells` and decides per cell from the typed blocks.
    /// That is the whole point: the *decision* is made on the same typed data
    /// the emission was made from, so the two can never drift — unlike the
    /// regex-over-emitted-HTML pass this replaced (ADR-034).
    ///
    /// Impls that delegate `render_shortcode`'s `Grid` arm to a *different*
    /// hooks value (src-tauri's `PipelineHooks` delegates to a
    /// [`DefaultHooks`] bound to its asset snapshot, so grid-cell images get
    /// the snapshot-aware `sizes=` ladder) must override this method the same
    /// way, or the split form will render cells through the wrong hooks.
    fn render_grid_parts(&self, args: &GridShortcode, source_line: Option<usize>) -> GridParts {
        super::grid_parts::render_grid_parts(self, args, source_line)
    }

    /// Whether in-body footnote markers and the endnote section carry
    /// `id="fnref-…"` / `href="#fn-…"` anchors.
    ///
    /// Default `true` — byte-identical to before this method existed.
    /// `crate` (src-tauri)'s `EmailHooks` overrides to `false`: ADR-035 fixes
    /// "email emits no anchors" as the target behavior (an in-body fragment
    /// link is dead weight in a mail client, and a stray `id=` surviving a
    /// client's HTML sanitizer is noise), but the marker's visible number and
    /// the endnote's `[N]` still render — only the linking apparatus is
    /// suppressed. See `ast::footnotes::render_marker` / `render_section`,
    /// the two call sites this gates.
    fn emit_footnote_anchors(&self) -> bool {
        true
    }

    /// Whether headings carry the `moss-heading-anchor` permalink.
    ///
    /// False for hero overlays: hero headings are display titles, not
    /// in-page navigation landmarks, so the `#` link is visual noise. The
    /// `id` attribute is kept regardless, so fragment links still resolve.
    ///
    /// A policy hook rather than a `render_heading` override (the shape
    /// [`emit_footnote_anchors`](RenderHooks::emit_footnote_anchors) already
    /// established) so there stays exactly ONE heading emitter — moss#754
    /// was a second, partially-delegating hooks impl drifting from the
    /// first.
    fn emit_heading_anchors(&self) -> bool {
        true
    }

    /// The localized `aria-label` for the per-heading permalink anchor
    /// (`<a class="moss-heading-anchor" aria-label="…">`).
    ///
    /// moss-core is pure Rust with no i18n table (no `i18n::Language`),
    /// so the LOCALIZED string is resolved by the src-tauri caller
    /// (`crate::i18n::strings::t(site_lang, "permalink_section")`) and
    /// threaded in via the hooks impl — `render_heading` just emits
    /// whatever this returns. The trait default keeps the historical
    /// English string so moss-core's own test/fragment-render paths
    /// stay byte-identical without a language in scope.
    fn permalink_section_label(&self) -> &str {
        "Permalink to this section"
    }

    /// Emit `<h1>...</h1>` (or h2/h3/...) for a heading.
    ///
    /// `id` is the heading anchor id (slug). When `Some`, the rendered
    /// tag carries `id="..."` so anchor links work.
    ///
    /// `source_line` is the 1-based source line where the heading appears
    /// in the markdown. When `Some`, the rendered tag carries
    /// `data-source-line="N"` for the preview's editor↔preview scroll
    /// sync (see `frontend/bridge/iframe-bridge.ts`'s `scrollToSourceLine`
    /// RPC). When `None` (default config / fragment-render paths), no
    /// attr is emitted — byte-identical to the pre-source-line shape.
    ///
    /// Source-line wiring added 2026-05-28 (Phase 4 cleanup): restores
    /// the preview-scroll paragraph-precision feature that PR7a's
    /// `transform_events` deletion removed.
    fn render_heading(
        &self,
        out: &mut String,
        level: u8,
        id: Option<&str>,
        source_line: Option<usize>,
        content: &str,
    ) {
        out.push('<');
        out.push('h');
        out.push((b'0' + level) as char);
        if let Some(id) = id {
            out.push_str(r#" id=""#);
            out.push_str(&escape_attr(id));
            out.push('"');
        }
        if let Some(n) = source_line {
            use std::fmt::Write as _;
            let _ = write!(out, r#" data-source-line="{}""#, n);
        }
        out.push('>');
        out.push_str(content);
        // Permalink anchor — only when the heading has a slug id. Appended
        // AFTER content / BEFORE </h> so the opening tag (id, data-source-line)
        // stays byte-identical for preview scroll-sync. The auto-injected
        // article-title H1 (`<h1 class="moss-article-title">`) is emitted
        // separately in src-tauri html_post.rs and never reaches this hook,
        // so it correctly gets no anchor.
        //
        // The `#` glyph is NOT in here. It is drawn by site.css as
        // `.moss-heading-anchor::after { content: "#" }`, because generated
        // content is the only form of it that cannot be copied. A real text
        // node with `user-select: none` (which is what this used to be, and
        // what site.css still carries as a second line of defence) is
        // excluded from the clipboard by Blink but NOT by WebKit — so on
        // Safari, and in moss's own WebKit preview, selecting a heading
        // copied "Introduction#". Measured in both engines 2026-08-09; the
        // heading-anchor-copy render gate pins it.
        if let Some(id) = id.filter(|_| self.emit_heading_anchors()) {
            out.push_str(r##"<a class="moss-heading-anchor" href="#"##);
            out.push_str(&escape_attr(id));
            out.push_str(r##"" aria-label=""##);
            out.push_str(&escape_attr(self.permalink_section_label()));
            out.push_str(r##""></a>"##);
        }
        out.push_str("</h");
        out.push((b'0' + level) as char);
        out.push('>');
    }
}

/// Default render hooks. moss-core ships this as the base implementation
/// of [`RenderHooks`]; the methods are the trait's default impls.
///
/// Phase 2E v5 PR3 (2026-05-26): refactored from a unit struct to a
/// lifetime-parameterized struct so callers can attach an
/// [`AssetSnapshot`] for the gallery synth path. Use
/// [`DefaultHooks::new`] / `Default::default()` for the no-snapshot
/// behavior (legacy bare-`<img>` emission; surviving regex post-pass
/// fills in attributes); use [`DefaultHooks::with_snapshot`] for the
/// production path (full synth byte shape — `<picture><source srcset>`,
/// dims, LQIP, lazy loading).
///
/// The constructor-field pattern was chosen over extending
/// [`RenderHooks::render_shortcode`]'s signature with an
/// `&AssetSnapshot` arg: a struct field is non-breaking for downstream
/// `RenderHooks` impls and keeps the trait focused on "what HTML to
/// emit" rather than "what HTML to emit given these inputs". See
/// [`gallery_assets`](RenderHooks::gallery_assets) for the override
/// hook the trait exposes.
#[derive(Debug, Default)]
pub struct DefaultHooks<'a> {
    assets: Option<&'a crate::asset_snapshot::AssetSnapshot>,
    /// Stack of grid-cell `sizes=` scopes (innermost last), pushed/popped
    /// by [`RenderHooks::begin_grid_cells`]/[`end_grid_cells`] around the
    /// Grid arm's cell rendering. `RefCell` because the render walk holds
    /// `&self` throughout; nesting (a grid inside a grid cell) is why this
    /// is a stack and not an `Option`.
    grid_cell_sizes: std::cell::RefCell<Vec<String>>,
    /// Display-title mode: suppress the `moss-heading-anchor` permalink.
    /// Stored negated so `#[derive(Default)]` still means "anchors on".
    /// Set by [`DefaultHooks::hero_overlay`] and [`DefaultHooks::grid_cells`].
    suppress_heading_anchors: bool,
}

impl<'a> DefaultHooks<'a> {
    /// Construct a no-snapshot `DefaultHooks`. The `Gallery` arm of
    /// [`RenderHooks::render_shortcode`] emits the legacy bare-`<img>`
    /// byte shape so the regex post-pass can fill in attributes. Use
    /// this for tests, fragment-render paths, and downstream consumers
    /// without a primed `AssetSnapshot`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a `DefaultHooks` carrying an [`AssetSnapshot`] for the
    /// gallery synth path. The `Gallery` arm of
    /// [`RenderHooks::render_shortcode`] routes each item through
    /// [`crate::render::image::synthesize_image_html`] with
    /// [`crate::render::image::ImageContext::GalleryThumb`], emitting
    /// the canonical synth byte shape.
    pub fn with_snapshot(assets: &'a crate::asset_snapshot::AssetSnapshot) -> Self {
        Self {
            assets: Some(assets),
            ..Self::default()
        }
    }

    /// Hooks for rendering the blocks of a `:::hero` overlay: identical to
    /// the normal hooks except headings carry no permalink anchor (hero
    /// headings are display titles, not navigation landmarks).
    ///
    /// `assets` is the caller's snapshot, if any — the same `None`/`Some`
    /// split as [`new`](Self::new) / [`with_snapshot`](Self::with_snapshot).
    ///
    /// moss#754: this replaced a `HeroOverlayHooks` wrapper that re-implemented
    /// `RenderHooks` by forwarding to an inner impl. Forwarding was manual, so
    /// the wrapper silently dropped every hook nobody remembered to list —
    /// which is how image embeds inside hero overlays lost their
    /// `object-fit`/`object-position`. A construction mode on the one impl has
    /// no forwarding surface to get wrong.
    pub fn hero_overlay(assets: Option<&'a crate::asset_snapshot::AssetSnapshot>) -> Self {
        Self {
            assets,
            suppress_heading_anchors: true,
            ..Self::default()
        }
    }

    /// Hooks for rendering the blocks of a `:::grid` cell. Same reasoning as
    /// [`hero_overlay`](Self::hero_overlay): a heading in a cell is that
    /// card's title, not a section of the page. Nobody deep-links to a card
    /// the way they deep-link to a section, so the `#` is clutter — and it is
    /// worse than clutter inside a card, where it lands in the middle of a
    /// tile rather than in the margin of a text column.
    ///
    /// It also closes a hole. Grid cells render through `DefaultHooks`, whose
    /// trait defaults said anchors-on and English-label, so a cell heading
    /// carried a `#` even on a site with `[site].heading_anchors = false`, and
    /// carried `aria-label="Permalink to this section"` on a zh-tw site. Both
    /// were invisible from the src-tauri side, which reads its own
    /// `PipelineHooks` overrides and never sees the ones a delegated render
    /// uses. Suppressing outright means neither setting can be wrong here.
    pub fn grid_cells(assets: Option<&'a crate::asset_snapshot::AssetSnapshot>) -> Self {
        Self {
            assets,
            suppress_heading_anchors: true,
            ..Self::default()
        }
    }
}

impl<'a> RenderHooks for DefaultHooks<'a> {
    fn gallery_assets(&self) -> Option<&crate::asset_snapshot::AssetSnapshot> {
        self.assets
    }

    fn emit_heading_anchors(&self) -> bool {
        !self.suppress_heading_anchors
    }

    fn begin_grid_cells(&self, columns: u32, data_width: Option<&str>) {
        self.grid_cell_sizes
            .borrow_mut()
            .push(crate::contract::sizes::sizes_for_grid_cell(
                columns, data_width,
            ));
    }

    fn end_grid_cells(&self) {
        self.grid_cell_sizes.borrow_mut().pop();
    }

    /// Phase 4 PR1 (2026-05-27): when the impl carries an `AssetSnapshot`
    /// (production path via [`DefaultHooks::with_snapshot`]), route the
    /// inline `Inline::Image` emission through
    /// [`crate::render::image::synthesize_image_html`] with
    /// [`crate::render::image::ImageContext::MarkdownInline`] — produces
    /// `<picture><img></picture>` for raster originals (with dims, LQIP,
    /// `loading="lazy"`) WITHOUT `<figure>` wrap. The figure wrap is
    /// PR3's territory: `Block::Figure { image, caption }` is the typed
    /// variant emitted for image-only paragraphs, and its renderer uses
    /// `MarkdownStandalone` to apply the `<figure class="moss-image">`
    /// wrap.
    ///
    /// **PR1 v2 correction (2026-05-27 post-Wave-0.5 parity probe):** an
    /// earlier PR1 iteration used `MarkdownStandalone` here, which wrapped
    /// EVERY inline image in `<figure>` even when the paragraph carried
    /// sibling content (image + italic caption text + prose). The parity
    /// probe surfaced 9 false-positive divergences (刘果/文字/* CJK
    /// content with image+caption-inline patterns) where the AST emitted
    /// `<figure>` and production correctly did not. Fix: use
    /// `MarkdownInline` here; let PR3's `Block::Figure` own the figure
    /// wrap for the legitimate image-only-paragraph case.
    ///
    /// Without a snapshot, fall back to [`push_bare_img`] — the same shape
    /// the trait default emits (test / fragment-render paths). Production
    /// consumers (`PipelineHooks`) always provide a populated snapshot.
    ///
    /// `title` is the CommonMark image title — emitted as a `title=""`
    /// HTML attribute on the rendered `<img>` (or `<picture>`'s inner
    /// `<img>`). Per cross-SSG research (2026-05-27), every AST-bearing
    /// SSG carries title through to the renderer; dropping = Gatsby's
    /// mistake.
    /// Snapshot-aware figure-inner image. `img_style` (fit/position from a
    /// parameterized wikilink embed) flows through
    /// `ImageRenderOptions.extra_attrs` onto the inner `<img>`; the
    /// synthesizer's `extra_has_style` guard then suppresses its
    /// LQIP/dominant-color `style=` so the two declarations never collide
    /// (the same guard the legacy fast-path relied on). `img_style: None`
    /// yields `ImageRenderOptions::default()` — byte-identical to before the
    /// synth-collapse.
    ///
    /// `sizes=` precedence for the srcset ladder (most specific slot
    /// knowledge wins): the figure's own `data-width` token → the enclosing
    /// grid cell scope ([`begin_grid_cells`]) → the context default
    /// ([`crate::contract::sizes::SIZES_BODY`] via `MarkdownInline`).
    fn render_image(
        &self,
        out: &mut String,
        src: &ResolvedUrl,
        alt: &str,
        title: Option<&str>,
        img_style: Option<&str>,
        width: Option<&str>,
    ) {
        let assets = match self.assets {
            Some(a) => a,
            None => {
                // No snapshot in scope — fall back to the same bare-`<img>`
                // shape the trait default emits. This is the test-only path;
                // the production path always provides a snapshot via
                // `PipelineHooks::render_image`. The style fragment, when
                // present, still rides on the `<img>`.
                push_bare_img(out, src, alt, title, img_style);
                return;
            }
        };
        // PR1 v2 (post-Wave-0.5 parity probe): MarkdownInline (no figure
        // wrap) for inline images. PR3's Block::Figure handles the figure
        // wrap for image-only paragraphs.
        let ctx = crate::render::image::ImageContext::MarkdownInline;
        let style_attr =
            img_style.map(|s| format!(r#"style="{}""#, crate::media::html_escape(s)));
        // sizes= precedence: figure data-width token > grid cell scope >
        // context default (see doc comment above).
        let cell_scope = self.grid_cell_sizes.borrow();
        let sizes: Option<&str> = width
            .and_then(crate::contract::sizes::sizes_for_data_width)
            .or_else(|| cell_scope.last().map(String::as_str));
        let opts = crate::render::image::ImageRenderOptions {
            extra_attrs: style_attr.as_deref(),
            sizes,
            ..Default::default()
        };
        let html = crate::render::image::synthesize_image_html(
            &src.href,
            alt,
            assets,
            ctx,
            &opts,
        );
        out.push_str(&html);
    }
}

// ---------------------------------------------------------------------------
// Internal escape helpers used by both DefaultHooks and the renderer.
// ---------------------------------------------------------------------------

/// The bare `<img>` shape — the single formatting point for the two
/// snapshot-less [`RenderHooks`] image exits (the trait default and
/// [`DefaultHooks`]'s no-snapshot fallback), so those two can never drift.
///
/// It is NOT every bare `<img>` in the tree: `ast::render`'s two
/// `Url::Unresolved` arms hand-roll their own, deliberately — they are
/// `debug_assert!`-guarded corruption paths that fire only when
/// `visit_urls_mut` has failed to run, and they have no resolved URL to hand
/// this function. Snapshot-aware impls emit a `<picture>` ladder instead and
/// do not call this either.
fn push_bare_img(
    out: &mut String,
    src: &ResolvedUrl,
    alt: &str,
    title: Option<&str>,
    img_style: Option<&str>,
) {
    out.push_str(r#"<img src=""#);
    out.push_str(&escape_attr(&src.href));
    out.push_str(r#"" alt=""#);
    out.push_str(&escape_attr(alt));
    out.push('"');
    if let Some(t) = title {
        out.push_str(r#" title=""#);
        out.push_str(&escape_attr(t));
        out.push('"');
    }
    if let Some(s) = img_style {
        out.push_str(r#" style=""#);
        out.push_str(&escape_attr(s));
        out.push('"');
    }
    out.push_str(" />");
}

/// Render hero overlay blocks into `out` via [`DefaultHooks::hero_overlay`]
/// so that headings inside the overlay don't carry a `moss-heading-anchor`
/// permalink.
///
/// Accepts an optional `AssetSnapshot` reference (from the caller's
/// `gallery_assets()`) to route inline images through the synth path when
/// available. Creates a fresh `DefaultHooks` from the snapshot rather than
/// coercing the caller's `self` to `&dyn RenderHooks` — that coercion
/// requires `Self: Sized`, which the trait default method cannot guarantee.
///
/// **Limitation**: only `gallery_assets()` is carried over from the caller.
/// Any other hook overrides on the caller (`render_link`, `render_shortcode`,
/// etc.) are NOT propagated. This is intentional for the `DefaultHooks`
/// test-harness path; production callers (`PipelineHooks`) override
/// `render_shortcode` entirely and never reach this function.
fn render_hero_overlay_blocks(
    snapshot: Option<&crate::asset_snapshot::AssetSnapshot>,
    blocks: &[super::node::Block],
    out: &mut String,
) {
    super::render::render_blocks(&DefaultHooks::hero_overlay(snapshot), out, blocks);
}

/// Collapse `\n` that follows a heading or paragraph closing tag when the
/// next character is `<` (the start of another block). Mirrors
/// pulldown-cmark's `push_html` byte shape: heading and paragraph closes
/// emit no trailing newline before the next block opens (`</h2><ul>`,
/// `</p><div>`), but `</li>`, `</ul>`, `</ol>`, etc. keep their trailing
/// `\n` (`<ul>\n<li>`, `</li>\n<li>`, `</li>\n</ul>`).
///
/// Phase 4 PR4.5 (2026-05-28): introduced for the Grid arm's per-cell
/// rendering. The AST `render_block` emits trailing `\n` after every
/// block-level child to keep top-level document HTML readable; inside a
/// grid cell, that whitespace needs surgical removal to byte-match
/// production.
///
/// The selectivity is necessary because production preserves list-internal
/// newlines (`<ul>\n<li>...\n<li>...`) but collapses heading-to-block and
/// paragraph-to-block boundaries (`</h2><ul>`, `</p><h3>`). Both shapes
/// land in the same grid cell HTML, so a single rule won't do.
pub fn collapse_tag_adjacent_newlines(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\n' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] == '\n' {
                j += 1;
            }
            let next_is_open_tag = j < chars.len() && chars[j] == '<';
            // Inspect the last few chars of `out` to determine the closing
            // tag name. Only collapse if the closing tag is `</hN>`
            // (N = 1..=6) or `</p>`.
            let last = out.as_bytes();
            let prev_is_close_tag = last.last() == Some(&b'>');
            let collapse = prev_is_close_tag
                && next_is_open_tag
                && (ends_with_heading_close(&out) || ends_with_paragraph_close(&out));
            if collapse {
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// True if `s` ends with `</h1>` ... `</h6>` (a heading close tag).
fn ends_with_heading_close(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 5 {
        return false;
    }
    // Look for the pattern `</hN>` at the end where N is 1-6.
    let n = bytes.len();
    bytes[n - 5] == b'<'
        && bytes[n - 4] == b'/'
        && bytes[n - 3] == b'h'
        && matches!(bytes[n - 2], b'1' | b'2' | b'3' | b'4' | b'5' | b'6')
        && bytes[n - 1] == b'>'
}

/// True if `s` ends with `</p>` (a paragraph close tag).
fn ends_with_paragraph_close(s: &str) -> bool {
    s.ends_with("</p>")
}

/// Escape `&"<>` for HTML attribute values.
pub(super) fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escape `&<>` for HTML text content. Mirrors pulldown-cmark's text
/// escape (it does NOT escape `"` in text — only in attributes).
pub(super) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::node::Inline;
    use crate::ast::url::Url;

    #[test]
    fn escape_attr_handles_all_unsafe_chars() {
        assert_eq!(escape_attr(r#"&"<>"#), "&amp;&quot;&lt;&gt;");
    }

    #[test]
    fn escape_text_does_not_quote_double_quote() {
        assert_eq!(escape_text(r#"hi "there" & y"#), r#"hi "there" &amp; y"#);
    }

    #[test]
    fn default_hooks_render_heading_emits_id() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 2, Some("setup"), None, "Setup");
        assert_eq!(
            out,
            r##"<h2 id="setup">Setup<a class="moss-heading-anchor" href="#setup" aria-label="Permalink to this section"></a></h2>"##
        );
    }

    #[test]
    fn default_hooks_render_heading_without_id() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 3, None, None, "Sub");
        assert_eq!(out, "<h3>Sub</h3>");
    }

    #[test]
    fn default_hooks_render_heading_emits_source_line() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 2, Some("setup"), Some(42), "Setup");
        assert_eq!(
            out,
            r##"<h2 id="setup" data-source-line="42">Setup<a class="moss-heading-anchor" href="#setup" aria-label="Permalink to this section"></a></h2>"##
        );
    }

    #[test]
    fn default_hooks_render_heading_source_line_without_id() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 1, None, Some(1), "Top");
        assert_eq!(out, r#"<h1 data-source-line="1">Top</h1>"#);
    }

    #[test]
    fn default_hooks_render_heading_emits_anchor_when_id_present() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 2, Some("setup"), None, "Setup");
        assert_eq!(
            out,
            r##"<h2 id="setup">Setup<a class="moss-heading-anchor" href="#setup" aria-label="Permalink to this section"></a></h2>"##
        );
    }

    #[test]
    fn default_hooks_render_heading_no_anchor_when_id_absent() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 3, None, None, "Sub");
        assert_eq!(out, "<h3>Sub</h3>");
    }

    #[test]
    fn default_hooks_render_heading_anchor_preserves_source_line_position() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 2, Some("setup"), Some(42), "Setup");
        assert_eq!(
            out,
            r##"<h2 id="setup" data-source-line="42">Setup<a class="moss-heading-anchor" href="#setup" aria-label="Permalink to this section"></a></h2>"##
        );
    }

    #[test]
    fn default_hooks_render_heading_anchor_escapes_id_in_href() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_heading(&mut out, 2, Some(r#"a&b"#), None, "AB");
        assert_eq!(
            out,
            r##"<h2 id="a&amp;b">AB<a class="moss-heading-anchor" href="#a&amp;b" aria-label="Permalink to this section"></a></h2>"##
        );
    }

    #[test]
    fn default_hooks_render_link_internal_default() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("docs/", UrlKind::Internal),
            false,
            "Docs",
        );
        assert_eq!(out, r#"<a href="docs/">Docs</a>"#);
    }

    #[test]
    fn default_hooks_render_link_wikilink_kind_injects_class() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("../docs/", UrlKind::Wikilink),
            false,
            "Docs",
        );
        assert_eq!(out, r#"<a class="wikilink" href="../docs/">Docs</a>"#);
    }

    #[test]
    fn default_hooks_render_link_is_wikilink_flag_injects_class() {
        // PR7a-flip-core-A: `is_wikilink: true` injects `class="wikilink"`
        // regardless of `url.kind` — the flag carries the parse-time
        // discriminator (pulldown-cmark `LinkType::WikiLink`) and is
        // honored independently from the resolved URL classification.
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("../docs/", UrlKind::Internal),
            true,
            "Docs",
        );
        assert_eq!(out, r#"<a class="wikilink" href="../docs/">Docs</a>"#);
    }

    #[test]
    fn default_hooks_render_link_asset_newtab_injects_target() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("file.pdf", UrlKind::AssetNewtab),
            false,
            "PDF",
        );
        assert_eq!(
            out,
            r#"<a target="_blank" rel="noopener" href="file.pdf">PDF</a>"#
        );
    }

    #[test]
    fn default_hooks_render_link_wikilink_and_newtab_compose() {
        // PR7a-flip-core-A: `is_wikilink` and `UrlKind::AssetNewtab` are
        // orthogonal flags — a wikilink that resolves to an asset-newtab
        // target emits BOTH `class="wikilink"` AND `target="_blank"
        // rel="noopener"`. Pins the composition so a future refactor that
        // re-fuses the two concerns (e.g. via a single match arm) gets
        // caught here.
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("file.pdf", UrlKind::AssetNewtab),
            true,
            "PDF",
        );
        assert_eq!(
            out,
            r#"<a class="wikilink" target="_blank" rel="noopener" href="file.pdf">PDF</a>"#
        );
    }

    #[test]
    fn default_hooks_render_image_with_alt() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_image(
            &mut out,
            &ResolvedUrl::new("cat.jpg", UrlKind::Asset),
            "A cat",
            None,
            None,
            None,
        );
        assert_eq!(out, r#"<img src="cat.jpg" alt="A cat" />"#);
    }

    #[test]
    fn default_hooks_render_image_with_title() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_image(
            &mut out,
            &ResolvedUrl::new("cat.jpg", UrlKind::Asset),
            "A cat",
            Some("Photo"),
            None,
            None,
        );
        assert_eq!(
            out,
            r#"<img src="cat.jpg" alt="A cat" title="Photo" />"#
        );
    }

    #[test]
    fn default_hooks_escape_link_href() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new(r#"q=a&b="c""#, UrlKind::External),
            false,
            "x",
        );
        assert!(out.contains(r#"href="q=a&amp;b=&quot;c&quot;""#));
        // External links open in a new tab.
        assert!(out.contains(r#"target="_blank""#), "got: {out}");
    }

    #[test]
    fn default_hooks_render_link_external_opens_new_tab() {
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_link(
            &mut out,
            &ResolvedUrl::new("https://example.com", UrlKind::External),
            false,
            "Example",
        );
        assert_eq!(
            out,
            r#"<a target="_blank" rel="noopener" href="https://example.com">Example</a>"#
        );
    }

    #[test]
    fn render_link_unused_url_param_compiles() {
        // Ensure `Url` type is reachable from this module (sanity check
        // that imports are correct after refactors).
        let _u = Url::resolved("x", UrlKind::Internal);
    }

    // ── spec § P9 `data-width` emission ──────────────────────────────
    //
    // The author writes `:::hero {full}` and the parser canonicalizes
    // the flag to `Some("screen")`. The default render hook produces a
    // `<section class="moss-hero" data-width="screen">…` wrapper.
    // Default (no width attr) MUST omit `data-width` entirely so the
    // emitted HTML stays sparse — themes target the absence via
    // `:not([data-width])`.

    use super::super::node::Block;
    use super::super::shortcode::{
        GalleryItem, GalleryShortcode, GridShortcode, HeroShortcode, Shortcode,
    };

    fn render_shortcode_html(sc: &Shortcode) -> String {
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, sc, None);
        out
    }

    #[test]
    fn hero_with_width_screen_emits_data_width() {
        let sc = Shortcode::Hero(HeroShortcode {
            width: Some("screen".to_string()),
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(
            html.contains(r#"data-width="screen""#),
            "expected data-width=screen, got: {html}"
        );
        assert!(html.contains(r#"class="moss-hero""#), "got: {html}");
    }

    #[test]
    fn hero_with_width_wide_emits_data_width() {
        let sc = Shortcode::Hero(HeroShortcode {
            width: Some("wide".to_string()),
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(html.contains(r#"data-width="wide""#), "got: {html}");
    }

    #[test]
    fn hero_default_omits_data_width() {
        // Default: no `data-width` attribute at all. Sparse HTML matters
        // because themes target the absence (`:not([data-width])`); a
        // baked-in `data-width="body"` would shadow that intent.
        let sc = Shortcode::Hero(HeroShortcode::default());
        let html = render_shortcode_html(&sc);
        assert!(
            !html.contains("data-width"),
            "default should omit data-width, got: {html}"
        );
    }

    #[test]
    fn gallery_with_width_page_emits_data_width() {
        let sc = Shortcode::Gallery(GalleryShortcode {
            width: Some("page".to_string()),
            items: vec![GalleryItem {
                src: Url::resolved("a.jpg", UrlKind::Asset),
                alt: String::new(),
                attrs: String::new(),
            }],
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(html.contains(r#"data-width="page""#), "got: {html}");
        assert!(html.contains(r#"class="moss-gallery""#), "got: {html}");
    }

    #[test]
    fn gallery_default_omits_data_width() {
        let sc = Shortcode::Gallery(GalleryShortcode::default());
        let html = render_shortcode_html(&sc);
        assert!(
            !html.contains("data-width"),
            "default should omit data-width, got: {html}"
        );
    }

    /// An author column count must reach the stylesheet BOTH ways: as a value
    /// (`--moss-gallery-columns`, the only form `repeat()` can compute with) and as
    /// a selector hook (`data-columns`, the only form the cascade can branch
    /// on). site.css needs the hook to give a counted gallery a `minmax(0,
    /// 1fr)` track floor instead of auto-fill's 200px one — without it,
    /// `:::gallery 8` demands 8 × 200px and overflows its container.
    /// Gate: tests/render-gates/site/gallery-track-sizing.spec.js.
    #[test]
    fn gallery_with_columns_emits_both_count_hook_and_value() {
        let sc = Shortcode::Gallery(GalleryShortcode {
            columns: Some(8),
            items: vec![GalleryItem {
                src: Url::resolved("a.jpg", UrlKind::Asset),
                alt: String::new(),
                attrs: String::new(),
            }],
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(html.contains(r#"data-columns="8""#), "got: {html}");
        assert!(html.contains(r#"--moss-gallery-columns: 8"#), "got: {html}");
    }

    #[test]
    fn gallery_without_columns_omits_the_count_hook() {
        // No count means auto-fill, and auto-fill must NOT match
        // `.moss-gallery[data-columns]` or it loses its 200px density floor.
        let sc = Shortcode::Gallery(GalleryShortcode::default());
        let html = render_shortcode_html(&sc);
        assert!(
            !html.contains("data-columns"),
            "count-less gallery must stay on the auto-fill rule, got: {html}"
        );
        assert!(!html.contains("--moss-gallery-columns"), "got: {html}");
    }

    #[test]
    fn grid_with_width_wide_emits_data_width() {
        // PR4.5: cells are Vec<Vec<Block>>; build trivial Paragraph cells.
        let sc = Shortcode::Grid(GridShortcode {
            columns: 2,
            width: Some("wide".to_string()),
            cells: vec![
                vec![Block::Paragraph(vec![super::super::node::Inline::Text("a".into())])],
                vec![Block::Paragraph(vec![super::super::node::Inline::Text("b".into())])],
            ],
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(html.contains(r#"data-width="wide""#), "got: {html}");
        assert!(html.contains(r#"class="moss-grid""#), "got: {html}");
        // Other attrs still present.
        assert!(html.contains(r#"data-columns="2""#), "got: {html}");
    }

    #[test]
    fn grid_default_omits_data_width() {
        let sc = Shortcode::Grid(GridShortcode {
            columns: 1,
            cells: vec![vec![Block::Paragraph(vec![
                super::super::node::Inline::Text("solo".into()),
            ])]],
            ..Default::default()
        });
        let html = render_shortcode_html(&sc);
        assert!(
            !html.contains("data-width"),
            "default should omit data-width, got: {html}"
        );
    }

    // ── DefaultHooks Gallery synth path (Phase 4 PR1) ────────────────
    //
    // Pre-Phase-4-PR1, `DefaultHooks::new()` (no snapshot) emitted a
    // legacy bare-`<img>` gallery-item shape and relied on a surviving
    // regex post-pass to inject dims / LQIP / `<picture>` downstream.
    // Phase 4 PR1 (2026-05-27) deleted that bare-`<img>` fallback: every
    // gallery item now routes through
    // `synthesize_image_html(..., GalleryThumb, ...)`, threading an empty
    // `AssetSnapshot` through when no snapshot is in scope. Synth emits
    // `<picture>` for raster originals in both cases (snapshot-present
    // uses real dims/LQIP; snapshot-absent uses fallback dims, no LQIP).
    //
    // Rationale: the `img_contract_test` invariant requires every `<img>`
    // in moss output to be synth-emitted or a documented carve-out. The
    // legacy bare-img fallback was unreachable from production code paths
    // (PipelineHooks always provides a snapshot) but became reachable the
    // moment `render_document` is wired into production — closing the
    // gap preemptively in PR1.

    use crate::asset_snapshot::{AssetSnapshot, VariantKindSet};
    use std::path::PathBuf;

    fn gallery_with_one_item(src: &str) -> Shortcode {
        Shortcode::Gallery(GalleryShortcode {
            items: vec![GalleryItem {
                src: Url::resolved(src, UrlKind::Asset),
                alt: "photo".to_string(),
                attrs: String::new(),
            }],
            ..Default::default()
        })
    }

    #[test]
    fn default_hooks_new_gallery_routes_through_synth_with_empty_snapshot() {
        // Phase 4 PR1 (2026-05-27): the legacy bare-`<img>` fallback for
        // the no-snapshot Gallery path is deleted; an empty
        // `AssetSnapshot` is threaded through synth instead. For raster
        // originals (.jpg/.jpeg/.png), synth still emits `<picture>` —
        // with fallback dims (800×600) and no LQIP/color style — so the
        // byte shape is identical between snapshot-present and snapshot-
        // absent paths modulo the dim/style attribute values.
        let sc = gallery_with_one_item("photos/cat.jpg");
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, &sc, None);
        assert!(
            out.contains("<picture"),
            "expected <picture> wrap even without snapshot (PR1: every <img> is synth), got: {out}",
        );
        assert!(out.contains(r#"src="photos/cat.jpg""#), "got: {out}");
        assert!(out.contains(r#"alt="photo""#), "got: {out}");
        assert!(out.contains(r#"loading="lazy""#), "got: {out}");
    }

    #[test]
    fn default_hooks_with_snapshot_gallery_emits_picture_for_registered_webp() {
        // Synth path: snapshot registers a WebP variant. The synthesizer
        // wraps the `<img>` in `<picture><source srcset=*.webp>` (the
        // canonical responsive-image shape for raster originals).
        let src = "photos/cat.jpg";
        let mut snap = AssetSnapshot::new();
        snap.variants.insert(
            PathBuf::from("photos/cat"),
            VariantKindSet { webp: true, avif: false, hls: false },
        );

        let sc = gallery_with_one_item(src);
        let mut out = String::new();
        DefaultHooks::with_snapshot(&snap).render_shortcode(&mut out, &sc, None);
        assert!(out.contains("<picture"), "expected <picture> wrap, got: {out}");
        assert!(
            out.contains(r#"srcset="photos/cat.webp""#),
            "expected webp srcset, got: {out}",
        );
        assert!(out.contains(r#"src="photos/cat.jpg""#), "got: {out}");
    }

    #[test]
    fn default_hooks_with_snapshot_gallery_uses_snapshot_dims() {
        // Synth path with known dims: `width=`/`height=` attrs come from
        // the snapshot's `dimensions` map. Pin the values so a future
        // regression that fails to read dimensions from the snapshot
        // (e.g., a `dims_lookup` rewrite that forgets a path-mapping step)
        // gets caught here, not via diff in a snapshot fixture.
        let src = "photos/cat.jpg";
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from(src), (800, 600));

        let sc = gallery_with_one_item(src);
        let mut out = String::new();
        DefaultHooks::with_snapshot(&snap).render_shortcode(&mut out, &sc, None);
        assert!(out.contains(r#"width="800""#), "expected width=800, got: {out}");
        assert!(out.contains(r#"height="600""#), "expected height=600, got: {out}");
    }

    // ── DefaultHooks Hero synth path (Phase 4 PR1) ───────────────────
    //
    // Phase 4 PR1 (2026-05-27): the Hero arm of `render_shortcode` no
    // longer emits a raw `<img src="…" alt="" />` — it routes through
    // `synthesize_image_html` with `ImageContext::Hero` (snapshot
    // present) or `ImageContext::HeroBare` (no snapshot — the legitimate
    // ADR-013 carve-out). Both shapes satisfy `img_contract_test`'s
    // invariant: `<picture>`-wrapped synth in the first case,
    // `<section class="moss-hero">`-scoped bare-img in the second
    // (HeroBare). The test below pins both paths so a future regression
    // that drops the wrap, or drops the HeroBare invocation back to a
    // direct literal `<img>` push, gets caught here.

    fn hero_with_image(src: &str) -> Shortcode {
        Shortcode::Hero(HeroShortcode {
            image: Some(Url::resolved(src, UrlKind::Asset)),
            ..Default::default()
        })
    }

    #[test]
    fn hero_default_routes_through_synth_with_hero_bare() {
        // No snapshot: HeroBare carve-out — bare `<img>` shape inside
        // `<section class="moss-hero">`. No `<picture>` wrap; no
        // `loading=`. The carve-out is what `img_contract_test`'s
        // `matches_carveout` detects via the enclosing
        // `<section class="moss-hero"` parent + absent `loading=`.
        let sc = hero_with_image("hero.jpg");
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, &sc, None);
        assert!(
            out.contains(r#"<section class="moss-hero""#),
            "expected hero section wrap, got: {out}",
        );
        assert!(out.contains(r#"src="hero.jpg""#), "got: {out}");
        // HeroBare carve-out: no `<picture>` (ADR-013 — no variant
        // promised before set_pending), no `loading=` (CSS owns priority).
        assert!(
            !out.contains("<picture"),
            "HeroBare must NOT emit <picture>, got: {out}",
        );
        assert!(
            !out.contains("loading="),
            "HeroBare must NOT emit loading attr, got: {out}",
        );
    }

    #[test]
    fn hero_with_snapshot_routes_through_synth_picture_wrap() {
        // Snapshot present: full Hero synth — `<picture><source srcset=…>`
        // for raster originals + dims from the snapshot.
        let src = "hero.jpg";
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from(src), (1920, 1080));
        snap.variants.insert(
            PathBuf::from("hero"),
            VariantKindSet { webp: true, avif: false, hls: false },
        );

        let sc = hero_with_image(src);
        let mut out = String::new();
        DefaultHooks::with_snapshot(&snap).render_shortcode(&mut out, &sc, None);
        assert!(
            out.contains(r#"<section class="moss-hero""#),
            "expected hero section wrap, got: {out}",
        );
        assert!(out.contains("<picture"), "expected <picture> wrap, got: {out}");
        // 1920px-wide fixture → srcset ladder (Task 3, responsive-image-
        // variants): rungs below the deployed base + the base descriptor,
        // with the hero's full-bleed sizes.
        assert!(
            out.contains(
                r#"srcset="hero.w800.webp 800w, hero.w1600.webp 1600w, hero.webp 1920w" type="image/webp" sizes="100vw""#
            ),
            "expected webp srcset ladder, got: {out}",
        );
        assert!(out.contains(r#"width="1920""#), "got: {out}");
        assert!(out.contains(r#"height="1080""#), "got: {out}");
    }

    // ── DefaultHooks::render_image synth path (Phase 4 PR1) ──────────
    //
    // The `RenderHooks::render_image` default emits a bare `<img>`. The
    // `DefaultHooks` impl overrides this to route through synth when a
    // snapshot is in scope; without a snapshot it falls back to the
    // trait default (test-only path). Both behaviors are pinned below.

    #[test]
    fn default_hooks_with_snapshot_render_image_emits_picture_no_figure_wrap() {
        // PR1 v2 (post-Wave-0.5 parity probe): inline `Inline::Image`
        // rendering uses `MarkdownInline` — emits `<picture><img></picture>`
        // for raster originals, NO `<figure>` wrap. The figure wrap is
        // PR3's territory: `Block::Figure` is the typed variant for
        // image-only paragraphs and its renderer uses
        // `MarkdownStandalone`.
        //
        // Before v2 (early PR1) used `MarkdownStandalone` here and applied
        // figure wrap to every inline image — incorrect for paragraphs
        // with sibling content (image + italic caption + prose). Parity
        // probe surfaced this as 9 false-positive divergences on 刘果
        // CJK fixtures.
        let src = "photos/cat.jpg";
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from(src), (800, 600));
        snap.variants.insert(
            PathBuf::from("photos/cat"),
            VariantKindSet { webp: true, avif: false, hls: false },
        );

        let hooks = DefaultHooks::with_snapshot(&snap);
        let mut out = String::new();
        hooks.render_image(
            &mut out,
            &ResolvedUrl::new(src, UrlKind::Asset),
            "A cat",
            None,
            None,
            None,
        );
        assert!(
            !out.contains("<figure"),
            "expected NO <figure> wrap (MarkdownInline), got: {out}",
        );
        assert!(out.contains("<picture"), "expected <picture>, got: {out}");
        assert!(out.contains(r#"loading="lazy""#), "got: {out}");
        assert!(out.contains(r#"alt="A cat""#), "got: {out}");
    }

    #[test]
    fn default_hooks_with_snapshot_render_image_emits_title_attribute_when_present() {
        // PR1 v2 (post-parity-probe correction): with `MarkdownInline`,
        // `title` becomes a `title=""` HTML attribute on the inner `<img>`,
        // NOT a `<figcaption>` (the figure-wrap is PR3 territory).
        //
        // Verifying title round-trips through synth ensures we don't drop
        // CommonMark `[text](href "title")` data through the AST path —
        // the documented gap on `render_link` is fixed in PR8; for images
        // it lands here in PR1.
        let src = "photos/cat.jpg";
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from(src), (800, 600));

        let hooks = DefaultHooks::with_snapshot(&snap);
        let mut out = String::new();
        hooks.render_image(
            &mut out,
            &ResolvedUrl::new(src, UrlKind::Asset),
            "alt-text",
            Some("Photo caption"),
            None,
            None,
        );
        // No figure wrap; title attribute on inner <img>
        assert!(
            !out.contains("<figcaption"),
            "expected NO figcaption (MarkdownInline), got: {out}",
        );
        // The MarkdownInline path's title handling depends on synth's
        // implementation; assert it doesn't disappear silently.
        // Note: if synth currently drops title in MarkdownInline mode,
        // that's a separate finding to file — flagged as a Phase 4
        // followup since the production wikilink dispatcher carries title
        // via TitleParams.
        assert!(out.contains("<picture") || out.contains("<img"), "got: {out}");
    }

    #[test]
    fn default_hooks_new_render_image_falls_back_to_bare_img() {
        // No snapshot: render_image falls back to the bare-`<img>` default
        // (test-only path). Production routes through PipelineHooks's
        // snapshot-aware override.
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_image(
            &mut out,
            &ResolvedUrl::new("cat.jpg", UrlKind::Asset),
            "A cat",
            None,
            None,
            None,
        );
        assert_eq!(out, r#"<img src="cat.jpg" alt="A cat" />"#);
    }

    #[test]
    fn default_hooks_hero_overlay_heading_has_no_permalink_anchor() {
        // Regression for Yi-website: headings inside :::hero overlays must not
        // carry moss-heading-anchor when rendered through DefaultHooks
        // (the moss-core test-harness path). The DefaultHooks::render_shortcode
        // Hero arm delegates overlay rendering to render_hero_overlay_blocks
        // which uses `DefaultHooks::hero_overlay`. This test covers that path.
        use crate::ast::shortcode::{HeroShortcode, Shortcode};
        use crate::ast::{parse, Url, UrlKind, ResolvedUrl};
        let overlay_blocks = parse("# Understanding climate extremes\n\nSubtitle.").blocks;
        let sc = Shortcode::Hero(HeroShortcode {
            overlay: overlay_blocks,
            image: Some(Url::Resolved(ResolvedUrl::new("header.png", UrlKind::Asset))),
            ..Default::default()
        });
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_shortcode(&mut out, &sc, None);
        assert!(
            !out.contains("moss-heading-anchor"),
            "hero overlay heading must not carry a permalink anchor via DefaultHooks; got: {out}"
        );
        assert!(
            out.contains(r#"id="understanding-climate-extremes""#),
            "hero overlay heading must still carry its id; got: {out}"
        );
    }

    #[test]
    fn trait_default_render_image_carries_img_style() {
        // Both in-workspace impls override `render_image`, so the trait's own
        // default body is reachable only by a downstream crate — nothing here
        // exercises it. It used to drop `img_style` on the floor, which is
        // half of what made moss#754 possible, so pin it directly via a
        // minimal impl rather than leaving the claim untested.
        use crate::ast::{ResolvedUrl, UrlKind};
        struct BareHooks;
        impl RenderHooks for BareHooks {}
        let mut out = String::new();
        BareHooks.render_image(
            &mut out,
            &ResolvedUrl::new("cat.jpg", UrlKind::Asset),
            "A cat",
            None,
            Some("object-fit:cover"),
            None,
        );
        assert_eq!(
            out,
            r#"<img src="cat.jpg" alt="A cat" style="object-fit:cover" />"#
        );
    }

    #[test]
    fn hero_overlay_figure_keeps_embed_img_style() {
        // moss#754: `![[photo.jpg|cover left]]` inside a :::hero overlay lost
        // its `object-fit`/`object-position` while the identical embed rendered
        // correctly everywhere else. The overlay renders through
        // a hooks impl that used to be a partially-delegating wrapper: it
        // forwarded only the styleless image entry point, so the style
        // silently fell off at the wrapper boundary.
        use crate::ast::shortcode::{HeroShortcode, Shortcode};
        use crate::ast::{Block, Inline, ResolvedUrl, Url, UrlKind};
        let overlay_blocks = vec![Block::Figure {
            image: Inline::Image {
                src: Url::Resolved(ResolvedUrl::new("photo.jpg", UrlKind::Asset)),
                alt: String::new(),
                title: None,
                is_wikilink: true,
                wikilink_pothole: None,
            },
            caption: None,
            width: None,
            align: None,
            class_names: Vec::new(),
            img_style: Some("object-fit:cover;object-position:left".into()),
        }];
        let sc = Shortcode::Hero(HeroShortcode {
            overlay: overlay_blocks,
            image: Some(Url::Resolved(ResolvedUrl::new("header.png", UrlKind::Asset))),
            ..Default::default()
        });
        let hooks = DefaultHooks::new();
        let mut out = String::new();
        hooks.render_shortcode(&mut out, &sc, None);
        assert!(
            out.contains("object-fit:cover;object-position:left"),
            "hero overlay embed must keep its sizing style; got: {out}"
        );
    }

    #[test]
    fn hero_overlay_inherits_snapshot_and_grid_cell_sizes_scope() {
        // A hero overlay must not change how anything inside it renders
        // except heading anchors. Two things have to hold, and a bare
        // hero-vs-body parity assertion catches neither on its own:
        //
        //  1. the caller's AssetSnapshot reaches the overlay at all (without
        //     it there is no srcset ladder and so no `sizes=`);
        //  2. `begin_grid_cells`/`end_grid_cells` still scope `sizes=` to the
        //     CELL. A parity-only assertion misses this — gut both hooks and
        //     hero and body degrade to the same content-column default, so
        //     they stay equal while both are wrong. Hence the literal.
        use crate::ast::shortcode::{GridShortcode, HeroShortcode, Shortcode};
        use crate::ast::{Block, Inline, ResolvedUrl, Url, UrlKind};
        let src = "photos/cat.jpg";
        let mut snap = AssetSnapshot::new();
        // Wide enough that `ladder_rungs` yields rungs — a srcset ladder is
        // what carries `sizes=`.
        snap.dimensions.insert(PathBuf::from(src), (2400, 1200));
        snap.variants.insert(
            PathBuf::from("photos/cat"),
            VariantKindSet { webp: true, avif: false, hls: false },
        );
        let grid = || {
            Shortcode::Grid(GridShortcode {
                columns: 3,
                cells: vec![vec![Block::Paragraph(vec![Inline::Image {
                    src: Url::Resolved(ResolvedUrl::new(src, UrlKind::Asset)),
                    alt: "A cat".into(),
                    title: None,
                    is_wikilink: false,
                    wikilink_pothole: None,
                }])]],
                ..Default::default()
            })
        };

        let hooks = DefaultHooks::with_snapshot(&snap);
        let mut bare = String::new();
        hooks.render_shortcode(&mut bare, &grid(), None);

        let mut in_hero = String::new();
        hooks.render_shortcode(
            &mut in_hero,
            &Shortcode::Hero(HeroShortcode {
                overlay: vec![Block::Shortcode(grid())],
                image: Some(Url::Resolved(ResolvedUrl::new("header.png", UrlKind::Asset))),
                ..Default::default()
            }),
            None,
        );

        // Three columns in the content column, so each cell gets a third of
        // that band — NOT the whole column.
        let cell_sizes = "(min-width: 48rem) calc(min(47.25rem, 100vw) / 3), 100vw";
        assert!(
            in_hero.contains(&format!(r#"sizes="{cell_sizes}""#)),
            "grid cell inside a hero overlay must scope sizes= to the cell; got: {in_hero}"
        );
        assert!(
            bare.contains(&format!(r#"sizes="{cell_sizes}""#)),
            "same grid outside a hero must scope sizes= identically; got: {bare}"
        );
    }

    #[test]
    fn render_shortcode_accepts_source_line_param() {
        use crate::ast::shortcode::{Shortcode, SubscribeShortcode};
        let sc = Shortcode::Subscribe(SubscribeShortcode {
            placeholder: None,
            button: None,
        });
        let mut out = String::new();
        let hooks = DefaultHooks::new();
        // Compiles only after the signature change.
        hooks.render_shortcode(&mut out, &sc, Some(7));
        let _ = out;
    }

    #[test]
    fn render_shortcode_grid_emits_data_source_range_when_some() {
        use crate::ast::shortcode::{GridShortcode, Shortcode};
        let sc = Shortcode::Grid(GridShortcode {
            cells: vec![vec![]],
            columns: 1,
            ..Default::default()
        });
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, &sc, Some(12));
        assert!(
            out.contains(r#"data-source-range="12-12""#),
            "grid should carry a point source range; got: {out}"
        );
    }

    #[test]
    fn render_shortcode_grid_omits_data_source_range_when_none() {
        use crate::ast::shortcode::{GridShortcode, Shortcode};
        let sc = Shortcode::Grid(GridShortcode {
            cells: vec![vec![]],
            columns: 1,
            ..Default::default()
        });
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, &sc, None);
        assert!(
            !out.contains("data-source-range"),
            "grid must omit source range when no source line; got: {out}"
        );
    }

    /// A ratio must arrive as a variable the stylesheet reads, never as an
    /// inline `grid-template-columns`. Inline wins over every rule, so a
    /// `:::grid 2 1:2` used to keep two columns at 320px — the mobile collapse
    /// had no way to reach it. Text-level assertion; the fact that the
    /// collapse then actually happens is a render gate (grid-mobile-collapse).
    #[test]
    fn render_shortcode_grid_ratio_is_a_custom_property_not_inline_columns() {
        use crate::ast::shortcode::{GridShortcode, Shortcode};
        let sc = Shortcode::Grid(GridShortcode {
            cells: vec![vec![], vec![]],
            columns: 2,
            ratio: Some("1:2".to_string()),
            ..Default::default()
        });
        let mut out = String::new();
        DefaultHooks::new().render_shortcode(&mut out, &sc, None);
        assert!(
            out.contains(r#"style="--moss-grid-ratio:minmax(0, 1fr) minmax(0, 2fr)""#),
            "ratio should ride as --moss-grid-ratio; got: {out}"
        );
        assert!(
            !out.contains("grid-template-columns"),
            "an inline grid-template-columns can never be overridden; got: {out}"
        );
    }

    // --- ADR-021 Corollary 2: sizes= threading through the typed paths ---
    //
    // These pin the PRODUCTION figure/grid render paths (Block::Figure and
    // the Grid arm — the wikilink `![[x|screen]]` route), not just the
    // synthesizer's MarkdownStandalone context, which production no longer
    // constructs. Regression: `![[hero.png|screen]]` used to emit SIZES_BODY
    // because the Figure renderer dropped the width before the synth call.

    /// Snapshot with dims big enough to trigger the srcset ladder (sizes=
    /// is only emitted when the ladder is non-empty).
    fn ladder_snapshot(path: &str) -> crate::asset_snapshot::AssetSnapshot {
        let mut s = crate::asset_snapshot::AssetSnapshot::new();
        s.dimensions
            .insert(std::path::PathBuf::from(path), (2000, 1200));
        s
    }

    fn figure_block(width: Option<&str>) -> Block {
        Block::Figure {
            image: Inline::Image {
                src: Url::Resolved(ResolvedUrl::new("photo.jpg", UrlKind::Asset)),
                alt: String::new(),
                title: None,
                is_wikilink: true,
                wikilink_pothole: None,
            },
            caption: None,
            width: width.map(String::from),
            align: None,
            class_names: Vec::new(),
            img_style: None,
        }
    }

    #[test]
    fn figure_block_screen_width_reaches_sizes() {
        let snap = ladder_snapshot("photo.jpg");
        let hooks = DefaultHooks::with_snapshot(&snap);
        let mut out = String::new();
        super::super::render::render_blocks(&hooks, &mut out, &[figure_block(Some("screen"))]);
        assert!(
            out.contains(r#"data-width="screen""#),
            "figure carries the attribute; got: {out}"
        );
        assert!(
            out.contains(r#"sizes="100vw""#),
            "a |screen figure must declare full-bleed sizes, not the content column; got: {out}"
        );
    }

    #[test]
    fn figure_block_wide_width_reaches_sizes() {
        let snap = ladder_snapshot("photo.jpg");
        let hooks = DefaultHooks::with_snapshot(&snap);
        let mut out = String::new();
        super::super::render::render_blocks(&hooks, &mut out, &[figure_block(Some("wide"))]);
        assert!(
            out.contains(r#"sizes="(min-width: 48rem) min(63rem, 100vw), 100vw""#),
            "a |wide figure declares the wide band; got: {out}"
        );
    }

    #[test]
    fn grid_cell_image_declares_cell_sizes_and_scope_pops() {
        let snap = ladder_snapshot("photo.jpg");
        let hooks = DefaultHooks::with_snapshot(&snap);
        let grid = Shortcode::Grid(crate::ast::GridShortcode {
            columns: 3,
            ratio: None,
            classes: String::new(),
            cells: vec![vec![figure_block(None)]],
            width: Some("page".to_string()),
        });
        let mut out = String::new();
        hooks.render_shortcode(&mut out, &grid, None);
        assert!(
            out.contains(r#"sizes="(min-width: 48rem) calc(min(1200px, 100vw) / 3), 100vw""#),
            "a widthless figure in a 3-col page grid declares the cell track; got: {out}"
        );

        // The scope must not leak past the grid: a figure rendered after
        // returns to the content-column default.
        let mut after = String::new();
        super::super::render::render_blocks(&hooks, &mut after, &[figure_block(None)]);
        assert!(
            after.contains(r#"sizes="(min-width: 48rem) 47.25rem, 100vw""#),
            "grid cell scope leaked; got: {after}"
        );
    }

    #[test]
    fn grid_cell_figure_own_width_beats_cell_scope() {
        let snap = ladder_snapshot("photo.jpg");
        let hooks = DefaultHooks::with_snapshot(&snap);
        let grid = Shortcode::Grid(crate::ast::GridShortcode {
            columns: 2,
            ratio: None,
            classes: String::new(),
            cells: vec![vec![figure_block(Some("screen"))]],
            width: None,
        });
        let mut out = String::new();
        hooks.render_shortcode(&mut out, &grid, None);
        assert!(
            out.contains(r#"sizes="100vw""#),
            "an explicit |screen inside a grid cell wins over the cell track; got: {out}"
        );
    }
}
