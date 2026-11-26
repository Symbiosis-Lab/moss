//! Navigation generation module for moss static site generator
//!
//! Handles the creation of the main site navigation.

use crate::build::types::ParsedDocument;
use moss_core::page::nav::{nav_bar_pages, nav_items, sort_nav_items, BreadcrumbSegment, NavItem};

use crate::i18n::Language;
use crate::i18n::link::TranslationLink;

/// The masthead's `.nav-left` (breadcrumb trail / site name). A child module
/// so it can build from `NavigationBuilder`'s private state without widening
/// any of it.
mod breadcrumb;
/// The floating nav island. A child module for the same reason.
mod island;
/// The search / language / theme cluster, shared by the masthead and the
/// island of a site with no header.
mod icons;
/// `generate_footer`, and `strip_empty_footer`, which settles the footer it
/// emits once slots are resolved. A child module for the same reason as the
/// two above.
pub(crate) mod footer;
/// `[site].header` — see `HeaderMode`. Not a private-state module like the
/// two above (nothing here reads `NavigationBuilder`'s privates), just a
/// sibling kept close to its one reader.
mod header_mode;
pub use header_mode::{HeaderMode, HeaderScroll};

/// Builder for generating navigation components
pub struct NavigationBuilder<'a> {
    documents: &'a [ParsedDocument],
    site_title: &'a str,
    current_page_url: Option<&'a str>,
    lang: Language,
    breadcrumb_segments: Option<Vec<BreadcrumbSegment>>,
    /// What the nav island is for this page.
    island: island::IslandShape,
    /// Render the trail's own `is_current` segment instead of the ordinary
    /// skip (`breadcrumb.rs`'s `nav_left_html`) — an explorer root's only
    /// use (`render/html.rs`/`render/blocking.rs`, both via `is_explorer_root`).
    /// Every other page relies on its own visible `<h1>` to say "you are
    /// here" and would show its own title twice if this were ever on by
    /// default; an explorer root's `<h1>` is `.visually-hidden` (design
    /// decision 7), so the breadcrumb is the only thing left to say it.
    breadcrumb_current_visible: bool,
    /// Translations for the current page (for nav language toggle)
    translations: Vec<TranslationLink>,
    /// Language of the current page — the INTERFACE language, so it picks nav
    /// strings and filters the footer link list. Never the switcher's chip:
    /// see `current_lang_tag`.
    current_lang: Language,
    /// The current page's DECLARED tag, which is what the switcher's own chip
    /// says. On a `fr` site `current_lang` is `En` (moss ships no French
    /// interface), so reading it there labeled the page "EN" while every link
    /// beside it was already tag-labeled.
    current_lang_tag: String,
    /// Optional logo path (e.g., "/assets/logo.svg")
    logo_path: Option<String>,
    /// Snapshot the logo's intrinsic size is read from. `None` leaves the
    /// logo without `width`/`height`.
    assets: Option<&'a moss_core::asset_snapshot::AssetSnapshot>,
    /// Whether the site has content subfolders (organized mode vs flat mode)
    has_content_folders: bool,
    /// Whether this build has a live search index (`LayoutConfig::assets.search` —
    /// the fully resolved gate, NOT the raw `[site].search` bool). When true
    /// a `.nav-search-btn` is emitted into `.nav-icons`; the client runtime
    /// binds to it and lazy-loads `/_moss/pagefind/pagefind.js` on first use.
    has_search: bool,
    color_scheme: crate::build::page::color_scheme::ColorScheme,
    /// Editor preview (`emit_source_lines`): `data-source-fm` annotations.
    /// The trail names `breadcrumb:` (the page's own toggle; fm outranks the
    /// surrounding `<nav data-source-none>` in the bridge's resolver).
    /// `fm_logo` is set ONLY when the page IS the homepage declaring the
    /// `logo:` shown — anywhere else the field lives in a different file,
    /// and a chip absent from the open file cannot be revealed.
    fm_breadcrumb: bool,
    fm_logo: bool,
    /// `[site].header`, resolved. See `HeaderMode`.
    header_mode: HeaderMode,
    /// Term-kind roots that are nav entries, every edition's.
    term_nav: Vec<crate::build::terms::TermNavEntry>,
    /// The edition of the page this nav is for, which picks its entries.
    page_edition: &'a str,
}

impl<'a> NavigationBuilder<'a> {
    pub fn new(
        documents: &'a [ParsedDocument],
        site_title: &'a str,
        current_page_url: Option<&'a str>,
        lang: Language,
        has_content_folders: bool,
    ) -> Self {
        Self {
            documents,
            site_title,
            current_page_url,
            lang,
            breadcrumb_segments: None,
            island: island::IslandShape::default(),
            breadcrumb_current_visible: false,
            translations: Vec::new(),
            current_lang: lang,
            current_lang_tag: lang.as_bcp47_attr().to_string(),
            logo_path: None,
            assets: None,
            has_content_folders,
            has_search: false,
            color_scheme: Default::default(),
            fm_breadcrumb: false,
            fm_logo: false,
            header_mode: HeaderMode::Brand,
            term_nav: Vec::new(),
            page_edition: "",
        }
    }

    /// Add term-kind roots as nav entries after the content entries: those of
    /// `page_edition`, the edition of the page this nav is for.
    pub fn with_term_nav(mut self, term_nav: Vec<crate::build::terms::TermNavEntry>, page_edition: &'a str) -> Self {
        self.term_nav = term_nav;
        self.page_edition = page_edition;
        self
    }

    /// Editor-preview `data-source-fm` annotations — see the field docs.
    pub fn with_source_fm(mut self, breadcrumb: bool, logo: bool) -> Self {
        self.fm_breadcrumb = breadcrumb;
        self.fm_logo = logo;
        self
    }

    pub fn with_color_scheme(mut self, color_scheme: crate::build::page::color_scheme::ColorScheme) -> Self {
        self.color_scheme = color_scheme;
        self
    }

    /// Set `[site].header` — see `HeaderMode`.
    pub fn with_header_mode(mut self, header_mode: HeaderMode) -> Self {
        self.header_mode = header_mode;
        if header_mode == HeaderMode::None {
            self.island = island::IslandShape::Pinned { sections: true };
        }
        self
    }

    /// Enable the nav search button. Pass the resolved gate from
    /// `LayoutConfig::assets.search`, never the raw `[site].search` config
    /// value — see the field doc on
    /// [`SiteAssets::search`][crate::build::types::SiteAssets::search].
    pub fn with_search(mut self, has_search: bool) -> Self {
        self.has_search = has_search;
        self
    }

    /// Set breadcrumb segments for this navigation.
    /// When set, the site name in `.nav-left` is replaced with a breadcrumb trail.
    ///
    /// `force_current_visible` is the SAME `force` an explorer-root caller
    /// already passed to [`breadcrumb_segments`] to get `segments`
    /// in the first place — one switch, not two kept manually in sync. It
    /// used to be a separate chained `.with_breadcrumb_current_visible()`
    /// call every explorer-root call site had to remember to add right
    /// after this one; a caller that set one and not the other (compute the
    /// segments with `force: true` but forget the builder call, or vice
    /// versa) would have gotten a trail whose own "current page" segment
    /// disagreed with whether its heading was hidden. See the
    /// `breadcrumb_current_visible` field doc for what it does.
    pub fn with_breadcrumb(mut self, segments: Vec<BreadcrumbSegment>, force_current_visible: bool) -> Self {
        self.breadcrumb_segments = Some(segments);
        self.breadcrumb_current_visible = force_current_visible;
        self
    }

    /// Drop the sections button from a pinned island, for a page whose sections
    /// are too short to list.
    pub fn without_island_sections(mut self) -> Self {
        if let island::IslandShape::Pinned { sections } = &mut self.island {
            *sections = false;
        }
        self
    }

    /// Give a page with no trail an island whose only crumb is `title`.
    pub fn with_island_only(mut self, title: &str) -> Self {
        if self.island == island::IslandShape::Trail {
            self.island = island::IslandShape::TitleOnly(title.to_string());
        }
        self
    }

    /// Set translations for the current page (shown as language toggle in nav).
    pub fn with_translations(mut self, lang: Language, lang_tag: &str, translations: Vec<TranslationLink>) -> Self {
        self.current_lang = lang;
        self.current_lang_tag = lang_tag.to_string();
        self.translations = translations;
        self
    }

    /// Read the logo's intrinsic size from this snapshot.
    pub fn with_asset_snapshot(mut self, assets: &'a moss_core::asset_snapshot::AssetSnapshot) -> Self {
        self.assets = Some(assets);
        self
    }

    /// Set the site logo path (rendered before site name in nav).
    pub fn with_logo(mut self, logo_path: String) -> Self {
        self.logo_path = Some(logo_path);
        self
    }

    /// Whether `doc` is in this page's edition, for the nav bar and the footer
    /// link list alike. Keyed on `effective_lang`: a content-detected language
    /// without a language tree keeps the site language's items. The edition
    /// tells apart trees that share one interface language.
    pub(super) fn in_page_edition(&self, doc: &ParsedDocument) -> bool {
        doc.lang == self.effective_lang() && doc.edition == self.page_edition
    }

    fn effective_lang(&self) -> Language {
        moss_core::page::nav::chrome_lang(self.current_page_url, self.current_lang, self.lang)
    }

    fn home_path(&self) -> String {
        moss_core::page::nav::home_path(self.current_page_url, self.page_edition, self.current_lang, self.lang)
    }

    /// Logo + site name content (used in both plain and breadcrumb modes).
    ///
    /// Phase 2B (2026-05-25): the bare `<img class="site-logo">` emission
    /// is delegated to the unified image synthesizer
    /// (`ImageContext::SiteLogo`) so every `<img>` moss produces flows
    /// through one shape-defining seam. Byte shape is preserved exactly:
    /// `<img class="site-logo" src="..." alt="" aria-hidden="true">`.
    fn logo_html(&self) -> String {
        // Editor preview: name the `logo:` field — only when this page owns it
        // (see `fm_logo`).
        self.logo_img(self.fm_logo.then_some(r#"data-source-fm="logo""#))
    }

    fn logo_img(&self, extra_attrs: Option<&'static str>) -> String {
        // Without a snapshot the logo is synthesized with an empty one, so its
        // size is unknown and the bare shape comes out.
        let empty;
        let assets = match self.assets {
            Some(assets) => assets,
            None => {
                empty = moss_core::asset_snapshot::AssetSnapshot::new();
                &empty
            }
        };
        self.logo_path.as_ref().map(|path| {
            moss_core::render::image::synthesize_image_html(
                path,
                "",
                assets,
                moss_core::render::image::ImageContext::SiteLogo,
                &moss_core::render::image::ImageRenderOptions { extra_attrs, ..Default::default() },
            )
        }).unwrap_or_default()
    }

    /// Generates navigation HTML with site name on left and items on right.
    /// Only shows docs with `nav: Some(true)` (explicit opt-in).
    /// If nav items exist: renders hamburger + nav-links + nav-icons.
    /// If no nav items: just site name + nav-icons.
    pub fn generate_navigation(&self) -> String {
        // No header band under `[site].header = "none"`; the island carries
        // the controls (`nav/island.rs`).
        if self.header_mode == HeaderMode::None {
            return String::new();
        }
        let home_path = self.home_path();
        let logo_html = self.logo_html();
        let nav_mode = self.header_mode == HeaderMode::Nav;

        // `.nav-left` is the trail or the plain name (nav/breadcrumb.rs), left out
        // whole under `[site].header = "nav"`: an empty one would still claim row 1.
        let site_name = if nav_mode {
            String::new()
        } else {
            self.nav_left_html(&home_path, &logo_html)
        };

        // Adaptive auto-navigation model:
        // - Organized mode (has content folders): root-level non-index files auto-appear in nav
        // - Flat mode (no content folders): only keyword filenames (about, etc.) auto-appear
        // - nav: true/false always wins (explicit opt-in/out)
        let nav_documents = nav_bar_pages(self.documents, self.effective_lang(), self.page_edition, self.has_content_folders);
        let mut entries = nav_items(&nav_documents, self.current_page_url, nav_mode, self.current_lang);

        // Term kinds that asked for a place in the nav, the page's own
        // edition's. One with a `weight` is ordered among the entries by the
        // rule below; one without follows every page entry, in config order.
        let mut unweighted_terms: Vec<String> = Vec::new();
        for crate::build::terms::TermNavEntry { root: key, label, weight, .. } in
            self.term_nav.iter().filter(|entry| entry.edition == self.page_edition)
        {
            let is_current = self.current_page_url == Some(format!("{key}/index.html").as_str());
            let attrs = if is_current { r#" class="active" aria-current="page""# } else { "" };
            let item = format!(
                r#"<a href="/{}/"{attrs}>{}</a>"#, // allow:served-path-url-construct (nav href to a term kind's root page, not a framework asset)
                crate::build::features::html_escape(key),
                crate::build::features::html_escape(label),
            );
            match weight {
                Some(_) => entries.push(NavItem { weight: *weight, label: label.as_str(), html: item }),
                None => unweighted_terms.push(item),
            }
        }

        sort_nav_items(&mut entries);
        let mut page_items: Vec<String> = entries.into_iter().map(|item| item.html).collect();
        page_items.extend(unweighted_terms);

        // `nav` header mode always opens the link list with Home, ahead of
        // the site's own nav items and outside their weight order — it is
        // the one way back to the site root now that the brand link is gone
        // from the masthead, so it leads even on a site with no nav items of
        // its own.
        if nav_mode {
            let home_label = crate::i18n::t(self.current_lang, "nav_home");
            // Compared as hrefs, not `url_path`s: there is no ParsedDocument
            // for "the home page" in hand here, but `home_path` and every
            // other page_items href are both already `to_pretty_url`-built,
            // so comparing the two forms is comparing like with like.
            let is_home = self.current_page_url.is_some_and(|url| {
                let pretty = crate::build::scan::article_map::to_pretty_url(url);
                format!("/{}", pretty.trim_start_matches('/')) == home_path
            });
            let current_attrs = if is_home { r#" class="active" aria-current="page""# } else { "" };
            page_items.insert(0, format!(r#"<a href="{}"{}>{}</a>"#, home_path, current_attrs, home_label));
        }

        let has_nav_items = !page_items.is_empty();

        // Hamburger (only if nav items exist)
        //
        // aria-expanded reflects the collapsed-by-default state emitted here;
        // theme.ts's toggleMobileMenu keeps it in sync on open/close, and the
        // outside-click handler resets it on close. aria-controls points at
        // the nav-links id below, so a screen reader can name what the button
        // discloses (WCAG 4.1.2).
        let hamburger = if has_nav_items {
            format!(
                r#"<button class="mobile-menu-button" onclick="toggleMobileMenu()" aria-label="{label}" aria-expanded="false" aria-controls="nav-links"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="18" x2="21" y2="18"/></svg></button>"#,
                label = crate::i18n::t(self.current_lang, "nav_menu"),
            )
        } else {
            String::new()
        };

        // A hidden copy of the logo opens both floats. The theme sizes it as it
        // sizes the logo, so a logo taller than the toggles makes each float as
        // tall and the toggles and links centre on it, as in a flex row.
        let strut = if !nav_mode && self.logo_path.is_some() {
            format!(r#"<span class="nav-logo-strut" aria-hidden="true">{}</span>"#, self.logo_img(None))
        } else {
            String::new()
        };

        // Nav links (only if nav items exist). `id="nav-links"` is the
        // aria-controls target above; theme.ts also uses it to toggle `inert`
        // on the closed mobile menu so its (invisible) links leave the tab
        // order (WCAG 2.4.7/2.4.3).
        let nav_links = if has_nav_items {
            format!(r#"<div class="nav-links" id="nav-links">{}{}</div>"#, strut, page_items.join(""))
        } else {
            String::new()
        };

        // The toggles come first so a plain name can wrap beside them as a
        // float from the first paint; site.css orders them to the end edge
        // wherever the row is flex or grid. Focus follows this order.
        let nav_icons = self.nav_icons_html(&strut);
        let nav_right = format!(r#"<div class="nav-right">{}{}</div>"#, hamburger, nav_links);

        format!("{}{}{}", nav_icons, site_name, nav_right)
    }

}

/// Whether a document should be excluded from article listings because
/// it's a navigation item (appears in the nav bar instead).
///
/// In organized mode, all top-level non-index files (of any edition) are nav items.
/// In flat mode, no files are auto-excluded from listings — even keyword
/// nav files like "about" appear in both the nav bar AND the article list.
/// (The nav bar uses `is_nav_keyword` to decide what to show; this function
/// decides what to *exclude from listings*.)
pub fn is_listing_nav_item(doc: &crate::build::types::ParsedDocument, has_content_folders: bool) -> bool {
    match doc.nav {
        Some(true) => true,
        Some(false) => false,
        None => {
            if has_content_folders {
                doc.is_root_level && !crate::build::render::lang_roots::is_language_root(&doc.url_path)
            } else {
                false // Flat mode: all root files are articles in listings
            }
        }
    }
}

#[cfg(test)]
#[path = "nav_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "nav_submenu_tests.rs"]
mod submenu_tests;
