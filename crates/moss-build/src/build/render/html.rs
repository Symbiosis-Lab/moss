//! HTML generation for all page types in the moss static site generator.
//!
//! The `generate_html` function converts a `ParsedDocument` (plus site context)
//! into a complete HTML page using the template system.

use crate::{build::types::ParsedDocument, types::content::ProjectStructure};
use moss_core::PageKind;

// Generator submodule imports
use crate::build::components;
use crate::build::render::grid_cells;
use crate::build::markdown::html_post::splice_after_title_block;
use crate::build::components::nav::{NavigationBuilder, compute_breadcrumb_segments};
use crate::build::page::layout::LayoutConfig;
use crate::build::page::page::generate_year_grouped_article_list;
use crate::build::assets::paths::PathResolver;
use crate::build::media::qr;
use crate::build::page::shell::{
    ShellProcessor, ShellRegistry, ShellType, ShellVars,
};

// Sibling module imports (within build/render/)
use super::config::{resolve_logo_url, resolve_data_attr, resolve_comments_attr};
use super::credits;

/// Load a JS asset: read from disk in dev (for Vite hot reload), use include_str! in release.
///
/// Shared by `html.rs` (hash computation) and `blocking.rs` (writing assets to disk)
/// so that hashes stay consistent when JS files change on disk during a dev session.
pub(crate) fn load_js_asset(disk_path: &str, embedded: &str) -> String {
    #[cfg(dev)]
    {
        let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(disk_path);
        std::fs::read_to_string(&full).unwrap_or_else(|_| embedded.to_string())
    }
    #[cfg(not(dev))]
    {
        embedded.to_string()
    }
}

/// Default theme colors for auto-generated OG cards. Mirrors the theme color
/// constants used by the page template (see `shell.rs`). Accent is a
/// muted earth tone consistent with moss's brand palette.
pub const DEFAULT_OG_BG: &str = "#faf8f5";
pub const DEFAULT_OG_FG: &str = "#1a1816";
pub const DEFAULT_OG_ACCENT: &str = "#8a7a6f";

/// Look up the homepage `ParsedDocument` for a given page language.
///
/// Multilingual sites have one homepage per language: the site-default
/// homepage at `index.html` and per-language homepages at `<lang>/index.html`.
/// Pages in a non-default language should pull homepage-cascade fields
/// (cover, hero, description) from THEIR language's homepage so share cards
/// don't surface English copy on Chinese pages.
///
/// Fallback order: language-specific homepage → site-default homepage → None.
/// Used by both the OG cover chain and the OG description chain.
fn find_homepage_doc<'a>(
    all_docs: &'a [crate::build::types::ParsedDocument],
    page_lang: crate::i18n::Language,
    site_lang: crate::i18n::Language,
) -> Option<&'a crate::build::types::ParsedDocument> {
    if page_lang != site_lang {
        let lang_homepage = format!("{}/index.html", page_lang.code());
        if let Some(d) = all_docs.iter().find(|d| d.url_path == lang_homepage) {
            return Some(d);
        }
    }
    all_docs.iter().find(|d| d.url_path == "index.html")
}

pub fn generate_html(
    doc: Option<&ParsedDocument>,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    rss_link: Option<&str>,
    analytics_script: Option<&str>,
    site_lang: crate::i18n::Language,
    css_version: Option<&str>,
    has_user_css: bool,
    has_sidebar_layout: bool,
    user_css_version: Option<&str>,
    has_user_js: bool,
    user_js_version: Option<&str>,
    content_graph: Option<&moss_core::content_graph::ContentGraph>,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_url: &crate::build::site_url::SiteUrl,
    show_rss_in_footer: bool,
    emit_source_lines: bool,
    favicon_filename: &str,
    output_dir: Option<&std::path::Path>,
    source_root: &std::path::Path,
) -> Result<String, String> {
    // Resolves its own script snapshot: this entry point renders one page in
    // isolation (tests, one-off callers), so there is no build-wide snapshot to
    // share. The real multi-page path is `generate_html_collect_og`, which takes
    // the caller's — see the comment on the hash block in `generate_html_inner`.
    let scripts = crate::build::emit::scripts::ScriptAssets::resolve();
    // Pass `None` for og_outputs so the inner skips auto-card rendering when
    // there is no caller-provided sink to track the generated PNG path.
    // Without this, an auto-card would be written to disk and immediately
    // orphaned because there is no way to register it for stale-cleanup.
    generate_html_inner(
        doc, all_docs, project, layout_config, is_homepage, rss_link,
        analytics_script, site_lang, css_version, has_user_css,
        has_sidebar_layout, user_css_version, has_user_js, user_js_version,
        content_graph, dir_overrides, site_url, show_rss_in_footer,
        emit_source_lines, favicon_filename, false, output_dir,
        None,
        source_root, &scripts,
    )
}

/// Like `generate_html`, but also collects the relative path of any auto-
/// generated OG card PNG that was written under `output_dir/_moss/og/`. The
/// caller appends those paths to `site_hashes.image_outputs` so blocking-
/// phase stale cleanup preserves them.
pub fn generate_html_collect_og(
    doc: Option<&ParsedDocument>,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    rss_link: Option<&str>,
    analytics_script: Option<&str>,
    site_lang: crate::i18n::Language,
    css_version: Option<&str>,
    has_user_css: bool,
    has_sidebar_layout: bool,
    user_css_version: Option<&str>,
    has_user_js: bool,
    user_js_version: Option<&str>,
    content_graph: Option<&moss_core::content_graph::ContentGraph>,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_url: &crate::build::site_url::SiteUrl,
    show_rss_in_footer: bool,
    emit_source_lines: bool,
    favicon_filename: &str,
    // Whether this build rasterized `favicon-{16,32,180}.png` — the build's
    // answer, never a probe of `output_dir`, where a stale trio can outlive
    // the build that wrote it.
    favicon_has_raster_pngs: bool,
    output_dir: Option<&std::path::Path>,
    og_outputs: &mut crate::build::page::og_card::OgSink<'_>,
    source_root: &std::path::Path,
    // The build's one script snapshot, taken once in `blocking.rs`. Passing it
    // rather than re-resolving keeps every page's `<script src>` naming a file
    // the same snapshot emitted.
    scripts: &crate::build::emit::scripts::ScriptAssets,
) -> Result<String, String> {
    generate_html_inner(
        doc, all_docs, project, layout_config, is_homepage, rss_link,
        analytics_script, site_lang, css_version, has_user_css,
        has_sidebar_layout, user_css_version, has_user_js, user_js_version,
        content_graph, dir_overrides, site_url, show_rss_in_footer,
        emit_source_lines, favicon_filename, favicon_has_raster_pngs, output_dir,
        Some(og_outputs),
        source_root, scripts,
    )
}

/// The folder-index heading to prepend in the no-cover branch: the shared
/// `<h1 class="moss-folder-title">`, unless something else on the page is
/// already showing the title. Two things can be —
///
/// - the folder renders as a nav item, so the nav bar shows it, or
/// - the body opens with a `:::hero` that carries its own heading.
///
/// Covered folder indexes bypass this helper entirely (they render the label
/// inside the cover via `folder_cover::render`), and synthetic folder indexes
/// are generated in `render/blocking.rs` and never reach it — so this is the
/// only folder-title site that suppresses.
fn no_cover_folder_heading(
    doc: &crate::build::types::ParsedDocument,
    h1_text: &str,
    has_content_folders: bool,
    emit_source_fm: bool,
) -> String {
    if crate::build::components::nav::is_nav_bar_item_doc(doc, has_content_folders) {
        String::new()
    } else if moss_core::heading::hero_at_top_owns_title(&doc.content) {
        // The same rule the article path applies, which this site never asked.
        // A folder note that opens with a hero carrying its own heading got
        // BOTH that heading and an injected `moss-folder-title` — two titles,
        // one above the other, on every season index of the site that adopted
        // full-bleed covers.
        //
        // `heading::compute`'s `visible` cannot be reused here: it is false for
        // every folder note regardless (`!is_index_file`), so it answers a
        // different question. The hero predicate is the shared part, and it is
        // the part that was missing.
        String::new()
    } else {
        crate::build::components::folder_title::render(h1_text, emit_source_fm)
    }
}

/// Editor preview: stamp a frontmatter-synthesized children listing with
/// `data-source-fm="children"` on its `.moss-cards-container` — ONE attribute
/// for the whole `children_*` family (children / children_style /
/// children_group / children_depth / children_limit …), whose chips can all
/// reveal the same container. Called only on the two listings synthesized
/// FROM frontmatter (homepage + folder index); a body `![[folder/]]` embed is
/// expanded in `expand_markers_in_documents` and keeps its own source line.
fn annotate_children_listing(listing: String, emit_source_lines: bool) -> String {
    if !emit_source_lines {
        return listing;
    }
    listing.replacen(
        r#"<div class="moss-cards-container">"#,
        r#"<div class="moss-cards-container" data-source-fm="children">"#,
        1,
    )
}

/// Resolves `children_source:`'s wikilink target to a folder's site path (no
/// leading/trailing slash; `""` for the root) — the NAMED folder, never the
/// hosting page's own `url_path`. Shared by both `children:` branches below.
fn resolve_children_source_folder_path(children_source: &str, all_docs: &[ParsedDocument]) -> String {
    let stem = crate::build::markdown::frontmatter_ref_to_stem(children_source);
    all_docs
        .iter()
        .find(|d| {
            let d_stem = d.url_path.trim_end_matches("/index.html").trim_end_matches('/');
            d_stem.eq_ignore_ascii_case(&stem) && d.kind == PageKind::Folder
        })
        .map(|d| d.url_path.trim_end_matches("index.html").trim_end_matches('/').to_string())
        .unwrap_or_else(|| stem.to_lowercase())
}

/// The browser-tab `<title>` text: `"{page} - {site}"` for sub-pages, bare
/// `"{page}"` for the homepage or when the page title already equals the site
/// title (avoids `X - X` doubling). Single source of truth for tab titles.
pub(crate) fn tab_title(page_title: &str, site_title: &str, is_homepage: bool) -> String {
    if !is_homepage && page_title != site_title {
        format!("{} - {}", page_title, site_title)
    } else {
        page_title.to_string()
    }
}

fn generate_html_inner(
    doc: Option<&ParsedDocument>,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    rss_link: Option<&str>,
    _analytics_script: Option<&str>,
    site_lang: crate::i18n::Language,
    css_version: Option<&str>,
    has_user_css: bool,
    has_sidebar_layout: bool,
    user_css_version: Option<&str>,
    has_user_js: bool,
    user_js_version: Option<&str>,
    content_graph: Option<&moss_core::content_graph::ContentGraph>,
    dir_overrides: &std::collections::HashMap<String, String>,
    site_url: &crate::build::site_url::SiteUrl,
    show_rss_in_footer: bool,
    emit_source_lines: bool,
    favicon_filename: &str,
    favicon_has_raster_pngs: bool,
    output_dir: Option<&std::path::Path>,
    mut og_outputs: Option<&mut crate::build::page::og_card::OgSink<'_>>,
    source_root: &std::path::Path,
    scripts: &crate::build::emit::scripts::ScriptAssets,
) -> Result<String, String> {
    // The language moss's own interface is drawn in: the page's own when it
    // declares one moss has strings for, else the site default. Distinct from
    // `<html lang>`, which describes the content and may name a language moss
    // has no interface for at all (#977).
    let ui_lang = doc.map(|d| d.lang).unwrap_or(site_lang);

    // The page's effective typesetting — its own, else the site's. Computed
    // here (not beside its original use below) because `resolve_page_body`,
    // which also needs it for card-meta dates, runs before that point.
    let resolved_typesetting: Option<&str> = super::config::effective_typesetting(
        doc.and_then(|d| d.typesetting.as_deref()),
        layout_config.typesetting.as_deref(),
    );
    let vertical_typesetting = resolved_typesetting == Some("vertical");

    // `<html lang>`, hreflang and Schema.org `inLanguage` all emit THIS one
    // string, so they cannot contradict each other — deriving it three times
    // below is how they drifted apart (moss#1177).
    let page_lang_tag = doc
        .and_then(|d| d.lang_tag.clone())
        .unwrap_or_else(|| layout_config.lang_tag.clone());

    // Get homepage document for metadata
    let homepage_doc = project
        .homepage_file
        .as_ref()
        .and_then(|_| all_docs.iter().find(|d| d.url_path == "index.html"));

    // Use site_name from LayoutConfig (which handles folder name for Minimal layout)
    let site_title = if layout_config.site_name.is_empty() {
        // For Full layout, fall back to homepage title (skip filename-derived stems)
        homepage_doc
            .map(|d| d.title.clone())
            .filter(|t| !moss_core::home::is_index_stem(t))
            .unwrap_or_else(|| crate::i18n::t(site_lang, "site").to_string())
    } else {
        layout_config.site_name.clone()
    };

    // Per-language site name: if the current page is non-default language,
    // look for the translated homepage and use its title as site_name.
    // e.g., Chinese pages use "青苔" instead of "moss".
    let site_title = if let Some(d) = doc {
        if d.lang != site_lang {
            let lang_homepage = format!("{}/index.html", d.lang.code());
            all_docs.iter()
                .find(|dd| dd.url_path == lang_homepage)
                .map(|dd| dd.title.clone())
                .filter(|t| !t.is_empty() && !moss_core::home::is_index_stem(t))
                .unwrap_or(site_title)
        } else {
            site_title
        }
    } else {
        site_title
    };

    // Generate analytics script tag from homepage frontmatter
    let analytics_script = homepage_doc
        .and_then(|d| d.analytics.as_ref())
        .map(|analytics| analytics.to_script_tag());

    // Cache-busted script filenames come from the caller's snapshot, never from
    // a fresh read here. Under `#[cfg(dev)]` `load_js_asset` reads from disk, so
    // re-resolving per page raced `npm run dev`'s esbuild: a rebuild landing
    // mid-render gave later pages a `theme.<hash>.js` URL the emitter — which
    // wrote from the snapshot taken at the top of the build — never produced.

    // Initialize path resolver early — used for feed path, series nav, CSS/JS, etc.
    let path_resolver = {
        let pr = match css_version {
            Some(version) => PathResolver::new().with_css_version(version),
            None => PathResolver::new(),
        };
        let pr = match user_css_version {
            Some(v) => pr.with_user_css_version(v),
            None => pr,
        };
        let pr = match user_js_version {
            Some(v) => pr.with_user_js_version(v),
            None => pr,
        };
        let pr = pr.with_js_version(scripts.hash("theme"));
        pr.with_favicon_filename(favicon_filename)
            .with_favicon_has_raster_pngs(favicon_has_raster_pngs)
            .with_dir_overrides(dir_overrides.clone())
    };

    let current_page_url = doc.map(|d| d.url_path.as_str());
    // `logo:` is a whole-site field read from a homepage. The nav shows it on
    // every page, but only the homepage that DECLARES it has the field in the
    // file being edited — only there may the nav logo carry
    // `data-source-fm="logo"` (a chip absent from the open file cannot be
    // revealed, so annotating any other page would be a lie).
    let logo_is_own_field = doc.is_some_and(|d| {
        d.logo.is_some()
            && (d.url_path == "index.html"
                || d.url_path == format!("{}/index.html", d.lang.code()))
    });
    let nav_builder = NavigationBuilder::new(
        all_docs,
        &site_title,
        current_page_url,
        site_lang,
        project.has_content_folders,
    )
    .with_search(layout_config.assets.search)
    .with_source_fm(emit_source_lines, emit_source_lines && logo_is_own_field);
    // Per-language logo: check if the translated homepage has its own logo,
    // otherwise fall back to the default homepage's logo.
    let logo_for_page = if let Some(d) = doc {
        if d.lang != site_lang {
            let lang_homepage = format!("{}/index.html", d.lang.code());
            let lang_logo = all_docs.iter()
                .find(|dd| dd.url_path == lang_homepage)
                .and_then(|dd| dd.logo.as_ref())
                .map(|l| resolve_logo_url(l));
            lang_logo.or_else(|| layout_config.logo_path.clone())
        } else {
            layout_config.logo_path.clone()
        }
    } else {
        layout_config.logo_path.clone()
    };
    let nav_builder = if let Some(logo) = logo_for_page {
        nav_builder.with_logo(logo)
    } else {
        nav_builder
    };

    // Compute breadcrumb segments if the page qualifies
    let nav_builder = if let Some(d) = doc {
        if let Some(segments) = compute_breadcrumb_segments(d, all_docs, &site_title, project.has_content_folders) {
            nav_builder.with_breadcrumb(segments)
        } else {
            nav_builder
        }
    } else {
        nav_builder
    };

    // Wire language toggle into nav. Single source of truth for switcher
    // languages: per-article translations, gap-filled with each OTHER language's
    // homepage root, current language always excluded (see
    // i18n::site_languages::other_language_links). Replaces the old
    // homepage-translations fallback that inherited the homepage's POV and
    // duplicated the current language (e.g. "EN / EN").
    let nav_builder = if let Some(d) = doc {
        let lang_roots = super::lang_roots::site_lang_roots(all_docs, site_lang);
        let multi = super::lang_roots::site_publishes_multiple_languages(all_docs, &lang_roots, site_lang);
        let links = crate::i18n::site_languages::other_language_links(
            &page_lang_tag, &d.translations, &lang_roots, multi);
        // Always adopt the page's own language (nav-item scoping + localized
        // labels), whether or not translation links exist. An empty `links`
        // simply renders no language toggle.
        nav_builder.with_translations(d.lang, &page_lang_tag, links)
    } else {
        nav_builder
    };

    let footer_html = Some(nav_builder.generate_footer(show_rss_in_footer));

    let processor = ShellProcessor::new();

    // Determine template type based on layout and content folders
    let shell_type = ShellRegistry::select_shell_type(doc, is_homepage, project.has_content_folders);
    let is_article_page = shell_type == ShellType::Article;

    // True when the page should advertise a social share card + be indexed: a
    // real public page (not draft/slot_only). `listed: false` pages are still
    // public, so they keep their card. Shared by the og/twitter/cover/
    // auto-card/JSON-LD gates so they cannot drift apart.
    let is_card_eligible = doc.map_or(false, |d| d.is_public_page());

    // Non-public pages (drafts) carry a noindex robots directive so crawlers
    // never index them even if the URL leaks. Public pages — including
    // `listed: false` (off-feed but indexable) — get no robots meta.
    let robots_meta = match doc {
        Some(d) if !d.is_public_page() => {
            Some("\n    <meta name=\"robots\" content=\"noindex\">".to_string())
        }
        _ => None,
    };

    // Prepare content based on page type
    let (page_title, homepage_content) = match (doc, is_homepage) {
        (Some(doc), true) => {
            // Homepage with document (generate_homepage_with_blog_feed equivalent)
            //
            // (An `<article>`-unwrapping slice used to sit here. `html_content`
            // is a body fragment — `<article>` only ever comes from the
            // article-content template, which wraps this value rather than
            // being wrapped by it — so the slice never fired.)

            // Obsidian-match: the homepage injects NO title h1 of its own, so
            // an authored leading `# Foo` is the author's content and is kept
            // verbatim. (Removed the pre-2026-05-30 dedup that stripped a
            // leading <h1> matching doc.title — it only ever deleted the
            // author's heading.) See docs/reference/title-rendering.md.

            // Build the media lookup once: shared between folder-card
            // color resolution and the post-pass placeholder enrichment.
            let media_lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
                &project.image_files,
                &project.video_files,
                &dir_overrides,
                // One page in isolation: no `BuildServices`, so no variants.
                None,
            );

            let mut content = grid_cells::resolve_page_body(
                doc,
                all_docs,
                content_graph,
                &dir_overrides,
                &project.root_path,
                &media_lookup,
                resolved_typesetting,
            )
            .to_html();

            // The homepage has no title of moss's own for the byline to sit
            // under; render/credits.rs says where it goes instead, and why a
            // `layout: article` homepage is the article path's page, not this one.
            if !is_article_page {
                content = credits::splice_byline_at_page_head(content, &doc.byline, emit_source_lines, doc.place_line.as_deref());
            }

            // Folder card <img> tags inherit width/height/loading/LQIP/color
            // attributes from the moss-core image synthesizer at grid generation
            // time (`grid_card::render` and `folder_cover::render` thread the
            // `MediaDimensionLookup` through the synth). Phase 2E v5 PR5
            // (2026-05-26) retired the Stage 3 regex post-pass that ran here.

            // Append children list for home page
            // Uses the same generate_children pipeline as folder index pages,
            // supporting children_style auto-detection and children_depth control.
            // `children_in: sidebar` is the new (post-consolidation) way to
            // route children to the right rail; the deprecated `sidebar:` field
            // also routes via the alias. Both suppress body-children rendering
            // identically. We read `from_sidebar_alias` (not `sidebar.is_some()`)
            // so a conflict like `sidebar: "[[A]]" + children: "[[B]]"` — where
            // the alias yields and warns "sidebar ignored" — actually renders
            // body children rather than silently suppressing them.
            let has_sidebar = doc.from_sidebar_alias.unwrap_or(false)
                || doc.children_in.as_deref() == Some("sidebar");

            // Resolve the target folder path for the marker synthesis (the
            // double-listing suppression check that also consumed it was removed
            // with the children-style revert; only the synthesis remains).
            let target_folder_path = doc.children_source.as_deref()
                .map(|r| resolve_children_source_folder_path(r, all_docs))
                .unwrap_or_default();

            let show_children = !has_sidebar && doc.children.unwrap_or(true);
            if show_children {
                let marker = crate::build::folder_embed::synthesize_children_marker(
                    doc,
                    &target_folder_path,
                    "index.md",
                    true,  // is_homepage = true
                );
                let resolved_html = crate::build::folder_embed::resolve_markers(
                    &marker,
                    "index.md",
                    all_docs,
                    project,
                    &dir_overrides,
                    doc.lang, // per-page language, not site default
                    resolved_typesetting,
                    Some(&media_lookup),
                    layout_config.assets.math,
                );
                content.push_str(&annotate_children_listing(resolved_html, emit_source_lines));
            }

            // Colophon at the very foot, after the children listing.
            if !is_article_page {
                credits::push_colophon(&mut content, &doc.colophon, emit_source_lines);
            }

            // Homepage <title> tag is chrome; use the plain-text label.
            (doc.label.clone(), content)
        }
        (Some(doc), false) => {
            // Regular page content. Body H1 (if any) is preserved verbatim and
            // is the sole source of the visible heading — moss never injects one.
            // See docs/archive/2026-04-17-title-simplification.md.
            // Build the media lookup once: shared between folder-card
            // color resolution and the post-pass placeholder enrichment.
            let media_lookup = crate::build::media::dimensions::MediaDimensionLookup::new(
                &project.image_files,
                &project.video_files,
                &dir_overrides,
                // One page in isolation: no `BuildServices`, so no variants.
                None,
            );

            let body = grid_cells::resolve_page_body(
                doc,
                all_docs,
                content_graph,
                &dir_overrides,
                &project.root_path,
                &media_lookup,
                resolved_typesetting,
            );
            let mut content = body.to_html();

            // Check if this is a folder index page (any non-root index page).
            // Must use doc.kind == PageKind::Folder (source was index.md/readme.md/_index.md/main.md)
            // because all pages have pretty URLs ending in /index.html.
            let is_folder_index =
                doc.kind == PageKind::Folder && doc.url_path.ends_with("/index.html") && doc.url_path != "index.html";

            // A home-override page (`home: true` on a non-root file like
            // `en/Mountain Home.md` → `en/index.html`) is the language-specific
            // homepage, not a folder listing. The root home (`/index.html`)
            // skips the folder-title H1 because its `is_homepage: true` branch
            // never enters this block; home-override pages must do the same to
            // stay symmetric with the user's mental model — "click language
            // toggle, see the other language's home page, identically styled."
            //
            // Reads the ONE centralized signal `doc.is_home_override` (set in
            // the markdown pipeline) instead of re-deriving from
            // `translation_key == "home"`.
            //
            // Additionally, a filename-based language root home
            // (`zh-hans/index.md` → `zh-hans/index.html`) must also suppress
            // the folder-title H1. These pages use the standard `index.md`
            // convention inside a language-tree subfolder, so `is_home_override`
            // is `false` (only `home: true` frontmatter sets it), but they ARE
            // the language-specific homepage. The depth-1 slash-count guard
            // is LOAD-BEARING: `zh-hans/index.html` (one '/') qualifies, but
            // `zh-hans/news/index.html` (two '/') is a genuine content section
            // and must KEEP its folder-title H1.
            let is_lang_tree_root_home = is_folder_index
                && doc
                    .source_path
                    .as_deref()
                    .and_then(|p| moss_core::home::lang_tree_prefix(p))
                    .is_some()
                && doc.url_path.chars().filter(|&c| c == '/').count() == 1;
            let is_home_override = is_folder_index && (doc.is_home_override || is_lang_tree_root_home);

            // Wrap content in book-open layout with cover on the left (before
            // placeholder pass so LQIP attributes are added to the cover <img>).
            // Title and byline are prepended into the cover body so they
            // appear beside the cover, not above it.
            if is_folder_index && !is_home_override {
                // Split pipe-encoded cover to separate path from display attrs.
                let (resolved_cover, cover_attrs) = match doc.cover.as_deref() {
                    Some(c) => {
                        let (path_part, attrs_str) = moss_core::media::split_pipe(c);
                        (
                            Some(path_resolver.resolve_url(path_part)),
                            moss_core::media::parse_media_attrs(attrs_str),
                        )
                    }
                    None => (None, moss_core::media::MediaAttrs::default()),
                };
                // Source for the visible folder-title h1 is `doc.title` — the
                // index cascade `title:` if set, else the filename/folder name
                // (`filename_text`), NEVER body content (Obsidian-match,
                // 2026-05-30). An author's body `# Custom` is kept verbatim as
                // content and renders below this injected folder-title h1; moss
                // no longer dedups it. See docs/reference/title-rendering.md.
                let h1_text = &doc.title;
                // `layout: article` on a folder-index page reads as a plain
                // article: no auto-inserted cover component (row or hero) at
                // all, even when `cover:` is set. The article body owns its
                // own hero/imagery inline; moss never overrides that
                // rendering treatment for this layout.
                let is_article_layout = doc.layout.as_deref() == Some("article");
                // Credits are page-level, not article-level: `byline:` under
                // the title, `colophon:` at the foot, here as on an article.
                // (`description:` held the byline's slot as a standfirst until
                // 2026-08-08; it is metadata — see components/folder_cover.rs.)
                // Skipped when `is_article_page`, because a `layout: article`
                // folder runs through BOTH assemblies and the article path
                // below emits both. Same boolean as that gate, so "exactly one
                // fires" reads off one variable. See render/credits.rs.
                let folder_byline = (!is_article_page)
                    .then(|| credits::render_byline_html(&doc.byline, emit_source_lines, doc.place_line.as_deref()))
                    .flatten()
                    .unwrap_or_default();
                if resolved_cover.is_some() && !is_article_layout {
                    // Cover branch: folder_cover renders the cover-row with the
                    // <h1 class="moss-folder-title"> inside
                    // .moss-collection-cover-body.
                    //
                    // The cover thumbnail takes about a third of the content
                    // width, so the cover-body column beside it is narrow.
                    // Only the lede belongs there: a `:::grid` typeset in that
                    // column is squeezed to a fraction of its intended width
                    // (`Illuminated Books.md` — `cover:` + `:::grid 2`), and a
                    // long-form article body is read in a ~20-character measure
                    // with half the viewport empty beside it (moss#903 bug 4).
                    //
                    // `BodyPlan::lede_segments` is where the plan says the lede
                    // ends. Everything past it re-appends as a plain sibling
                    // AFTER the cover row closes, the same placement the
                    // auto-generated children listing already uses. The split
                    // lands on a block boundary between independently
                    // serialized segments, so neither side can end mid-element
                    // — the question the old byte-walking `split_lead_before_grid`
                    // tried to answer by counting tags, and got wrong on the
                    // first multi-byte character.
                    let (lead, trailer) = body.split_at_lede();
                    let cover_type = crate::build::media::cover::detect_cover_type(
                        resolved_cover.as_deref().unwrap_or(""),
                        doc.cover_type.as_deref(),
                    );
                    // Byline between the folder title and the body, in-column.
                    let cover_row_html = components::folder_cover::render(
                        resolved_cover.as_deref(),
                        h1_text,
                        &format!("{}{}", folder_byline, lead),
                        cover_type,
                        &cover_attrs,
                        Some(&media_lookup),
                        emit_source_lines,
                    );
                    content = format!("{}{}", cover_row_html, trailer);
                } else {
                    // No-cover branch: prepend the <h1 class="moss-folder-title">
                    // unless this folder is a nav item — the nav bar already
                    // shows its title.
                    // Same byline as the cover branch, directly after the title.
                    // Both empty still means empty, so the guard below keeps
                    // working for a nav folder with no byline.
                    let heading = format!(
                        "{}{}",
                        no_cover_folder_heading(
                            doc,
                            h1_text,
                            project.has_content_folders,
                            emit_source_lines,
                        ),
                        folder_byline,
                    );
                    content = if heading.is_empty() {
                        content
                    } else {
                        format!("{}\n{}", heading, content)
                    };
                }
            } else if !is_article_page {
                // No title block of moss's own — a home-override or language-root
                // folder page, or a plain page — so the homepage rule applies.
                content = credits::splice_byline_at_page_head(content, &doc.byline, emit_source_lines, doc.place_line.as_deref());
            }

            // Folder card <img> tags: same LQIP/dimensions inheritance as the
            // homepage branch above.

            // Append article listing for folder index pages. D1: children is
            // bool — true/None = show, false = hide; `has_sidebar` routing is
            // the same alias-aware check as the homepage branch above.
            let has_sidebar = doc.from_sidebar_alias.unwrap_or(false)
                || doc.children_in.as_deref() == Some("sidebar");
            // A page that won a term claim (`author_page:`/`tag_page:`) hosts
            // that term's member listing at any path — article or folder index.
            // `term_listing` is only set when the author didn't route
            // `children` explicitly, so the claim never overrides their intent
            // (`build::terms::derive_terms`).
            let term_listing = doc.term_listing.as_deref();
            // An ordinary page can host ANOTHER folder's listing too (archive
            // §4, 2026-09-11) — a folder index or term listing keeps its own.
            let children_source_target = doc.children_source.as_deref()
                .map(|r| resolve_children_source_folder_path(r, all_docs));
            if is_folder_index || term_listing.is_some() || children_source_target.is_some() {
                let folder_path = term_listing.map(str::to_string).unwrap_or_else(|| {
                    if is_folder_index {
                        doc.url_path.trim_end_matches("/index.html").trim_end_matches("index.html").trim_end_matches('/').to_string()
                    } else {
                        children_source_target.unwrap_or_default()
                    }
                });
                // children_source's host needs its REAL source path here
                // (not a folder_path guess), so render_one's self-exclusion
                // — matching `from` against `source_path` — can find it.
                let from_md_path = if term_listing.is_some() || is_folder_index {
                    if folder_path.is_empty() { "index.md".to_string() } else { format!("{}/index.md", folder_path) }
                } else {
                    doc.source_path.clone().unwrap_or_else(|| format!("{}/index.md", folder_path))
                };

                let show_folder_children = !has_sidebar && doc.children.unwrap_or(true);
                if show_folder_children {
                    let marker = crate::build::folder_embed::synthesize_children_marker(
                        doc,
                        &folder_path,
                        &from_md_path,
                        false, // is_homepage = false
                    );
                    let resolved_html = crate::build::folder_embed::resolve_markers(
                        &marker,
                        &from_md_path,
                        all_docs,
                        project,
                        &dir_overrides,
                        doc.lang, // per-page language, not site default
                        resolved_typesetting,
                        Some(&media_lookup),
                        layout_config.assets.math,
                    );
                    content.push_str(&annotate_children_listing(resolved_html, emit_source_lines));
                }
            }

            // Colophon at the very foot — after the children listing, which is
            // part of the page. Same `is_article_page` gate as the byline above,
            // and the same position the article path would use.
            if !is_article_page {
                credits::push_colophon(&mut content, &doc.colophon, emit_source_lines);
            }

            // The <title> tag is chrome; use the plain-text label.
            (doc.label.clone(), content)
        }
        (None, false) => {
            // Auto-generated index page with just the article list. When the
            // folder has no listable articles the list is empty — render
            // nothing rather than an empty card container, so an empty folder
            // stays honest (matches the placeholder removal in year_group /
            // folder_embed).
            let article_list =
                generate_year_grouped_article_list(all_docs, project, true, site_lang, resolved_typesetting);
            let content = if article_list.is_empty() {
                String::new()
            } else {
                format!(r#"<div class="moss-cards-container"><div class="moss-cards" data-layout="minimal">{}</div></div>"#, article_list)
            };
            (site_title.clone(), content)
        }
        (None, true) => {
            return Err("Cannot generate homepage without document".to_string());
        }
    };

    // Compute the browser-tab <title> text once, in Rust, for ALL pages.
    // Non-homepage pages get "{page title} - {site title}" so that e.g. a folder
    // index at /docs/ gets "<title>Documentation - moss</title>"; the homepage (and
    // any page whose title already equals the site title) keeps just the site name.
    // Templates all use a bare `{title}` — there is no `{site_name}` template var.
    let title_for_tab = tab_title(&page_title, &site_title, is_homepage);

    // Single unified navigation for all pages
    let navigation = nav_builder.generate_navigation();
    // …plus its floating continuation for long pages (ADR-049). Empty unless
    // this page has a breadcrumb trail, and empty site-wide when the author
    // turned the island off in Settings → Services.
    let nav_island = if layout_config.floating_nav {
        nav_builder.generate_nav_island()
    } else {
        String::new()
    };

    // Generate series navigation for article pages in ordered folders.
    //
    // Output routing: article pages emit series-nav into the `{post_article}`
    // template token (sibling of `<article>` inside `<main>`, AFTER the
    // `<!-- slot:after-article -->` plugin slot). This makes the comment toggle
    // — which injects into `slot:after-article` — sit DIRECTLY before
    // series-nav, so the toggle hugs series-nav's `border-top` divider with
    // mirrored gaps (see `.moss-comments + .moss-series-nav` in site.css).
    let mut homepage_content = homepage_content;
    let mut post_article = String::new();
    if is_article_page && doc.is_some() {
        let d = doc.unwrap();
        // Find parent folder's index doc to check for `series`
        let parts: Vec<&str> = d.url_path.split('/').collect();
        if parts.len() >= 3 {
            // e.g. "series/part-1/index.html" -> parent folder = "series"
            let parent_folder = parts[..parts.len() - 2].join("/");
            let parent_index = format!("{}/index.html", parent_folder);
            if let Some(parent_doc) = all_docs.iter().find(|pd| pd.url_path == parent_index) {
                // Decide whether to render series-nav and, if so, sort siblings via
                // the resolved-sort dispatch on the parent folder.
                //
                // Chrome trigger (Task 13):
                //   1. parent.series == Flag(true)  -> chrome on
                //   2. parent.series == Flag(false) -> chrome off
                //   3. parent.series == None        -> direct_children_sort.series_default
                //      (true when axis == Weight OR explicit_order is Some — the
                //      inference says "this folder looks ordered")
                //   4. The leaf page can opt out with `series: false`,
                //      regardless of parent (Task 13 step 2). Opting out is
                //      whole-chain, not just self: the page renders no nav of
                //      its own (`nav_enabled` below) AND drops out of every
                //      sibling's chain (`is_sequence_step`), so its neighbours
                //      become adjacent to each other. Half of that — silencing
                //      only its own nav — would leave the page before it
                //      pointing "next →" at a page that leads nowhere back.
                //
                // After `FrontMatter::normalize()` runs (Task 5), the legacy
                // `series: [list]` form (SeriesField::Ordered) is consumed into
                // `sort: List + series: Flag(true)`, so only Flag/None reach here
                // for both parent and leaf.
                //
                // Route through the direct-children helper to match the
                // new contract — series-nav iterates direct siblings only,
                // so re-resolution on a flatten scope would be wrong.
                let resolved = parent_doc.resolve_for_direct_children();
                let parent_chrome_explicit: Option<bool> = match &parent_doc.series {
                    Some(crate::build::types::SeriesField::Flag(b)) => Some(*b),
                    Some(crate::build::types::SeriesField::Ordered(_)) => Some(true), // defensive (normalize rewrites this)
                    None => None,
                };
                let parent_chrome_default = resolved.series_default;
                let parent_chrome_on = parent_chrome_explicit.unwrap_or(parent_chrome_default);
                let page_opted_out = matches!(
                    &d.series,
                    Some(crate::build::types::SeriesField::Flag(false))
                );
                let nav_enabled = parent_chrome_on && !page_opted_out;
                let sorted_siblings: Option<Vec<&ParsedDocument>> = if nav_enabled {
                    let parent_prefix = format!("{}/", parent_folder);
                    let sorted = sequence_siblings(
                        all_docs, &parent_index, &parent_prefix, &resolved,
                    );
                    if sorted.is_empty() { None } else { Some(sorted) }
                } else {
                    None
                };

                if let Some(ref siblings) = sorted_siblings {
                    // By url_path: every folder index's clean_stem is "index" (#1012).
                    if let Some(pos) = siblings.iter().position(|pd| pd.url_path == d.url_path) {
                        let prev = if pos > 0 {
                            // chrome; plain-text label
                            Some((siblings[pos - 1].label.clone(),
                                  path_resolver.resolve_url(&siblings[pos - 1].url_path)))
                        } else {
                            None
                        };
                        let next = if pos + 1 < siblings.len() {
                            // chrome; plain-text label
                            Some((siblings[pos + 1].label.clone(),
                                  path_resolver.resolve_url(&siblings[pos + 1].url_path)))
                        } else {
                            None
                        };
                        let resolved_parent = path_resolver.resolve_url(&parent_index);
                        // Series nav parent title is chrome; use the plain-text label.
                        let series_html = components::series_nav::render(
                            &parent_doc.label,
                            &resolved_parent,
                            prev.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
                            next.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
                            // Where you are, over the same set the arrows walk.
                            // A lone page in a folder gets no ordinal — "1 of 1"
                            // is noise, not orientation.
                            if siblings.len() > 1 {
                                Some((pos + 1, siblings.len()))
                            } else {
                                None
                            },
                            d.lang, resolved_typesetting,
                        );
                        if !series_html.is_empty() {
                            // Article pages: emit as sibling of <article>
                            // via the {post_article} template token.
                            post_article.push_str(&series_html);
                        }
                    }
                }
            }
        }
    }

    // Sidebar rendering: source comes from one of two paths:
    //   - Deprecated alias: `doc.from_sidebar_alias = Some(true)` means
    //     `apply_sidebar_alias` ran and `doc.sidebar` is the original wikilink.
    //   - Direct new form: `doc.children_in == "sidebar"` + `doc.children_source`.
    //
    // The alias provenance flag (not `doc.sidebar.is_some()`) gates which path
    // is active. Otherwise a conflict like `sidebar: "[[A]]" + children: "[[B]]"`
    // — where the alias deliberately yields and warns "sidebar is ignored" —
    // would still drive the right rail from `sidebar:`, contradicting the warning.
    //
    // Two display modes:
    //   - Cross-referencing (target folder != current page's folder): truncated +
    //     "More" link. Alias path: always 3 items + always-show More (preserves
    //     today's behavior). Direct path: cap at `children_limit` if set, More
    //     only when truncated.
    //   - Self-referencing (target folder == current page's folder): all items,
    //     no "More" link.
    let sidebar_source: Option<&str> = doc.and_then(|d| {
        if d.from_sidebar_alias.unwrap_or(false) {
            d.sidebar.as_deref()
        } else if d.children_in.as_deref() == Some("sidebar") {
            d.children_source.as_deref()
        } else {
            None
        }
    });
    let use_sidebar_layout = sidebar_source.is_some();

    let (sidebar_html, page_wrapper_class) = if use_sidebar_layout {
        let sidebar_ref = sidebar_source.unwrap();
        let target_name = crate::build::markdown::frontmatter_ref_to_stem(sidebar_ref);

        // Resolve the sidebar reference: find a folder index whose slug matches.
        // Match against the last path segment of folder index pages (case-insensitive).
        let target_folder_doc = all_docs.iter().find(|d| {
            if !d.url_path.ends_with("/index.html") {
                return false;
            }
            let slug = d.url_path.trim_end_matches("/index.html");
            // Match on the last segment (e.g., "news" from "blog/news/index.html")
            let last_segment = slug.rsplit('/').next().unwrap_or(slug);
            last_segment.eq_ignore_ascii_case(&target_name)
                || d.clean_stem.eq_ignore_ascii_case(&target_name)
        });

        if let Some(target_doc) = target_folder_doc {
            let target_slug = target_doc.url_path.trim_end_matches("/index.html");
            let target_prefix = format!("{}/", target_slug);

            // Detect self-referencing: the current page lives under (or is) the target folder
            let is_self_ref = doc.map_or(false, |d| {
                let current_prefix = if d.url_path.ends_with("/index.html") && d.url_path != "index.html" {
                    d.url_path.trim_end_matches("/index.html").to_string()
                } else {
                    // For non-index pages, use parent folder
                    d.url_path.rsplitn(2, '/').nth(1).unwrap_or("").to_string()
                };
                current_prefix == target_slug
            });

            // Sidebar article collection is based on physical folder membership only.
            // also_in cross-references are not supported in wikilink-based sidebars —
            // articles must be physical children of the target folder.
            //
            // Collect dated children of the target folder
            let mut dated_articles: Vec<components::ArticleListItemProps> = all_docs.iter()
                .filter(|d| {
                    d.url_path.starts_with(&target_prefix)
                        && d.url_path != target_doc.url_path
                        && d.is_listable()
                        && d.date.is_some()
                })
                // Sidebar always shows direct children only, regardless of children_depth.
                // Nested subfolder articles are excluded from sidebar listings.
                .filter(|d| is_direct_child(&d.url_path, &target_prefix))
                .map(|d| {
                    let (date_display, date_raw, _explicit) = components::extract_date_from_doc(d, &project.root_path);
                    components::ArticleListItemProps {
                        date_display,
                        date_raw,
                        url: crate::build::scan::article_map::to_pretty_url(&d.url_path),
                        // chrome; plain-text label
                        title: d.label.clone(),
                        url_path: d.url_path.clone(),
                    }
                })
                .collect();

            dated_articles.sort_by(|a, b| moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key()));

            // Resolve effective limit and More-link rule:
            //   - Alias path (`doc.sidebar` is set): legacy default 3 on cross-ref;
            //     explicit `children_limit` overrides; More always shown on
            //     cross-ref to preserve today's behavior.
            //   - Direct path (only `children_in: sidebar`, no `sidebar:` field):
            //     respect `children_limit` if set, no cap otherwise; More shown
            //     only when truncation actually happened.
            //
            // The `from_alias` branch exists to keep today's "More always on cross-ref"
            // semantics for sites still using `sidebar:`. When the alias is removed
            // (#633), every sidebar feed uses the truncation-only rule — same as
            // the body feed.
            let from_alias = doc.map_or(false, |d| d.from_sidebar_alias.unwrap_or(false));
            // `Some(0)` is treated as "no limit" rather than "render zero items"
            // — a 0 cap would produce a header + no items + More link, which is
            // user error rather than a meaningful state.
            let explicit_limit = doc
                .and_then(|d| d.children_limit)
                .filter(|n| *n > 0)
                .map(|n| n as usize);
            let effective_limit: Option<usize> = if is_self_ref {
                None
            } else {
                explicit_limit.or(if from_alias { Some(3) } else { None })
            };

            let target_url = crate::build::scan::article_map::to_pretty_url(&target_doc.url_path);
            let (articles_to_show, more_url) = if is_self_ref {
                (dated_articles, None)
            } else if let Some(n) = effective_limit {
                let did_truncate = dated_articles.len() > n;
                let truncated: Vec<_> = dated_articles.into_iter().take(n).collect();
                let show_more = if from_alias { true } else { did_truncate };
                (truncated, if show_more { Some(target_url) } else { None })
            } else {
                (dated_articles, None)
            };

            let sidebar = components::sidebar_nav::render(
                &articles_to_show,
                more_url.as_deref(),
                ui_lang,
            );
            (Some(sidebar), " has-sidebar".to_string())
        } else {
            // Wikilink didn't resolve to any folder — skip sidebar silently
            (None, String::new())
        }
    } else {
        (None, String::new())
    };

    // Decide whether this page needs the <model-viewer> head script, before
    // homepage_content is moved into ShellVars::content below.
    let embed_head_assets = if moss_core::render::model::page_needs_model_viewer_script(&homepage_content) {
        moss_core::render::model::MODEL_VIEWER_SCRIPT.to_string()
    } else {
        String::new()
    };

    // Resolve the homepage doc the current page's DESCRIPTION cascades from.
    // Per-language: pulls from the page's-language homepage when available,
    // with site-default homepage as backup (otherwise zh-hans/ pages would
    // surface the English homepage's description on share cards). Active for
    // any non-homepage doc (articles AND folder-index pages); the homepage
    // itself uses its own data via `doc` and skips the cascade so we don't
    // double-walk the page rungs against the site-wide ones. The cover chain
    // has no site-wide tail (cover.rs module doc).
    let cascade_homepage: Option<&crate::build::types::ParsedDocument> = if doc.is_some() && !is_homepage {
        find_homepage_doc(all_docs, ui_lang, site_lang)
    } else {
        None
    };
    // The page's own picture, or the card moss draws when it has none — and
    // when the picture is a shape no platform crop can hold, the card it
    // draws AROUND that picture. `og_choice` holds the whole decision.
    let (resolved_cover_for_og, cover_dims_for_og) = crate::build::page::og_choice::resolve(
        &crate::build::page::og_choice::OgCoverInputs {
            doc,
            project,
            path_resolver: &path_resolver,
            source_root,
            dir_overrides,
            site_title: &site_title,
            is_homepage,
            is_card_eligible,
            ui_lang,
            vertical_typesetting,
            output_dir,
        },
        og_outputs.as_deref_mut(),
    );

    // Build the date-line HTML once for article pages with a date.
    // We splice it into homepage_content (just below the title block) so the
    // reader sees: H1 → optional blockquote-deck → date row + reading prefs →
    // body. The {date_line} template token is left empty so the template's
    // legacy "above the body" position is no longer used.
    let article_date_line_html: Option<String> = if is_article_page
        && doc.is_some()
        && doc.unwrap().date.is_some()
    {
        let d = doc.unwrap();
        let date_str = d.date.as_ref().unwrap();
        let doc_lang = d.lang;
        let formatted = components::format_display_date(date_str, doc_lang, resolved_typesetting);
        let date_fm_attr = if emit_source_lines { r#" data-source-fm="date""# } else { "" };
        Some(format!(
            r#"<div class="date-line"><span class="date"{}>{}</span><div class="font-anchor"><button class="font-trigger size-std" aria-label="{}" aria-expanded="false" type="button"></button><div class="font-pill" id="fontPill"><button data-scale="small" aria-label="{}"></button><button data-scale="" class="active" aria-label="{}"></button><button data-scale="large" aria-label="{}"></button><button data-scale="xlarge" aria-label="{}"></button></div></div></div>"#,
            date_fm_attr, formatted,
            crate::i18n::t(doc_lang, "reading_preferences"),
            crate::i18n::t(doc_lang, "text_small"),
            crate::i18n::t(doc_lang, "text_standard"),
            crate::i18n::t(doc_lang, "text_large"),
            crate::i18n::t(doc_lang, "text_xlarge"),
        ))
    } else {
        None
    };

    // Splice the date-line AND the `after-title` slot marker into
    // homepage_content so both land after the title block (H1 + optional
    // blockquote deck) rather than above it. The review colophon / book block
    // is injected into the `after-title` marker by the enhance hook; emitting
    // the marker HERE — directly below the date / reading-prefs row — makes that
    // block render as the first thing after the title and date, before the body.
    // (The marker previously lived in the article template ABOVE `{content}`,
    // which rendered the book block above the title.) The marker is emitted for
    // every article page, with or without a date, so dateless reviews keep their
    // colophon; an unfilled marker is stripped by inject_slots. See
    // splice_after_title_block in html_post.rs for the title-block detection
    // (matches the # H1 + > blockquote deck pattern documented in
    // docs/reference/content-structure.md).
    if is_article_page {
        let mut after_title_block = article_date_line_html.unwrap_or_default();
        // Byline rows (frontmatter `byline:`) sit between the date row and the
        // after-title slot: they belong to the masthead, but they are not part
        // of `.date-line`, which is a single flex row owning the reading-size
        // control — and which is emitted only when the page has a date, while
        // a byline must render with or without one.
        if let Some(byline) = doc
            .and_then(|d| credits::render_byline_html(&d.byline, emit_source_lines, d.place_line.as_deref()))
        {
            after_title_block.push_str(&byline);
        }
        after_title_block.push_str("<!-- slot:after-title -->");
        homepage_content = splice_after_title_block(&homepage_content, &after_title_block);
        // Colophon rows go at the very end of the article body — after the
        // prose and any footnote section, inside `<article>`. Where the piece
        // first ran, contributor bios, production credits: everything a reader
        // does not need before the piece.
        if let Some(d) = doc {
            credits::push_colophon(&mut homepage_content, &d.colophon, emit_source_lines);
        }
    }

    // Resolve the share-card description once for the page, using the
    // 6-rung fallback chain: page description → page hero overlay →
    // page body → homepage description → homepage hero overlay → homepage
    // body. Article pages (sub-pages of the homepage) read all rungs; the
    // homepage itself uses only its own rungs 1-3 to avoid double-walking.
    // Other page types (folder index) get the per-page rungs and skip the
    // homepage cascade for consistency with the existing cover behavior.
    let share_card_description: Option<String> = doc.and_then(|d| {
        let inputs = crate::build::page::meta::DescriptionChainInputs {
            page_description: d.description.as_deref(),
            page_hero_overlay_text: d.hero_overlay_text.as_deref(),
            page_content: &d.content,
            homepage_description: cascade_homepage.and_then(|h| h.description.as_deref()),
            homepage_hero_overlay_text: cascade_homepage
                .and_then(|h| h.hero_overlay_text.as_deref()),
            homepage_content: cascade_homepage.map(|h| h.content.as_str()),
            math: layout_config.assets.math,
        };
        crate::build::page::meta::resolve_page_description_with_fallbacks(&inputs)
    });
    let share_card_description_str: &str = share_card_description.as_deref().unwrap_or("");

    // Homepage meta URL: absolute root when deployed, "/" otherwise. Computed
    // once here so the og:type=website tags and the WebSite JSON-LD node share
    // the exact same value (no re-derivation drift between the two meta blocks).
    let homepage_meta_url: String = if site_url.is_deployed() {
        site_url.as_str().to_string()
    } else {
        "/".to_string()
    };

    // Build template variables
    let vars = ShellVars {
        title: title_for_tab,
        css_path: path_resolver.css_path(),
        js_path: path_resolver.js_path(),
        lazy_chunk_attrs: path_resolver.lazy_chunk_attrs(
            scripts.hash("share-card"),
            layout_config.assets.video_ladder.then(|| scripts.hash("hls")),
        ),
        navigation,
        nav_island,
        homepage_content: homepage_content.clone(),
        latest_list: None,
        favicon: Some(path_resolver.favicon_link()),
        rss_link: rss_link.map(|s| s.to_string()),
        analytics: analytics_script.map(|s| s.to_string()),
        footer: footer_html,
        // Article-specific variables
        date: if is_article_page && doc.is_some() {
            doc.unwrap().date.clone()
        } else {
            None
        },
        formatted_date: if is_article_page && doc.is_some() && doc.unwrap().date.is_some() {
            let date_str = doc.unwrap().date.as_ref().unwrap();
            Some(components::format_display_date(date_str, doc.unwrap().lang, resolved_typesetting))
        } else {
            None
        },
        // The date-line HTML is now spliced into `homepage_content` directly
        // (see article_date_line_html above) so it lands AFTER the title block
        // rather than above it. The {date_line} template token is therefore
        // always empty; the article.html template still has the token for
        // back-compat with custom templates that consume it.
        date_line: None,
        short_date: if is_article_page && doc.is_some() && doc.unwrap().date.is_some() {
            let date_str = doc.unwrap().date.as_ref().unwrap();
            Some(components::format_compact_date(date_str, doc.unwrap().lang, resolved_typesetting))
        } else {
            None
        },
        content: if is_article_page {
            Some(homepage_content)
        } else {
            None
        },
        // `data-page="home"` is the only thing that tells a stylesheet which
        // page it is on. A site's front page is routinely treated differently
        // from the rest of it — a hero that fills the screen, no footer under
        // it — and without a marker on <body> a theme has to guess from
        // content that happens to be unique to the homepage today.
        body_attrs: {
            let mut attrs = String::new();
            if emit_source_lines {
                attrs.push_str(" data-moss-preview");
            }
            if is_homepage {
                attrs.push_str(r#" data-page="home""#);
            }
            attrs
        },
        page_wrapper_class,
        // Hero section from document (placed outside <main> for full-width).
        // Enabled for all pages (homepage + articles) — needed for image pages
        // and any article using :::hero shortcode.
        hero_section: doc.and_then(|d| d.hero_html.clone()),
        // The cover and the QR the page itself has, published to the reader
        // runtime on `<article class="container">` so share-card.ts finds
        // neither by guessing at markup. Absent attribute = the card draws
        // nothing there, and each rule lives with the code that decides it.
        share_cover_attr: doc
            .map(|d| {
                crate::build::page::cover::share_cover_attr(
                    d.hero_image_url.as_deref(),
                    d.cover.as_deref(),
                    d.cover_type.as_deref(),
                    |raw| path_resolver.resolve_url(raw),
                )
            })
            .unwrap_or_default(),
        // No document means the synthetic homepage — a public page at the root,
        // and one a reader can share like any other. Leaving it out here was
        // the same gap folder notes had: a card with an empty corner on the one
        // page most likely to be shared.
        share_qr_attr: match doc {
            Some(d) => qr::share_qr_attr(&d.url_path, d.is_public_page(), site_url),
            None => qr::share_qr_attr("index.html", true, site_url),
        },
        // Main element class for sidebar layout
        main_class: if use_sidebar_layout {
            "has-sidebar".to_string()
        } else {
            String::new()
        },
        // Sidebar is opt-in: only inject when the `sidebar` field is set
        latest_sidebar: if use_sidebar_layout {
            sidebar_html
        } else {
            None
        },
        // Semantic markup (Open Graph and Schema.org)
        // Resolve description for ALL pages with a source document, not just articles/homepage.
        // Folder index pages (Page template) also need <meta name="description"> populated.
        // share_card_description was computed above using the 6-rung fallback chain.
        description: share_card_description.clone(),
        og_tags: if is_article_page && is_card_eligible {
            let d = doc.unwrap();
            // Build a typed page path from the full url_path (e.g. "about/index.html"),
            // then derive the pretty URL by stripping the trailing "index.html" from the
            // relative form — preserving the trailing slash for directory pages.
            let page_path = crate::build::served_path::ServedPath::from_source(&d.url_path)
                .map_err(|e| format!("og:url path for {}: {}", d.url_path, e))?;
            let page_relative = page_path.to_relative_url()
                .trim_end_matches("index.html").to_string();
            // og:url: only emit absolute URL when deployed; otherwise use the
            // relative path so unconfigured builds don't emit http://localhost URLs.
            let url = if site_url.is_deployed() {
                site_url.to_absolute(&page_relative)
            } else {
                page_relative
            };
            // og:title is chrome; use the plain-text label.
            Some(crate::build::page::meta::build_og_tags(
                &d.label,
                share_card_description_str,
                &url,
                &site_title,
                d.date.as_deref(),
                resolved_cover_for_og.as_ref(),
                cover_dims_for_og,
                d.tags.as_deref().unwrap_or(&[]),
                "",          // locale — TODO(4.3c): plumb real per-page lang from frontmatter
                site_url,
            ))
        } else if is_card_eligible {
            // Non-article listable page (homepage, nav landing, folder index):
            // og:type=website with a REAL og:site_name (the site, not the page
            // label). For the homepage, og:title stays the site title (the
            // homepage IS the site — preserves prior output, esp. title-less
            // homepages that fall back to a stem). For nav landing / folder
            // index pages, og:title is the page label so they unfurl with
            // their own name.
            let d = doc.unwrap();
            let (og_title, og_url): (&str, String) = if is_homepage {
                // Homepage og:url: shared with the WebSite JSON-LD node below.
                (site_title.as_str(), homepage_meta_url.clone())
            } else {
                let page_path = crate::build::served_path::ServedPath::from_source(&d.url_path)
                    .map_err(|e| format!("og:url path for {}: {}", d.url_path, e))?;
                let page_relative = page_path.to_relative_url()
                    .trim_end_matches("index.html").to_string();
                let url = if site_url.is_deployed() {
                    site_url.to_absolute(&page_relative)
                } else {
                    page_relative
                };
                (d.label.as_str(), url)
            };
            Some(crate::build::page::meta::build_og_tags_website(
                og_title,
                &site_title,
                share_card_description_str,
                &og_url,
                resolved_cover_for_og.as_ref(),
                cover_dims_for_og,
                "",          // locale — TODO(4.3c): plumb real per-page lang from frontmatter
                site_url,
            ))
        } else {
            None
        },
        // Twitter Card tags: mirror og:title/description/image so Twitter and
        // iMessage Link Presentation render correctly. Falls back to title for
        // alt text inside build_twitter_tags when None.
        twitter_tags: if is_article_page && is_card_eligible {
            let d = doc.unwrap();
            Some(crate::build::page::meta::build_twitter_tags(
                &d.label,
                share_card_description_str,
                resolved_cover_for_og.as_ref(),
                // When the cover came from auto-card, alt should match the
                // card's visible text (page label). For frontmatter covers,
                // leave None — build_twitter_tags falls back to title.
                if cover_dims_for_og.is_some() { Some(&d.label) } else { None },
                site_url,
            ))
        } else if is_card_eligible {
            // Non-article listable page: twitter:title mirrors og:title — the
            // site title on the homepage, the page label on nav landing /
            // folder index pages. Auto-card visible text matches.
            let d = doc.unwrap();
            let card_title: &str = if is_homepage { site_title.as_str() } else { d.label.as_str() };
            Some(crate::build::page::meta::build_twitter_tags(
                card_title,
                share_card_description_str,
                resolved_cover_for_og.as_ref(),
                if cover_dims_for_og.is_some() { Some(card_title) } else { None },
                site_url,
            ))
        } else {
            None
        },
        // Canonical link: only emit when deployed (is_deployed()) — localhost
        // canonical links produce noise with no benefit and could mislead crawlers.
        // `www_canonical = false` keeps canonical_url a no-op on bare hosts and
        // preserves an existing www-prefix. Not a gap: www-vs-apex is already
        // decided upstream in build::site_url's `domain` + `cdn_active` branch.
        //
        // Homepage canonical is bare `https://host/`; non-homepage pages go
        // through canonical_url() which takes a typed ServedPath.
        canonical_link: {
            // For linkblog / imported pages (`external_url` in frontmatter), the
            // canonical URL is the external destination — emit that regardless of
            // whether moss itself is deployed, so search engines de-dupe to the
            // source. JSON Feed 1.1 calls this the same as the href in a
            // linkblog post.
            let external_url = doc
                .and_then(|d| crate::build::scan::page_map::external_url(&d.raw_frontmatter));
            let target = match external_url {
                Some(src) => Some(src),
                // A page with no document is the synthetic homepage, which
                // lives at the root like any `index.md` would.
                None if site_url.is_deployed() => {
                    let url_path = doc.map(|d| d.url_path.as_str()).unwrap_or("index.html");
                    Some(
                        crate::build::page::canonical::canonical_for_url_path(site_url, url_path)
                            .map_err(|e| format!("canonical path for {}: {}", url_path, e))?,
                    )
                }
                None => None,
            };
            target.map(|url| {
                format!(
                    r#"<link rel="canonical" href="{}">"#,
                    crate::build::page::meta::escape_html_attr(&url)
                )
            })
        },
        // Gated on is_deployed() like canonical_link above: a relative hreflang
        // misleads crawlers, so absent is safer than wrong. The helper handles
        // the empty-translations and host-stripping cases itself.
        hreflang_links: if site_url.is_deployed() {
            doc.and_then(|d| crate::build::page::meta::build_hreflang_link_tags(
                &page_lang_tag,
                &d.url_path,
                &d.translations,
                site_url.as_str(),
                &layout_config.lang_tag,
            ))
        } else {
            None
        },
        // Populated when the blocking phase rasterized PNG favicon sizes
        // (`favicon_has_raster_pngs`).
        // None when no SVG was available to rasterize from — emitting an
        // SVG-pointing tag would be worse than no tag (Apple Link Presentation
        // can't use SVG and would fail more loudly than just falling back).
        apple_touch_icon: path_resolver.apple_touch_icon_link(),
        schema_json_ld: if is_article_page && is_card_eligible {
            let d = doc.unwrap();
            // Same pretty-URL derivation as og:url: build from full url_path,
            // then strip "index.html" to preserve the trailing slash for directory pages.
            let page_path = crate::build::served_path::ServedPath::from_source(&d.url_path)
                .map_err(|e| format!("schema JSON-LD path for {}: {}", d.url_path, e))?;
            let page_relative = page_path.to_relative_url()
                .trim_end_matches("index.html").to_string();
            let url = if site_url.is_deployed() {
                site_url.to_absolute(&page_relative)
            } else {
                page_relative
            };
            // Schema.org headline is chrome; use the plain-text label.
            // Reuse the same resolved_cover_for_og for the JSON-LD image field.
            Some(crate::build::page::meta::build_schema_json_ld(
                &d.label,
                share_card_description_str,
                &url,
                &site_title,
                d.date.as_deref(),
                resolved_cover_for_og.as_ref(),
                d.tags.as_deref().unwrap_or(&[]),
                site_url,
            ))
        } else if is_homepage && is_card_eligible {
            // Homepage: emit a WebSite JSON-LD node for entity identity.
            // Reuses the same description + url already fed to the og:website
            // tags, and the site language code. Gated on `is_card_eligible`
            // (= `doc.is_public_page()`) so it stays symmetric with the og:website
            // meta block, which carries the same gate — a `draft: true`
            // homepage emits neither, never one without the other.
            Some(crate::build::page::meta::build_schema_website(
                &site_title,
                share_card_description_str,
                &homepage_meta_url,
                // The PAGE's tag: this block is homepage-only, and a homepage
                // declaring `lang: fr` on an `en` site would otherwise emit
                // `<html lang="fr">` with `"inLanguage": "en"` one line apart.
                &page_lang_tag,
            ))
        } else {
            None
        },
        content_width_attr: resolve_data_attr(
            doc.and_then(|d| d.content_width.as_ref()),
            layout_config.content_width.as_ref(), "content-width", None),
        typesetting_attr: resolve_data_attr(
            doc.and_then(|d| d.typesetting.as_ref()),
            layout_config.typesetting.as_ref(), "typesetting", Some("horizontal")),
        comments_attr: resolve_comments_attr(
            doc.and_then(|d| d.comments), layout_config.comments),
        // The content's language, not the chrome's: `ui_lang`'s three variants
        // said `en` on a `fr` site while site-languages.json said `fr` (#977).
        lang: page_lang_tag.clone(),
        ui_lang,
        user_css_link: if has_user_css {
            Some(format!(
                r#"<link rel="stylesheet" href="{}" layer="themes">"#,
                path_resolver.user_css_path()
            ))
        } else {
            None
        },
        user_js_tag: if has_user_js {
            Some(path_resolver.user_js_tag())
        } else {
            None
        },
        has_sidebar_layout,
        embed_head_assets,
        post_article,
        // The same block `render::blocking` emits, from the same table and
        // the same gate facts — `LayoutConfig::assets` is now the one place
        // those facts live, so the two render paths cannot disagree about
        // which scripts a page loads.
        runtime_js_tags: scripts.shell_tags(&layout_config.assets, &path_resolver),
        robots_meta,
    };

    Ok(processor.process(shell_type, vars))
}

/// Is this page a **step in the reading order** — one of the pages the series
/// nav walks a reader through, prev by prev?
///
/// That is a different question from "does this page show up in the folder's
/// listing?" (`is_listable`), even though the two agree for every page that
/// says nothing. They part ways in exactly two places:
///
/// 1. **A subfolder's own index page that said nothing.** It belongs in the
///    listing — it is how a reader reaches the subfolder — but by default it
///    is a doorway, not a step, so it is not threaded into its siblings'
///    prev/next chain. `layout: article` overrides that: the author has said
///    "this is a piece", and a piece that happens to own an appendix folder is
///    still something you read in order (#1012). Kind comes from the kind-aware
///    `SortableDoc::is_folder_index`, never the URL — under pretty URLs *every*
///    article also ends with `<stem>/index.html`.
/// 2. **`series: false` on a leaf page.** It stays in the listing but steps
///    out of the sequence, so no sibling may point at it as prev or next. This
///    is the whole-chain half of the opt-out; the page's own nav is separately
///    suppressed by `nav_enabled` at the call site.
///
/// Every page that sets neither answers exactly as `is_listable` does.
fn is_sequence_step(pd: &ParsedDocument) -> bool {
    use moss_core::sort::SortableDoc;
    pd.is_listable()
        && (!pd.is_folder_index() || ShellRegistry::is_article_layout(pd))
        && !matches!(&pd.series, Some(crate::build::types::SeriesField::Flag(false)))
}

/// The ordered prev/next chain for the folder at `parent_prefix`: its direct
/// children that are steps in the reading order, sorted by the folder's
/// resolved sort. Excludes the folder's own index (`parent_index`).
fn sequence_siblings<'a>(
    all_docs: &'a [ParsedDocument],
    parent_index: &str,
    parent_prefix: &str,
    resolved: &moss_core::sort::ResolvedSort,
) -> Vec<&'a ParsedDocument> {
    let siblings: Vec<&ParsedDocument> = all_docs.iter()
        .filter(|pd| {
            pd.url_path != parent_index
                && pd.url_path.starts_with(parent_prefix)
                && is_sequence_step(pd)
        })
        .filter(|pd| is_direct_child(&pd.url_path, parent_prefix))
        .collect();
    moss_core::sort::sort_by_resolved(&siblings, resolved)
}

/// Is `url_path` a direct child of `prefix` — one segment below it, tolerating
/// the pretty-URL `<slug>/index.html` form? False when it is not below it at all.
fn is_direct_child(url_path: &str, prefix: &str) -> bool {
    url_path.strip_prefix(prefix).is_some_and(|rem| {
        !rem.trim_end_matches("/index.html")
            .trim_end_matches('/')
            .contains('/')
    })
}

#[cfg(test)]
#[path = "html_tests.rs"]
mod tests;
