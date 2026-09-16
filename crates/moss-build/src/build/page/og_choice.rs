//! Which picture a page's `og:image` / `twitter:image` points at.
//!
// Resolve cover image and dimensions for og:image / twitter:image.
//
// Priority chain (see `crate::build::page::cover::resolve_cover_chain`):
//   1. Page frontmatter `cover` field (resolved through path_resolver).
//   2. Filename convention: feature.*/cover.*/thumbnail.* in bundle dir.
//   3. This page's own hero image (typed via `doc.hero_image_url`).
//   4. First markdown-origin `![]()` image in the page body (typed via
//      `doc.body_cover_path` — Step 5 of the structural-html-emission
//      migration; replaces the prior `first_body_image` regex scrape).
//   5. Auto-generated 1200x630 OG card (when output_dir + an og_outputs
//      sink are both provided and a title is available). Falls back to no
//      cover on rasterization error.
//
// The same (cover, dims) tuple is reused by og_tags and twitter_tags below
// so the auto-card is only rendered once per page. The auto-card is the
// ONLY source that sets `cover_dims_for_og = Some((1200, 630))` — the
// other sources leave dims `None` (we don't probe arbitrary cover URLs).
// `vertical_typesetting` / `resolved_typesetting` are computed near the
// top of this function (beside `ui_lang`) — `resolve_page_body` needs
// them earlier than this point.//!
//! Extracted from `render/html.rs` 2026-09-11, when the plate card pushed
//! that file further over its line budget. It is one decision with one
//! answer, and it reads better away from the 1,700 lines it was inside.

use crate::build::page::meta::CoverRef;
use crate::build::page::og_card::{CardInputs, CardPlate, OgSink};
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;
use crate::build::render::html::{DEFAULT_OG_ACCENT, DEFAULT_OG_BG, DEFAULT_OG_FG};
use crate::build::assets::paths::PathResolver;

/// Everything the decision reads. One struct because the call site is a
/// 1,700-line function and eleven positional arguments there would be a
/// riddle.
pub struct OgCoverInputs<'a> {
    pub doc: Option<&'a ParsedDocument>,
    pub project: &'a ProjectStructure,
    pub path_resolver: &'a PathResolver,
    pub source_root: &'a std::path::Path,
    pub dir_overrides: &'a std::collections::HashMap<String, String>,
    pub site_title: &'a str,
    pub is_homepage: bool,
    pub is_card_eligible: bool,
    pub ui_lang: crate::i18n::Language,
    pub vertical_typesetting: bool,
    pub output_dir: Option<&'a std::path::Path>,
}

/// The cover to emit, and its dimensions when moss knows them.
pub fn resolve(
    input: &OgCoverInputs<'_>,
    mut og_outputs: Option<&mut OgSink<'_>>,
) -> (Option<CoverRef>, Option<(u32, u32)>) {
    let OgCoverInputs {
        doc,
        project,
        path_resolver,
        source_root,
        dir_overrides,
        site_title,
        is_homepage,
        is_card_eligible,
        ui_lang,
        vertical_typesetting,
        output_dir,
    } = *input;
    let resolve_cover_url = |raw: &str| -> String {
        let (path_part, _) = moss_core::media::split_pipe(raw);
        path_resolver.resolve_url(path_part)
    };
    // No `is_card_eligible` guard on any rung: every reader of
    // `resolved_cover_for_og` below is already behind that gate, so a
    // draft's cover is computed and then never emitted. Guarding some
    // rungs and not others only made the block read as a leak.
    let page_cover_resolved: Option<String> =
        doc.and_then(|d| d.cover.as_deref()).map(resolve_cover_url);
    // Rung 3 is this page's own hero. The home page reaches it like any
    // other page, so it needs no self-lookup branch.
    let page_hero_image_url = doc.and_then(|d| d.hero_image_url.as_deref());
    // Body-image fallback: typed `body_cover_path` captured by
    // `transform_events` at parse time. Articles and homepages both
    // expose this field (the homepage's `doc` is the same
    // `ParsedDocument` shape); other template branches don't have a
    // page-level doc and so contribute None — `resolve_cover_chain`
    // gracefully short-circuits the rung.
    let body_cover_for_chain: Option<&str> = doc
        .and_then(|d| d.body_cover_path.as_deref());

    // Bundle dir: parent of the page's source file (e.g. posts/my-post/).
    // Used by the filename-convention rung in resolve_cover_chain.
    let bundle_dir: Option<std::path::PathBuf> = doc
        .and_then(|d| d.source_path.as_deref())
        .and_then(|sp| std::path::Path::new(sp).parent())
        .map(|rel| source_root.join(rel));
    let bundle_dir_ref: Option<&std::path::Path> = bundle_dir.as_deref();

    // Try the cheap chain: the page's own picture.
    let cheap_choice = crate::build::page::cover::resolve_cover_chain(
        &crate::build::page::cover::CoverChainInputs {
            page_cover: page_cover_resolved.as_deref(),
            page_hero_image_url,
            body_cover_path: body_cover_for_chain,
            bundle_dir: bundle_dir_ref,
            source_root,
        },
    );

    // The homepage card shows the site name; every other listable page
    // (article, nav landing, folder index) shows its own page label. Both
    // the title-only card and the plate card set it, hence the hoist.
    let card_title: &str = if is_homepage {
        site_title
    } else {
        doc.map(|d| d.label.as_str()).unwrap_or("")
    };
    // Only render where there is a sink to track the path; otherwise we'd
    // orphan the PNG (no caller would register it for stale-cleanup).
    let can_render_card = is_card_eligible
        && output_dir.is_some()
        && og_outputs.is_some()
        && !card_title.is_empty();
    let mut render_card = |plate: Option<CardPlate>| {
        let inputs = CardInputs {
            title: card_title,
            site_name: site_title,
            bg_color: DEFAULT_OG_BG,
            fg_color: DEFAULT_OG_FG,
            accent_color: DEFAULT_OG_ACCENT,
            // Orders the card's CJK glyph-fallback chain: a Traditional
            // page must not be set in a Simplified (or Korean) face.
            lang: ui_lang,
            vertical: vertical_typesetting,
            plate,
        };
        let sink = og_outputs.as_mut().expect("guarded by can_render_card");
        match sink.render(&inputs, output_dir.unwrap()) {
            Ok(served_path) => {
                Some(CoverRef::Local(served_path.clone()))
            }
            Err(e) => {
                log::warn!("OG card render failed for '{}': {}", card_title, e);
                None
            }
        }
    };

    if let Some(cover_ref) = cheap_choice {
        // Frontmatter, filename convention, hero, or first body image. A
        // cover no platform crop can hold becomes a PLATE — drawn whole
        // into a card moss composes — and everything else is handed over
        // as-is with the dimensions moss scanned; `cover::plate_source`
        // holds both decisions and their reasons. Failing to render the
        // plate lands in the same arm as never wanting one: the file
        // itself is what moss sent before plates existed.
        let plate = can_render_card
            .then(|| {
                crate::build::page::cover::plate_source(
                    &project.image_files, &cover_ref, dir_overrides, source_root,
                )
            })
            .flatten();
        match plate.as_ref().and_then(|p| render_card(Some(p.as_plate()))) {
            Some(card) => (Some(card), Some((1200u32, 630u32))),
            None => {
                let dims = crate::build::page::cover::scanned_dims(
                    &project.image_files, &cover_ref, dir_overrides,
                );
                (Some(cover_ref), dims)
            }
        }
    } else if can_render_card {
        match render_card(None) {
            Some(card) => (Some(card), Some((1200u32, 630u32))),
            None => (None, None),
        }
    } else {
        (None, None)
    }
}
