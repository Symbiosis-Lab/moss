//! Handing a site to another account on mosspub.com hosting: the owner's
//! request, which mails the receiver one link, and its cancel.

use bytes::Bytes;
use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::client::{MossSetaClient, SetaError};

/// What a transfer moves, as the service planned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferPlan {
    pub site_id: String,
    pub from: String,
    pub to: String,
    /// Linked domains registered elsewhere, which move with the site.
    pub domains: Vec<String>,
    pub subscription: Option<TransferSubscription>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferSubscription {
    pub plan: String,
    pub origin: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransferResponse {
    pub plan: TransferPlan,
    /// Unix seconds when the mailed link stops working; absent on a dry run.
    #[serde(default)]
    pub expires_at: Option<i64>,
}

impl MossSetaClient {
    /// Ask to transfer `site_id` to `to`; with `dry_run`, only plan it. With
    /// `keep_publishing` the receiver may also take this folder's key, so the
    /// folder keeps publishing as them.
    pub async fn request_transfer(
        &self,
        site_id: &str,
        to: &str,
        dry_run: bool,
        keep_publishing: bool,
    ) -> Result<TransferResponse, SetaError> {
        let body = serde_json::json!({
            "to": to,
            "dry_run": dry_run,
            "keep_publishing": keep_publishing,
            "client": { "name": "moss", "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS },
        });
        Self::send_json(self.signed_json(Method::POST, &format!("/api/sites/{}/transfer", site_id), &body)?).await
    }

    /// Withdraw the site's pending transfer; its mailed link stops working.
    pub async fn cancel_transfer(&self, site_id: &str) -> Result<(), SetaError> {
        let path = format!("/api/sites/{}/transfer", site_id);
        let response = self.signed(Method::DELETE, &path, Bytes::new())?.send().await?;
        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }
        Ok(())
    }
}
