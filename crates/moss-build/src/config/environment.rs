//! Hosting environment — which seta URL, site suffix, and beacon host apply.
//!
//! The build tree resolves the environment for feed/comment/subscribe URLs, so
//! the type must be reachable from `moss-build`. Resolution *policy* (env vars >
//! config key > default) stays app-side in `domain::config::resolve_environment`.

use serde::{Deserialize, Serialize};

/// Hosting environment for moss deployments.
///
/// Controls which seta URL, site suffix, and beacon host are used.
/// The environment is resolved from:
/// 1. `MOSS_ENV` env var (highest priority)
/// 2. `.moss/config.toml` top-level `environment` field
/// 3. Default: `Production`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum HostingEnvironment {
    Production,
    Staging,
    Local,
}

impl HostingEnvironment {
    pub fn seta_url(&self) -> &'static str {
        match self {
            Self::Production => "https://api.mosspub.com",
            Self::Staging => "https://staging.mosspub.com",
            Self::Local => "http://localhost:8080",
        }
    }

    /// Every suffix this environment's hosting may serve a site under. Which
    /// one a site answers on is the server's to say, at publish; the first is
    /// what a folder is assumed to be under until a publish has said. A suffix
    /// is what follows the site id, so it need not start at a label:
    /// `-test.mosspub.page` makes `<site_id>-test` one label.
    pub fn site_suffixes(&self) -> &'static [&'static str] {
        match self {
            Self::Production => &[".mosspub.com", ".mosspub.page"],
            Self::Staging => &[".staging.mosspub.com", "-test.mosspub.page"],
            Self::Local => &[".localhost"],
        }
    }

    pub fn site_suffix(&self) -> &'static str {
        self.site_suffixes()[0]
    }

    /// The address a site is assumed to be at until a publish has said:
    /// `https://<site_id>` under the first suffix.
    pub fn assumed_site_url(&self, site_id: &str) -> String {
        format!("https://{site_id}{}", self.site_suffix())
    }

    /// Whether `url` is exactly the address this environment's hosting serves
    /// `site_id` at: `https://<site_id><suffix>`, nothing before or after.
    pub fn serves(&self, site_id: &str, url: &str) -> bool {
        let suffix = url.strip_prefix("https://").and_then(|host| host.strip_prefix(site_id));
        !site_id.is_empty() && suffix.is_some_and(|suffix| self.site_suffixes().contains(&suffix))
    }

    /// The host sites are published under, as a person reads it: `mosspub.com`.
    pub fn host(&self) -> &'static str {
        domain_of(self.site_suffix())
    }

    pub fn beacon_host(&self) -> &'static str {
        match self {
            Self::Production => "api.mosspub.com",
            Self::Staging => "staging.mosspub.com",
            Self::Local => "localhost:8080",
        }
    }

    /// Artalk comment server base URL, PER ENVIRONMENT. Each env has
    /// its own Artalk instance so comment testing never touches production data.
    /// Staging Artalk lives at `staging.mosspub.com/comments` (Caddy →
    /// `artalk-staging`); local/dev uses staging (never production). Self-hosted
    /// sites override via `[services.comments] server_url`.
    pub fn comments_server_url(&self) -> &'static str {
        match self {
            Self::Production => "https://api.mosspub.com/comments",
            Self::Staging => "https://staging.mosspub.com/comments",
            Self::Local => "https://staging.mosspub.com/comments",
        }
    }

    /// The address of one of moss's own hosted services, for a plugin that
    /// names the service rather than the address. `None` where the service
    /// does not run, so nothing is published that would point at the wrong
    /// one: commerce runs only as its test server so far.
    pub fn service_url(&self, service: &str) -> Option<&'static str> {
        match (service, self) {
            ("commerce", Self::Staging | Self::Local) => Some("https://commerce-staging.mosspub.com"),
            _ => None,
        }
    }

    /// The name `parse_env_name` accepts for this environment — the one
    /// spelling shared by the config key, the CLI grammar and the UI.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Staging => "staging",
            Self::Local => "local",
        }
    }

    pub fn is_production(&self) -> bool {
        matches!(self, Self::Production)
    }
}

impl Default for HostingEnvironment {
    fn default() -> Self {
        Self::Production
    }
}

/// The domain a suffix puts sites under: all of it after a leading dot, or
/// after the part that ends the site's own label.
fn domain_of(suffix: &str) -> &str {
    suffix.strip_prefix('.').or_else(|| suffix.split_once('.').map(|(_, domain)| domain)).unwrap_or(suffix)
}

/// Parse a named environment string. Returns `None` for unrecognized values.
pub fn parse_env_name(name: &str) -> Option<HostingEnvironment> {
    match name {
        "staging" => Some(HostingEnvironment::Staging),
        "local" => Some(HostingEnvironment::Local),
        "production" => Some(HostingEnvironment::Production),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `name()` writes the config key and `parse_env_name` reads it back; the
    /// two spellings must agree for every variant, and nothing else may parse.
    #[test]
    fn name_and_parse_env_name_round_trip() {
        for env in [HostingEnvironment::Production, HostingEnvironment::Staging, HostingEnvironment::Local] {
            assert_eq!(parse_env_name(env.name()), Some(env));
        }
        assert_eq!(parse_env_name("nonsense"), None);
    }

    #[test]
    fn serves_only_the_exact_address_of_the_site_under_its_own_suffixes() {
        let (prod, staging) = (HostingEnvironment::Production, HostingEnvironment::Staging);
        for url in ["https://her-blog.mosspub.com", "https://her-blog.mosspub.page"] {
            assert!(prod.serves("her-blog", url), "{url}");
            assert!(!staging.serves("her-blog", url), "{url}");
        }
        for url in ["https://her-blog.staging.mosspub.com", "https://her-blog-test.mosspub.page"] {
            assert!(staging.serves("her-blog", url), "{url}");
            assert!(!prod.serves("her-blog", url), "{url}");
        }
        for url in [
            "http://her-blog.mosspub.page",
            "https://her-blog.mosspub.page/",
            "https://her-blog.mosspub.page/a",
            "https://her-blog.mosspub.page.evil.example",
            "https://her-blog.evil.example",
            "https://her-blog-2.mosspub.page",
            "https://her-blog.staging.mosspub.page",
            "https://her-blog.test.mosspub.page",
            "https://her-blog-test.mosspub.com",
            "https://not-her-blog.mosspub.page",
            "https://mosspub.page",
            "her-blog.mosspub.page",
            "",
        ] {
            assert!(!prod.serves("her-blog", url) && !staging.serves("her-blog", url), "{url:?}");
        }
        assert!(!prod.serves("", "https://.mosspub.page"));
        assert!(!staging.serves("", "https://-test.mosspub.page"));
        assert!(!staging.serves("her", "https://her-blog-test.mosspub.page"));
    }

    #[test]
    fn the_host_is_the_domain_sites_are_under_whether_or_not_the_suffix_starts_a_label() {
        assert_eq!(HostingEnvironment::Production.host(), "mosspub.com");
        assert_eq!(HostingEnvironment::Staging.host(), "staging.mosspub.com");
        assert_eq!(HostingEnvironment::Local.host(), "localhost");
        assert_eq!(domain_of("-test.mosspub.page"), "mosspub.page");
        assert_eq!(domain_of(".mosspub.page"), "mosspub.page");
    }
}
