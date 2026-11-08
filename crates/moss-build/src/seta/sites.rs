//! Site registration, metadata, publish/push, and analytics endpoints for moss-seta.

use super::client::NOT_OWNER;
use bytes::Bytes;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use super::client::{MossSetaClient, SetaError};

// Sizing policy (request size, concurrency, timeouts) lives in `upload_policy`,
// pure arithmetic tested without a server; the chunk loop is `chunked_upload`.
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
///   { "need": string[], "remove": string[], "server_total": number, "internal_remove"?: number }
/// `remove` excludes `_moss/` internal derived assets on new setas (filtered
/// at the server's response boundary); it is informational only — actual file
/// removal happens implicitly via the server's generation swap at commit.
/// `internal_remove` (optional) counts the `_moss/` paths that filter dropped.
#[derive(Debug, Deserialize, Serialize)]
pub struct SyncManifestResponse {
    pub need: Vec<String>,
    pub remove: Vec<String>,
    /// TOTAL file count of the server's live manifest, UNFILTERED (includes
    /// `_moss/` files). The true denominator for the client's mass-removal
    /// deploy guard. `serde(default)` because older seta servers omit it.
    #[serde(default)]
    pub server_total: Option<u64>,
    /// How many `_moss/` paths the server left out of `remove`. They are not
    /// wipe evidence, but a publish whose only change is one of them
    /// vanishing still has to commit. Absent on older servers, which counts
    /// as 0.
    #[serde(default)]
    pub internal_remove: Option<u64>,
}

impl SyncManifestResponse {
    /// Whether this publish changes anything the new generation must carry:
    /// files to upload, content to remove, or theme and internal files that
    /// disappeared. When false the live site already matches.
    pub fn needs_commit(&self) -> bool {
        !self.need.is_empty() || !self.remove.is_empty() || self.internal_remove.unwrap_or(0) > 0
    }
}

/// Response from POST /api/sites/{id}/commit (finalize sync).
#[derive(Debug, Deserialize, Serialize)]
pub struct CommitResponse {
    pub url: String,
    /// Despite the name, the number of files in the site after this publish,
    /// changed or not. It is not how many files this publish transferred; the
    /// client counts those itself from the manifest comparison.
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

/// The owner-gated `GET /api/sites/{id}/generation`, classified once, from
/// [`MossSetaClient::generation`].
#[derive(Debug, PartialEq)]
pub enum Generation {
    /// This key's account owns the site.
    Live(LiveGeneration),
    /// seta's own 403: the key belongs to another account, or to none.
    NotOwner,
    /// The owner's key, not signed in to this site, named by its address.
    NotSignedIn { site: String },
    /// A site the server does not have, or a server too old to have the route.
    NotFound,
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
    /// Daily visitor code, a hash the server computes per reader and day.
    /// Rows written before the server carried it have none.
    pub visitor: Option<String>,
    #[serde(rename = "utmCampaign")]
    pub utm_campaign: Option<String>,
    /// `pageview`, `click`, `engage`, `out` or `download`. Absent from servers
    /// older than row kinds; readers then take a null `event` as a pageview.
    pub kind: Option<String>,
    /// Engage rows only: deepest quarter of the page reached (25 to 100).
    pub scroll: Option<u8>,
    /// Engage rows only: time on screen as a range, such as `10-30s`.
    pub visible: Option<String>,
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
        self.post_json("/api/sites", &serde_json::json!({ "id": site_id })).await
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
        let path = format!("/api/sites/{}/sync", urlencoding::encode(site_id));

        log::debug!(target: "seta", "POST {}{} ({} files in manifest) generation={}", self.base_url, path, manifest.len(), generation_id);

        // Serialized ONCE, outside the retry, for two reasons: a retry then
        // re-sends the identical buffer instead of re-encoding a 10k-entry map,
        // and the exact body length is what sets the deadline below.
        let body = Bytes::from(
            serde_json::to_vec(&serde_json::json!({ "files": manifest }))
                .map_err(|e| SetaError::Parse(e.to_string()))?,
        );
        // The deadline scales with the body (I1). A flat 150 s here was a
        // deadline on the SITE'S SIZE: a 10k-file manifest cannot be POSTed in
        // 150 s on a slow uplink, so that site could never publish at any
        // bandwidth. See `upload_policy::manifest_request_timeout`.
        let timeout = manifest_request_timeout(body.len() as u64);

        // Retried with an upload's budget: a large site's manifest is a
        // sizeable body on a slow uplink, and one edge blip here would fail the
        // publish before any content moved. Each attempt signs afresh.
        let budget = crate::seta::client::RetryBudget::new();
        let (this, path_ref, body_ref) = (self, path.as_str(), &body);
        let response = crate::seta::client::retry_transient(
            "sync manifest",
            &budget,
            || async move {
                let resp = this
                    .signed(Method::POST, path_ref, body_ref.clone())?
                    .timeout(timeout)
                    .header("Content-Type", "application/json")
                    .header("X-Moss-Generation", generation_id)
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
    ///   attempts for the upload summary. `None` from contexts with no
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
            urlencoding::encode(site_id),
            urlencoding::encode(file_path),
        );
        log::debug!(target: "seta", "PUT {}{} ({} bytes) generation={}", self.base_url, path, body.len(), generation_id);

        // `Bytes` so each retry re-sends the same buffer without copying it.
        // Each attempt signs afresh: a signed header is good for one request.
        let body = Bytes::from(body);
        let (this, path, body) = (self, path.as_str(), &body);
        let budget = crate::seta::client::RetryBudget::new();
        crate::seta::client::retry_transient(file_path, &budget, || async move {
            let request = this.signed(Method::PUT, path, body.clone())?;
            if let Some(tp) = stats {
                tp.note_request();
            }
            let response = request
                .timeout(UPLOAD_REQUEST_TIMEOUT)
                .header("Content-Type", "application/octet-stream")
                .header("X-Moss-Generation", generation_id)
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
            urlencoding::encode(site_id),
            urlencoding::encode(file_path),
        );
        log::debug!(target: "seta", "PUT {}{} (symlink → {}) generation={}", self.base_url, path, target, generation_id);

        let target = Bytes::from(target.into_bytes());
        let (this, path, target) = (self, path.as_str(), &target);
        let budget = crate::seta::client::RetryBudget::new();
        crate::seta::client::retry_transient(file_path, &budget, || async move {
            let response = this
                .signed(Method::PUT, path, target.clone())?
                // Stated, not inherited: the retry budget reserves
                // UPLOAD_REQUEST_TIMEOUT per attempt (`client::MAX_ATTEMPT_DURATION`).
                .timeout(UPLOAD_REQUEST_TIMEOUT)
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("X-Moss-Entry", crate::types::content::MODE_SYMLINK)
                .header("X-Moss-Generation", generation_id)
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
            urlencoding::encode(site_id),
            urlencoding::encode(generation_id),
        );
        let response = self.signed(Method::GET, &path, Bytes::new())?.timeout(UPLOAD_REQUEST_TIMEOUT).send().await?;
        if response.status().as_u16() == 404 {
            log::debug!(target: "seta", "GET {} → 404: server predates the resume endpoint, uploading from zero", path);
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
        let path = format!("/api/sites/{}/commit", urlencoding::encode(site_id));

        log::debug!(target: "seta", "POST {}{} ({} files)", self.base_url, path, manifest.len());

        // Retried for the same reason as sync_manifest: an edge blip on the
        // commit is the most expensive possible moment to fail, because every
        // byte has already been uploaded. 5xx and 429 are transient; the 422
        // completeness gate is a 4xx and still fails fast, which is correct —
        // retrying an incomplete generation cannot make it complete.
        // Same shape as sync_manifest: serialize once, and let the deadline
        // scale with the body rather than with nothing (I1).
        let body = Bytes::from(
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
                let resp = this
                    .signed(Method::POST, path_ref, body_ref.clone())?
                    .timeout(timeout)
                    .header("Content-Type", "application/json")
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
        match self.generation(site_id, None).await? {
            Generation::Live(live) => Ok(live),
            Generation::NotFound => Ok(LiveGeneration::default()),
            Generation::NotOwner => Err(SetaError::Api { status: 403, message: NOT_OWNER.to_string() }),
            Generation::NotSignedIn { site } => Err(SetaError::NotSignedIn { site }),
        }
    }

    /// The owner-gated read every deploy starts with. Only seta's own 403,
    /// in its own words, means another owner: a 401 (a clock or signing
    /// fault), an edge challenge or a proxy's 403 is not an answer, so each
    /// stays an error.
    pub async fn generation(&self, site_id: &str, timeout: Option<std::time::Duration>) -> Result<Generation, SetaError> {
        let path = format!("/api/sites/{}/generation", urlencoding::encode(site_id));
        let mut request = self.signed(Method::GET, &path, Bytes::new())?;
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request.send().await?;
        match response.status().as_u16() {
            200..=299 => Ok(Generation::Live(response.json().await.map_err(|e| SetaError::Parse(e.to_string()))?)),
            404 => Ok(Generation::NotFound),
            _ => match SetaError::from_failed_response(response).await {
                SetaError::Api { status: 403, message } if message == NOT_OWNER => Ok(Generation::NotOwner),
                SetaError::NotSignedIn { site } => Ok(Generation::NotSignedIn { site }),
                other => Err(other),
            },
        }
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

        let resp: DomainsResponse = self.get_json(&format!("/api/sites/{}/domains", urlencoding::encode(site_id))).await?;
        Ok(resp.domains.into_iter().map(|d| d.domain).collect())
    }

    /// Pull runtime data (comments, analytics) from the site.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    pub async fn pull_site_data(&self, site_id: &str) -> Result<SiteDataResponse, SetaError> {
        self.get_json(&format!("/api/sites/{}/data", urlencoding::encode(site_id))).await
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
        self.get_json(&format!("/api/sites/{}/events?since={}&limit={}", urlencoding::encode(site_id), since, limit)).await
    }
}

#[cfg(test)]
#[path = "sites_tests.rs"]
mod tests;
