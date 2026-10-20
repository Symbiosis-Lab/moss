//! Common shell facts for authored pages and generated folder indexes.

use crate::build::assets::paths::{compute_content_hash, PathResolver};
use crate::build::assets::theme_modules::ThemeModules;
use crate::build::cloud_ledger::InputEvidence;
use crate::build::components::nav::{NavigationBuilder, compute_breadcrumb_segments};
use crate::build::emit::scripts::ScriptAssets;
use crate::build::page::layout::LayoutConfig;
use crate::build::page::meta::escape_html_attr;
use crate::build::page::shell::ShellVars;
use crate::build::served_path::served_address;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::types::content::ProjectStructure;
use std::collections::HashMap;
use std::path::Path;

use super::config::{resolve_logo_url, resolve_data_attr, resolve_comments_attr};

pub(super) enum PageFamily {
    Authored,
    GeneratedFolder,
}

/// The same asset snapshot every page links and the emitter writes.
pub(super) struct ChromeAssets<'a> {
    pub paths: PathResolver,
    pub scripts: &'a ScriptAssets,
    pub has_user_css: bool,
    pub has_user_js: bool,
    pub page_owned: &'a PageOwnedAssets,
    pub head_alternates: Option<&'a str>,
    pub theme_modules: &'a ThemeModules,
}

pub(super) struct PageChromeContext<'a, 'd> {
    pub documents: &'d [ParsedDocument],
    pub project: &'a ProjectStructure,
    pub layout: &'a LayoutConfig,
    pub site_lang: Language,
    pub assets: ChromeAssets<'a>,
    pub show_rss_in_footer: bool,
    pub emit_source_lines: bool,
    pub has_sidebar_layout: bool,
    /// The home page's `breadcrumb:` value, read once per build.
    pub home_breadcrumb: Option<moss_core::frontmatter_typed::BreadcrumbSetting>,
    /// The build's media lookup, for the logo's intrinsic size.
    pub media_lookup: &'a crate::build::media::dimensions::MediaDimensionLookup,
}

impl PageChromeContext<'_, '_> {
    pub fn site_title(&self, doc: Option<&ParsedDocument>) -> String {
        let fallback = if self.layout.site_name.is_empty() {
            self.project.homepage_file.as_ref()
                .and_then(|_| self.documents.iter().find(|d| d.url_path == "index.html"))
                .map(|d| d.title.clone())
                .filter(|t| !moss_core::home::is_index_stem(t))
                .unwrap_or_else(|| crate::i18n::t(self.site_lang, "site").to_string())
        } else {
            self.layout.site_name.clone()
        };
        doc.map_or(fallback.clone(), |d| {
            super::html::localized_site_title(self.documents, d.lang, self.site_lang, &fallback)
        })
    }

    /// Page-specific body and metadata are filled by the caller. The generated
    /// family keeps its root logo and CSS whitespace for output compatibility.
    pub fn shell_vars(
        &self,
        doc: Option<&ParsedDocument>,
        family: PageFamily,
        is_homepage: bool,
        force_breadcrumb: bool,
        site_url: &crate::build::site_url::SiteUrl,
    ) -> ShellVars {
        let generated = matches!(family, PageFamily::GeneratedFolder);
        let contents_open = doc.and_then(|d| d.raw_frontmatter.get("contents")).and_then(|v| v.as_str()) == Some("open");
        let site_title = self.site_title(doc);
        let ui_lang = doc.map(|d| d.lang).unwrap_or(self.site_lang);
        let page_lang_tag = doc.and_then(|d| d.lang_tag.clone())
            .unwrap_or_else(|| self.layout.lang_tag.clone());
        let logo_is_own_field = doc.is_some_and(|d| {
            d.logo.is_some() && (d.url_path == "index.html"
                || d.url_path == format!("{}/index.html", d.lang.code()))
        });
        // Generated folders historically scope the initial navigation in their
        // own language; authored pages scope it in the site language.
        let mut nav = NavigationBuilder::new(
            self.documents, &site_title, doc.map(|d| d.url_path.as_str()),
            if generated { ui_lang } else { self.site_lang }, self.project.has_content_folders,
        ).with_search(self.layout.assets.search).with_header_mode(self.layout.header).with_term_nav(self.layout.term_nav.clone(), doc.map_or("", |d| d.edition.as_str()))
            .with_asset_snapshot(self.media_lookup.asset_snapshot());
        if !generated {
            nav = nav.with_source_fm(self.emit_source_lines, self.emit_source_lines && logo_is_own_field);
        }
        let logo = if !generated && ui_lang != self.site_lang {
            let root = format!("{}/index.html", ui_lang.code());
            self.documents.iter().find(|d| d.url_path == root)
                .and_then(|d| d.logo.as_ref()).map(|l| resolve_logo_url(l))
                .or_else(|| self.layout.logo_path.clone())
        } else {
            self.layout.logo_path.clone()
        };
        if let Some(logo) = logo {
            nav = nav.with_logo(logo);
        }
        if let Some(d) = doc {
            if let Some(segments) = compute_breadcrumb_segments(
                d, self.documents, &site_title, self.project.has_content_folders, force_breadcrumb,
            ) {
                nav = nav.with_breadcrumb(segments, force_breadcrumb);
            }
            let roots = super::lang_roots::site_lang_roots(self.documents, self.site_lang);
            let multi = super::lang_roots::site_publishes_multiple_languages(self.documents, &roots, self.site_lang);
            let links = crate::i18n::site_languages::other_language_links(
                &page_lang_tag, &d.translations, &roots, multi,
            );
            if contents_open { nav = nav.with_island_only(&d.title); }
            nav = nav.with_translations(ui_lang, &page_lang_tag, links);
        }
        // Gated like the canonical link: a relative hreflang misleads
        // crawlers, so absent is safer than wrong.
        let hreflang_links = doc.filter(|_| site_url.is_deployed()).and_then(|d| {
            crate::build::page::meta::build_hreflang_link_tags(
                &page_lang_tag, &d.url_path, &d.translations, site_url.as_str(), &self.layout.lang_tag,
            )
        });
        let paths = &self.assets.paths;
        let page_source = doc.and_then(|d| d.source_path.as_deref());
        let (theme_js_tag, theme_importmap) = self.assets.theme_modules.shell_tags(paths, self.assets.has_user_js);
        let mut body_attrs = String::new();
        if self.emit_source_lines { body_attrs.push_str(" data-moss-preview"); }
        if is_homepage { body_attrs.push_str(r#" data-page="home""#); }
        // The page's published address, spelled as the sitemap and links spell
        // it. A page with no document is the synthetic homepage.
        let address = served_address(doc.map(|d| d.url_path.as_str()).unwrap_or("index.html"));
        body_attrs.push_str(&format!(r#" data-path="{}""#, escape_html_attr(&address)));
        if contents_open { body_attrs.push_str(r#" data-contents="open""#); }
        if let Some(section) = doc.and_then(|d| d.section.as_ref()) {
            if !section.segment.is_empty() {
                body_attrs.push_str(&format!(r#" data-section="{}""#, escape_html_attr(&section.segment)));
            }
            body_attrs.push_str(&format!(r#" style="{}""#, escape_html_attr(&crate::build::scan::section_text::section_style(section))));
        }
        let homepage = self.project.homepage_file.as_ref()
            .and_then(|_| self.documents.iter().find(|d| d.url_path == "index.html"));
        // Generated indexes have always read root analytics even when the
        // scanner has no homepage-file designation (for an empty site).
        let analytics_home = if generated {
            self.documents.iter().find(|d| d.url_path == "index.html")
        } else { homepage };
        ShellVars {
            title: String::new(),
            css_path: paths.css_path(), js_path: paths.js_path(),
            lazy_chunk_attrs: paths.lazy_chunk_attrs(
                self.assets.scripts.hash("share-card"),
                self.layout.assets.video_ladder.then(|| self.assets.scripts.hash("hls")),
            ),
            navigation: nav.generate_navigation(),
            nav_island: if self.layout.floating_nav || contents_open { nav.generate_nav_island() } else { String::new() },
            footer: Some(nav.generate_footer(self.show_rss_in_footer)),
            favicon: Some(paths.favicon_link()),
            apple_touch_icon: paths.apple_touch_icon_link(),
            head_alternates: self.assets.head_alternates.map(str::to_string),
            analytics: analytics_home.and_then(|d| d.analytics.as_ref()).map(|a| a.to_script_tag()),
            body_attrs,
            lang: page_lang_tag, ui_lang,
            user_css_link: Some(format!(
                "{}{}",
                self.assets.has_user_css.then(|| format!(
                    "{}<link rel=\"stylesheet\" href=\"{}\" layer=\"themes\">",
                    if generated { "\n    " } else { "" }, paths.user_css_path(),
                )).unwrap_or_default(),
                self.assets.page_owned.link(page_source, paths),
            )).filter(|tags| !tags.is_empty()),
            user_js_tag: Some(format!(
                "{}{}",
                theme_js_tag.unwrap_or_default(),
                self.assets.page_owned.script(page_source, paths),
            )).filter(|tags| !tags.is_empty()),
            theme_importmap,
            has_sidebar_layout: self.has_sidebar_layout,
            runtime_js_tags: self.assets.scripts.shell_tags(&self.layout.assets, paths),
            content_width_attr: resolve_data_attr(doc.and_then(|d| d.content_width.as_ref()), self.layout.content_width.as_ref(), "content-width", None),
            typesetting_attr: resolve_data_attr(doc.and_then(|d| d.typesetting.as_ref()), self.layout.typesetting.as_ref(), "typesetting", Some("horizontal")),
            comments_attr: resolve_comments_attr(doc.and_then(|d| d.comments), self.layout.comments),
            homepage_content: String::new(), latest_list: None, latest_sidebar: None,
            page_wrapper_class: String::new(), hero_section: None,
            share_cover_attr: String::new(), share_qr_attr: String::new(), main_class: String::new(),
            date: None, formatted_date: None, date_line: None, short_date: None, content: None,
            description: None, og_tags: None, twitter_tags: None, canonical_link: None,
            hreflang_links, schema_json_ld: None, embed_head_assets: String::new(),
            post_article: String::new(), lightbox: String::new(), robots_meta: None,
        }
    }
}

// A page's own script and stylesheet: `essay.js` and `essay.css` beside
// `essay.md` (same folder, same stem as the page's source file). The files stay
// ordinary site files, published at their own address; this only decides which
// page links them. It is resolved at render time rather than stored on
// `ParsedDocument` because a parse-cache replay would otherwise keep describing
// a sibling that has since appeared or gone.

/// A sibling file's source path and the hash of its bytes.
#[derive(Debug, Clone)]
struct Owned {
    source: String,
    hash: String,
}

#[derive(Debug, Clone, Default)]
struct Pair {
    css: Option<Owned>,
    js: Option<Owned>,
}

/// Every page-owned file of one build, keyed by the owning page's source path.
#[derive(Debug, Default)]
pub(super) struct PageOwnedAssets {
    by_page: HashMap<String, Pair>,
}

fn stem_key(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(stem, _)| stem)
}

impl PageOwnedAssets {
    pub fn collect(project: &ProjectStructure, root: &Path, evidence: &InputEvidence) -> Result<Self, String> {
        let page_stems: HashMap<&str, &str> = project
            .markdown_files
            .iter()
            .map(|f| (stem_key(&f.path), f.path.as_str()))
            .collect();
        let mut by_page: HashMap<String, Pair> = HashMap::new();
        for file in &project.other_files {
            let is_css = file.file_type == "css";
            if !is_css && file.file_type != "js" {
                continue;
            }
            let Some(page) = page_stems.get(stem_key(&file.path)) else { continue };
            let label = if is_css { "page stylesheet" } else { "page script" };
            let Some(text) = crate::build::cloud_readiness::read_optional_build_input(
                &root.join(&file.path), label, evidence,
            )? else { continue };
            let owned = Owned { source: file.path.clone(), hash: compute_content_hash(&text) };
            let pair = by_page.entry((*page).to_string()).or_default();
            if is_css { pair.css = Some(owned) } else { pair.js = Some(owned) }
        }
        Ok(Self { by_page })
    }

    fn href(owned: &Owned, paths: &PathResolver) -> String {
        escape_html_attr(&format!("{}?v={}", paths.resolve_url(&owned.source), owned.hash))
    }

    /// The `<link>` for the page's stylesheet, with its leading indent, or `""`.
    pub fn link(&self, page_source: Option<&str>, paths: &PathResolver) -> String {
        page_source.and_then(|p| self.by_page.get(p)).and_then(|pair| pair.css.as_ref()).map_or_else(
            String::new,
            |css| format!("\n    <link rel=\"stylesheet\" href=\"{}\" layer=\"themes\">", Self::href(css, paths)),
        )
    }

    /// The module `<script>` for the page's script, with its leading indent, or `""`.
    pub fn script(&self, page_source: Option<&str>, paths: &PathResolver) -> String {
        page_source.and_then(|p| self.by_page.get(p)).and_then(|pair| pair.js.as_ref()).map_or_else(
            String::new,
            |js| format!("\n    <script type=\"module\" src=\"{}\"></script>", Self::href(js, paths)),
        )
    }
}
