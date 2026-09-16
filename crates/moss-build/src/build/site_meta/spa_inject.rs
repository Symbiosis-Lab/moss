//! Inject site-level meta defaults into a bundled SPA's `<head>` when those
//! tags are missing. The SPA always wins on conflicts — moss never overwrites
//! tags the SPA author already set.

use lol_html::html_content::ContentType;
use lol_html::{element, HtmlRewriter, Settings};

pub struct SpaDefaults<'a> {
    pub description: Option<&'a str>,
    pub canonical_url: Option<&'a str>,
    pub og_tags: Option<&'a str>,         // pre-rendered block (multiple <meta> lines)
    pub twitter_tags: Option<&'a str>,
    pub apple_touch_icon: Option<&'a str>,// pre-rendered <link>
    pub raster_favicons: Option<&'a str>,
    pub theme_color_light: Option<&'a str>,
    pub theme_color_dark: Option<&'a str>,
}

/// Owned variant of `SpaDefaults` for stashing on `BackgroundContext` until the
/// background asset phase reads it. moss builds this once per site from the
/// homepage frontmatter + the per-page-folder canonical URL, then borrows
/// from it via `as_borrowed()` when `inject_defaults` runs.
///
/// `canonical_url` is computed per SPA folder (since the URL path changes
/// per app), so it lives separately on the call site, not in this struct.
#[derive(Debug, Clone, Default)]
pub struct SpaDefaultsOwned {
    pub description: Option<String>,
    pub og_tags: Option<String>,
    pub twitter_tags: Option<String>,
    pub apple_touch_icon: Option<String>,
    pub raster_favicons: Option<String>,
    pub theme_color_light: Option<String>,
    pub theme_color_dark: Option<String>,
    /// Site host used to derive a per-folder canonical URL at injection time.
    /// e.g. `"www.example.com"`. None when the site has no configured domain.
    pub site_host: Option<String>,
    /// Whether this site's canonical host carries `www.` — true exactly when
    /// its CDN hostname is active (see
    /// `crate::build::site_url::ResolveInputs::cdn_active`). Forwarded to
    /// `crate::build::page::canonical::canonical_url`.
    pub site_www_canonical: bool,
}

impl SpaDefaultsOwned {
    /// Borrow as a `SpaDefaults` for a specific SPA's canonical URL.
    pub fn as_borrowed<'a>(&'a self, canonical_url: Option<&'a str>) -> SpaDefaults<'a> {
        SpaDefaults {
            description: self.description.as_deref(),
            canonical_url,
            og_tags: self.og_tags.as_deref(),
            twitter_tags: self.twitter_tags.as_deref(),
            apple_touch_icon: self.apple_touch_icon.as_deref(),
            raster_favicons: self.raster_favicons.as_deref(),
            theme_color_light: self.theme_color_light.as_deref(),
            theme_color_dark: self.theme_color_dark.as_deref(),
        }
    }

    /// True when at least one default would be injected (worth running).
    pub fn has_anything(&self) -> bool {
        self.description.is_some()
            || self.og_tags.is_some()
            || self.twitter_tags.is_some()
            || self.apple_touch_icon.is_some()
            || self.raster_favicons.is_some()
            || self.theme_color_light.is_some()
            || self.theme_color_dark.is_some()
            || self.site_host.is_some()
    }
}

/// Returns a new HTML string with missing tags injected into `<head>`.
/// Tags already present in the SPA's `<head>` are preserved unchanged.
///
/// When the input has no `<head>` element, the function returns the input
/// unchanged. This is rare for bundled SPAs (Vite always emits a head) and
/// the safe default for malformed input — lol_html's `element!("head", ...)`
/// handler simply does not match, so additions are silently dropped rather
/// than injected into an arbitrary location.
pub fn inject_defaults(html: &str, defaults: &SpaDefaults) -> Result<String, String> {
    // First pass: scan the SPA's existing <head> to find which tag names/properties
    // are already present.
    let present = scan_present_tags(html);

    // Build the injection block — only what's missing.
    let mut additions = String::new();
    if !present.has_meta("description") {
        if let Some(d) = defaults.description {
            additions.push_str(&format!(
                r#"<meta name="description" content="{}">"#,
                escape_attr(d)
            ));
            additions.push('\n');
        }
    }
    if !present.has_link("canonical") {
        if let Some(u) = defaults.canonical_url {
            additions.push_str(&format!(r#"<link rel="canonical" href="{}">"#, escape_attr(u)));
            additions.push('\n');
        }
    }
    if !present.has_og_image() {
        if let Some(b) = defaults.og_tags {
            additions.push_str(b);
            additions.push('\n');
        }
    }
    if !present.has_twitter() {
        if let Some(b) = defaults.twitter_tags {
            additions.push_str(b);
            additions.push('\n');
        }
    }
    if !present.has_link("apple-touch-icon") {
        if let Some(t) = defaults.apple_touch_icon {
            additions.push_str(t);
            additions.push('\n');
        }
    }
    if !present.has_link("icon") {
        if let Some(f) = defaults.raster_favicons {
            additions.push_str(f);
            additions.push('\n');
        }
    }
    if !present.has_meta("theme-color") {
        if let Some(c) = defaults.theme_color_light {
            additions.push_str(&format!(
                r#"<meta name="theme-color" content="{}" media="(prefers-color-scheme: light)">"#,
                escape_attr(c)
            ));
            additions.push('\n');
        }
        if let Some(c) = defaults.theme_color_dark {
            additions.push_str(&format!(
                r#"<meta name="theme-color" content="{}" media="(prefers-color-scheme: dark)">"#,
                escape_attr(c)
            ));
            additions.push('\n');
        }
    }

    if additions.is_empty() {
        return Ok(html.to_string());
    }

    let mut output = Vec::new();
    let additions_clone = additions.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("head", |el| {
                el.append(&additions_clone, ContentType::Html);
                Ok(())
            })],
            ..Settings::default()
        },
        |c: &[u8]| output.extend_from_slice(c),
    );
    rewriter.write(html.as_bytes()).map_err(|e| e.to_string())?;
    rewriter.end().map_err(|e| e.to_string())?;
    String::from_utf8(output).map_err(|e| e.to_string())
}

#[derive(Default)]
struct PresentTags {
    meta_names: Vec<String>,
    meta_properties: Vec<String>,
    link_rels: Vec<String>,
}

impl PresentTags {
    fn has_meta(&self, name: &str) -> bool {
        self.meta_names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
    fn has_link(&self, rel: &str) -> bool {
        self.link_rels.iter().any(|r| r.eq_ignore_ascii_case(rel))
    }
    fn has_og_image(&self) -> bool {
        self.meta_properties.iter().any(|p| p.eq_ignore_ascii_case("og:image"))
    }
    fn has_twitter(&self) -> bool {
        self.meta_names.iter().any(|n| n.to_ascii_lowercase().starts_with("twitter:"))
    }
}

fn scan_present_tags(html: &str) -> PresentTags {
    use std::cell::RefCell;
    let result = RefCell::new(PresentTags::default());

    let mut sink: Vec<u8> = Vec::new();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("meta", |el| {
                    let mut r = result.borrow_mut();
                    if let Some(n) = el.get_attribute("name") { r.meta_names.push(n); }
                    if let Some(p) = el.get_attribute("property") { r.meta_properties.push(p); }
                    Ok(())
                }),
                element!("link", |el| {
                    if let Some(r_attr) = el.get_attribute("rel") {
                        result.borrow_mut().link_rels.push(r_attr);
                    }
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |c: &[u8]| sink.extend_from_slice(c),
    );
    let _ = rewriter.write(html.as_bytes());
    let _ = rewriter.end();
    result.into_inner()
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> SpaDefaults<'static> {
        SpaDefaults {
            description: Some("Site default desc"),
            canonical_url: Some("https://www.example.com/app/"),
            og_tags: Some(r#"<meta property="og:image" content="/cover.png">"#),
            twitter_tags: Some(r#"<meta name="twitter:card" content="summary">"#),
            apple_touch_icon: Some(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#),
            raster_favicons: Some(r#"<link rel="icon" href="/favicon-32.png">"#),
            theme_color_light: Some("#fff"),
            theme_color_dark: Some("#000"),
        }
    }

    #[test]
    fn injects_into_empty_head() {
        let html = "<html><head><title>App</title></head><body></body></html>";
        let out = inject_defaults(html, &defaults()).unwrap();
        assert!(out.contains(r#"<meta name="description" content="Site default desc">"#));
        assert!(out.contains("og:image"));
        assert!(out.contains("twitter:card"));
        assert!(out.contains("apple-touch-icon"));
        assert!(out.contains("rel=\"icon\""));
        assert!(out.contains("link rel=\"canonical\""));
    }

    #[test]
    fn does_not_overwrite_existing_description() {
        let html = r#"<html><head><title>App</title><meta name="description" content="SPA's own"></head><body></body></html>"#;
        let out = inject_defaults(html, &defaults()).unwrap();
        assert!(out.contains(r#"content="SPA's own""#));
        // Should not contain our default
        assert!(!out.contains("Site default desc"));
    }

    #[test]
    fn does_not_overwrite_existing_og_image() {
        let html = r#"<html><head><title>App</title><meta property="og:image" content="/spa.jpg"></head><body></body></html>"#;
        let out = inject_defaults(html, &defaults()).unwrap();
        assert!(out.contains(r#"content="/spa.jpg""#));
        assert!(!out.contains("/cover.png"));
    }

    #[test]
    fn no_op_when_all_present() {
        let html = r#"<html><head>
            <title>App</title>
            <meta name="description" content="x">
            <meta property="og:image" content="x">
            <meta name="twitter:card" content="x">
            <link rel="apple-touch-icon" href="x">
            <link rel="icon" href="x">
            <link rel="canonical" href="x">
            <meta name="theme-color" content="x">
        </head><body></body></html>"#;
        let out = inject_defaults(html, &defaults()).unwrap();
        // Output should not have grown by injection
        assert_eq!(out.matches("apple-touch-icon").count(), 1);
        assert_eq!(out.matches("og:image").count(), 1);
    }

    #[test]
    fn preserves_spa_title() {
        let html = r#"<html><head><title>SPA Title</title></head><body></body></html>"#;
        let out = inject_defaults(html, &defaults()).unwrap();
        assert!(out.contains("<title>SPA Title</title>"));
    }

    #[test]
    fn no_head_html_passes_through_unchanged() {
        let html = "<html><body><h1>No head</h1></body></html>";
        let out = inject_defaults(html, &defaults()).unwrap();
        // lol_html's element!("head", ...) won't match — additions silently dropped.
        // Document this as intentional: a malformed/headless HTML file is rare for
        // bundled SPAs and the safe behavior is to leave it untouched.
        assert_eq!(out, html, "no-head HTML should pass through unchanged");
    }
}
