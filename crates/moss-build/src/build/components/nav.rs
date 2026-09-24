//! Navigation generation module for moss static site generator
//!
//! Handles the creation of the main site navigation.

use crate::build::types::ParsedDocument;

use crate::i18n::Language;
use crate::i18n::link::TranslationLink;

/// The masthead's `.nav-left` (breadcrumb trail / site name). A child module
/// so it can build from `NavigationBuilder`'s private state without widening
/// any of it.
mod breadcrumb;
/// The floating nav island. A child module for the same reason.
mod island;

/// A single segment of a breadcrumb trail
pub struct BreadcrumbSegment {
    /// Display title for this segment
    pub title: String,
    /// Relative URL for this segment (empty for current page)
    pub url: String,
    /// Whether this is the current (last) page in the trail
    pub is_current: bool,
}

/// Builder for generating navigation components
pub struct NavigationBuilder<'a> {
    documents: &'a [ParsedDocument],
    site_title: &'a str,
    current_page_url: Option<&'a str>,
    lang: Language,
    breadcrumb_segments: Option<Vec<BreadcrumbSegment>>,
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
    /// Whether the site has content subfolders (organized mode vs flat mode)
    has_content_folders: bool,
    /// Whether this build has a live search index (`LayoutConfig::assets.search` —
    /// the fully resolved gate, NOT the raw `[site].search` bool). When true
    /// a `.nav-search-btn` is emitted into `.nav-icons`; the client runtime
    /// binds to it and lazy-loads `/_moss/pagefind/pagefind.js` on first use.
    has_search: bool,
    /// Editor preview (`emit_source_lines`): `data-source-fm` annotations.
    /// The trail names `breadcrumb:` (the page's own toggle; fm outranks the
    /// surrounding `<nav data-source-none>` in the bridge's resolver).
    /// `fm_logo` is set ONLY when the page IS the homepage declaring the
    /// `logo:` shown — anywhere else the field lives in a different file,
    /// and a chip absent from the open file cannot be revealed.
    fm_breadcrumb: bool,
    fm_logo: bool,
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
            translations: Vec::new(),
            current_lang: lang,
            current_lang_tag: lang.as_bcp47_attr().to_string(),
            logo_path: None,
            has_content_folders,
            has_search: false,
            fm_breadcrumb: false,
            fm_logo: false,
        }
    }

    /// Editor-preview `data-source-fm` annotations — see the field docs.
    pub fn with_source_fm(mut self, breadcrumb: bool, logo: bool) -> Self {
        self.fm_breadcrumb = breadcrumb;
        self.fm_logo = logo;
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
    pub fn with_breadcrumb(mut self, segments: Vec<BreadcrumbSegment>) -> Self {
        self.breadcrumb_segments = Some(segments);
        self
    }

    /// Set translations for the current page (shown as language toggle in nav).
    pub fn with_translations(mut self, lang: Language, lang_tag: &str, translations: Vec<TranslationLink>) -> Self {
        self.current_lang = lang;
        self.current_lang_tag = lang_tag.to_string();
        self.translations = translations;
        self
    }

    /// Set the site logo path (rendered before site name in nav).
    pub fn with_logo(mut self, logo_path: String) -> Self {
        self.logo_path = Some(logo_path);
        self
    }

    /// The language whose chrome this page shows.
    ///
    /// A page's DETECTED language only redirects chrome (home link, nav-item
    /// scoping) when the page actually lives under a language tree
    /// (`en/…`, `zh-hans/…`). Content-detection alone must not: an
    /// English-titled page on a Chinese site with no `en/` tree would get a
    /// `/en/` home link that 404s and an empty nav (no docs match `En`).
    fn effective_lang(&self) -> Language {
        let in_lang_tree = self
            .current_page_url
            .and_then(moss_core::home::lang_tree_prefix)
            .is_some();
        if in_lang_tree { self.current_lang } else { self.lang }
    }

    /// Where the site-name link points: "/" for the site language,
    /// "/zh-hans/" for a page inside a language tree.
    fn home_path(&self) -> String {
        // The tree segment the page ACTUALLY lives under, never a code spelled
        // back out of the interface enum. `en-us/` and `en-gb/` are language
        // trees moss accepts but `Language::code()` cannot spell, so their
        // pages linked home to `/en/` — a path no build emits. The
        // folder name is the path, by construction.
        match self.current_page_url.and_then(moss_core::home::lang_tree_prefix) {
            Some(tree) if self.effective_lang() != self.lang => {
                format!("/{tree}/") // allow:served-path-url-construct (nav href to language-root page, not a framework asset)
            }
            _ => "/".to_string(),
        }
    }

    /// Logo + site name content (used in both plain and breadcrumb modes).
    ///
    /// Phase 2B (2026-05-25): the bare `<img class="site-logo">` emission
    /// is delegated to the unified image synthesizer
    /// (`ImageContext::SiteLogo`) so every `<img>` moss produces flows
    /// through one shape-defining seam. Byte shape is preserved exactly:
    /// `<img class="site-logo" src="..." alt="" aria-hidden="true">`.
    fn logo_html(&self) -> String {
        self.logo_path.as_ref().map(|path| {
            moss_core::render::image::synthesize_image_html(
                path,
                "",
                &moss_core::asset_snapshot::AssetSnapshot::new(),
                moss_core::render::image::ImageContext::SiteLogo,
                &moss_core::render::image::ImageRenderOptions {
                    // Editor preview: name the `logo:` field — only when this
                    // page owns it (see `fm_logo`).
                    extra_attrs: self.fm_logo.then_some(r#"data-source-fm="logo""#),
                    ..Default::default()
                },
            )
        }).unwrap_or_default()
    }

    /// Generates navigation HTML with site name on left and items on right.
    /// Only shows docs with `nav: Some(true)` (explicit opt-in).
    /// If nav items exist: renders hamburger + nav-links + nav-icons.
    /// If no nav items: just site name + nav-icons.
    pub fn generate_navigation(&self) -> String {
        let effective_lang = self.effective_lang();
        let home_path = self.home_path();
        let logo_html = self.logo_html();

        // Build nav-left: breadcrumb trail or plain site name (nav/breadcrumb.rs)
        let site_name = self.nav_left_html(&home_path, &logo_html);

        // Adaptive auto-navigation model:
        // - Organized mode (has content folders): root-level non-index files auto-appear in nav
        // - Flat mode (no content folders): only keyword filenames (about, etc.) auto-appear
        // - nav: true/false always wins (explicit opt-in/out)
        let mut nav_documents: Vec<&ParsedDocument> = self.documents.iter()
            .filter(|doc| {
                // Scope nav to the current page's language tree (per-render filter,
                // not an intrinsic property — stays here, outside the predicate).
                // Keyed on effective_lang: a content-detected language without a
                // language tree keeps the site language's nav items.
                if doc.lang != effective_lang { return false; }
                is_nav_bar_item_doc(doc, self.has_content_folders)
            })
            .collect();

        nav_documents.sort_by(|a, b| {
            match (a.weight, b.weight) {
                // Tied weights fall through to alphabetical so the order is
                // independent of filesystem walk order. Without the tiebreaker
                // macOS and Linux scan_folder() produce different nav orders
                // for items with the same explicit weight.
                (Some(aw), Some(bw)) => aw.cmp(&bw).then_with(|| a.label.cmp(&b.label)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                // Sort alphabetically by the plain-text chrome label.
                (None, None) => a.label.cmp(&b.label),
            }
        });

        let page_items: Vec<String> = nav_documents.iter()
            .map(|doc| {
                // Nav link text uses the chrome label (plain text).
                let label = doc.label.clone();
                let pretty = crate::build::scan::article_map::to_pretty_url(&doc.url_path);
                let href = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (nav href to user content page, not a framework asset)
                let class = if self.current_page_url.map_or(false, |url| url == doc.url_path) {
                    r#" class="active""#
                } else {
                    ""
                };
                format!(r#"<a href="{}"{class}>{}</a>"#, href, label)
            })
            .collect();

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
                r#"<button class="mobile-menu-button" onclick="toggleMobileMenu()" aria-label="{}" aria-expanded="false" aria-controls="nav-links"><svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="18" x2="21" y2="18"/></svg></button>"#,
                crate::i18n::t(self.current_lang, "nav_toggle_menu"),
            )
        } else {
            String::new()
        };

        // Nav links (only if nav items exist). `id="nav-links"` is the
        // aria-controls target above; theme.ts also uses it to toggle `inert`
        // on the closed mobile menu so its (invisible) links leave the tab
        // order (WCAG 2.4.7/2.4.3).
        let nav_links = if has_nav_items {
            format!(r#"<div class="nav-links" id="nav-links">{}</div>"#, page_items.join(""))
        } else {
            String::new()
        };

        // Icon items: search (left), language toggle, theme toggle (right).
        // Left to right the cluster runs from the most content-related control
        // to the most presentation-related one: search sits nearest the nav
        // links it complements (it IS navigation), language changes what
        // content you read, theme is pure presentation and keeps its
        // established right-most anchor.
        let mut icon_items: Vec<String> = Vec::new();

        // Search icon — only when this build actually shipped an index
        // (`_moss/pagefind/`).
        // The client runtime binds the click; with JS off the
        // button is inert, which is why it is a `<button>` and not a link.
        if self.has_search {
            let search_label = crate::i18n::t(self.current_lang, "nav_search");
            icon_items.push(format!(
                r#"<button class="nav-search-btn" type="button" aria-label="{}"><svg class="search-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" viewBox="0 0 32 32"><circle cx="14" cy="14" r="8.5"/><path d="m20.2 20.2 6.3 6.3"/></svg></button>"#,
                search_label,
            ));
        }

        // Language toggle in nav (site-wide: shows all site languages)
        if !self.translations.is_empty() {
            let current = format!(
                r#"<span class="nav-lang-current">{}</span>"#,
                crate::i18n::switcher_label(&self.current_lang_tag)
            );
            let links: Vec<String> = self.translations.iter().map(|t| {
                let pretty = crate::build::scan::article_map::to_pretty_url(&t.url_path);
                let href = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (language toggle nav href to translation page, not a framework asset)
                format!(
                    r#"<a href="{}" class="nav-lang-link" hreflang="{}">{}</a>"#,
                    // The declared tag, not `code()` — whose own doc says to use
                    // the BCP-47 form for an hreflang, and which collapses an
                    // unshipped language onto one of three variants.
                    href, t.lang_tag, t.display_name
                )
            }).collect();
            let mut parts = vec![current];
            parts.extend(links);
            icon_items.push(format!(
                r#"<div class="nav-lang-toggle" aria-label="{}">{}</div>"#,
                crate::i18n::t(self.current_lang, "nav_language"),
                parts.join(" / ")
            ));
        }

        // Theme toggle icon (toggles light ↔ dark) — "classic" style from toggles.dev
        //
        // No `data-tooltip` on any toggle — only `aria-label`. The glyphs are
        // their own labels (a sun/moon, a magnifier), so a pill repeating
        // "Toggle theme" was noise; worse, on touch there is no un-hover, so a
        // tapped toggle's hint just hung there. Hover hints in the nav now
        // exist only where they carry information the row has hidden — a
        // truncated breadcrumb label, a fold's `…` levels — and those are
        // JS-promoted (breadcrumb-hint.ts, breadcrumb-fold.ts), never emitted
        // here.
        let theme_label = crate::i18n::t(self.current_lang, "nav_toggle_theme");
        icon_items.push(format!(
            r#"<button class="nav-theme-btn" type="button" aria-label="{}" onclick="toggleTheme()"><svg class="theme-toggle-icon" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" width="1em" height="1em" fill="currentColor" stroke-linecap="round" viewBox="0 0 32 32"><clipPath id="theme-toggle__classic__cutout"><path d="M0-5h30a1 1 0 0 0 9 13v24H0Z"/></clipPath><g clip-path="url(#theme-toggle__classic__cutout)"><circle cx="16" cy="16" r="9.34"/><g stroke="currentColor" stroke-width="1"><path d="M16 5.5v-4"/><path d="M16 30.5v-4"/><path d="M1.5 16h4"/><path d="M26.5 16h4"/><path d="m23.4 8.6 2.8-2.8"/><path d="m5.7 26.3 2.9-2.9"/><path d="m5.8 5.8 2.8 2.8"/><path d="m23.4 23.4 2.9 2.9"/></g></g></svg></button>"#,
            theme_label,
        ));

        let nav_icons = format!(r#"<div class="nav-icons">{}</div>"#, icon_items.join(""));
        let nav_right = format!(r#"<div class="nav-right">{}{}{}</div>"#, hamburger, nav_links, nav_icons);

        format!("{}{}", site_name, nav_right)
    }

    /// Generates footer HTML.
    ///
    /// Layout:
    /// ```html
    /// <footer class="container">
    ///   {footer.md HTML, if present}
    ///   {default link list <p class="footer-default">, if any links}
    ///   {auto-injected subscribe form, if moss-hosted with [channels.email]}
    /// </footer>
    /// ```
    ///
    /// Three-segment vertical stack: leading author chrome (footer.md) →
    /// auto-generated link list → trailing widget (subscribe form). The
    /// trailing-widget position is a deliberate design decision — links lead,
    /// the auto-injected widget trails.
    ///
    /// Flat HTML — authored content sits as direct children of `<footer>`.
    /// The default visual chrome (border-top divider, padding, muted
    /// typography) lives on `footer.container` directly in CSS; the footer
    /// renders at the body font-size by default. This shape gives sites two
    /// ways to customize:
    ///
    /// 1. Override `footer.container { ... }` to replace the default chrome.
    /// 2. Use `body > footer.container > selector` rules to target individual
    ///    elements for custom designs (e.g. brand text +
    ///    :::grid + copyright + :::subscribe stack with custom flex layout).
    ///
    /// History: An earlier "verbatim footer" design (commit 6e47a8024)
    /// stripped the `.footer-content` wrapper but left no chrome on
    /// `<footer>` either, removing the divider + muted typography from the
    /// default look. The current shape restores the chrome on `footer.container`
    /// directly so existing sites' `body > footer.container > *` direct-
    /// child selectors keep working.
    ///
    /// When `footer.md` is absent, the wrapper still emits with default
    /// content: an auto-generated link list from pages with `footer: true`
    /// frontmatter, plus an optional RSS link.
    ///
    /// `current_page_url` is used to mark the matching footer link as
    /// `.active` (parallel to `generate_navigation`).
    pub fn generate_footer(&self, show_rss: bool) -> String {
        let mut footer_pages: Vec<&ParsedDocument> = self.documents
            .iter()
            .filter(|d| d.footer == Some(true) && d.lang == self.current_lang)
            .collect();
        footer_pages.sort_by(|a, b| match (a.weight, b.weight) {
            // Tied weights fall through to alphabetical for cross-platform
            // determinism — see comment in generate_navigation().
            (Some(aw), Some(bw)) => aw.cmp(&bw).then_with(|| a.label.cmp(&b.label)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            // Sort alphabetically by the plain-text chrome label.
            (None, None) => a.label.cmp(&b.label),
        });

        let mut default_links = Vec::new();
        for doc in &footer_pages {
            let pretty = crate::build::scan::article_map::to_pretty_url(&doc.url_path);
            let href = format!("/{}", pretty.trim_start_matches('/')); // allow:served-path-url-construct (footer nav href to user content page, not a framework asset)
            let class = if self.current_page_url.map_or(false, |url| url == doc.url_path) {
                "footer-link active"
            } else {
                "footer-link"
            };
            // Footer link text is chrome; use the plain-text label.
            default_links.push(format!(
                r#"<a href="{}" class="{}">{}</a>"#,
                href, class, doc.label
            ));
        }

        if show_rss {
            let rss_url = crate::build::served_path::ServedPath::for_rss("").unwrap().to_relative_url();
            default_links.push(format!(
                r#"<a href="{rss_url}" class="footer-link" data-external>{}</a>"#,
                crate::i18n::t(self.lang, "rss"),
            ));
        }

        // Two slot markers, resolved during the pre-ship slot pass:
        //   slot:footer-left  — author chrome (footer.md) or empty
        //   slot:footer-end   — auto-injected subscribe form or empty
        // When both content slots are empty, only the default link list shows.
        //
        // Footer LAYOUT is driven purely by CSS: `footer.container:has(> .moss-subscribe)`
        // lays the footer out as a flex row (links left, subscribe form
        // right-anchored) — see site.css. There is no `data-moss-shape` marker:
        // the retired attribute existed only to toggle that layout from the
        // build side, but keying the CSS on the presence of the (now-unified)
        // `.moss-subscribe` form covers the auto-injected AND footer.md cases
        // uniformly, so the footer open tag is a plain `<footer class="container">`.
        //
        // The default link list is emitted in a wrapping <p> with class
        // `footer-default` so themes can hide it (`.footer-default { display:
        // none }`) when they author a richer footer.
        //
        // No `.footer-content` wrapper here: the visual chrome (divider,
        // padding, muted typography) lives on `<footer class="container">`
        // directly. This keeps the HTML flat — author content sits as direct
        // children of <footer>, which lets sites use `body > footer.container
        // > selector` rules to target individual elements
        // for their custom design.
        let default_inner = if default_links.is_empty() {
            String::new()
        } else {
            format!(
                "\n        <p class=\"footer-default\">{}</p>",
                default_links.join(" · ")
            )
        };

        format!(
            r#"<footer class="container">
        <!-- slot:footer-left -->{default_inner}
        <!-- slot:footer-end -->
</footer>"#
        )
    }

}

/// Filenames that auto-detect as nav pages in flat mode.
/// Only these keywords cause a root-level file to appear in the nav bar
/// when the site has no content folders.
pub fn is_nav_keyword(stem: &str) -> bool {
    matches!(stem.to_lowercase().as_str(), "about" | "关于" | "關於")
}

/// True when a doc reads as a navigational/landing page by its position and
/// name, BEFORE any explicit `nav:` override: a root-level non-index file in
/// organized mode, or an `about`-keyword root file in flat mode.
/// `is_root_index` is `url_path == "index.html"` (the site home).
///
/// This is the no-override (`None`) case of [`is_nav_bar_item`] — the structural
/// "should this auto-appear in the nav bar?" detection, extracted so it can be
/// tested in isolation. It is NOT used for template/layout selection:
/// `select_shell_type` keys Page-vs-Article off `url_path` depth, a
/// deliberately different question (every non-index file's `url_path` ends in
/// `/index.html`, so that test classifies pages differently than this predicate).
pub fn is_navigational_page(
    is_root_level: bool,
    is_root_index: bool,
    clean_stem: &str,
    has_content_folders: bool,
) -> bool {
    is_root_level && !is_root_index && (has_content_folders || is_nav_keyword(clean_stem))
}

/// Does this document render as an item in the top navigation bar?
///
/// Single source of truth for "is a top-nav item": `generate_navigation`,
/// `has_nav_items`, and title-injection suppression all consult this. NOT the
/// same question as `is_listing_nav_item` (article-listing exclusion), which
/// intentionally diverges in flat mode.
///
/// `slot_only` wins over an explicit `nav: true` — slot files (e.g. `footer.md`)
/// are structural chrome and are excluded unconditionally.
/// `draft` also wins over an explicit `nav: true` — draft pages are hidden from
/// all listings, including the top nav bar.
/// The `None` (no explicit frontmatter) case delegates to `is_navigational_page`.
///
/// `is_root_index` is the top-level site index (`url_path == "index.html"`),
/// never a nav item. A subfolder `index.html` is excluded by the
/// `!is_root_level` arm instead, not this flag.
pub fn is_nav_bar_item(
    nav: Option<bool>,
    draft: bool,
    slot_only: bool,
    is_root_level: bool,
    is_root_index: bool,
    clean_stem: &str,
    has_content_folders: bool,
) -> bool {
    // Slot files (footer.md) are chrome; drafts are hidden. Neither is a nav item,
    // and draft wins even over an explicit `nav: true`.
    if slot_only || draft {
        return false;
    }
    match nav {
        Some(v) => v, // explicit opt-in / opt-out
        None => is_navigational_page(is_root_level, is_root_index, clean_stem, has_content_folders),
    }
}

/// `ParsedDocument` convenience wrapper over [`is_nav_bar_item`].
///
/// Does NOT filter by language — callers that care about the current language
/// tree must pre-filter or pass a per-language document slice. The language
/// guard lives in `generate_navigation`, not in the predicate.
pub fn is_nav_bar_item_doc(
    doc: &crate::build::types::ParsedDocument,
    has_content_folders: bool,
) -> bool {
    is_nav_bar_item(
        doc.nav,
        doc.draft == Some(true),
        doc.slot_only,
        doc.is_root_level,
        doc.url_path == "index.html",
        &doc.clean_stem,
        has_content_folders,
    )
}

/// Whether a document should be excluded from article listings because
/// it's a navigation item (appears in the nav bar instead).
///
/// In organized mode, all root-level non-index files are nav items.
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
                doc.is_root_level && doc.url_path != "index.html"
            } else {
                false // Flat mode: all root files are articles in listings
            }
        }
    }
}

/// Whether any documents qualify as nav bar items under the adaptive model.
/// Used by breadcrumb auto-enable: breadcrumbs turn on when the nav bar is empty.
pub fn has_nav_items(docs: &[crate::build::types::ParsedDocument], has_content_folders: bool) -> bool {
    docs.iter().any(|doc| is_nav_bar_item_doc(doc, has_content_folders))
}

/// Titlecase a path segment: replace hyphens with spaces, capitalize each word.
pub fn titlecase_segment(segment: &str) -> String {
    segment
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Compute breadcrumb segments for a given document.
///
/// Returns `Some(segments)` when breadcrumbs should be shown, `None` otherwise.
///
/// Breadcrumbs are shown when:
/// 1. The homepage has `breadcrumb: true` (site-wide enable)
/// 2. The current page has NOT set `breadcrumb: false` (per-page override)
/// 3. The page's effective depth > 0 (homepages don't get breadcrumbs)
///
/// The first segment is always the site name linking home.
/// The last segment is the current page as plain text (not a link).
/// Middle segments link to their folder's index page.
///
/// **Translation roots**: A folder whose index page is a translation of the site
/// homepage (linked via `translationKey` or stem convention) is transparent in
/// breadcrumbs — it doesn't appear as a segment. Instead, the home segment links
/// to that folder's URL.
pub fn compute_breadcrumb_segments(
    doc: &ParsedDocument,
    all_docs: &[ParsedDocument],
    site_title: &str,
    has_content_folders: bool,
) -> Option<Vec<BreadcrumbSegment>> {
    // Parse URL path into folder parts
    let parts: Vec<&str> = doc.url_path.split('/').collect();
    let folder_parts = &parts[..parts.len() - 1];
    let filename = parts.last().unwrap_or(&"");

    // Detect translation root: if the first folder's index is a translation of the
    // root homepage, skip it from breadcrumb segments and use its URL as home.
    let (effective_parts, home_url) = if !folder_parts.is_empty() {
        let first_folder_index = format!("{}/index.html", folder_parts[0]);
        let is_translation_root = all_docs.iter()
            .find(|d| d.url_path == first_folder_index)
            .map(|d| d.translations.iter().any(|t| t.url_path == "index.html"))
            .unwrap_or(false);
        if is_translation_root {
            (&folder_parts[1..], format!("/{}/", folder_parts[0])) // allow:served-path-url-construct (breadcrumb home href to translation root, not a framework asset)
        } else {
            (folder_parts, "/".to_string())
        }
    } else {
        (folder_parts, "/".to_string())
    };

    // Rule 3: homepages don't get breadcrumbs.
    // For translation roots, effective depth is after stripping the prefix.
    // e.g. zh-hans/index.html has effective depth 0 (it IS the translation homepage).
    if effective_parts.is_empty() && *filename == "index.html" {
        return None;
    }
    if effective_parts.is_empty() {
        return None;
    }

    // Rule 1: breadcrumb enable logic.
    // Explicit homepage setting wins. Otherwise auto-enable when no nav items exist
    // (the nav bar only has site name + theme toggle, so breadcrumbs provide orientation).
    let homepage_breadcrumb = all_docs
        .iter()
        .find(|d| d.url_path == "index.html")
        .and_then(|d| d.breadcrumb);

    let breadcrumb_enabled = match homepage_breadcrumb {
        Some(true) => true,
        Some(false) => false,
        None => {
            // Auto-enable when no nav items exist
            let has_nav_items = has_nav_items(all_docs, has_content_folders);
            !has_nav_items
        }
    };

    if !breadcrumb_enabled {
        return None;
    }

    // Rule 2: per-page override — breadcrumb: false disables
    if doc.breadcrumb == Some(false) {
        return None;
    }

    let mut segments = Vec::new();

    // First segment: site name linking (language-specific) home
    segments.push(BreadcrumbSegment {
        title: site_title.to_string(),
        url: home_url.clone(),
        is_current: false,
    });

    // Determine the lang prefix for reconstructing full paths (if translation root was stripped)
    let lang_prefix = if home_url != "/" {
        Some(folder_parts[0])
    } else {
        None
    };

    // Build a URL for a folder path, prepending the language prefix if present
    let folder_url = |effective_path: &str| -> String {
        if let Some(prefix) = lang_prefix {
            format!("/{}/{}/", prefix, effective_path) // allow:served-path-url-construct (breadcrumb href to user content folder with language prefix, not a framework asset)
        } else {
            format!("/{}/", effective_path) // allow:served-path-url-construct (breadcrumb href to user content folder, not a framework asset)
        }
    };

    // Middle and final segments: iterate effective_parts (translation root already excluded)
    for (i, part) in effective_parts.iter().enumerate() {
        let is_last = i == effective_parts.len() - 1;

        // Build full path for looking up the folder's index document
        let effective_path = effective_parts[..=i].join("/");
        let folder_index_path = if let Some(prefix) = lang_prefix {
            format!("{}/{}/index.html", prefix, effective_path)
        } else {
            format!("{}/index.html", effective_path)
        };

        // Look up the folder's chrome label (plain text) from all_docs.
        let title = all_docs
            .iter()
            .find(|d| d.url_path == folder_index_path)
            .map(|d| d.label.clone())
            .unwrap_or_else(|| titlecase_segment(part));

        if is_last {
            // For the last folder part, check if the file is index.html
            // If so, this IS the current page
            if *filename == "index.html" {
                segments.push(BreadcrumbSegment {
                    title,
                    url: String::new(),
                    is_current: true,
                });
            } else {
                // There's a non-index file, so the folder is a middle segment
                // and the file itself is the current page
                segments.push(BreadcrumbSegment {
                    title,
                    url: folder_url(&effective_path),
                    is_current: false,
                });
                segments.push(BreadcrumbSegment {
                    // Breadcrumb text is chrome; use the plain-text label.
                    title: doc.label.clone(),
                    url: String::new(),
                    is_current: true,
                });
            }
        } else {
            // Middle segment: link to the folder index
            segments.push(BreadcrumbSegment {
                title,
                url: folder_url(&effective_path),
                is_current: false,
            });
        }
    }

    Some(segments)
}

#[cfg(test)]
#[path = "nav_tests.rs"]
mod tests;
