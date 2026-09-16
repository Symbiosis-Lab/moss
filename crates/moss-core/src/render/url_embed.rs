//! URL embed synthesizer — provider-aware `<iframe>` for external URLs.
//!
//! `![[https://...]]` wikilink embeds are routed here. `detect_provider`
//! matches the URL against known platforms (YouTube, Vimeo, CodePen) and
//! returns a [`ProviderEmbed`] carrying the canonical embed URL plus the
//! correct `allow`/`sandbox`/`allowfullscreen` attributes. Unknown URLs
//! fall through to a generic passthrough.
//!
//! `synthesize_url_embed_html` combines the provider result with the
//! pothole params (sizing, alias) and delegates final HTML synthesis to
//! [`crate::render::iframe::synthesize_iframe_html`].

use crate::asset_snapshot::AssetSnapshot;
use crate::render::iframe::synthesize_iframe_html;
use crate::resolve::embed_renderer::Sizing;
use crate::resolve::title_params::TitleParams;
use crate::resolve::wikilink_dispatch::PotholeContent;

/// Provider-specific iframe configuration for a matched URL.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEmbed {
    /// Canonical embed URL (may differ from the input URL).
    pub embed_url: String,
    /// Value for the `allow=` attribute. Empty string → attribute omitted.
    pub allow: &'static str,
    /// Value for the `sandbox=` attribute. Empty string → attribute omitted.
    pub sandbox: &'static str,
    /// Whether to emit the boolean `allowfullscreen` attribute.
    pub allowfullscreen: bool,
    /// Lowercase provider name emitted as `data-provider="…"`. Empty → omitted.
    pub provider_name: &'static str,
}

/// Detect which provider (if any) serves this URL and return the appropriate
/// [`ProviderEmbed`] configuration.
///
/// Matched providers: `youtube`, `vimeo`, `codepen`.
/// Everything else returns a generic passthrough with empty policy strings.
pub fn detect_provider(url: &str) -> ProviderEmbed {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    let (host, path_and_query) = match after_scheme
        .find('/')
        .and_then(|i| after_scheme.split_at_checked(i))
    {
        Some(split) => split,
        None => (after_scheme, ""),
    };

    let bare_host = host.strip_prefix("www.").unwrap_or(host);

    // YouTube
    if bare_host == "youtube.com" || bare_host == "youtu.be" {
        if let Some((id, start)) = extract_youtube_id(bare_host, path_and_query) {
            let embed_url = match start {
                Some(t) => format!("https://www.youtube.com/embed/{}?start={}", id, t),
                None => format!("https://www.youtube.com/embed/{}", id),
            };
            return ProviderEmbed {
                embed_url,
                allow: "accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share",
                sandbox: "",
                allowfullscreen: true,
                provider_name: "youtube",
            };
        }
    }

    // Vimeo
    if bare_host == "vimeo.com" || bare_host == "player.vimeo.com" {
        if let Some(embed_url) = extract_vimeo_embed_url(bare_host, path_and_query) {
            return ProviderEmbed {
                embed_url,
                allow: "autoplay; fullscreen; picture-in-picture",
                sandbox: "",
                allowfullscreen: true,
                provider_name: "vimeo",
            };
        }
    }

    // CodePen
    if bare_host == "codepen.io" {
        if let Some(embed_url) = extract_codepen_embed_url(path_and_query) {
            return ProviderEmbed {
                embed_url,
                allow: "clipboard-read; clipboard-write",
                sandbox: "allow-scripts allow-same-origin allow-forms allow-modals allow-popups allow-presentation",
                allowfullscreen: true,
                provider_name: "codepen",
            };
        }
    }

    // Generic fallback
    ProviderEmbed {
        embed_url: url.to_string(),
        allow: "",
        sandbox: "",
        allowfullscreen: false,
        provider_name: "",
    }
}

/// Synthesize `<iframe>` HTML for an external URL embed.
///
/// 1. Detects the provider and rewrites the URL to the canonical embed form.
/// 2. Merges pothole sizing/alias into `TitleParams`.
/// 3. Delegates to [`synthesize_iframe_html`].
/// 4. Injects `data-provider="…"` for known providers.
pub fn synthesize_url_embed_html(
    url: &str,
    pothole: &PotholeContent,
    assets: &AssetSnapshot,
) -> String {
    let provider = detect_provider(url);
    let mut params = TitleParams::default();

    match pothole {
        PotholeContent::Empty => {}
        PotholeContent::WidthToken { width, rest_alias } => {
            params.insert("data-width", *width);
            if !rest_alias.is_empty() {
                apply_alias_to_params(rest_alias.as_str(), &mut params);
            }
        }
        PotholeContent::Alias(alias) => {
            apply_alias_to_params(alias.as_str(), &mut params);
        }
        PotholeContent::Params(kv) => {
            for (k, v) in &kv.params {
                params.insert(k.clone(), v.clone());
            }
        }
    }

    if !provider.allow.is_empty() {
        params.insert("allow", provider.allow);
    }
    if !provider.sandbox.is_empty() {
        params.insert("sandbox", provider.sandbox);
    }
    if provider.allowfullscreen {
        params.insert("allowfullscreen", "true");
    }
    if !provider.provider_name.is_empty() {
        params.insert("data-provider", provider.provider_name);
    }

    synthesize_iframe_html(&params, &provider.embed_url, assets)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn apply_alias_to_params(alias: &str, params: &mut TitleParams) {
    match Sizing::parse(alias) {
        Some(Sizing::Width(w)) => {
            params.insert("width", w.to_css());
        }
        Some(Sizing::Box(w, h)) => {
            params.insert("width", w.to_css());
            params.insert("height", h.to_css());
        }
        None => {
            params.insert("title", alias);
        }
    }
}

/// Find the value of `key` in a `&`-separated query string (e.g. `"v=abc&t=30"`).
fn find_query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for param in query.split('&') {
        if let Some(val) = param.strip_prefix(key) {
            if let Some(val) = val.strip_prefix('=') {
                return Some(val);
            }
        }
    }
    None
}

/// Parse a YouTube timestamp (`t=` or `start=`) from a query string.
/// Strips a trailing `s` suffix (e.g. `30s` → `"30"`). Returns `None` if
/// not present or not purely numeric after stripping.
fn parse_youtube_timestamp(query: &str) -> Option<String> {
    let raw = find_query_param(query, "t")
        .or_else(|| find_query_param(query, "start"))?;
    let digits = raw.strip_suffix('s').unwrap_or(raw);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

fn extract_youtube_id(host: &str, path_and_query: &str) -> Option<(String, Option<String>)> {
    if host == "youtu.be" {
        let trimmed = path_and_query.trim_start_matches('/');
        let id = trimmed.split(['/', '?', '#']).next()?;
        let id = validate_youtube_id(id)?.to_string();
        let start = path_and_query
            .split_once('?')
            .and_then(|(_, query)| parse_youtube_timestamp(query));
        return Some((id, start));
    }

    if let Some(rest) = path_and_query.strip_prefix("/embed/") {
        let id = rest.split(['/', '?', '#']).next()?;
        let id = validate_youtube_id(id)?.to_string();
        let start = rest
            .split_once('?')
            .and_then(|(_, query)| parse_youtube_timestamp(query));
        return Some((id, start));
    }

    if let Some(rest) = path_and_query.strip_prefix("/shorts/") {
        let id = rest.split(['/', '?', '#']).next()?;
        let id = validate_youtube_id(id)?.to_string();
        let start = rest
            .split_once('?')
            .and_then(|(_, query)| parse_youtube_timestamp(query));
        return Some((id, start));
    }

    if let Some(rest) = path_and_query.strip_prefix("/live/") {
        let id = rest.split(['/', '?', '#']).next()?;
        let id = validate_youtube_id(id)?.to_string();
        let start = rest
            .split_once('?')
            .and_then(|(_, query)| parse_youtube_timestamp(query));
        return Some((id, start));
    }

    if path_and_query.starts_with("/watch") {
        let (_, query) = path_and_query.split_once('?')?;
        for param in query.split('&') {
            if let Some(id) = param.strip_prefix("v=") {
                let id = id.split(['&', '#']).next().unwrap_or(id);
                if let Some(id) = validate_youtube_id(id) {
                    let start = parse_youtube_timestamp(query);
                    return Some((id.to_string(), start));
                }
            }
        }
    }

    None
}

fn validate_youtube_id(id: &str) -> Option<&str> {
    if id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        Some(id)
    } else {
        None
    }
}

fn extract_vimeo_embed_url(bare_host: &str, path_and_query: &str) -> Option<String> {
    if bare_host == "player.vimeo.com" {
        return Some(format!("https://player.vimeo.com{}", path_and_query));
    }

    // `split_at_checked`, not `split_once`: the query half keeps its leading '?'.
    let (path, query) = match path_and_query
        .find('?')
        .and_then(|i| path_and_query.split_at_checked(i))
    {
        Some((path, query)) => (path, Some(query)),
        None => (path_and_query, None),
    };
    let mut segments = path.trim_start_matches('/').split('/');

    let id_str = segments.next()?;
    if id_str.is_empty() || !id_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let hash = segments
        .next()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()));

    Some(match hash {
        Some(h) => format!("https://player.vimeo.com/video/{}?h={}", id_str, h),
        None => match query {
            Some(q) => format!("https://player.vimeo.com/video/{}{}", id_str, q),
            None => format!("https://player.vimeo.com/video/{}", id_str),
        },
    })
}

fn extract_codepen_embed_url(path_and_query: &str) -> Option<String> {
    let path = path_and_query.split('?').next().unwrap_or(path_and_query);
    let trimmed = path.trim_start_matches('/');
    let mut parts = trimmed.splitn(3, '/');
    let user = parts.next()?;
    let kind = parts.next()?;
    let slug = parts
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");

    if slug.is_empty() {
        return None;
    }

    match kind {
        "pen" | "embed" => Some(format!("https://codepen.io/{}/embed/{}", user, slug)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot() -> AssetSnapshot {
        AssetSnapshot::new()
    }

    // YouTube
    #[test]
    fn detect_provider_youtube_watch() {
        let p = detect_provider("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert!(p.allow.contains("encrypted-media"), "allow: {}", p.allow);
        assert!(p.allowfullscreen);
        assert_eq!(p.provider_name, "youtube");
        assert_eq!(p.sandbox, "");
    }

    #[test]
    fn detect_provider_youtube_shortlink() {
        let p = detect_provider("https://youtu.be/dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.provider_name, "youtube");
    }

    #[test]
    fn detect_provider_youtube_shorts() {
        let p = detect_provider("https://www.youtube.com/shorts/dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.provider_name, "youtube");
    }

    #[test]
    fn detect_provider_youtube_live() {
        let p = detect_provider("https://www.youtube.com/live/dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.provider_name, "youtube");
    }

    #[test]
    fn detect_provider_youtube_already_embed() {
        let p = detect_provider("https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.provider_name, "youtube");
    }

    #[test]
    fn detect_provider_youtube_no_www() {
        let p = detect_provider("https://youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(p.embed_url, "https://www.youtube.com/embed/dQw4w9WgXcQ");
        assert_eq!(p.provider_name, "youtube");
    }

    // Vimeo
    #[test]
    fn detect_provider_vimeo_standard() {
        let p = detect_provider("https://vimeo.com/123456789");
        assert_eq!(p.embed_url, "https://player.vimeo.com/video/123456789");
        assert!(p.allow.contains("fullscreen"), "allow: {}", p.allow);
        assert!(p.allowfullscreen);
        assert_eq!(p.provider_name, "vimeo");
        assert_eq!(p.sandbox, "");
    }

    #[test]
    fn detect_provider_vimeo_unlisted() {
        let p = detect_provider("https://vimeo.com/123456789/abc123def");
        assert_eq!(
            p.embed_url,
            "https://player.vimeo.com/video/123456789?h=abc123def"
        );
        assert_eq!(p.provider_name, "vimeo");
    }

    #[test]
    fn detect_provider_vimeo_player_passthrough() {
        let p = detect_provider("https://player.vimeo.com/video/123456789");
        assert_eq!(p.embed_url, "https://player.vimeo.com/video/123456789");
        assert_eq!(p.provider_name, "vimeo");
    }

    // CodePen
    #[test]
    fn detect_provider_codepen_pen_to_embed() {
        let p = detect_provider("https://codepen.io/someuser/pen/abcDEF");
        assert_eq!(p.embed_url, "https://codepen.io/someuser/embed/abcDEF");
        assert!(
            p.sandbox.contains("allow-scripts"),
            "sandbox: {}",
            p.sandbox
        );
        assert!(
            p.allow.contains("clipboard-write"),
            "allow: {}",
            p.allow
        );
        assert!(p.allowfullscreen);
        assert_eq!(p.provider_name, "codepen");
    }

    #[test]
    fn detect_provider_codepen_embed_passthrough() {
        let p = detect_provider("https://codepen.io/someuser/embed/abcDEF");
        assert_eq!(p.embed_url, "https://codepen.io/someuser/embed/abcDEF");
        assert_eq!(p.provider_name, "codepen");
    }

    // Generic fallback
    #[test]
    fn detect_provider_generic_https() {
        let p = detect_provider("https://example.com/page");
        assert_eq!(p.embed_url, "https://example.com/page");
        assert_eq!(p.allow, "");
        assert_eq!(p.sandbox, "");
        assert!(!p.allowfullscreen);
        assert_eq!(p.provider_name, "");
    }

    #[test]
    fn detect_provider_generic_http() {
        let p = detect_provider("http://example.com/page");
        assert_eq!(p.embed_url, "http://example.com/page");
        assert_eq!(p.provider_name, "");
    }

    #[test]
    fn detect_provider_malformed_url() {
        let p = detect_provider("https://");
        assert_eq!(p.embed_url, "https://");
        assert_eq!(p.provider_name, "");
    }

    // synthesize_url_embed_html
    #[test]
    fn synthesize_url_embed_full_youtube_shape() {
        let out = synthesize_url_embed_html(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            &PotholeContent::Empty,
            &empty_snapshot(),
        );
        assert!(
            out.contains(r#"src="https://www.youtube.com/embed/dQw4w9WgXcQ""#),
            "got: {out}"
        );
        assert!(out.contains(r#"class="moss-embed""#), "got: {out}");
        assert!(out.contains(r#"data-type="iframe""#), "got: {out}");
        assert!(out.contains(r#"data-provider="youtube""#), "got: {out}");
        assert!(out.contains("allowfullscreen"), "got: {out}");
        assert!(out.contains("allow="), "got: {out}");
    }

    #[test]
    fn synthesize_url_embed_generic_no_provider_attr() {
        let out = synthesize_url_embed_html(
            "https://example.com/embed",
            &PotholeContent::Empty,
            &empty_snapshot(),
        );
        assert!(
            out.contains(r#"src="https://example.com/embed""#),
            "got: {out}"
        );
        assert!(
            !out.contains("data-provider="),
            "generic should have no data-provider, got: {out}"
        );
        assert!(
            !out.contains("allow="),
            "generic should have no allow, got: {out}"
        );
        assert!(
            !out.contains("allowfullscreen"),
            "generic should have no allowfullscreen, got: {out}"
        );
    }

    #[test]
    fn synthesize_url_embed_pothole_width_token() {
        let out = synthesize_url_embed_html(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            &PotholeContent::WidthToken {
                width: "wide",
                rest_alias: String::new(),
            },
            &empty_snapshot(),
        );
        assert!(out.contains(r#"data-width="wide""#), "got: {out}");
    }

    #[test]
    fn synthesize_url_embed_pothole_sizing() {
        use crate::resolve::wikilink_dispatch::parse_pothole_params;
        let pothole = parse_pothole_params("640x360");
        let out = synthesize_url_embed_html(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            &pothole,
            &empty_snapshot(),
        );
        assert!(out.contains(r#"width="640px""#), "got: {out}");
        assert!(out.contains(r#"height="360px""#), "got: {out}");
    }

    #[test]
    fn synthesize_url_embed_pothole_alias_becomes_title() {
        let out = synthesize_url_embed_html(
            "https://vimeo.com/123456789",
            &PotholeContent::Alias("My video".to_string()),
            &empty_snapshot(),
        );
        assert!(out.contains(r#"title="My video""#), "got: {out}");
    }

    #[test]
    fn synthesize_url_embed_codepen_has_sandbox() {
        let out = synthesize_url_embed_html(
            "https://codepen.io/user/pen/abc",
            &PotholeContent::Empty,
            &empty_snapshot(),
        );
        assert!(out.contains("sandbox="), "got: {out}");
        assert!(out.contains("allow-scripts"), "got: {out}");
        assert!(out.contains(r#"data-provider="codepen""#), "got: {out}");
    }

    #[test]
    fn detect_provider_youtube_watch_with_timestamp() {
        let p = detect_provider("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42");
        assert!(p.embed_url.contains("start=42"), "timestamp should be preserved, got: {}", p.embed_url);
    }

    #[test]
    fn detect_provider_youtube_shortlink_with_timestamp() {
        let p = detect_provider("https://youtu.be/dQw4w9WgXcQ?t=30");
        assert!(p.embed_url.contains("start=30"), "got: {}", p.embed_url);
    }

    #[test]
    fn detect_provider_youtube_invalid_id_falls_to_generic() {
        // Short IDs (not 11 chars) should fall through to generic
        let p = detect_provider("https://www.youtube.com/watch?v=short");
        assert_eq!(p.provider_name, "", "short ID should not match youtube");
        assert_eq!(p.embed_url, "https://www.youtube.com/watch?v=short");
    }

    #[test]
    fn detect_provider_youtube_no_v_param_falls_to_generic() {
        let p = detect_provider("https://www.youtube.com/watch");
        assert_eq!(p.provider_name, "");
    }

    #[test]
    fn synthesize_url_embed_alias_xss_is_escaped() {
        // Alias text goes into title= attribute; HTML special chars must be escaped
        let out = synthesize_url_embed_html(
            "https://vimeo.com/123456789",
            &PotholeContent::Alias(r#"My "video" <test>"#.to_string()),
            &empty_snapshot(),
        );
        // The raw quote and angle bracket must not appear unescaped in the output
        assert!(!out.contains(r#"title="My "video""#), "unescaped quote in title, got: {out}");
        assert!(!out.contains("<test>"), "unescaped angle bracket in title, got: {out}");
    }

    #[test]
    fn dispatch_generic_url_with_query_preserved_in_src() {
        // Generic URL with query string: split_dest_url splits on ?, reassemble_url
        // puts it back, detect_provider keeps it unchanged in embed_url, and
        // synthesize_iframe_html HTML-escapes & → &amp; in the src attribute.
        let out = synthesize_url_embed_html(
            "https://example.com/x?a=1&b=2",
            &PotholeContent::Empty,
            &empty_snapshot(),
        );
        // & in query string must be escaped as &amp; in HTML attribute
        assert!(
            out.contains(r#"src="https://example.com/x?a=1&amp;b=2""#),
            "query string must survive and be HTML-escaped, got: {out}"
        );
        assert!(!out.contains("data-provider="), "generic, got: {out}");
    }

    #[test]
    fn detect_provider_youtube_timestamp_with_s_suffix() {
        // YouTube links often use t=30s (with trailing 's' unit); should be
        // normalised to ?start=30 (without the 's')
        let p = detect_provider("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=30s");
        assert!(p.embed_url.contains("start=30"), "s-suffix timestamp should be normalised, got: {}", p.embed_url);
        assert!(!p.embed_url.contains("30s"), "raw 30s must not appear, got: {}", p.embed_url);
    }
}
