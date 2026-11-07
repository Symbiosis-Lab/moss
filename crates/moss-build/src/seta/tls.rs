//! HTTPS cert provisioning and custom domain linking for moss-seta.

use super::client::{MossSetaClient, SetaError};
use reqwest::Method;

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
        let path = format!("/api/sites/{}/domains", urlencoding::encode(site_id));
        let response = self.signed_json(Method::POST, &path, &serde_json::json!({ "domain": domain }))?.send().await?;

        let status = response.status().as_u16();
        log::debug!(target: "seta", "Response status: {}", status);

        if matches!(status, 200 | 201) {
            return Ok(());
        }
        // A 409: the domain is claimed by another site.
        if status == 409 {
            log::warn!(target: "seta", "Custom domain {} is already claimed by another site (409)", domain);
        }
        Err(SetaError::from_failed_response(response).await)
    }
}
