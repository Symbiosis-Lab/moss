//! HTML generation for all page types in the moss static site generator.
//!
//! The `generate_html` function converts a `ParsedDocument` (plus site context)
//! into a complete HTML page using the template system.

use crate::build::types::ParsedDocument;
#[cfg(test)]
use crate::types::content::ProjectStructure;
use moss_core::PageKind;

// Generator submodule imports
use crate::build::components;
use crate::build::components::field_places::Placements;
use crate::i18n::Language;
use crate::build::render::grid_cells;
use crate::build::markdown::html_post::splice_after_title_block;
use crate::build::page::layout::LayoutConfig;
use crate::build::page::page::generate_year_grouped_article_list;
use crate::build::assets::paths::PathResolver;
use crate::build::media::qr;
use crate::build::markdown::body_plan::LocatorPlacement;
use crate::build::page::shell::{
    ShellProcessor, ShellRegistry, ShellType, ShellVars,
};

// Sibling module imports (within build/render/)
use super::page_chrome::{PageChromeContext, PageFamily};
#[cfg(test)]
use super::page_chrome::ChromeAssets;
use super::build_shared::BuildShared;
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

/// Resolve the site name visible on a page in `page_lang` from that edition's
/// authored root. A missing or filename-derived localized title falls back to
/// the canonical site name, so English and single-language sites are unchanged.
pub(crate) fn localized_site_title(
    all_docs: &[ParsedDocument],
    page_lang: crate::i18n::Language,
    site_lang: crate::i18n::Language,
    fallback: &str,
) -> String {
    if page_lang == site_lang {
        return fallback.to_string();
    }
    find_homepage_doc(all_docs, page_lang, site_lang)
        .filter(|d| d.url_path != "index.html" && d.source_path.is_some())
        .map(|d| d.title.clone())
        .filter(|t| !t.is_empty() && !moss_core::home::is_index_stem(t))
        .unwrap_or_else(|| fallback.to_string())
}

/// One page rendered outside any build, with shared state built for it alone.
#[cfg(test)]
pub fn generate_html(
    doc: Option<&ParsedDocument>,
    all_docs: &[ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    head_alternates: Option<&str>,
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
    // Pass `None` for og_outputs so the inner skips auto-card rendering when
    // there is no caller-provided sink to track the generated PNG path.
    // Without this, an auto-card would be written to disk and immediately
    // orphaned because there is no way to register it for stale-cleanup.
    generate_html_inner(
        doc, all_docs, project, layout_config, is_homepage, head_alternates,
        analytics_script, site_lang, css_version, has_user_css,
        has_sidebar_layout, user_css_version, has_user_js, user_js_version,
        content_graph, dir_overrides, site_url, show_rss_in_footer,
        emit_source_lines, favicon_filename, false, output_dir,
        None,
        source_root,
        &BuildShared::new(crate::build::emit::scripts::ScriptAssets::resolve(), project, dir_overrides),
    )
}

/// Like `generate_html`, but also collects the relative path of any auto-
/// generated OG card PNG that was written under `output_dir/_moss/og/`. The
/// caller appends those paths to `site_hashes.image_outputs` so blocking-
/// phase stale cleanup preserves them.
#[cfg(test)]
pub fn generate_html_collect_og<'d>(
    doc: Option<&ParsedDocument>,
    all_docs: &'d [ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    head_alternates: Option<&str>,
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
    // Built once per build by the caller, never per page.
    shared: &BuildShared<'d>,
) -> Result<String, String> {
    generate_html_inner(
        doc, all_docs, project, layout_config, is_homepage, head_alternates,
        analytics_script, site_lang, css_version, has_user_css,
        has_sidebar_layout, user_css_version, has_user_js, user_js_version,
        content_graph, dir_overrides, site_url, show_rss_in_footer,
        emit_source_lines, favicon_filename, favicon_has_raster_pngs, output_dir,
        Some(og_outputs),
        source_root, shared,
    )
}

/// The folder-index heading to prepend in the no-cover branch: the shared
/// `<h1 class="moss-folder-title">`, unless something else on the page is
/// already showing the title. Three things can be —
///
/// - the body has its own `# Title` (anywhere outside a code fence), which is
///   then both the visible title and the heading landmark: nothing is
///   emitted, for nav and non-nav folders alike,
/// - the body opens with a `:::hero` that carries its own heading: nothing is
///   emitted, since the hero already supplies the `<h1>`.
///
/// A nav folder shows its title like any other folder.
///
/// Covered folder indexes bypass this helper entirely (they render the label
/// inside the cover via `folder_cover::render`), and synthetic folder indexes
/// are generated in `render/blocking.rs` and never reach it — so this is the
/// only folder-title site that omits or hides it.
fn no_cover_folder_heading(
    doc: &crate::build::types::ParsedDocument,
    h1_text: &str,
    emit_source_fm: bool,
) -> String {
    use moss_core::heading::TitleOutcome;
    match moss_core::heading::title_outcome(&doc.content, true) {
        TitleOutcome::None => String::new(),
        TitleOutcome::Hidden => crate::build::components::folder_title::render(h1_text, emit_source_fm, true),
        TitleOutcome::Visible => crate::build::components::folder_title::render(h1_text, emit_source_fm, false),
    }
}

/// Split a pipe-encoded `cover:` value ("path|attrs") into its resolved URL
/// and display attrs. Shared by the folder-index cover branch and the
/// claimed-leaf-page cover branch below — both draw the page's own `cover:`
/// through the same two steps, and used to spell them out twice.
fn resolve_page_cover(
    cover: &str,
    path_resolver: &PathResolver,
) -> (String, moss_core::media::MediaAttrs) {
    let (path_part, attrs_str) = moss_core::media::split_pipe(cover);
    (
        path_resolver.resolve_url(path_part),
        moss_core::media::parse_media_attrs(attrs_str),
    )
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
/// The flag is true when the target is an edition home listing its own edition
/// only; the listing then carries the same filters as that home's.
fn resolve_children_source_folder_path(children_source: &str, all_docs: &[ParsedDocument]) -> (String, bool) {
    if let Some(home) = crate::build::render::lang_roots::named_edition_home(children_source, all_docs) {
        let defaults = crate::build::folder_embed::home_defaults(home);
        return (defaults.tree.to_string(), defaults.scoped);
    }
    let stem = crate::build::markdown::frontmatter_ref_to_stem(children_source);
    let path = all_docs
        .iter()
        .find(|d| {
            let d_stem = d.url_path.trim_end_matches("/index.html").trim_end_matches('/');
            d_stem.eq_ignore_ascii_case(&stem) && d.kind == PageKind::Folder
        })
        .map(|d| d.url_path.trim_end_matches("index.html").trim_end_matches('/').to_string())
        .unwrap_or_else(|| stem.to_lowercase());
    (path, false)
}

/// Insert the place-map locator into `body` at `placement` (a no-op for
/// [`LocatorPlacement::Masthead`], which the masthead functions draw instead).
/// Shared by the homepage branch and the ordinary non-homepage branch below.
fn splice_body_locator(
    body: &mut crate::build::markdown::body_plan::BodyPlan,
    doc: &ParsedDocument,
    layout: &LayoutConfig,
    placement: LocatorPlacement,
) {
    if matches!(placement, LocatorPlacement::Masthead | LocatorPlacement::Facts) {
        return;
    }
    if let Some(locator) = credits::render_place_locator(doc, layout) {
        body.insert_locator(locator, placement);
    }
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

/// A home page's name followed by its `tagline:` when it has one, for the tab
/// title and the share tags. Everything else that names the site (nav,
/// `og:site_name`, feeds, structured data) keeps the bare name.
pub(crate) fn with_tagline(name: &str, doc: Option<&ParsedDocument>) -> String {
    let tagline = doc
        .and_then(|d| d.raw_frontmatter.get("tagline"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty());
    match tagline {
        Some(t) => format!("{name} - {t}"),
        None => name.to_string(),
    }
}

/// The one line the build says when a real folder index at a place
/// namespace root composes its own intro with the term map below it,
/// naming the site-relative source file that supplied the intro. Pulled out
/// of the `log::info!` call site so the message is unit-tested directly —
/// the same reasoning `cli_output::log_line_for`'s own doc gives for not
/// installing a process-global logger in a test.
fn namespace_root_map_notice(source_path: &str) -> String {
    format!("{source_path}: supplies this page's intro; the place map composes below it")
}

/// Whether `doc` is a places-explorer root, for an `Option<&ParsedDocument>`
/// (the nav breadcrumb's own shape, built before the match arm below ever
/// unwraps `doc`) rather than [`PlaceMapRenderContext::is_explorer_root_for_doc`]'s
/// bare `&ParsedDocument` — a thin `Option` adapter so the breadcrumb and
/// that match arm's own heading/map placement still read the identical
/// answer, not two separately inlined copies of it.
fn is_explorer_root_doc(doc: Option<&ParsedDocument>, layout_config: &LayoutConfig) -> bool {
    doc.is_some_and(|d| {
        layout_config.place_maps.as_ref().is_some_and(|maps| maps.is_explorer_root_for_doc(d))
    })
}

#[cfg(test)]
fn generate_html_inner<'d>(
    doc: Option<&ParsedDocument>,
    all_docs: &'d [ParsedDocument],
    project: &ProjectStructure,
    layout_config: &LayoutConfig,
    is_homepage: bool,
    head_alternates: Option<&str>,
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
    og_outputs: Option<&mut crate::build::page::og_card::OgSink<'_>>,
    source_root: &std::path::Path,
    shared: &BuildShared<'d>,
) -> Result<String, String> {
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
        let pr = pr.with_js_version(shared.scripts.hash("theme"));
        pr.with_favicon_filename(favicon_filename)
            // This entry point takes one flag for the whole SVG-style set.
            .with_favicon_raster_sizes(if favicon_has_raster_pngs {
                crate::build::site_meta::favicon::SIZES.to_vec()
            } else {
                Vec::new()
            })
            .with_dir_overrides(dir_overrides.clone())
    };

    let context = RenderCtx {
        chrome: PageChromeContext {
            documents: all_docs, project, layout: layout_config, site_lang,
            assets: ChromeAssets { paths: path_resolver, scripts: &shared.scripts, has_user_css, has_user_js, page_owned: &Default::default(), head_alternates, theme_modules: &shared.theme_modules },
            show_rss_in_footer, emit_source_lines, has_sidebar_layout,
            home_breadcrumb: crate::build::components::nav::home_breadcrumb_setting(all_docs),
            media_lookup: &shared.media_lookup,
        },
        content_graph, dir_overrides, site_url, output_dir, source_root, shared,
    };
    render_page(&context, doc, is_homepage, og_outputs)
}

/// Build inputs shared by authored pages and the generated-folder shell.
pub(super) struct RenderCtx<'a, 'd> {
    pub chrome: PageChromeContext<'a, 'd>,
    pub content_graph: Option<&'a moss_core::content_graph::ContentGraph>,
    pub dir_overrides: &'a std::collections::HashMap<String, String>,
    pub site_url: &'a crate::build::site_url::SiteUrl,
    pub output_dir: Option<&'a std::path::Path>,
    pub source_root: &'a std::path::Path,
    pub shared: &'a BuildShared<'d>,
}

/// What a page is, decided once before any of its body is composed: the
/// branches that follow read these and never re-derive or change them.
struct PageFacts<'a> {
    ui_lang: Language,
    page_lang_tag: String,
    site_title: String,
    typesetting: Option<&'a str>,
    vertical_typesetting: bool,
    shell_type: ShellType,
    is_article_page: bool,
    is_explorer_root: bool,
    /// Where the place-map locator goes, for every kind of page. Vertical
    /// pages put it after the opening text; a page with a facts column has it
    /// head that column; horizontal articles float it before the text; the
    /// rest keep it in the masthead.
    locator_placement: LocatorPlacement,
    /// The declared fields the page draws, less the eyebrow, which goes into
    /// the body above the title.
    fields: Placements,
    eyebrow: String,
    /// A real public page (not draft/slot_only), so it advertises a share card
    /// and is indexed; `listed: false` pages keep their card. One flag for the
    /// og/twitter/cover/auto-card/JSON-LD gates so they cannot drift apart.
    is_card_eligible: bool,
    /// The page's children are listed in the right rail, not below its body.
    /// Read from the alias provenance flag (not `sidebar.is_some()`) so that a
    /// `sidebar:` the alias yielded to a `children:` conflict still lists
    /// its body children.
    children_in_sidebar: bool,
    /// The folder the right-rail list is drawn from: the deprecated `sidebar:`
    /// field when the alias is active, else `children_source` under
    /// `children_in: sidebar`.
    sidebar_source: Option<&'a str>,
    /// The share-card description: page description, hero overlay, body, then
    /// the same three rungs of the page-language homepage. The homepage reads
    /// only its own rungs.
    share_card_description: Option<String>,
    /// Absolute root when deployed, "/" otherwise; shared by the website
    /// og tags and the WebSite JSON-LD node.
    homepage_meta_url: String,
}

impl<'a> PageFacts<'a> {
    fn new(context: &'a RenderCtx<'_, '_>, doc: Option<&'a ParsedDocument>, is_homepage: bool) -> Self {
        let all_docs = context.chrome.documents;
        let layout_config = context.chrome.layout;
        let site_lang = context.chrome.site_lang;
        let typesetting = super::config::effective_typesetting(
            doc.and_then(|d| d.typesetting.as_deref()), layout_config.typesetting.as_deref(),
        );
        let vertical_typesetting = typesetting == Some("vertical");
        let shell_type = ShellRegistry::select_shell_type(doc, is_homepage, context.chrome.project.has_content_folders);
        let is_article_page = shell_type == ShellType::Article;
        let ui_lang = doc.map(|d| d.lang).unwrap_or(site_lang);

        let mut fields = doc.map_or_else(Default::default, |d| {
            crate::build::components::field_places::draw_for_page(d, all_docs, context.content_graph, typesetting)
        });
        let eyebrow = std::mem::take(&mut fields.eyebrow);
        let locator_placement = if vertical_typesetting {
            LocatorPlacement::AfterText
        } else if !fields.facts.is_empty() {
            LocatorPlacement::Facts
        } else if is_article_page {
            LocatorPlacement::BeforeText
        } else {
            LocatorPlacement::Masthead
        };
        if locator_placement == LocatorPlacement::Facts {
            if let Some(locator) = doc.and_then(|d| credits::render_place_locator(d, layout_config)) {
                fields.head_facts_with(&locator);
            }
        }

        let children_in_sidebar = doc.is_some_and(|d| {
            d.from_sidebar_alias.unwrap_or(false) || d.children_in.as_deref() == Some("sidebar")
        });
        let sidebar_source = doc.and_then(|d| {
            if d.from_sidebar_alias.unwrap_or(false) {
                d.sidebar.as_deref()
            } else if d.children_in.as_deref() == Some("sidebar") {
                d.children_source.as_deref()
            } else {
                None
            }
        });

        // Per-language: the page's-language homepage when there is one, else the
        // site-default homepage, so zh-hans/ pages do not surface the English
        // homepage's description.
        let cascade_homepage = if doc.is_some() && !is_homepage {
            find_homepage_doc(all_docs, ui_lang, site_lang)
        } else {
            None
        };
        let share_card_description = doc.and_then(|d| {
            let inputs = crate::build::page::meta::DescriptionChainInputs {
                page_description: d.description.as_deref(),
                page_hero_overlay_text: d.hero_overlay_text.as_deref(),
                page_content: &d.content,
                homepage_description: cascade_homepage.and_then(|h| h.description.as_deref()),
                homepage_hero_overlay_text: cascade_homepage.and_then(|h| h.hero_overlay_text.as_deref()),
                homepage_content: cascade_homepage.map(|h| h.content.as_str()),
                math: layout_config.assets.math,
            };
            crate::build::page::meta::resolve_page_description_with_fallbacks(&inputs)
        });
        let homepage_meta_url = if context.site_url.is_deployed() {
            context.site_url.as_str().to_string()
        } else {
            "/".to_string()
        };

        PageFacts {
            ui_lang,
            page_lang_tag: doc.and_then(|d| d.lang_tag.clone()).unwrap_or_else(|| layout_config.lang_tag.clone()),
            site_title: context.chrome.site_title(doc),
            typesetting,
            vertical_typesetting,
            shell_type,
            is_article_page,
            is_explorer_root: is_explorer_root_doc(doc, layout_config),
            locator_placement,
            fields,
            eyebrow,
            is_card_eligible: doc.map_or(false, |d| d.is_public_page()),
            children_in_sidebar,
            sidebar_source,
            share_card_description,
            homepage_meta_url,
        }
    }

    /// The page's body plan with the locator spliced in, and `eyebrow` (when
    /// not empty) above everything.
    fn body_plan(&self, context: &RenderCtx<'_, '_>, doc: &ParsedDocument, eyebrow: &str) -> crate::build::markdown::body_plan::BodyPlan {
        let mut plan = grid_cells::resolve_page_body(
            doc,
            context.chrome.documents,
            context.content_graph,
            context.dir_overrides,
            &context.chrome.project.root_path,
            &context.shared.media_lookup,
            self.typesetting,
            context.shared.now,
        );
        splice_body_locator(&mut plan, doc, context.chrome.layout, self.locator_placement);
        if !eyebrow.is_empty() {
            plan.prepend_html(eyebrow.to_string());
        }
        plan
    }

    /// The listing of `folder_path`'s children, as the listing a folder index
    /// shows, in the page's own language.
    fn children_listing(
        &self,
        context: &RenderCtx<'_, '_>,
        doc: &ParsedDocument,
        folder_path: &str,
        from_md_path: &str,
        names_scoped_home: bool,
    ) -> String {
        let marker = crate::build::folder_embed::synthesize_children_marker_for(
            doc,
            folder_path,
            from_md_path,
            names_scoped_home,
        );
        let listed = crate::build::folder_embed::ListedUrls::default();
        let resolved_html = crate::build::folder_embed::resolve_markers_with_place_maps(
            &marker,
            from_md_path,
            context.chrome.documents,
            context.chrome.project,
            context.dir_overrides,
            doc.lang,
            self.typesetting,
            Some(&context.shared.media_lookup),
            context.chrome.layout.assets.math,
            None,
            Some(&listed),
            context.shared.now,
        );
        crate::build::folder_embed::note_repeated_child_listing(doc, &listed);
        crate::build::emit::listing_style::wrap(
            doc,
            annotate_children_listing(resolved_html, context.chrome.emit_source_lines),
        )
    }

    /// A folder index, a page that won a term claim (`author_page:` /
    /// `tag_page:`, at any path) or a page hosting another folder's
    /// `children_source` ends in that listing; a term page and a place
    /// namespace root also get their map.
    fn host_listing(&self, context: &RenderCtx<'_, '_>, doc: &ParsedDocument, content: String, is_folder_index: bool) -> String {
        let term_listing = doc.term_listing.as_deref();
        let children_source_target = doc.children_source.as_deref()
            .map(|r| resolve_children_source_folder_path(r, context.chrome.documents));
        if !(is_folder_index || term_listing.is_some() || children_source_target.is_some()) {
            return content;
        }
        let names_scoped_home = children_source_target.as_ref().is_some_and(|t| t.1);
        let folder_path = term_listing.map(str::to_string).unwrap_or_else(|| {
            if is_folder_index {
                doc.url_path.trim_end_matches("/index.html").trim_end_matches("index.html").trim_end_matches('/').to_string()
            } else {
                children_source_target.map(|t| t.0).unwrap_or_default()
            }
        });
        // A `children_source` host needs its real source path, so the
        // listing's self-exclusion can match it.
        let from_md_path = if term_listing.is_some() || is_folder_index {
            if folder_path.is_empty() { "index.md".to_string() } else { format!("{}/index.md", folder_path) }
        } else {
            doc.source_path.clone().unwrap_or_else(|| format!("{}/index.md", folder_path))
        };

        // An explorer root hosts no listing: the map is the page.
        let show_folder_children = !self.children_in_sidebar && doc.children.unwrap_or(true) && !self.is_explorer_root;
        // Where the page's own body ends: a claimed term page's map and
        // listing are placed relative to it (see `insert_before_sources`).
        let body_end = content.len();
        let mut content = content;
        // A claimed sub-term and a real place-namespace-root folder index get
        // the same map, under the page-level `map: false` opt-out.
        let root_map_key = doc.is_place_namespace_root.then(|| folder_path.as_str());
        let map_key = term_listing.or(root_map_key).filter(|_| doc.shows_own_map(true));
        if let Some(map) = map_key.and_then(|key| {
            context.chrome.layout.place_maps.as_ref().and_then(|maps| {
                maps.render_term_map(key, context.chrome.documents.iter(), &doc.url_path, 1, doc.route, false)
            })
        }) {
            if doc.is_place_namespace_root && term_listing.is_none() {
                // `log::info!`, not a CLI-visible macro: this names a
                // composition, not a problem, so it never counts toward `--strict`.
                log::info!(
                    "{}",
                    namespace_root_map_notice(doc.source_path.as_deref().unwrap_or(doc.url_path.as_str()))
                );
            }
            if self.is_explorer_root {
                content = format!("{map}{content}");
            } else {
                content.push_str(&map);
            }
        }
        if show_folder_children {
            content.push_str(&self.children_listing(context, doc, &folder_path, &from_md_path, names_scoped_home));
        }
        if term_listing.is_some() && !self.is_explorer_root {
            let hosted = content.split_off(body_end);
            content = crate::build::terms::insert_before_sources(&content, &hosted, doc.lang);
        }
        content
    }
}

/// The shell variables every page shares; a page's calendar alternates sit in
/// the head beside the feed's.
fn authored_chrome(context: &RenderCtx<'_, '_>, doc: Option<&ParsedDocument>, is_homepage: bool, is_explorer_root: bool) -> ShellVars {
    let mut shell = context.chrome.shell_vars(doc, PageFamily::Authored, is_homepage, is_explorer_root, context.site_url);
    if let Some(d) = doc {
        let head = context.shared.calendar.head_link(d, d.lang);
        if !head.is_empty() {
            shell.head_alternates.get_or_insert_with(String::new).push_str(&head);
        }
    }
    shell
}

/// `html` with the page's colophon rows at its foot.
fn with_colophon(mut html: String, doc: &ParsedDocument, emit_source_lines: bool) -> String {
    credits::push_colophon(&mut html, &doc.colophon, emit_source_lines);
    html
}

/// The series navigation of a page whose folder is an ordered series, emitted
/// into the `{post_article}` token: a sibling of `<article>` after the
/// `after-article` slot, so the comment toggle that injects there sits directly
/// before it.
///
/// The chrome is on when the parent says `series: true`, or says nothing and
/// its sort looks ordered (a weight axis or an explicit order); the page can
/// opt out with `series: false`. The opt-out is whole-chain: the page renders
/// no nav and drops out of every sibling's chain (`is_sequence_step`), so its
/// neighbours become adjacent. Silencing only its own nav would leave the page
/// before it pointing "next" at a page that leads nowhere back.
fn series_nav_html<'d>(context: &RenderCtx<'_, 'd>, d: &ParsedDocument, typesetting: Option<&str>) -> Option<String> {
    let all_docs = context.chrome.documents;
    let path_resolver = &context.chrome.assets.paths;
    let parts: Vec<&str> = d.url_path.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    // "series/part-1/index.html" -> parent folder "series"
    let parent_folder = parts[..parts.len() - 2].join("/");
    let parent_index = format!("{}/index.html", parent_folder);
    let parent_doc = all_docs.iter().find(|pd| pd.url_path == parent_index)?;
    // The nav walks direct siblings only, so a flatten scope would be wrong.
    let resolved = parent_doc.resolve_for_direct_children();
    let page_opted_out = matches!(&d.series, Some(crate::build::types::SeriesField::Flag(false)));
    if !series_chrome_on(parent_doc) || page_opted_out {
        return None;
    }
    let parent_prefix = format!("{}/", parent_folder);
    let siblings = context.shared.sequences.chain(&parent_index, || sequence_siblings(
        all_docs, &parent_index, &parent_prefix, &resolved,
    ));
    // By url_path: every folder index's clean_stem is "index".
    let pos = siblings.iter().position(|pd| pd.url_path == d.url_path)?;
    // Labels are chrome: plain text.
    let neighbour = |i: usize| (siblings[i].label.clone(), path_resolver.resolve_url(&siblings[i].url_path));
    let prev = (pos > 0).then(|| neighbour(pos - 1));
    let next = (pos + 1 < siblings.len()).then(|| neighbour(pos + 1));
    let resolved_parent = path_resolver.resolve_url(&parent_index);
    let html = components::series_nav::render(
        &parent_doc.label,
        &resolved_parent,
        prev.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
        next.as_ref().map(|(t, u)| (t.as_str(), u.as_str())),
        // Where you are, over the same set the arrows walk. A lone page gets
        // no ordinal: "1 of 1" is noise, not orientation.
        if siblings.len() > 1 { Some((pos + 1, siblings.len())) } else { None },
        d.lang, typesetting,
    );
    (!html.is_empty()).then_some(html)
}

pub(super) fn render_page<'d>(
    context: &RenderCtx<'_, 'd>,
    doc: Option<&ParsedDocument>,
    is_homepage: bool,
    mut og_outputs: Option<&mut crate::build::page::og_card::OgSink<'_>>,
) -> Result<String, String> {
    let all_docs = context.chrome.documents;
    let project = context.chrome.project;
    let layout_config = context.chrome.layout;
    let site_lang = context.chrome.site_lang;
    let emit_source_lines = context.chrome.emit_source_lines;
    let dir_overrides = context.dir_overrides;
    let site_url = context.site_url;
    let output_dir = context.output_dir;
    let source_root = context.source_root;
    let media_lookup = &context.shared.media_lookup;
    let now = context.shared.now;
    let path_resolver = &context.chrome.assets.paths;
    let facts = PageFacts::new(context, doc, is_homepage);
    let ui_lang = facts.ui_lang;
    let vertical_typesetting = facts.vertical_typesetting;
    let page_lang_tag = &facts.page_lang_tag;
    let site_title = &facts.site_title;
    let resolved_typesetting = facts.typesetting;
    let shell_type = facts.shell_type;
    let is_article_page = facts.is_article_page;
    let locator_placement = facts.locator_placement;
    let is_card_eligible = facts.is_card_eligible;
    let chrome = authored_chrome(context, doc, is_homepage, facts.is_explorer_root);
    let processor = ShellProcessor::new();

    let (page_title, homepage_content) = match (doc, is_homepage) {
        (Some(doc), true) => {
            // The homepage injects no title h1 of its own: an authored leading
            // `# Foo` is the author's content and is kept verbatim.
            let content = facts.body_plan(context, doc, "").to_html();

            // The homepage has no title of moss's own for the byline to sit
            // under; render/credits.rs says where it goes instead, and why a
            // `layout: article` homepage is the article path's page, not this one.
            let content = if is_article_page {
                content
            } else {
                let content = credits::splice_page_masthead(content, doc, layout_config, emit_source_lines, locator_placement, &facts.fields);

                // One top-level heading is required: the header names the
                // site, so the heading is visually hidden; with no header it
                // is shown, unless an opening hero stands in for it.
                use moss_core::heading::{title_outcome, TitleOutcome};
                let outcome = title_outcome(&doc.content, true);
                if outcome == TitleOutcome::None {
                    content
                } else {
                    let headerless = layout_config.header == crate::build::components::nav::HeaderMode::None;
                    let h1 = crate::build::components::folder_title::render(&doc.label, emit_source_lines, !(headerless && outcome == TitleOutcome::Visible));
                    format!("{h1}\n{content}")
                }
            };

            // A home lists its own tree: "" for the root home, `en` for
            // `en/index.html`. `children_source` redirects it elsewhere.
            let own_tree = crate::build::folder_embed::home_defaults(doc).tree;
            let (target_folder_path, names_scoped_home) = doc.children_source.as_deref()
                .map(|r| resolve_children_source_folder_path(r, all_docs))
                .unwrap_or_else(|| (own_tree.to_string(), false));
            let from_md_path = if own_tree.is_empty() { "index.md".to_string() } else { format!("{own_tree}/index.md") };

            let content = if !facts.children_in_sidebar && doc.children.unwrap_or(true) {
                content + &facts.children_listing(context, doc, &target_folder_path, &from_md_path, names_scoped_home)
            } else {
                content
            };
            // Colophon at the very foot, after the children listing.
            let content = if is_article_page { content } else { with_colophon(content, doc, emit_source_lines) };

            // Homepage <title> tag is chrome; use the plain-text label.
            (doc.label.clone(), content)
        }
        (Some(doc), false) => {
            // The body's own H1 (if any) is the sole source of the visible
            // heading — moss never injects one. The locator is spliced in
            // before any cover-wrap below reads the body, so a claimed leaf's
            // `split_at_lede` sees it as part of the body it is splitting.
            let body = facts.body_plan(context, doc, &facts.eyebrow);
            let content = {
                let mut html = body.to_html();
                if crate::build::components::field_places::title_block_follows_picture(doc) {
                    crate::build::components::field_places::mark_body_start(&mut html);
                }
                html
            };

            // A folder index is any non-root page whose source was
            // index.md/readme.md/_index.md/main.md (every page's URL ends in
            // /index.html, so the URL alone cannot say).
            let is_folder_index =
                doc.kind == PageKind::Folder && doc.url_path.ends_with("/index.html") && doc.url_path != "index.html";

            // A home-override page (`home: true` on a non-root file) and a
            // language-root folder page (`zh-hans/index.md`) are language-specific
            // homepages, not folder listings: like the root home they take no
            // folder-title H1. `is_home_override` only covers the first; the
            // depth-1 slash count is LOAD-BEARING for the second, since
            // `zh-hans/news/index.html` is a genuine section and keeps its H1.
            let is_lang_tree_root_home = is_folder_index
                && doc
                    .source_path
                    .as_deref()
                    .and_then(|p| moss_core::home::lang_tree_prefix(p))
                    .is_some()
                && doc.url_path.chars().filter(|&c| c == '/').count() == 1;
            let is_home_override = is_folder_index && (doc.is_home_override || is_lang_tree_root_home);

            // A page that won a term claim hosts that term's listing below its
            // own body, but must draw its cover and title like any other page,
            // so the cover branch below reads it too.
            let term_listing = doc.term_listing.as_deref();
            let is_explorer_root = facts.is_explorer_root;

            // A folder index wraps its content in the book-open layout with the
            // cover on the left; title and byline go into the cover body so
            // they appear beside the cover, not above it.
            let content = if is_folder_index && !is_home_override {
                // Split pipe-encoded cover to separate path from display attrs.
                let (resolved_cover, cover_attrs) = match doc.cover.as_deref() {
                    Some(c) => {
                        let (url, attrs) = resolve_page_cover(c, path_resolver);
                        (Some(url), attrs)
                    }
                    None => (None, moss_core::media::MediaAttrs::default()),
                };
                // The visible folder-title h1 is the index cascade `title:`, else
                // the folder name, never body content: an author's body `# Custom`
                // is kept verbatim and renders below it.
                let h1_text = &doc.title;
                // `layout: article` reads as a plain article: no auto-inserted
                // cover component, even when `cover:` is set.
                let is_article_layout = doc.layout.as_deref() == Some("article");
                // `byline:` under the title; skipped when `is_article_page`,
                // because a `layout: article` folder runs through both
                // assemblies and the article path below emits both.
                let folder_byline = (!is_article_page)
                    .then(|| {
                        let masthead = credits::render_page_masthead(doc, layout_config, emit_source_lines, locator_placement);
                        format!("{}{masthead}", facts.fields.subtitle)
                    })
                    .unwrap_or_default();
                if resolved_cover.is_some() && !is_article_layout && !is_explorer_root {
                    // The cover thumbnail takes about a third of the content
                    // width, so only the lede belongs in the narrow column
                    // beside it: a `:::grid` typeset there is squeezed, and a
                    // long-form body would be read in a ~20-character measure.
                    // `BodyPlan::lede_segments` says where the lede ends;
                    // everything past it re-appends as a plain sibling after the
                    // cover row closes, where the generated children listing sits.
                    let (lead, trailer) = body.split_at_lede();
                    let cover_type = crate::build::media::cover::detect_cover_type(
                        resolved_cover.as_deref().unwrap_or(""),
                        doc.cover_type.as_deref(),
                    );
                    // Byline between the folder title and the body, in-column.
                    let cover_row_html = components::folder_cover::render(
                        resolved_cover.as_deref(),
                        Some(h1_text),
                        h1_text,
                        &format!("{}{}", folder_byline, lead),
                        cover_type,
                        &cover_attrs,
                        Some(media_lookup),
                        emit_source_lines,
                    );
                    format!("{}{}", cover_row_html, trailer)
                } else {
                    // No cover: prepend the folder-title h1 unless the body or an
                    // opening hero already supplies the heading. An explorer root
                    // gets a hidden one instead (no byline), skipping that dedup,
                    // which guards a second visible title.
                    let heading = if is_explorer_root {
                        crate::build::components::folder_title::render(h1_text, emit_source_lines, true)
                    } else {
                        format!(
                            "{}{}",
                            no_cover_folder_heading(
                                doc,
                                h1_text,
                                emit_source_lines,
                            ),
                            folder_byline,
                        )
                    };
                    if heading.is_empty() {
                        content
                    } else {
                        format!("{}\n{}", heading, content)
                    }
                }
            } else {
                // A claimed page that is not itself a folder index (a leaf note
                // that won `author_page:`/`tag_page:`) draws its cover through
                // the same book-open layout: cover on the left, lede beside it,
                // the rest released to full width. `folder_cover::render` takes
                // an empty label because this shell's own visible `<h1>` is
                // already the head of `lead`, so the page keeps exactly one.
                let content = match (term_listing, doc.cover.as_deref()) {
                    (Some(_), Some(c)) => {
                        let (resolved, cover_attrs) = resolve_page_cover(c, path_resolver);
                        let cover_type = crate::build::media::cover::detect_cover_type(&resolved, doc.cover_type.as_deref());
                        let (lead, trailer) = body.split_at_lede();
                        // A body with no heading of its own (a hero with no
                        // `# H1`, or an empty `title:`) gets none from the
                        // pipeline, so the column supplies the page's title,
                        // visually hidden, to keep one `<h1>` on the page.
                        let lead = if lead.contains("<h1") || trailer.contains("<h1") {
                            lead
                        } else {
                            format!(
                                "{}{}",
                                components::folder_title::render(&doc.title, emit_source_lines, true),
                                lead,
                            )
                        };
                        format!(
                            "{}{}",
                            components::folder_cover::render(
                                Some(&resolved),
                                None,
                                &doc.title,
                                &lead,
                                cover_type,
                                &cover_attrs,
                                Some(media_lookup),
                                emit_source_lines,
                            ),
                            trailer,
                        )
                    }
                    _ => content,
                };
                if is_article_page {
                    content
                } else {
                    // No title block of moss's own, so the homepage rule
                    // applies. After any cover-wrap above: `splice_page_masthead`
                    // recognizes that wrapper's empty title placeholder and
                    // lands the byline beside the real title inside it.
                    credits::splice_page_masthead(content, doc, layout_config, emit_source_lines, locator_placement, &facts.fields)
                }
            };

            let content = facts.host_listing(context, doc, content, is_folder_index);
            // Colophon at the very foot, after the listing, which is part of
            // the page; the same gate as the byline above.
            let content = if is_article_page { content } else { with_colophon(content, doc, emit_source_lines) };

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
    let home_title = with_tagline(&site_title, doc);
    let title_for_tab = if is_homepage {
        with_tagline(&tab_title(&page_title, &site_title, true), doc)
    } else {
        tab_title(&page_title, &site_title, false)
    };

    let post_article = doc
        .filter(|_| is_article_page)
        .and_then(|d| series_nav_html(context, d, resolved_typesetting))
        .unwrap_or_default();

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
    let sidebar_source = facts.sidebar_source;
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
                        && (d.date.is_some() || d.event.is_some())
                })
                // Sidebar always shows direct children only, regardless of children_depth.
                // Nested subfolder articles are excluded from sidebar listings.
                .filter(|d| is_direct_child(&d.url_path, &target_prefix))
                .map(|d| {
                    let (date_display, date_raw, _explicit) = components::extract_date_from_doc(d, &project.root_path);
                    components::ArticleListItemProps {
                        credits: None,
                        subtitle: None,
                        date_display,
                        date_raw,
                        url: crate::build::scan::article_map::to_pretty_url(&d.url_path),
                        // chrome; plain-text label
                        title: d.label.clone(),
                        url_path: d.url_path.clone(),
                        weight: d.weight,
                        place: d.place_names.clone(),
                        when_html: None,
                        when: None,
                        source_file: d.source_path.clone(),
                        lang: None,
                    }
                })
                .collect();

            // Always newest-first: a "Latest" sidebar has no folder of its own
            // whose `sort: date-asc` could apply.
            dated_articles.sort_by(|a, b| moss_core::sort::cmp_date_axis(&a.date_sort_key(), &b.date_sort_key(), false));

            // Resolve effective limit and More-link rule:
            //   - Alias path (`doc.sidebar` is set): legacy default 3 on cross-ref;
            //     explicit `children_limit` overrides; More always shown on
            //     cross-ref to preserve today's behavior.
            //   - Direct path (only `children_in: sidebar`, no `sidebar:` field):
            //     respect `children_limit` if set, no cap otherwise; More shown
            //     only when truncation actually happened.
            //
            // The `from_alias` branch exists to keep today's "More always on cross-ref"
            // semantics for sites still using `sidebar:`. When the alias is removed,
            // every sidebar feed uses the truncation-only rule — same as
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
    // An event page shows its event facts here instead of the posted date.
    // Its "Add to calendar" link goes in this meta line when the page has one,
    // and closes the body otherwise (a Page-shell event has no meta line). This
    // is the one place that decides which spot gets the link.
    let event_calendar_href = doc.and_then(|d| context.shared.calendar.event_href(d));
    let event_meta_html = doc
        .filter(|_| is_article_page)
        .and_then(|d| components::event_meta::render(d, d.lang, emit_source_lines, event_calendar_href.as_deref(), components::event_meta::UpcomingLabel::Hidden, now));
    let calendar_at_body_end = if event_meta_html.is_some() { None } else { event_calendar_href };
    let article_date_line_html: Option<String> = if is_article_page
        && doc.is_some()
        && (doc.unwrap().date.is_some() || event_meta_html.is_some())
    {
        let d = doc.unwrap();
        let doc_lang = d.lang;
        // The size control's "Aa" would hang off the end of the event facts.
        let has_size_control = event_meta_html.is_none() && d.reader_controls != Some(false);
        let when_html = event_meta_html.unwrap_or_else(|| {
            let formatted = components::format_display_date(d.date.as_ref().unwrap(), doc_lang, resolved_typesetting);
            let date_fm_attr = if emit_source_lines { r#" data-source-fm="date""# } else { "" };
            format!(r#"<span class="date"{}>{}</span>"#, date_fm_attr, formatted)
        });
        let size_control_html = if has_size_control {
            format!(
                r#"<div class="font-anchor"><button class="font-trigger size-std" aria-label="{}" aria-expanded="false" type="button"></button><div class="font-pill" id="fontPill"><button data-scale="small" aria-label="{}"></button><button data-scale="" class="active" aria-label="{}"></button><button data-scale="large" aria-label="{}"></button><button data-scale="xlarge" aria-label="{}"></button></div></div>"#,
                crate::i18n::t(doc_lang, "reading_preferences"),
                crate::i18n::t(doc_lang, "text_small"),
                crate::i18n::t(doc_lang, "text_standard"),
                crate::i18n::t(doc_lang, "text_large"),
                crate::i18n::t(doc_lang, "text_xlarge"),
            )
        } else {
            String::new()
        };
        Some(format!(r#"<div class="date-line">{when_html}{size_control_html}</div>"#))
    } else {
        None
    };

    // Splice the date-line AND the `after-title` slot marker into the content
    // so both land after the title block (H1 + optional blockquote deck). The
    // enhance hook injects the review colophon / book block into the marker,
    // so emitting it directly below the date row makes that block the first
    // thing after the title and date. The marker is emitted for every article
    // page, dated or not, so dateless reviews keep their colophon; an unfilled
    // marker is stripped by inject_slots.
    let homepage_content = if is_article_page {
        let mut after_title_block = facts.fields.subtitle.clone();
        after_title_block.push_str(&article_date_line_html.unwrap_or_default());
        // Byline rows (frontmatter `byline:`) sit between the date row and the
        // after-title slot: they belong to the masthead but not to
        // `.date-line`, a single flex row owning the reading-size control that
        // is emitted only when the page has a date. Byline and place line only,
        // not `render_page_masthead`: on an article the locator is already in
        // the body, before its first text block.
        if let Some(d) = doc {
            if let Some(masthead) =
                credits::render_byline_html(&d.byline, emit_source_lines, d.place_line.as_deref())
            {
                after_title_block.push_str(&masthead);
            }
        }
        after_title_block.push_str("<!-- slot:after-title -->");
        after_title_block.push_str(&facts.fields.after_masthead());
        let spliced = splice_after_title_block(&homepage_content, &after_title_block);
        // Colophon rows close the article body, after the prose and any
        // footnote section, inside `<article>`; a page's syndication line
        // follows them.
        match doc {
            Some(d) => {
                let mut html = with_colophon(spliced, d, emit_source_lines);
                if d.kind != moss_core::PageKind::Folder {
                    if let Some(line) = crate::build::features::syndication::render_syndication_line(&d.syndicated_urls(), d.lang) {
                        html.push_str(&line);
                    }
                }
                html
            }
            None => spliced,
        }
    } else {
        homepage_content
    };
    let homepage_content = {
        let mut finished = homepage_content;
        // The `breadcrumb: label` link is chrome around the finished title
        // block, so it goes on last: the splices above see the title at the
        // head of the content and need not step past anything.
        if let Some(d) = doc {
            if let Some(parent) = crate::build::components::nav::parent_label_segment(d, all_docs, context.chrome.home_breadcrumb) {
                crate::build::markdown::html_post::prepend_parent_label(&mut finished, &parent);
            }
            crate::build::components::field_places::warn_title_block_unplaced(d, &finished);
        }
        if doc.is_some_and(crate::build::components::field_places::title_block_after_picture) {
            finished = crate::build::components::field_places::after_leading_picture(&finished);
        } else if doc.is_some_and(crate::build::components::field_places::title_block_beside_picture) {
            finished = crate::build::components::field_places::beside_leading_picture(&finished);
        }
        finished = crate::build::page::default_width::apply(finished, doc.and_then(|d| d.content_width.as_deref()).or(layout_config.content_width.as_deref()));
        // Calendar links that close the body: a folder's download and subscribe
        // links, and an event's "Add to calendar" when no meta line carries it.
        // A page with its listing off shows no events here; its embedded
        // listings carry their own folders' links (`folder_embed`).
        if let Some(d) = doc {
            let cards = crate::build::components::field_places::EndCards {
                graph: context.content_graph,
                root_path: &project.root_path,
                dir_overrides,
                typesetting: resolved_typesetting,
                now,
                media_lookup,
            };
            finished.push_str(&crate::build::components::field_places::end_for_page(d, all_docs, &cards));
            if let Some(href) = calendar_at_body_end.as_deref() {
                finished.push_str(&context.shared.calendar.event_link_html(href, d.lang));
            }
            if d.prints_folder_calendar_links() {
                finished.push_str(&context.shared.calendar.folder_links_html(d, site_url, d.lang));
            }
        }
        finished
    };
    let share_card_description = facts.share_card_description.clone();
    let share_card_description_str: &str = share_card_description.as_deref().unwrap_or("");
    let homepage_meta_url = &facts.homepage_meta_url;


    // Build template variables
    let vars = ShellVars {
        title: title_for_tab,
        homepage_content: homepage_content.clone(),
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
        page_wrapper_class,
        // Placed outside <main> so a hero spans the full width, on any page.
        hero_section: doc.and_then(|d| d.hero_html.clone()),
        // The cover and the QR the page itself has, published to the reader
        // runtime on `<article class="container">` so share-card.ts finds
        // neither by guessing at markup. Absent attribute = the card draws
        // nothing there, and each rule lives with the code that decides it.
        share_cover_attr: doc
            .filter(|_| layout_config.assets.selection_tools)
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
            Some(d) => qr::share_qr_attr(&d.url_path, d.is_public_page(), site_url, layout_config.assets.selection_tools),
            None => qr::share_qr_attr("index.html", true, site_url, layout_config.assets.selection_tools),
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
            let url = crate::build::page::meta::page_meta_url(d, site_url)?;
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
            // `is_homepage` is true for EVERY locale's own home, but
            // `homepage_meta_url` (shared with the WebSite JSON-LD node
            // below, which describes the SITE and must always point at its
            // true root) is the bare site root regardless of which
            // language's home is rendering. Reusing it for og:url too was
            // only ever correct for that one bare-root page — a language
            // root (`zh-hans/index.html`) needs its OWN absolute URL here,
            // the same derivation the non-homepage branch already does.
            let (og_title, og_url): (&str, String) = if is_homepage && d.url_path == "index.html" {
                (home_title.as_str(), homepage_meta_url.clone())
            } else {
                let url = crate::build::page::meta::page_meta_url(d, site_url)?;
                let title = if is_homepage { home_title.as_str() } else { d.label.as_str() };
                (title, url)
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
            let card_title: &str = if is_homepage { home_title.as_str() } else { d.label.as_str() };
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
        schema_json_ld: crate::build::page::meta::page_json_ld(
            doc, is_article_page, is_homepage, share_card_description_str, &site_title,
            resolved_cover_for_og.as_ref(), site_url, &homepage_meta_url, &page_lang_tag,
        )?,
        embed_head_assets,
        lightbox: if doc.is_some_and(|d| d.features.viewer) {
            format!("{}\n    ", crate::build::media_collection::lightbox_html(chrome.ui_lang))
        } else {
            String::new()
        },
        post_article,
        ..chrome
    };

    let page = processor.process(shell_type, vars);
    if page.contains(moss_core::resolve::embed_renderer::folder_list::MARKER_FOLDER_LIST) {
        let name = doc.map_or("a generated page", |d| d.source_path.as_deref().unwrap_or(&d.url_path));
        crate::build::cli_output::log_warn_problem!("[{name}] a folder listing was left unexpanded in the built page");
    }
    Ok(page)
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
///    still something you read in order. Kind comes from the kind-aware
///    `SortableDoc::is_folder_index`, never the URL — under pretty URLs *every*
///    article also ends with `<stem>/index.html`.
/// 2. **`series: false` on a leaf page.** It stays in the listing but steps
///    out of the sequence, so no sibling may point at it as prev or next. This
///    is the whole-chain half of the opt-out; the page's own nav is separately
///    suppressed by `nav_enabled` at the call site.
///
/// Every page that sets neither answers exactly as `is_listable` does.
pub(crate) fn is_sequence_step(pd: &ParsedDocument) -> bool {
    use moss_core::sort::SortableDoc;
    pd.is_listable()
        && (!pd.is_folder_index() || ShellRegistry::is_article_layout(pd))
        && !matches!(&pd.series, Some(crate::build::types::SeriesField::Flag(false)))
}

/// Whether `parent_doc`'s series prev/next chrome is on for its children —
/// the chrome-trigger precedence: an explicit `series:` on the parent wins
/// (`Flag(true)`/`Flag(false)`, or the legacy `Ordered(_)` list form, which
/// always means on); otherwise the resolved sort's own `series_default`
/// (weight axis, or an explicit list order) decides. Shared by the two
/// places that need this one precedence decision on a parent folder: this
/// module's own series-nav block (a leaf's own opt-out, `page_opted_out`
/// below, is a separate, per-leaf check on top of it) and
/// `render::incremental::listing::is_series_member`, which needs the same
/// answer to know whether an edit anywhere in the chain can move any
/// sibling's rendered chrome.
pub(crate) fn series_chrome_on(parent_doc: &ParsedDocument) -> bool {
    match &parent_doc.series {
        Some(crate::build::types::SeriesField::Flag(b)) => *b,
        Some(crate::build::types::SeriesField::Ordered(_)) => true,
        None => parent_doc.resolve_for_direct_children().series_default,
    }
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
    #[cfg(test)]
    tests::SEQUENCES_ORDERED.with(|n| n.set(n.get() + 1));
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
