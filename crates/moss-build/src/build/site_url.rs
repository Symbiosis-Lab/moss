//! The `SiteUrl` type: a validated base URL for a moss build.
//!
//! `SiteUrl` is the resolved-and-validated host string that gets baked
//! into `og:image`, `og:url`, canonical link, sitemap, and RSS feed
//! entries. The inner `String` is not public; constructors (`parse`,
//! `from_host`) are the only way to build one, and they enforce:
//!
//! - URL begins with `http://` or `https://`
//! - Non-empty after `trim()`
//! - No trailing slash (stripped during construction)
//!
//! `to_absolute(path)` joins this URL to a root-relative path. `host()`
//! returns the URL with the scheme stripped (for callers like
//! `canonical_url` that want host without protocol).
//!
//! The build-time priority chain that produces a `SiteUrl` (CLI flag →
//! env var → state.toml → localhost defaults) lives in `resolve_site_url`
//! below.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteUrl(String);

#[derive(Debug)]
pub enum SiteUrlError {
    InvalidScheme(String),
    Empty,
}

impl std::fmt::Display for SiteUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScheme(u) => write!(f, "site URL must start with http:// or https://: {}", u),
            Self::Empty => write!(f, "site URL is empty"),
        }
    }
}

impl std::error::Error for SiteUrlError {}

impl SiteUrl {
    /// Construct from an explicit string (CLI flag, env var). Strips any
    /// trailing slash. Rejects empty strings and URLs without http(s)://.
    pub fn parse(raw: &str) -> Result<Self, SiteUrlError> {
        let trimmed = raw.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(SiteUrlError::Empty);
        }
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(SiteUrlError::InvalidScheme(raw.to_string()));
        }
        Ok(SiteUrl(trimmed.to_string()))
    }

    /// Construct from a host (no scheme). Applies https:// scheme. Used
    /// when `state.toml` records a domain without protocol.
    pub fn from_host(host: &str) -> Result<Self, SiteUrlError> {
        Self::parse(&format!("https://{}", host))
    }

    pub fn as_str(&self) -> &str { &self.0 }

    /// Compose an absolute URL from a root-relative path. The path must
    /// begin with '/'; if not, one is inserted.
    pub fn to_absolute(&self, root_relative: &str) -> String {
        if root_relative.starts_with('/') {
            format!("{}{}", self.0, root_relative)
        } else {
            format!("{}/{}", self.0, root_relative)
        }
    }

    /// Extract host without scheme. Needed by callers that feed
    /// `canonical_url(host, path, www_canonical)` — the existing
    /// canonical-link function takes host, not full URL.
    pub fn host(&self) -> &str {
        self.0
            .strip_prefix("https://")
            .or_else(|| self.0.strip_prefix("http://"))
            .unwrap_or(&self.0)
    }

    /// The scheme this site is served over — `"http"` or `"https"`.
    ///
    /// Almost every site is https, but onion sites are served over plain http
    /// (see [`Self::is_deployed`]), and an address that forces https there is
    /// one the reader's browser cannot open. Anything that writes a whole URL
    /// for a human or a scanner to follow — the canonical link, the QR payload
    /// — has to ask rather than assume.
    pub fn scheme(&self) -> &str {
        if self.0.starts_with("http://") { "http" } else { "https" }
    }

    /// True when this URL represents a real deployed host, false when it's a
    /// localhost fallback. Used by the HTML emitter to gate canonical link,
    /// og:url, and feed generation: emitting those tags with localhost values
    /// would produce useless noise in production HTML, so unconfigured-site
    /// builds skip them entirely.
    ///
    /// The check is scheme-agnostic (not `https://`-only): onion sites are
    /// served over `http://<addr>.onion` and must still emit canonical/OG/RSS.
    /// A resolved URL counts as deployed when it has a real host that is not the
    /// `localhost` / `127.0.0.1` loopback used by the preview and build
    /// defaults.
    pub fn is_deployed(&self) -> bool {
        // `host()` strips the scheme (e.g. "localhost:1420", "abc.onion",
        // "example.com", "127.0.0.1:8080"). Strip any path then port so we
        // compare the bare host.
        let host = self.host();
        let host_only = host.split('/').next().unwrap_or(host);
        let host_only = host_only.split(':').next().unwrap_or(host_only);
        !host_only.is_empty() && host_only != "localhost" && host_only != "127.0.0.1"
    }
}

/// Inputs to the resolution chain. Caller (typically `build/render/blocking.rs`)
/// gathers these before invoking `resolve_site_url`.
pub struct ResolveInputs<'a> {
    /// Value of `--site-url` CLI flag, if passed.
    pub cli_flag: Option<&'a str>,
    /// Value of `MOSS_SITE_URL` env var, if set. Owned because
    /// `std::env::var` returns `String`; the caller would otherwise
    /// have to hold a temporary alive across this call.
    pub env_var: Option<String>,
    /// `.moss/config.toml [site].domain` — the authored custom domain, when
    /// set. Authored intent, so it lives in the git-tracked file; the build
    /// never consults server state for it.
    pub domain: Option<String>,
    /// `.moss/state.toml [deployment].site_url` — a plugin deployment's own
    /// canonical URL (e.g. an OnionPress `http://<addr>.onion`). Consulted after
    /// `domain` (a bought custom domain still wins → the onion is an
    /// Onion-Location alternate) and before `site_id`. Its scheme is preserved,
    /// so an http onion address stays http.
    pub site_url: Option<String>,
    /// `.moss/state.toml [deployment].site_id` — mosspub.com slug.
    pub site_id: Option<String>,
    /// Whether the CDN hostname for this domain is serving — seta's
    /// `cdn_status == "active"`, read by `get_domain_cdn`.
    ///
    /// This is what drives `www.` vs bare-apex, and it is deliberately an
    /// OBSERVATION rather than a fact about where the domain was bought. A
    /// purchased domain and one the user brought from another registrar both
    /// end up on the same Cloudflare custom hostname, so "did the hostname go
    /// active" is one rule for both flows, and RSS, canonical links, `og:url`,
    /// the sitemap and share cards all follow it automatically. The older rule
    /// — purchased means `www.`, external always means bare apex — predates
    /// external-domain CDN support and was wrong for it in both directions.
    pub cdn_active: bool,
    /// Active `moss preview` Vite port. None for non-preview builds.
    pub preview_port: Option<u16>,
    /// Hosting-environment site suffix, e.g. `.mosspub.com` (Production),
    /// `.staging.mosspub.com` (Staging), or `.localhost` (Local).
    /// Resolved from `HostingEnvironment::site_suffix()` at the call site.
    /// Defaults to `".mosspub.com"` (Production) so callers without a
    /// project path in scope don't need to special-case the common case.
    pub site_suffix: String,
}

impl<'a> Default for ResolveInputs<'a> {
    fn default() -> Self {
        Self {
            cli_flag: None,
            env_var: None,
            domain: None,
            site_url: None,
            site_id: None,
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        }
    }
}

/// Resolve the site's canonical URL for this build. Walks the priority chain:
///
///   1. `--site-url=<url>` CLI flag (`cli_flag`)
///   2. `MOSS_SITE_URL=<url>` env var (`env_var`)
///   3. `.moss/config.toml [site].domain` — the authored domain (with `www.`
///      policy when the domain's CDN hostname is active)
///   4. `.moss/state.toml [deployment].site_url` (a plugin deploy's canonical
///      URL, e.g. an onion address — scheme preserved)
///   5. `.moss/state.toml [deployment].site_id` → `https://{id}.mosspub.com`
///   6. Preview default: `http://localhost:<port>` when `preview_port` is set
///   7. Build default: `http://localhost`
///
/// Returns `Err` only when an explicitly-provided value (CLI flag or env var)
/// is malformed. State-derived and default branches always succeed.
pub fn resolve_site_url(inputs: &ResolveInputs) -> Result<SiteUrl, SiteUrlError> {
    if let Some(raw) = inputs.cli_flag {
        return SiteUrl::parse(raw);
    }
    if let Some(raw) = inputs.env_var.as_deref() {
        return SiteUrl::parse(raw);
    }
    if let Some(domain) = inputs.domain.as_deref() {
        // The canonical host follows what verification observed: `www.` once
        // the CDN hostname is active, the bare apex otherwise. See
        // `ResolveInputs::cdn_active`.
        let host = if inputs.cdn_active && !domain.starts_with("www.") {
            format!("www.{}", domain)
        } else {
            domain.to_string()
        };
        return SiteUrl::from_host(&host);
    }
    // A plugin deployment's own canonical URL (e.g. an OnionPress onion
    // address). Ranked after `domain` and before `site_id`, and parsed as-is so
    // an http:// onion scheme is preserved. A malformed persisted value falls
    // through to `site_id` rather than hard-failing the build — matching the
    // never-error contract of the state-derived branches.
    if let Some(url) = inputs.site_url.as_deref().and_then(|raw| SiteUrl::parse(raw).ok()) {
        return Ok(url);
    }
    if let Some(site_id) = inputs.site_id.as_deref() {
        return SiteUrl::from_host(&format!("{}{}", site_id, inputs.site_suffix));
    }
    if let Some(port) = inputs.preview_port {
        return SiteUrl::parse(&format!("http://localhost:{}", port));
    }
    SiteUrl::parse("http://localhost")
}

/// Resolve the site URL for a project exactly as a build does — the ONE
/// resolution path, shared by the render pipeline and the deploy guard so
/// the two can never diverge. `cli_override` is the `--site-url` flag
/// (build config); the deploy guard passes `None`.
pub fn resolve_for_project(
    project_path: &str,
    cli_override: Option<&str>,
) -> Result<SiteUrl, String> {
    let domain_cfg = match crate::build::site_config::get_domain_config(project_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            // A silent default here is what shipped localhost-flavored
            // builds: an unreadable state.toml (iCloud-dataless file,
            // partial write) must not quietly mean "site not deployed".
            log::warn!(
                "site URL: could not read deployment state ({}); treating project as undeployed",
                e
            );
            Default::default()
        }
    };
    // Whether the CDN hostname is active is an observation the desktop (or
    // `moss domain link`) makes by asking seta, which a build must not do —
    // it has no identity, no network budget and no business blocking on a
    // remote call. That observation is recorded as `[deployment].observed
    // .cdn_status` by `record_cdn_status`, and read back here as
    // `domain_cfg.cdn_status`. Absence (never observed, or observed anything
    // other than "active") falls back to the bare apex — an unverified
    // `www.` is a canonical URL nothing answers on.
    let cdn_active = domain_cfg.cdn_status.as_deref() == Some("active");
    let env_var = std::env::var("MOSS_SITE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let preview_port = std::env::var("VITE_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok());
    let cli_flag = cli_override.filter(|s| !s.trim().is_empty());
    let site_suffix = crate::build::site_config::resolve_environment(project_path)
        .site_suffix()
        .to_string();
    let inputs = ResolveInputs {
        cli_flag,
        env_var,
        domain: domain_cfg.domain,
        // A plugin deploy's own canonical URL (e.g. an OnionPress .onion) —
        // ranked after `domain`, before `site_id` (see resolve_site_url).
        // Resolution is shared with the deploy guard, so an onion build and
        // the publish that ships it can never disagree about this URL.
        site_url: domain_cfg.site_url,
        site_id: domain_cfg.site_id,
        cdn_active,
        preview_port,
        site_suffix,
    };
    resolve_site_url(&inputs).map_err(|e| format!("site URL: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_trailing_slash() {
        let u = SiteUrl::parse("https://example.com/").unwrap();
        assert_eq!(u.as_str(), "https://example.com");
    }

    #[test]
    fn parse_rejects_missing_scheme() {
        assert!(matches!(
            SiteUrl::parse("example.com"),
            Err(SiteUrlError::InvalidScheme(_))
        ));
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(matches!(SiteUrl::parse(""), Err(SiteUrlError::Empty)));
        assert!(matches!(SiteUrl::parse("   "), Err(SiteUrlError::Empty)));
    }

    #[test]
    fn from_host_applies_https_scheme() {
        let u = SiteUrl::from_host("example.com").unwrap();
        assert_eq!(u.as_str(), "https://example.com");
    }

    #[test]
    fn to_absolute_joins_with_leading_slash() {
        let u = SiteUrl::parse("https://example.com").unwrap();
        assert_eq!(u.to_absolute("/_moss/og/abc.png"), "https://example.com/_moss/og/abc.png");
    }

    #[test]
    fn to_absolute_inserts_slash_when_missing() {
        let u = SiteUrl::parse("https://example.com").unwrap();
        assert_eq!(u.to_absolute("foo.png"), "https://example.com/foo.png");
    }

    #[test]
    fn host_strips_https_scheme() {
        let u = SiteUrl::parse("https://example.com").unwrap();
        assert_eq!(u.host(), "example.com");
    }

    #[test]
    fn is_deployed_true_for_https() {
        assert!(SiteUrl::parse("https://example.com").unwrap().is_deployed());
    }

    #[test]
    fn is_deployed_false_for_http_localhost() {
        assert!(!SiteUrl::parse("http://localhost").unwrap().is_deployed());
        assert!(!SiteUrl::parse("http://localhost:1420").unwrap().is_deployed());
    }

    #[test]
    fn host_strips_http_scheme() {
        let u = SiteUrl::parse("http://localhost:1420").unwrap();
        assert_eq!(u.host(), "localhost:1420");
    }

    #[test]
    fn resolve_prefers_cli_over_env_over_state() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: Some("https://from-cli.test"),
            env_var: Some("https://from-env.test".to_string()),
            domain: Some("from-state-domain.test".to_string()),
            site_id: Some("from-state-site-id".to_string()),
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://from-cli.test");
    }

    #[test]
    fn resolve_uses_env_when_cli_absent() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: Some("https://from-env.test".to_string()),
            domain: Some("from-state.test".to_string()),
            site_id: None,
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://from-env.test");
    }

    #[test]
    fn resolve_uses_www_once_the_cdn_hostname_is_active() {
        // One rule for both flows — purchased or brought from another
        // registrar, the host that serves is `www.` once the CDN hostname
        // went active. See ResolveInputs::cdn_active.
        let inputs = ResolveInputs {
            domain: Some("example.com".to_string()),
            site_id: Some("ignored-when-domain-present".to_string()),
            cdn_active: true,
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://www.example.com");
    }

    #[test]
    fn resolve_uses_bare_apex_until_the_cdn_hostname_is_active() {
        // Everything that is not "active" — never started, waiting on DCV,
        // validating, failed — canonicalizes to the apex. Baking an
        // unverified `www.` into canonical/og:url/RSS points every crawler at
        // a name nothing answers on yet.
        let inputs = ResolveInputs {
            domain: Some("example.com".to_string()),
            cdn_active: false,
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://example.com");
    }

    #[test]
    fn resolve_uses_site_id_as_mosspub_subdomain_when_no_domain() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: None,
            domain: None,
            site_id: Some("her-blog".to_string()),
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://her-blog.mosspub.com");
    }

    #[test]
    fn resolve_uses_preview_port_default_when_nothing_else_set() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: None,
            domain: None,
            site_id: None,
            cdn_active: false,
            preview_port: Some(1420),
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "http://localhost:1420");
    }

    #[test]
    fn resolve_uses_build_default_when_no_preview_port() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: None,
            domain: None,
            site_id: None,
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "http://localhost");
    }

    #[test]
    fn resolve_rejects_invalid_cli_flag_with_clear_error() {
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: Some("not-a-url"),
            env_var: None,
            domain: None,
            site_id: None,
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        assert!(matches!(
            resolve_site_url(&inputs),
            Err(SiteUrlError::InvalidScheme(_))
        ));
    }

    #[test]
    fn resolve_rejects_invalid_env_var_with_clear_error() {
        // Mirror of the cli_flag rejection test for the env-var branch.
        // Both branches route through SiteUrl::parse, so error reporting
        // should be identical.
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: Some("not-a-url".to_string()),
            domain: None,
            site_id: None,
            cdn_active: false,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        assert!(matches!(
            resolve_site_url(&inputs),
            Err(SiteUrlError::InvalidScheme(_))
        ));
    }

    #[test]
    fn resolve_keeps_www_prefix_when_already_present() {
        // Idempotency: a domain already recorded as "www.something" must not
        // get a second "www." prefix prepended once its CDN goes active.
        let inputs = ResolveInputs {
            site_url: None,
            cli_flag: None,
            env_var: None,
            domain: Some("www.example.com".to_string()),
            site_id: None,
            cdn_active: true,
            preview_port: None,
            site_suffix: ".mosspub.com".to_string(),
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://www.example.com");
    }

    #[test]
    fn site_url_uses_environment_suffix() {
        let inputs = ResolveInputs {
            site_url: None,
            site_id: Some("her-blog".into()),
            site_suffix: ".staging.mosspub.com".into(),
            ..Default::default()
        };
        let url = resolve_site_url(&inputs).unwrap();
        assert_eq!(url.host(), "her-blog.staging.mosspub.com");
    }

    // ── [deployment].site_url — plugin/onion canonical URL ──────────────

    #[test]
    fn is_deployed_true_for_http_onion() {
        // Onion sites are served over http:// (no TLS), but have a real host —
        // they MUST emit canonical/OG/RSS. The old https-only gate suppressed them.
        let u = SiteUrl::parse("http://abcdefghijklmnop.onion").unwrap();
        assert!(u.is_deployed(), "an http onion address is a real deployed host");
    }

    #[test]
    fn is_deployed_false_for_http_loopback_ip() {
        // The build/preview loopback defaults must stay not-deployed regardless
        // of the scheme-agnostic widening.
        assert!(!SiteUrl::parse("http://127.0.0.1").unwrap().is_deployed());
        assert!(!SiteUrl::parse("http://127.0.0.1:8080").unwrap().is_deployed());
    }

    #[test]
    fn resolve_http_onion_site_url_is_deployed_and_preserves_scheme() {
        let inputs = ResolveInputs {
            site_url: Some("http://abcdefghijklmnop.onion".to_string()),
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        // Scheme preserved (http, not silently upgraded to https)...
        assert_eq!(resolved.as_str(), "http://abcdefghijklmnop.onion");
        // ...and it counts as deployed → canonical/OG/RSS are emitted.
        assert!(resolved.is_deployed());
    }

    #[test]
    fn resolve_site_url_wins_over_site_id_when_no_domain() {
        // With no custom domain, a persisted plugin site_url takes precedence
        // over the mosspub site_id fallback.
        let inputs = ResolveInputs {
            site_url: Some("http://onionaddr.onion".to_string()),
            site_id: Some("her-blog".to_string()),
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "http://onionaddr.onion");
    }

    #[test]
    fn resolve_domain_wins_over_site_url() {
        // A bought custom domain outranks the onion site_url — the onion becomes
        // an Onion-Location alternate, the canonical stays on the real domain.
        let inputs = ResolveInputs {
            domain: Some("example.com".to_string()),
            site_url: Some("http://onionaddr.onion".to_string()),
            site_id: Some("her-blog".to_string()),
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://example.com");
    }

    #[test]
    fn resolve_malformed_site_url_falls_through_to_site_id() {
        // A garbage persisted site_url must not hard-fail the build; it falls
        // through to site_id (matching the never-error state-derived contract).
        let inputs = ResolveInputs {
            site_url: Some("not-a-url".to_string()),
            site_id: Some("her-blog".to_string()),
            ..Default::default()
        };
        let resolved = resolve_site_url(&inputs).unwrap();
        assert_eq!(resolved.as_str(), "https://her-blog.mosspub.com");
    }

    // ── resolve_for_project — cdn_active derives from the recorded observation ──

    #[test]
    fn resolve_for_project_uses_www_once_cdn_status_is_recorded_active() {
        // A domain linked from the CLI and a domain set up through Settings
        // both resolve through the same recorded fact: whichever path last
        // called `record_cdn_status(_, "active")`.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_str().unwrap();
        crate::vault::config::save_site_str(path, "domain", "example.com").unwrap();
        crate::vault::deployment_state::record_cdn_status(path, "active").unwrap();
        let resolved = resolve_for_project(path, None).unwrap();
        assert_eq!(resolved.as_str(), "https://www.example.com");
    }

    #[test]
    fn resolve_for_project_uses_bare_apex_with_no_cdn_observation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().to_str().unwrap();
        crate::vault::config::save_site_str(path, "domain", "example.com").unwrap();
        let resolved = resolve_for_project(path, None).unwrap();
        assert_eq!(resolved.as_str(), "https://example.com");
    }
}
