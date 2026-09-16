//! DNS record CRUD endpoints for moss-seta.
//!
//! Split out of the app-side `domain/moss_seta_client.rs` (since deleted) per
//! docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 7.

use serde::{Deserialize, Serialize};
use super::client::{MossSetaClient, SetaError};

/// Response from DNS zone GET endpoint.
#[derive(Debug, Deserialize, Serialize, specta::Type)]
pub struct DnsZoneResponse {
    pub domain: String,
    pub records: Vec<DnsZoneRecord>,
}

/// A DNS record returned by the zone GET endpoint.
#[derive(Debug, Deserialize, Serialize, specta::Type)]
pub struct DnsZoneRecord {
    #[serde(rename = "type")]
    pub record_type: String,
    pub subdomain: String,
    pub value: String,
    pub priority: Option<i32>,
}

/// Response from DNS configuration endpoint.
#[derive(Debug, Deserialize, Serialize)]
pub struct DnsConfigureResponse {
    pub success: bool,
    pub domain: String,
    pub records_set: u32,
    pub verified_records: Option<Vec<VerifiedDnsRecord>>,
}

/// A verified DNS record returned by the API after configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct VerifiedDnsRecord {
    #[serde(rename = "type")]
    pub record_type: String,
    pub subdomain: String,
    pub value: String,
}

impl MossSetaClient {
    /// Configure DNS records for a domain via moss-seta.
    ///
    /// Sends DNS records to the backend which configures them via OpenSRS
    /// and verifies the configuration by reading them back.
    ///
    /// # Arguments
    /// * `domain` - The domain to configure DNS for
    /// * `records` - DNS records to set (A, CNAME, etc.)
    pub async fn configure_dns(
        &self,
        domain: &str,
        records: &[crate::config::deployment::DnsRecord],
    ) -> Result<DnsConfigureResponse, SetaError> {
        let path = format!("/api/domains/{}/dns", domain);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "POST {} domain={} records={}", url, domain, records.len());

        // Convert domain DnsRecord to API format
        let api_records: Vec<serde_json::Value> = records.iter().map(|r| {
            serde_json::json!({
                "type": r.record_type,
                "subdomain": r.name,
                "value": r.value,
            })
        }).collect();

        let response = self.client
            .post(&url)
            .header("Authorization", auth_header)
            .json(&serde_json::json!({ "records": api_records }))
            .send()
            .await?;

        log::debug!(target: "seta", "Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Get current DNS zone records for a domain.
    ///
    /// Fetches the live DNS records from OpenSRS via moss-seta.
    /// Requires domain ownership verification (authenticated endpoint).
    ///
    /// # Arguments
    /// * `domain` - The domain to fetch DNS records for
    pub async fn get_dns_zone(&self, domain: &str) -> Result<DnsZoneResponse, SetaError> {
        let path = format!("/api/domains/{}/dns", domain);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} domain={}", url, domain);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        log::debug!(target: "seta", "Response status: {}", response.status());

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }
}
