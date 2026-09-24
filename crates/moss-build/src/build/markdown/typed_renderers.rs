//! Hero rendering + the link-preview helpers shared with
//! `build::render::grid_cells`.
//!
//! Phase 4 PR4.5 (2026-05-28) deleted the Grid-rendering surface from
//! this file. `Shortcode::Grid::cells` is now `Vec<Vec<Block>>` (parsed
//! at extract time in `moss_core::ast::shortcode_extract::parse_grid`),
//! URLs are resolved by `resolve_shortcode_urls` in `pipeline.rs`, and
//! the AST-level `DefaultHooks::render_shortcode` Grid arm (called via
//! `PipelineHooks::render_shortcode` delegation) emits production HTML.
//! Compound-link cells flow through the typed `Block::LinkCard { url,
//! children }` variant introduced in PR4.5.
//!
//! What remains here:
//! - [`render_hero_html_typed`] — Hero rendering. Called from the
//!   hoisting branch of `apply_typed_shortcodes`; the rendered HTML
//!   travels separately to the article template's hero slot.
//! - [`extract_domain`], [`render_link_preview`], [`format_data_width`],
//!   [`format_class_attr`] — the byte shapes
//!   `build::render::grid_cells` emits for a grid cell that turns out to
//!   be an outbound link.
//!
//! An `<a href=…>`-matching regex (`LINK_RE`) used to live here for
//! `grid_post.rs`'s post-HTML scanning. That file was deleted: which
//! cells are links is read off the typed `Vec<Block>` the serializer
//! rendered from, so nothing re-matches emitted anchors.

/// Extract the registrable domain from a URL for link-preview labeling.
///
/// Strips scheme, `www.`, and any path/query/fragment. `"https://www.foo.com/bar"`
/// → `"foo.com"`. Used by both this file's `render_compound_link_cell`
/// (compound-link cells inside grids) and `crate::build::render::grid_cells`
/// (the cell that resolved to an outbound link).
pub(crate) fn extract_domain(url: &str) -> String {
    url.trim_start_matches("https://")
       .trim_start_matches("http://")
       .trim_start_matches("www.")
       .split('/')
       .next()
       .unwrap_or(url)
       .to_string()
}

/// Render a link-preview card: title row (when known) + favicon/domain row.
///
/// `title` is the page's `og:title` or `<title>`. When absent, the card
/// collapses to a single `[favicon] domain.com` row. The raw URL is never
/// used as a title fallback — that would print the same string twice.
///
/// Description and hero image are intentionally not rendered: a personal
/// site's link list is reading-recommendation territory, not a social-media
/// preview surface. Less chrome, more legible.
pub(crate) fn render_link_preview(href: &str, title: Option<&str>, domain: &str, favicon: Option<&str>) -> String {
    use crate::build::media::cover::html_escape;
    let title_html = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!(r#"<span class="link-preview-title">{}</span>"#, html_escape(t)))
        .unwrap_or_default();
    // Step 4 of structural-html-emission: route the favicon through the
    // synthesizer with ImageContext::Favicon, which short-circuits to a
    // bare 16×16 <img> (no manifest, no <picture>, no LQIP). Output is
    // byte-identical to the prior inline format!() call; the routing is
    // architectural so every <img> in moss output now flows through one
    // function.
    //
    // Phase 1 B1 (2026-05-25): the synthesizer takes `&AssetSnapshot` after
    // the data-source switch. `Favicon` short-circuits before any snapshot
    // probe, so an empty snapshot is correct.
    let favicon_assets = moss_core::asset_snapshot::AssetSnapshot::new();
    let favicon_html = favicon
        .filter(|f| !f.is_empty())
        .map(|f| moss_core::render::image::synthesize_image_html(
            f,
            "",
            &favicon_assets,
            moss_core::render::image::ImageContext::Favicon,
            &moss_core::render::image::ImageRenderOptions {
                class: Some("link-preview-favicon"),
                ..Default::default()
            },
        ))
        .unwrap_or_default();
    format!(
        r#"<a href="{}" class="moss-grid-card link-preview" target="_blank" rel="noopener">{}<span class="link-preview-domain">{}{}</span></a>"#,
        html_escape(href), title_html, favicon_html, html_escape(domain)
    )
}

/// Format a CSS class attribute with a base class and optional extra classes.
///
/// - `("moss-grid", "profiles featured")` → `class="moss-grid profiles featured"`
/// - `("moss-grid", "")` → `class="moss-grid"`
fn format_class_attr(base: &str, extra: &str) -> String {
    if extra.is_empty() {
        format!(r#"class="{}""#, base)
    } else {
        format!(r#"class="{} {}""#, base, extra)
    }
}

/// Format the spec § P9 `data-width` attribute fragment (with leading space)
/// or the empty string when the author did not set a width.
///
/// `None` → `""` so the wrapper's HTML stays sparse and themes can target
/// the absence (`.moss-hero:not([data-width])`).
/// `Some("screen")` → ` data-width="screen"`.
///
/// The value is trusted (already canonicalized to one of body | wide | page
/// | screen by `match_width_token` in the attrs parser). The function does
/// not validate or escape — feeding an unsanitized value here would be a
/// programming error.
pub(crate) fn format_data_width(width: Option<&str>) -> String {
    match width {
        Some(w) => format!(r#" data-width="{}""#, w),
        None => String::new(),
    }
}

/// Whether a hero media href names a video (the parser's media grammar
/// accepts video extensions; a video background renders as an ambient
/// loop — `autoplay muted loop playsinline`, no controls — through the
/// same synthesizer as `![[clip.mp4|loop]]`).
fn hero_href_is_video(href: &str) -> bool {
    let ext = href.rsplit('.').next().unwrap_or("");
    matches!(
        moss_core::resolve::ext_kind::reference_kind_for_ext(ext),
        moss_core::resolve::ext_kind::ExtKind::Video
    )
}

/// Ambient-loop params for hero background videos.
fn hero_video_params() -> moss_core::resolve::title_params::TitleParams {
    let mut p = moss_core::resolve::title_params::TitleParams::default();
    p.params.insert("loop".to_string(), "1".to_string());
    p
}

/// Render the typed [`HeroShortcode`] AST node to HTML.
///
/// Mirrors [`render_hero_html`] but reads from the typed-AST representation
/// (post-[`crate::build::markdown::pipeline::resolve_shortcode_urls`] —
/// `image` is `Url::Resolved`). Used by `apply_typed_shortcodes` for the
/// :::hero block once Step 2 of the unified-grammar migration moves Hero
/// into the typed AST.
///
/// Click-to-source: when `source_line` is `Some(n)`, the outermost
/// `<section class="moss-hero">` carries `data-source-range="n-n"` (a point
/// range) so the editor bridge's `resolveSourceTarget` routes a click back
/// to the `:::hero` line. `None` for callers without a source position
/// (parity probes, fragment renders).
pub(crate) fn render_hero_html_typed<R: Fn(&str) -> String>(
    args: &moss_core::ast::HeroShortcode,
    // Phase 4 PR4.5 (2026-05-28): Hero overlay is now typed
    // `Vec<Block>` with URLs already resolved by
    // `resolve_shortcode_urls`. The resolver closure isn't read here
    // anymore, but the parameter is kept to preserve the public
    // signature for callers and parity probes.
    _resolve_link: &R,
    // Step 3 of structural-html-emission: when `Some`, the hero's inline
    // `<img>` is synthesized via `image_render::synthesize_image_html` with
    // `ImageContext::Hero` (and `eager: true` so the hero gets
    // `loading="eager" fetchpriority="high"`). When `None`, falls back to
    // the raw `<img>` emission that the regex pass picks up — preserves
    // behavior for test/fragment-render contexts without a media manifest.
    media_lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    // Click-to-source: when `Some(n)`, the hero's outermost `<section>`
    // carries `data-source-range="n-n"` (a point range) so the editor
    // bridge's `resolveSourceTarget` routes clicks back to the `:::hero`
    // line. `None` for callers that don't carry source positions.
    source_line: Option<usize>,
    dominant_color: Option<&str>,
) -> String {
    use crate::build::media::cover::html_escape;

    let resolved_image: Option<String> = match &args.image {
        Some(moss_core::ast::Url::Resolved(r)) => Some(r.href.clone()),
        Some(moss_core::ast::Url::Unresolved(s)) => {
            debug_assert!(
                false,
                "Url::Unresolved({s:?}) reached render_hero_html_typed \
                 — visit_urls_mut missing for Hero"
            );
            Some(s.clone())
        }
        None => None,
    };

    let attrs = moss_core::media::parse_media_attrs(&args.attrs);
    let class_attr = format_class_attr("moss-hero", &args.classes);
    let width_attr = format_data_width(args.width.as_deref());
    // `:::hero {.plate}` (2026-09-11): whole-image, never-upscaled variant.
    // Detected from the raw class list so authors never spell `data-fit`
    // themselves; see `moss_core::render::image::hero_is_plate`.
    let is_plate = moss_core::render::image::hero_is_plate(&args.classes);
    let plate_attr = if is_plate {
        r#" data-fit="plate""#
    } else {
        ""
    };
    let source_range_attr = match source_line {
        Some(n) => format!(r#" data-source-range="{n}-{n}""#),
        None => String::new(),
    };
    let mobile_attr = if args.mobile.as_deref() == Some("overlay") {
        r#" data-mobile="overlay""#.to_string()
    } else {
        String::new()
    };
    // A pale hero photo needs a stronger legibility scrim than a mid-tone or
    // dark one — the same gradient that carries white type over a dusk photo
    // washes out over a watercolour. Classify from the scan-cached dominant
    // colour so CSS can pick the right ramp. Only heroes carrying overlay text
    // get the attribute; the scrim is gated on that anyway.
    //
    // A multi-slide hero is classified from its primary image alone: the
    // attribute sits on the <section>, so one ramp covers every crossfaded
    // slide. Deliberate — per-slide scrims would need the ::before split into
    // per-slide layers, and a mixed-tone slideshow is rare enough that the
    // first slide is the honest proxy.
    let tone_attr = if args.overlay.is_empty() {
        String::new()
    } else {
        resolved_image
            .as_deref()
            .zip(media_lookup)
            .and_then(|(href, lookup)| {
                let key = href.strip_prefix('/').unwrap_or(href);
                lookup.get_dominant_color(key)
            })
            .and_then(|raw| crate::build::components::color_extract::is_light_cover(&raw))
            .filter(|&light| light)
            .map(|_| r#" data-hero-tone="light""#.to_string())
            .unwrap_or_default()
    };
    let mobile_style_attr = if args.mobile.as_deref() != Some("overlay") {
        dominant_color
            .map(|c| format!(
                r#" data-cover-color style="--moss-cover-color: {}""#,
                html_escape(c)
            ))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let style = attrs
        .to_inline_style()
        .map(|s| format!(" style=\"{}\"", html_escape(&s)))
        .unwrap_or_default();

    // Build the asset snapshot once when a media lookup is available. Both the
    // image synthesizer (img_part) and the overlay renderer need it; building
    // once avoids iterating the full media manifest twice per hero render.
    let media_assets = media_lookup
        // BUG 6: index under the lookup's real `dir_overrides` so a hero image
        // under an overridden dir resolves to its output-URL key and gets real
        // dims instead of the 800x600 fallback.
        .map(|lookup| lookup.asset_snapshot());

    // Resolve the extra slides (multi-image hero). Ambient rotation is
    // capped at 6 slides — the CSS timing table covers 2..=6, and a hero
    // carrying more is beyond "interchangeable mood".
    let mut extra_hrefs: Vec<String> = Vec::new();
    for image in &args.extra_images {
        match image {
            moss_core::ast::Url::Resolved(r) => extra_hrefs.push(r.href.clone()),
            moss_core::ast::Url::Unresolved(u) => {
                debug_assert!(
                    false,
                    "Url::Unresolved({u:?}) reached render_hero_html_typed \
                     — visit_urls_mut missing for Hero extra_images"
                );
                extra_hrefs.push(u.clone());
            }
        }
    }
    if extra_hrefs.len() > 5 {
        log::warn!(
            "hero: {} background slides — ambient crossfade caps at 6, dropping the rest",
            extra_hrefs.len() + 1
        );
        extra_hrefs.truncate(5);
    }

    let img_part = match (&resolved_image, media_lookup) {
        (Some(href), _) if hero_href_is_video(href) => {
            let empty_snapshot = moss_core::asset_snapshot::AssetSnapshot::new();
            let assets = media_assets.unwrap_or(&empty_snapshot);
            moss_core::render::video::synthesize_video_html(
                &hero_video_params(),
                &moss_core::media::Placement::default(),
                href,
                assets,
            )
        }
        (Some(href), Some(_lookup)) => {
            // Route through synthesizer: gives dims, LQIP, optional
            // <picture><source srcset>, plus loading=eager + fetchpriority.
            // The hero's inline `style="object-fit:..."` from MediaAttrs is
            // passed via extra_attrs so it lands AFTER alt, matching the
            // pre-Step-3 byte shape (regex pass preserved trailing attrs).
            let extra = if style.is_empty() {
                None
            } else {
                // style already starts with a space — strip it so the
                // synthesizer's extra_attrs joiner adds exactly one.
                Some(style.trim_start().to_string())
            };
            let opts = moss_core::render::image::ImageRenderOptions {
                eager: true,
                extra_attrs: extra.as_deref(),
                ..Default::default()
            };
            let assets = media_assets.expect("media_lookup is Some so assets is Some");
            moss_core::render::image::synthesize_image_html(
                href,
                "",
                &assets,
                moss_core::render::image::ImageContext::Hero { plate: is_plate },
                &opts,
            )
        }
        (Some(href), None) => {
            // Phase 2E PR2 (2026-05-26): route through the synthesizer's
            // `HeroBare` carve-out. The byte shape is identical to the
            // legacy `format!` above — `<img src="X" alt=""[ STYLE] />`
            // — but the path through `synthesize_image_html` lets us
            // retire the regex post-pass in a follow-up PR3 once every
            // emitter routes through the synthesizer. The asset-publish
            // invariant rules out `ImageContext::Hero` for this branch:
            // no `MediaDimensionLookup` means no AssetRegistry has been
            // primed for the source's `.webp` companion, and emitting a
            // `<source srcset="*.webp">` against an unregistered URL
            // would 404 in preview mode (see `.claude/CLAUDE.md` §
            // "Asset publish invariant").
            //
            // The inline `style` fragment (already ` style="..."` with a
            // leading space + escaped) is threaded via `extra_attrs`,
            // which the synthesizer joins with a single leading space
            // — strip the existing leading space first to avoid a
            // double space.
            let extra = if style.is_empty() {
                None
            } else {
                Some(style.trim_start().to_string())
            };
            let opts = moss_core::render::image::ImageRenderOptions {
                extra_attrs: extra.as_deref(),
                ..Default::default()
            };
            let empty_snapshot = moss_core::asset_snapshot::AssetSnapshot::new();
            moss_core::render::image::synthesize_image_html(
                href,
                "",
                &empty_snapshot,
                moss_core::render::image::ImageContext::HeroBare,
                &opts,
            )
        }
        (None, _) => String::new(),
    };

    // Multi-image hero: wrap every slide (primary first) and mark the
    // section with data-slides so site.css drives the crossfade. A single
    // image keeps today's exact byte shape.
    let (img_part, slides_attr) = if extra_hrefs.is_empty() || resolved_image.is_none() {
        (img_part, String::new())
    } else {
        // The slides wrapper is the positioning context: absolute slides
        // fill IT (the image box), never the whole section — which is what
        // keeps the mobile stacked layout's text below the image safe.
        let mut slides = format!(
            r#"<div class="moss-hero-slides"><div class="moss-hero-slide">{img_part}</div>"#
        );
        // The primary's frame-level style (object-fit/position pipe attrs)
        // applies to every IMAGE slide so the crossfade set renders
        // uniformly; the video synthesizer has no extra-attrs channel, so
        // video slides keep the CSS default (cover).
        let extra_style = if style.is_empty() {
            None
        } else {
            Some(style.trim_start().to_string())
        };
        for href in &extra_hrefs {
            if hero_href_is_video(href) {
                let empty_snapshot = moss_core::asset_snapshot::AssetSnapshot::new();
                let assets = media_assets.unwrap_or(&empty_snapshot);
                let synth = moss_core::render::video::synthesize_video_html(
                    &hero_video_params(),
                    &moss_core::media::Placement::default(),
                    href,
                    assets,
                );
                slides.push_str(&format!(r#"<div class="moss-hero-slide">{synth}</div>"#));
                continue;
            }
            let opts = moss_core::render::image::ImageRenderOptions {
                extra_attrs: extra_style.as_deref(),
                ..Default::default()
            };
            let synth = match media_lookup {
                Some(_) => {
                    let assets =
                        media_assets.expect("media_lookup is Some so assets is Some");
                    moss_core::render::image::synthesize_image_html(
                        href,
                        "",
                        assets,
                        moss_core::render::image::ImageContext::Hero { plate: is_plate },
                        &opts,
                    )
                }
                None => {
                    let empty_snapshot = moss_core::asset_snapshot::AssetSnapshot::new();
                    moss_core::render::image::synthesize_image_html(
                        href,
                        "",
                        &empty_snapshot,
                        moss_core::render::image::ImageContext::HeroBare,
                        &opts,
                    )
                }
            };
            slides.push_str(&format!(r#"<div class="moss-hero-slide">{synth}</div>"#));
        }
        slides.push_str("</div>");
        (slides, format!(r#" data-slides="{}""#, extra_hrefs.len() + 1))
    };

    // A caption goes BELOW the hero, as a sibling of the `<section>`, never
    // inside it. Two reasons, both structural rather than stylistic. The
    // section is a fixed-height cropping frame (`max-height` + `overflow:
    // hidden`), so anything laid out in its flow is liable to be cut off; and
    // `.moss-hero ~ main` in site.css reaches past the hero to the article, a
    // general-sibling selector that keeps working with a caption between them
    // but would not if the hero were wrapped.
    //
    // `data-captioned` on the section is the hook for the other half of the
    // promise: a hero that names its subject must not crop the subject out.
    // See the `[data-captioned]` rules in site.css.
    let (caption_attr, caption_html) = if args.caption.is_empty() {
        (String::new(), String::new())
    } else {
        (
            " data-captioned".to_string(),
            format!(
                r#"<p class="moss-hero-caption">{}</p>"#,
                crate::build::render::credits::render_display_row(&args.caption)
            ),
        )
    };

    if args.overlay.is_empty() {
        format!(
            "<section {}{}{}{}{}{}{}{}>{}</section>{}",
            class_attr, slides_attr, width_attr, source_range_attr, mobile_attr,
            mobile_style_attr, plate_attr, caption_attr, img_part, caption_html
        )
    } else {
        // Phase 4 PR4.5 (2026-05-28): overlay is now typed `Vec<Block>`.
        // Render via `moss_core::ast::render::render_blocks` with a
        // `DefaultHooks::with_snapshot(...)` so inner inline `Inline::Image`
        // emissions route through synth (production `<picture>` shape).
        //
        // The resolver doesn't need threading through here — overlay URLs
        // were already classified by `resolve_shortcode_urls` in
        // `pipeline.rs` (which descends into `args.overlay` via
        // `resolve_urls_in_block`). The renderer reads `Url::Resolved`
        // directly.
        //
        // `_resolve_link` is retained in the signature to preserve the
        // public API for downstream callers; PR4.5 leaves Hero as a
        // hoisted shortcode rendered out of band (not via
        // `render_document` proper), so the parameter pays the cost of
        // an unused-arg until PR7a flips production to `render_document`.
        let mut html_content = String::new();
        let hooks = moss_core::ast::DefaultHooks::hero_overlay(media_assets);
        moss_core::ast::render::render_blocks(&hooks, &mut html_content, &args.overlay);
        // Collapse tag-adjacent newlines to match production push_html
        // byte shape (no `>\n<`). Inside text content, embedded newlines
        // are preserved.
        let collapsed = moss_core::ast::hooks::collapse_tag_adjacent_newlines(&html_content);
        format!(
            "<section {}{}{}{}{}{}{}{}{}>{}<div class=\"moss-hero-content\">{}</div></section>{}",
            class_attr,
            slides_attr,
            width_attr,
            source_range_attr,
            mobile_attr,
            mobile_style_attr,
            tone_attr,
            plate_attr,
            caption_attr,
            img_part,
            collapsed.trim(),
            caption_html
        )
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A cover credit has to survive as text below the picture. Printing it
    /// over the photograph — the only thing the overlay could do with it —
    /// makes it unreadable and, for a photographer's name, wrong. So the
    /// caption is a sibling of the section, after it, and the section is
    /// marked so the stylesheet can stop cropping the subject out.
    #[test]
    fn hero_caption_renders_below_the_image_not_over_it() {
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::resolved("cover.jpg", moss_core::ast::UrlKind::Asset)),
            caption: "封面：基輔米迦勒修道院門口的陣亡將士紀念牆（拍攝：糜緒洋）".to_string(),
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), None, None, None);
        assert!(
            html.contains(r#"</section><p class="moss-hero-caption">封面：基輔米迦勒修道院門口的陣亡將士紀念牆（拍攝：糜緒洋）</p>"#),
            "caption must follow the section, outside it: {html}"
        );
        assert!(html.contains(" data-captioned>"), "got: {html}");
        assert!(
            !html.contains("moss-hero-content"),
            "a caption is not an overlay: {html}"
        );
    }

    /// A caption is a credit line, and credits carry links — the same reason
    /// `byline:` / `colophon:` rows render as inline markdown. It shares that
    /// renderer rather than getting a second one.
    #[test]
    fn hero_caption_renders_inline_markdown() {
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::resolved("cover.jpg", moss_core::ast::UrlKind::Asset)),
            caption: "Photo by [A. Photographer](https://example.com)".to_string(),
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), None, None, None);
        assert!(
            html.contains(
                r#"<p class="moss-hero-caption">Photo by <a target="_blank" rel="noopener" href="https://example.com">A. Photographer</a></p>"#
            ),
            "got: {html}"
        );
    }

    #[test]
    fn hero_multi_image_emits_slides_and_data_slides() {
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::resolved("a.jpg", moss_core::ast::UrlKind::Asset)),
            extra_images: vec![
                moss_core::ast::Url::resolved("b.jpg", moss_core::ast::UrlKind::Asset),
                moss_core::ast::Url::resolved("c.jpg", moss_core::ast::UrlKind::Asset),
            ],
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), None, None, None);
        assert!(html.contains(r#"data-slides="3""#), "got: {html}");
        assert_eq!(html.matches(r#"<div class="moss-hero-slides">"#).count(), 1, "got: {html}");
        assert_eq!(html.matches(r#"<div class="moss-hero-slide">"#).count(), 3, "got: {html}");
        assert!(html.contains(r#"src="a.jpg""#) && html.contains(r#"src="c.jpg""#), "got: {html}");
    }

    #[test]
    fn hero_video_sources_render_as_ambient_loops() {
        // A video hero source — primary or slide — synthesizes the same
        // ambient loop as `![[clip.mp4|loop]]`: autoplay muted loop, no
        // controls, never a broken `<img src="x.mp4">`.
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::resolved("intro.mp4", moss_core::ast::UrlKind::Asset)),
            extra_images: vec![
                moss_core::ast::Url::resolved("b.jpg", moss_core::ast::UrlKind::Asset),
                moss_core::ast::Url::resolved("sea.webm", moss_core::ast::UrlKind::Asset),
            ],
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), None, None, None);
        assert!(html.contains(r#"data-slides="3""#), "got: {html}");
        assert_eq!(html.matches("<video").count(), 2, "got: {html}");
        assert_eq!(html.matches("autoplay muted loop playsinline").count(), 2, "got: {html}");
        assert!(!html.contains(r#"<img src="intro.mp4""#), "got: {html}");
        assert!(html.contains(r#"src="b.jpg""#), "image slide intact: {html}");
    }

    /// A plain one-image hero emits none of the optional machinery — no slide
    /// wrapper, no caption, no attribute either of them would add. Every
    /// feature the hero grows has to leave this shape byte-identical.
    #[test]
    fn hero_single_image_markup_stays_bare() {
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::resolved("a.jpg", moss_core::ast::UrlKind::Asset)),
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), None, None, None);
        assert!(!html.contains("data-slides"), "got: {html}");
        assert!(!html.contains("moss-hero-slide"), "got: {html}");
        assert!(!html.contains("data-captioned"), "got: {html}");
        assert!(!html.contains("moss-hero-caption"), "got: {html}");
        assert!(html.trim_end().ends_with("</section>"), "got: {html}");
    }

    #[test]
    fn test_render_link_preview_with_favicon() {
        let html = render_link_preview(
            "https://example.com",
            Some("Example"),
            "example.com",
            Some("https://example.com/icon.png"),
        );
        assert!(html.contains(r#"<img class="link-preview-favicon" src="https://example.com/icon.png""#));
        assert!(html.contains(r#"width="16" height="16""#));
        assert!(html.contains(r#"<span class="link-preview-domain"><img class="link-preview-favicon"#));
        assert_eq!(html.matches("link-preview-title").count(), 1);
    }

    #[test]
    fn test_render_link_preview_without_favicon() {
        let html = render_link_preview(
            "https://example.com",
            Some("Example"),
            "example.com",
            None,
        );
        assert!(!html.contains("link-preview-favicon"));
        assert!(html.contains(r#"<span class="link-preview-domain">example.com</span>"#));
    }

    #[test]
    fn test_render_link_preview_no_title_collapses_to_domain_row() {
        let html = render_link_preview(
            "https://example.com",
            None,
            "example.com",
            Some("https://example.com/icon.png"),
        );
        assert!(!html.contains("link-preview-title"));
        assert!(!html.contains("https://example.com<"));
        assert!(html.contains("link-preview-domain"));
    }

    #[test]
    fn test_render_link_preview_never_renders_description() {
        let html = render_link_preview(
            "https://example.com",
            Some("My Page"),
            "example.com",
            None,
        );
        assert!(!html.contains("link-preview-desc"));
    }


    #[test]
    fn format_data_width_none_yields_empty_string() {
        assert_eq!(format_data_width(None), "");
    }

    #[test]
    fn format_data_width_some_yields_leading_space() {
        // Leading space is critical: the caller splices this fragment
        // directly between two attributes (`class="..."` + `>`); without
        // the space the result would collide with the preceding attr.
        assert_eq!(
            format_data_width(Some("screen")),
            r#" data-width="screen""#
        );
    }

    #[test]
    fn render_hero_html_typed_with_width_screen_emits_data_width() {
        let args = moss_core::ast::HeroShortcode {
            width: Some("screen".to_string()),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(
            html.contains(r#"data-width="screen""#),
            "got: {html}"
        );
        assert!(html.contains(r#"class="moss-hero""#), "got: {html}");
    }

    #[test]
    fn render_hero_html_typed_default_omits_data_width() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(
            !html.contains("data-width"),
            "default should omit data-width, got: {html}"
        );
    }

    #[test]
    fn render_hero_html_typed_plate_class_emits_data_fit() {
        // `:::hero {.plate}` — the author writes the class, moss reads it
        // and emits the canonical `data-fit="plate"` attribute so the
        // theme never has to know the internal trigger.
        let args = moss_core::ast::HeroShortcode {
            classes: "plate".to_string(),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(html.contains(r#"data-fit="plate""#), "got: {html}");
    }

    #[test]
    fn render_hero_html_typed_default_omits_data_fit() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(!html.contains("data-fit"), "got: {html}");
    }

    #[test]
    fn render_hero_html_typed_plate_among_other_classes_still_detected() {
        // The trigger is a token match, not equality — `.plate` combined
        // with a site-specific class must still be detected.
        let args = moss_core::ast::HeroShortcode {
            classes: "landing plate".to_string(),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(html.contains(r#"data-fit="plate""#), "got: {html}");
        assert!(html.contains("landing"), "raw class list still passes through: {html}");
    }

    // Phase 4 PR4.5 (2026-05-28): render_grid_html_typed deleted —
    // Grid renders via `PipelineHooks::render_shortcode` → DefaultHooks
    // Grid arm in `crates/moss-core/src/ast/hooks.rs`. Test coverage
    // for the AST-level Grid shape lives in `moss-core` lib tests.

    /// A regression on the branch production actually takes. The overlay is
    /// rendered by `render_hero_html_typed` with a PRIMED snapshot, so the
    /// embed's fit/position must survive the synth path
    /// (`ImageRenderOptions.extra_attrs`) and not just the bare-`<img>`
    /// fallback — the two are different code, and only the fallback was
    /// covered before.
    #[test]
    fn hero_overlay_embed_keeps_img_style_through_the_synth_path() {
        use crate::build::media::dimensions::MediaDimensionLookup;
        use moss_core::ast::{Block, HeroShortcode, Inline, ResolvedUrl, Url, UrlKind};
        let meta = crate::types::content::MediaMetadata {
            is_animated: false,
            path: "photo.jpg".to_string(),
            file_type: "jpg".to_string(),
            size: 0,
            modified: None,
            // Wide enough to produce srcset rungs, so the ladder (and its
            // `sizes=`) is exercised rather than the single-URL shape.
            dimensions: Some((2400, 1200)),
            dominant_color: Some("#123456".to_string()),
            lqip_data_uri: Some("data:image/jpeg;base64,zzz".to_string()),
        };
        let lookup = MediaDimensionLookup::new(&[meta], &[], &std::collections::HashMap::new(), None);
        let args = HeroShortcode {
            overlay: vec![Block::Figure {
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
            }],
            image: Some(Url::Resolved(ResolvedUrl::new("header.png", UrlKind::Asset))),
            ..Default::default()
        };
        let html = render_hero_html_typed(&args, &|s: &str| s.to_string(), Some(&lookup), None, None);
        assert!(
            html.contains("<picture"),
            "a primed snapshot must take the synth path, not the bare fallback; got: {html}"
        );
        assert!(
            html.contains("object-fit:cover;object-position:left"),
            "hero overlay embed must keep its sizing style on the synth path; got: {html}"
        );
        // The synthesizer suppresses its own LQIP `style=` when the embed
        // supplies one, so the two declarations cannot collide.
        assert_eq!(
            html.matches(" style=").count(),
            1,
            "exactly one style= on the overlay image; got: {html}"
        );
    }

    #[test]
    fn hero_overlay_heading_has_no_permalink_anchor() {
        // Headings inside :::hero are visual titles; the moss-heading-anchor
        // permalink link must not appear (a real site's main.md regression).
        use moss_core::ast::{parse, HeroShortcode, Url};
        let overlay_md = "# Understanding climate extremes\n\nSubtitle prose.";
        let parsed = parse(overlay_md);
        let args = HeroShortcode {
            overlay: parsed.blocks,
            image: Some(Url::Resolved(moss_core::ast::ResolvedUrl {
                href: "header.png".to_string(),
                kind: moss_core::ast::UrlKind::Asset,
            })),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(
            !html.contains("moss-heading-anchor"),
            "hero heading must not carry a permalink anchor; got: {html}"
        );
        assert!(
            html.contains(r#"id="understanding-climate-extremes""#),
            "hero heading must still carry its id for anchor links; got: {html}"
        );
    }

    #[test]
    fn render_hero_html_typed_emits_source_range_when_source_line_given() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, Some(5), None);
        assert!(
            html.contains(r#"data-source-range="5-5""#),
            "hero with source_line=Some(5) should emit data-source-range=\"5-5\", got: {html}"
        );
    }

    #[test]
    fn render_hero_html_typed_omits_source_range_when_none() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(
            !html.contains("data-source-range"),
            "hero with source_line=None should not emit data-source-range, got: {html}"
        );
    }

    #[test]
    fn render_hero_emits_cover_color_css_var_when_dominant_color_provided() {
        let args = moss_core::ast::HeroShortcode {
            overlay: vec![moss_core::ast::Block::Paragraph(vec![
                moss_core::ast::Inline::Text("Text".to_string()),
            ])],
            overlay_text: "Text".to_string(),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, Some("hsla(212, 14%, 29%, 1)"));
        assert!(html.contains("data-cover-color"), "section must carry data-cover-color, got: {html}");
        assert!(
            html.contains("--moss-cover-color: hsla(212, 14%, 29%, 1)"),
            "section must carry --moss-cover-color with the dominant color, got: {html}"
        );
        assert!(!html.contains("--moss-hero-mobile-bg"), "must not emit old var, got: {html}");
        assert!(!html.contains("--moss-hero-mobile-color"), "must not emit old companion var, got: {html}");
    }

    #[test]
    fn render_hero_no_css_vars_when_no_dominant_color() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(!html.contains("--moss-cover-color"), "got: {html}");
        assert!(!html.contains("data-cover-color"), "got: {html}");
    }

    #[test]
    fn render_hero_tags_a_pale_image_for_a_stronger_scrim() {
        // A near-white hero photo leaves white overlay text far under AA with
        // the default scrim; CSS keys the stronger ramp off this attribute.
        let html = hero_html_with_cover_color("#EDF6F4");
        assert!(
            html.contains(r#"data-hero-tone="light""#),
            "pale hero must be tagged light, got: {html}"
        );
    }

    #[test]
    fn render_hero_leaves_a_dark_image_untagged() {
        // The default scrim already carries white type here, and tagging would
        // darken two-thirds of an image that needed no help.
        let html = hero_html_with_cover_color("#1B2A38");
        assert!(
            !html.contains("data-hero-tone"),
            "dark hero must keep the default scrim, got: {html}"
        );
    }

    #[test]
    fn render_hero_image_only_is_never_tone_tagged() {
        // No overlay text means no scrim at all, so tone is meaningless.
        let lookup = lookup_with_color("/img.png", "#EDF6F4");
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::Resolved(moss_core::ast::ResolvedUrl {
                href: "/img.png".to_string(),
                kind: moss_core::ast::UrlKind::Asset,
            })),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, Some(&lookup), None, None);
        assert!(!html.contains("data-hero-tone"), "got: {html}");
    }

    fn lookup_with_color(
        href: &str,
        raw: &str,
    ) -> crate::build::media::dimensions::MediaDimensionLookup {
        let img = crate::types::content::MediaMetadata {
            path: href.trim_start_matches('/').to_string(),
            file_type: "png".to_string(),
            dominant_color: Some(raw.to_string()),
            ..Default::default()
        };
        crate::build::media::dimensions::MediaDimensionLookup::new(
            &[img],
            &[],
            &std::collections::HashMap::new(),
            None,
        )
    }

    fn hero_html_with_cover_color(raw: &str) -> String {
        let lookup = lookup_with_color("/img.png", raw);
        let args = moss_core::ast::HeroShortcode {
            image: Some(moss_core::ast::Url::Resolved(moss_core::ast::ResolvedUrl {
                href: "/img.png".to_string(),
                kind: moss_core::ast::UrlKind::Asset,
            })),
            overlay: vec![moss_core::ast::Block::Paragraph(vec![
                moss_core::ast::Inline::Text("Text".to_string()),
            ])],
            overlay_text: "Text".to_string(),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        render_hero_html_typed(&args, &resolver, Some(&lookup), None, None)
    }

    #[test]
    fn render_hero_overlay_mode_emits_data_mobile() {
        let args = moss_core::ast::HeroShortcode {
            mobile: Some("overlay".to_string()),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(html.contains(r#"data-mobile="overlay""#), "got: {html}");
    }

    #[test]
    fn render_hero_overlay_mode_suppresses_cover_color_even_with_color() {
        let args = moss_core::ast::HeroShortcode {
            mobile: Some("overlay".to_string()),
            ..Default::default()
        };
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, Some("hsla(0, 50%, 20%, 1)"));
        assert!(!html.contains("--moss-cover-color"), "overlay must suppress cover color var, got: {html}");
        assert!(!html.contains("data-cover-color"), "overlay must suppress data attr, got: {html}");
    }

    #[test]
    fn render_hero_default_omits_data_mobile() {
        let args = moss_core::ast::HeroShortcode::default();
        let resolver = |s: &str| s.to_string();
        let html = render_hero_html_typed(&args, &resolver, None, None, None);
        assert!(!html.contains("data-mobile"), "got: {html}");
    }

    #[test]
    fn format_class_attr_without_extra() {
        assert_eq!(format_class_attr("moss-grid", ""), r#"class="moss-grid""#);
    }

    #[test]
    fn format_class_attr_with_extra() {
        assert_eq!(
            format_class_attr("moss-grid", "profiles featured"),
            r#"class="moss-grid profiles featured""#
        );
    }
}
