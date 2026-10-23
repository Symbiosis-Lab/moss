//! A hero that draws from a folder listing (`:::hero {from="work/|when:upcoming"}`).
//!
//! The hero shows the first page the same listing would show as an embed: the
//! members and their order come from [`super::list_pages`] and
//! [`super::members::order`], so the two cannot disagree. The fence's own
//! picture and words are the hero when the listing has no page.

use moss_core::ast::{Block, ButtonItem, ButtonsShortcode, Inline, Shortcode, Url, UrlKind};
use moss_core::resolve::embed_renderer::folder_list::{listing_marker, marker_body};

use super::{list_pages, members, parse_marker_body, warn_unknown_options, Listed};
use crate::build::components::child_list::explicit_card_text;
use crate::build::components::event_meta;
use crate::build::types::ParsedDocument;
use crate::types::content::ProjectStructure;

/// A hero that draws from a folder listing, and what shows when the listing is empty.
#[derive(Debug, Clone)]
pub(crate) struct HeroFrom {
    pub drawn: moss_core::ast::HeroDrawnFrom,
    pub fallback_html: Option<String>,
    pub fallback_image_url: Option<String>,
    pub fallback_overlay_text: Option<String>,
}

/// What a hero page carries after this pass.
struct Hero {
    html: Option<String>,
    /// The drawn event has not ended, so the page swaps to its fallback at view time.
    hero_ends: bool,
    image_url: Option<String>,
    overlay_text: Option<String>,
}

type DirOverrides = std::collections::HashMap<String, String>;

/// Settle every `from=` hero: drawn from its listing's first page, else the
/// fallback the page parsed with. Always sets all three hero fields, so a
/// repeated pass starts from the same place.
pub(crate) fn resolve_all(
    documents: &mut [ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &DirOverrides,
    math: bool,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    now: chrono::NaiveDateTime,
) {
    let settled: Vec<(usize, Hero)> = documents
        .iter()
        .enumerate()
        .filter_map(|(i, doc)| {
            let plan = doc.hero_from.as_ref()?;
            Some((i, settle(doc, plan, documents, project, dir_overrides, math, media_lookup, now)))
        })
        .collect();
    for (i, hero) in settled {
        documents[i].features.hero_ends = hero.hero_ends;
        documents[i].hero_html = hero.html;
        documents[i].hero_image_url = hero.image_url;
        documents[i].hero_overlay_text = hero.overlay_text;
    }
}

#[allow(clippy::too_many_arguments)]
fn settle(
    doc: &ParsedDocument,
    plan: &HeroFrom,
    documents: &[ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &DirOverrides,
    math: bool,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    now: chrono::NaiveDateTime,
) -> Hero {
    let page_path = doc.source_path.as_deref().unwrap_or(&doc.url_path);
    let spec = plan.drawn.shortcode.from.as_deref().unwrap_or_default();
    match first_page(spec, page_path, documents, project, dir_overrides, math, now) {
        Some(page) => draw(doc, plan, page, dir_overrides, media_lookup, now),
        None => Hero {
            html: plan.fallback_html.clone(),
            hero_ends: false,
            image_url: plan.fallback_image_url.clone(),
            overlay_text: plan.fallback_overlay_text.clone(),
        },
    }
}

/// The first page of the listing `spec` names (a folder embed's target and
/// options, as written after `![[`), or `None` with a warning when it names no
/// listing.
fn first_page<'a>(
    spec: &str,
    page_path: &str,
    documents: &'a [ParsedDocument],
    project: &ProjectStructure,
    dir_overrides: &DirOverrides,
    math: bool,
    now: chrono::NaiveDateTime,
) -> Option<&'a ParsedDocument> {
    let marker = listing_marker(spec, page_path);
    let Some(mut parsed) = marker.as_deref().and_then(marker_body).and_then(parse_marker_body) else {
        crate::build::cli_output::log_warn_problem!("[{page_path}] hero from='{spec}' does not name a folder listing such as 'work/' or 'work/|when:upcoming'");
        return None;
    };
    parsed.limit = None;
    warn_unknown_options(page_path, &parsed);
    let target = parsed.path;

    let (target_doc, is_flatten, pages) = match list_pages(&parsed, page_path, documents, project, dir_overrides, now) {
        Ok(Listed::Pages { target_doc, is_flatten, pages, .. }) => (target_doc, is_flatten, pages),
        Ok(Listed::Iframe(_)) => {
            crate::build::cli_output::log_warn_problem!("[{page_path}] hero from '{target}' names an app, not a listing");
            return None;
        }
        Err(problem) => {
            crate::build::cli_output::log_warn_problem!("[{page_path}] unresolved hero listing: {problem}");
            return None;
        }
    };
    // A folder lists newest first, so the first of `when:upcoming` would be the
    // furthest away. `group:upcoming` is the listing's own soonest-first order.
    let group = parsed
        .group
        .clone()
        .or_else(|| (parsed.when == Some("upcoming")).then(|| "upcoming".to_string()))
        .map(moss_core::Resolved::frontmatter);
    let ordered = members::order(&pages, target_doc, is_flatten, parsed.sort, None, group.as_ref(), None, None, math, now);
    ordered.limited.first().copied()
}

/// The hero's own attributes and fallback picture, with the page's picture
/// and words in place of the fence's.
fn draw(
    doc: &ParsedDocument,
    plan: &HeroFrom,
    page: &ParsedDocument,
    dir_overrides: &DirOverrides,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    now: chrono::NaiveDateTime,
) -> Hero {
    let lang = doc.lang;
    let resolver = crate::build::assets::paths::PathResolver::new().with_dir_overrides(dir_overrides.clone());
    let mut hero = plan.drawn.shortcode.clone();

    if let Some(cover) = page.listing_cover() {
        let (path, attrs) = moss_core::media::split_pipe(cover);
        let href = resolver.resolve_url(path);
        hero.image = Some(Url::resolved(href, UrlKind::Asset));
        hero.attrs = attrs.to_string();
        hero.image_alt = String::new();
        hero.extra_images.clear();
        hero.slide_captions.clear();
        hero.caption.clear();
    }

    let description = explicit_card_text(page);
    let mut words = vec![Block::Heading { level: 1, children: vec![Inline::Text(page.label.clone())], id: None, contents_end: false, contents_label: None }];
    if let Some(meta) = event_meta::render(page, lang, false, None, event_meta::UpcomingLabel::Shown, now) {
        words.push(Block::Paragraph(vec![Inline::Other(meta)]));
    }
    if let Some(text) = &description {
        words.push(Block::Paragraph(vec![Inline::Text(text.clone())]));
    }
    let href = resolver.resolve_url(&crate::build::scan::article_map::to_pretty_url(&page.url_path));
    words.push(Block::Shortcode(Shortcode::Buttons(ButtonsShortcode {
        classes: String::new(),
        items: vec![ButtonItem {
            text: crate::i18n::t(lang, "hero_more_about").replace("{}", &page.label),
            url: Url::resolved(href, UrlKind::Internal),
        }],
    })));
    hero.overlay = words;
    hero.overlay_text = description.clone().unwrap_or_default();
    // The overlay is no longer what the author wrote, so its source lines no longer line up with it.
    hero.source.fragments.clear();

    let image_url = match &hero.image {
        Some(Url::Resolved(r)) => Some(r.href.clone()),
        _ => None,
    };
    let mobile_bg = image_url.as_deref().and_then(|href| media_lookup.get_cover_color_muted(href));
    let drawn = crate::build::markdown::typed_renderers::render_hero_html_typed(
        &hero,
        Some(media_lookup),
        plan.drawn.source_line,
        mobile_bg.as_deref(),
        lang,
    );
    // An event not yet over is drawn for now; `hero-ends.js` swaps in the
    // fallback, kept inert in a template, once a visitor opens the page after
    // the end. The page is rebuilt with the next event at the next build.
    let ends = page.event.as_ref().and_then(|ev| super::upcoming::ends_if_upcoming(ev.start, ev.end, now));
    let html = match &ends {
        Some(ends) => format!("{drawn}<template data-hero-ends=\"{ends}\">{}</template>", plan.fallback_html.as_deref().unwrap_or_default()),
        None => drawn,
    };
    Hero { html: Some(html), hero_ends: ends.is_some(), image_url, overlay_text: description }
}
