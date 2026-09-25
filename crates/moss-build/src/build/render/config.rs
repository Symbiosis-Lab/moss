//! Site-level configuration struct and HTML attribute resolvers shared by blocking and html modules.

use crate::build::incremental_gates::IncrementalGates;

/// Resolve a logo path from frontmatter to a root-relative URL.
/// Absolute paths and HTTP URLs pass through unchanged.
pub(super) fn resolve_logo_url(path: &str) -> String {
    if path.starts_with('/') || path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("/{}", path) // allow:served-path-url-construct (user logo path from [site].logo frontmatter, not a framework asset)
    }
}


/// Bundled into a single struct to keep `generate_blocking_content`'s signature clean.
pub struct SiteConfig {
    /// The site's RESOLVED language code, not the raw `[site] lang` — the
    /// ladder (`[site] lang` → homepage `lang:` → detection → system language)
    /// runs once at the construction site, the same way `implicit_figure` and
    /// `math` resolve their defaults there. Consumers do not re-decide.
    ///
    /// A BCP-47 code, so a site declaring one moss ships no UI for keeps it;
    /// the render path maps it to a `Language` and falls back to English
    /// itself, where the fallback means "no translation table".
    pub lang: String,
    pub typesetting: Option<String>,
    pub content_width: Option<String>,
    pub comments: Option<bool>,
    /// `[site].implicit_figure` — when true, an image alone in a paragraph
    /// with non-empty alt renders as `<figure>` with the alt as caption
    /// (Pandoc-style implicit figure). Resolved at the construction site:
    /// pre-existing sites get `false` from the v2→v3 config migration; new
    /// sites (no config.toml yet) default to `true`. Stored as a resolved
    /// bool so consumers don't have to re-decide the default.
    pub implicit_figure: bool,
    /// `[site].math` — when true, `$…$` and `$$…$$` are parsed as LaTeX
    /// equations. Default ON: an absent key resolves to `true`
    /// at the construction site, so every site gets math without writing
    /// any config.
    ///
    /// Default-on is safe because enabling it changes what a *character*
    /// means, and the empirical check found zero `$` outside fenced code
    /// (which pulldown never parses as math) in any existing vault. An
    /// author who writes prose where `$` pairs up — unspaced CJK price
    /// lists like `一个$5，两个$10` are the known false positive — turns
    /// it off with `[site].math = false`.
    ///
    /// Stored resolved, like `implicit_figure`, so consumers never
    /// re-decide the default.
    pub math: bool,
    /// `[site].hard_line_breaks` — when true, a single newline inside a
    /// paragraph renders as `<br>`, matching Obsidian's default
    /// (`strictLineBreaks = false`). Default ON: moss's compatibility target
    /// is Obsidian, and this is the one place ordinary two-line paragraphs
    /// diverged. Measured before flipping (mosspub.com, 2,347 pages): 4.8%
    /// of pages gain a break, concentrated in one demo site; poetry sites —
    /// the content most sensitive to breaks — measured zero because their
    /// authors already write explicit breaks. Authors who want CommonMark's
    /// "newline is a space" write `[site].hard_line_breaks = false`.
    ///
    /// Stored resolved, like `math`, so consumers never re-decide the default.
    pub hard_line_breaks: bool,
    /// `[site].link_preview` — when true, hovering a wikilink shows a
    /// preview card (title/excerpt/cover from `_moss/previews.json`).
    /// Default ON: an absent key resolves to `true` at the construction
    /// site. Authors who don't want the popup write
    /// `[site].link_preview = false`.
    ///
    /// Stored resolved, like `math`, so consumers never re-decide the default.
    pub link_preview: bool,
    /// `[site].heading_anchors` — when true, headings get a trailing `#`
    /// permalink anchor for deep-linking. Default ON: an absent key
    /// resolves to `true` at the construction site. Authors who don't want
    /// the anchor write `[site].heading_anchors = false`.
    ///
    /// Stored resolved, like `math`, so consumers never re-decide the default.
    pub heading_anchors: bool,
    /// `[site].floating_nav` — the floating nav island. Default OFF
    /// since 2026-08-30: an absent key resolves to `false` at the construction
    /// site, so a site gets no floating nav until it asks for one. Authors who
    /// want it write `[site].floating_nav = true` (or flip the Services-tab
    /// toggle, which writes exactly that key).
    ///
    /// Stored resolved, like `math`, so consumers never re-decide the default.
    pub floating_nav: bool,
    /// CLI `--site-url` override. When set, `resolve_site_url` will use this
    /// value instead of deriving the URL from `.moss/state.toml`. `None` means
    /// "derive from deployment state as usual". Consumed by Task 2.5.
    pub site_url_override: Option<String>,
    /// `[site].ai_policy` — site-level AI crawler policy that drives the
    /// generated robots.txt. One of `"standard"` (default), `"unrestricted"`,
    /// or `"restricted"`. `None` falls back to the standard policy.
    pub ai_policy: Option<String>,
    /// `[site].search` — full-text search, resolved at the construction site
    /// in `build/pipeline.rs`: `true` when the author turned the Services-tab
    /// toggle on. By design, the gate is expressed here in the
    /// per-invocation config, not as a side-channel read inside the pipeline.
    ///
    /// Note this is still not the final "is search live" answer — the render
    /// phase ANDs it with `site_url.is_deployed()` (the same condition RSS
    /// uses) so preview/dev builds neither index nor show the nav button.
    pub search: bool,
    /// The kinds table `build::terms::term_kinds` resolves from
    /// `.moss/config.toml`'s `[terms]` section — every name-list field's
    /// namespace, built-in (`authors`, `tags`) and declared alike. The one
    /// thing `build::terms::derive_terms` reads to decide term membership.
    pub term_kinds: Vec<crate::build::terms::TermKind>,
    /// Decoded geography, gazetteer, place namespace and locator preference.
    pub place_maps: Option<crate::build::place_map::PlaceMapRenderContext>,
    /// What this build may reuse from the last one. Both bits are
    /// resolved at the entry point (by design, the render phase reads neither the
    /// build trigger nor the environment). See [`IncrementalGates`].
    pub incremental: IncrementalGates,
}

impl SiteConfig {
    /// The `[site]` answers `process_markdown_file` reads, as the one value
    /// both it and the parse cache's fingerprint take.
    pub fn markdown(&self) -> crate::build::markdown::SiteMarkdown<'_> {
        crate::build::markdown::SiteMarkdown {
            implicit_figure: self.implicit_figure,
            math: self.math,
            hard_line_breaks: self.hard_line_breaks,
            heading_anchors: self.heading_anchors,
            typesetting: self.typesetting.as_deref(),
        }
    }
}

/// Hand-written rather than derived for one field: `#[derive(Default)]` gives
/// `lang: ""`, which is not a language code — it reaches `<html lang="">` and
/// the site-languages artifact as an invalid value that nothing rejects.
/// English is what "no site language resolved" means everywhere else here.
impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            lang: "en".to_string(),
            // Every other field keeps the derived zero it has always had —
            // written out rather than `..Default::default()` (which cannot
            // recurse) so a new field is a compile error here, not a silent
            // false. The `true` defaults these docs describe are resolved at
            // the construction site, never here.
            typesetting: None,
            content_width: None,
            comments: None,
            implicit_figure: false,
            math: false,
            hard_line_breaks: false,
            link_preview: false,
            heading_anchors: false,
            floating_nav: false,
            site_url_override: None,
            ai_policy: None,
            search: false,
            term_kinds: Vec::new(),
            place_maps: None,
            incremental: IncrementalGates::default(),
        }
    }
}

/// A page's effective typesetting: its own `typesetting:` if it set one, else
/// `[site].typesetting`. The one owner of that precedence — the page shell,
/// the stylesheet's vertical partial and a body image's `sizes=` (through
/// [`crate::build::markdown::SiteMarkdown`]) must agree on which pages are
/// vertical, and three hand-written copies of this line were how they could
/// drift apart.
pub(crate) fn effective_typesetting<'a>(page: Option<&'a str>, site: Option<&'a str>) -> Option<&'a str> {
    page.or(site)
}

/// `suppress` omits the attribute when the resolved value equals the default (e.g. "horizontal").
pub(super) fn resolve_data_attr(
    page_value: Option<&String>,
    site_default: Option<&String>,
    attr_name: &str,
    suppress: Option<&str>,
) -> String {
    page_value
        .or(site_default)
        .filter(|v| suppress.map_or(true, |d| v.as_str() != d))
        // Escaped, because this is the one `data-` attribute whose value is
        // raw author input. `content_width`/`typesetting` are filtered to a
        // known set on the CASCADE path (scan/cascade.rs) but not on the
        // page-frontmatter path (markdown/pipeline.rs) nor the `[site]` path
        // (build/pipeline.rs's `site_str`), so `full" onload="…` reached
        // `<body>` verbatim. One boundary, enforced once, at the emission site
        // every path funnels through.
        .map(|v| {
            format!(r#" data-{}="{}""#, attr_name, crate::build::page::meta::escape_html_attr(v))
        })
        .unwrap_or_default()
}


/// The one ladder for "does this page prefer comments on or off": an explicit
/// page-level value always wins; absent that, the site-wide default decides.
/// `None` means neither layer expressed a preference — callers supply their
/// own fallback (`generate_native_slots` uses a per-page-kind default;
/// `resolve_comments_attr` below leaves the attribute off).
///
/// `pub(crate)` because both the `[site] comments` HTML attribute (this
/// module) and the native comments-section injector
/// (`build::features::generate_native_slots`) must agree on the same
/// resolution — the two used to diverge, which is how `[site] comments =
/// false` left the section rendered while only the JS attribute went false.
pub(crate) fn resolve_comments_pref(page_value: Option<bool>, site_default: Option<bool>) -> Option<bool> {
    page_value.or(site_default)
}

/// None = no preference (follow plugin default), Some(true) = opt-in, Some(false) = opt-out.
pub(super) fn resolve_comments_attr(page_value: Option<bool>, site_default: Option<bool>) -> String {
    match resolve_comments_pref(page_value, site_default) {
        Some(false) => r#" data-comments="false""#.to_string(),
        Some(true) => r#" data-comments="true""#.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_data_attr;

    /// `content_width` is filtered to a known set on the cascade path but not
    /// on the page-frontmatter or `[site]` paths, so the value arriving here
    /// can be anything the author typed.
    #[test]
    fn an_author_value_cannot_open_a_second_attribute() {
        let hostile = r#"full" onload="alert(1)"#.to_string();
        let out = resolve_data_attr(Some(&hostile), None, "content-width", None);
        assert!(!out.contains(r#"onload="alert(1)"#), "{out}");
        assert_eq!(out, r#" data-content-width="full&quot; onload=&quot;alert(1)""#);
    }

    #[test]
    fn an_ordinary_value_is_unchanged() {
        let wide = "wide".to_string();
        assert_eq!(
            resolve_data_attr(Some(&wide), None, "content-width", None),
            r#" data-content-width="wide""#
        );
    }
}
