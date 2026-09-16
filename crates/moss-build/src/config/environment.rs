//! Hosting environment — which seta URL, site suffix, and beacon host apply.
//!
//! Moved from the app crate's `domain/types.rs` (2026-08-27, M6a B3): the
//! build tree resolves the environment for feed/comment/subscribe URLs, so
//! the type must be reachable from `moss-build` when the pipeline crosses.
//! Resolution *policy* (env vars > config key > default) stays app-side in
//! `domain::config::resolve_environment` until the eviction-aware config
//! read crosses with the cloud cluster (ADR-059).

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

    pub fn site_suffix(&self) -> &'static str {
        match self {
            Self::Production => ".mosspub.com",
            Self::Staging => ".staging.mosspub.com",
            Self::Local => ".localhost",
        }
    }

    pub fn beacon_host(&self) -> &'static str {
        match self {
            Self::Production => "api.mosspub.com",
            Self::Staging => "staging.mosspub.com",
            Self::Local => "localhost:8080",
        }
    }

    /// Artalk comment server base URL, PER ENVIRONMENT (ADR-026). Each env has
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
}
