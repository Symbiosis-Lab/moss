//! Layout configuration module for moss static site generator
//!
//! Provides site name resolution from homepage title or folder name.

use crate::build::types::SiteAssets;

/// Layout configuration for rendering decisions
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Site name (derived from homepage title or folder name)
    pub site_name: String,
    /// Path to site logo relative to output root (e.g., "/assets/logo.svg")
    pub logo_path: Option<String>,
    /// Typesetting direction from site config: "horizontal" (default) or "vertical"
    pub typesetting: Option<String>,
    /// Content width preset from site config: "wide" or "full"
    pub content_width: Option<String>,
    /// Site-wide comments preference: true = opt-in, false = opt-out, None = plugin default
    pub comments: Option<bool>,
    /// The site-level facts that gate optional assets — which runtime
    /// scripts get a `<script>` tag, which CSS partials ship. One embedded
    /// [`SiteAssets`] rather than a private copy of five of its bools: the
    /// render paths used to read `LayoutConfig::math` while the emitter read
    /// `SiteAssets::math`, two names for one fact that could disagree.
    /// `with_math`/`with_search`/`with_link_preview`/
    /// `with_heading_anchors` write here; `with_assets` replaces the whole set
    /// once the page fold and the asset registry know the rest.
    ///
    /// Every gate in here is SITE-level, never per page: the preview's
    /// morph-guard forces a full reload when the script set differs across a
    /// navigation, so a tag that came and went per page would turn every such
    /// navigation into a reload.
    pub assets: SiteAssets,
    /// `[site].floating_nav` — the floating nav island (default OFF
    /// since 2026-08-30, explicit opt-in). Site-level for the same reason as
    /// `math`: the island is chrome, present on every breadcrumbed page or none.
    pub floating_nav: bool,
    /// The SITE's declared language as a BCP-47 tag — what `<html lang>` says
    /// on a page that declares none of its own.
    ///
    /// A tag, not a [`Language`]: moss draws its interface in three languages
    /// and `lang` describes the content, so `[site] lang = "fr"`
    /// belongs here in full even though the chrome falls back to English.
    /// Before 2026-09-01 the render path only had the three-variant enum, so
    /// a French site's artifact said `fr` while its pages said `en`.
    pub lang_tag: String,
    pub place_maps: Option<crate::build::place_map::PlaceMapRenderContext>,
}

impl LayoutConfig {
    /// Create layout configuration.
    ///
    /// # Site Name Priority
    /// 1. `homepage_title` from index.md frontmatter (if non-empty)
    /// 2. `folder_name` as fallback
    pub fn new(folder_name: &str, homepage_title: Option<&str>) -> Self {
        let site_name = homepage_title
            .filter(|t| !t.is_empty())
            .unwrap_or(folder_name)
            .to_string();
        Self {
            site_name,
            logo_path: None,
            typesetting: None,
            content_width: None,
            comments: None,
            assets: SiteAssets {
                math: true,
                search: false,
                link_preview: true,
                heading_anchors: true,
                ..SiteAssets::default()
            },
            floating_nav: false,
            lang_tag: "en".to_string(),
            place_maps: None,
        }
    }

    /// Set the site's declared language tag (from the resolved site language).
    pub fn with_lang_tag(mut self, lang_tag: String) -> Self {
        self.lang_tag = lang_tag;
        self
    }

    /// Set the logo path (from homepage frontmatter `logo` field)
    pub fn with_logo(mut self, logo_path: String) -> Self {
        self.logo_path = Some(logo_path);
        self
    }

    /// Set the typesetting direction (from site config)
    pub fn with_typesetting(mut self, typesetting: String) -> Self {
        self.typesetting = Some(typesetting);
        self
    }

    /// Set the content width preset (from site config)
    pub fn with_content_width(mut self, content_width: String) -> Self {
        self.content_width = Some(content_width);
        self
    }

    /// Set the site-wide comments preference (from site config)
    pub fn with_comments(mut self, comments: bool) -> Self {
        self.comments = Some(comments);
        self
    }

    /// Set the site-wide math preference (`[site].math`, default true)
    pub fn with_math(mut self, math: bool) -> Self {
        self.assets.math = math;
        self
    }

    /// Set the resolved search gate — `[site].search` as resolved at the
    /// `SiteConfig` construction site. It must be the SAME value the index
    /// emitter is gated on: the nav search button reads it too, so a
    /// divergent value points readers at a bundle that was never written
    /// (a dead button on every page).
    pub fn with_search(mut self, search: bool) -> Self {
        self.assets.search = search;
        self
    }

    /// Set the site-wide wikilink hover-preview preference (`[site].link_preview`, default true)
    pub fn with_link_preview(mut self, link_preview: bool) -> Self {
        self.assets.link_preview = link_preview;
        self
    }

    /// Set the site-wide heading-anchor preference (`[site].heading_anchors`, default true)
    pub fn with_heading_anchors(mut self, heading_anchors: bool) -> Self {
        self.assets.heading_anchors = heading_anchors;
        self
    }

    /// Set the site-wide floating-nav preference (`[site].floating_nav`, default false)
    pub fn with_floating_nav(mut self, floating_nav: bool) -> Self {
        self.floating_nav = floating_nav;
        self
    }

    /// Replace the whole gate set with the build's folded [`SiteAssets`].
    /// Called once, after the page fold and the asset registry know the
    /// content-derived facts (`has_footnotes`, `media_pages`, `video_ladder`);
    /// the per-flag builders above are what the config phase — and tests —
    /// use before that point.
    pub fn with_assets(mut self, assets: SiteAssets) -> Self {
        self.assets = assets;
        self
    }

    pub fn with_place_maps(mut self, place_maps: Option<crate::build::place_map::PlaceMapRenderContext>) -> Self {
        self.place_maps = place_maps;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_site_name_uses_homepage_title() {
        let config = LayoutConfig::new("my-blog", Some("山居"));
        assert_eq!(config.site_name, "山居");
    }

    #[test]
    fn test_site_name_fallback_to_folder() {
        let config = LayoutConfig::new("fallback-name", None);
        assert_eq!(config.site_name, "fallback-name");
    }

    #[test]
    fn test_site_name_fallback_when_empty() {
        let config = LayoutConfig::new("fallback-name", Some(""));
        assert_eq!(config.site_name, "fallback-name");
    }

    #[test]
    fn floating_nav_is_opt_in() {
        // As amended 2026-08-30: a second navigation bar is an
        // addition to someone's site, so it is asked for rather than
        // inherited — and `render/html.rs` emits the island only when this is
        // true, which makes this line the whole of "no island by default".
        assert!(!LayoutConfig::new("any-site", None).floating_nav);
        assert!(LayoutConfig::new("any-site", None).with_floating_nav(true).floating_nav);
    }
}
