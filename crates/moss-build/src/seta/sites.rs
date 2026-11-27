//! Site registration, metadata, publish/push, and analytics endpoints for moss-seta.

use super::client::NOT_OWNER;
use bytes::Bytes;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use super::client::{MossSetaClient, SetaError};

// Sizing policy (request size, concurrency, timeouts) lives in `upload_policy`,
// pure arithmetic tested without a server; the chunk loop is `chunked_upload`.
use super::upload_policy::UPLOAD_REQUEST_TIMEOUT;

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

/// One page of a site's records from a read-back route (`?since=&limit=` →
/// `{records, cursor, more}`). Records stay as seta wrote them, so a field
/// it adds reaches the folder without a release of moss. The analytics route
/// names its records `events`; comments carry `answers` for the pending ids asked about.
#[derive(Debug, Clone, Deserialize)]
pub struct RecordsPage {
    #[serde(alias = "events")]
    pub records: Vec<serde_json::Value>,
    pub cursor: u64,
    pub more: bool,
    #[serde(default)]
    pub answers: Vec<serde_json::Value>,
}

impl MossSetaClient {
    // ========== Site Hosting (Public) Endpoints ==========

    /// Check if a site ID is available on mosspub.com (public, no auth required).
    /// `Ok(false)` is the server's own "taken"; any failed request is an `Err`.
    pub async fn check_site_id_available(&self, site_id: &str) -> Result<bool, SetaError> {
        let url = format!("{}/api/sites/{}/available", self.base_url, site_id);

        log::debug!(target: "seta", "GET {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
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

    /// Upload a symlink entry to the site.
    ///
    /// The symlink is encoded as a regular PUT to `/api/sites/:id/files/<path>`
    /// with header `X-Moss-Entry: 120000` and the target path as the body.
    /// Server-side `writeSymlink` validates target containment and creates a
    /// real POSIX symlink at the link's path. Caddy serves it at request time
    /// (it follows symlinks by default in v2.7+).
    ///
    /// # Arguments
    /// * `generation_id` - The deploy id; sent as `X-Moss-Generation` so the
    ///   server stages the link in that deploy.
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
            let request = this
                .signed(Method::PUT, path, target.clone())?
                // Stated, not inherited: the retry budget reserves
                // UPLOAD_REQUEST_TIMEOUT per attempt (`client::MAX_ATTEMPT_DURATION`).
                .timeout(UPLOAD_REQUEST_TIMEOUT)
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("X-Moss-Entry", crate::types::content::MODE_SYMLINK)
                .header("X-Moss-Generation", generation_id);
            let response = this.send(request).await?;
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
        let response = self.send(self.signed(Method::GET, &path, Bytes::new())?.timeout(UPLOAD_REQUEST_TIMEOUT)).await?;
        if response.status().as_u16() == 404 {
            log::debug!(target: "seta", "GET {} → 404: server predates the resume endpoint, uploading from zero", path);
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(SetaError::from_failed_response(response).await);
        }
        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }

    /// The generation the site serves and the time it went live,
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
        let response = self.send(request).await?;
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

    /// A page of the site's analytics events after `since`.
    pub async fn pull_events(&self, site_id: &str, since: u64, limit: u32) -> Result<RecordsPage, SetaError> {
        self.get_json(&format!("/api/sites/{}/events?since={}&limit={}", urlencoding::encode(site_id), since, limit)).await
    }

    /// A page of the site's subscriber stream after `since`: consent,
    /// suppression and form entries, each with its subject's email.
    pub async fn pull_subscriber_events(&self, site_id: &str, since: u64, limit: u32) -> Result<RecordsPage, SetaError> {
        self.get_json(&format!("/api/sites/{}/subscriber-events?since={}&limit={}", urlencoding::encode(site_id), since, limit)).await
    }

    /// A page of the site's request form entries after `since`.
    pub async fn pull_form_entries(&self, site_id: &str, since: u64, limit: u32) -> Result<RecordsPage, SetaError> {
        self.get_json(&format!("/api/sites/{}/form-entries?since={}&limit={}", urlencoding::encode(site_id), since, limit)).await
    }

    /// A page of the site's comments after `since`, with each commenter's
    /// email, and the answers for the `pending` ids (at most 200).
    pub async fn pull_comment_records(&self, site_id: &str, since: u64, limit: u32, pending: &[u64]) -> Result<RecordsPage, SetaError> {
        let ids: Vec<String> = pending.iter().map(u64::to_string).collect();
        let ask = if ids.is_empty() { String::new() } else { format!("&pending={}", ids.join(",")) };
        self.get_json(&format!("/api/sites/{}/comment-records?since={}&limit={}{}", urlencoding::encode(site_id), since, limit, ask)).await
    }

    /// A page of the site's booking changes after `since`, from commerce:
    /// call it on [`MossSetaClient::at`] commerce's address.
    pub async fn pull_bookings(&self, site_id: &str, since: u64, limit: u32) -> Result<RecordsPage, SetaError> {
        self.get_json(&format!("/sites/{}/bookings?since={}&limit={}", urlencoding::encode(site_id), since, limit)).await
    }
}

#[cfg(test)]
#[path = "sites_tests.rs"]
mod tests;
