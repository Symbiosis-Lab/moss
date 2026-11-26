//! `[services]` schema v2 — the typed shape of `.moss/config.toml`'s services.
//!
//! The build tree decides footer chrome and slot resolution from these types,
//! so they must be reachable from `moss-build`. The
//! path-taking readers live in `build::site_config` and the write primitive in
//! `vault::config` — this module is the vocabulary,
//! not the file access.
//!
//! One concept: `[services.<kind>]` is the single place a site says HOW an
//! integration is wired. Presence of a section implies the service is enabled
//! unless `enabled = false`. Absence means "off for self-hosted; platform
//! default for moss-hosted."

use super::environment::HostingEnvironment;
use serde::{Deserialize, Serialize};

/// Identifies a service kind. Use this instead of stringly-typed keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Analytics,
    Comments,
    Email,
    Forms,
}

impl ServiceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Analytics => "analytics",
            Self::Comments => "comments",
            Self::Email => "email",
            Self::Forms => "forms",
        }
    }

    pub const ALL: [ServiceKind; 4] = [Self::Analytics, Self::Comments, Self::Email, Self::Forms];

    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|k| k.as_str() == s).copied()
    }
}

/// Fields common to every `[services.<kind>]` entry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct ServiceCommon {
    /// Explicit on/off. `None` means "on by presence."
    /// Write `enabled = false` to disable a moss-platform default on a
    /// moss-hosted site.
    pub enabled: Option<bool>,
    /// Provider identifier. Required whenever the service is enabled on a
    /// self-hosted site.
    pub provider: Option<String>,
}

/// `[services.analytics]`. Provider: `goatcounter`, or the name of a plugin in
/// the site's folder that is an analytics service, with the site's `site_id`
/// there.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct AnalyticsService {
    #[serde(flatten)]
    pub common: ServiceCommon,
    pub script: Option<String>,
    pub site_id: Option<String>,
}

impl AnalyticsService {
    /// The plugin that serves analytics, and the site's id there.
    pub fn plugin(&self) -> Option<(&str, &str)> {
        self.common.provider.as_deref().filter(|p| *p != "goatcounter").map(|p| (p, self.site_id.as_deref().unwrap_or_default()))
    }
}

/// `[services.comments]`. Provider: `artalk`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct CommentsService {
    #[serde(flatten)]
    pub common: ServiceCommon,
    pub server_url: Option<String>,
}

impl CommentsService {
    /// Resolved Artalk server (design §3): explicit self-hosted override →
    /// env-derived moss-operated server (requires the site to be DEPLOYED — a
    /// custom domain OR a moss `site_id`) → empty string (never deployed: render
    /// inactive form, skip sync). A moss-hosted site has a `site_id` but often no
    /// custom domain, and its comments still live at the env host — so deployment
    /// is signalled by EITHER, not by a custom domain alone.
    pub fn resolved_server_url(
        &self,
        domain: Option<&str>,
        site_id: Option<&str>,
        env: &HostingEnvironment,
    ) -> String {
        if let Some(custom) = &self.server_url {
            return custom.clone();
        }
        if domain.is_some() || site_id.is_some() {
            return env.comments_server_url().to_string();
        }
        String::new()
    }
}

/// `[services.email]`. Provider today: `buttondown`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct EmailService {
    #[serde(flatten)]
    pub common: ServiceCommon,
    pub api_key: Option<String>,
}

/// `[services.forms]`. Where request forms post: `<endpoint>/<form name>`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct FormsService {
    #[serde(flatten)]
    pub common: ServiceCommon,
    pub endpoint: Option<String>,
}

pub mod booking;
pub use booking::BookingService;

/// A service address must be a web address; the message names the table it came from.
pub(crate) fn endpoint_problem(table: &str, endpoint: &str) -> Option<String> {
    (!endpoint.starts_with("https://") && !endpoint.starts_with("http://"))
        .then(|| format!("[services.{table}] endpoint '{endpoint}' must start with https:// or http://"))
}

/// Root `[services]` table, one field per `ServiceKind`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[derive(specta::Type)]
pub struct ServicesConfig {
    pub analytics: Option<AnalyticsService>,
    pub comments: Option<CommentsService>,
    pub email: Option<EmailService>,
    pub forms: Option<FormsService>,
}

impl ServicesConfig {
    /// The comments service, unless it is absent or `enabled = false`: comments
    /// are opt-in on every host, and a present-but-disabled section behaves
    /// like an absent one.
    pub fn comments_on(&self) -> Option<&CommentsService> {
        self.comments.as_ref().filter(|c| c.common.enabled != Some(false))
    }

    /// Whether a service is effectively on. `hosted_on_moss` comes from the
    /// derived `deploy_method == "moss"` on the flat deployment view — see
    /// `domain::config::get_domain_config`. Pass it in; this type does
    /// not read config or state itself.
    ///
    /// Defaults when section is absent are per-kind:
    /// - Analytics: moss-hosted defaults on (platform pageview beacon).
    /// - Email:     moss-hosted defaults on (platform newsletter form).
    /// - Comments:  always opt-in. Comments need moderation, identity, and
    ///   provider choice; we do not silently inject a third-party widget on
    ///   every page just because the site is moss-hosted. Section absent =
    ///   off, on every host.
    pub fn is_enabled(&self, kind: ServiceKind, hosted_on_moss: bool) -> bool {
        let common = match kind {
            ServiceKind::Analytics => self.analytics.as_ref().map(|s| &s.common),
            ServiceKind::Comments => self.comments.as_ref().map(|s| &s.common),
            ServiceKind::Email => self.email.as_ref().map(|s| &s.common),
            ServiceKind::Forms => self.forms.as_ref().map(|s| &s.common),
        };
        match common {
            // Section present: enabled unless explicitly off.
            Some(c) => c.enabled.unwrap_or(true),
            // Section absent: per-kind default. Comments require explicit opt-in.
            None => match kind {
                ServiceKind::Comments | ServiceKind::Forms => false,
                ServiceKind::Analytics | ServiceKind::Email => hosted_on_moss,
            },
        }
    }

    /// Where request forms post, without a trailing slash: `None` when the
    /// section is absent, switched off, or has no endpoint.
    pub fn forms_endpoint(&self) -> Option<String> {
        let forms = self.forms.as_ref().filter(|_| self.is_enabled(ServiceKind::Forms, false))?;
        let endpoint = forms.endpoint.as_deref()?.trim().trim_end_matches('/');
        (!endpoint.is_empty()).then(|| endpoint.to_string())
    }

    /// Validate provider values and wiring coherence. Returns the first error.
    /// Called from `get_services_config`; also safe to call after in-memory edits.
    ///
    /// Rules:
    /// - `provider` values must be in the per-kind allowlist.
    /// - For analytics/email: a section with `enabled = true` and no provider is
    ///   a load error on self-hosted sites. On moss-hosted sites, "enabled with
    ///   no wiring" falls back to the platform default (error suppressed).
    /// - For comments: always wired via env-derived server URL — `enabled = true`
    ///   with no explicit `server_url` is valid on any host. Only the provider
    ///   allowlist check (`["artalk"]`) applies.
    pub fn validate(&self, hosted_on_moss: bool) -> Result<(), String> {
        fn check(
            kind: &str,
            common: &ServiceCommon,
            allowed_providers: &[&str],
            wired: bool,
            hosted_on_moss: bool,
        ) -> Result<(), String> {
            if let Some(p) = &common.provider {
                if !allowed_providers.contains(&p.as_str()) {
                    return Err(format!(
                        "[services.{kind}] unknown provider '{p}' (allowed: {:?})",
                        allowed_providers
                    ));
                }
            }
            // enabled = true with no wiring is only a problem for self-hosted sites.
            if common.enabled == Some(true) && common.provider.is_none() && !wired && !hosted_on_moss {
                return Err(format!(
                    "[services.{kind}] enabled = true but no provider configured"
                ));
            }
            Ok(())
        }

        if let Some(a) = &self.analytics {
            if let Some((plugin, "")) = a.plugin() {
                return Err(format!("[services.analytics] provider is \"{plugin}\", which needs the site's site_id there"));
            }
            check("analytics", &a.common, &[a.plugin().map_or("goatcounter", |(p, _)| p)], a.script.is_some() || a.plugin().is_some(), hosted_on_moss)?;
        }
        if let Some(c) = &self.comments {
            // Comments are always wired post-v4: env-derived server URL means
            // `enabled = true` with no explicit `server_url` is valid on any host
            // (GitHub-hosted sites use the moss-operated server
            // via env derivation; the old "no wiring" error was a false alarm).
            // Provider allowlist check (`["artalk"]`) is kept.
            check("comments", &c.common, &["artalk"], /*wired=*/ true, hosted_on_moss)?;
        }
        if let Some(e) = &self.email {
            check("email", &e.common, &["buttondown"], e.api_key.is_some(), hosted_on_moss)?;
        }
        if let Some(problem) = self.forms.as_ref().and_then(|f| f.endpoint.as_deref()).and_then(|e| endpoint_problem("forms", e)) {
            return Err(problem);
        }
        Ok(())
    }
}

#[cfg(test)]
mod forms_tests {
    use super::*;

    fn parse(toml_src: &str) -> ServicesConfig {
        toml::from_str::<toml::Value>(toml_src).unwrap()["services"].clone().try_into().unwrap()
    }

    #[test]
    fn forms_is_a_service_kind_with_its_own_name() {
        assert_eq!(ServiceKind::Forms.as_str(), "forms");
        assert_eq!(ServiceKind::from_str("forms"), Some(ServiceKind::Forms));
        assert!(ServiceKind::ALL.contains(&ServiceKind::Forms));
    }

    #[test]
    fn the_endpoint_is_read_and_loses_a_trailing_slash() {
        let s = parse("[services.forms]\nendpoint = \"https://forms.example/f/\"\n");
        assert_eq!(s.forms_endpoint().as_deref(), Some("https://forms.example/f"));
        assert!(s.is_enabled(ServiceKind::Forms, false));
    }

    #[test]
    fn no_section_or_no_endpoint_or_disabled_is_no_endpoint() {
        assert_eq!(ServicesConfig::default().forms_endpoint(), None);
        assert_eq!(parse("[services.forms]\n").forms_endpoint(), None);
        let off = parse("[services.forms]\nendpoint = \"https://forms.example\"\nenabled = false\n");
        assert_eq!(off.forms_endpoint(), None);
        assert!(!off.is_enabled(ServiceKind::Forms, true));
    }

    #[test]
    fn an_endpoint_that_is_not_a_web_address_is_a_load_error() {
        let bad = parse("[services.forms]\nendpoint = \"forms.example\"\n");
        assert!(bad.validate(false).unwrap_err().contains("[services.forms] endpoint"));
        assert!(parse("[services.forms]\nendpoint = \"https://forms.example\"\n").validate(false).is_ok());
    }
}

/// Who may subscribe; a subscriber may read members pages.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MembersConfig {
    #[serde(default)]
    pub join: MemberJoin,
    /// Where readers' replies to the site's mail go; unset, the owner's login
    /// email. Private: never in the public `members.json`.
    #[serde(default, skip_serializing)]
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberJoin {
    #[default]
    Open,
    Invite,
}

impl MembersConfig {
    pub fn read(config: &crate::config::ConfigFile) -> Result<Self, String> {
        let mut members: Self = config.section(&["members"]).map_or(Ok(Self::default()), |table| {
            table.clone().try_into().map_err(|e| format!("[members]: {e}"))
        })?;
        members.reply_to = members.reply_to.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        if let Some(address) = &members.reply_to {
            if !is_plain_address(address) {
                return Err(format!("[members] reply_to: \"{address}\" is not one email address, such as \"desk@example.org\""));
            }
        }
        Ok(members)
    }
}

/// One `name@domain.tld`, the only shape a mail header takes from it as is.
fn is_plain_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else { return false };
    let part = |s: &str| !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control() || "@<>()[],;:\"\\".contains(c));
    address.len() <= 254 && part(local) && part(domain) && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod members_tests {
    use super::MembersConfig;

    fn read(toml_src: &str) -> Result<MembersConfig, String> {
        MembersConfig::read(&crate::config::ConfigFile::parse(toml_src).unwrap())
    }

    #[test]
    fn reply_to_is_one_address_trimmed_and_blank_is_unset() {
        assert_eq!(read("[members]\nreply_to = \" desk@harbor-mail.org \"\n").unwrap().reply_to.as_deref(), Some("desk@harbor-mail.org"));
        assert_eq!(read("[members]\nreply_to = \"\"\n").unwrap().reply_to, None);
        assert_eq!(read("").unwrap().reply_to, None);
    }

    #[test]
    fn reply_to_that_is_not_one_plain_address_stops_the_build() {
        for bad in ["desk", "Desk <desk@harbor-mail.org>", "a@harbor-mail.org, b@harbor-mail.org", "a@harbor-mail.org\\r\\nBcc: x@y.org", "a@b@harbor-mail.org", "desk@localhost"] {
            let err = read(&format!("[members]\nreply_to = \"{bad}\"\n")).unwrap_err();
            assert!(err.starts_with("[members] reply_to:"), "{bad}: {err}");
        }
    }

    #[test]
    fn reply_to_never_reaches_the_public_policy() {
        let members = read("[members]\njoin = \"invite\"\nreply_to = \"desk@harbor-mail.org\"\n").unwrap();
        assert_eq!(serde_json::to_string(&members).unwrap(), r#"{"join":"invite"}"#);
    }
}

#[cfg(test)]
mod analytics_plugin_tests {
    #[test]
    fn a_plugin_provider_needs_the_site_s_id() {
        let services = |toml: &str| toml::from_str::<super::ServicesConfig>(toml).unwrap().validate(false);
        assert_eq!(services("[analytics]\nprovider = \"cloudflare\"\nsite_id = \"t0ken\"\n"), Ok(()));
        assert!(services("[analytics]\nprovider = \"cloudflare\"\n").unwrap_err().contains("site_id"));
        assert_eq!(services("[analytics]\nprovider = \"goatcounter\"\nscript = \"https://x.goatcounter.com/count\"\n"), Ok(()));
    }
}
