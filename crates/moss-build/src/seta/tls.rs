//! HTTPS cert provisioning and custom domain linking for moss-seta.
//!
//! Split out of the app-side `domain/moss_seta_client.rs` (since deleted) per
//! docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 7.

use super::client::{MossSetaClient, SetaError};

impl MossSetaClient {
    /// Link a custom domain to a moss-hosted site.
    ///
    /// Creates a symlink on the VPS so Caddy can serve the site for this domain
    /// and a DB record so `/caddy/ask` returns 200 (enabling on-demand TLS).
    ///
    /// Idempotent: if the domain is already linked to this site, returns Ok.
    pub async fn link_custom_domain(
        &self,
        site_id: &str,
        domain: &str,
    ) -> Result<(), SetaError> {
        let path = format!("/api/sites/{}/domains", site_id);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "POST {} site_id={} domain={}", url, site_id, domain);

        let response = self.client
            .post(&url)
            .header("Authorization", auth_header)
            .json(&serde_json::json!({ "domain": domain }))
            .send()
            .await?;

        let status = response.status().as_u16();
        log::debug!(target: "seta", "Response status: {}", status);

        if matches!(status, 200 | 201) {
            return Ok(());
        }
        // A 409 means the domain is claimed by another site. It used to build
        // its own `SetaError::Api` from `response.text()`, which meant the one
        // message a user actually reads here (orchestrator surfaces it as a
        // cross-site conflict) was an uncapped raw body, with no cf-ray logged
        // and no challenge classification. The status is all this arm needs;
        // `from_failed_response` owns turning the response into an error.
        if status == 409 {
            log::warn!(target: "seta", "Custom domain {} is already claimed by another site (409)", domain);
        }
        Err(SetaError::from_failed_response(response).await)
    }
}
