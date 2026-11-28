//! seta's deploy resource: open a deploy with the manifest, send the files it
//! needs, commit it if no send did.
//!
//!   POST /api/sites/:id/deploys                  manifest
//!   POST /api/sites/:id/deploys/:deploy/files    files
//!   POST /api/sites/:id/deploys/:deploy/commit   nothing
//!
//! Each answers one shape, [`DeployAnswer`]: `live`, or `open` with what is
//! still needed. A refusal is the same coded body the old routes answer, read
//! by `SetaError::from_failed_response`.

use std::collections::HashMap;
use std::io::Write as _;

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use super::client::{MossSetaClient, SetaError};
use super::upload_policy::{manifest_request_timeout, UPLOAD_REQUEST_TIMEOUT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeployState {
    Live,
    Open,
}

/// One entry the server lacks, and the path to read its bytes from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Need {
    pub entry: String,
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Limits {
    /// The largest file a send takes as a part; a larger one goes chunked.
    pub request_bytes: u64,
    pub concurrency_hint: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeployAnswer {
    /// The deploy id: the generation this manifest names.
    pub deploy: String,
    pub state: DeployState,
    #[serde(default)]
    pub needed: Vec<Need>,
    pub limits: Option<Limits>,
    pub url: Option<String>,
    /// The number of files in the site after this publish, on a commit.
    pub files_updated: Option<u32>,
    pub files_removed: Option<u32>,
    pub timestamp: Option<u64>,
    pub live_generation: Option<String>,
}

/// A part's form-data name is its manifest entry; the filename is required
/// for the server to read it as bytes.
fn multipart(parts: &[(&str, &str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = format!("moss-{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::new();
    for (name, content_type, bytes) in parts {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"f\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Open's manifest part: gzip JSON `{ files, sizes, gated?, allow_removal? }`,
/// `sizes` keyed by entry.
fn manifest_gz(files: &HashMap<String, String>, sizes: &HashMap<String, u64>, allow_removal: bool) -> Result<Vec<u8>, SetaError> {
    let by_entry: HashMap<&str, u64> = files.iter().filter_map(|(path, entry)| Some((entry.as_str(), *sizes.get(path)?))).collect();
    let mut json = serde_json::json!({ "files": files, "sizes": by_entry });
    let gated = crate::build::emit::members::gated_entries(files);
    if !gated.is_empty() {
        json["gated"] = serde_json::json!(gated);
    }
    if allow_removal {
        json["allow_removal"] = serde_json::json!(true);
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(json.to_string().as_bytes()).and_then(|()| gz.finish()).map_err(|e| SetaError::Io(e.to_string()))
}

impl MossSetaClient {
    /// Opens the deploy `files` names. Opening it again is the resume.
    pub async fn open_deploy(
        &self,
        site_id: &str,
        files: &HashMap<String, String>,
        sizes: &HashMap<String, u64>,
        allow_removal: bool,
    ) -> Result<DeployAnswer, SetaError> {
        let manifest = manifest_gz(files, sizes, allow_removal)?;
        let (content_type, body) = multipart(&[("manifest", "application/gzip", &manifest)]);
        let path = format!("/api/sites/{}/deploys", urlencoding::encode(site_id));
        log::debug!(target: "seta", "POST {}{} ({} files, {} bytes)", self.base_url, path, files.len(), body.len());
        let timeout = manifest_request_timeout(body.len() as u64);
        self.post_deploy("open deploy", &path, content_type, body.into(), timeout).await
    }

    /// Sends `parts` (entry, bytes) into an open deploy. The answer is live when
    /// they were the last it needed.
    pub async fn send_to_deploy(&self, site_id: &str, deploy: &str, parts: &[(String, Vec<u8>)]) -> Result<DeployAnswer, SetaError> {
        let named: Vec<(&str, &str, &[u8])> = parts.iter().map(|(entry, bytes)| (entry.as_str(), "application/octet-stream", bytes.as_slice())).collect();
        let (content_type, body) = multipart(&named);
        let path = format!("/api/sites/{}/deploys/{}/files", urlencoding::encode(site_id), urlencoding::encode(deploy));
        log::debug!(target: "seta", "POST {}{} ({} files, {} bytes)", self.base_url, path, parts.len(), body.len());
        self.post_deploy("send to deploy", &path, content_type, body.into(), UPLOAD_REQUEST_TIMEOUT).await
    }

    /// Commits a deploy no send saw complete: its last piece went chunked or
    /// as a link.
    pub async fn commit_deploy(&self, site_id: &str, deploy: &str) -> Result<DeployAnswer, SetaError> {
        let path = format!("/api/sites/{}/deploys/{}/commit", urlencoding::encode(site_id), urlencoding::encode(deploy));
        self.post_deploy("commit deploy", &path, String::new(), Bytes::new(), UPLOAD_REQUEST_TIMEOUT).await
    }

    async fn post_deploy(&self, what: &str, path: &str, content_type: String, body: Bytes, timeout: std::time::Duration) -> Result<DeployAnswer, SetaError> {
        let budget = super::client::RetryBudget::new();
        let (path, body, content_type) = (path, &body, content_type.as_str());
        let response = super::client::retry_transient(what, &budget, || async move {
            let mut request = self.signed(Method::POST, path, body.clone())?.timeout(timeout);
            if !content_type.is_empty() {
                request = request.header("Content-Type", content_type);
            }
            let response = self.send(request).await?;
            if !response.status().is_success() {
                return Err(SetaError::from_failed_response(response).await);
            }
            Ok(response)
        })
        .await?;
        response.json().await.map_err(|e| SetaError::Parse(e.to_string()))
    }
}
