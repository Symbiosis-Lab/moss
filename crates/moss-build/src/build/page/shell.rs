//! Shell system for static site generation.
//!
//! This module provides the HTML templating infrastructure including:
//! - Shell types for different page layouts (Page, Article)
//! - Shell registry for managing HTML templates
//! - Shell variables for type-safe template processing
//! - Shell processor for variable replacement
//!
//! Terminology follows Hugo conventions:
//! Reference: https://gohugo.io/quick-reference/glossary/

use crate::build::types::ParsedDocument;
use moss_core::PageKind;

/// Shell types for different page layouts
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellType {
    /// Homepage and general pages with sidebar
    Page,
    /// Journal entries with centered layout
    Article,
}

/// Shell registry for managing HTML content fragments.
///
/// Both page types share the single `SHELL_TEMPLATE` (head + chrome) and
/// differ only in the `<main>` content fragment selected here. The shell is
/// spliced with the fragment in `ShellProcessor::process`.
pub struct ShellRegistry {
    page_content: &'static str,
    article_content: &'static str,
}

impl ShellRegistry {
    pub fn new() -> Self {
        Self {
            page_content: PAGE_CONTENT,
            article_content: ARTICLE_CONTENT,
        }
    }

    /// Returns the `<main>` content fragment for the given page type. The
    /// caller splices it into `SHELL_TEMPLATE`'s `{main_content}` slot.
    pub fn get_shell(&self, shell_type: ShellType) -> &'static str {
        match shell_type {
            ShellType::Page => self.page_content,
            ShellType::Article => self.article_content,
        }
    }

    /// Did the author write `layout: article` on this page?
    ///
    /// This is the *authored intent*, not the resolved template. It is a much
    /// narrower question than `select_shell_type(..) == Article`, which is
    /// also true of every ordinary nested page that said nothing at all.
    ///
    /// Ask this — never the resolved `ShellType` — when the behaviour should
    /// follow from the author calling the page a piece. Series navigation is the
    /// case that forced the distinction: a folder index is skipped by
    /// its siblings' prev/next chain *because it is a doorway*, and `layout:
    /// article` is how an author says this particular one is a step instead. If
    /// that had been keyed on the resolved template, every nested folder index
    /// on an organized site would have become a step by accident.
    pub fn is_article_layout(doc: &ParsedDocument) -> bool {
        doc.layout.as_deref() == Some("article")
    }

    /// Determines the appropriate template type for a given document and context.
    ///
    /// Adaptive selection based on site mode:
    /// - Organized (has_content_folders): root non-index = Page, nested = Article
    /// - Flat (!has_content_folders): nav keywords (about) = Page, everything else = Article
    ///
    /// Synthetic asset (per-image) pages always render as Page, regardless of
    /// nesting: an image-only page has no authored prose, so it gets the clean
    /// Page shell (`og:type=website`, no schema.org Article JSON-LD) rather than
    /// blog-Article chrome. Without this, a nested per-image page would fall
    /// through the organized-mode "nested = Article" arm and emit spurious
    /// Article metadata on a content-less hero-only page.
    ///
    /// # Arguments
    /// * `doc` - The parsed document, if any
    /// * `is_homepage` - Whether this is the homepage
    /// * `has_content_folders` - Whether the site has content subfolders
    pub fn select_shell_type(
        doc: Option<&ParsedDocument>,
        is_homepage: bool,
        has_content_folders: bool,
    ) -> ShellType {
        match doc {
            Some(doc) => {
                // Frontmatter override wins
                if doc.layout.as_deref() == Some("page") { return ShellType::Page; }
                if Self::is_article_layout(doc) { return ShellType::Article; }
                // Homepage and folder index files are always Pages (unless the
                // layout override above applies) — the clean Page shell, not
                // blog-Article chrome.
                if is_homepage || doc.kind == PageKind::Folder {
                    return ShellType::Page;
                }
                // An explicit `nav: true` pins the page into the nav bar, so it
                // renders with the clean Page layout — not blog-Article chrome
                // (comments, series nav, schema.org Article). Overridable by
                // `layout:` above; `nav: false` falls through to the auto-detect
                // below (it only removes the page from the nav bar).
                if doc.nav == Some(true) {
                    return ShellType::Page;
                }
                if has_content_folders {
                    // Organized mode: root-level = Page, nested = Article
                    if !doc.url_path.contains('/') {
                        ShellType::Page
                    } else {
                        ShellType::Article
                    }
                } else {
                    // Flat mode: nav keywords = Page, everything else = Article
                    if doc.is_root_level && crate::build::components::nav::is_nav_keyword(&doc.clean_stem) {
                        ShellType::Page
                    } else {
                        ShellType::Article
                    }
                }
            }
            None => ShellType::Page,
        }
    }
}

impl Default for ShellRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Shell variable for type-safe template processing
#[derive(Debug)]
pub struct ShellVars {
    pub title: String,
    pub css_path: String,
    pub js_path: String,
    /// The `data-*` pointers to lazy JS chunks, for the theme script tag:
    /// `data-share-card="…"` always, `data-hls="…"` only on a site with a
    /// video ladder.
    pub lazy_chunk_attrs: String,
    pub navigation: String,
    /// Floating nav island markup, or empty when the page has no
    /// breadcrumb trail to continue. Emitted as a sibling *before* `<header>`
    /// so it is `position: fixed` against the viewport rather than against any
    /// transformed ancestor the masthead might acquire.
    pub nav_island: String,
    pub homepage_content: String,
    pub latest_list: Option<String>,
    pub latest_sidebar: Option<String>,
    pub favicon: Option<String>,
    /// RSS feed link for head section
    pub rss_link: Option<String>,
    /// Analytics script tag for head section
    pub analytics: Option<String>,
    pub footer: Option<String>,
    /// Body attributes for preview mode (e.g., ` data-moss-preview`)
    pub body_attrs: String,
    /// Page wrapper class modifier (e.g., " has-sidebar" for sidebar layout)
    /// DEPRECATED: Use main_class instead. Kept for backward compatibility.
    pub page_wrapper_class: String,
    /// Hero section HTML (placed outside <main> for full-width display)
    pub hero_section: Option<String>,
    /// `data-share-cover="<url>"` for `<article class="container">`, or empty.
    ///
    /// The page's own cover image, handed to the reader runtime instead of
    /// left for it to guess: `share-card.ts` draws this at the top of a quote
    /// card. The build knows which image the author chose (hero, then
    /// `cover:`), so it says so — a JS selector over rendered markup rots the
    /// moment the markup changes, which is exactly how the cover strip went
    /// missing (`.article-cover`, a class moss has never emitted). Empty when
    /// the page has no author-chosen cover; empty means "no cover strip", and
    /// never the auto-generated og card. Chosen by
    /// `crate::build::page::cover::share_cover_url`.
    pub share_cover_attr: String,
    /// The served path of this page's QR code, handed to the reader runtime for
    /// the same reason as `share_cover_attr`: so no JavaScript has to guess it.
    /// Empty when the page has no QR (unpublished site, draft) — and empty is
    /// then the *complete* answer, not a hint to go looking. Chosen by
    /// `crate::build::media::qr::share_qr_for_page`.
    pub share_qr_attr: String,
    /// Main element class (e.g., "has-sidebar" for sidebar layout)
    pub main_class: String,
    // Article-specific variables
    pub date: Option<String>,
    pub formatted_date: Option<String>,
    /// Date line + Aa font panel HTML (only for dated articles)
    pub date_line: Option<String>,
    pub short_date: Option<String>,
    pub content: Option<String>,
    /// Meta description for SEO
    pub description: Option<String>,
    /// Open Graph meta tags (article:published_time, og:title, etc.)
    pub og_tags: Option<String>,
    /// Pre-rendered Twitter Card meta tags block (twitter:card, twitter:title,
    /// twitter:description, twitter:image, twitter:image:alt). Empty/None when
    /// no twitter card should emit (homepage with no description, etc.).
    pub twitter_tags: Option<String>,
    /// Pre-rendered <link rel="canonical" href="..."> tag, or None when no
    /// canonical URL is available (preview-only builds, etc.).
    pub canonical_link: Option<String>,
    /// Pre-rendered block of <link rel="alternate" hreflang="..."> tags for
    /// multilingual pages, or None when the page has no translations or no
    /// site domain is configured. Skipped on visual-template pages (image-only
    /// covers don't need translation hints).
    pub hreflang_links: Option<String>,
    /// Pre-rendered <link rel="apple-touch-icon" href="..."> tag. None until
    /// Task 4.3c wires favicon-rasterizer PNG output through here. Emitting an
    /// SVG-pointing tag would be worse than no tag (Apple Link Presentation
    /// can't use SVG and would fail more loudly than just falling back).
    pub apple_touch_icon: Option<String>,
    /// Schema.org JSON-LD structured data
    pub schema_json_ld: Option<String>,
    /// Data attribute for content width override: `data-content-width="wide"` or empty string
    pub content_width_attr: String,
    /// Data attribute for typesetting direction: `data-typesetting="vertical"` or empty string
    pub typesetting_attr: String,
    /// Data attribute for comments preference: `data-comments="false"` (opt-out),
    /// `data-comments="true"` (opt-in), or empty string (no preference, follow plugin default)
    pub comments_attr: String,
    /// HTML lang attribute value (e.g., "en", "zh-hans")
    pub lang: String,
    /// The **interface** language: the one moss's own chrome speaks here.
    ///
    /// Not the same thing as `lang` above, and the difference is the reason
    /// this is a field at all. `lang` describes the *content*, and a document
    /// may declare a language moss has no interface for (`lang="ja"`, see
    /// `i18n::declared_lang_tag`); the colophon credit and the skip link are
    /// moss speaking, so they follow this instead. The colophon's `<a>` carries
    /// it as its own `lang`, which is also what the CSS giving 青苔 its lead
    /// and its face keys on.
    ///
    /// `substitutions` derives the four strings from it — the colophon's
    /// announced name, its visible wordmark, its BCP-47 tag, and the skip-link
    /// label — the way it derives `moss_mark` from the SVG. They were four
    /// pre-rendered `String`s until 2026-08-28, which meant both construction
    /// sites spelled the same four `i18n::t` calls out identically.
    pub ui_lang: crate::i18n::Language,
    /// Optional user CSS link tag (inserted after the default stylesheet).
    /// Source: `.moss/theme/style.css`; served at `/_moss/theme/style.css`.
    /// Holds the `<link rel="stylesheet">` tag when that file is present.
    pub user_css_link: Option<String>,
    /// Optional user JS script tag (inserted after theme.js, before body-end slot).
    /// Source: `.moss/theme/script.js`; served at `/_moss/theme/script.js`.
    /// Includes the preceding `window.mossTheme.base` global so the script can
    /// resolve co-located theme assets via `new URL("asset.woff2", mossTheme.base)`.
    pub user_js_tag: Option<String>,
    /// Site-wide flag: true when ANY document in the site has `sidebar` set.
    /// Used by Task 3 to add `class="has-sidebar-layout"` on the `<html>` element
    /// for content width adjustments.
    pub has_sidebar_layout: bool,
    /// HTML to inject at the `<!-- slot:head-end -->` slot: the
    /// `<model-viewer>` `<script type=module>` tag when the page's rendered
    /// HTML contains a 3D embed (`render::model::page_needs_model_viewer_script`),
    /// empty string otherwise.
    pub embed_head_assets: String,
    /// HTML for native post-article modules (currently series-nav for ordered
    /// folders) that render as siblings of `<article>` inside `<main>`,
    /// AFTER the `<!-- slot:after-article -->` plugin slot. This is the
    /// `{post_article}` placeholder in `article.html`. Empty for pages that
    /// don't use the article template, or for article pages without any such
    /// module. See `render/html.rs` for the population site.
    ///
    /// Architectural note: this lives outside the `slots.merge` plugin slot
    /// system because the renderer (series_nav) needs `path_resolver`,
    /// `all_docs`, and `parent_doc.label`, which `features.rs::apply_features`
    /// does not currently receive. Promoting to a `Slot::PostArticle` plus
    /// per-page enhance content would be the cleaner long-term home, and the
    /// natural moment to do that is when a second native post-article module
    /// (share buttons, author bio, related articles) lands.
    pub post_article: String,
    /// The shell's whole runtime `<script>` block — every gated `_moss/js/*`
    /// tag, already ordered and `defer`-marked, or empty when this build ships
    /// none. Built by `emit::scripts::ScriptAssets::shell_tags` from the
    /// `SITE_SCRIPTS` table, so which scripts appear is decided in one place
    /// and a new script needs no new template variable. Whole-block rather
    /// than one var per script because every gate is SITE-level: the set is
    /// identical on every page of a build.
    pub runtime_js_tags: String,
    /// Robots meta tag (e.g. noindex for drafts). Empty when omitted.
    pub robots_meta: Option<String>,
}

/// Shell processor for centralized template variable replacement
pub struct ShellProcessor {
    registry: ShellRegistry,
}

impl ShellProcessor {
    pub fn new() -> Self {
        Self {
            registry: ShellRegistry::new(),
        }
    }

    /// Process template with variables and return HTML.
    ///
    /// Placeholder substitution is a SINGLE PASS over template text only
    /// (`substitute_template_tokens`); substituted values are never rescanned,
    /// so author content can contain literal `{name}`-style text.
    pub fn process(&self, shell_type: ShellType, vars: ShellVars) -> String {
        // Compose FIRST: splice the per-type `<main>` content fragment into the
        // shared shell's `{main_content}` slot. Both operands are static
        // template text (no user-derived value is in the string yet), so the
        // combined result is still pure template — safe for the single
        // substitution pass below.
        let fragment = self.registry.get_shell(shell_type);
        let template = SHELL_TEMPLATE.replace("{main_content}", fragment);

        // Preview flag for the chrome post-passes; read before the map takes `body_attrs`.
        let is_preview = vars.body_attrs.contains("data-moss-preview");

        // Theme color meta tags — derived from the --moss-color-bg token in tokens.json.
        // Note: these reflect the static token bg value baked into tokens.json.
        // Author background overrides (via .moss/theme/style.css) are applied at
        // toggle-time by theme.js JS sync, NOT tracked here at build time.
        let (bg_light, bg_dark) = {
            use moss_core::contract::tokens::{bg_colors, load_tokens};
            let t = load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {}", e));
            let (l, d) = bg_colors(&t);
            (l.to_owned(), d.to_owned())
        };

        // main_class_attr: expands to ` class="X"` when non-empty, empty string otherwise
        let main_class_attr =
            if vars.main_class.is_empty() { String::new() } else { format!(" class=\"{}\"", vars.main_class) };
        // html_class_attr: adds class="has-sidebar-layout" on <html> when site has sidebar docs
        let html_class_attr =
            if vars.has_sidebar_layout { " class=\"has-sidebar-layout\"".to_string() } else { String::new() };
        // latest_sidebar: when present, add trailing newline for template formatting
        let latest_sidebar =
            vars.latest_sidebar.map(|s| format!("{}\n        ", s)).unwrap_or_default();

        // Every placeholder the templates may carry, in one lookup table. Tokens
        // absent from the current templates (main_class, date, …) stay registered
        // so a template that reintroduces them skips the unknown-token drop.
        let values: std::collections::HashMap<&'static str, String> = std::collections::HashMap::from([
            // Escaped like every other attribute value here. The allowlist in
            // `resolve_site_default_lang` is what actually closes this today;
            // this is the second lock, so a future producer that reaches
            // `<html lang>` without passing the allowlist cannot break out of
            // the attribute.
            ("lang", crate::build::page::meta::escape_html_attr(&vars.lang)),
            ("html_class_attr", html_class_attr),
            // `<title>` is RCDATA, so `</title><script>` in a frontmatter title
            // closes the element and escapes into the page. Character
            // references are decoded there, so escaping is invisible to the
            // reader. The same title is already escaped for `og:title`.
            ("title", crate::build::page::meta::escape_html_attr(&vars.title)),
            ("css_path", vars.css_path),
            ("js_path", vars.js_path),
            ("lazy_chunk_attrs", vars.lazy_chunk_attrs),
            ("navigation", vars.navigation),
            ("nav_island", vars.nav_island),
            ("homepage_content", vars.homepage_content),
            ("body_attrs", vars.body_attrs),
            ("page_wrapper_class", vars.page_wrapper_class),
            ("hero_section", vars.hero_section.unwrap_or_default()),
            ("share_cover_attr", vars.share_cover_attr),
            ("share_qr_attr", vars.share_qr_attr),
            ("main_class", vars.main_class),
            ("main_class_attr", main_class_attr),
            ("content_width_attr", vars.content_width_attr),
            ("typesetting_attr", vars.typesetting_attr),
            ("comments_attr", vars.comments_attr),
            // moss's own voice, derived from one language rather than passed
            // in four already-rendered pieces — see `ShellVars::ui_lang`.
            ("colophon_text", crate::i18n::t(vars.ui_lang, "published_with_moss").to_string()),
            ("colophon_name", crate::i18n::t(vars.ui_lang, "colophon_name").to_string()),
            ("colophon_lang", vars.ui_lang.as_bcp47_attr().to_string()),
            ("moss_mark", MOSS_MARK_SVG.to_string()),
            ("skip_link_label", crate::i18n::t(vars.ui_lang, "skip_to_content").to_string()),
            ("latest_list", vars.latest_list.unwrap_or_default()),
            ("latest_sidebar", latest_sidebar),
            ("favicon", vars.favicon.unwrap_or_default()),
            ("robots_meta", vars.robots_meta.unwrap_or_default()),
            ("rss_link", vars.rss_link.unwrap_or_default()),
            ("analytics", vars.analytics.unwrap_or_default()),
            ("footer", vars.footer.unwrap_or_default()),
            // Article-specific variables
            ("date", vars.date.unwrap_or_default()),
            ("formatted_date", vars.formatted_date.unwrap_or_default()),
            ("date_line", vars.date_line.unwrap_or_default()),
            ("short_date", vars.short_date.unwrap_or_default()),
            ("content", vars.content.unwrap_or_default()),
            // Semantic markup for articles (Open Graph and Schema.org).
            // Description is escaped for HTML attribute safety (quotes, angle brackets).
            ("description", crate::build::page::meta::escape_html_attr(&vars.description.unwrap_or_default())),
            ("og_tags", vars.og_tags.unwrap_or_default()),
            ("canonical_link", vars.canonical_link.unwrap_or_default()),
            ("hreflang_links", vars.hreflang_links.unwrap_or_default()),
            ("twitter_tags", vars.twitter_tags.unwrap_or_default()),
            ("apple_touch_icon", vars.apple_touch_icon.unwrap_or_default()),
            ("theme_color_light", bg_light),
            ("theme_color_dark", bg_dark),
            ("schema_json_ld", vars.schema_json_ld.unwrap_or_default()),
            // User CSS link (placed after default stylesheet in <head>)
            ("user_css_link", vars.user_css_link.unwrap_or_default()),
            // Native post-article modules (series-nav, etc.; article fragment only)
            ("post_article", vars.post_article),
            // Every gated runtime script tag, in SITE_SCRIPTS order
            ("runtime_js_tags", vars.runtime_js_tags),
            // User JS tag (placed after theme.js, before body-end slot)
            ("user_js_tag", vars.user_js_tag.unwrap_or_default()),
        ]);

        let mut result = substitute_template_tokens(&template, &values);

        // Typed embed head_assets (e.g., <script type=module> for <model-viewer>).
        // Prepended to the plugin slot so that plugin ENHANCE hooks can still add
        // their own assets after ours. Anchors on template chrome, not a placeholder.
        if !vars.embed_head_assets.is_empty() {
            result = result.replace(
                "<!-- slot:head-end -->",
                &format!("{}<!-- slot:head-end -->", vars.embed_head_assets),
            );
        }

        // In preview mode, mark template chrome (nav, footer, series-nav,
        // reading-preferences widget) as unmappable so the iframe-bridge can
        // suppress click-to-source on these elements. These post-passes run on
        // the substituted document ON PURPOSE: series-nav and the font-anchor
        // widget arrive via substituted values ({post_article}, {date_line}).
        if is_preview {
            // Main site nav — the FIRST <nav> in every template is the
            // <nav class="main-nav container"> header bar.
            result = result.replacen("<nav ", "<nav data-source-none ", 1);
            // Footer chrome — always a single <footer> per page.
            result = result.replacen("<footer ", "<footer data-source-none ", 1);
            // Series-nav — emitted as a SECOND <nav class="moss-series-nav">
            // via {post_article}; replacen with count=1 would hit main-nav
            // first, so target the class-prefixed opening tag directly.
            result = result.replace(
                r#"<nav class="moss-series-nav""#,
                r#"<nav class="moss-series-nav" data-source-none"#,
            );
            // Reading-preferences widget (font-trigger + font-pill) — injected
            // as template chrome alongside the article date.  Not author-
            // editable; suppress source-mapping for the whole anchor div.
            result = result.replace(
                r#"<div class="font-anchor""#,
                r#"<div class="font-anchor" data-source-none"#,
            );
        }

        result
    }
}

/// Substitute `{token}` placeholders in TEMPLATE text, in a single
/// left-to-right pass. A token is a maximal run of ASCII lowercase/underscore
/// immediately followed by `}`.
///
/// - Known token (present in `values`) → the value is appended VERBATIM and
///   never rescanned, so values carrying author content (`{content}`,
///   `{title}`, `{navigation}`, …) can contain literal `{name}`-style text
///   without it being re-substituted or stripped.
/// - Unknown token matching `{lowercase_with_underscores}` → dropped. This is
///   the safety net for template/code mismatches (a placeholder present in a
///   template but missing from `ShellVars`), and it can only ever fire on
///   template text — user content never flows through this scan.
/// - Anything else (`{}`, `{CONSTANT}`, `{item1}`, JS/CSS braces) → kept
///   literally.
fn substitute_template_tokens(
    template: &str,
    values: &std::collections::HashMap<&'static str, String>,
) -> String {
    let mut out = String::with_capacity(template.len() + 1024);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        // `open` is an ASCII `{` and `token_len` counts ASCII bytes, so every
        // boundary below is real; `get` keeps a malformed template from
        // aborting the build.
        let (Some(before), Some(after)) = (rest.get(..open), rest.get(open + 1..)) else {
            break;
        };
        out.push_str(before);
        let token_len = after
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || *b == b'_')
            .count();
        if token_len > 0 && after.as_bytes().get(token_len) == Some(&b'}') {
            if let Some(value) = after.get(..token_len).and_then(|token| values.get(token)) {
                out.push_str(value);
            } // else: unknown template token — drop it (safety net).
            rest = after.get(token_len + 1..).unwrap_or_default();
        } else {
            // Not a placeholder — keep the `{` literally and continue.
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

impl Default for ShellProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced CSS styling optimized for Writers & Publishers.
///
/// Typography-first design system with:
/// - 18px base font size for comfortable long-form reading
/// - 67ch optimal line length for sustained reading
/// - 1.7 line-height for enhanced readability
/// - Warm, paper-inspired color palette reducing eye strain
/// - CSS custom properties for maintainability
/// - Dark mode support with consistent color relationships
/// - Mobile-responsive design with appropriate font scaling
///
/// IMPORTANT: This CSS is embedded at compile time using include_str!
///
/// This embedded CSS is used for generating static sites, not for preview windows.
/// Preview windows load from the development server at localhost:8080, which serves
/// fresh CSS files from disk.
///
/// The embedded CSS ensures consistent styling in generated static sites across
/// different environments. For reliable rebuilds when CSS changes, see build.rs
/// which uses rerun-if-changed to track asset dependencies.
pub const DEFAULT_CSS: &str = include_str!("../../assets/css/site.css");

/// Minifies CSS by removing comments, collapsing whitespace, and removing
/// unnecessary characters. This is a simple, regex-free minifier that covers
/// the most impactful optimizations without needing a full CSS parser.
pub fn minify_css(css: &str) -> String {
    let mut result = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Pass 1: strip comments and collapse whitespace
    while i < len {
        // Strip /* ... */ comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        // Preserve quoted strings as-is
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            result.push(bytes[i] as char);
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < len {
                    result.push(bytes[i] as char);
                    i += 1;
                }
                result.push(bytes[i] as char);
                i += 1;
            }
            if i < len {
                result.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }

        // Collapse whitespace (newlines, tabs, multiple spaces) to a single space
        if bytes[i].is_ascii_whitespace() {
            // Skip all consecutive whitespace
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            // Only emit a space if needed (not after/before certain chars).
            // NOTE: ':' must NOT appear in the `next` set. When whitespace precedes
            // a colon the colon starts a pseudo-class (e.g. ".foo :hover"), and the
            // space is the descendant combinator — stripping it changes ".foo :hover"
            // (all :hover descendants of .foo) into ".foo:hover" (.foo itself when
            // hovered), which is a different set of elements entirely. Property-value
            // colons ("color: red") never have whitespace before them in well-formed
            // source CSS, so they are not affected by this rule.
            let last = result.as_bytes().last().copied().unwrap_or(0);
            let next = if i < len { bytes[i] } else { 0 };
            if !matches!(last, b'{' | b'}' | b';' | b':' | b',' | b'>' | b'(' | 0)
                && !matches!(next, b'{' | b'}' | b';' | b',' | b')' | 0)
            {
                result.push(' ');
            }
            continue;
        }

        result.push(bytes[i] as char);
        i += 1;
    }

    // Pass 2: remove last semicolons before } and spaces around certain chars
    let mut final_result = String::with_capacity(result.len());
    let rbytes = result.as_bytes();
    let rlen = rbytes.len();
    i = 0;
    while i < rlen {
        // Remove semicolons directly before }
        if rbytes[i] == b';' {
            // Look ahead past any whitespace to check for }
            let mut j = i + 1;
            while j < rlen && rbytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < rlen && rbytes[j] == b'}' {
                i += 1; // skip the semicolon
                continue;
            }
        }
        final_result.push(rbytes[i] as char);
        i += 1;
    }

    final_result
}
/// Shared HTML shell: full `<head>` + chrome (header/nav, hero, footer,
/// colophon, scripts) with a single `{main_content}` slot for the per-type
/// `<main>` content fragment. Spliced in `ShellProcessor::process`.
pub const SHELL_TEMPLATE: &str = include_str!("../../assets/templates/shell.html");

/// The small moss mark, for `shell.html`'s `{moss_mark}`.
///
/// The colophon ships in every page of every built site, so it inlines the
/// mark rather than linking it — and an inlined copy is a copy. Substituting
/// the file itself is what keeps the two from drifting; the launcher's copy,
/// which has no template to substitute into, is guarded by `mark_sync_test`.
///
/// The whole `<svg>` goes in, not the paths out of it. Unwrapping it used to
/// give the colophon's own class and `aria-hidden` somewhere to sit; a wrapper
/// in shell.html carries both instead, which costs one element and deletes a
/// tag-aware pass over markup — one that a two-tone mark had already made take
/// the element apart rather than lift a single `d` out of it.
const MOSS_MARK_SVG: &str = include_str!("../../../icons/mark-small.svg");
/// `<main>` content fragment for Article pages (centered article + post-article).
pub const ARTICLE_CONTENT: &str = include_str!("../../assets/templates/article-content.html");
/// `<main>` content fragment for Page pages (homepage / folders / nav pages).
pub const PAGE_CONTENT: &str = include_str!("../../assets/templates/page-content.html");
pub const DEFAULT_FAVICON: &str = include_str!("../../../icons/icon.svg");

// `pub(crate)` for one item: `tests::blanked`, the CSS reader the stylesheet
// partial-ownership gate also needs. It sits on this side because the
// dependency already runs this way — `stylesheet.rs` calls `minify_css` here,
// which is the crate's other reader of raw CSS text.
#[cfg(test)]
#[path = "shell_tests.rs"]
pub(crate) mod tests;
