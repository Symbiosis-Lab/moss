//! CDN activation for a custom hostname (`/api/domains/:host/cdn`).
//!
//! Its own file for the same reason `dns.rs` and `tls.rs` are: this module is
//! organised one file per endpoint family, and CDN activation is a family of
//! its own — the one piece of domain state the desktop genuinely cannot
//! observe for itself (see `domain/connect/probe.rs`). Everything else about a
//! domain is checked locally against the deployer's `DnsTarget`; whether
//! Cloudflare has finished validating moss's custom hostname is only visible
//! to the server that asked for it.

use super::client::{MossSetaClient, SetaError};
use crate::config::deployment::DnsRecord;
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

/// Where a domain's CDN hostname is in its lifecycle, as the server sees it.
///
/// `Unknown` is not a server state — it is what an unrecognised string
/// deserializes to. seta ships ahead of the desktop by design (its `staging`
/// and `main` both deploy to production), so a build in the field WILL meet a
/// status word it has never heard of, and the whole domain panel going dark
/// over one unfamiliar token would be the wrong trade. An unknown status
/// simply is not `Active`, which is the only question the canonical-host rule
/// asks of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CdnStatus {
    /// No CDN hostname exists for this domain yet.
    Disabled,
    /// The hostname exists; Cloudflare is waiting on the Delegated-DCV records.
    PendingDcv,
    /// The records are in place; Cloudflare is validating and issuing.
    Validating,
    /// The hostname serves traffic. The canonical host becomes `www.{domain}`.
    Active,
    /// Cloudflare gave up on this hostname. Recreate it with `start_domain_cdn`.
    Failed,
    /// A status word this build does not know. See the type doc.
    Unknown,
}

impl CdnStatus {
    /// Read a status word off the wire. Anything unrecognised is `Unknown`.
    pub fn from_wire(raw: &str) -> Self {
        match raw {
            "disabled" => Self::Disabled,
            "pending_dcv" => Self::PendingDcv,
            "validating" => Self::Validating,
            "active" => Self::Active,
            "failed" => Self::Failed,
            other => {
                log::debug!(target: "seta", "unknown cdn_status from server: {}", other);
                Self::Unknown
            }
        }
    }

    /// The hostname is serving. This is the observation the canonical host
    /// follows — see `build::site_url::resolve_site_url`.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// The wire word, for persisting into `[deployment].observed`. `Unknown`
    /// has none — the original token was dropped at parse time, and storing a
    /// word this build invented would be worse than storing nothing.
    pub fn wire_word(&self) -> Option<&'static str> {
        match self {
            Self::Disabled => Some("disabled"),
            Self::PendingDcv => Some("pending_dcv"),
            Self::Validating => Some("validating"),
            Self::Active => Some("active"),
            Self::Failed => Some("failed"),
            Self::Unknown => None,
        }
    }
}

impl<'de> Deserialize<'de> for CdnStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_wire(&raw))
    }
}

/// The CDN state of one custom hostname, plus the records that get it there.
///
/// The records are `DnsRecord` — the same shape a deploy plugin's `DnsTarget`
/// carries — rather than the server's own `{type, subdomain, value}` triple,
/// so the records table renders one shape whatever produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct DomainCdn {
    pub cdn_status: CdnStatus,
    /// What the user has to have in their zone. Empty while the CDN is
    /// disabled — there is nothing to paste until a hostname exists.
    pub records: Vec<DnsRecord>,
}

/// The wire shape of `/api/domains/:host/cdn`, kept private so the mapping to
/// `DnsRecord` happens in exactly one place.
#[derive(Deserialize)]
struct CdnWire {
    cdn_status: CdnStatus,
    #[serde(default)]
    records: Vec<CdnRecordWire>,
}

#[derive(Deserialize)]
struct CdnRecordWire {
    #[serde(rename = "type")]
    record_type: String,
    subdomain: String,
    value: String,
}

impl From<CdnWire> for DomainCdn {
    fn from(wire: CdnWire) -> Self {
        DomainCdn {
            cdn_status: wire.cdn_status,
            records: wire
                .records
                .into_iter()
                .map(|r| DnsRecord {
                    record_type: r.record_type,
                    // seta calls it `subdomain`; a `DnsRecord` calls the same
                    // thing `name` ("@" for apex, "www").
                    name: r.subdomain,
                    value: r.value,
                    ttl: None,
                })
                .collect(),
        }
    }
}

impl MossSetaClient {
    /// Read the CDN state of a domain the authenticated user owns.
    ///
    /// Calls `GET /api/domains/:host/cdn`. Purely an observation: it never
    /// creates a hostname, so it is safe to poll behind the status row.
    pub async fn get_domain_cdn(&self, host: &str) -> Result<DomainCdn, SetaError> {
        let path = format!("/api/domains/{}/cdn", host);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} host={}", url, host);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        log::debug!(target: "seta", "Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response
            .json::<CdnWire>()
            .await
            .map(DomainCdn::from)
            .map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Start — or restart — CDN activation for a domain the user brought from
    /// another registrar.
    ///
    /// Calls `POST /api/domains/:host/cdn` and answers with the same shape as
    /// the GET. Recreating a hostname mints FRESH DCV values, so the records
    /// block must re-render from this response and never from cached values.
    ///
    /// The server refuses with 409 for a moss-purchased domain (whose zone
    /// seta writes itself), 403 when the domain is not the caller's, and 502
    /// when Cloudflare fails — all of which arrive as `SetaError::Api`.
    pub async fn start_domain_cdn(&self, host: &str) -> Result<DomainCdn, SetaError> {
        let path = format!("/api/domains/{}/cdn", host);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "POST {} host={}", url, host);

        let response = self.client
            .post(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        log::debug!(target: "seta", "Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response
            .json::<CdnWire>()
            .await
            .map(DomainCdn::from)
            .map_err(|e| SetaError::Parse(e.to_string()))
    }
}

#[cfg(test)]
#[path = "cdn_tests.rs"]
mod tests;
