//! Common shell facts for authored pages and generated folder indexes.

use crate::build::assets::paths::PathResolver;
use crate::build::components::nav::{NavigationBuilder, compute_breadcrumb_segments};
use crate::build::emit::scripts::ScriptAssets;
use crate::build::page::layout::LayoutConfig;
use crate::build::page::shell::ShellVars;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::types::content::ProjectStructure;

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
    pub rss_link: Option<&'a str>,
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
    ) -> ShellVars {
        let generated = matches!(family, PageFamily::GeneratedFolder);
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
        ).with_search(self.layout.assets.search).with_header_mode(self.layout.header);
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
            nav = nav.with_translations(ui_lang, &page_lang_tag, links);
        }
        let paths = &self.assets.paths;
        let mut body_attrs = String::new();
        if self.emit_source_lines { body_attrs.push_str(" data-moss-preview"); }
        if is_homepage { body_attrs.push_str(r#" data-page="home""#); }
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
            nav_island: if self.layout.floating_nav { nav.generate_nav_island() } else { String::new() },
            footer: Some(nav.generate_footer(self.show_rss_in_footer)),
            favicon: Some(paths.favicon_link()),
            apple_touch_icon: paths.apple_touch_icon_link(),
            rss_link: self.assets.rss_link.map(str::to_string),
            analytics: analytics_home.and_then(|d| d.analytics.as_ref()).map(|a| a.to_script_tag()),
            body_attrs,
            lang: page_lang_tag, ui_lang,
            user_css_link: self.assets.has_user_css.then(|| format!(
                "{}<link rel=\"stylesheet\" href=\"{}\" layer=\"themes\">",
                if generated { "\n    " } else { "" }, paths.user_css_path(),
            )),
            user_js_tag: self.assets.has_user_js.then(|| paths.user_js_tag()),
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
            hreflang_links: None, schema_json_ld: None, embed_head_assets: String::new(),
            post_article: String::new(), robots_meta: None,
        }
    }
}
