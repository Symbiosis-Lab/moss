//! Download, binary-execution, and HTTP-fetch helpers.
//!
//! All Tauri commands live in the parent `runtime.rs`; this
//! module holds only the non-command implementation bodies that those commands
//! delegate to, plus the concurrency primitives they share.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use tokio::sync::Semaphore;

use super::portable::{DownloadAssetResult, BinaryExecutionResult, FetchResult};

// ============================================================================
// Download Concurrency Control
// ============================================================================

/// Maximum number of concurrent downloads allowed.
const DOWNLOAD_CONCURRENCY_LIMIT: usize = 5;

/// Global semaphore controlling download concurrency.
pub(super) static DOWNLOAD_SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| {
    Semaphore::new(DOWNLOAD_CONCURRENCY_LIMIT)
});

// ============================================================================
// HTTP Fetch Helpers
// ============================================================================

/// HTTP method variants for `do_http_request`.
pub(super) enum HttpMethod {
    /// Plain GET with no body.
    Get,
    /// POST with a JSON string body; `Content-Type: application/json` is set automatically.
    Post { body: String },
    /// POST with a raw byte body and an explicit `Content-Type` (e.g. a
    /// pre-built `multipart/form-data` body carrying its boundary). Used for
    /// binary uploads the JSON `Post` variant cannot express.
    PostBytes { body: Vec<u8>, content_type: String },
}

/// Shared implementation for `fetch_url`, `http_post`, and `http_get`.
///
/// Validates the URL, defaults the timeout, builds a ureq agent, fires the
/// request, and returns a `FetchResult` with a base64-encoded body.  The
/// `tag` parameter is embedded in log lines so callers can be distinguished
/// in the log stream (`"fetch_url"`, `"http_post"`, `"http_get"`).
pub(super) async fn do_http_request(
    url: String,
    method: HttpMethod,
    headers: Option<std::collections::HashMap<String, String>>,
    timeout_ms: Option<u64>,
    tag: &'static str,
) -> Result<FetchResult, String> {
    use base64::Engine;
    use std::time::Duration;

    if url.is_empty() {
        return Err("URL cannot be empty".to_string());
    }

    let parsed_url: url::Url = url
        .parse()
        .map_err(|e| format!("Invalid URL '{}': {}", url, e))?;

    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err(format!(
            "Invalid URL scheme '{}': only http and https are allowed",
            parsed_url.scheme()
        ));
    }

    let timeout_ms_val = timeout_ms.unwrap_or(30_000);
    let timeout = Duration::from_millis(timeout_ms_val);

    let method_label = match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post { .. } => "POST",
        HttpMethod::PostBytes { .. } => "POST",
    };

    log::debug!("{}: starting {} request to {}", tag, method_label, url);

    let url_clone = url.clone();
    let result = tokio::task::spawn_blocking(move || {
        let agent = crate::system::proxy::proxied_ureq_agent(&url_clone, timeout);

        let mut request = match &method {
            HttpMethod::Get => agent.get(&url_clone),
            HttpMethod::Post { .. } => agent
                .post(&url_clone)
                .set("Content-Type", "application/json"),
            HttpMethod::PostBytes { content_type, .. } => {
                agent.post(&url_clone).set("Content-Type", content_type)
            }
        };

        let method_sets_content_type =
            matches!(method, HttpMethod::Post { .. } | HttpMethod::PostBytes { .. });
        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                // For POST variants the method already set the correct
                // Content-Type (notably the multipart boundary for PostBytes);
                // never let a caller header clobber it. GET has no body, so a
                // caller Content-Type there is harmless and passed through.
                if method_sets_content_type && key.eq_ignore_ascii_case("content-type") {
                    continue;
                }
                request = request.set(&key, &value);
            }
        }

        let response = match method {
            HttpMethod::Get => request.call(),
            HttpMethod::Post { body } => request.send_string(&body),
            HttpMethod::PostBytes { body, .. } => request.send_bytes(&body),
        };

        process_ureq_response(response, &url_clone, timeout_ms_val)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?;

    let (status, ok, content_type, bytes) = result?;
    if ok {
        log::debug!("{}: completed {} ({} bytes)", tag, url, bytes.len());
    } else {
        log::warn!(
            "{}: HTTP {} from {} ({} bytes) body: {}",
            tag,
            status,
            url,
            bytes.len(),
            body_snippet(&bytes, 256)
        );
    }
    let body_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(FetchResult { status, ok, body_base64, content_type })
}

/// Build a `multipart/form-data` request body. Returns the body bytes and the
/// `Content-Type` header value (which carries the generated boundary).
///
/// Text fields are emitted first, in order, then file parts — the ordering the
/// GraphQL multipart request spec requires (`operations`, then `map`, then the
/// files). `name`/`filename` have `"`, CR and LF stripped so a crafted value
/// cannot break out of the part headers.
pub(super) fn build_multipart_body(
    text_fields: &[(String, String)],
    files: &[(String, String, String, Vec<u8>)],
) -> (Vec<u8>, String) {
    let sanitize = |s: &str| s.replace(['"', '\r', '\n'], "");
    let boundary = format!("----mossFormBoundary{}", uuid::Uuid::new_v4().simple());
    let mut body: Vec<u8> = Vec::new();

    for (name, value) in text_fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", sanitize(name))
                .as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }

    for (field, filename, content_type, bytes) in files {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                sanitize(field),
                sanitize(filename),
            )
            .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", sanitize(content_type)).as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (body, content_type)
}

/// Render an error-response body for the log: lossy UTF-8, whitespace
/// collapsed to single spaces, truncated to `max` characters so a giant
/// HTML error page cannot flood the log. Error bodies are the only way to
/// distinguish e.g. Matters' 500-for-expired-token from a real outage
/// (their TOKEN_INVALID code travels in the body), so on failure the body
/// is evidence, not noise.
pub(crate) fn body_snippet(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut s = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max {
        s = s.chars().take(max).collect();
        s.push('…');
    }
    s
}

/// Process ureq response or error into a standardized result tuple.
///
/// Shared helper for `fetch_url`, `http_post`, and `download_asset`.  Extracts
/// status, ok flag, content-type, and body bytes from successful responses, and
/// converts transport errors into descriptive strings.
pub fn process_ureq_response(
    response: Result<ureq::Response, ureq::Error>,
    url: &str,
    timeout_ms: u64,
) -> Result<(u16, bool, Option<String>, Vec<u8>), String> {
    use std::io::Read;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let ok = status >= 200 && status < 300;
            let content_type = resp.header("content-type").map(|s| s.to_string());

            let mut bytes = Vec::new();
            resp.into_reader()
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Failed to read response body: {}", e))?;

            Ok((status, ok, content_type, bytes))
        }
        Err(ureq::Error::Status(status, resp)) => {
            let ok = false;
            let content_type = resp.header("content-type").map(|s| s.to_string());
            let mut bytes = Vec::new();
            let _ = resp.into_reader().read_to_end(&mut bytes);
            Ok((status, ok, content_type, bytes))
        }
        Err(ureq::Error::Transport(e)) => {
            use std::error::Error as StdError;

            let msg = if e.kind() == ureq::ErrorKind::Io {
                if let Some(io_err) = e.source().and_then(|e| e.downcast_ref::<std::io::Error>()) {
                    if io_err.kind() == std::io::ErrorKind::TimedOut {
                        format!("Request timeout after {}ms: {}", timeout_ms, url)
                    } else {
                        format!("IO error: {}", e)
                    }
                } else {
                    format!("IO error: {}", e)
                }
            } else if e.kind() == ureq::ErrorKind::ConnectionFailed {
                format!("Connection failed: {}", e)
            } else {
                format!("Request failed: {}", e)
            };
            log::warn!("⚠️ HTTP request failed for {}: {}", url, msg);
            Err(msg)
        }
    }
}

// ============================================================================
// Asset Download Helpers
// ============================================================================

/// Check if a string is a valid UUID (8-4-4-4-12 hex format)
pub(super) fn is_uuid(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Extract UUID from URL path segments (for Matters asset URLs)
pub(super) fn extract_uuid_from_url(url: &url::Url) -> Option<String> {
    url.path_segments()?.find_map(|segment| {
        if is_uuid(segment) {
            return Some(segment.to_string());
        }
        if let Some((potential_uuid, _ext)) = segment.rsplit_once('.') {
            if is_uuid(potential_uuid) {
                return Some(potential_uuid.to_string());
            }
        }
        None
    })
}

/// Map content-type to file extension
pub(super) fn content_type_to_extension(content_type: &str) -> Option<&'static str> {
    let base_type = content_type.split(';').next().unwrap_or("").trim();
    match base_type {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        "image/avif" => Some("avif"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "audio/mpeg" => Some("mp3"),
        "audio/wav" => Some("wav"),
        "application/pdf" => Some("pdf"),
        _ => None,
    }
}

/// Extract filename from URL, add extension from content-type if needed.
/// Prioritises UUID from URL path for Matters asset URLs.
pub(super) fn derive_filename(url: &url::Url, content_type: Option<&str>) -> String {
    if let Some(uuid) = extract_uuid_from_url(url) {
        if let Some(ct) = content_type {
            if let Some(ext) = content_type_to_extension(ct) {
                return format!("{}.{}", uuid, ext);
            }
        }
        return uuid;
    }

    let base = url
        .path_segments()
        .and_then(|s| s.last())
        .filter(|s| !s.is_empty())
        .unwrap_or("download");

    if base.contains('.') {
        return base.to_string();
    }

    if let Some(ct) = content_type {
        if let Some(ext) = content_type_to_extension(ct) {
            return format!("{}.{}", base, ext);
        }
    }

    base.to_string()
}

/// Inner implementation of `download_asset` — testable without `tauri::State`.
pub async fn download_asset_impl(
    url: String,
    project_path: &str,
    target_dir: String,
    timeout_ms: Option<u64>,
) -> Result<DownloadAssetResult, String> {
    use std::time::Duration;

    if url.is_empty() {
        return Err("URL cannot be empty".to_string());
    }
    if target_dir.is_empty() {
        return Err("Target directory cannot be empty".to_string());
    }
    if target_dir.contains("..") {
        return Err("Invalid path: directory traversal not allowed".to_string());
    }

    let parsed_url: url::Url = url
        .parse()
        .map_err(|e| format!("Invalid URL '{}': {}", url, e))?;

    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err(format!(
            "Invalid URL scheme '{}': only http and https are allowed",
            parsed_url.scheme()
        ));
    }

    let timeout_ms_val = timeout_ms.unwrap_or(30_000);
    let timeout_duration = Duration::from_millis(timeout_ms_val);

    let _permit = DOWNLOAD_SEMAPHORE
        .acquire()
        .await
        .map_err(|_| "Download semaphore closed".to_string())?;

    log::debug!("download_asset: starting request to {} (permit acquired)", url);

    let url_clone = url.clone();
    let url_for_timeout_msg = url.clone();
    let result = tokio::time::timeout(timeout_duration, async {
        let url_inner = url_clone.clone();
        let timeout_for_ureq = timeout_duration;

        tokio::task::spawn_blocking(move || {
            let agent = crate::system::proxy::proxied_ureq_agent(&url_inner, timeout_for_ureq);
            let response = agent.get(&url_inner).call();
            process_ureq_response(response, &url_inner, timeout_ms_val)
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?
    })
    .await
    .map_err(|_| {
        format!(
            "Download timeout after {}ms: {}",
            timeout_duration.as_millis(),
            url_for_timeout_msg
        )
    })?;

    let (status, ok, content_type, bytes) = result?;

    if !ok {
        return Ok(DownloadAssetResult {
            status,
            ok,
            content_type,
            bytes_written: 0,
            actual_path: String::new(),
        });
    }

    let bytes_len = bytes.len() as u64;

    let filename = derive_filename(&parsed_url, content_type.as_deref());
    let relative_path = format!("{}/{}", target_dir.trim_end_matches('/'), filename);

    let file_path = Path::new(project_path).join(&relative_path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    // allow:raw_write downloaded asset lands in the plugin's target dir under the project root, not .moss/build/ — freshly created path, no evicted destination
    fs::write(&file_path, &bytes).map_err(|e| format!("Failed to write file: {}", e))?;

    log::debug!(
        "download_asset: saved {} ({} bytes) to {}",
        url,
        bytes_len,
        relative_path
    );

    Ok(DownloadAssetResult {
        status,
        ok,
        content_type,
        bytes_written: bytes_len,
        actual_path: relative_path,
    })
}

// ============================================================================
// Full HTTP Fetch (fetch shim backend — PUT/HEAD + full header capture)
// ============================================================================

/// HTTP method variants for the richer fetch path (report 5 §1d). Adds PUT + HEAD
/// on top of GET/POST. Body is a JSON string for POST/PUT.
pub(crate) enum FetchMethod {
    Get,
    Head,
    Post { body: String },
    Put { body: String },
}

/// Full HTTP result with ALL response headers (lowercased keys) for the fetch
/// shim's case-insensitive Headers.get (report 5 §1c: X-OAuth-Scopes at auth.ts:150).
pub(crate) struct FetchFull {
    pub status: u16,
    pub ok: bool,
    pub headers: std::collections::HashMap<String, String>, // keys lowercased
    pub body: Vec<u8>,
}

pub(crate) async fn do_http_request_full(
    url: String,
    method: FetchMethod,
    req_headers: Option<std::collections::HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<FetchFull, String> {
    use std::time::Duration;
    if url.is_empty() { return Err("URL cannot be empty".to_string()); }
    let parsed: url::Url = url.parse().map_err(|e| format!("Invalid URL '{url}': {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("Invalid URL scheme '{}': only http/https allowed", parsed.scheme()));
    }
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(30_000));
    let url_c = url.clone();
    tokio::task::spawn_blocking(move || {
        let agent = crate::system::proxy::proxied_ureq_agent(&url_c, timeout);
        let mut req = match &method {
            FetchMethod::Get => agent.get(&url_c),
            FetchMethod::Head => agent.head(&url_c),
            FetchMethod::Post { .. } => agent.post(&url_c).set("Content-Type", "application/json"),
            FetchMethod::Put { .. } => agent.put(&url_c).set("Content-Type", "application/json"),
        };
        if let Some(h) = req_headers {
            for (k, v) in h { req = req.set(&k, &v); }
        }
        // A HEAD response carries no body by definition (RFC 9110 §9.3.2), yet the
        // server still sends the entity's `Content-Length` header. Calling
        // `read_to_end` on a HEAD reader therefore blocks/errors waiting for bytes
        // that never arrive (Caddy/HTTP-2 keep-alive: UnexpectedEof or a full
        // timeout), which surfaced as a false-negative `isArticleLive` HEAD check
        // in the matters plugin — a live article (curl HEAD → 200) was skipped and
        // never syndicated. Never read the body for HEAD; leave it empty. Compute
        // this BEFORE the match below consumes `method` (Post/Put move `body` out).
        let is_head = matches!(method, FetchMethod::Head);
        let resp = match method {
            FetchMethod::Get | FetchMethod::Head => req.call(),
            FetchMethod::Post { body } | FetchMethod::Put { body } => req.send_string(&body),
        };
        match resp {
            Ok(r) => {
                let status = r.status();
                let ok = (200..300).contains(&status);
                let mut headers = std::collections::HashMap::new();
                for name in r.headers_names() {
                    if let Some(val) = r.header(&name) {
                        headers.insert(name.to_lowercase(), val.to_string());
                    }
                }
                let mut body = Vec::new();
                if !is_head {
                    use std::io::Read;
                    r.into_reader().read_to_end(&mut body).map_err(|e| format!("read body: {e}"))?;
                }
                Ok(FetchFull { status, ok, headers, body })
            }
            // ureq returns Err(Status) for non-2xx — surface it as a real response,
            // because fetch() never rejects on 4xx/5xx (it sets .ok=false).
            Err(ureq::Error::Status(status, r)) => {
                let ok = false;
                let mut headers = std::collections::HashMap::new();
                for name in r.headers_names() {
                    if let Some(val) = r.header(&name) { headers.insert(name.to_lowercase(), val.to_string()); }
                }
                use std::io::Read;
                let mut body = Vec::new();
                let _ = r.into_reader().read_to_end(&mut body);
                Ok(FetchFull { status, ok, headers, body })
            }
            Err(e) => Err(format!("request failed: {e}")),
        }
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

#[cfg(test)]
mod fetch_full_tests {
    use super::*;
    #[tokio::test]
    async fn rejects_empty_and_bad_scheme() {
        assert!(do_http_request_full("".into(), FetchMethod::Get, None, None).await.is_err());
        assert!(do_http_request_full("ftp://x".into(), FetchMethod::Get, None, None).await.is_err());
    }
    // A live HEAD against a stable endpoint proves HEAD + header capture (network).
    // Gate behind MOSS_NET_TESTS so CI without network still passes.
    #[tokio::test]
    async fn head_and_headers_when_net_available() {
        if std::env::var("MOSS_NET_TESTS").is_err() { return; }
        let r = do_http_request_full("https://api.github.com".into(), FetchMethod::Head, None, Some(5000)).await.unwrap();
        assert!(r.status > 0);
        assert!(r.headers.keys().all(|k| k == &k.to_lowercase()));
    }
}

// ============================================================================
// Binary Execution Helpers
// ============================================================================

/// Shared request validation for both execution paths (blocking here,
/// streaming app-side in `binary.rs`).
pub fn validate_binary_request(
    binary_path: &str,
    working_dir: &Option<String>,
) -> Result<(), String> {
    if binary_path.is_empty() {
        return Err("Binary path cannot be empty".to_string());
    }
    if let Some(dir) = working_dir {
        if dir.is_empty() {
            return Err("Working directory cannot be empty string".to_string());
        }
        if !Path::new(dir).exists() {
            return Err(format!("Working directory does not exist: {}", dir));
        }
    }
    Ok(())
}

/// Headless half of the #1019 split: validation + timeout + blocking
/// execution, no AppHandle anywhere. The QuickJS engine's `execute_binary`
/// arm calls this directly (it never streams); the webview command's
/// streaming branch lives app-side in `binary.rs`.
pub async fn execute_binary_blocking_impl(
    binary_path: String,
    args: Vec<String>,
    working_dir: Option<String>,
    env_vars: Option<HashMap<String, String>>,
    timeout_ms: Option<u64>,
    stdin_data: Option<String>,
) -> Result<BinaryExecutionResult, String> {
    validate_binary_request(&binary_path, &working_dir)?;
    let timeout_duration =
        std::time::Duration::from_millis(timeout_ms.unwrap_or(300_000));
    match tokio::time::timeout(
        timeout_duration,
        execute_binary_blocking(&binary_path, &args, &working_dir, env_vars, stdin_data),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err(format!(
            "Binary execution timed out after {} ms",
            timeout_duration.as_millis()
        )),
    }
}

/// Non-streaming execution path: uses `std::process::Command` in a blocking task.
pub(crate) async fn execute_binary_blocking(
    binary_path: &str,
    args: &[String],
    working_dir: &Option<String>,
    env_vars: Option<HashMap<String, String>>,
    stdin_data: Option<String>,
) -> Result<BinaryExecutionResult, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(binary_path);
    cmd.args(args);

    if let Some(dir) = working_dir {
        cmd.current_dir(Path::new(dir));
    }
    if let Some(vars) = env_vars {
        for (key, value) in vars {
            cmd.env(key, value);
        }
    }
    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let binary_path_owned = binary_path.to_string();

    tokio::task::spawn_blocking(move || {
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn binary '{}': {}", binary_path_owned, e))?;

        if let Some(data) = stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(data.as_bytes())
                    .map_err(|e| format!("Failed to write to stdin: {}", e))?;
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for binary '{}': {}", binary_path_owned, e))?;

        let exit_code = output.status.code().unwrap_or(-1);
        Ok(BinaryExecutionResult {
            exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}


#[cfg(test)]
mod tests {
    #[test]
    fn body_snippet_collapses_whitespace_and_truncates() {
        let body = "{\n  \"errors\": [\n    { \"message\": \"token invalid\" }\n  ]\n}";
        let s = super::body_snippet(body.as_bytes(), 256);
        assert!(!s.contains('\n'), "newlines must be collapsed: {s}");
        assert!(s.contains("token invalid"));

        let long = "x".repeat(1000);
        let t = super::body_snippet(long.as_bytes(), 256);
        assert!(t.chars().count() <= 257, "256 chars + ellipsis, got {}", t.chars().count());
        assert!(t.ends_with('…'));
    }

    #[test]
    fn body_snippet_survives_invalid_utf8_and_multibyte_content() {
        let s = super::body_snippet(&[0xff, 0xfe, b'o', b'k'], 256);
        assert!(s.contains("ok"));

        // 300 multibyte chars exceed the 256-CHAR cap (truncation is by
        // chars, not bytes) and must truncate without panicking.
        let cjk = "错".repeat(300);
        let t = super::body_snippet(cjk.as_bytes(), 256);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 257);
    }

    #[test]
    fn build_multipart_body_orders_fields_then_files_with_boundary() {
        let text = vec![
            ("operations".to_string(), "{\"query\":\"q\"}".to_string()),
            ("map".to_string(), "{\"0\":[\"variables.input.file\"]}".to_string()),
        ];
        let files = vec![(
            "0".to_string(),
            "photo.jpg".to_string(),
            "image/jpeg".to_string(),
            vec![0xFFu8, 0xD8, 0xFF, 0x00, 0x10],
        )];
        let (body, content_type) = super::build_multipart_body(&text, &files);

        assert!(content_type.starts_with("multipart/form-data; boundary=----mossFormBoundary"));
        let boundary = content_type.split("boundary=").nth(1).unwrap();

        let text_body = String::from_utf8_lossy(&body);
        // operations must appear before map before the file part.
        let op_at = text_body.find("name=\"operations\"").expect("operations present");
        let map_at = text_body.find("name=\"map\"").expect("map present");
        let file_at = text_body.find("filename=\"photo.jpg\"").expect("file present");
        assert!(op_at < map_at && map_at < file_at, "field ordering wrong");

        assert!(text_body.contains("Content-Type: image/jpeg"));
        assert!(text_body.contains(&format!("--{boundary}--")), "closing boundary present");
        // Raw (possibly non-UTF8) file bytes are embedded verbatim.
        assert!(body.windows(5).any(|w| w == [0xFF, 0xD8, 0xFF, 0x00, 0x10]));
    }

    #[test]
    fn build_multipart_body_sanitizes_quotes_and_newlines_in_filename() {
        let files = vec![(
            "0".to_string(),
            "ev\"il\r\nX: y.jpg".to_string(),
            "image/jpeg".to_string(),
            vec![1u8, 2, 3],
        )];
        let (body, _ct) = super::build_multipart_body(&[], &files);
        let s = String::from_utf8_lossy(&body);
        // The injected quote/CRLF must be stripped so the part header stays intact
        // (stripped outright — no replacement char), so "ev\"il\r\nX" → "evilX".
        assert!(s.contains("filename=\"evilX: y.jpg\""), "got: {s}");
        assert!(!s.contains("Content-Disposition: form-data; name=\"0\"; filename=\"evil\""));
    }

    /// LIVE verification against Matters staging (server.matters.icu): proves the
    /// REAL `build_multipart_body` + ureq `PostBytes` path produces a body the
    /// Matters GraphQL `singleFileUpload` accepts, for both image (`embed`) and
    /// audio (`embedaudio`). Ignored by default — run explicitly with a token +
    /// draft minted out-of-band:
    ///   MATTERS_TOKEN=… MATTERS_DRAFT_ID=… MATTERS_TEST_IMG=… MATTERS_TEST_AUDIO=… \
    ///     cargo test --lib multipart_upload_against_matters_staging -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "hits live Matters staging; requires MATTERS_TOKEN/MATTERS_DRAFT_ID/MATTERS_TEST_IMG/MATTERS_TEST_AUDIO"]
    async fn multipart_upload_against_matters_staging() {
        use base64::Engine;
        use std::collections::HashMap;

        let token = std::env::var("MATTERS_TOKEN").expect("MATTERS_TOKEN");
        let draft_id = std::env::var("MATTERS_DRAFT_ID").expect("MATTERS_DRAFT_ID");
        let mutation = "mutation U($input: SingleFileUploadInput!) { singleFileUpload(input: $input) { id path type } }";

        let upload = |asset_type: &'static str, path_env: &'static str, filename: &'static str, ct: &'static str| {
            let token = token.clone();
            let draft_id = draft_id.clone();
            async move {
                let bytes = std::fs::read(std::env::var(path_env).expect(path_env)).expect("read file");
                let operations = serde_json::json!({
                    "query": mutation,
                    "variables": { "input": { "type": asset_type, "entityType": "draft", "entityId": draft_id, "file": null } }
                })
                .to_string();
                let text_fields = vec![
                    ("operations".to_string(), operations),
                    ("map".to_string(), "{\"0\":[\"variables.input.file\"]}".to_string()),
                ];
                let files = vec![("0".to_string(), filename.to_string(), ct.to_string(), bytes)];
                let (body, content_type) = super::build_multipart_body(&text_fields, &files);

                let mut headers = HashMap::new();
                headers.insert("x-access-token".to_string(), token);
                headers.insert("apollo-require-preflight".to_string(), "true".to_string());

                let result = super::do_http_request(
                    "https://server.matters.icu/graphql".to_string(),
                    super::HttpMethod::PostBytes { body, content_type },
                    Some(headers),
                    Some(60_000),
                    "test_multipart",
                )
                .await
                .expect("request transport ok");

                let resp = base64::engine::general_purpose::STANDARD
                    .decode(result.body_base64)
                    .unwrap();
                String::from_utf8_lossy(&resp).to_string()
            }
        };

        let img_resp = upload("embed", "MATTERS_TEST_IMG", "photo.jpg", "image/jpeg").await;
        println!("IMAGE embed -> {img_resp}");
        assert!(img_resp.contains("\"path\""), "image upload had no path: {img_resp}");
        assert!(!img_resp.contains("\"errors\""), "image upload errored: {img_resp}");

        let aud_resp = upload("embedaudio", "MATTERS_TEST_AUDIO", "song.mp3", "audio/mpeg").await;
        println!("AUDIO embedaudio -> {aud_resp}");
        assert!(aud_resp.contains("\"path\""), "audio upload had no path: {aud_resp}");
        assert!(!aud_resp.contains("\"errors\""), "audio upload errored: {aud_resp}");
    }
}

#[cfg(test)]
mod execute_binary_tests {
    //! Moved beside their subject when `execute_binary_blocking` crossed
    //! into moss-build (#1019 slice-2 fix-forward): these exercise the
    //! process-spawning core, not the app-side streaming adapter.
    use super::execute_binary_blocking;
    use crate::plugins::runtime::portable::BinaryExecutionResult;
    use std::collections::HashMap;

    // Binary Execution Tests
    // ============================================================================

    #[test]
    fn test_binary_execution_result_serialization() {
        let result = BinaryExecutionResult {
            exit_code: 0,
            stdout: "Hello, World!".to_string(),
            stderr: "".to_string(),
            success: true,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"exit_code\":0"));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("Hello, World!"));
    }

    #[tokio::test]
    async fn test_execute_binary_empty_path() {
        let result = execute_binary_blocking("", &vec![], &None, None, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to spawn"));
    }

    #[tokio::test]
    async fn test_execute_binary_nonexistent_working_dir() {
        let result = execute_binary_blocking(
            "echo",
            &vec!["hello".to_string()],
            &Some("/nonexistent/path/that/does/not/exist".to_string()),
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_binary_empty_working_dir_string() {
        let result = execute_binary_blocking(
            "echo",
            &vec!["hello".to_string()],
            &Some("".to_string()),
            None,
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_binary_echo_success() {
        #[cfg(unix)]
        {
            let result =
                execute_binary_blocking("echo", &vec!["hello".to_string()], &None, None, None).await;
            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(output.success);
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("hello"));
        }
    }

    #[tokio::test]
    async fn test_execute_binary_with_env_vars() {
        #[cfg(unix)]
        {
            let mut env_vars = HashMap::new();
            env_vars.insert("TEST_VAR".to_string(), "test_value".to_string());

            let result = execute_binary_blocking(
                "sh",
                &vec!["-c".to_string(), "echo $TEST_VAR".to_string()],
                &None,
                Some(env_vars),
                None,
            )
            .await;

            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(output.success);
            assert!(output.stdout.contains("test_value"));
        }
    }

    #[tokio::test]
    async fn test_execute_binary_nonzero_exit() {
        #[cfg(unix)]
        {
            let result = execute_binary_blocking(
                "sh",
                &vec!["-c".to_string(), "exit 42".to_string()],
                &None,
                None,
                None,
            )
            .await;
            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(!output.success);
            assert_eq!(output.exit_code, 42);
        }
    }

    #[tokio::test]
    async fn test_execute_binary_nonexistent_binary() {
        let result = execute_binary_blocking(
            "/nonexistent/binary/that/does/not/exist",
            &vec![],
            &None,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to spawn"));
    }

    #[tokio::test]
    async fn test_execute_binary_with_stdin() {
        #[cfg(unix)]
        {
            let result = execute_binary_blocking(
                "cat",
                &vec![],
                &None,
                None,
                Some("hello from stdin".to_string()),
            )
            .await;
            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(output.success);
            assert_eq!(output.exit_code, 0);
            assert!(output.stdout.contains("hello from stdin"));
        }
    }

    #[tokio::test]
    async fn test_execute_binary_stdin_multiline() {
        #[cfg(unix)]
        {
            let result = execute_binary_blocking(
                "wc",
                &vec!["-l".to_string()],
                &None,
                None,
                Some("line1\nline2\nline3\n".to_string()),
            )
            .await;
            assert!(result.is_ok());
            let output = result.unwrap();
            assert!(output.success);
            let count: i32 = output.stdout.trim().parse().unwrap_or(-1);
            assert_eq!(count, 3);
        }
    }
}
