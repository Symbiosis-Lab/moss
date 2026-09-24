//! Site registration, metadata, publish/push, and analytics endpoints for moss-seta.
//!
//! Split out of the app-side `domain/moss_seta_client.rs` (since deleted) per
//! docs/archive/2026-04-24-codebase-restructure-continuation-plan.md Task 7.

use serde::{Deserialize, Serialize};
use super::client::{MossSetaClient, SetaError};

// Sizing policy — how big a request may be, how many may be in flight, and how
// to react to a timeout — lives in `upload_policy`. It is pure arithmetic over
// numbers, and it is the whole substance of the 2026-08-03 publish failure, so
// it is kept where it can be read and unit-tested without a TCP harness. The
// chunk loop that consumes the rest of those numbers lives in
// `chunked_upload`; this file only needs the per-request ceiling.
use super::upload_policy::{manifest_request_timeout, UPLOAD_REQUEST_TIMEOUT};

/// Response from POST /api/sites (register a site).
///
/// The server returns TWO shapes from this endpoint:
///   - New site (201):        `{ "id": "...", "url": "..." }`
///   - Already-owned (200):   `{ "site_id": "..." }`  (idempotent re-register)
/// Accept both: `site_id` deserializes from either `id` or `site_id`, and `url`
/// is optional (absent on the idempotent path). Without this, re-registering an
/// owned site fails with "error decoding response body".
#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterSiteResponse {
    #[serde(rename = "id", alias = "site_id")]
    pub site_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// Response from POST /api/sites/{id}/sync (manifest diff).
///
/// Wire contract (pinned across moss/moss-seta, 2026-07-22):
///   { "need": string[], "remove": string[], "server_total": number }
/// `remove` excludes `_moss/` internal derived assets on new setas (filtered
/// at the server's response boundary); it is informational only — actual file
/// removal happens implicitly via the server's generation swap at commit.
#[derive(Debug, Deserialize, Serialize)]
pub struct SyncManifestResponse {
    pub need: Vec<String>,
    pub remove: Vec<String>,
    /// TOTAL file count of the server's live manifest, UNFILTERED (includes
    /// `_moss/` files). The true denominator for the client's mass-removal
    /// deploy guard. `serde(default)` because older seta servers omit it.
    #[serde(default)]
    pub server_total: Option<u64>,
}

/// Response from POST /api/sites/{id}/commit (finalize sync).
#[derive(Debug, Deserialize, Serialize)]
pub struct CommitResponse {
    pub url: String,
    pub files_updated: u32,
    pub files_removed: u32,
    /// Server timestamp (Unix seconds) of the deployment.
    pub timestamp: u64,
    /// The generation_id now live on the server after this commit.
    ///
    /// Echoed back from the server so the client can detect deploy skew
    /// (client published generation A but server is now live on B — possible
    /// if two deploys raced). Uses `serde(default)` so an older seta server
    /// that omits this field still deserializes cleanly.
    #[serde(default)]
    pub generation_id: Option<String>,
}

/// Response from GET /api/sites/{id}/generation: which generation the site is
/// serving, and when it went live. Both `None` for a site that has never
/// deployed — and for an old server with no such endpoint, which answers 404.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct LiveGeneration {
    pub generation_id: Option<String>,
    /// Server clock, Unix seconds.
    pub deployed_at: Option<i64>,
}

/// One live chunked-upload session, from GET /api/sites/{id}/uploads.
///
/// Wire contract (moss-seta `listUploadSessions`): `offset` is the byte count
/// the server actually holds in the `.partial`, and `size` is the length the
/// session was created for — the client must check `size` against the local
/// file before adopting a session, or it would resume a *different* file that
/// happens to share a path.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadSession {
    #[serde(rename = "uploadId")]
    pub upload_id: String,
    #[serde(rename = "filePath")]
    pub file_path: String,
    pub size: u64,
    pub offset: u64,
}

/// Response from GET /api/sites/{id}/data (pull runtime data).
///
/// Only the `subscribers` field is consumed by the client — analytics and
/// comments come from dedicated sync paths (`events.jsonl` and Artalk
/// respectively). Unknown fields in the server response are ignored.
#[derive(Debug, Deserialize)]
pub struct SiteDataResponse {
    pub subscribers: Option<Vec<String>>,
}

/// A single raw analytics event row returned by GET /api/sites/{id}/events.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventRow {
    pub id: u64,
    pub ts: u64,
    pub path: String,
    pub referrer: Option<String>,
    pub country: Option<String>,
    pub browser: Option<String>,
    pub os: Option<String>,
    pub screen: Option<String>,
    #[serde(rename = "utmSource")]
    pub utm_source: Option<String>,
    #[serde(rename = "utmMedium")]
    pub utm_medium: Option<String>,
    pub event: Option<String>,
}

/// Response from GET /api/sites/{id}/events (batch of raw events for delta sync).
#[derive(Debug, Clone, Deserialize)]
pub struct EventsBatchResponse {
    pub events: Vec<EventRow>,
    pub cursor: u64,
    pub more: bool,
}

impl MossSetaClient {
    // ========== Site Hosting (Public) Endpoints ==========

    /// Check if a site ID is available on mosspub.com (public, no auth required).
    pub async fn check_site_id_available(&self, site_id: &str) -> Result<bool, SetaError> {
        let url = format!("{}/api/sites/{}/available", self.base_url, site_id);

        log::debug!(target: "seta", "GET {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Ok(false);
        }

        #[derive(Deserialize)]
        struct AvailabilityResponse {
            available: bool,
        }

        let body: AvailabilityResponse = response
            .json()
            .await
            .map_err(|e| SetaError::Parse(e.to_string()))?;
        Ok(body.available)
    }

    // ========== Site Hosting (Authenticated) Endpoints ==========

    /// Register a new site with mosspub.com.
    ///
    /// # Arguments
    /// * `site_id` - Desired site identifier (e.g., "her-blog" → her-blog.mosspub.com)
    pub async fn register_site(&self, site_id: &str) -> Result<RegisterSiteResponse, SetaError> {
        let auth_header = self.sign_request_payload("/api/sites")?;
        let url = self.secure_url("/api/sites");

        log::debug!(target: "seta", "POST {} site_id={}", url, site_id);

        let response = self.client
            .post(&url)
            .header("Authorization", auth_header)
            .json(&serde_json::json!({ "id": site_id }))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Send manifest (path -> hash) to server, receive list of files to upload/remove.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    /// * `manifest` - Map of relative file paths to content hashes
    /// * `generation_id` - The deploy generation being synced; sent as `X-Moss-Generation`
    ///   so the server scopes the diff to the correct generation directory.
    pub async fn sync_manifest(
        &self,
        site_id: &str,
        manifest: &std::collections::HashMap<String, String>,
        generation_id: &str,
    ) -> Result<SyncManifestResponse, SetaError> {
        let path = format!("/api/sites/{}/sync", site_id);

        log::debug!(target: "seta", "POST {}{} ({} files in manifest) generation={}", self.base_url, path, manifest.len(), generation_id);

        // Serialized ONCE, outside the retry, for two reasons: a retry then
        // re-sends the identical buffer instead of re-encoding a 10k-entry map,
        // and the exact body length is what sets the deadline below.
        let body = bytes::Bytes::from(
            serde_json::to_vec(&serde_json::json!({ "files": manifest }))
                .map_err(|e| SetaError::Parse(e.to_string()))?,
        );
        // The deadline scales with the body (I1). A flat 150 s here was a
        // deadline on the SITE'S SIZE: a 10k-file manifest cannot be POSTed in
        // 150 s on a slow uplink, so that site could never publish at any
        // bandwidth. See `upload_policy::manifest_request_timeout`.
        let timeout = manifest_request_timeout(body.len() as u64);

        // Retried, with the same budget discipline as an upload. A large site's
        // manifest POST is itself a sizeable body on a slow uplink, and until
        // 2026-08-03 this call had NO retry and only the 120s default timeout —
        // so a single edge blip here failed the publish before a byte of
        // content had moved. The closure re-signs every attempt (seta rejects
        // clock skew over 300s), which is why the whole sign-and-send unit is
        // inside it.
        let budget = crate::seta::client::RetryBudget::new();
        let (this, path_ref, body_ref) = (self, path.as_str(), &body);
        let response = crate::seta::client::retry_transient(
            "sync manifest",
            &budget,
            || async move {
                let (url, auth_header) = this.sign_and_build(path_ref)?;
                let resp = this
                    .client
                    .post(&url)
                    .timeout(timeout)
                    .header("Authorization", auth_header)
                    .header("Content-Type", "application/json")
                    .header("X-Moss-Generation", generation_id)
                    .body(body_ref.clone())
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    return Err(SetaError::from_failed_response(resp).await);
                }
                Ok(resp)
            },
        )
        .await?;

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Upload a single file to the site.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    /// * `file_path` - Relative path within the site (e.g., "index.html")
    /// * `body` - Raw file contents
    /// * `generation_id` - The deploy generation being uploaded; sent as `X-Moss-Generation`
    ///   so the server writes the file into the correct generation directory.
    /// * `stats` - The deploy's shared [`upload_policy::Throughput`], counting
    ///   attempts for the upload summary (#1133). `None` from contexts with no
    ///   deploy-level accounting (tests).
    pub async fn upload_file(
        &self,
        site_id: &str,
        file_path: &str,
        body: Vec<u8>,
        generation_id: &str,
        stats: Option<&crate::seta::upload_policy::Throughput>,
    ) -> Result<(), SetaError> {
        let path = format!(
            "/api/sites/{}/files/{}",
            site_id,
            urlencoding::encode(file_path),
        );
        log::debug!(target: "seta", "PUT {}{} ({} bytes) generation={}", self.base_url, path, body.len(), generation_id);

        // `Bytes` so each retry re-sends the same buffer without copying it.
        // Signing happens *inside* the retried closure: the header signs a
        // timestamp seta rejects past 300s of skew, so a retry must re-sign.
        let body = bytes::Bytes::from(body);
        let (this, path, body) = (self, path.as_str(), &body);
        let budget = crate::seta::client::RetryBudget::new();
        crate::seta::client::retry_transient(file_path, &budget, || async move {
            let (url, auth_header) = this.sign_and_build(path)?;
            if let Some(tp) = stats {
                tp.note_request();
            }
            let response = this.client
                .put(&url)
                .timeout(UPLOAD_REQUEST_TIMEOUT)
                .header("Authorization", auth_header)
                .header("Content-Type", "application/octet-stream")
                .header("X-Moss-Generation", generation_id)
                .body(body.clone())
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(SetaError::from_failed_response(response).await);
            }
            if let Some(tp) = stats {
                tp.note_request_ok();
            }
            Ok(())
        })
        .await
    }

    /// Upload a symlink entry to the site.
    ///
    /// The symlink is encoded as a regular PUT to `/api/sites/:id/files/<path>`
    /// with header `X-Moss-Entry: 120000` and the target path as the body.
    /// Server-side `writeSymlink` validates target containment and creates a
    /// real POSIX symlink at the link's path. Caddy serves it at request time
    /// (it follows symlinks by default in v2.7+).
    ///
    /// See `docs/reference/deploy-upload-contract.md` for the full
    /// wire-format contract.
    ///
    /// # Arguments
    /// * `generation_id` - The deploy generation being uploaded; sent as `X-Moss-Generation`
    ///   so the server writes the symlink into the correct generation directory.
    pub async fn upload_symlink(
        &self,
        site_id: &str,
        file_path: &str,
        target: String,
        generation_id: &str,
    ) -> Result<(), SetaError> {
        let path = format!(
            "/api/sites/{}/files/{}",
            site_id,
            urlencoding::encode(file_path),
        );
        log::debug!(target: "seta", "PUT {}{} (symlink → {}) generation={}", self.base_url, path, target, generation_id);

        let target = bytes::Bytes::from(target.into_bytes());
        let (this, path, target) = (self, path.as_str(), &target);
        let budget = crate::seta::client::RetryBudget::new();
        crate::seta::client::retry_transient(file_path, &budget, || async move {
            let (url, auth_header) = this.sign_and_build(path)?;
            let response = this.client
                .put(&url)
                // Explicit, not inherited. Every other upload verb overrides
                // the client's 120 s default, and `client::MAX_ATTEMPT_DURATION`
                // — the headroom the retry budget reserves — is defined as
                // UPLOAD_REQUEST_TIMEOUT. Riding the default here made this the
                // one verb whose worst case the budget arithmetic did not
                // describe. The body is a path string, so the value is
                // irrelevant to throughput; being stated is the point.
                .timeout(UPLOAD_REQUEST_TIMEOUT)
                .header("Authorization", auth_header)
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("X-Moss-Entry", crate::types::content::MODE_SYMLINK)
                .header("X-Moss-Generation", generation_id)
                .body(target.clone())
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(SetaError::from_failed_response(response).await);
            }
            Ok(())
        })
        .await
    }


    /// Ask the server which chunked-upload sessions it already holds for this
    /// generation.
    ///
    /// `GET /api/sites/:id/uploads?generation_id=<16hex>` →
    /// `200 [{uploadId, filePath, size, offset}]`. This is invariant I2 in one
    /// call: **the server is the sole authority on what it already has**, so
    /// the client persists no durable deploy state and a retried publish
    /// resumes each large file where the last attempt stopped instead of
    /// restarting it at byte 0.
    ///
    /// # A 404 is "no sessions", never an error
    ///
    /// An older seta has no such route, and Hono answers with a 404 that
    /// `from_failed_response` would turn into a terminal `SetaError::Api`
    /// (`is_transient` excludes 404). Without the explicit arm below, a new
    /// client would hard-fail **every publish** against a server that has not
    /// been upgraded yet. The new server never 404s this route — "nothing
    /// staged" is `200 []` — precisely so the two cases stay distinguishable.
    ///
    /// Any other failure is also non-fatal to the caller by construction:
    /// resume may only ever remove work, so a caller that cannot get the list
    /// simply uploads from zero.
    pub async fn list_upload_sessions(
        &self,
        site_id: &str,
        generation_id: &str,
    ) -> Result<Vec<UploadSession>, SetaError> {
        let path = format!(
            "/api/sites/{}/uploads?generation_id={}",
            site_id,
            urlencoding::encode(generation_id),
        );
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} (resumable upload sessions)", url);

        let response = self.client
            .get(&url)
            .timeout(UPLOAD_REQUEST_TIMEOUT)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if response.status().as_u16() == 404 {
            log::debug!(
                target: "seta",
                "GET {} → 404: server predates the resume endpoint, uploading from zero",
                url
            );
            return Ok(Vec::new());
        }

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Commit (finalize) the sync, making uploaded files live.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    /// * `manifest` - The final manifest (path → hash) to commit
    pub async fn commit_sync(
        &self,
        site_id: &str,
        manifest: &std::collections::HashMap<String, String>,
        generation_id: &str,
    ) -> Result<CommitResponse, SetaError> {
        let path = format!("/api/sites/{}/commit", site_id);

        log::debug!(target: "seta", "POST {}{} ({} files)", self.base_url, path, manifest.len());

        // Retried for the same reason as sync_manifest: an edge blip on the
        // commit is the most expensive possible moment to fail, because every
        // byte has already been uploaded. 5xx and 429 are transient; the 422
        // completeness gate is a 4xx and still fails fast, which is correct —
        // retrying an incomplete generation cannot make it complete.
        // Same shape as sync_manifest: serialize once, and let the deadline
        // scale with the body rather than with nothing (I1).
        let body = bytes::Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "manifest": manifest,
                "generation_id": generation_id,
            }))
            .map_err(|e| SetaError::Parse(e.to_string()))?,
        );
        let timeout = manifest_request_timeout(body.len() as u64);

        let budget = crate::seta::client::RetryBudget::new();
        let (this, path_ref, body_ref) = (self, path.as_str(), &body);
        let response = crate::seta::client::retry_transient(
            "commit deploy",
            &budget,
            || async move {
                let (url, auth_header) = this.sign_and_build(path_ref)?;
                let resp = this
                    .client
                    .post(&url)
                    .timeout(timeout)
                    .header("Authorization", auth_header)
                    .header("Content-Type", "application/json")
                    .body(body_ref.clone())
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    return Err(SetaError::from_failed_response(resp).await);
                }
                Ok(resp)
            },
        )
        .await?;

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Returns the server's currently-live generation id, or None when the site
    /// has no live generation (never deployed) OR the server doesn't support the
    /// endpoint (old seta → 404). Used to short-circuit a no-op redeploy.
    ///
    /// On 404 → `Ok(None)` (old server, no endpoint).
    /// On 2xx → parse and return `Ok(resp.generation_id)` (which is itself
    ///   `None` for a never-deployed site).
    /// On other non-2xx → `Err(SetaError)` (caller treats any Err as "proceed").
    pub async fn get_live_generation(&self, site_id: &str) -> Result<Option<String>, SetaError> {
        Ok(self.live_generation(site_id).await?.generation_id)
    }

    /// [`Self::get_live_generation`] with the time the generation went live,
    /// which `moss deploy` compares with this folder's last publish. 404 is
    /// `LiveGeneration::default()` — nothing known, for the same reason.
    pub async fn live_generation(&self, site_id: &str) -> Result<LiveGeneration, SetaError> {
        let path = format!("/api/sites/{}/generation", site_id);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} (live generation check)", url);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        // 404 means the server doesn't support this endpoint yet (old seta).
        // Treat as "no live generation known" → proceed with normal deploy.
        if response.status().as_u16() == 404 {
            log::debug!(target: "seta", "GET {} → 404, server does not support /generation endpoint", url);
            return Ok(LiveGeneration::default());
        }

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// The custom-domain hostnames seta holds for this site
    /// (`GET /api/sites/{id}/domains`). Feeds the local
    /// `[deployment].observed` record on project open; a site with no linked
    /// domains returns an empty list.
    pub async fn list_site_domains(&self, site_id: &str) -> Result<Vec<String>, SetaError> {
        #[derive(Debug, Deserialize)]
        struct DomainRow {
            domain: String,
        }
        #[derive(Debug, Deserialize)]
        struct DomainsResponse {
            domains: Vec<DomainRow>,
        }

        let path = format!("/api/sites/{}/domains", site_id);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} (custom domains)", url);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        let resp: DomainsResponse = response
            .json()
            .await
            .map_err(|e| SetaError::Parse(e.to_string()))?;
        Ok(resp.domains.into_iter().map(|d| d.domain).collect())
    }

    /// Pull runtime data (comments, analytics) from the site.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    pub async fn pull_site_data(&self, site_id: &str) -> Result<SiteDataResponse, SetaError> {
        let path = format!("/api/sites/{}/data", site_id);
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {}", url);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// Pull a single page of raw analytics events for delta sync.
    ///
    /// Calls `GET /api/sites/{id}/events?since={cursor}&limit={limit}` with BIP-340
    /// authentication. Signed over the full path including query parameters.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    /// * `since` - Cursor (event ID) — fetch events with id > since
    /// * `limit` - Maximum number of events to return per page
    pub async fn pull_events(
        &self,
        site_id: &str,
        since: u64,
        limit: u32,
    ) -> Result<EventsBatchResponse, SetaError> {
        let path = format!(
            "/api/sites/{}/events?since={}&limit={}",
            site_id, since, limit
        );
        let (url, auth_header) = self.sign_and_build(&path)?;

        log::debug!(target: "seta", "GET {} (events delta sync since={})", url, since);

        let response = self.client
            .get(&url)
            .header("Authorization", auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }

        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }
}

#[cfg(test)]
#[path = "sites_tests.rs"]
mod tests;
